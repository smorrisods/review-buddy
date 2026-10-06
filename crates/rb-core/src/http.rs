//! Small, pure helpers the forge clients share for HTTP details.

use crate::Timestamp;

/// Parses an HTTP `Date` header (`Tue, 06 Oct 2026 10:00:00 GMT`).
pub fn parse_http_date(text: &str) -> Option<Timestamp> {
    let (_, rest) = text.trim().split_once(", ")?;
    let mut parts = rest.split_whitespace();
    let day: i64 = parts.next()?.parse().ok()?;
    let name = parts.next()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| m.eq_ignore_ascii_case(name))? as i64
        + 1;
    let year: i64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let mut field = || -> Option<i64> { clock.next()?.parse().ok() };
    let (h, mi, s) = (field()?, field()?, field()?);
    if !(1..=31).contains(&day) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(Timestamp(days * 86_400 + h * 3600 + mi * 60 + s))
}

/// The web address of a forge whose API lives at `api_base`: the base with its API suffix
/// (`/api/v3`, `/api/v4`) removed, keeping the scheme, port and any path prefix. `None` when
/// the base doesn't end in `suffix`.
pub fn web_base_from_api(api_base: &url::Url, suffix: &str) -> Option<url::Url> {
    let path = api_base.path().trim_end_matches('/');
    let prefix = path.strip_suffix(suffix)?;
    let mut web = api_base.clone();
    web.set_path(prefix);
    web.set_query(None);
    web.set_fragment(None);
    Some(web)
}

/// Joins a web base and a path without doubling or dropping slashes.
pub fn join_web(base: &url::Url, path: &str) -> Option<url::Url> {
    let root = base.as_str().trim_end_matches('/');
    url::Url::parse(&format!("{root}/{}", path.trim_start_matches('/'))).ok()
}

/// An error and everything beneath it, joined with `: `.
pub fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(inner) = source {
        text.push_str(": ");
        text.push_str(&inner.to_string());
        source = inner.source();
    }
    text
}

/// Calm copy for a failed TLS handshake, or `None` when `chain` doesn't look like one.
pub fn tls_reason(chain: &str) -> Option<String> {
    let lower = chain.to_ascii_lowercase();
    ["certificate", "tls", "ssl handshake", "handshake"]
        .iter()
        .any(|w| lower.contains(w))
        .then(|| "the TLS certificate couldn't be verified. If the server uses a private certificate authority, add its certificate to your system trust store, then try again".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    #[test]
    fn http_dates() {
        assert_eq!(
            parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"),
            Some(Timestamp(0))
        );
        assert_eq!(
            parse_http_date("Tue, 14 Nov 2023 22:13:20 GMT"),
            Some(Timestamp(1_700_000_000))
        );
        assert_eq!(parse_http_date("yesterday"), None);
        assert_eq!(parse_http_date("Tue, 14 Foo 2023 22:13:20 GMT"), None);
    }

    #[test]
    fn web_base_keeps_scheme_port_and_prefix() {
        let u = |s: &str| Url::parse(s).unwrap();
        let web = |s: &str, suffix| web_base_from_api(&u(s), suffix).map(|u| u.to_string());
        assert_eq!(
            web("https://x.test:8443/ghe/api/v3/", "/api/v3").as_deref(),
            Some("https://x.test:8443/ghe")
        );
        assert_eq!(
            web("http://x.test/gitlab/api/v4", "/api/v4").as_deref(),
            Some("http://x.test/gitlab")
        );
        assert_eq!(web("http://x.test/other", "/api/v4"), None);
    }

    #[test]
    fn join_web_handles_slashes() {
        let base = Url::parse("https://x.test/gitlab").unwrap();
        assert_eq!(
            join_web(&base, "/g/p/-/merge_requests/1").unwrap().as_str(),
            "https://x.test/gitlab/g/p/-/merge_requests/1"
        );
        let root = Url::parse("https://x.test/").unwrap();
        assert_eq!(
            join_web(&root, "a/b/pull/1").unwrap().as_str(),
            "https://x.test/a/b/pull/1"
        );
    }

    #[test]
    fn tls_failures_get_a_fix() {
        assert!(tls_reason("invalid peer certificate: UnknownIssuer").is_some());
        assert!(tls_reason("connection refused").is_none());
    }
}
