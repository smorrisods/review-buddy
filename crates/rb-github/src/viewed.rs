//! "Viewed" marks on a pull request's files, the checkbox on GitHub's Files changed page.
//! Read with `PullRequestChangedFile.viewerViewedState` and written with `markFileAsViewed` and
//! `unmarkFileAsViewed`, so a file ticked here is ticked on the web too and the other way round.

use rb_core::{ChangeId, Error, Result};
use serde::Deserialize;
use serde_json::json;

use crate::files::repo_parts;
use crate::graphql;
use crate::review::send_graphql;
use crate::GithubClient;

/// 100 files a page, so this covers the 3,000 files GitHub lists for a pull request.
const MAX_PAGES: usize = 30;

const FILES: &str = r"
query($owner: String!, $name: String!, $number: Int!, $after: String) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      files(first: 100, after: $after) {
        pageInfo { hasNextPage endCursor }
        nodes { path viewerViewedState }
      }
    }
  }
}
";

const PULL_REQUEST_ID: &str = r"
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) { pullRequest(number: $number) { id } }
}
";

const MARK: &str = r"
mutation($input: MarkFileAsViewedInput!) {
  markFileAsViewed(input: $input) { pullRequest { id } }
}
";

const UNMARK: &str = r"
mutation($input: UnmarkFileAsViewedInput!) {
  unmarkFileAsViewed(input: $input) { pullRequest { id } }
}
";

#[derive(Deserialize)]
struct Data {
    repository: Option<Repo>,
}

#[derive(Deserialize)]
struct Repo {
    #[serde(rename = "pullRequest")]
    pull_request: Option<Pr>,
}

#[derive(Deserialize)]
struct Pr {
    #[serde(default)]
    id: String,
    files: Option<Files>,
}

#[derive(Deserialize)]
struct Files {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<File>,
}

#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct File {
    path: String,
    #[serde(rename = "viewerViewedState")]
    state: String,
}

fn gone() -> Error {
    Error::NotFound(
        "that pull request is gone, or your token can't see it. Check the address and your access"
            .to_string(),
    )
}

/// The paths marked viewed. A file with new changes since you viewed it (`DISMISSED`) counts as
/// not viewed, as it does on the web.
pub async fn read(client: &GithubClient, id: &ChangeId) -> Result<Vec<String>> {
    let (owner, name) = repo_parts(id)?;
    let mut viewed = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let response: graphql::Response<Data> = graphql::query(
            client,
            FILES,
            json!({ "owner": owner, "name": name, "number": id.number, "after": after }),
        )
        .await?;
        let files = response
            .data
            .repository
            .and_then(|r| r.pull_request)
            .ok_or_else(gone)?
            .files
            .ok_or_else(gone)?;
        viewed.extend(
            files
                .nodes
                .into_iter()
                .filter(|f| f.state == "VIEWED")
                .map(|f| f.path),
        );
        match (files.page_info.has_next_page, files.page_info.end_cursor) {
            (true, Some(cursor)) => after = Some(cursor),
            _ => break,
        }
    }
    Ok(viewed)
}

/// Marks or unmarks one file. The mutation wants the pull request's node id, so it is looked up
/// first.
pub async fn write(client: &GithubClient, id: &ChangeId, path: &str, viewed: bool) -> Result<()> {
    let (owner, name) = repo_parts(id)?;
    let data = send_graphql(
        client,
        json!({
            "query": PULL_REQUEST_ID,
            "variables": { "owner": owner, "name": name, "number": id.number },
        }),
    )
    .await?;
    let found: Data = serde_json::from_value(data).map_err(|_| gone())?;
    let pr = found
        .repository
        .and_then(|r| r.pull_request)
        .filter(|p| !p.id.is_empty())
        .ok_or_else(gone)?;
    let (query, input) = if viewed {
        (MARK, json!({ "pullRequestId": pr.id, "path": path }))
    } else {
        (UNMARK, json!({ "pullRequestId": pr.id, "path": path }))
    };
    send_graphql(
        client,
        json!({ "query": query, "variables": { "input": input } }),
    )
    .await
    .map(|_| ())
}
