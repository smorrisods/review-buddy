use rb_core::Error;
use reqwest::header::HeaderMap;
use reqwest::StatusCode;

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

pub(crate) fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    header(headers, name)?.trim().parse().ok()
}

/// The URL in `X-GitHub-SSO: required; url=...`, if any.
pub(crate) fn sso_url(headers: &HeaderMap) -> Option<String> {
    let value = header(headers, "x-github-sso")?;
    value
        .split(';')
        .find_map(|part| part.trim().strip_prefix("url=").map(str::to_string))
}

pub(crate) fn sso_required(headers: &HeaderMap) -> bool {
    header(headers, "x-github-sso").is_some_and(|v| v.trim_start().starts_with("required"))
}

fn message(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    value.get("message")?.as_str().map(str::to_string)
}

fn is_rate_limited(status: StatusCode, headers: &HeaderMap, message: &str) -> bool {
    if status == StatusCode::TOO_MANY_REQUESTS {
        return true;
    }
    status == StatusCode::FORBIDDEN
        && (header_u64(headers, "x-ratelimit-remaining") == Some(0)
            || headers.contains_key("retry-after")
            || message.to_ascii_lowercase().contains("rate limit"))
}

fn retry_after(headers: &HeaderMap, now_epoch: u64) -> Option<u64> {
    header_u64(headers, "retry-after").or_else(|| {
        header_u64(headers, "x-ratelimit-reset").map(|reset| reset.saturating_sub(now_epoch))
    })
}

/// Maps a non-success response to a forge-neutral error with a next step in the copy.
pub(crate) fn map_status(
    host: &str,
    status: StatusCode,
    headers: &HeaderMap,
    body: &[u8],
    now_epoch: u64,
) -> Error {
    let detail = message(body).unwrap_or_default();
    match status {
        StatusCode::UNAUTHORIZED => Error::Unauthorized {
            host: host.to_string(),
        },
        s if s == StatusCode::FORBIDDEN || s == StatusCode::TOO_MANY_REQUESTS => {
            if sso_required(headers) {
                let reason = match sso_url(headers) {
                    Some(url) => format!(
                        "your organisation requires SSO. Authorise the token for it at {url}, then try again"
                    ),
                    None => "your organisation requires SSO. Authorise the token for it in your GitHub token settings, then try again".to_string(),
                };
                Error::Forbidden {
                    host: host.to_string(),
                    reason,
                }
            } else if is_rate_limited(s, headers, &detail) {
                Error::RateLimited {
                    host: host.to_string(),
                    retry_after_secs: retry_after(headers, now_epoch),
                }
            } else {
                let reason = if detail.is_empty() {
                    "the token may be missing a scope. Check `repo` and `read:org`".to_string()
                } else {
                    format!("{detail}. Check the token's scopes (`repo`, `read:org`)")
                };
                Error::Forbidden {
                    host: host.to_string(),
                    reason,
                }
            }
        }
        StatusCode::NOT_FOUND => Error::NotFound(if detail.is_empty() {
            format!("nothing at that address on {host}, or the token can't see it")
        } else {
            format!("{detail} (on {host}, or the token can't see it)")
        }),
        StatusCode::CONFLICT => Error::Conflict(if detail.is_empty() {
            "the change moved on. Refresh and try again".to_string()
        } else {
            format!("{detail}. Refresh and try again")
        }),
        other => Error::Api(format!(
            "{host} answered {}{}",
            other.as_u16(),
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
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

    #[test]
    fn sso_url_is_extracted() {
        let h = headers(&[(
            "x-github-sso",
            "required; url=https://github.com/orgs/o/sso?x=1",
        )]);
        assert_eq!(sso_url(&h).unwrap(), "https://github.com/orgs/o/sso?x=1");
        assert!(sso_required(&h));
        assert!(!sso_required(&headers(&[(
            "x-github-sso",
            "partial-results; organizations=1"
        )])));
    }

    #[test]
    fn statuses_map() {
        let none = HeaderMap::new();
        let m = |s, h: &HeaderMap, b: &str| map_status("github.com", s, h, b.as_bytes(), 1000);
        assert!(matches!(
            m(StatusCode::UNAUTHORIZED, &none, ""),
            Error::Unauthorized { .. }
        ));
        assert!(matches!(
            m(StatusCode::NOT_FOUND, &none, r#"{"message":"Not Found"}"#),
            Error::NotFound(s) if s.contains("Not Found")
        ));
        assert!(matches!(
            m(StatusCode::CONFLICT, &none, ""),
            Error::Conflict(_)
        ));
        assert!(matches!(
            m(StatusCode::FORBIDDEN, &none, ""),
            Error::Forbidden { .. }
        ));
        assert!(matches!(
            m(StatusCode::BAD_GATEWAY, &none, ""),
            Error::Api(_)
        ));
    }

    #[test]
    fn rate_limit_uses_reset_or_retry_after() {
        let h = headers(&[
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "1090"),
        ]);
        let e = map_status("h", StatusCode::FORBIDDEN, &h, b"", 1000);
        assert_eq!(
            e,
            Error::RateLimited {
                host: "h".into(),
                retry_after_secs: Some(90)
            }
        );
        let h = headers(&[("retry-after", "30")]);
        let e = map_status("h", StatusCode::TOO_MANY_REQUESTS, &h, b"", 1000);
        assert!(matches!(
            e,
            Error::RateLimited {
                retry_after_secs: Some(30),
                ..
            }
        ));
    }
}
