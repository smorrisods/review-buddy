use rb_core::Error;
use reqwest::header::HeaderMap;
use reqwest::StatusCode;

const DEFAULT_SCOPES: &str = "`api` and `read_user`";

pub(crate) fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

struct Detail {
    message: String,
    scopes: Option<String>,
    insufficient_scope: bool,
}

/// GitLab errors come as `{"message": "..."}`, `{"error": "..."}`, or an OAuth-style
/// `{"error": "insufficient_scope", "scope": "api read_api"}`.
fn detail(body: &[u8]) -> Detail {
    let value: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let error = text("error");
    let message = match value.get("message") {
        Some(serde_json::Value::String(s)) => s.clone(),
        _ => text("error_description")
            .or_else(|| error.clone())
            .unwrap_or_default(),
    };
    Detail {
        message,
        scopes: text("scope").map(|s| s.split_whitespace().collect::<Vec<_>>().join(", ")),
        insufficient_scope: error.as_deref() == Some("insufficient_scope"),
    }
}

/// Maps a non-success response to a forge-neutral error with a next step in the copy.
pub(crate) fn map_status(
    host: &str,
    status: StatusCode,
    headers: &HeaderMap,
    body: &[u8],
    now_epoch: u64,
) -> Error {
    let d = detail(body);
    match status {
        StatusCode::UNAUTHORIZED => Error::Unauthorized {
            host: host.to_string(),
        },
        StatusCode::FORBIDDEN if d.insufficient_scope => {
            let scopes = d
                .scopes
                .filter(|s| !s.is_empty())
                .map_or_else(|| DEFAULT_SCOPES.to_string(), |s| format!("`{s}`"));
            Error::Forbidden {
                host: host.to_string(),
                reason: format!(
                    "the token is missing a scope. Create one with {scopes} and sign in again"
                ),
            }
        }
        StatusCode::FORBIDDEN => Error::Forbidden {
            host: host.to_string(),
            reason: if d.message.is_empty() {
                format!("the token may be missing a scope. Check {DEFAULT_SCOPES}")
            } else {
                format!("{}. Check the token's scopes ({DEFAULT_SCOPES})", d.message)
            },
        },
        StatusCode::TOO_MANY_REQUESTS => Error::RateLimited {
            host: host.to_string(),
            retry_after_secs: header_u64(headers, "retry-after").or_else(|| {
                header_u64(headers, "ratelimit-reset").map(|r| r.saturating_sub(now_epoch))
            }),
        },
        StatusCode::NOT_FOUND => Error::NotFound(if d.message.is_empty() {
            format!("nothing at that address on {host}, or the token can't see it")
        } else {
            format!("{} (on {host}, or the token can't see it)", d.message)
        }),
        StatusCode::CONFLICT => Error::Conflict(if d.message.is_empty() {
            "the change moved on. Refresh and try again".to_string()
        } else {
            format!("{}. Refresh and try again", d.message)
        }),
        other => Error::Api(format!(
            "{host} answered {}{}",
            other.as_u16(),
            if d.message.is_empty() {
                String::new()
            } else {
                format!(": {}", d.message)
            }
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    HeaderName::from_bytes(k.as_bytes()).unwrap(),
                    HeaderValue::from_str(v).unwrap(),
                )
            })
            .collect()
    }

    fn m(s: StatusCode, h: &HeaderMap, b: &str) -> Error {
        map_status("gitlab.com", s, h, b.as_bytes(), 1000)
    }

    #[test]
    fn statuses_map() {
        let none = HeaderMap::new();
        assert!(matches!(
            m(StatusCode::UNAUTHORIZED, &none, ""),
            Error::Unauthorized { .. }
        ));
        assert!(matches!(
            m(StatusCode::NOT_FOUND, &none, r#"{"message":"404 Not found"}"#),
            Error::NotFound(s) if s.contains("404 Not found")
        ));
        assert!(matches!(
            m(StatusCode::CONFLICT, &none, ""),
            Error::Conflict(_)
        ));
        assert!(matches!(
            m(StatusCode::BAD_GATEWAY, &none, ""),
            Error::Api(_)
        ));
    }

    #[test]
    fn forbidden_names_scopes() {
        let none = HeaderMap::new();
        let Error::Forbidden { reason, .. } = m(
            StatusCode::FORBIDDEN,
            &none,
            r#"{"error":"insufficient_scope","scope":"api read_api"}"#,
        ) else {
            panic!("expected Forbidden")
        };
        assert!(reason.contains("`api, read_api`"), "{reason}");
        let Error::Forbidden { reason, .. } = m(StatusCode::FORBIDDEN, &none, "") else {
            panic!("expected Forbidden")
        };
        assert!(reason.contains("`api` and `read_user`"));
    }

    #[test]
    fn rate_limit_uses_retry_after_or_reset() {
        let h = headers(&[("retry-after", "30")]);
        assert_eq!(
            m(StatusCode::TOO_MANY_REQUESTS, &h, ""),
            Error::RateLimited {
                host: "gitlab.com".into(),
                retry_after_secs: Some(30)
            }
        );
        let h = headers(&[("ratelimit-reset", "1090")]);
        assert!(matches!(
            m(StatusCode::TOO_MANY_REQUESTS, &h, ""),
            Error::RateLimited {
                retry_after_secs: Some(90),
                ..
            }
        ));
    }
}
