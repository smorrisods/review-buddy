# GitHub and GitLab integration

Both forges sit behind one `Provider` trait (see `architecture.md`). This page lists what each trait method calls. Endpoint versions are the ones the 1.0 targets; `review-buddy doctor` reports anything an instance doesn't support.

## Authentication

| | GitHub | GitLab |
|---|---|---|
| CLI reuse | `gh auth token --hostname <host>` (read on each start; never copied) | `glab config get token --host <host>`, falling back to `glab auth status -t` |
| Token | Fine-grained or classic PAT. Scopes: `repo`, `read:org` (plus `workflow` for re-running CI) | PAT or group/project access token. Scopes: `api`, `read_user` |
| Storage | OS keyring, `review-buddy/<host>` | same |
| Test | `GET /user` + `GET /rate_limit` | `GET /user` + `GET /personal_access_tokens/self` (shows expiry) |

Token expiry is surfaced 7 days ahead in the footer and in Settings → Sources.

## Listing changes

### GitHub

GraphQL `search` with `type: ISSUE` and these queries, merged and de-duplicated:

- `is:pr is:open review-requested:@me`
- `is:pr is:open reviewed-by:@me`
- `is:pr is:open assignee:@me`
- `is:pr is:open author:@me`
- `is:pr is:open mentions:@me`

Each has a source scope added (`org:liminal-hq`, `repo:o/r`, `user:smorris`). One query fetches title, author, draft, timestamps, `headRefName`, `baseRefName`, `headRefOid`, `additions`, `deletions`, `changedFiles`, `reviewRequests`, `latestReviews`, and `commits(last:1){ statusCheckRollup { state } }`, so a full refresh costs about 5 GraphQL calls per source.

A scope with several qualifiers (for example two orgs and a repo) runs the five searches once per qualifier. Results page 50 at a time, up to 10 pages per search. A change's role comes from which searches matched plus its own reviewer, assignee and author fields, and `my_review`, `my_reviewed_sha` and `i_commented` come from `latestReviews` and the last 30 comments.

GraphQL has no ETags, so the page's `etag` is a fingerprint of every change's node id and `updatedAt`. When the `since` you pass matches, the page is `not_modified` and nothing downstream is rebuilt; the searches still run. Before each request the client checks the last `x-ratelimit-remaining` and the query's `rateLimit` block, and below 25 it stops and returns `RateLimited` with the seconds until reset. Partial SAML errors are ignored while any results came back, and become a `Forbidden` asking for SSO authorisation when nothing did.

### GitLab

`GET /merge_requests?state=opened&scope=all` with:

- `reviewer_username=<me>`
- `assignee_username=<me>`
- `author_username=<me>`
- `my_reaction_emoji` is not used; mentions come from `GET /todos?action=mentioned&type=MergeRequest`

These are scoped per source with `/groups/:id/merge_requests` or `/projects/:id/merge_requests`. Approval state comes from `GET /projects/:id/merge_requests/:iid/approvals`; pipeline status from `head_pipeline` on the MR detail. The list requests run in parallel; the details load lazily for the selected change and the visible rows.

## Diffs

| | GitHub | GitLab |
|---|---|---|
| File list + patches | `GET /repos/:o/:r/pulls/:n/files?per_page=100` (paginated; `patch` per file) | `GET /projects/:id/merge_requests/:iid/diffs?per_page=50` (paginated). Fall back to `/changes` on older instances |
| Very large / truncated | `GET …/pulls/:n` with `Accept: application/vnd.github.diff`, or show "open in browser" | `diff.too_large` / `collapsed` → load the raw blob pair from `/repository/files/:path/raw?ref=` |
| SHAs needed for comments | `head.sha` | `diff_refs { base_sha, start_sha, head_sha }` |
| Viewed state | GraphQL `markFileAsViewed` / `unmarkFileAsViewed` | local only |

Patches are parsed into hunks locally (unified format); side by side is built from the same hunks.

On GitHub, a file whose `patch` is absent (binary or too large) comes back with no patch, and the diff view shows a stub with "open in browser" rather than fetching the whole `.diff`. Pagination stops with an error past 3,000 files, and a next-page link to another host is never followed.

## Threads and comments

