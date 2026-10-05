//! Check runs plus legacy commit statuses merged into one list.
//! See "CI" in docs/integrations.md.
//!
//! Neutral, skipped and cancelled runs keep their own `CiState`. Branch-protection "required"
//! flags need an extra admin-scoped call, so `required` stays unknown.

use rb_core::{ChangeId, Check, CiState, Result};
use serde::Deserialize;
use url::Url;

use crate::files::repo_parts;
use crate::time::parse_rfc3339;
use crate::GithubClient;

const MAX_PAGES: usize = 10;

#[derive(Deserialize)]
struct PullHead {
    head: Sha,
}

#[derive(Deserialize)]
struct Sha {
    sha: String,
}

#[derive(Deserialize)]
struct RunsPage {
    #[serde(default)]
    check_runs: Vec<Run>,
}

#[derive(Deserialize)]
struct Run {
    name: String,
    status: String,
    conclusion: Option<String>,
    details_url: Option<String>,
    html_url: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
}

#[derive(Deserialize)]
struct StatusPage {
    #[serde(default)]
    statuses: Vec<Status>,
}

#[derive(Deserialize)]
struct Status {
    context: String,
    state: String,
    target_url: Option<String>,
}

fn run_state(status: &str, conclusion: Option<&str>) -> CiState {
    if status != "completed" {
        return CiState::Running;
    }
    match conclusion {
        Some("success") => CiState::Pass,
        Some("neutral") => CiState::Neutral,
        Some("skipped") => CiState::Skipped,
        Some("cancelled") => CiState::Cancelled,
        None => CiState::None,
        Some(_) => CiState::Fail,
    }
}

fn status_state(state: &str) -> CiState {
    match state {
        "success" => CiState::Pass,
        "pending" => CiState::Running,
        "failure" | "error" => CiState::Fail,
        _ => CiState::None,
    }
}

fn url(raw: Option<String>) -> Option<Url> {
    raw.and_then(|u| Url::parse(&u).ok())
}

fn merge(runs: Vec<Run>, statuses: Vec<Status>) -> Vec<Check> {
    let mut out: Vec<Check> = runs
        .into_iter()
        .map(|r| Check {
            state: run_state(&r.status, r.conclusion.as_deref()),
            url: url(r.details_url.or(r.html_url)),
            started_at: r.started_at.as_deref().and_then(parse_rfc3339),
            completed_at: r.completed_at.as_deref().and_then(parse_rfc3339),
            required: None,
            name: r.name,
        })
        .collect();
    for s in statuses {
        if out.iter().any(|c| c.name == s.context) {
            continue;
        }
        out.push(Check {
            state: status_state(&s.state),
            url: url(s.target_url),
            started_at: None,
            completed_at: None,
            required: None,
            name: s.context,
        });
    }
    out
}

pub(crate) async fn checks(client: &GithubClient, id: &ChangeId) -> Result<Vec<Check>> {
    let (owner, name) = repo_parts(id)?;
    let (head, _) = client
        .get_json::<PullHead>(&format!("/repos/{owner}/{name}/pulls/{}", id.number))
        .await?;
    let sha = head.head.sha;

    let runs_path = format!("/repos/{owner}/{name}/commits/{sha}/check-runs?per_page=100");
    let (pages, _) = client.get_pages(&runs_path, MAX_PAGES).await?;
    let mut runs = Vec::new();
    for raw in pages {
        runs.extend(client.parse::<RunsPage>(&raw.body)?.check_runs);
    }

    let status_path = format!("/repos/{owner}/{name}/commits/{sha}/status?per_page=100");
    let (pages, _) = client.get_pages(&status_path, MAX_PAGES).await?;
    let mut statuses = Vec::new();
    for raw in pages {
        statuses.extend(client.parse::<StatusPage>(&raw.body)?.statuses);
    }
    Ok(merge(runs, statuses))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_states() {
        assert_eq!(run_state("queued", None), CiState::Running);
        assert_eq!(run_state("in_progress", None), CiState::Running);
        assert_eq!(run_state("completed", Some("success")), CiState::Pass);
        assert_eq!(run_state("completed", Some("neutral")), CiState::Neutral);
        assert_eq!(run_state("completed", Some("skipped")), CiState::Skipped);
        assert_eq!(
            run_state("completed", Some("cancelled")),
            CiState::Cancelled
        );
        assert_eq!(run_state("completed", None), CiState::None);
        assert_eq!(run_state("completed", Some("timed_out")), CiState::Fail);
        assert_eq!(
            run_state("completed", Some("action_required")),
            CiState::Fail
        );
    }

    #[test]
    fn status_states() {
        assert_eq!(status_state("success"), CiState::Pass);
        assert_eq!(status_state("pending"), CiState::Running);
        assert_eq!(status_state("error"), CiState::Fail);
    }
}
