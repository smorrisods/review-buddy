//! rb-github: GitHub client, token resolution, GraphQL layer and `Provider` implementation.

mod auth;
mod changes;
mod checks;
mod client;
mod error;
mod files;
pub mod graphql;
mod provider;
mod review;
mod threads;
mod time;

pub use auth::{Auth, AuthError, ResolvedToken, TokenOrigin};
pub use client::{GithubClient, RateLimit, TokenReport};
pub use provider::GithubProvider;
pub use review::{plan_review, PlannedCall, ReviewPlan};
