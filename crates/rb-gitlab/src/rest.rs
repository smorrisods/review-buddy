//! Small helpers shared by the read and write modules: addresses and paginated lists.

use rb_core::{ChangeId, Result};
use serde::de::DeserializeOwned;

use crate::changes::{encode, get};
use crate::error::header_u64;
use crate::GitlabClient;

pub(crate) fn mr_base(id: &ChangeId) -> String {
    format!(
        "/projects/{}/merge_requests/{}",
        encode(&id.repo),
        id.number
    )
}

/// Every page of a list endpoint, following `X-Next-Page`. The flag is true when `max_pages`
/// ran out while GitLab still had more.
pub(crate) async fn list<T: DeserializeOwned>(
    client: &GitlabClient,
    path: &str,
    per_page: u32,
    max_pages: usize,
) -> Result<(Vec<T>, bool)> {
    let mut out = Vec::new();
    let mut page = 1_u64;
    for _ in 0..max_pages {
        let query = [
            ("per_page", per_page.to_string()),
            ("page", page.to_string()),
        ];
        let (body, headers) = get(client, path, &query).await?;
        out.extend(client.parse::<Vec<T>>(&body)?);
        match header_u64(&headers, "x-next-page") {
            Some(next) if next > 0 => page = next,
            _ => return Ok((out, false)),
        }
    }
    Ok((out, true))
}

/// Reply and resolve only get a `ThreadId`, so it carries the merge request too:
/// `<project path>!<iid>!<discussion id>`. A pending draft note is `<…>!<iid>!draft:<note id>`.
pub(crate) fn thread_id(id: &ChangeId, discussion: &str) -> rb_core::ThreadId {
    rb_core::ThreadId::new(format!("{}!{}!{discussion}", id.repo, id.number))
}

/// The merge request base path and the discussion id inside a `ThreadId`.
pub(crate) fn split_thread_id(thread: &rb_core::ThreadId) -> Result<(String, String)> {
    let mut parts = thread.as_str().splitn(3, '!');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(repo), Some(iid), Some(discussion))
            if !repo.is_empty() && iid.parse::<u64>().is_ok() && !discussion.is_empty() =>
        {
            Ok((
                format!("/projects/{}/merge_requests/{iid}", encode(repo)),
                discussion.to_string(),
            ))
        }
        _ => Err(rb_core::Error::NotFound(
            "that thread isn't one Review Buddy can reach on GitLab. Refresh and try again"
                .to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::{ForgeKind, SourceId};

    #[test]
    fn thread_ids_round_trip() {
        let id = ChangeId {
            source_id: SourceId::new("lab"),
            kind: ForgeKind::GitLab,
            repo: "platform/sub/flow".into(),
            number: 11,
        };
        let t = thread_id(&id, "abc123");
        assert_eq!(
            split_thread_id(&t).unwrap(),
            (
                "/projects/platform%2Fsub%2Fflow/merge_requests/11".to_string(),
                "abc123".to_string()
            )
        );
        assert!(split_thread_id(&rb_core::ThreadId::new("PRRT_x")).is_err());
    }
}
