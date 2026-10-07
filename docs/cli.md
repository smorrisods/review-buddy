# Command line

Review Buddy has two faces. Run `review-buddy` with no command and it opens the TUI. Run it with a command and it behaves like `gh`: non-interactive, scriptable and pipe-friendly, across every GitHub and GitLab source you have configured. Both faces share the same config, sources, `Provider` implementations, `rb-core` types, triage engine and cache, so `review-buddy pr list` and the queue on screen never disagree.

This doc is the design for the command line. `docs/configuration.md` keeps a short summary, and `crates/review-buddy/src/cli.rs` is the single source of truth for the flags themselves (the man page is generated from it).

## Status

This page is the design for the whole command line. What runs today is the **read-only core plus sign-in, source and config management**, against GitHub, GitLab and `--demo`:

| Runs today | Planned |
|---|---|
| `open`, `queue`, `pr list`, `pr view`, `pr diff`, `pr checks` (including `--watch`, `--interval`, `--fail-fast` and `--required`), `pr open` (all also as `mr`), `auth status`, `auth login|logout|token`, `source list`, `source test|add`, `config paths`, `config get|list|reset-layout`, `theme list`, `doctor`, `completion`, `--json`, `--jq`, `--web`, `--color`, `--no-color`, `--demo`, `--frozen-time`, `--yes`, selectors and every exit code in the table below | `pr review|comment|merge|checkout|rerun` (v0.3, not declared in the binary yet, so they are a usage error); `triage explain` and `theme check|export` (declared, but not built: they exit `2` with `Not built yet`); `api` (later) |

Declared commands that aren't built exit `2` with `Not built yet. It's planned for <milestone>.` (the milestone text for `triage explain` and `theme check|export` still reads v0.1.0). The `mr` alias works. GitHub and GitLab sources both load against a live forge (see [GitLab sources](#gitlab-sources)), including GitHub Enterprise Server and self-hosted GitLab (see `integrations.md`), and there is no `--no-cache` or `--no-unicode` flag yet. The global flags `--demo-scene`, `--jax-mood` and `--size` are accepted, and have no effect yet. `--setup` (with `--plain` for prompts) runs first run; see `docs/configuration.md`.

## GitLab sources

Every read command (`queue`, `pr list|view|diff|checks|open`) works against gitlab.com and self-hosted GitLab through the same `Provider` code path as GitHub, with the same columns and the same `--json` fields. `mr` is a hidden alias of `pr` (it isn't listed in `--help`), so `review-buddy mr list`, `mr view !1182`, `mr diff`, `mr checks` and `mr open` print exactly what the `pr` forms print.

- **Refs.** Output is native: `platform/flow!1182`, with the project path in full, subgroups included. On input `!` and `#` are interchangeable, so `!1182`, `#1182`, `platform/infra/terraform!3` and `lab:platform/infra/terraform!3` all work.
- **URLs.** Any merge request address is a selector: `https://gitlab.com/group/sub/proj/-/merge_requests/12`, with a tab suffix (`/diffs`, `/commits`), a query or fragment, a trailing slash, or the older form without `/-/`. The host picks the source. For a self-hosted instance served below a path (`https://example.com/gitlab`), set `api_url = "https://example.com/gitlab/api/v4"` on the source and the root is dropped before the project is matched.
- **Scope.** A group in a source's scope covers its subgroups, so `groups = ["platform"]` matches `platform/infra/terraform`, and `groups = ["platform/infra"]` matches only that subgroup.
- **`pr view`.** Adds a `Merge` row (`ready to merge`, `waiting on reviews or checks`, `has conflicts to resolve` and so on) when the forge says. Approvals show in the reviewer list as `approved`. `--comments` heads the conversation with the total and how many threads are unresolved, tags each thread `resolved` or `outdated`, and shows ranges as `path:start-end`; general notes sit under `Conversation`.
- **`pr diff`.** GitLab sends hunks only, so piped output rebuilds the `diff --git` header from the file's status: new and deleted files get `new file mode` / `deleted file mode` and `/dev/null`, and renames get `similarity index`, `rename from` and `rename to`, even when nothing in the file changed. The result goes through `git apply --check`. File modes are written as `100644`, since GitLab's diff list doesn't carry them. Binary and oversized files are skipped with a note on stderr.
- **`pr checks`.** Lists the head pipeline's jobs as `stage / name`. A failed job with `allow_failure` shows as `neutral` and counts as not required, so `--required` leaves it out; a trigger job is listed like any other.
- **Errors.** A rejected token exits `4` and says to run `review-buddy auth login --host <host>`. A token without the `api` or `read_api` scope also exits `4`, names the scopes to create it with, and points at `review-buddy auth status`. A capability the instance lacks exits `5`.