| | GitHub | GitLab |
|---|---|---|
| Read | GraphQL `reviewThreads { isResolved, isOutdated, path, line, originalLine, startLine, diffSide, startDiffSide, comments { state } }` (includes your pending review; comments in state `PENDING` mark the thread as pending) + `GET /issues/:n/comments`, shown as unanchored conversation threads. A range comment is anchored at its last line and keeps its `start_line` and `start_side`; an outdated thread falls back to `originalLine` | `GET /projects/:id/merge_requests/:iid/discussions` |
| Pending review | `POST /repos/:o/:r/pulls/:n/reviews` with no `event` creates a pending review; new comments are added via GraphQL `addPullRequestReviewThread` | `POST …/merge_requests/:iid/draft_notes` |
| Line comment | `path`, `line`, `side: RIGHT` (or `LEFT` for deleted lines) | `position { position_type: "text", base_sha, start_sha, head_sha, new_path, old_path, new_line \| old_line }` |
| Range comment | add `start_line`, `start_side` | `position.line_range { start { line_code, type }, end { … } }` |
| Reply | `POST …/pulls/:n/comments/:id/replies` | `POST …/discussions/:id/notes` |
| Resolve | GraphQL `resolveReviewThread` | `PUT …/discussions/:id?resolved=true` |

## Suggestions

Both forges render a fenced suggestion block in a comment body as an applicable change.

- **GitHub:** ` ```suggestion ` … ` ``` `. It replaces exactly the commented line or range.
- **GitLab:** ` ```suggestion:-0+0 ` … ` ``` `. Review Buddy writes the offsets from the range: for a comment anchored on the last line of an N-line range, `-{N-1}+0`.

`⌃S` builds the block from the selected lines, leaving deleted lines out, and keeps their indentation. If the range contains only deleted lines, the composer says "Suggestions apply to the new file. Select added or unchanged lines."

## Submitting a review

| Verdict | GitHub | GitLab |
|---|---|---|
| Approve | `POST …/pulls/:n/reviews/:id/events` `event: APPROVE` (submits the pending review with its comments) | `POST …/draft_notes/bulk_publish`, then `POST …/merge_requests/:iid/approve` (with `sha` = the head SHA, so stale approvals fail cleanly) |
| Request changes | `event: REQUEST_CHANGES`, `body` required | `bulk_publish`, then the reviewer state set to `requested_changes`. **Capability probed on connect** (instance version from `GET /version`, plus a dry check of the reviewers endpoint). Where it's unsupported, `Capabilities.request_changes = false`: `x` is hidden from chips and the palette, and pressing it explains why and suggests `c` |
| Comment only | `event: COMMENT` | `bulk_publish` |

## Merge

| | GitHub | GitLab |
|---|---|---|
| Pre-check | `mergeable`, `mergeStateStatus`, branch protection via GraphQL | `detailed_merge_status` on the MR |
| Merge | `PUT /repos/:o/:r/pulls/:n/merge` `{ merge_method, sha }` | `PUT …/merge_requests/:iid/merge` `{ squash, should_remove_source_branch, sha }` |
| Delete branch | `DELETE /repos/:o/:r/git/refs/heads/:branch` (when `delete_branch_on_merge` and the repo doesn't auto-delete) | via `should_remove_source_branch` |

The confirm modal shows the blocking reason when `mergeStateStatus` / `detailed_merge_status` isn't clean, e.g. "Needs 1 more approval (code owners: @liminal-hq/authoring)".

## CI

| | GitHub | GitLab |
|---|---|---|
| Read | `GET /repos/:o/:r/commits/:sha/check-runs` + `/status` (legacy statuses), merged by name with the check run winning. Neutral, skipped and cancelled keep their own `CiState` and show calmly with distinct glyphs; `started_at`/`completed_at` give each run's duration; the branch-protection "required" flag isn't read, so `required` stays unknown | `GET /projects/:id/pipelines/:pid/jobs` |
| Re-run failed | `POST /repos/:o/:r/actions/runs/:run_id/rerun-failed-jobs` per failed workflow run | `POST /projects/:id/pipelines/:pid/retry` |
| Logs | `details_url` in the browser (`o`) | `web_url` of the job |

## Checkout

Shells out to `git` in the repo found under `checkout.root`:

- GitHub: `git fetch <remote> pull/<n>/head:<branch>` then `git switch <branch>`
- GitLab: `git fetch <remote> merge-requests/<iid>/head:<branch>` then `git switch <branch>`
- If the tree is dirty and `use_worktree_if_dirty`: `git worktree add <root>.review/<branch> <branch>`

## Rate limits, caching, errors

- Every GET uses an ETag cache (`If-None-Match`) stored in SQLite; 304s don't count against GitHub's REST limit.
- GitHub GraphQL cost is read from `rateLimit { remaining, resetAt }`. Below 10% left, refresh pauses for that source with a footer note.
- GitLab `RateLimit-Remaining` / `Retry-After` are honoured.
- Retries use exponential backoff with full jitter (max 3) on 5xx and network errors, but never on 4xx.
- Error copy maps status codes to fixes: 401 "Token rejected. Press e to paste a new one." · 403 with SSO "This org needs SSO approval for your token. o to open the approval page." · 404 on an MR "It may have been closed or moved. r to refresh." · 409 on merge "Head moved since you reviewed. Refresh and look again."
