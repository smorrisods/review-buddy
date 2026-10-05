use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Forge-neutral errors. Messages say what happened; the UI adds what to do next.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("the token for {host} was rejected")]
    Unauthorized { host: String },
    #[error("{host} refused the request: {reason}")]
    Forbidden { host: String, reason: String },
    #[error("rate limited by {host}")]
    RateLimited {
        host: String,
        retry_after_secs: Option<u64>,
    },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("{0} isn't supported by this forge")]
    Unsupported(String),
    #[error("couldn't reach {host}: {reason}")]
    Network { host: String, reason: String },
    #[error("unexpected response: {0}")]
    Api(String),
}
