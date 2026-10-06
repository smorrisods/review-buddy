# Configuration

**Status.** The whole file is parsed and validated (unknown keys under `[ui]`, `[review]`, `[diff]` and `[refresh]` are errors, so typos are caught), but only some options change behaviour yet. Each table below has an **Applied** note. Everything else is accepted and kept so your file keeps working as the options arrive. `config.example.toml` at the repo root is the complete, commented starting point, and `review-buddy config list` prints every resolved value with where it came from.

| Applied | Parsed, not applied yet |
|---|---|
| `ui.theme` (and `REVIEW_BUDDY_THEME`), `ui.colour_depth`, `ui.mouse`, `ui.reduced_motion` (and `REVIEW_BUDDY_REDUCED_MOTION`, for the refresh spinner), `diff.tab_width`, `review.confirm_post_now`, `refresh.interval`, `refresh.on_focus`, `refresh.max_concurrency_per_host`, `triage.show`, `triage.bucket_limit`, `triage.noise_authors` and `triage.stale_after` (the dashboard and the `queue`, `pr list` and `pr view` commands), every `[[source]]` key (`name`, `kind`, `host`, `api_url`, `auth`, `token_command`, `scope`, `in_all`, `tag_colour`, `enabled`) for GitHub and GitLab, the config layering below | `ui.layout`, `ui.jax`, `ui.unicode`, `ui.date_locale`, every other `[review]` and `[diff]` key, `triage.noise_collapsed` (Noise is always one collapsed row), `[[triage.rule]]`, a source's `include_drafts` (drafts follow the `drafts` Show filter instead), `[checkout]`, `[keys]` |

## File locations (XDG Base Directory)

