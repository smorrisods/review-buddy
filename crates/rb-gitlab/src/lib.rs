//! rb-gitlab: GitLab REST v4 client, token resolution and `Provider` skeleton.

mod auth;
mod client;
mod error;
mod provider;

pub use auth::{Auth, AuthError, ResolvedToken, TokenOrigin};
pub use client::{GitlabClient, RateLimit, TokenReport};
pub use provider::GitlabProvider;
