# GitHub and GitLab integration

Both forges sit behind one `Provider` trait (see `architecture.md`). This page lists what each trait method calls. Endpoint versions are the ones the 1.0 targets; `review-buddy doctor` reports anything an instance doesn't support.

## Status

`rb-github` implements listing, change detail, files, threads, checks and review writes (comment-only, approve, and replies), behind the `Provider` trait. `rb-gitlab` implements sign-in, listing, change detail, files, threads, pipeline jobs and review writes (comment, approve, reply, resolve) behind the same trait, for gitlab.com and self-hosted GitLab. On both, merge and re-run failed jobs report `Unsupported` until v0.3, and checkout isn't built yet. A capability probe on connect (see "Capability probe") decides which optional actions each source offers, and GitHub Enterprise Server and self-hosted GitLab are covered by "GitHub Enterprise Server and self-hosted GitLab" below. Suggestions (`⌃S`), range comments from the keyboard and viewed-file marks describe later milestones. Token expiry warnings in the footer and Settings → Sources are planned too (Settings → Sources shows the expiry after you test a token).

## Authentication

| | GitHub | GitLab |
|---|---|---|
| CLI reuse | `gh auth token --hostname <host>` (read on each start; never copied) | `glab config get token --host <host>`, falling back to `glab auth status -t` |
| Token | Fine-grained or classic PAT. Scopes: `repo`, `read:org` (plus `workflow` for re-running CI) | PAT or group/project access token. Scopes: `api`, `read_user` |
| Storage | OS keyring, `review-buddy/<host>` | same |
| Test | `GET /user` + `GET /rate_limit` | `GET /user` + `GET /personal_access_tokens/self` (shows expiry) |

On GitLab, a token read from `glab` is sent as `Authorization: Bearer` (it may be an OAuth token, and GitLab accepts personal access tokens that way too); every other token is sent as `PRIVATE-TOKEN`. The API base is `https://<host>/api/v4` unless `api_url` overrides it (a trailing slash is fine). If `GET /personal_access_tokens/self` answers 401, 403 or 404, the test still reports the user and just omits scopes and expiry. `RateLimit-Remaining` and `RateLimit-Reset` are tracked, and a 429 reports its `Retry-After`.

Token expiry is planned to surface 7 days ahead in the footer and in Settings → Sources.

## GitHub Enterprise Server and self-hosted GitLab

