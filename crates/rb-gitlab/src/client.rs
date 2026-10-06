use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rb_core::{Error, Result, User};
use rb_platform::Secret;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use url::Url;

use crate::error::{header_u64, map_status};

/// The most recent rate-limit numbers GitLab reported. `reset` is Unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub limit: u64,
    pub remaining: u64,
    pub reset: u64,
}

impl RateLimit {
    fn from_headers(headers: &HeaderMap) -> Option<Self> {
        Some(Self {
            limit: header_u64(headers, "ratelimit-limit").unwrap_or(0),
            remaining: header_u64(headers, "ratelimit-remaining")?,
            reset: header_u64(headers, "ratelimit-reset")?,
        })
    }
}

/// What `auth status` and `doctor` show about a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenReport {
    pub login: String,
    pub name: Option<String>,
    /// Empty when the token can't describe itself (some OAuth and older tokens).
    pub scopes: Vec<String>,
    /// `YYYY-MM-DD`, when the token expires.
    pub expires: Option<String>,
    pub rate: Option<RateLimit>,
}

#[derive(Deserialize)]
struct UserBody {
    username: String,
    name: Option<String>,
}

/// What `doctor` learns about the server itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    pub version: String,
    pub revision: Option<String>,
    /// The server's clock from its `Date` header, in Unix seconds.
    pub server_time: Option<i64>,
}

#[derive(Deserialize)]
struct VersionBody {
    version: String,
    revision: Option<String>,
}

