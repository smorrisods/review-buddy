//! A minimal GraphQL layer: hand-written queries, typed `serde` responses.
//! See "GraphQL typing" in docs/architecture.md for why this isn't `graphql_client`.

use rb_core::{Error, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::GithubClient;

#[derive(Serialize)]
struct Request<'a> {
    query: &'a str,
    variables: &'a serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GraphqlError {
    pub message: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// Filled when the query asks for `rateLimit { cost remaining resetAt }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct QueryCost {
    pub cost: u64,
    pub remaining: u64,
    #[serde(rename = "resetAt")]
    pub reset_at: String,
}

#[derive(Debug, Clone)]
pub struct Response<T> {
    pub data: T,
    pub cost: Option<QueryCost>,
    /// Partial errors that arrived alongside usable data (for example an unreadable repo).
    pub errors: Vec<GraphqlError>,
}

#[derive(Deserialize)]
struct Envelope {
    data: Option<serde_json::Value>,
    #[serde(default)]
    errors: Vec<GraphqlError>,
}

pub async fn query<T: DeserializeOwned>(
    client: &GithubClient,
    query: &str,
    variables: serde_json::Value,
) -> Result<Response<T>> {
    let req = client.graphql_request().json(&Request {
        query,
        variables: &variables,
    });
    let raw = client.send(req).await?;
    let envelope: Envelope = client.parse(&raw.body)?;
    let host = client.host();
    let Some(mut data) = envelope.data.filter(|d| !d.is_null()) else {
        return Err(map_errors(host, &envelope.errors));
    };
    let cost = data
        .get_mut("rateLimit")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let data = serde_json::from_value(data).map_err(|e| {
        Error::Api(format!(
            "{host} sent something unexpected ({e}). Try again, or run `review-buddy doctor`"
        ))
    })?;
    Ok(Response {
        data,
        cost,
        errors: envelope.errors,
    })
}

fn map_errors(host: &str, errors: &[GraphqlError]) -> Error {
    let Some(first) = errors.first() else {
        return Error::Api(format!("{host} returned no data. Try again"));
    };
    match first.kind.as_deref() {
        Some("NOT_FOUND") => Error::NotFound(format!(
            "{} (on {host}, or the token can't see it)",
            first.message
        )),
        Some("RATE_LIMITED") => Error::RateLimited {
            host: host.to_string(),
            retry_after_secs: None,
        },
        Some("FORBIDDEN") | Some("INSUFFICIENT_SCOPES") => Error::Forbidden {
            host: host.to_string(),
            reason: format!(
                "{}. Check the token's scopes (`repo`, `read:org`)",
                first.message
            ),
        },
        _ => Error::Api(format!("{host} reported: {}", first.message)),
    }
}
