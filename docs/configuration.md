# Configuration

## File locations (XDG Base Directory)

Review Buddy follows the [XDG Base Directory spec](https://specifications.freedesktop.org/basedir-spec/latest/) on Linux, the BSDs **and macOS**. Like `gh`, `glab`, `git` and most terminal tools, it uses `~/.config` on a Mac, not `~/Library`. Each variable is read at start-up. If it is unset, empty or not an absolute path (the spec says relative paths must be ignored), the default is used.

| What | Variable | Default | Path used | Contents |
|---|---|---|---|---|
| Config | `$XDG_CONFIG_HOME` | `~/.config` | `…/review-buddy/` | `config.toml`, `config.d/*.toml`, `themes/*.toml` |
| System config | `$XDG_CONFIG_DIRS` | `/etc/xdg` | `…/review-buddy/` | Admin or distro defaults, same layout. Read-only |
| Data | `$XDG_DATA_HOME` | `~/.local/share` | `…/review-buddy/` | `themes/` installed by theme packs or `review-buddy theme install` |
| System data | `$XDG_DATA_DIRS` | `/usr/local/share:/usr/share` | `…/review-buddy/` | `themes/` shipped by distro packages |
| Cache | `$XDG_CACHE_HOME` | `~/.cache` | `…/review-buddy/` | `cache.sqlite` (summaries, patches, ETags), highlighted-diff cache. Safe to delete at any time |
| State | `$XDG_STATE_HOME` | `~/.local/state` | `…/review-buddy/` | `drafts/`, `queue.jsonl` (offline actions), `session.toml` (last source, selection, layout), `logs/` |
| Runtime | `$XDG_RUNTIME_DIR` | none (falls back to the state dir) | `…/review-buddy/` | `instance.lock` so two copies don't refresh the same cache at once |

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

The app only ever **writes** to step 3 (or the file from step 5). Settings changes made in the UI go there, edited with `toml_edit`, so your comments and ordering are kept. `review-buddy config paths` prints every resolved location and which files were loaded.

### Themes search order

The first match by id wins:

1. `$XDG_CONFIG_HOME/review-buddy/themes/`: your own edits
2. `$XDG_DATA_HOME/review-buddy/themes/`: installed theme packs
3. each `$XDG_DATA_DIRS/review-buddy/themes/`: packaged themes
4. built-ins embedded in the binary

### Creating directories

Directories are created only when something is first written. Created directories use mode `0700`, files `0600`. If `$XDG_RUNTIME_DIR` exists but is not owned by you with `0700`, it is ignored and a warning goes to the log, as the spec requires.

### Migration

If a legacy `~/.review-buddy/` or `~/.review-buddy.toml` exists, first run offers to move it into the XDG locations and leaves a one-line `MOVED.txt` behind.

The app edits the config with `toml_edit`, so your comments and ordering are kept. `config.example.toml` at the repo root is a complete, commented example.

## `[ui]`

| Key | Default | Notes |
|---|---|---|
| `theme` | `"liminal-hq"` | Any built-in or user theme id |
| `layout` | `"panes"` | `panes` · `split` · `queue` |
| `jax` | `true` | Toggle with `J` |
| `reduced_motion` | `false` | Freezes Jax and disables blinking cursors. Also read from the env var `REVIEW_BUDDY_REDUCED_MOTION` |
| `unicode` | `true` | `false` = ASCII glyphs and plain borders |
| `colour_depth` | `"auto"` | `auto` · `truecolor` · `256` · `16` |
| `mouse` | `true` | Click, drag-select, scroll |
| `date_locale` | `"en-CA"` | Ages are relative ("2h"); absolute dates use this locale |

## `[review]`

| Key | Default | Notes |
|---|---|---|
| `merge_method` | `"squash"` | `merge` · `squash` · `rebase`; falls back to the repo default if not allowed |
| `confirm_merge` | `true` | Can't be turned off for protected branches |
| `delete_branch_on_merge` | `true` | |
| `mark_viewed_on_open` | `true` | Marks a file viewed on GitHub; local-only on GitLab |
| `request_changes_needs_summary` | `true` | Opens the composer before submitting |

## `[diff]`

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

| Key | Default | Notes |
|---|---|---|
| `interval` | `"5m"` | Pull only, while the app is open. `"off"` = manual `r` only |
| `on_focus` | `true` | Refresh when the terminal regains focus (if the terminal reports focus events) |
| `max_concurrency_per_host` | `4` | |

## `[triage]`

| Key | Default | Notes |
|---|---|---|
| `noise_authors` | `["renovate[bot]", "dependabot[bot]", "release-please[bot]"]` | Exact logins or globs (`*[bot]`) |
| `bucket_limit` | `20` | Rows shown per bucket before `+N more` |
| `show` | `["reviewing", "assigned", "authored"]` | The default Show filters. Add `"drafts"` to show drafts, or `"noise"` to mix Noise into the normal buckets |
| `noise_collapsed` | `true` | Show Noise as one collapsed row at the end of the queue |
| `stale_after` | `"14d"` | Older items drop to Can wait |

### `[[triage.rule]]`

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

Settings → Review shows the active rules in order and which rule bucketed the selected change (`bucketed by rule 3 · repo platform/infra, path **/*.tf`). `review-buddy triage explain <url>` prints the same from the command line.

## `[checkout]`

| Key | Default | Notes |
|---|---|---|
| `root` | `"~/src/{repo}"` | `{host}`, `{owner}`, `{repo}` placeholders |
| `use_worktree_if_dirty` | `true` | Adds `git worktree` at `{root}.review/{branch}` instead of switching |
| `clone_if_missing` | `"ask"` | `ask` · `always` · `never` |

## `[[source]]`

Repeat one table per source. Order sets the `2`–`9` tab keys (`1` is always All).

| Key | Required | Notes |
|---|---|---|
| `name` | yes | Short label in tabs |
| `kind` | yes | `github` · `gitlab` |
| `host` | yes | `github.com`, `ghe.corp.example`, `gitlab.com`, `gitlab.work.ca` |
| `api_url` | no | Override the derived URL (`https://{host}/api/v3`, `https://{host}/api/v4`) |
| `auth` | no | `cli` (gh / glab), `token` (keyring), `env:VAR_NAME`, or `command` (runs `token_command`). Default `cli` if the CLI is signed in, else `token` |
| `token_command` | no | Used when `auth = "command"`, e.g. `"pass show gitlab/work"` or `"secret-tool lookup service gitlab host work"`. Stdout (trimmed) is the token. Run at start and on 401. For SSH or headless Linux without Secret Service |
| `scope` | no | GitHub: `orgs = [..]`, `repos = [..]`, `user = true`. GitLab: `groups = [..]`, `projects = [..]`. Omit for everything you can see |
| `in_all` | no | Default `true` |
| `include_drafts` | no | Default `false` |
| `tag_colour` | no | Theme role (`github`, `gitlab`, `accent`, `interactive`, `cyan`, …) or a hex |
| `enabled` | no | Default `true` |

## `[keys]`

See `keybindings.md`.

## Environment variables

| Var | Effect |
|---|---|
| `REVIEW_BUDDY_CONFIG` | Path to an alternate config file (replaces the user config layers) |
| `XDG_CONFIG_HOME`, `XDG_CONFIG_DIRS`, `XDG_DATA_HOME`, `XDG_DATA_DIRS`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`, `XDG_RUNTIME_DIR` | Standard base directories; see above |
| `REVIEW_BUDDY_THEME` | Overrides `ui.theme` for this run |
| `REVIEW_BUDDY_REDUCED_MOTION=1` | Same as `ui.reduced_motion = true` |
| `GITHUB_TOKEN`, `GITLAB_TOKEN` | Used only by sources with `auth = "env:…"` |
| `NO_COLOR` | Honoured: roles collapse to bold, dim and reverse. On the command line, output is uncoloured |
| `REVIEW_BUDDY_SOURCE`, `REVIEW_BUDDY_REPO` | Defaults for the command line's `--source` and `--repo` |
| `REVIEW_BUDDY_PAGER` | Pager for `pr diff` and `pr view`; falls back to `$PAGER`, then `less -FRX` |
| `REVIEW_BUDDY_PROMPT_DISABLED=1` | The command line never prompts; writes need `--yes` |

## Command line

With no command, `review-buddy` opens the TUI. With a command it behaves like `gh`: non-interactive, pipe-friendly, with `--json`/`--jq` for scripts. The full design, including selectors, output rules and exit codes, is in `docs/cli.md`.

```text
review-buddy                          open the queue (TUI)
review-buddy --setup                  re-run first run
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

`pr` is also available as `mr`. Selectors accept a URL, `owner/repo#N`, `source:owner/repo!N`, a bare number with `--repo`, a branch, or nothing (the current branch).
