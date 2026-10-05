//! Loading shared by the demo and live paths: both go through the `Provider` trait.

use rb_core::{ChangeId, Provider, ReviewDraft, Timestamp};

use crate::app::{ChangeInfo, DiffData};

/// The slower, per-change data the detail pane shows: description, checks and threads.
pub async fn fetch_info(provider: &dyn Provider, id: &ChangeId) -> rb_core::Result<ChangeInfo> {
    let (detail, checks, threads) = tokio::try_join!(
        provider.change_detail(id),
        provider.checks(id),
        provider.threads(id)
    )?;
    Ok(ChangeInfo {
        body: detail.body,
        checks,
        threads,
    })
}

/// The patches and threads for the diff screen, with your pending comments.
pub async fn fetch_diff(
    provider: &dyn Provider,
    id: &ChangeId,
    draft: ReviewDraft,
) -> rb_core::Result<DiffData> {
    let (files, threads) = tokio::try_join!(provider.files(id), provider.threads(id))?;
    Ok(DiffData::new(files, threads, draft))
}

pub fn now() -> Timestamp {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    Timestamp(i64::try_from(secs).unwrap_or(i64::MAX))
}
