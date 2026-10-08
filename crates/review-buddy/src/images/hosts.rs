//! Which hosts belong to a forge source, and so where its token may go.
//!
//! The token is sent only over https (or to a loopback address in tests) and only to a source's
//! web and API hosts. Content hosts such as `*.githubusercontent.com` count as the forge's own
//! for `ui.images = "forge-only"`, but are fetched without the token: the addresses they hand
//! out are signed, and a bearer token on top of a signature is refused by some of them.

use std::net::IpAddr;

use rb_core::{ForgeKind, Source};
use url::{Host, Url};

/// Whether a host belongs to the source's forge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    Forge,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Rule {
    Exact(String),
    /// A name ending in `.suffix`, never the bare suffix itself.
    Suffix(String),
}

impl Rule {
    fn matches(&self, host: &str) -> bool {
        match self {
            Self::Exact(name) => host == name,
            Self::Suffix(suffix) => {
                host.len() > suffix.len() + 1 && host.ends_with(&format!(".{suffix}"))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeHosts {
    owned: Vec<Rule>,
    token: Vec<Rule>,
    /// Lets http and loopback hosts through. For tests that talk to a local mock server.
    insecure: bool,
}

/// The host part of `name`, lowercase, without a port.
fn bare(name: &str) -> String {
    let name = name.trim().to_ascii_lowercase();
    match name.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !host.contains(':') => {
            host.to_string()
        }
        _ => name,
    }
}

fn host_of(address: &str) -> Option<String> {
    Url::parse(address)
        .ok()?
        .host_str()
        .map(str::to_ascii_lowercase)
}

impl ForgeHosts {
    /// The hosts of one configured source. `api_url` is the source's `api_url` when it has one.
    pub fn for_source(source: &Source, api_url: Option<&str>) -> Self {
        let host = bare(&source.host);
        let web = host.strip_prefix("api.").unwrap_or(&host).to_string();
        let mut owned = Vec::new();
        let mut token = Vec::new();
        match source.kind {
            ForgeKind::GitHub if web == "github.com" => {
                for name in ["github.com", "api.github.com"] {
                    owned.push(Rule::Exact(name.into()));
                    token.push(Rule::Exact(name.into()));
                }
                owned.push(Rule::Suffix("githubusercontent.com".into()));
            }
            ForgeKind::GitHub => {
                for name in [web.clone(), format!("api.{web}"), host.clone()] {
                    owned.push(Rule::Exact(name.clone()));
                    token.push(Rule::Exact(name));
                }
                owned.push(Rule::Suffix(web));
            }
            ForgeKind::GitLab => {
                owned.push(Rule::Exact(host.clone()));
                token.push(Rule::Exact(host));
            }
        }
        if let Some(name) = api_url.and_then(host_of) {
            owned.push(Rule::Exact(name.clone()));
            token.push(Rule::Exact(name));
        }
        Self {
            owned,
            token,
            insecure: false,
        }
    }

    /// Exactly the named hosts, owned and token-bearing, over http as well as https.
    pub fn insecure_for_tests(names: &[&str]) -> Self {
        let rules: Vec<Rule> = names.iter().map(|n| Rule::Exact(bare(n))).collect();
        Self {
            owned: rules.clone(),
            token: rules,
            insecure: true,
        }
    }

    pub fn trust(&self, url: &Url) -> Trust {
        match url.host_str().map(str::to_ascii_lowercase) {
            Some(host) if self.owned.iter().any(|r| r.matches(&host)) => Trust::Forge,
            _ => Trust::External,
        }
    }

    /// Whether the source's token may go to `url`.
    pub fn sends_token(&self, url: &Url) -> bool {
        let secure = url.scheme() == "https" || (self.insecure && url.scheme() == "http");
        secure
            && url.username().is_empty()
            && url.password().is_none()
            && url
                .host_str()
                .map(str::to_ascii_lowercase)
                .is_some_and(|host| self.token.iter().any(|r| r.matches(&host)))
    }

    /// Hosts that mean this machine or its network: never fetched for a third party.
    pub fn is_local(&self, url: &Url) -> bool {
        if self.insecure {
            return false;
        }
        match url.host() {
            Some(Host::Domain(name)) => {
                let name = name.to_ascii_lowercase();
                name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local")
            }
            Some(Host::Ipv4(ip)) => local_ip(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => local_ip(IpAddr::V6(ip)),
            None => true,
        }
    }

    pub fn allows_http(&self) -> bool {
        self.insecure
    }
}

fn local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// The host of an address, for messages. Paths and queries can carry signatures, so they are
/// never shown or logged.
pub fn host_label(url: &Url) -> String {
    url.host_str().unwrap_or("that address").to_string()
}

#[cfg(test)]
mod tests {
    use rb_core::{AuthMode, Scope, SourceId};

    use super::*;

    fn source(kind: ForgeKind, host: &str) -> Source {
        Source {
            id: SourceId::new("s"),
            kind,
            host: host.into(),
            label: "s".into(),
            scope: Scope::everything(),
            auth: AuthMode::Cli,
            in_all: true,
            include_drafts: true,
            tag_colour: None,
        }
    }

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn github_dot_com_owns_its_content_hosts_but_only_two_get_the_token() {
        let hosts = ForgeHosts::for_source(&source(ForgeKind::GitHub, "github.com"), None);
        for owned in [
            "https://github.com/user-attachments/assets/1",
            "https://user-images.githubusercontent.com/1/a.png",
            "https://private-user-images.githubusercontent.com/1/a.png?jwt=x",
            "https://objects.githubusercontent.com/a",
            "https://api.github.com/x",
        ] {
            assert_eq!(hosts.trust(&url(owned)), Trust::Forge, "{owned}");
        }
        assert!(hosts.sends_token(&url("https://github.com/user-attachments/assets/1")));
        assert!(hosts.sends_token(&url("https://api.github.com/x")));
        assert!(!hosts.sends_token(&url(
            "https://private-user-images.githubusercontent.com/1/a.png"
        )));
        assert!(!hosts.sends_token(&url("https://objects.githubusercontent.com/a")));
    }

    #[test]
    fn lookalike_hosts_are_external_and_never_get_the_token() {
        let hosts = ForgeHosts::for_source(&source(ForgeKind::GitHub, "github.com"), None);
        for other in [
            "https://github.com.evil.test/a.png",
            "https://evilgithub.com/a.png",
            "https://githubusercontent.com/a.png",
            "https://evilgithubusercontent.com/a.png",
            "https://github.com@evil.test/a.png",
            "https://example.com/github.com/a.png",
        ] {
            let parsed = url(other);
            assert_eq!(hosts.trust(&parsed), Trust::External, "{other}");
            assert!(!hosts.sends_token(&parsed), "{other}");
        }
    }

    #[test]
    fn the_token_never_goes_over_plain_http_or_with_userinfo() {
        let hosts = ForgeHosts::for_source(&source(ForgeKind::GitLab, "gitlab.example.com"), None);
        assert!(hosts.sends_token(&url("https://gitlab.example.com/-/uploads/x.png")));
        assert!(!hosts.sends_token(&url("http://gitlab.example.com/-/uploads/x.png")));
        assert!(!hosts.sends_token(&url("https://user:pw@gitlab.example.com/x.png")));
    }

    #[test]
    fn gitlab_owns_only_its_own_host_and_api_host() {
        let hosts = ForgeHosts::for_source(
            &source(ForgeKind::GitLab, "gitlab.example.com:8443"),
            Some("https://api.example.com/api/v4"),
        );
        assert_eq!(
            hosts.trust(&url("https://gitlab.example.com/x.png")),
            Trust::Forge
        );
        assert_eq!(
            hosts.trust(&url("https://api.example.com/x.png")),
            Trust::Forge
        );
        assert_eq!(
            hosts.trust(&url("https://cdn.example.com/x.png")),
            Trust::External
        );
        assert_eq!(
            hosts.trust(&url("https://sub.gitlab.example.com/x.png")),
            Trust::External
        );
    }

    #[test]
    fn enterprise_hosts_own_their_subdomains() {
        let hosts = ForgeHosts::for_source(&source(ForgeKind::GitHub, "ghe.example.com"), None);
        assert_eq!(
            hosts.trust(&url("https://media.ghe.example.com/x.png")),
            Trust::Forge
        );
        assert!(hosts.sends_token(&url("https://ghe.example.com/x.png")));
        assert!(!hosts.sends_token(&url("https://media.ghe.example.com/x.png")));
        assert_eq!(
            hosts.trust(&url("https://example.com/x.png")),
            Trust::External
        );
    }

    #[test]
    fn local_addresses_are_recognised() {
        let hosts = ForgeHosts::for_source(&source(ForgeKind::GitHub, "github.com"), None);
        for local in [
            "http://localhost/a.png",
            "http://127.0.0.1/a.png",
            "http://10.0.0.5/a.png",
            "http://192.168.1.1/a.png",
            "http://169.254.169.254/latest",
            "http://[::1]/a.png",
            "http://printer.local/a.png",
        ] {
            assert!(hosts.is_local(&url(local)), "{local}");
        }
        assert!(!hosts.is_local(&url("https://example.com/a.png")));
        assert!(!hosts.is_local(&url("http://8.8.8.8/a.png")));
    }

    #[test]
    fn labels_show_the_host_and_nothing_secret() {
        assert_eq!(
            host_label(&url("https://x.test/a.png?jwt=secret")),
            "x.test"
        );
    }
}