In demo mode, `--source` alone is enough to name a repository when exactly one repository in that source has the number, so `review-buddy --demo mr view !1182 -s platform` works. Live runs still need `--repo` or a matching git remote.

## Principles

- **No command, TUI. Any command, no TUI.** A command never enters the alternate screen, never draws a full-screen UI and never waits for a key unless it is a confirmation on a TTY. `review-buddy open <selector>` is the one command that deliberately launches the TUI (straight into a diff).
- **Forge-neutral.** Commands talk to `Provider`, never to GitHub or GitLab directly. What a source can't do is decided by `Capabilities`, and the error says so and offers an alternative.
- **Pipe-friendly by default.** Human tables on a TTY; stable, tab-separated, uncoloured, untruncated lines when piped. `--json <fields>` is the contract for scripts.
- **Preview before mutate.** Writes (`pr review`, `pr merge`) show what they will do and ask on a TTY, defaulting to **No**. Without a TTY they refuse unless `--yes` is passed.
- **Demo works everywhere.** `--demo` makes every command run on the offline fixtures, with writes mutating memory only and output labelled `(demo)`.
- **Calm copy.** Sentence case, no shouting, and every error says what to do next.

## Command tree

```text
review-buddy                              open the TUI (first run if there's no config)
review-buddy open <selector>              open the TUI straight into one change's diff

review-buddy queue                        your triaged queue, by bucket (like the TUI's All view)
review-buddy pr list                      list changes with filters (state, author, repo, label …)
review-buddy pr view [<selector>]         title, state, reviewers, checks and description
review-buddy pr diff [<selector>]         the patch (coloured on a TTY, raw when piped)
review-buddy pr checks [<selector>]       check runs or pipeline jobs, with --watch
review-buddy pr review [<selector>]       approve, request changes or comment
review-buddy pr comment [<selector>]      a standalone comment (alias of pr review --comment without a verdict)
review-buddy pr merge [<selector>]        merge, after a preview and confirmation
review-buddy pr checkout <selector>       fetch and switch to the change's branch
review-buddy pr open [<selector>]         open in the browser (same as pr view --web; --json is a usage error)
review-buddy pr rerun [<selector>]        re-run failed jobs

review-buddy auth status                  who you are on every host, how you're signed in, token scopes
review-buddy auth login [--host <host>]   store a token in the OS keyring (reads stdin with --with-token)
review-buddy auth logout --host <host>    remove a stored token from the keyring
review-buddy auth token [--host <host>]   print the token review-buddy would use (refuses on a TTY without --show)

review-buddy source list                  configured sources, enabled state, auth method, in All
review-buddy source test [<name>]         sign in and probe capabilities for one or every source
review-buddy source add                   append a [[source]] table to config.toml (flags only, no prompts when piped)

review-buddy triage explain <selector>    which rule or built-in bucketed a change

review-buddy config paths                 every resolved directory and which config files were loaded
review-buddy config get <key>             one resolved value and the layer it came from
review-buddy config list                  every resolved value with its origin
review-buddy config reset-layout          forget the remembered panel layout (session.toml)

review-buddy theme list|check <id>|export <id>
review-buddy doctor                       auth, scopes, rate limits, API versions and XDG paths
review-buddy completion <shell>           bash, zsh, fish, elvish or powershell
review-buddy api <path>                   (later) an authenticated request to one source's API
```

