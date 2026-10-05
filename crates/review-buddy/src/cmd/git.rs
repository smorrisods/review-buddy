//! Repository inference from the current directory's git remotes.
//!
//! The parsing is pure and works on plain strings. [`inspect`] is the one place that runs `git`.

use std::path::Path;
use std::process::Command;

use super::selector::RepoRef;

/// A remote name and its fetch URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

/// A `url.<base>.insteadOf = <prefix>` rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsteadOf {
    pub base: String,
    pub prefix: String,
}

/// What the current directory's repository tells us.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitInfo {
    pub remote: Option<RepoRef>,
    pub branch: Option<String>,
}

/// Reads the remote and branch of the repository containing `dir`; empty when there is none.
pub fn inspect(dir: &Path) -> GitInfo {
    let run = |args: &[&str]| -> Option<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let remotes = run(&["remote", "-v"])
        .map(|t| parse_remote_list(&t))
        .unwrap_or_default();
    let rules = run(&["config", "--get-regexp", r"^url\..*\.insteadof$"])
        .map(|t| parse_instead_of(&t))
        .unwrap_or_default();
    let branch = run(&["branch", "--show-current"])
        .map(|t| t.trim().to_string())
        .filter(|b| !b.is_empty());
    GitInfo {
        remote: preferred_remote(&remotes)
            .map(|r| apply_instead_of(&r.url, &rules))
            .and_then(|url| parse_remote_url(&url)),
        branch,
    }
}

/// Parses `git remote -v`, keeping each remote's fetch URL.
pub fn parse_remote_list(text: &str) -> Vec<Remote> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (name, url, kind) = (parts.next()?, parts.next()?, parts.next()?);
            (kind == "(fetch)").then(|| Remote {
                name: name.to_string(),
                url: url.to_string(),
            })
        })
        .collect()
}

/// Parses `git config --get-regexp` output for `url.<base>.insteadof <prefix>` lines.
pub fn parse_instead_of(text: &str) -> Vec<InsteadOf> {
    text.lines()
        .filter_map(|line| {
            let (key, prefix) = line.split_once(char::is_whitespace)?;
            let base = key
                .strip_prefix("url.")?
                .strip_suffix(".insteadof")
                .or_else(|| key.strip_prefix("url.")?.strip_suffix(".insteadOf"))?;
            Some(InsteadOf {
                base: base.to_string(),
                prefix: prefix.trim().to_string(),
            })
        })
        .collect()
}

/// `upstream` first, then `origin`, then whichever came first.
pub fn preferred_remote(remotes: &[Remote]) -> Option<&Remote> {
    ["upstream", "origin"]
        .iter()
        .find_map(|name| remotes.iter().find(|r| r.name == *name))
        .or_else(|| remotes.first())
}

/// Applies the longest matching `insteadOf` prefix, as git does.
pub fn apply_instead_of(url: &str, rules: &[InsteadOf]) -> String {
    rules
        .iter()
        .filter(|r| !r.prefix.is_empty() && url.starts_with(&r.prefix))
        .max_by_key(|r| r.prefix.len())
        .map_or_else(
            || url.to_string(),
            |r| format!("{}{}", r.base, &url[r.prefix.len()..]),
        )
}

/// Reads a git remote URL (https, ssh or scp-like) into a host and repository path.
pub fn parse_remote_url(url: &str) -> Option<RepoRef> {
    let url = url.trim();
    let (host, path) = match url.split_once("://") {
        Some((_, rest)) => {
            let (authority, path) = rest.split_once('/')?;
            (host_of(authority), path)
        }
        None => {
            let (authority, path) = url.split_once(':')?;
            if authority.contains('/') {
                return None;
            }
            (host_of(authority), path)
        }
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if host.is_empty() || path.split('/').count() < 2 || path.split('/').any(str::is_empty) {
        return None;
    }
    Some(RepoRef {
        host: Some(host.to_ascii_lowercase()),
        path: path.to_string(),
    })
}

fn host_of(authority: &str) -> String {
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    // A port is part of the authority in URL form but never part of the source host.
    host.split(':').next().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(host: &str, path: &str) -> Option<RepoRef> {
        Some(RepoRef {
            host: Some(host.into()),
            path: path.into(),
        })
    }

    #[test]
    fn remote_url_forms() {
        assert_eq!(
            parse_remote_url("https://github.com/liminal-hq/spindle.git"),
            repo("github.com", "liminal-hq/spindle")
        );
        assert_eq!(
            parse_remote_url("git@github.com:liminal-hq/spindle.git"),
            repo("github.com", "liminal-hq/spindle")
        );
        assert_eq!(
            parse_remote_url("ssh://git@gitlab.work.ca:2222/platform/infra/terraform.git"),
            repo("gitlab.work.ca", "platform/infra/terraform")
        );
        assert_eq!(
            parse_remote_url("https://me:tok@GitLab.work.ca/platform/flow/"),
            repo("gitlab.work.ca", "platform/flow")
        );
    }

    #[test]
    fn unusable_remote_urls_are_none() {
        for url in [
            "",
            "/srv/git/repo.git",
            "https://github.com/solo",
            "https://github.com",
            "file:///a",
        ] {
            assert_eq!(parse_remote_url(url), None, "{url}");
        }
    }

    #[test]
    fn remote_list_keeps_fetch_urls() {
        let text = "origin\tgit@github.com:me/fork.git (fetch)\norigin\tgit@github.com:me/fork.git (push)\nupstream\thttps://github.com/org/proj.git (fetch)\nupstream\thttps://github.com/org/proj.git (push)\n";
        let remotes = parse_remote_list(text);
        assert_eq!(remotes.len(), 2);
        assert_eq!(preferred_remote(&remotes).unwrap().name, "upstream");
    }

    #[test]
    fn origin_beats_others_and_first_is_the_fallback() {
        let r = |n: &str| Remote {
            name: n.into(),
            url: format!("https://h.com/{n}/x"),
        };
        assert_eq!(
            preferred_remote(&[r("fork"), r("origin")]).unwrap().name,
            "origin"
        );
        assert_eq!(
            preferred_remote(&[r("fork"), r("zed")]).unwrap().name,
            "fork"
        );
        assert_eq!(preferred_remote(&[]), None);
    }

    #[test]
    fn instead_of_rewrites_with_the_longest_prefix() {
        let rules = parse_instead_of(
            "url.git@github.com:.insteadof gh:\nurl.git@github.com:work/.insteadof ghw:\n",
        );
        assert_eq!(rules.len(), 2);
        assert_eq!(
            apply_instead_of("gh:me/proj", &rules),
            "git@github.com:me/proj"
        );
        assert_eq!(
            apply_instead_of("ghw:proj", &rules),
            "git@github.com:work/proj"
        );
        assert_eq!(
            apply_instead_of("https://x.com/a/b", &rules),
            "https://x.com/a/b"
        );
        assert_eq!(
            parse_remote_url(&apply_instead_of("gh:me/proj", &rules)),
            repo("github.com", "me/proj")
        );
    }
}