Both forges work against your own servers. Set `host` to the name you browse to (with the port if it isn't the default) and add an `api_url` when the API isn't at the usual place:

```toml
[[source]]
name = "work"
kind = "github"
host = "ghe.corp.example"
# https://ghe.corp.example/api/v3 and /api/graphql are derived without api_url

[[source]]
name = "lab"
kind = "gitlab"
host = "example.com"
api_url = "https://example.com/gitlab/api/v4"   # GitLab served below /gitlab
```

| Layout | GitHub | GitLab |
|---|---|---|
| Default | `https://<host>/api/v3`, GraphQL `https://<host>/api/graphql` (github.com: `https://api.github.com` and `/graphql`) | `https://<host>/api/v4` |
| Path prefix or relative root | `api_url = "https://example.com/ghe/api/v3"` gives GraphQL at `https://example.com/ghe/api/graphql` | `api_url = "https://example.com/gitlab/api/v4"` |
| Port | `host = "ghe.corp.example:8443"`, or put the port in `api_url` | same |
| Plain http | `api_url = "http://…"`, for test servers | same |

Links to a change (`pr open`, `open in browser`, copied URLs) are built from the same address, so they keep the scheme, port and prefix. URLs you paste as selectors are matched by host and port, with the prefix removed when the source's `api_url` names it. Git remotes (`https://`, `git@host:owner/repo.git` and `ssh://git@host:2222/…`) are matched to the source by host; an ssh port is ignored because it isn't the web port.

`review-buddy doctor` shows the resolved addresses, the server version (the GitHub Enterprise Server release, or GitLab's version and revision), and warns when the server's clock differs from yours by more than five minutes. Review Buddy trusts public certificate authorities only for now. If a server's certificate can't be verified, the error says so and asks for a publicly trusted certificate; private and self-signed certificates aren't supported yet.

## Listing changes

### GitHub

GraphQL `search` with `type: ISSUE` and these queries, merged and de-duplicated:

- `is:pr is:open review-requested:@me`
- `is:pr is:open reviewed-by:@me`
- `is:pr is:open assignee:@me`
- `is:pr is:open author:@me`
- `is:pr is:open mentions:@me`

Each has a source scope added (`org:liminal-hq`, `repo:o/r`, `user:smorris`). One query fetches title, author, draft, timestamps, `headRefName`, `baseRefName`, `headRefOid`, `additions`, `deletions`, `changedFiles`, `reviewRequests`, `latestReviews`, `reviewDecision`, `totalCommentsCount`, `reviewThreads(first: 50){ nodes { isResolved } }` and `commits(last:1){ statusCheckRollup { state } }`, so a full refresh costs about 5 GraphQL calls per source. The last three feed the queue's status cluster. A page of 50 changes asks for about 8,150 nodes against GitHub's limit of 500,000, and the thread connection adds 50 connection requests, which moves a page from about 3.5 to about 4 rate-limit points. Only the first 50 threads of a change are counted, so a longer conversation reads as a floor.

A scope with several qualifiers (for example two orgs and a repo) runs the five searches once per qualifier. Results page 50 at a time, up to 10 pages per search. A change's role comes from which searches matched plus its own reviewer, assignee and author fields, and `my_review`, `my_reviewed_sha` and `i_commented` come from `latestReviews` and the last 30 comments.

GraphQL has no ETags, so the page's `etag` is a fingerprint of every change's node id and `updatedAt`. When the `since` you pass matches, the page is `not_modified` and nothing downstream is rebuilt; the searches still run. Before each request the client checks the last `x-ratelimit-remaining` and the query's `rateLimit` block, and below 25 it stops and returns `RateLimited` with the seconds until reset. Partial SAML errors are ignored while any results came back, and become a `Forbidden` asking for SSO authorisation when nothing did.

### GitLab

`GET /merge_requests?state=opened&scope=all` with:

- `reviewer_username=<me>`
- `assignee_username=<me>`
- `author_username=<me>`
- `my_reaction_emoji` is not used; mentions come from `GET /todos?action=mentioned&type=MergeRequest`

These are scoped per source: a group (`owners`) uses `GET /groups/:id/merge_requests` with `include_subgroups=true`, a project (`repos`) uses `GET /projects/:id/merge_requests`, and an everything or `user` scope uses `GET /merge_requests`, keeping only your own namespace for `user`. Group and project paths are URL-encoded (`platform/sub` becomes `platform%2Fsub`). Mention to-dos have no group filter, so they are matched against the scope by project path, and done to-dos are ignored. All requests run in parallel, 100 per page, following `X-Next-Page` for up to 10 pages each, and the results merge and de-duplicate by merge request id.

A change's role comes from which queries matched plus its own reviewer, assignee and author fields (reviewing, then authored, assigned, mentioned). A draft is `draft`, `work_in_progress` or a `Draft:`/`WIP:` title prefix; a bot author is `author.bot` or a `project_…_bot`/`group_…_bot` username. `detailed_merge_status` maps to mergeability, and `head_pipeline` (or `pipeline`) maps to CI: `success` is pass, `running`/`pending`/`created`/`preparing`/`scheduled`/`waiting_for_resource` running, `failed` fail, `canceled` cancelled, `skipped` skipped, `manual` neutral. The list also carries `user_notes_count` (comments), `blocking_discussions_resolved` (whether any thread is open, with no count) and `detailed_merge_status = not_approved` (approval still required) for the queue's status cluster. The list has no approvals or line counts, so those come from the details (`/approvals`, diffs) that load when a change is selected.

The list endpoint carries no approvals, reviewer states, pipelines, line counts or commit counts, so queue rows show reviewers as requested, no CI state and no line counts until you open the change. `change_detail` then makes a handful of small calls: the merge request itself (`head_pipeline`, `diff_refs`, `changes_count`), `…/approvals`, `…/reviewers` (per-reviewer state, which gives `my_review` and `i_commented`), `…/commits?per_page=1` (commit count from `X-Total`) and the first 300 files of `…/diffs` (added and removed lines). Any of the extras that an instance doesn't offer is left unknown rather than failing the detail. `my_reviewed_sha` and `has_new_activity` aren't available from GitLab's REST API and stay unset.

GitLab does send ETags, but because a refresh merges several queries the page's `etag` is a fingerprint of every merge request's id and `updated_at`, as on GitHub; a matching `since` returns a `not_modified` page. Before each request the client checks the last `RateLimit-Remaining`, and below 10 it stops and returns `RateLimited` with the seconds until `RateLimit-Reset`. A `429` carries `Retry-After` the same way. Self-hosted instances work through `api_url`.

## Diffs

| | GitHub | GitLab |
|---|---|---|
| File list + patches | `GET /repos/:o/:r/pulls/:n/files?per_page=100` (paginated; `patch` per file) | `GET /projects/:id/merge_requests/:iid/diffs?per_page=50` (paginated). Fall back to `/changes` on older instances |
| Very large / truncated | `GET …/pulls/:n` with `Accept: application/vnd.github.diff`, or show "open in browser" | `too_large` / `collapsed` / `generated_file` → no patch, show "open in browser" |
| SHAs needed for comments | `head.sha` | `diff_refs { base_sha, start_sha, head_sha }` |
| Viewed state | GraphQL `markFileAsViewed` / `unmarkFileAsViewed` | local only |

Patches are parsed into hunks locally (unified format); side by side is built from the same hunks.

On GitLab, `diff` carries hunks only (no `---`/`+++` header), the same shape GitHub's `patch` has. Files are read 50 per page, up to GitLab's 1,000-file cap; `/changes` is used instead when `/diffs` answers 404. A file that is binary, `too_large`, `collapsed` or a `generated_file` comes back with no patch and shows the "open in browser" stub, like GitHub's, rather than loading the raw blob pair.

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

### Threads and drafts on GitLab

- **Threads:** each discussion is a thread with its id encoded as `<project path>!<iid>!<discussion id>`, so reply and resolve need nothing else. System notes are dropped, notes without a position are unanchored conversation threads, `position.line_range` gives a range (the end is `line`, the start is `start_line`/`start_side`), and a thread is resolved when all its resolvable notes are. GitLab has no outdated flag, so `outdated` stays false.
- **Pending review:** `GET …/draft_notes` returns only your drafts. A draft with a `discussion_id` joins that thread as a pending reply; any other is a pending thread, with the draft id as `draft:<id>`. Replying to or resolving a draft's thread says to submit the review first. Instances before GitLab 15.9 have no draft notes endpoint, and threads load without them.
- **Positions:** `base_sha`, `start_sha` and `head_sha` come from the merge request's `diff_refs`. The file's old path comes from the diff list, so renamed files anchor correctly. A comment on an unchanged line sends both `old_line` and `new_line`, an added line only `new_line`, and a removed line only `old_line`. A range adds `line_range` with a `line_code` (`<sha1 of the path>_<old>_<new>`), `type` and line numbers for each end.

## Suggestions

Both forges render a fenced suggestion block in a comment body as an applicable change.

- **GitHub:** ` ```suggestion ` … ` ``` `. It replaces exactly the commented line or range.
- **GitLab:** ` ```suggestion:-0+0 ` … ` ``` `. Review Buddy writes the offsets from the range: for a comment anchored on the last line of an N-line range, `-{N-1}+0`.

Planned for v0.3: `⌃S` builds the block from the selected lines, leaving deleted lines out, and keeps their indentation. If the range contains only deleted lines, the composer says "Suggestions apply to the new file. Select added or unchanged lines."

## Submitting a review

| Verdict | GitHub | GitLab |
|---|---|---|
| Approve | GraphQL `submitPullRequestReview` `event: APPROVE` (submits the pending review with its comments) | `POST …/draft_notes/bulk_publish`, then `POST …/merge_requests/:iid/approve` (with `sha` = the head SHA, so stale approvals fail cleanly) |
| Request changes | `event: REQUEST_CHANGES`, `body` required | Publish draft notes with `bulk_publish` and `reviewer_state: requested_changes`; a summary is required. **Capability probed on connect** (see "Capability probe"). Where it's unsupported, `Capabilities.request_changes = false`: `x` is hidden from chips and the review block, and pressing it explains why and suggests approving or commenting |
| Comment only | `event: COMMENT` | `bulk_publish` |

### Review writes on GitLab

- **Flow:** read your threads (which include drafts), create a draft note for each comment that isn't there already, publish with `POST …/draft_notes/bulk_publish`, then for an approval `POST …/approve` with `sha` set to the head SHA. A review summary has no GitLab equivalent, so it becomes one more draft note without a position. Comments already published by you with the same text on the same line are skipped too, so retrying after a failed approval never posts them twice. If you've already approved, the approval step is skipped.
- **Preview:** `rb_gitlab::plan_review(&ReviewDraft, Verdict)` is pure and mirrors GitHub's, with the same validation and summary text ("Approve with 2 comments").
- **Request changes:** on when the probe finds GitLab 17.3 or newer on an enterprise build (Premium or Ultimate). Viewed-file marks are off; range comments, resolving threads and replies are on.
- **Failures:** if creating or publishing drafts fails, they stay pending on GitLab, nothing is deleted, and the error says how many comments were added. If the approval fails after publishing, the error says the comments are already posted. A 401 on `approve` means this account can't approve here, not a bad token.
- **Errors:** 401 asks you to sign in again. 403 explains the token can read but not review and asks for the `api` scope. 404 says the merge request or project is gone or not visible. 400 or 422 about a line or position asks you to refresh the diff, other 422s show GitLab's reason. 409 (the head SHA moved) says there are new commits. 429 keeps its `Retry-After`.
- **Live smoke test:** `crates/rb-gitlab/tests/writes.rs` has an `#[ignore]`d `live_smoke` test that posts a comment-only review. It only runs when `REVIEW_BUDDY_LIVE_WRITE_GITLAB_PROJECT` names a throwaway project you created for this: `REVIEW_BUDDY_LIVE_WRITE_GITLAB_PROJECT=you/throwaway REVIEW_BUDDY_LIVE_WRITE_GITLAB_MR=1 GITLAB_TOKEN=… cargo test -p rb-gitlab --test writes -- --ignored live_smoke` (set `REVIEW_BUDDY_LIVE_WRITE_GITLAB_HOST` for self-hosted). Never point it at a real project.

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
| Read | `GET /repos/:o/:r/commits/:sha/check-runs` + `/status` (legacy statuses), merged by name with the check run winning. Neutral, skipped and cancelled keep their own `CiState` and show calmly with distinct glyphs; `started_at`/`completed_at` give each run's duration; the branch-protection "required" flag isn't read, so `required` stays unknown | `GET /projects/:id/pipelines/:pid/jobs` (and `/bridges`) for the merge request's head pipeline. Each job is named `stage / name`; `success` is pass, `running`, `pending` and `created` are running, `failed` is fail (neutral when `allow_failure`), `canceled` is cancelled, `skipped` and `manual` are skipped. `allow_failure` gives `required: false`, and `started_at`/`finished_at` give the duration. A downstream pipeline's own jobs aren't expanded |
| Re-run failed | `POST /repos/:o/:r/actions/runs/:run_id/rerun-failed-jobs` per failed workflow run | `POST /projects/:id/pipelines/:pid/retry` |
| Logs | `details_url` in the browser (`o`) | `web_url` of the job |

## Checkout

Shells out to `git` in the repo found under `checkout.root`:

- GitHub: `git fetch <remote> pull/<n>/head:<branch>` then `git switch <branch>`
- GitLab: `git fetch <remote> merge-requests/<iid>/head:<branch>` then `git switch <branch>`
- If the tree is dirty and `use_worktree_if_dirty`: `git worktree add <root>.review/<branch> <branch>`

## Rate limits, caching, errors

- Every GET uses an ETag cache (`If-None-Match`) stored in SQLite; 304s don't count against GitHub's REST limit.
- GitHub GraphQL cost is read from `rateLimit { remaining, resetAt }`. Below 10% left, refresh pauses for that source until the reset (the Sources pane says `paused until HH:MM`), and no other request goes to that host meanwhile.
- GitLab `RateLimit-Remaining` / `Retry-After` are honoured.
- A refresh that fails on a 5xx or a network error retries with exponential backoff and jitter, from 5 s up to 5 minutes, and never retries a 4xx by itself (see `docs/configuration.md`, `[refresh]`).
- Error copy maps status codes to fixes: 401 "Token rejected. Press e to paste a new one." · 403 with SSO "This org needs SSO approval for your token. o to open the approval page." · 404 on an MR "It may have been closed or moved. r to refresh." · 409 on merge "Head moved since you reviewed. Refresh and look again."

## Live loading in the interface

- A provider is built per enabled `[[source]]` by the factory in `providers/`, and `review-buddy` commands and the interface share it. GitHub tokens come from `gh auth token --hostname <host>` (then the keyring), the keyring, an `env:VAR` reference or a `token_command`; `api_url` points Enterprise or a test stub at its API. GitLab tokens come from `glab config get token --host <host>` (then `glab auth status -t`), the keyring, `env:VAR` or a `token_command`, with the same `api_url` override.
- On launch, cached rows from the SQLite cache paint first, then every source refreshes at once (bounded by `max_concurrency_per_host`) and replaces its cached rows. Refresh runs on launch, on `r`, every `refresh.interval` and when the terminal regains focus (`refresh.on_focus`). Transient failures retry with backoff and jitter. See `docs/configuration.md` for the full behaviour.
- A source that fails keeps its cached rows and shows a short reason in the Sources pane and the Detail pane, plus a toast with the next step. With no cache and no sign-in, the queue says "Sign in to see your reviews." and how to fix it. Rate limits say when the limit resets.
- Details, checks and threads load the first time a change is selected; files and threads load when its diff opens.

### Manual check against a real account (read-only)

Sign in with `gh auth login` (or set `auth = "env:GITHUB_TOKEN"`), add a `[[source]]` for `github.com`, run `review-buddy`, and check that rows appear, `r` refreshes, selecting a change fills the Detail pane, and `d` opens its diff. Then run `gh auth logout`, relaunch, and check that cached rows remain with a "sign-in needed" note. Nothing in this flow writes to GitHub.

## Capability probe

On connect, each source is asked what it can do, and the answer fills `Capabilities` for that source. `Provider::probe()` returns a `ProbeOutcome`: the capabilities, the instance version when there is one, whether the instance actually answered (`complete`), and a calm reason for each action that's off. A failed or malformed answer is never an error: the source falls back to its static capabilities and the probe isn't cached.

- **GitLab:** `GET /version` gives `version` and `enterprise`; the token's scopes come from `GET /personal_access_tokens/self`. Request changes needs 17.3 or newer on an enterprise build (the `requested_changes` reviewer state arrived in 16.11 behind a flag, was on by default in 17.2, lost the flag in 17.3, and is Premium and Ultimate; see [Merge request reviews](https://docs.gitlab.com/user/project/merge_requests/reviews/#request-changes)). An enterprise build without a licence looks the same as a licensed one here, so GitLab can still refuse; the error says so. Retrying pipelines needs the `api` scope (unknown scopes get the benefit of the doubt). Range comments, resolving threads and suggestion syntax are on everywhere; viewed-file marks are off. Project and group membership isn't probed, because it costs a request per project.
- **GitHub:** everything is on. Classic tokens report their scopes, and re-running workflows is turned off when the scopes include neither `repo` nor `workflow`. Fine-grained tokens and Apps don't list scopes, so they keep it on.
- **Caching:** the answer is saved in the cache database per forge and host, never per token, for 24 hours, so most launches send nothing. The interface probes each source once per session, after its first successful refresh. Signing in or out (`auth login`, `auth logout`) and changing sources in the first-run flow drop the saved answer. `doctor` and `source test` always ask afresh and save the result.
- **In the interface:** unsupported chips and the `x` line in the review block are hidden. Pressing `x` says why, for example "GitLab 16.9 on gitlab.example.com doesn't support requesting changes (it needs 17.3 or newer). Approve or comment instead." `doctor` and `source test` print the detected version and when it was probed; `--json` adds a `probe` object.
