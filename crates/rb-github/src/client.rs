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
}

#[derive(Deserialize)]
struct UserBody {
    login: String,
    name: Option<String>,
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
            Some(url) => url.to_string(),
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
        let (rates, _) = self.get_json::<RateBody>("/rate_limit").await?;
        Ok(TokenReport {
            login: user.login,
            name: user.name,
            scopes,
            core: rates.resources.core,
            graphql: rates.resources.graphql,
            sso_hint,
        })
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<(T, Raw)> {
        let url = self.rest_url(path);
        let raw = self.send(self.http.get(url)).await?;
        let value = self.parse(&raw.body)?;
        Ok((value, raw))
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
            .header("x-github-api-version", "2022-11-28")
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
    fn override_url() {
        let c = GithubClient::new("x", Some("http://127.0.0.1:9/"), Secret::new("t")).unwrap();
        assert_eq!(c.graphql_url().as_str(), "http://127.0.0.1:9/graphql");
        assert!(GithubClient::new("x", Some("not a url"), Secret::new("t")).is_err());
    }

    #[test]
    fn debug_hides_token() {
        let c = GithubClient::new("github.com", None, Secret::new("ghp_topsecret")).unwrap();
        assert!(!format!("{c:?}").contains("topsecret"));
    }
}