`pr` has the alias `mr`, so GitLab users can type `review-buddy mr view !1182`. The noun is a name only; both work on both forges.

### Existing commands

- `open <url>` keeps its meaning (launch the TUI into a diff) and widens to any selector.
- `theme`, `config paths`, `doctor` and `triage explain` keep their names and gain `--json`.
- `doctor` stays the broad health check; `auth status` is the narrow, scriptable one. `doctor` calls the same code for its auth section.
- `--setup`, `--config` and the `--demo-*` flags are unchanged.

## Global flags

| Flag | Env | Effect |
|---|---|---|
| `--config <path>` | `REVIEW_BUDDY_CONFIG` | Use one config file instead of the XDG user config |
| `-s, --source <name>` | `REVIEW_BUDDY_SOURCE` | Limit to one configured source (repeatable). Default: every source with `in_all = true` |
| `-R, --repo <[host/]owner/repo>` | `REVIEW_BUDDY_REPO` | Limit to one repository. A GitLab path may have subgroups (`platform/infra/terraform`). The host picks the source when more than one matches |
| `--json <fields>` | | Print JSON with the named fields (comma-separated). `--json` with no fields lists the available ones and exits 0 |
| `-q, --jq <expr>` | | Filter the JSON with a jq expression (built in, no `jq` binary needed). Implies `--json` with every field if none are given |
| `-w, --web` | | Open the result in the browser instead of printing it (where it makes sense) |
| `--color <when>` | `NO_COLOR`, `CLICOLOR_FORCE` | `auto` (default), `always` or `never`. `--no-color` is a shorthand for `never` |
| `--demo` | | Run on offline fixtures; nothing is sent and writes are labelled `(demo)` |
| `--frozen-time <iso>` | | Pin relative ages (requires `--demo`); used by tests and screenshots |
| `-y, --yes` | | Answer yes to a confirmation. Required for writes when stdin isn't a TTY |

The global flags are accepted before or after the command (`review-buddy --demo pr list` and `review-buddy pr list --demo` are the same). Selectors and per-command flags follow the command.

## Selectors

Every command that acts on one change takes a selector. In order of precedence:

