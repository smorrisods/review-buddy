//! rb-github: GitHub client, token resolution, GraphQL layer and `Provider` implementation.

mod auth;
mod client;
mod error;
pub mod graphql;
mod provider;

pub use auth::{Auth, AuthError, ResolvedToken, TokenOrigin};
pub use client::{GithubClient, RateLimit, TokenReport};
pub use provider::GithubProvider;
