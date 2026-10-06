//! The head pipeline's jobs as checks. See "CI" in docs/integrations.md.
//!
//! Each job becomes a `Check` named `stage / name`. A failed job that is allowed to fail shows
//! as neutral, and `allow_failure` becomes `required: false`. Trigger jobs (bridges) are listed
//! too, in the same way; the downstream pipeline's own jobs aren't expanded.

use rb_core::{ChangeId, Check, CiState, Error, Result};
use serde::Deserialize;
use url::Url;

use crate::rest::{list, mr_base};
use crate::time::parse_rfc3339;
use crate::GitlabClient;

const PER_PAGE: u32 = 100;
const MAX_PAGES: usize = 10;

#[derive(Deserialize)]
struct Mr {
    head_pipeline: Option<PipelineRef>,
}

#[derive(Deserialize)]
struct PipelineRef {
    id: u64,
}

#[derive(Deserialize)]
struct Job {
    #[serde(default)]
    name: String,
    #[serde(default)]
    stage: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    allow_failure: bool,
    web_url: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
}

fn state(status: &str, allow_failure: bool) -> CiState {
    match status {
        "success" | "success-with-warnings" => CiState::Pass,
        "running" | "pending" | "created" | "preparing" | "scheduled" | "waiting_for_resource" => {
            CiState::Running
        }
        "failed" if allow_failure => CiState::Neutral,
        "failed" => CiState::Fail,
        "canceled" | "canceling" => CiState::Cancelled,
        "skipped" | "manual" => CiState::Skipped,
        _ => CiState::None,
    }
}

fn map(job: Job) -> Check {
    let name = if job.stage.is_empty() {
        job.name
    } else {
        format!("{} / {}", job.stage, job.name)
    };
    Check {
        state: state(&job.status, job.allow_failure),
        url: job.web_url.and_then(|u| Url::parse(&u).ok()),
        started_at: job.started_at.as_deref().and_then(parse_rfc3339),
        completed_at: job.finished_at.as_deref().and_then(parse_rfc3339),
        required: Some(!job.allow_failure),
        name,
    }
}

pub(crate) async fn checks(client: &GitlabClient, id: &ChangeId) -> Result<Vec<Check>> {
    let base = mr_base(id);
    let mr: Mr = client.get_json(&base).await.map_err(|e| match e {
        Error::NotFound(_) => Error::NotFound(format!(
            "{} isn't there, or the token can't see it",
            id.short_ref()
        )),
        other => other,
    })?;
    let Some(pipeline) = mr.head_pipeline else {
        return Ok(Vec::new());
    };
    let project = crate::changes::encode(&id.repo);
    let jobs = format!("/projects/{project}/pipelines/{}/jobs", pipeline.id);
    let (jobs, _) = list::<Job>(client, &jobs, PER_PAGE, MAX_PAGES).await?;
    let mut out: Vec<Check> = jobs.into_iter().map(map).collect();
    let bridges = format!("/projects/{project}/pipelines/{}/bridges", pipeline.id);
    match list::<Job>(client, &bridges, PER_PAGE, MAX_PAGES).await {
        Ok((bridges, _)) => out.extend(bridges.into_iter().map(map)),
        Err(Error::NotFound(_) | Error::Forbidden { .. }) => {}
        Err(e) => return Err(e),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_states_map() {
        assert_eq!(state("success", false), CiState::Pass);
        assert_eq!(state("running", false), CiState::Running);
        assert_eq!(state("pending", false), CiState::Running);
        assert_eq!(state("created", false), CiState::Running);
        assert_eq!(state("failed", false), CiState::Fail);
        assert_eq!(state("failed", true), CiState::Neutral);
        assert_eq!(state("canceled", false), CiState::Cancelled);
        assert_eq!(state("skipped", false), CiState::Skipped);
        assert_eq!(state("manual", false), CiState::Skipped);
        assert_eq!(state("???", false), CiState::None);
    }

    #[test]
    fn names_carry_stage_and_durations() {
        let c = map(Job {
            name: "unit".into(),
            stage: "test".into(),
            status: "success".into(),
            allow_failure: true,
            web_url: Some("https://gitlab.test/j/1".into()),
            started_at: Some("2026-01-01T00:00:00.000Z".into()),
            finished_at: Some("2026-01-01T00:01:30.000Z".into()),
        });
        assert_eq!(c.name, "test / unit");
        assert_eq!(c.required, Some(false));
        assert_eq!(c.duration_secs(), Some(90));
    }
}