| Form | Example | Notes |
|---|---|---|
| Web URL | `https://github.com/liminal-hq/spindle/pull/214`, `https://gitlab.work.ca/platform/flow/-/merge_requests/88` | Host picks the source; works for any configured host, including Enterprise and self-hosted ones with a port (`https://ghe.corp.example:8443/…`) or a path prefix (`https://example.com/gitlab/…`, when the source's `api_url` carries the same prefix) |
| Source-qualified | `platform:platform/flow!88`, `liminal-hq:spindle#214` | `source:` is a configured source name. The repo may be bare when the source is scoped to one owner or group |
| Repo-qualified | `liminal-hq/spindle#214`, `platform/flow!88` | Matched against configured sources; if two sources could own it, the error lists them and suggests `--source` |
| Number only | `214`, `#214`, `!88` | Needs `--repo`, or a git repo in the current directory whose remote matches a source |
| Branch | `feat/titleset-menus` | The open change whose head is that branch, in the inferred repo |
| Nothing | | The open change for the current branch, in the current git repo |

`#` and `!` are interchangeable on input (`spindle!214` works) and native on output (`#214` on GitHub, `!1182` on GitLab). Repo inference reads `git remote` (preferring `upstream`, then `origin`), maps the remote host through `url.*.insteadOf`, and matches it to a source by host. A port on an `http(s)` remote is the web port and is matched against the source's `host`; an ssh port is ignored, and a port on only one side still matches. Inference never makes a network call to decide which repo you meant. When several sources cover a repo but are all the same forge on the same host, the first enabled one wins, so `pr view 214 -R liminal-hq/review-buddy` works without `-s`; sources on different forges or hosts stay ambiguous and the error lists them.

When a selector is ambiguous or matches nothing, the error says what was tried: `Couldn't find a change for branch feat/titleset-menus in liminal-hq/spindle. Pass a number or a URL, e.g. review-buddy pr view 214.`

## Output

### TTY vs pipe

Detection is on stdout (`std::io::IsTerminal`). Stderr carries progress and warnings in both modes and is never needed to parse results.

| | stdout is a TTY | stdout is piped |
|---|---|---|
| Lists | Aligned columns, relative ages (`2h`), truncated titles, bucket headings, CI glyphs (`● ◐ ✕`) | One row per line, tab-separated, no headings, full titles, ISO 8601 times, words instead of glyphs (`pass`, `running`, `fail`) |
| Colour | Theme roles via `rb-theme` at the detected depth | None, unless `--color always` |
| Pager | `pr diff` and `pr view` use `$REVIEW_BUDDY_PAGER`, `$PAGER`, else `less -FRX` | Never |
| Prompts | Writes confirm, defaulting to No | Never; writes need `--yes` |
| Empty results | `That's everything. Nothing is waiting on you.` on stderr | Nothing on stdout |

Pipe columns are a stable contract within a major version; new columns are only added at the end. `pr diff` piped is the exact unified patch, suitable for `git apply` or `delta`.

### JSON

- `--json number,title,url` prints an array for lists and an object for single changes. Field names are `camelCase` and come from `rb-core` types, never from a forge's API shape, so the same fields work on GitHub and GitLab.
- Common change fields: `source`, `forge`, `host`, `repo`, `number`, `ref` (`spindle#214`), `url`, `title`, `author`, `state`, `isDraft`, `createdAt`, `updatedAt`, `headRefName`, `baseRefName`, `headSha`, `additions`, `deletions`, `changedFiles`, `ci`, `reviewers`, `myRole`, `myReview`, `bucket`, `bucketReason`, `labels`, `body`, `checks`, `files`, `comments`.
- Expensive fields (`body`, `checks`, `files`, `comments`) are fetched only when asked for.
- `--jq` runs on the selected JSON with an embedded jq implementation (`jaq`), so it works on every platform with no extra binary.
- `--template` (Go-template style in `gh`) is out of scope; `--jq` covers it.

### Copy and glyphs

Human output follows SPEC §2: sentence case, Canadian English, middots for metadata, and the native `#`/`!` prefix. With `--no-unicode` (SPEC §12) glyphs fall back to ASCII. Colour is never the only signal.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success, including an empty list |
| `1` | Something went wrong (network, forge error, bad config). The message says what to do next |
| `2` | Usage error, or a declared command that isn't built yet (`Not built yet. It's planned for v0.1.0.`) |
| `3` | Cancelled: you answered No, or a write needed `--yes` without a TTY |
| `4` | Authentication needed: no credentials, or the token expired or lacks a scope |
| `5` | Not supported: the source's `Capabilities` don't allow it (for example request changes on an older GitLab) |
| `8` | `pr checks` only: checks are still running (failures exit `1`), matching `gh` |

Errors go to stderr as one or two calm lines, never a stack trace (that goes to the log):

```text
Token for gitlab.work.ca expired on 12 Jan.
Run review-buddy auth login --host gitlab.work.ca, or set auth = "cli" if glab is signed in.
```

## Commands in detail

### `queue`

The triaged queue across sources, exactly as the TUI would bucket it (same rules, same Show filters, same `bucket_limit` on a TTY). Flags: `--bucket wait|look|later|noise` (repeatable), `--show reviewing,assigned,authored,drafts,noise`, `--all` (ignore `bucket_limit`). `--show` replaces the configured Show filters for the run; projects named in a source's `hide_repos` are left out (and counted in a note on a TTY) unless `--repo` names one; `--bucket noise` or `--show noise` lists Noise as its own bucket at the end. Piped rows: `bucket  source  ref  ci  author  updatedAt  title`, with `ref` the full `owner/repo#number`. The bucket limit applies on a TTY only. Ends with `── That's everything.` on a TTY.

### `pr list`

Unbucketed listing, closer to `gh pr list`. Flags: `--state open|closed|merged|all` (default `open`), `--author`, `--assignee`, `--reviewer`, `--label` (repeatable), `--draft`, `--search <query>` (passed to the forge's search where supported), `-L, --limit` (default 30). `@me` works for user flags. Filters run on the fetched changes, so `--search` matches the title, repository, author, branch and labels, and `--assignee` accepts only `@me` for now. Piped rows: `source  ref  state  ci  author  updatedAt  title`, newest first.

### `pr view`

Header (`title`, `author wants head → base`, `+adds −dels · N files · opened 2h ago`), reviewers, checks summary, merge readiness when known, bucket and reason, then the description rendered from Markdown. `--comments` appends the conversation, with resolved and outdated threads tagged and ranges shown as `path:start-end`. `--web` opens it.

### `pr diff`

`--name-only`, `--stat`, `--file <path>` (repeatable), `--color`, and `--patch` to force the raw patch on a TTY. Coloured output uses the `rb-diff` model and theme roles; piped output is the forge's patch, byte for byte.

### `pr checks`

Check runs (GitHub) or pipeline jobs (GitLab) with state and duration, plus a URL column when any check has one. `--watch` refreshes on stderr until everything settles (`--interval`, default 10 s); `--fail-fast` exits on the first failure; `--required` limits to required checks where the forge reports them. Exit codes: `0` all passed, `1` some failed, `8` still running.

### `pr review` and `pr comment`

`--approve`, `--request-changes` or `--comment`, with `-b, --body <text>` or `-F, --body-file <path|->`. Request changes needs a body, as on the forge. Line comments are a TUI job; the CLI submits verdicts and general comments only. If you have pending drafts for the change from the TUI, the preview lists them and asks whether to include them (`--with-drafts` / `--without-drafts` to answer up front). Capability gaps exit `5` with an alternative.

### `pr merge`

`--merge|--squash|--rebase` (default `review.merge_method`), `--delete-branch` / `--keep-branch` (default `review.delete_branch_on_merge`), `--subject`, `--body`. The preview repeats the merge-confirm modal (SPEC §4.6) as text, including caveats. Missing required checks or approvals block the merge with an explanation; there is no `--admin` or `--force`. On a TTY it asks, defaulting to No; piped it needs `--yes`.

### `pr checkout`

Runs `git fetch` with the provider's `checkout_refspec` and switches to the branch, honouring `[checkout]` (`root`, `use_worktree_if_dirty`, `clone_if_missing`). `--branch <name>` overrides the local name, `--detach` checks out the commit. `--print` prints the git commands instead of running them (and is what `--demo` always does).

### `auth`

`auth status` prints one line per host: `✓ github.com  signed in as smorris via gh · scopes repo, read:org` or `✕ gitlab.work.ca  token expired 12 Jan · run review-buddy auth login --host gitlab.work.ca`. Exits `4` if any enabled source can't sign in.

`auth login [--host H] [--with-token]` reads a token from standard input with `--with-token`, or from a hidden prompt on a TTY, and refuses (exit `2`) when there is neither. It tests the token against the forge (`GET /user` on GitHub, `GET /user` and `GET /personal_access_tokens/self` on GitLab), keeps it in the OS keyring as `review-buddy/<host>` only if the test passes, and says who it signed in as and the scopes. The token is never printed and never written to `config.toml`, the cache or a log. The host comes from `--host`, `--source`, or the only configured source; a host with no source is treated as GitLab when its name contains `gitlab`, and as GitHub otherwise. A rejected token exits `4` and a keyring that won't keep it exits `1`, each with a fix.

`auth logout [--host H]` removes the keyring entry after a confirmation that defaults to No on a TTY; without a TTY it needs `--yes`. It never touches `gh` or `glab` sign-ins, and says so. Nothing stored is a calm no-op.

`auth token [--host H] [--show]` prints the token review-buddy would use for the host, following its source's `auth` setting (a host with no source tries the keyring, then `gh` or `glab`). It exists for scripts: the token goes to stdout only, where it came from goes to stderr, and a TTY on stdout gets a refusal (exit `2`) unless `--show` is passed. Under `--demo` it exits `2`, because demo mode has no tokens.

All three work under `--demo` where it makes sense: `login` and `logout` print what they would do, labelled `(demo)`, and read or change nothing.

### `source`

`source list` reads config only, no network.

`source test [name] [--require <capability>,…]` signs in to the named source (or every enabled one, or the ones `--source` names), prints who you are, and prints the source's `Capabilities` (request changes, range comments, viewed files, suggestions, resolve threads, re-run failed checks), from a fresh capability probe: the detected version, when it was probed, and why anything is off. `--json` adds `capabilities` and `probe` objects. `doctor` prints the same in its Capabilities section. Exit `4` if a source can't sign in; with `--require`, exit `5` when a tested source lacks a named capability (`request-changes`, `viewed-files`, `range-comments`, `suggestions`, `resolve-threads`, `rerun-failed`).

`source add [--kind github|gitlab] [--host H] [--name N] [--auth cli|token|env:VAR|command] [--token-command CMD] [--org O]… [--group G]… [--user] [--api-url URL] [--no-test]` appends a `[[source]]` to the config write target (the `--config` file when given), with `toml_edit` so comments and ordering stay. It validates the result first, previews the block, and asks to confirm with a default of No on a TTY; without a TTY it needs `--yes`. Kind is guessed from the host and the host defaults to `github.com` or `gitlab.com`, so `source add --yes` is enough for the common case. `--org` is GitHub's scope and `--group` GitLab's. Afterwards it tests the sign-in unless `--no-test`; a failed test leaves the source added, says so, and exits `4` with the fix (for `auth = "token"`, run `auth login`). Under `--demo` it prints the block and writes nothing.

### `doctor`

`doctor` is the broad health check. It never changes anything, and its auth section is the same code as `auth status`. For every enabled source (or the ones `--source` names) it reports:

- **Auth.** Who you are, how the token was found, scopes and expiry, or the fix when sign-in fails.
- **Rate limits.** GitHub's core and GraphQL budgets; GitLab's `RateLimit-*` headers when the instance sends them.
- **API versions.** The server's own version and the API Review Buddy speaks: the GitHub Enterprise Server release from the `X-GitHub-Enterprise-Version` header on `GET /meta`, or GitLab's `version` and `revision` from `GET /version`. github.com has no server version.
- **Endpoints.** The REST and GraphQL addresses Review Buddy will call and the web address it builds links from, after `host` and `api_url` are applied. Check this first when an Enterprise or self-hosted source misbehaves.
- **Clock.** Compares the server's `Date` header with this machine's clock and says so when they differ by more than five minutes, because token expiry checks compare dates.
- **Capabilities.** What each source can do, from the same probe as `source test`: when it was probed and which actions are unavailable. The server version isn't repeated here; it is shown once under API versions (and as `probe.version` in `--json`).
- **Paths.** The XDG directories and which config layers were loaded.

A host that can't be reached, or whose TLS certificate can't be verified, shows up under Auth with a next step: check the host name, `api_url` and VPN, or ask for a publicly trusted certificate. Review Buddy trusts public certificate authorities only for now, so private and self-signed certificates aren't supported yet.

Exit `4` only when a source can't sign in; a missing version or a clock difference is information, not a failure. `--json` fields are `version` (Review Buddy's own), `auth`, `rateLimits`, `apiVersions` (with `product`, `server`, `revision` and `note` per source), `endpoints`, `clock` (`skewSeconds` and `warning`), `capabilities` (per source: the `capabilities` map and a `probe` object with `version`, `probedAt`, `complete` and `reasons`) and `paths` (the same object as `config paths --json`). A self-hosted run looks like this:

```text
Review Buddy 0.2.0

Auth
  ✓ ghe.corp.example:8443 (work)  signed in as smorris via env:RB_TOKEN · scopes repo, read:org · expires 2026-12-01
  ✓ git.corp.example (lab)  signed in as smorris via env:RB_TOKEN · scopes api, read_user · expires 2027-01-12

Rate limits
  ghe.corp.example:8443 (work)  core 4900/5000 · resets in 38 min · graphql 4999/5000

API versions
  ghe.corp.example:8443 (work)  GitHub Enterprise Server 3.12.4 · REST 2022-11-28
  git.corp.example (lab)  GitLab 16.11.2-ee (abc123def) · REST v4

Endpoints
  ghe.corp.example:8443 (work)  REST https://ghe.corp.example:8443/api/v3 · GraphQL https://ghe.corp.example:8443/api/graphql · web https://ghe.corp.example:8443
  git.corp.example (lab)  REST https://git.corp.example/gitlab/api/v4 · web https://git.corp.example/gitlab

Clock
  ghe.corp.example:8443 (work)  in step with this machine
  git.corp.example (lab)  the server's clock is 8 min behind this machine. Token expiry checks may be wrong; sync this machine's clock

Capabilities
  ghe.corp.example:8443 (work)  probed 2026-10-05T10:00:00Z · everything available
  git.corp.example (lab)  probed 2026-10-05T10:00:00Z · not available: request changes, viewed files
```

Under `--demo` nothing is probed. The same sections appear, labelled `(demo)`:

```text
API versions
  github.com (liminal-hq)  GitHub REST 2022-11-28 (demo)
  gitlab.platform.example (platform)  GitLab REST v4 (demo)

Endpoints
  github.com (liminal-hq)  not used (demo)

Clock
  github.com (liminal-hq)  not checked (demo)

Capabilities
  github.com (liminal-hq)  demo capabilities (demo) · everything available
  gitlab.platform.example (platform)  demo capabilities (demo) · not available: request changes, viewed files
```

### `config`

`config paths` lists the resolved directories and loaded files. `config get <key>` prints one effective value, for example `ui.theme`, and `config list` prints every one in a stable order (the sections of `config.example.toml`, then `keys.*`, then each source as `source.<name>.<field>`). On a TTY `get` adds `(from <layer>)`; piped, it prints the bare value so it's safe in `$(…)`. `config list` piped is `key<TAB>value<TAB>origin`. With `--json key,value,origin` both give the origin: the config file that last set the key, `default`, or the override (`$REVIEW_BUDDY_THEME`). Lists print as JSON and durations as `5m`. An unknown key exits `2` with the closest matches.

`config paths` also lists the remembered layout file (`session.toml` in the state directory; `sessionFile` in `--json`, and a `session-file` row piped). `config reset-layout` removes that file, so the next run starts from `config.toml`. It asks first on a TTY (the default is No), needs `--yes` without one (exit `3` with nothing changed otherwise), says so and exits `0` when there is nothing to remove, and never touches `config.toml`. Under `--demo` it prints what it would remove and removes nothing. See `docs/configuration.md#remembered-layout`.

### `api` (later)

`review-buddy api --source work /user` or `api graphql -f query=…`: an authenticated request to one source, printing the response. It is a debugging tool, deliberately unrelated to `Provider`, and arrives after 1.0 if at all.

## Environment

| Var | Effect |
|---|---|
| `REVIEW_BUDDY_SOURCE` | Default for `--source` |
| `REVIEW_BUDDY_REPO` | Default for `--repo` |
| `REVIEW_BUDDY_PAGER` | Pager for `pr diff` and `pr view`; `cat` turns it off |
| `REVIEW_BUDDY_PROMPT_DISABLED=1` | Never prompt; behave as if stdin weren't a TTY |
| `REVIEW_BUDDY_NO_UPDATE_NOTIFIER` | Reserved; there is no update check today |
| `NO_COLOR`, `CLICOLOR_FORCE` | Standard colour switches; `--color` wins |

Tokens are never read from `REVIEW_BUDDY_*`; use `auth = "env:VAR"` on a source.

## Shell completions

`review-buddy completion <shell>` prints a script generated by `clap_complete` from `cli.rs`. Release archives and `.deb`/`.rpm` packages ship bash, zsh and fish completions next to the man page. Dynamic completion of source names and theme ids (read from config) comes later with `clap_complete`'s dynamic engine.

## How it fits the architecture

- **No new crate.** Commands live in the binary crate under `crates/review-buddy/src/cmd/` (one module per noun), next to the TUI. A separate `rb-cli` crate would only have one caller.
- **Shared plumbing.** A small `cmd::context` builds the same `Sources`, providers, cache handle and resolved theme the TUI uses, from the same config layering. `--demo` swaps in `DemoProvider` exactly as the TUI does.
- **Pure where possible.** Selector parsing, repo inference from remotes (given the remote list), field selection, table and TSV formatting live in `rb-core` or as pure functions in `cmd::output`, so they're unit tested without I/O. Commands are thin `async fn`s: resolve, call `Provider`, format.
- **Cache.** Reads use the cache with ETags like the TUI; `--no-cache` forces a fresh fetch. Commands take the instance lock only when they write to the cache, so they run fine while the TUI is open.
- **Triage.** `queue` and the `bucket`/`bucketReason` fields call the same `rb-core` triage engine as the TUI and `triage explain`.

## Testing

- **Unit:** selector grammar (every row of the table above, plus ambiguity errors), remote-to-source matching, `--json` field selection, TSV escaping (tabs and newlines in titles), exit-code mapping from error types.
- **CLI integration:** `assert_cmd` + `predicates` in `crates/review-buddy/tests/`, always with `--demo --frozen-time 2026-10-05T10:00` and a temp `XDG_*` so nothing touches the real machine. Each command is run piped and with `--json`, and the output is pinned with `insta` snapshots. TTY output is covered by forcing TTY mode through a hidden test flag rather than a pseudo-terminal.
- **Writes:** `pr review`, `pr merge` and `pr checkout` are tested in demo mode (memory only, `(demo)` label, `--print` for checkout) and against `wiremock` fixtures for each provider. Live write tests run only against throwaway repos, as for the TUI.
- **Builds:** the CLI compiles and passes clippy under default, `--no-default-features` and `--all-features`. Without `live`, network commands exit `2` with `This build has no network support. Try --demo.`

## Release plan

| Milestone | Commands |
|---|---|
| Foundation | The skeleton from `cli.rs`: flags, stubs that exit `2`, man page |
| v0.1.0 | **Built:** CLI core (output modes, `--json`/`--jq`, selectors, exit codes), `queue`, `pr list`, `pr view`, `pr diff`, `pr checks`, `pr open`, `open`, `auth status`, `source list`, `config paths`, `theme list`, `doctor`, `completion`, all against GitHub and `--demo`. Not built, though planned for 0.1: `triage explain`, `theme check`, `theme export` |
| v0.2.0 | **Built:** GitLab parity for every read command, `mr` alias exercised, `auth login`/`logout`/`token`, `source test`/`add`, `config get`/`list` |
| v0.3.0 | Writes: `pr review`, `pr comment`, `pr merge`, `pr checkout`, `pr rerun` |
| Later | `api`, dynamic completions, `pr checks --watch` polish |

## Open questions

- Should `queue` be the default when stdout isn't a TTY and no command is given (so `review-buddy | head` does something useful), or should that stay an error? This design says error: `review-buddy needs a terminal. Try review-buddy queue.`
- Is `mr` worth having as a visible alias, or only a hidden one?
