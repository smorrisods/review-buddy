use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rb_core::{Error, Result, User};
use rb_platform::Secret;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use url::Url;

use crate::error::{header_u64, map_status, sso_url};

const DEFAULT_API: &str = "https://api.github.com";

/// The REST API version every request pins.
pub const API_VERSION: &str = "2022-11-28";

/// The most recent rate-limit numbers GitHub reported. `reset` is Unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct RateLimit {
    pub limit: u64,
    pub remaining: u64,
    pub reset: u64,
}

impl RateLimit {
    fn from_headers(headers: &HeaderMap) -> Option<Self> {
        Some(Self {
            limit: header_u64(headers, "x-ratelimit-limit").unwrap_or(0),
            remaining: header_u64(headers, "x-ratelimit-remaining")?,
            reset: header_u64(headers, "x-ratelimit-reset")?,
        })
    }
}

/// What `doctor` learns about the server itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// From `X-GitHub-Enterprise-Version`; `None` on github.com.
    pub enterprise_version: Option<String>,
    /// The server's clock from its `Date` header, in Unix seconds.
    pub server_time: Option<i64>,
}

/// What `auth status` and `doctor` show about a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenReport {
    pub login: String,
    pub name: Option<String>,
    /// Empty for fine-grained tokens, which don't report scopes.
    pub scopes: Vec<String>,
    pub core: RateLimit,
    pub graphql: Option<RateLimit>,
    pub sso_hint: Option<String>,
    /// `YYYY-MM-DD`, when GitHub says the token expires.
    pub expires: Option<String>,
}

