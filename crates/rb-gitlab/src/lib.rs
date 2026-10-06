//! rb-gitlab: GitLab REST v4 client, token resolution and `Provider`.

mod auth;
mod changes;
mod checks;
mod client;
mod error;
mod files;
mod probe;
mod provider;
mod rest;
mod review;
mod threads;
mod time;

pub use auth::{Auth, AuthError, ResolvedToken, TokenOrigin};
pub use client::{GitlabClient, RateLimit, TokenReport};
pub use probe::{
    can_retry_pipelines, decide, probe, request_changes, RequestChanges, Version,
    REQUEST_CHANGES_SINCE,
};
pub use provider::GitlabProvider;
pub use review::{plan_review, PlannedCall, ReviewPlan};
