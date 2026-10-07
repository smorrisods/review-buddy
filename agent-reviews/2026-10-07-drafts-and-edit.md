# 2026-10-07: drafts that stay, and editing pending comments

**What worked.** A per-change store in `App`, synced after every `update` like the layout tracker, kept the diff screen simple and made the debounce and quit-flush testable without I/O. Keeping the disk model in its own module (`drafts.rs`) with a hash-named file per change made corrupt-file and permission tests small.

**Friction.**
- Syncing live content to the store meant "unsaved since opening" couldn't be judged from the store, so the diff holds a baseline taken at open and the leave confirm compares against it.
- Pty and CLI tests kept hitting a stale `review-buddy` binary under the shared target dir. Touching `src/lib.rs` and `src/main.rs` and rebuilding fixed it each time.
- A second quit warning path was needed for memory-only modes (demo, `ui.drafts = "off"`); with persistence on, quitting simply saves.

**Decisions.** Forge-side pending comments are only listed and editable where `Provider::supports_pending_edit` is true. "Post as general comment" moves an outdated comment into the review summary, since the provider has no standalone general-comment call. Draft ages in the same session use the queue's clock; the runtime stamps the real time when writing to disk.