#[derive(Deserialize)]
struct SelfBody {
    #[serde(default)]
    scopes: Vec<String>,
    expires_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scheme {
    PrivateToken,
    Bearer,
}

/// An authenticated GitLab HTTP client for one host (gitlab.com or self-hosted).
#[derive(Clone)]
pub struct GitlabClient {
    http: reqwest::Client,
    host: String,
    api_base: Url,
    token: Secret,
    scheme: Scheme,
    rate: Arc<Mutex<Option<RateLimit>>>,
}

impl fmt::Debug for GitlabClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitlabClient")
            .field("host", &self.host)
            .field("api_base", &self.api_base.as_str())
            .finish_non_exhaustive()
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl GitlabClient {
    /// `api_url` overrides the REST base (for self-hosted or tests). Without it the base is
    /// `https://<host>/api/v4`. Tokens go in the `PRIVATE-TOKEN` header unless
    /// [`with_bearer`](Self::with_bearer) is set.
    pub fn new(host: &str, api_url: Option<&str>, token: Secret) -> Result<Self> {
        let base = match api_url {
            Some(url) => url.trim().trim_end_matches('/').to_string(),
            None => format!("https://{host}/api/v4"),
        };
        let api_base = Url::parse(&base).map_err(|e| {
            Error::Api(format!(
                "the API address for {host} isn't a valid URL ({e}). Check `api_url` in your config"
            ))
        })?;
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
            api_base,
            token,
            scheme: Scheme::PrivateToken,
            rate: Arc::default(),
        })
    }

    /// Sends the token as `Authorization: Bearer`. Use it for tokens read from `glab`, which
    /// may be OAuth tokens; GitLab accepts personal access tokens that way too.
    pub fn with_bearer(mut self) -> Self {
        self.scheme = Scheme::Bearer;
        self
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn api_base(&self) -> &Url {
        &self.api_base
    }

    /// Where the forge's web pages live: the API base without `/api/v4`, keeping any scheme,
    /// port and relative URL root (`https://example.com/gitlab`), else `https://<host>`.
    pub fn web_base(&self) -> Url {
        rb_core::http::web_base_from_api(&self.api_base, "/api/v4").unwrap_or_else(|| {
            Url::parse(&format!("https://{}", self.host)).unwrap_or_else(|_| self.api_base.clone())
        })
    }

    /// `GET /version`, for the GitLab version and the server's clock.
    pub async fn probe(&self) -> Result<ServerInfo> {
        let (body, headers) = self.send_full(self.http.get(self.url("/version"))).await?;
        let v: VersionBody = self.parse(&body)?;
        Ok(ServerInfo {
            version: v.version,
            revision: v.revision,
            server_time: headers
                .get("date")
                .and_then(|d| d.to_str().ok())
                .and_then(rb_core::http::parse_http_date)
                .map(|t| t.0),
        })
    }

    /// The last rate-limit numbers seen on any response.
    pub fn rate_limit(&self) -> Option<RateLimit> {
        *self.rate.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub async fn whoami(&self) -> Result<User> {
        let user: UserBody = self.get_json("/user").await?;
        Ok(User {
            login: user.username,
            name: user.name,
        })
    }

    /// `GET /user`, then `GET /personal_access_tokens/self` for scopes and expiry. A token that
    /// can't describe itself still reports its user, with no scopes or expiry.
    pub async fn test_token(&self) -> Result<TokenReport> {
        let user: UserBody = self.get_json("/user").await?;
        let (scopes, expires) = match self
            .get_json::<SelfBody>("/personal_access_tokens/self")
            .await
        {
            Ok(body) => (body.scopes, body.expires_at.filter(|d| is_date(d))),
            Err(Error::NotFound(_) | Error::Unauthorized { .. } | Error::Forbidden { .. }) => {
                (Vec::new(), None)
            }
            Err(e) => return Err(e),
        };
        Ok(TokenReport {
            login: user.username,
            name: user.name,
            scopes,
            expires,
            rate: self.rate_limit(),
        })
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let body = self.send(self.http.get(self.url(path))).await?;
        self.parse(&body)
    }

    pub(crate) fn parse<T: DeserializeOwned>(&self, body: &[u8]) -> Result<T> {
        serde_json::from_slice(body).map_err(|e| {
            Error::Api(format!(
                "{} sent something unexpected ({e}). Try again, or run `review-buddy doctor`",
                self.host
            ))
        })
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.api_base.as_str().trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    /// A GET that also returns the response headers (pagination and totals live there).
    pub(crate) async fn get_page(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<(Vec<u8>, HeaderMap)> {
        self.send_full(self.http.get(self.url(path)).query(query))
            .await
    }

    /// A JSON write (`POST` or `PUT`) with the body serialised once; returns the response body.
    pub(crate) async fn write_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<Vec<u8>> {
        self.send(self.http.request(method, self.url(path)).json(body))
            .await
    }

    pub(crate) async fn send(&self, req: reqwest::RequestBuilder) -> Result<Vec<u8>> {
        self.send_full(req).await.map(|(body, _)| body)
    }

    async fn send_full(&self, req: reqwest::RequestBuilder) -> Result<(Vec<u8>, HeaderMap)> {
        let mut auth = match self.scheme {
            Scheme::PrivateToken => HeaderValue::from_str(self.token.expose()),
            Scheme::Bearer => HeaderValue::from_str(&format!("Bearer {}", self.token.expose())),
        }
        .map_err(|_| Error::Unauthorized {
            host: self.host.clone(),
        })?;
        auth.set_sensitive(true);
        let name = match self.scheme {
            Scheme::PrivateToken => "private-token",
            Scheme::Bearer => AUTHORIZATION.as_str(),
        };
        let response = req
            .header(name, auth)
            .header(ACCEPT, "application/json")
            .header(
                USER_AGENT,
                concat!("review-buddy/", env!("CARGO_PKG_VERSION")),
            )
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
            Ok((body, headers))
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

fn is_date(d: &str) -> bool {
    d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_override_urls() {
        let c = GitlabClient::new("gitlab.com", None, Secret::new("t")).unwrap();
        assert_eq!(c.api_base().as_str(), "https://gitlab.com/api/v4");
        let c = GitlabClient::new("x", Some("http://127.0.0.1:9/gl/api/v4/"), Secret::new("t"))
            .unwrap();
        assert_eq!(c.url("/user"), "http://127.0.0.1:9/gl/api/v4/user");
        assert!(GitlabClient::new("x", Some("not a url"), Secret::new("t")).is_err());
    }

    #[test]
    fn web_base_keeps_the_relative_root_and_port() {
        let web = |host: &str, api: Option<&str>| {
            GitlabClient::new(host, api, Secret::new("t"))
                .unwrap()
                .web_base()
                .to_string()
        };
        assert_eq!(web("gitlab.com", None), "https://gitlab.com/");
        assert_eq!(
            web("x", Some("https://corp.test:8443/gitlab/api/v4/")),
            "https://corp.test:8443/gitlab"
        );
        assert_eq!(
            web("gl.test", Some("http://127.0.0.1:9")),
            "https://gl.test/"
        );
    }

    #[test]
    fn debug_hides_token() {
        let c = GitlabClient::new("gitlab.com", None, Secret::new("glpat-topsecret")).unwrap();
        assert!(!format!("{c:?}").contains("topsecret"));
    }
}