Review Buddy follows the [XDG Base Directory spec](https://specifications.freedesktop.org/basedir-spec/latest/) on Linux, the BSDs **and macOS**. Like `gh`, `glab`, `git` and most terminal tools, it uses `~/.config` on a Mac, not `~/Library`. Each variable is read at start-up. If it is unset, empty or not an absolute path (the spec says relative paths must be ignored), the default is used.

| What | Variable | Default | Path used | Contents |
|---|---|---|---|---|
| Config | `$XDG_CONFIG_HOME` | `~/.config` | `…/review-buddy/` | `config.toml`, `config.d/*.toml`, `themes/*.toml` (user themes are not loaded yet) |
| System config | `$XDG_CONFIG_DIRS` | `/etc/xdg` | `…/review-buddy/` | Admin or distro defaults, same layout. Read-only |
| Data | `$XDG_DATA_HOME` | `~/.local/share` | `…/review-buddy/` | `themes/` installed by theme packs or `review-buddy theme install` |
| System data | `$XDG_DATA_DIRS` | `/usr/local/share:/usr/share` | `…/review-buddy/` | `themes/` shipped by distro packages |
| Cache | `$XDG_CACHE_HOME` | `~/.cache` | `…/review-buddy/` | `cache.sqlite` (summaries, details, ETags, capability probes). Safe to delete at any time |
| State | `$XDG_STATE_HOME` | `~/.local/state` | `…/review-buddy/` | Planned: `drafts/`, `queue.jsonl` (offline actions), `session.toml` (last source, selection, layout), `logs/`. Nothing is written here yet |
| Runtime | `$XDG_RUNTIME_DIR` | none (falls back to the state dir) | `…/review-buddy/` | Planned: `instance.lock` so two copies don't refresh the same cache at once. Not used yet |

Secrets never go to disk. They live in the OS keyring (Secret Service on Linux, Keychain on macOS, Credential Manager on Windows) under service `review-buddy`, account `<host>`.

**Windows** has no XDG, so config and data go to `%APPDATA%\review-buddy\`, and cache and state to `%LOCALAPPDATA%\review-buddy\{cache,state}\`. The XDG variables are still honoured if you set them (handy under MSYS2 or WSL interop).

### How config is resolved

Later layers override earlier ones key by key (tables merge; arrays and `[[source]]` lists replace):

1. Built-in defaults
2. `$XDG_CONFIG_DIRS`, each `review-buddy/config.toml`, **least important first**: the spec lists them most important first, so they are read in reverse
3. `$XDG_CONFIG_HOME/review-buddy/config.toml`
4. `$XDG_CONFIG_HOME/review-buddy/config.d/*.toml`, in lexical order (`10-work.toml`, `20-personal.toml`). Useful for keeping work sources in a separate file managed by dotfiles
5. `$REVIEW_BUDDY_CONFIG` or `--config <path>`. If set, this replaces steps 3–4 rather than layering on top
6. Environment overrides (`REVIEW_BUDDY_THEME` …) and command-line flags

The app only ever **writes** to step 3 (or the file from step 5). First run (`--setup`), `review-buddy source add` and Settings → Sources (`,`) write there; the rest of Settings follows in v0.4. Settings edits only the file that defines your `[[source]]` list: if a `config.d` file, a `$XDG_CONFIG_DIRS` file or a `--config` file other than the write target defines it, Settings shows where it comes from and leaves it alone. Edits use `toml_edit`, so your comments and ordering are kept. `review-buddy config paths` prints every resolved location and which files were loaded.

### Themes search order

**Planned.** Only the four built-in themes are available today (`theme list` shows them, and `T` cycles through them); theme files on disk aren't read yet.

The first match by id wins:

1. `$XDG_CONFIG_HOME/review-buddy/themes/`: your own edits
2. `$XDG_DATA_HOME/review-buddy/themes/`: installed theme packs
3. each `$XDG_DATA_DIRS/review-buddy/themes/`: packaged themes
4. built-ins embedded in the binary

### Creating directories

Directories are created only when something is first written. Created directories use mode `0700`, files `0600`. If `$XDG_RUNTIME_DIR` exists but is not owned by you with `0700`, it is ignored and a warning goes to the log, as the spec requires.

### Migration

Planned. A legacy `~/.review-buddy/` or `~/.review-buddy.toml` is not looked at, since there are no earlier releases to migrate from.

## `[ui]`

Applied: `theme`, `colour_depth`, `mouse` and `reduced_motion`.

| Key | Default | Notes |
|---|---|---|
| `theme` | `"liminal-hq"` | Any built-in or user theme id |
| `layout` | `"panes"` | `panes` · `split` · `queue`. Only `panes` exists today |
| `jax` | `true` | Jax is not drawn yet |
| `reduced_motion` | `false` | Swaps the refresh spinner for a still glyph; will also freeze Jax and disable blinking cursors. The env var `REVIEW_BUDDY_REDUCED_MOTION` sets it |
| `unicode` | `true` | `false` = ASCII glyphs and plain borders. Not applied yet |
| `colour_depth` | `"auto"` | `auto` · `truecolor` · `256` · `16` |
| `mouse` | `true` | Click, drag-select, scroll. `false` leaves mouse capture off so the terminal handles the mouse. Demo mode never reads the config and always captures it |
| `date_locale` | `"en-CA"` | Ages are relative ("2h"); absolute dates use this locale. Not applied yet |

## `[review]`

Applied: `confirm_post_now`. The merge, viewed and request-changes options arrive with those features in v0.3.

| Key | Default | Notes |
|---|---|---|
| `merge_method` | `"squash"` | `merge` · `squash` · `rebase`; falls back to the repo default if not allowed |
| `confirm_merge` | `true` | Can't be turned off for protected branches |
| `confirm_post_now` | `true` | Preview a comment or reply before `⌃⏎` posts it on its own. Approving always shows a preview |
| `delete_branch_on_merge` | `true` | |
| `mark_viewed_on_open` | `true` | Marks a file viewed on GitHub; local-only on GitLab |
| `request_changes_needs_summary` | `true` | Opens the composer before submitting |

## `[diff]`

Applied: `tab_width` (1–16). The diff is always unified, with syntax highlighting on; the other options arrive with side by side in v0.3.

| Key | Default | Notes |
|---|---|---|
| `view` | `"unified"` | `unified` · `side-by-side` |
| `auto_side_by_side` | `true` | Use side by side above `side_by_side_min_cols` |
| `side_by_side_min_cols` | `160` | |
| `context_lines` | `3` | |
| `ignore_whitespace` | `false` | Toggle with `w` |
| `syntax_highlight` | `true` | |
| `tab_width` | `4` | |

## `[refresh]`

Applied: all three keys. A refresh runs on launch, on `r`, on a timer (`interval`) and when the terminal regains focus. Every refresh is a pull; nothing runs while the app is closed.

| Key | Default | Notes |
|---|---|---|
| `interval` | `"5m"` | How often to refresh while the app is open and focused, as `"90s"`, `"2m"`, `"1h"`. Each wait is spread by up to 10% either way so windows don't line up. `"off"`, `"0"` or `"0s"` means manual `r` only |
| `on_focus` | `true` | Refresh when the terminal regains focus (if the terminal reports focus events), unless one started in the last 30 seconds |
| `max_concurrency_per_host` | `4` | Requests in flight at once to one host, shared by every source on it |

How a refresh behaves:

- **Each source is independent.** A failing source keeps its cached rows and never holds up the others. The Sources pane shows a spinner while it refreshes, then `offline`, `paused until HH:MM` or `sign-in needed` when something needs saying.
- **Conditional requests.** The page's ETag (or, for GitHub's GraphQL search, a fingerprint of the results) is saved per source and scope. When nothing changed, the cached rows are kept and nothing is rebuilt.
- **Transient failures back off.** Network errors and 5xx answers retry after 5 s, 10 s, 20 s and so on, up to 5 minutes, with the delay partly randomised. Pressing `r` retries straight away. Rejected tokens and other errors don't retry by themselves.
- **Rate limits pause the host.** When a host says it is rate limiting you, nothing is sent to it until the reset time (the Sources pane says `paused until HH:MM`), then the refresh resumes by itself. `r` doesn't override a pause.
- **Refreshes never stack.** Asking again while a refresh is under way, or while a source is waiting to retry, joins the one in flight.
- **Quiet by design.** Toasts appear when a source changes state (goes offline, is rate limited, comes back), not on every poll. When every source is offline and something is cached, the top bar reads `offline · cached HH:MM`. The footer shows when the last refresh finished. Times are UTC for now. With `ui.reduced_motion` the spinner is a still `…`.
- **Demo mode** never schedules refreshes.

## `[triage]`

The dashboard and the commands share one implementation: `noise_authors` and `stale_after` feed the built-in rules, `show` filters the queue by role and drafts, and `bucket_limit` cuts each bucket with a `+N more` row. Under `--demo` the defaults apply. The dashboard starts from the configured `show` and `s` opens a control to change it for the session; those changes aren't written back to `config.toml`. `review-buddy queue --show …` replaces `show` for one run. Items older than `stale_after` that would have been Waiting on you or Worth a look drop to Can wait, measured against the queue's own clock. Headings and the Sources counts reflect what the Show filters let through, and the queue's end note says how many they hide.

| Key | Default | Notes |
|---|---|---|
| `noise_authors` | `["renovate[bot]", "dependabot[bot]", "release-please[bot]"]` | Exact logins or globs (`*[bot]`) |
| `bucket_limit` | `20` | Rows shown per bucket before `+N more`. `⏎` or a click on that row shows the rest |
| `show` | `["reviewing", "assigned", "authored"]` | The starting Show filters. Add `"drafts"` to show drafts, or `"noise"` to mix Noise into the normal buckets (the `queue` command lists it as its own group instead) |
| `noise_collapsed` | `true` | Not applied yet. Noise is always shown as one collapsed row at the end of the queue |
| `stale_after` | `"14d"` | Older items drop to Can wait |

### `[[triage.rule]]`

Planned for v0.4. The rules are parsed but not run yet.

Ordered rules, tried **before** the built-in bucket rules. The first match wins. Every condition in a rule must match (AND). A list inside a condition matches any entry (OR). Globs use `*` and `**`.

| Key | Matches |
|---|---|
| `source` | Source name(s). Omit to apply to every source |
| `author` | Login(s) or globs (`"*[bot]"`) |
| `repo` | `owner/name` globs (`"platform/*"`) |
| `label` | Any of these labels is present |
| `path` | Any changed file matches (`"infra/**/*.tf"`) |
| `role` | Your role: `reviewing` · `assigned` · `authored` · `mentioned` |
| `draft` | `true` / `false` |
| `title` | Regex on the title |
| `ci` | `pass` · `running` · `fail` |
| **`bucket`** (required) | `wait` · `look` · `later` · `noise` |

```toml
# Work reviews are only "Waiting on you" when I'm explicitly assigned.
[[triage.rule]]
source = "platform"
role = "reviewing"
bucket = "look"

[[triage.rule]]
source = "platform"
role = "assigned"
bucket = "wait"

# Anything touching Terraform in infra needs my eyes.
[[triage.rule]]
repo = "platform/infra"
path = "**/*.tf"
bucket = "wait"

# Docs-only changes can wait, wherever they come from.
[[triage.rule]]
path = ["docs/**", "*.md"]
bucket = "later"
```

Settings → Review will show the active rules in order and which rule bucketed the selected change (`bucketed by rule 3 · repo platform/infra, path **/*.tf`), and `review-buddy triage explain <url>` will print the same from the command line. Neither exists yet.

## `[checkout]`

Planned for v0.3 with checkout. Parsed, not used yet.

| Key | Default | Notes |
|---|---|---|
| `root` | `"~/src/{repo}"` | `{host}`, `{owner}`, `{repo}` placeholders |
| `use_worktree_if_dirty` | `true` | Adds `git worktree` at `{root}.review/{branch}` instead of switching |
| `clone_if_missing` | `"ask"` | `ask` · `always` · `never` |

## `[[source]]`

Repeat one table per source. Order sets the `2`–`9` keys (`1` is always All). Both kinds load fully: lists, details, diffs, threads, checks and review writes. A failing source never stops the others.

| Key | Required | Notes |
|---|---|---|
| `name` | yes | Short label in tabs |
| `kind` | yes | `github` · `gitlab` |
| `host` | yes | A bare host name, with a port when the web UI uses one: `github.com`, `ghe.corp.example`, `ghe.corp.example:8443`, `gitlab.com`, `gitlab.work.ca`. No scheme and no path |
| `api_url` | no | Override the derived API base (GitHub: `https://api.github.com` for github.com, else `https://{host}/api/v3`; GitLab: `https://{host}/api/v4`). Needed for a path prefix or relative URL root (`https://example.com/gitlab/api/v4`), plain `http`, or an API on another port. A trailing slash is fine. GitHub's GraphQL address and every web link follow it: `…/api/v3` becomes `…/api/graphql`, and links use the base without `/api/v3` or `/api/v4`. `review-buddy doctor` prints the addresses it resolved |
| `auth` | no | `cli` (gh / glab), `token` (keyring), `env:VAR_NAME`, or `command` (runs `token_command`). Default `cli` if the CLI is signed in, else `token` |
| `token_command` | no | Used when `auth = "command"`, e.g. `"pass show gitlab/work"` or `"secret-tool lookup service gitlab host work"`. Stdout (trimmed) is the token. Run at start and on 401. For SSH or headless Linux without Secret Service |
| `scope` | no | GitHub: `orgs = [..]`, `repos = [..]`, `user = true`. GitLab: `groups = [..]`, `projects = [..]`. Omit for everything you can see |
| `in_all` | no | Default `true`. Set `false` to leave the source out of the All view; it stays selectable on its own (`1`–`9`, or its row or tab) and is marked "not in All" |
| `include_drafts` | no | Default `false`. Parsed but not applied: drafts are governed by `triage.show` |
| `tag_colour` | no | Colours the source's dot and its `GH`/`GL` tag. A theme role (`github`, `gitlab`, `accent`, `interactive`, `cyan`, `success`, …) or a `#rrggbb` hex. Background roles aren't allowed, and a bad value is a config error naming the source. Follows the terminal's colour depth, and `NO_COLOR` drops it (the tag text stays) |
| `enabled` | no | Default `true` |

## `[keys]`

Parsed but not applied yet. See `keybindings.md`.

## Environment variables

| Var | Effect |
|---|---|
| `REVIEW_BUDDY_CONFIG` | Path to an alternate config file (replaces the user config layers) |
| `XDG_CONFIG_HOME`, `XDG_CONFIG_DIRS`, `XDG_DATA_HOME`, `XDG_DATA_DIRS`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`, `XDG_RUNTIME_DIR` | Standard base directories; see above |
| `REVIEW_BUDDY_THEME` | Overrides `ui.theme` for this run |
| `REVIEW_BUDDY_COLOUR_DEPTH` | Forces the colour depth for this run: `truecolor`, `256` or `16` (overrides detection; `ui.colour_depth` in `config.toml` does the same persistently) |
| `REVIEW_BUDDY_REDUCED_MOTION=1` | Same as `ui.reduced_motion = true` (only the refresh spinner animates today) |
| `GITHUB_TOKEN`, `GITLAB_TOKEN` | Used only by sources with `auth = "env:…"` |
| `NO_COLOR` | Honoured: roles collapse to bold, dim and reverse. On the command line, output is uncoloured |
| `REVIEW_BUDDY_SOURCE`, `REVIEW_BUDDY_REPO` | Defaults for the command line's `--source` and `--repo` |
| `REVIEW_BUDDY_PAGER` | Pager for `pr diff` and `pr view`; falls back to `$PAGER`, then `less -FRX` |
| `REVIEW_BUDDY_PROMPT_DISABLED=1` | The command line never prompts; writes need `--yes` |

## Command line

With no command, `review-buddy` opens the TUI. With a command it behaves like `gh`: non-interactive, pipe-friendly, with `--json`/`--jq` for scripts. The full design, including selectors, output rules and exit codes, is in `docs/cli.md`. The block below is the target surface; what runs today is listed after it.

```text
review-buddy                          open the queue (TUI)
review-buddy --setup [--plain]         connect accounts and write config.toml (first run, re-runnable)
review-buddy open <selector>          open the TUI straight into one change's diff
review-buddy queue                    your triaged queue, by bucket
review-buddy pr list|view|diff|checks|open [<selector>]
review-buddy pr review|comment|merge|checkout|rerun [<selector>]
review-buddy auth status|login|logout|token
review-buddy source list|test|add
review-buddy triage explain <selector>   show which rule or built-in bucketed a change
review-buddy config paths|get|list    resolved directories, loaded files and values
review-buddy theme list|check <id>|export <id>
review-buddy doctor                   check auth, scopes, rate limits, API versions and XDG paths
review-buddy completion <shell>       print a shell completion script

global: --config <path> · -s/--source <name> · -R/--repo <owner/repo> · --json <fields> · -q/--jq <expr>
        -w/--web · --color auto|always|never · --no-color · -y/--yes
        --demo [--demo-scene <name>] [--frozen-time <iso>] [--jax-mood <mood>] [--size <COLSxROWS>]
```

These run: `queue`, `pr list|view|diff|checks|open` (also as `mr`), `open`, `auth status|login|logout|token`, `source list|test|add`, `config paths|get|list`, `theme list`, `doctor` and `completion`. `triage explain` and `theme check|export` exit `2` with `Not built yet`, and the write commands in the block above (`pr review|comment|merge|checkout|rerun`) aren't declared yet, so they are a usage error (exit `2`). `--demo-scene`, `--jax-mood` and `--size` are accepted but have no effect yet. `--setup` runs first run (see below).

### First run

With no config file in any location (and no sources), a TTY launch opens first run. `--setup` opens it again, and `--setup --plain` (alias `--no-tui`), or `--setup` without a terminal, runs the same flow as line prompts. Demo mode never shows first run and never touches your real config or keyring.

- **Where it writes.** The layered write target (`config paths` shows it): `--config` or `$REVIEW_BUDDY_CONFIG` if set, else the user `config.toml`. The file is rendered from `config.example.toml` so it keeps its comments, validated, then written atomically with mode `0600` after the confirm step.
- **Existing files.** Never overwritten without a confirmation that defaults to No. A confirmed replace keeps the old file as `config.toml.bak`, and keeps any `api_url` you had set for a host.
- **Tokens.** Reusing `gh`/`glab` writes `auth = "cli"`. A pasted token is tested, then stored in the OS keyring under `review-buddy/<host>` and the source gets `auth = "token"`. Tokens are never written to the config. If the keyring isn't available, the message suggests an `env:VAR` instead.
- **Scope.** One source per host, named after the host. Leave every organisation and "my repositories" unticked to include everything the account can see.
- **Plain exit codes.** `0` written and signed in, `1` couldn't write, `2` usage (for example `--demo`), `3` stopped without changes, `4` a new source couldn't sign in.

`pr` is also available as `mr`. Selectors accept a URL, `owner/repo#N`, `source:owner/repo!N`, a bare number with `--repo`, a branch, or nothing (the current branch).
