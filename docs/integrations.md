# GitHub and GitLab integration

Both forges sit behind one `Provider` trait (see `architecture.md`). This page lists what each trait method calls. Endpoint versions are the ones the 1.0 targets; `review-buddy doctor` reports anything an instance doesn't support.

## Status (v0.1.0)

`rb-github` implements listing, change detail, files, threads, checks and review writes (comment-only, approve, and replies), behind the `Provider` trait. Merge and re-run failed jobs report `Unsupported` until v0.3, and checkout isn't built yet. `rb-gitlab` so far implements sign-in only (token resolution, `whoami`, token test, rate-limit tracking); listing, diffs, threads and review writes are the **v0.2 plan**, so a GitLab source signs in and shows its token details in `auth status` and `doctor`, but loads no merge requests yet. Request changes, suggestions (`⌃S`), range comments, viewed-file marks and the `GitLab` columns throughout describe later milestones. Token expiry warnings in the footer and Settings → Sources, and Enterprise API-version checks beyond `doctor`, are planned too.

## Authentication

| | GitHub | GitLab |
|---|---|---|
| CLI reuse | `gh auth token --hostname <host>` (read on each start; never copied) | `glab config get token --host <host>`, falling back to `glab auth status -t` |
| Token | Fine-grained or classic PAT. Scopes: `repo`, `read:org` (plus `workflow` for re-running CI) | PAT or group/project access token. Scopes: `api`, `read_user` |
| Storage | OS keyring, `review-buddy/<host>` | same |
| Test | `GET /user` + `GET /rate_limit` | `GET /user` + `GET /personal_access_tokens/self` (shows expiry) |

On GitLab, a token read from `glab` is sent as `Authorization: Bearer` (it may be an OAuth token, and GitLab accepts personal access tokens that way too); every other token is sent as `PRIVATE-TOKEN`. The API base is `https://<host>/api/v4` unless `api_url` overrides it (a trailing slash is fine). If `GET /personal_access_tokens/self` answers 401, 403 or 404, the test still reports the user and just omits scopes and expiry. `RateLimit-Remaining` and `RateLimit-Reset` are tracked, and a 429 reports its `Retry-After`.

Token expiry is planned to surface 7 days ahead in the footer and in Settings → Sources.

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

These are scoped per source: a group (`owners`) uses `GET /groups/:id/merge_requests` with `include_subgroups=true`, a project (`repos`) uses `GET /projects/:id/merge_requests`, and an everything or `user` scope uses `GET /merge_requests`, keeping only your own namespace for `user`. Group and project paths are URL-encoded (`platform/sub` becomes `platform%2Fsub`). Mention to-dos have no group filter, so they are matched against the scope by project path, and done to-dos are ignored. All requests run in parallel, 100 per page, following `X-Next-Page` for up to 10 pages each, and the results merge and de-duplicate by merge request id.

A change's role comes from which queries matched plus its own reviewer, assignee and author fields (reviewing, then authored, assigned, mentioned). A draft is `draft`, `work_in_progress` or a `Draft:`/`WIP:` title prefix; a bot author is `author.bot` or a `project_…_bot`/`group_…_bot` username. `detailed_merge_status` maps to mergeability, and `head_pipeline` (or `pipeline`) maps to CI: `success` is pass, `running`/`pending`/`created`/`preparing`/`scheduled`/`waiting_for_resource` running, `failed` fail, `canceled` cancelled, `skipped` skipped, `manual` neutral.

The list endpoint carries no approvals, reviewer states, pipelines, line counts or commit counts, so queue rows show reviewers as requested, no CI state and no line counts until you open the change. `change_detail` then makes a handful of small calls: the merge request itself (`head_pipeline`, `diff_refs`, `changes_count`), `…/approvals`, `…/reviewers` (per-reviewer state, which gives `my_review` and `i_commented`), `…/commits?per_page=1` (commit count from `X-Total`) and the first 300 files of `…/diffs` (added and removed lines). Any of the extras that an instance doesn't offer is left unknown rather than failing the detail. `my_reviewed_sha` and `has_new_activity` aren't available from GitLab's REST API and stay unset.

GitLab does send ETags, but because a refresh merges several queries the page's `etag` is a fingerprint of every merge request's id and `updated_at`, as on GitHub; a matching `since` returns a `not_modified` page. Before each request the client checks the last `RateLimit-Remaining`, and below 10 it stops and returns `RateLimited` with the seconds until `RateLimit-Reset`. A `429` carries `Retry-After` the same way. Self-hosted instances work through `api_url`.

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
| Pending review | GraphQL: your existing pending review on the change is reused (found with `reviews(states: [PENDING])`), otherwise `addPullRequestReview` creates one; each draft comment is added with `addPullRequestReviewThread`. Comments already on the pending review (same path, line and body) are skipped, so a retry never duplicates | `POST …/merge_requests/:iid/draft_notes` |
| Line comment | `path`, `line`, `side: RIGHT` (or `LEFT` for deleted lines) | `position { position_type: "text", base_sha, start_sha, head_sha, new_path, old_path, new_line \| old_line }` |
| Range comment | add `start_line`, `start_side` | `position.line_range { start { line_code, type }, end { … } }` |
| Reply | GraphQL `addPullRequestReviewThreadReply` on the thread's node id | `POST …/discussions/:id/notes` |
| Resolve | GraphQL `resolveReviewThread` / `unresolveReviewThread` | `PUT …/discussions/:id?resolved=true` |

