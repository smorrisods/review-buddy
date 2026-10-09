# GitHub fixtures

Every file here is **hand-written to match the GitHub GraphQL and REST API documentation** (github.com and GitHub Enterprise Server 3.x). They are not captured from a live account, so they carry no real repository, user or token data: users are `octo` and friends, and tokens never appear. Fields are kept in the shape GitHub returns them, including `rateLimit` blocks and partial-error bodies, so a change to what we read can't pass by accident.

| Fixture | Endpoint |
| --- | --- |
| `author`, `assignee`, `mentions`, `reviewed_by`, `review_requested_p1`, `review_requested_p2`, `empty` | GraphQL `search` queries behind the queue (`author:@me`, `assignee:@me`, `mentions:@me`, `reviewed-by:@me`, `review-requested:@me`), paged and empty |
| `list_many_threads` | A GraphQL search page for one change with more review threads (60) than the 50 the list asks for, to pin the comment count as a floor |
| `sso_partial` | A GraphQL search answering with data and an `errors` entry for an organisation that needs SSO |
| `detail`, `detail_missing` | GraphQL pull request detail, and the `repository: null` answer for one that is gone |
| `files_p1`, `files_p2` | REST `GET /repos/:owner/:repo/pulls/:n/files`, paged, with modified and renamed files |
| `threads_p1`, `threads_p2`, `thread_comments_more` | GraphQL review threads, paged, and the follow-up for a thread with more comments |
| `issue_comments` | REST `GET /repos/:owner/:repo/issues/:n/comments` (the unanchored conversation) |
| `pull_head` | REST `GET /repos/:owner/:repo/pulls/:n` (the head SHA) |
| `check_runs_p1`, `check_runs_p2`, `commit_status` | REST check runs, paged, and the legacy combined commit status |

`tests/fixtures_hygiene.rs` fails when a fixture is not valid JSON, is not referenced by any test, or is missing from this table. When you add a fixture, use it from a test and add it above.
