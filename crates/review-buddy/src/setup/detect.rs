//! Finds the hosts a person probably wants to connect, without touching the network.
//!
//! Everything is injected: programs run through a [`CommandRunner`], and the filesystem is read
//! from the paths in [`Roots`], so tests use a fake runner and a temporary directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rb_core::ForgeKind;
use rb_platform::auth::CliTool;
use rb_platform::CommandRunner;

/// How deep under the scan root a repository can sit, counting the root's children as 1.
pub const SCAN_DEPTH: usize = 3;
/// A shallow scan stops after this many directory entries, so a huge `~/src` stays quick.
const SCAN_BUDGET: usize = 4_000;

/// Where one host turned up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evidence {
    /// Signed in with a forge CLI.
    Cli { tool: CliTool, user: String },
    /// Named by an `insteadOf` rewrite in a git config file.
    GitConfig,
    /// A remote of a repository under the scan root.
    Remote { repo: String },
}

/// One host, with everything that pointed at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundHost {
    pub host: String,
    pub kind: ForgeKind,
    pub evidence: Vec<Evidence>,
}

impl FoundHost {
    /// The forge CLI sign-in for this host, if there is one.
    pub fn cli(&self) -> Option<(CliTool, &str)> {
        self.evidence.iter().find_map(|e| match e {
            Evidence::Cli { tool, user } => Some((*tool, user.as_str())),
            _ => None,
        })
    }

    /// `found in ~/.gitconfig`, `found in a repo under ~/src` or `signed in with gh`.
    pub fn found_in(&self) -> String {
        if let Some((tool, _)) = self.cli() {
            return format!("signed in with {}", tool_name(tool));
        }
        if self.evidence.contains(&Evidence::GitConfig) {
            return "found in your git config".to_string();
        }
        "found in a repository you use".to_string()
    }
}

pub fn tool_name(tool: CliTool) -> &'static str {
    match tool {
        CliTool::Gh => "gh",
        CliTool::Glab => "glab",
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detection {
    pub hosts: Vec<FoundHost>,
    /// Calm one-liners about things that were skipped.
    pub notes: Vec<String>,
}

/// The files and folders detection reads.
#[derive(Debug, Clone)]
pub struct Roots {
    /// Git config files to read for `url.*.insteadOf`.
    pub git_configs: Vec<PathBuf>,
    /// Where repositories live (`~/src`).
    pub scan_root: PathBuf,
}

impl Roots {
    /// `~/.gitconfig`, `$XDG_CONFIG_HOME/git/config` and `~/src`.
    pub fn from_env(env: &dyn rb_paths::Env) -> Self {
        let home = env.home_dir().unwrap_or_default();
        let xdg = env
            .var("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map_or_else(|| home.join(".config"), PathBuf::from);
        Self {
            git_configs: vec![home.join(".gitconfig"), xdg.join("git").join("config")],
            scan_root: home.join("src"),
        }
    }
}

pub fn detect(runner: &dyn CommandRunner, roots: &Roots) -> Detection {
    let mut found: BTreeMap<String, FoundHost> = BTreeMap::new();
    let mut notes = Vec::new();
    let mut add = |host: &str, kind: ForgeKind, evidence: Evidence| {
        let entry = found.entry(host.to_string()).or_insert_with(|| FoundHost {
            host: host.to_string(),
            kind,
            evidence: Vec::new(),
        });
        if !entry.evidence.contains(&evidence) {
            entry.evidence.push(evidence);
        }
    };

    for (tool, kind) in [
        (CliTool::Gh, ForgeKind::GitHub),
        (CliTool::Glab, ForgeKind::GitLab),
    ] {
        for (host, user) in cli_sign_ins(tool, runner) {
            add(&host, kind, Evidence::Cli { tool, user });
        }
    }

    let mut unknown = Vec::new();
    let mut guess =
        |host: &str, evidence: Evidence, add: &mut dyn FnMut(&str, ForgeKind, Evidence)| {
            match guess_kind(host) {
                Some(kind) => add(host, kind, evidence),
                None if !unknown.contains(&host.to_string()) => unknown.push(host.to_string()),
                None => {}
            }
        };
    for path in &roots.git_configs {
        if let Ok(text) = fs::read_to_string(path) {
            for host in insteadof_hosts(&text) {
                guess(&host, Evidence::GitConfig, &mut add);
            }
        }
    }
    for (repo, host) in remote_hosts(&roots.scan_root) {
        guess(&host, Evidence::Remote { repo }, &mut add);
    }
    // A host a CLI already vouched for needs no guess; unknown ones that never got a kind are noted.
    unknown.retain(|h| !found.contains_key(h));
    for host in unknown {
        notes.push(format!(
            "Skipped {host}: it isn't clear whether it's GitHub or GitLab. Add it by hand with a [[source]] in config.toml."
        ));
    }

    Detection {
        hosts: found.into_values().collect(),
        notes,
    }
}

/// Signed-in hosts and accounts from `gh auth status` or `glab auth status`. A missing program
/// or a signed-out one is simply no hosts.
pub fn cli_sign_ins(tool: CliTool, runner: &dyn CommandRunner) -> Vec<(String, String)> {
    let program = tool_name(tool);
    let Ok(out) = runner.run(program, &["auth", "status"], None) else {
        return Vec::new();
    };
    parse_auth_status(&format!("{}\n{}", out.stdout, out.stderr))
}

/// Reads `Logged in to <host> account <user>` and `Logged in to <host> as <user>` lines.
fn parse_auth_status(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.split_once("Logged in to ").map(|(_, r)| r) else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let (Some(host), Some(joiner), Some(user)) = (words.next(), words.next(), words.next())
        else {
            continue;
        };
        if joiner != "account" && joiner != "as" {
            continue;
        }
        let host = host.to_ascii_lowercase();
        if !out.iter().any(|(h, _)| *h == host) {
            out.push((host, user.trim_end_matches(['.', ',']).to_string()));
        }
    }
    out
}

pub fn guess_kind(host: &str) -> Option<ForgeKind> {
    let h = host.to_ascii_lowercase();
    if h == "github.com" || h.contains("github") {
        Some(ForgeKind::GitHub)
    } else if h == "gitlab.com" || h.contains("gitlab") {
        Some(ForgeKind::GitLab)
    } else {
        None
    }
}

/// The host in a remote address: `https://h/x`, `ssh://git@h:22/x` and `git@h:x` all give `h`.
pub fn host_of(url: &str) -> Option<String> {
    let url = url.trim().trim_matches('"');
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
    let end = rest.find(['/', ':']).unwrap_or(rest.len());
    let host = rest[..end].trim().to_ascii_lowercase();
    let valid = !host.is_empty()
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    valid.then_some(host)
}

/// Hosts named on either side of a `[url "…"] insteadOf = …` rewrite.
pub fn insteadof_hosts(config: &str) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    let mut section: Option<String> = None;
    let mut push = |url: &str| {
        if let Some(h) = host_of(url) {
            if !hosts.contains(&h) {
                hosts.push(h);
            }
        }
    };
    for line in config.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = header
                .trim()
                .strip_prefix("url")
                .map(str::trim)
                .map(|s| s.trim_matches('"').to_string());
            if let Some(url) = &section {
                push(url);
            }
            continue;
        }
        if section.is_none() {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim().eq_ignore_ascii_case("insteadof") {
                push(value);
            }
        }
    }
    hosts
}