## Suggestions

Both forges render a fenced suggestion block in a comment body as an applicable change.

- **GitHub:** ` ```suggestion ` … ` ``` `. It replaces exactly the commented line or range.
- **GitLab:** ` ```suggestion:-0+0 ` … ` ``` `. Review Buddy writes the offsets from the range: for a comment anchored on the last line of an N-line range, `-{N-1}+0`.

Planned for v0.3: `⌃S` builds the block from the selected lines, leaving deleted lines out, and keeps their indentation. If the range contains only deleted lines, the composer says "Suggestions apply to the new file. Select added or unchanged lines."

## Submitting a review

| Verdict | GitHub | GitLab |
|---|---|---|
| Approve | GraphQL `submitPullRequestReview` `event: APPROVE` (submits the pending review with its comments) | `POST …/draft_notes/bulk_publish`, then `POST …/merge_requests/:iid/approve` (with `sha` = the head SHA, so stale approvals fail cleanly) |
| Request changes | `event: REQUEST_CHANGES`, `body` required | `bulk_publish`, then the reviewer state set to `requested_changes`. **Capability probed on connect** (instance version from `GET /version`, plus a dry check of the reviewers endpoint). Where it's unsupported, `Capabilities.request_changes = false`: `x` is hidden from chips and the palette, and pressing it explains why and suggests `c` |
| Comment only | `event: COMMENT` | `bulk_publish` |

### Review writes on GitHub

- **Preview:** `rb_github::plan_review(&ReviewDraft, Verdict)` is pure. It validates the draft (request changes needs a summary, a comment-only review needs something to say, no empty comments or backwards ranges) and lists the calls, so a confirm modal and tests can show what will happen.
- **Post now:** a single comment posted immediately is a draft with that one comment and `Verdict::Comment`. GitHub records it as a one-comment review. Replying on an existing thread uses `reply`.
- **Failures after the pending review exists:** the review stays pending on GitHub, nothing is deleted, and the error says how many comments were added and what to do next. Retrying submits the rest.
- **Errors:** 401 asks you to sign in again. 403 explains that the token can read but not review and suggests the `repo` scope (SSO responses keep their authorise link). 404 says the pull request or repository is gone or not visible. A line that isn't in the diff (422) asks you to refresh the diff. 409 and rate limits keep their own copy. Tokens never appear in errors or request bodies.
- **Live smoke test:** `crates/rb-github/tests/writes.rs` has an `#[ignore]`d `live_smoke` test that posts a comment-only review. It only runs when `REVIEW_BUDDY_LIVE_WRITE_REPO` names a throwaway repository you created for this: `REVIEW_BUDDY_LIVE_WRITE_REPO=you/throwaway REVIEW_BUDDY_LIVE_WRITE_PR=1 GITHUB_TOKEN=… cargo test -p rb-github --test writes -- --ignored live_smoke`. Never point it at a real project.

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

## Live loading in the interface

- A provider is built per enabled `[[source]]` by the factory in `providers/`, and `review-buddy` commands and the interface share it. GitHub tokens come from `gh auth token --hostname <host>` (then the keyring), the keyring, an `env:VAR` reference or a `token_command`; `api_url` points Enterprise or a test stub at its API. GitLab tokens come from `glab config get token --host <host>` (then `glab auth status -t`), the keyring, `env:VAR` or a `token_command`, with the same `api_url` override.
- On launch, cached rows from the SQLite cache paint first, then every source refreshes at once (bounded by `max_concurrency_per_host`) and replaces its cached rows. Refresh runs on launch, on `r` and when the terminal regains focus (`refresh.on_focus`); there is no background polling yet.
- A source that fails keeps its cached rows and shows a short reason in the Sources pane and the Detail pane, plus a toast with the next step. With no cache and no sign-in, the queue says "Sign in to see your reviews." and how to fix it. Rate limits say when the limit resets.
- Details, checks and threads load the first time a change is selected; files and threads load when its diff opens.

### Manual check against a real account (read-only)

Sign in with `gh auth login` (or set `auth = "env:GITHUB_TOKEN"`), add a `[[source]]` for `github.com`, run `review-buddy`, and check that rows appear, `r` refreshes, selecting a change fills the Detail pane, and `d` opens its diff. Then run `gh auth logout`, relaunch, and check that cached rows remain with a "sign-in needed" note. Nothing in this flow writes to GitHub.