#[derive(Deserialize)]
struct UserBody {
    login: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct OrgBody {
    login: String,
}

#[derive(Deserialize)]
struct RateBody {
    resources: Resources,
}

#[derive(Deserialize)]
struct Resources {
    core: RateLimit,
    graphql: Option<RateLimit>,
}

pub(crate) struct Raw {
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

/// An authenticated GitHub HTTP client for one host (github.com or Enterprise).
#[derive(Clone)]
pub struct GithubClient {
    http: reqwest::Client,
    host: String,
    rest_base: Url,
    graphql_url: Url,
    token: Secret,
    rate: Arc<Mutex<Option<RateLimit>>>,
}

impl fmt::Debug for GithubClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GithubClient")
            .field("host", &self.host)
            .field("rest_base", &self.rest_base.as_str())
            .finish_non_exhaustive()
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl GithubClient {
    /// `api_url` overrides the REST base (for Enterprise or tests). Without it, github.com
    /// uses `https://api.github.com` and any other host uses `https://<host>/api/v3`.
    pub fn new(host: &str, api_url: Option<&str>, token: Secret) -> Result<Self> {
        let base = match api_url {
            Some(url) => url.trim().to_string(),
            None if host == "github.com" => DEFAULT_API.to_string(),
            None => format!("https://{host}/api/v3"),
        };
        let rest_base = Url::parse(base.trim_end_matches('/')).map_err(|e| {
            Error::Api(format!(
                "the API address for {host} isn't a valid URL ({e}). Check `api_url` in your config"
            ))
        })?;
        let graphql_url = graphql_url_for(&rest_base);
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Network {
                host: host.to_string(),
                reason: e.without_url().to_string(),
            })?;
        Ok(Self {
            http,
            host: host.to_string(),
            rest_base,
            graphql_url,
            token,
            rate: Arc::default(),
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn rest_base(&self) -> &Url {
        &self.rest_base
    }

    pub fn graphql_url(&self) -> &Url {
        &self.graphql_url
    }

    /// Where the forge's web pages live: `https://github.com` for github.com, the REST base
    /// without `/api/v3` for Enterprise (keeping any scheme, port and path prefix), and
    /// `https://<host>` when the override doesn't follow the Enterprise layout.
    pub fn web_base(&self) -> Url {
        if self.rest_base.host_str() == Some("api.github.com") {
            return Url::parse("https://github.com").expect("static URL");
        }
        rb_core::http::web_base_from_api(&self.rest_base, "/api/v3").unwrap_or_else(|| {
            Url::parse(&format!("https://{}", self.host)).unwrap_or_else(|_| self.rest_base.clone())
        })
    }

    /// `GET /meta`, for the Enterprise Server version and the server's clock. Needs no scopes.
    pub async fn probe(&self) -> Result<ServerInfo> {
        let raw = self.send(self.http.get(self.rest_url("/meta"))).await?;
        let header = |name: &str| {
            raw.headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.trim().to_string())
        };
        Ok(ServerInfo {
            enterprise_version: header("x-github-enterprise-version"),
            server_time: header("date")
                .and_then(|d| rb_core::http::parse_http_date(&d))
                .map(|t| t.0),
        })
    }

    /// The last rate-limit numbers seen on any response.
    pub fn rate_limit(&self) -> Option<RateLimit> {
        *self.rate.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub async fn whoami(&self) -> Result<User> {
        let (user, _) = self.get_json::<UserBody>("/user").await?;
        Ok(User {
            login: user.login,
            name: user.name,
        })
    }

    pub async fn test_token(&self) -> Result<TokenReport> {
        let (user, raw) = self.get_json::<UserBody>("/user").await?;
        let scopes = raw
            .headers
            .get("x-oauth-scopes")
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let sso_hint = raw
            .headers
            .get("x-github-sso")
            .and_then(|v| v.to_str().ok())
            .map(|v| match sso_url(&raw.headers) {
                Some(url) => format!("Authorise the token for your organisation at {url}"),
                None => format!("Some organisations need SSO authorisation ({v})"),
            });
        let expires = raw
            .headers
            .get("github-authentication-token-expiration")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split_whitespace().next())
            .filter(|d| d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-')
            .map(str::to_string);
        let (rates, _) = self.get_json::<RateBody>("/rate_limit").await?;
        Ok(TokenReport {
            login: user.login,
            name: user.name,
            scopes,
            core: rates.resources.core,
            graphql: rates.resources.graphql,
            sso_hint,
            expires,
        })
    }

    /// The organisations the signed-in account belongs to (`/user/orgs`), by login. At most
    /// five pages of 100; more than that is far beyond what a source picker can show.
    pub async fn list_orgs(&self) -> Result<Vec<String>> {
        let (pages, _) = self.get_pages("/user/orgs?per_page=100", 5).await?;
        let mut logins = Vec::new();
        for raw in pages {
            let orgs: Vec<OrgBody> = self.parse(&raw.body)?;
            logins.extend(orgs.into_iter().map(|o| o.login));
        }
        Ok(logins)
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<(T, Raw)> {
        let url = self.rest_url(path);
        let raw = self.send(self.http.get(url)).await?;
        let value = self.parse(&raw.body)?;
        Ok((value, raw))
    }

    /// GETs `path` and follows `Link: rel="next"` for at most `max_pages` pages. The second
    /// value is true when more pages existed beyond the cap.
    pub(crate) async fn get_pages(&self, path: &str, max_pages: usize) -> Result<(Vec<Raw>, bool)> {
        let mut url = self.rest_url(path);
        let mut pages = Vec::new();
        loop {
            let raw = self.send(self.http.get(&url)).await?;
            let next = next_link(&raw.headers);
            pages.push(raw);
            let Some(next) = next else {
                return Ok((pages, false));
            };
            if pages.len() >= max_pages {
                return Ok((pages, true));
            }
            let same_origin = Url::parse(&next).is_ok_and(|n| {
                n.scheme() == self.rest_base.scheme()
                    && n.host_str() == self.rest_base.host_str()
                    && n.port_or_known_default() == self.rest_base.port_or_known_default()
            });
            if !same_origin {
                return Err(Error::Api(format!(
                    "{} sent a next-page link to a different host, so it was not followed. Try again, or run `review-buddy doctor`",
                    self.host
                )));
            }
            url = next;
        }
    }

    pub(crate) fn parse<T: DeserializeOwned>(&self, body: &[u8]) -> Result<T> {
        serde_json::from_slice(body).map_err(|e| {
            Error::Api(format!(
                "{} sent something unexpected ({e}). Try again, or run `review-buddy doctor`",
                self.host
            ))
        })
    }

    pub(crate) fn graphql_request(&self) -> reqwest::RequestBuilder {
        self.http.post(self.graphql_url.clone())
    }

    fn rest_url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.rest_base.as_str().trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    pub(crate) async fn send(&self, req: reqwest::RequestBuilder) -> Result<Raw> {
        let mut auth =
            HeaderValue::from_str(&format!("Bearer {}", self.token.expose())).map_err(|_| {
                Error::Unauthorized {
                    host: self.host.clone(),
                }
            })?;
        auth.set_sensitive(true);
        let response = req
            .header(AUTHORIZATION, auth)
            .header(ACCEPT, "application/vnd.github+json")
            .header(
                USER_AGENT,
                concat!("review-buddy/", env!("CARGO_PKG_VERSION")),
            )
            .header("x-github-api-version", API_VERSION)
            .send()
            .await
            .map_err(|e| self.network_error(e))?;
        let status = response.status();
        let headers = response.headers().clone();
        if let Some(rate) = RateLimit::from_headers(&headers) {
            *self.rate.lock().unwrap_or_else(|e| e.into_inner()) = Some(rate);
        }
        let body = response
            .bytes()
            .await
            .map_err(|e| self.network_error(e))?
            .to_vec();
        if status.is_success() {
            Ok(Raw { headers, body })
        } else {
            Err(map_status(&self.host, status, &headers, &body, now_epoch()))
        }
    }

    fn network_error(&self, e: reqwest::Error) -> Error {
        let reason = if e.is_timeout() {
            "the request timed out. Check your connection and try again".to_string()
        } else if let Some(tls) = rb_core::http::tls_reason(&rb_core::http::error_chain(&e)) {
            tls
        } else if e.is_connect() {
            "the connection failed. Check your network, VPN or `api_url`".to_string()
        } else {
            e.without_url().to_string()
        };
        Error::Network {
            host: self.host.clone(),
            reason,
        }
    }
}

fn next_link(headers: &HeaderMap) -> Option<String> {
    let link = headers.get("link")?.to_str().ok()?;
    link.split(',').find_map(|part| {
        let (target, params) = part.split_once(';')?;
        let is_next = params
            .split(';')
            .any(|p| p.trim().replace(' ', "") == "rel=\"next\"");
        let target = target.trim().strip_prefix('<')?.strip_suffix('>')?;
        is_next.then(|| target.to_string())
    })
}

/// Enterprise serves REST at `/api/v3` and GraphQL at `/api/graphql`; github.com and
/// plain overrides use `<base>/graphql`.
fn graphql_url_for(rest_base: &Url) -> Url {
    let mut url = rest_base.clone();
    let path = rest_base.path().trim_end_matches('/');
    let new_path = match path.strip_suffix("/api/v3") {
        Some(prefix) => format!("{prefix}/api/graphql"),
        None => format!("{path}/graphql"),
    };
    url.set_path(&new_path);
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_urls() {
        let c = GithubClient::new("github.com", None, Secret::new("t")).unwrap();
        assert_eq!(c.rest_base().as_str(), "https://api.github.com/");
        assert_eq!(c.graphql_url().as_str(), "https://api.github.com/graphql");
        let c = GithubClient::new("ghe.example.com", None, Secret::new("t")).unwrap();
        assert_eq!(c.rest_base().as_str(), "https://ghe.example.com/api/v3");
        assert_eq!(
            c.graphql_url().as_str(),
            "https://ghe.example.com/api/graphql"
        );
    }

    #[test]
    fn next_link_is_found() {
        let mut h = HeaderMap::new();
        h.insert(
            "link",
            HeaderValue::from_static(
                "<https://a/x?page=1>; rel=\"prev\", <https://a/x?page=3>; rel=\"next\", <https://a/x?page=9>; rel=\"last\"",
            ),
        );
        assert_eq!(next_link(&h).as_deref(), Some("https://a/x?page=3"));
        assert_eq!(next_link(&HeaderMap::new()), None);
    }

    #[test]
    fn override_url() {
        let c = GithubClient::new("x", Some("http://127.0.0.1:9/"), Secret::new("t")).unwrap();
        assert_eq!(c.graphql_url().as_str(), "http://127.0.0.1:9/graphql");
        assert!(GithubClient::new("x", Some("not a url"), Secret::new("t")).is_err());
    }

    #[test]
    fn web_base_follows_the_api_base() {
        let web = |host: &str, api: Option<&str>| {
            GithubClient::new(host, api, Secret::new("t"))
                .unwrap()
                .web_base()
                .to_string()
        };
        assert_eq!(web("github.com", None), "https://github.com/");
        assert_eq!(web("ghe.example.com", None), "https://ghe.example.com/");
        assert_eq!(
            web("x", Some("http://127.0.0.1:8080/ghe/api/v3/")),
            "http://127.0.0.1:8080/ghe"
        );
        assert_eq!(web("ghe.test:8443", None), "https://ghe.test:8443/");
        assert_eq!(
            web("ghe.test", Some("http://127.0.0.1:9")),
            "https://ghe.test/"
        );
    }

    #[test]
    fn enterprise_urls_with_prefix_port_and_trailing_slash() {
        let c = GithubClient::new(
            "x",
            Some(" https://corp.test:8443/ghe/api/v3/ "),
            Secret::new("t"),
        )
        .unwrap();
        assert_eq!(
            c.rest_url("/user"),
            "https://corp.test:8443/ghe/api/v3/user"
        );
        assert_eq!(
            c.graphql_url().as_str(),
            "https://corp.test:8443/ghe/api/graphql"
        );
    }

    #[test]
    fn debug_hides_token() {
        let c = GithubClient::new("github.com", None, Secret::new("ghp_topsecret")).unwrap();
        assert!(!format!("{c:?}").contains("topsecret"));
    }
}