/// `(repository folder, host)` for every remote of every repository found within [`SCAN_DEPTH`].
pub fn remote_hosts(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut budget = SCAN_BUDGET;
    scan(root, 1, &mut budget, &mut out);
    out
}

fn scan(dir: &Path, depth: usize, budget: &mut usize, out: &mut Vec<(String, String)>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let path = entry.path();
        if let Ok(config) = fs::read_to_string(path.join(".git").join("config")) {
            for host in remote_urls(&config).iter().filter_map(|u| host_of(u)) {
                if !out.iter().any(|(r, h)| *r == name && *h == host) {
                    out.push((name.clone(), host));
                }
            }
        } else if depth < SCAN_DEPTH {
            scan(&path, depth + 1, budget, out);
        }
    }
}

fn remote_urls(config: &str) -> Vec<String> {
    let mut in_remote = false;
    let mut urls = Vec::new();
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_remote = line.starts_with("[remote ");
        } else if in_remote {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "url" {
                    urls.push(value.trim().to_string());
                }
            }
        }
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_platform::{CommandOutput, PlatformError};
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake(HashMap<&'static str, CommandOutput>);

    impl Fake {
        fn with(mut self, program: &'static str, stdout: &str, stderr: &str) -> Self {
            self.0.insert(
                program,
                CommandOutput {
                    success: true,
                    stdout: stdout.into(),
                    stderr: stderr.into(),
                },
            );
            self
        }
    }

    impl CommandRunner for Fake {
        fn run(
            &self,
            program: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            self.0
                .get(program)
                .cloned()
                .ok_or_else(|| PlatformError::Spawn {
                    program: program.into(),
                    reason: "not found".into(),
                })
        }
    }

    const GH: &str = "github.com\n  ✓ Logged in to github.com account smorris (keyring)\n  - Active account: true\n  - Token scopes: 'repo'\nghe.corp.test\n  ✓ Logged in to ghe.corp.test as scott-w (oauth_token)\n";
    const GLAB: &str = "gitlab.com\n  ✓ Logged in to gitlab.com as scott (/home/u/.config/glab-cli/config.yml)\n  ✓ Token found: **********\n";

    fn roots(dir: &Path) -> Roots {
        Roots {
            git_configs: vec![dir.join(".gitconfig")],
            scan_root: dir.join("src"),
        }
    }

    #[test]
    fn gh_and_glab_sign_ins_become_hosts_with_users() {
        let runner = Fake::default().with("gh", GH, "").with("glab", "", GLAB);
        let t = tempfile::tempdir().unwrap();
        let d = detect(&runner, &roots(t.path()));
        let names: Vec<_> = d.hosts.iter().map(|h| h.host.as_str()).collect();
        assert_eq!(names, ["ghe.corp.test", "github.com", "gitlab.com"]);
        let gh = d.hosts.iter().find(|h| h.host == "github.com").unwrap();
        assert_eq!(gh.cli(), Some((CliTool::Gh, "smorris")));
        assert_eq!(gh.kind, ForgeKind::GitHub);
        let gl = d.hosts.iter().find(|h| h.host == "gitlab.com").unwrap();
        assert_eq!(gl.cli(), Some((CliTool::Glab, "scott")));
        assert_eq!(gl.kind, ForgeKind::GitLab);
        assert_eq!(gl.found_in(), "signed in with glab");
    }

    #[test]
    fn missing_or_signed_out_tools_find_nothing() {
        let t = tempfile::tempdir().unwrap();
        assert!(detect(&Fake::default(), &roots(t.path())).hosts.is_empty());
        let out = Fake::default().with("gh", "", "You are not logged into any GitHub hosts.\n");
        assert!(detect(&out, &roots(t.path())).hosts.is_empty());
    }

    #[test]
    fn insteadof_in_git_config_names_both_sides() {
        let text = "[user]\n\tname = X\n[url \"git@gitlab.work.ca:\"]\n\tinsteadOf = https://gitlab.work.ca/\n[url \"ssh://git@github.com/\"]\n\tinsteadof = gh:\n[core]\n\teditor = vi\n";
        assert_eq!(insteadof_hosts(text), ["gitlab.work.ca", "github.com"]);
    }

    #[test]
    fn config_hosts_have_no_credentials_and_unknown_ones_are_noted() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(
            t.path().join(".gitconfig"),
            "[url \"git@gitlab.work.ca:\"]\n\tinsteadOf = https://gitlab.work.ca/\n[url \"git@git.mystery.org:\"]\n\tinsteadOf = mys:\n",
        )
        .unwrap();
        let d = detect(&Fake::default(), &roots(t.path()));
        assert_eq!(d.hosts.len(), 1);
        assert_eq!(d.hosts[0].host, "gitlab.work.ca");
        assert_eq!(d.hosts[0].kind, ForgeKind::GitLab);
        assert!(d.hosts[0].cli().is_none());
        assert_eq!(d.hosts[0].found_in(), "found in your git config");
        assert_eq!(d.notes.len(), 1);
        assert!(d.notes[0].contains("git.mystery.org"));
    }

    fn repo(root: &Path, rel: &str, url: &str) {
        let git = root.join(rel).join(".git");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::write(
            git.join("config"),
            format!("[core]\n\tbare = false\n[remote \"origin\"]\n\turl = {url}\n"),
        )
        .unwrap();
    }

    #[test]
    fn remotes_under_the_scan_root_are_found_within_the_depth_limit() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        repo(&src, "a", "git@github.com:me/a.git");
        repo(&src, "work/b", "https://gitlab.work.ca/platform/b.git");
        repo(&src, "x/y/z/too-deep", "https://github.com/deep/deep.git");
        repo(&src, ".hidden", "https://github.com/hidden/h.git");
        repo(&src, "node_modules", "https://github.com/n/n.git");
        let found = remote_hosts(&src);
        assert_eq!(
            found,
            [
                ("a".to_string(), "github.com".to_string()),
                ("b".to_string(), "gitlab.work.ca".to_string())
            ]
        );
        let d = detect(&Fake::default(), &roots(t.path()));
        assert_eq!(d.hosts.len(), 2);
        assert!(matches!(d.hosts[0].evidence[0], Evidence::Remote { .. }));
    }

    #[test]
    fn a_missing_scan_root_is_fine() {
        let t = tempfile::tempdir().unwrap();
        assert!(remote_hosts(&t.path().join("nope")).is_empty());
    }

    #[test]
    fn evidence_merges_per_host() {
        let t = tempfile::tempdir().unwrap();
        repo(&t.path().join("src"), "a", "git@github.com:me/a.git");
        let runner = Fake::default().with("gh", GH, "");
        let d = detect(&runner, &roots(t.path()));
        let gh = d.hosts.iter().find(|h| h.host == "github.com").unwrap();
        assert_eq!(gh.evidence.len(), 2);
        assert_eq!(gh.found_in(), "signed in with gh");
    }

    #[test]
    fn host_extraction_handles_each_address_shape() {
        for (url, host) in [
            ("https://github.com/a/b.git", Some("github.com")),
            ("git@gitlab.work.ca:g/p.git", Some("gitlab.work.ca")),
            ("ssh://git@ghe.corp.test:2222/a/b", Some("ghe.corp.test")),
            ("https://user:pw@GitHub.com/a", Some("github.com")),
            ("gh:", None),
            ("../local/path", None),
        ] {
            assert_eq!(host_of(url).as_deref(), host, "{url}");
        }
    }
}
