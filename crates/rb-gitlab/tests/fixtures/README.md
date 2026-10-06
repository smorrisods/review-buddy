# GitLab fixtures

Every file here is **hand-written to match the GitLab 16.x and 17.x REST API documentation** (`/api/v4`). They are not captured from a live instance, so they carry no real project, user or token data: hosts are `gitlab.test`, users are `octo` and friends, and tokens never appear. Fields are kept in the shape GitLab returns them, including fields the client ignores, so a change to what we read can't pass by accident.

| Fixture | Endpoint |
| --- | --- |
| `author`, `assignee`, `author_group`, `reviewer_p1`, `reviewer_p2`, `mr_reviewers` | `GET /merge_requests` and `GET /groups/:id/merge_requests` lists (the queue) |
| `mr_detail` | `GET /projects/:id/merge_requests/:iid` |
| `commits` | `GET /projects/:id/merge_requests/:iid/commits` |
| `files_p1`, `files_p2`, `diffs`, `diffs_shapes` | `GET /projects/:id/merge_requests/:iid/diffs` (paginated, with added, removed, renamed and binary files) |
| `mr_changes` | `GET /projects/:id/merge_requests/:iid/changes`, the fallback for instances without `/diffs` |
| `discussions` | `GET /projects/:id/merge_requests/:iid/discussions` |
| `draft_notes` | `GET /projects/:id/merge_requests/:iid/draft_notes` |
| `approvals` | `GET /projects/:id/merge_requests/:iid/approvals` |
| `pipeline_jobs`, `pipeline_bridges` | `GET /projects/:id/pipelines/:id/jobs` and `/bridges` |
| `todos` | `GET /todos` (mentions and review requests) |
| `user` | `GET /user` |
| `version`, `version_ce` | `GET /version` on an Enterprise Edition and a Community Edition instance |
| `personal_access_token_self` | `GET /personal_access_tokens/self` (scopes and expiry) |
| `error_401`, `error_403`, `error_404`, `error_429` | Error bodies, served with the matching status |

`tests/fixtures_hygiene.rs` fails when a fixture is not valid JSON, is not referenced by any test, or is missing from this table. When you add a fixture, use it from a test and add it above.
