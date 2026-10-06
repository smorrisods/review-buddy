//! rb-gitlab: GitLab REST v4 client, token resolution and `Provider`.

mod auth;
mod changes;
mod checks;
mod client;
mod error;
mod files;
mod provider;
mod rest;
mod review;
mod threads;
mod time;

pub use auth::{Auth, AuthError, ResolvedToken, TokenOrigin};
pub use client::{GitlabClient, RateLimit, TokenReport};
pub use provider::GitlabProvider;
pub use review::{plan_review, PlannedCall, ReviewPlan};
