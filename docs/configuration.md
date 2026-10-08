# Configuration

**Status.** The whole file is parsed and validated (unknown keys under `[ui]`, `[review]`, `[diff]` and `[refresh]` are errors, so typos are caught), but only some options change behaviour yet. Each table below has an **Applied** note. Everything else is accepted and kept so your file keeps working as the options arrive. `config.example.toml` at the repo root is the complete, commented starting point, and `review-buddy config list` prints every resolved value with where it came from.

| Applied | Parsed, not applied yet |
|---|---|
| `ui.theme` (and `REVIEW_BUDDY_THEME`), `ui.colour_depth`, `ui.background` and `[ui.theme_background]` (and `REVIEW_BUDDY_BACKGROUND`), `ui.mouse`, `ui.images` (and `REVIEW_BUDDY_IMAGES`), `ui.drafts`, `ui.remember_layout`, `[ui.terminal]`, `ui.queue_width` and `ui.queue_height`, `ui.sources`, `ui.detail`, `ui.detail_position` (and `REVIEW_BUDDY_DETAIL_POSITION`), `ui.reduced_motion` (and `REVIEW_BUDDY_REDUCED_MOTION`, for the refresh spinner), `diff.tab_width`, `diff.wrap`, `review.confirm_post_now`, `refresh.interval`, `refresh.on_focus`, `refresh.max_concurrency_per_host`, `triage.show`, `triage.bucket_limit`, `triage.noise_authors` and `triage.stale_after` (the dashboard and the `queue`, `pr list` and `pr view` commands), every `[[source]]` key (`name`, `kind`, `host`, `api_url`, `auth`, `token_command`, `scope`, `in_all`, `tag_colour`, `hide_repos`, `enabled`) for GitHub and GitLab, the config layering below | `ui.layout`, `ui.jax`, `ui.unicode`, `ui.date_locale`, every other `[review]` and `[diff]` key (including `diff.syntax_highlight`), `triage.noise_collapsed` (Noise is always one collapsed row), `[[triage.rule]]`, a source's `include_drafts` (drafts follow the `drafts` Show filter instead), `[checkout]`, `[keys]` |

## File locations (XDG Base Directory)

Review Buddy follows the [XDG Base Directory spec](https://specifications.freedesktop.org/basedir-spec/latest/) on Linux, the BSDs **and macOS**. Like `gh`, `glab`, `git` and most terminal tools, it uses `~/.config` on a Mac, not `~/Library`. Each variable is read at start-up. If it is unset, empty or not an absolute path (the spec says relative paths must be ignored), the default is used.

| What | Variable | Default | Path used | Contents |
|---|---|---|---|---|
| Config | `$XDG_CONFIG_HOME` | `~/.config` | `…/review-buddy/` | `config.toml`, `config.d/*.toml`, `themes/*.toml` (user themes are not loaded yet) |
| System config | `$XDG_CONFIG_DIRS` | `/etc/xdg` | `…/review-buddy/` | Admin or distro defaults, same layout. Read-only |
| Data | `$XDG_DATA_HOME` | `~/.local/share` | `…/review-buddy/` | `themes/` installed by theme packs or `review-buddy theme install` |
| System data | `$XDG_DATA_DIRS` | `/usr/local/share:/usr/share` | `…/review-buddy/` | `themes/` shipped by distro packages |
| Cache | `$XDG_CACHE_HOME` | `~/.cache` | `…/review-buddy/` | `cache.sqlite` (summaries, details, ETags, capability probes) and `images/` (fetched description pictures, at most 100 MB). Safe to delete at any time |
| State | `$XDG_STATE_HOME` | `~/.local/state` | `…/review-buddy/` | `session.toml` (the remembered panel layout, see [Remembered layout](#remembered-layout)), `drafts/` (unsent review comments, see [Review drafts on disk](#review-drafts-on-disk)) and `worktrees/` (the terminal pane's managed checkouts). Planned: `queue.jsonl` (offline actions), `logs/` |
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

Applied: `theme`, `colour_depth`, `images`, `background`, `theme_background`, `mouse`, `remember_layout`, `drafts`, `reduced_motion`, `sources`, `detail`, `detail_position`, `queue_width`, `queue_height` and the `[ui.terminal]` table (below).

| Key | Default | Notes |
|---|---|---|
| `theme` | `"liminal-hq"` | Any built-in or user theme id |
| `layout` | `"panes"` | `panes` · `split` · `queue`. Only `panes` exists today |
| `sources` | `"auto"` | `auto` · `left` · `top`. Where the Sources sit. `auto` shows the pane at 130 columns or more and the tab strip below; `left` always shows the pane; `top` always shows the strip, which frees 26 columns for the Queue and Detail. `S` cycles it for the session, and the choice is remembered between runs |
| `detail` | `"auto"` | `auto` · `open` · `closed`. `closed` starts with the Detail pane closed, so the Queue uses the full width; `auto` and `open` show it. `p` toggles it for the session. Neither key is saved to the file |
| `detail_position` | `"auto"` | `auto` · `right` · `left` · `top` · `bottom`. Where the Detail pane sits relative to the Queue. `auto` puts it on the right when the terminal is at least 110 columns wide and the Queue (48) and Detail (50) fit side by side, otherwise below the Queue. `left` swaps the two panes; `top` and `bottom` stack them, giving the Queue about 55 percent of the height and at least 8 rows. The env var `REVIEW_BUDDY_DETAIL_POSITION` sets it for one run, and `P` rotates it (auto, bottom, left, top, right; skipping a stop that looks the same as what is on screen) for the session. The choice is remembered between runs (see [Remembered layout](#remembered-layout)); the key never edits the file |
| `queue_width` | automatic | Starting width of the Queue when Detail sits beside it: a number of columns (`52`) or a percentage of the space the Queue and Detail share (`"60%"`, 10 to 90). The Queue never goes below 30 columns and Detail never below 36. Without it the Queue is 48 columns (60 with Sources on top at 130 columns or more). Dragging the seam or `<` / `>` change it for the session, and `W` saves the current size here. `config get ui.queue_width` shows `auto` when unset |
| `queue_height` | automatic | The same for the Queue's height when Detail is above or below it: rows (`14`) or a percentage of the height (`"50%"`). At least 6 rows each. Without it the Queue gets about 55 percent and at least 8 rows |
| `jax` | `true` | Jax is not drawn yet |
| `reduced_motion` | `false` | Swaps the refresh spinner for a still glyph; will also freeze Jax and disable blinking cursors. The env var `REVIEW_BUDDY_REDUCED_MOTION` sets it |
| `images` | `"auto"` | `auto` · `off` · `halfblocks` · `forge-only`. Pictures in change descriptions. `auto` draws them with the best protocol the terminal reports (sixel, kitty, iTerm2), else halfblocks on a truecolour terminal, else a note; `off` shows notes only and fetches nothing; `halfblocks` skips the terminal query and always uses halfblocks; `forge-only` is `auto` that fetches only from the source's own hosts. The env var `REVIEW_BUDDY_IMAGES` takes the same values for one run. `NO_COLOR` always means notes. See [Images](#images) |
| `unicode` | `true` | `false` = ASCII glyphs and plain borders. Not applied yet |
| `colour_depth` | `"auto"` | `auto` · `truecolor` · `256` · `16` |
| `background` | `"theme"` | `theme` · `yes` · `no`. Whether the frame paints a background: `theme` follows the theme (Afterglow paints, Liminal HQ and Dusk don't), `yes` always paints, `no` never does. `NO_COLOR` never paints, and 16 colours paint only with `yes`. `B` cycles it for the session and `REVIEW_BUDDY_BACKGROUND` overrides it for one run. See [theming](theming.md#background) |
| `theme_background` | _none_ | A `[ui.theme_background]` table of theme id to `theme` · `yes` · `no`, for example `dusk = "yes"`. Beats `background` for that theme. Precedence, strongest first: the `B` key, `REVIEW_BUDDY_BACKGROUND`, this table, `background`, the theme's own default |
| `mouse` | `true` | Click, drag-select, scroll. `false` leaves mouse capture off so the terminal handles the mouse; `M` turns it on or off for the session either way. Demo mode never reads the config and always captures it |
| `remember_layout` | `true` | Remember the panel layout between runs: where Detail sits (`P`), where Sources sit (`S`), whether Detail is open (`p`), the dragged sizes and the background mode (`B`), in `session.toml` in the state directory. `false` neither reads nor writes that file. See [Remembered layout](#remembered-layout) |
| `drafts` | `"local"` | `local` · `off`. Where your unsent review comments are kept. `local` saves them (with the summary and chosen verdict) under the state directory's `drafts/` folder so they survive leaving the diff and restarting the app; `off` keeps them in memory for the session only and never writes to disk. Demo mode is always memory only. See [Review drafts](keybindings.md#review-drafts) |
| `date_locale` | `"en-CA"` | Ages are relative ("2h"); absolute dates use this locale. Not applied yet |

### Images

Pictures in a description are fetched when the change is selected, from the address the author wrote. That has consequences worth knowing.

- **A remote image tells its host who is looking.** Fetching `https://tracker.example/pixel.png` shows that host your IP address and when you opened the change, the same as opening the page in a browser would. Third-party images are fetched anonymously (no token, no cookies, no referrer). If that bothers you, set `ui.images = "forge-only"` to fetch only from your forge's own hosts, or `"off"` to fetch nothing. Under `forge-only` an image on another host is not requested at all, and its note says why.
- **What counts as the forge's own.** For github.com: `github.com`, `api.github.com` and anything under `githubusercontent.com` (user-images, private-user-images, objects, camo). For GitHub Enterprise: the host, its `api.` host, anything under the host and the host of `api_url`. For GitLab: the source's host and the host of `api_url`. Anything else is a third party.
- **Where the token goes.** The source's token is sent only to its web and API hosts (`github.com` and `api.github.com`, or the configured host and `api_url` host), only over https, and never to the content hosts, whose addresses are signed. Every redirect is checked again: the token is dropped as soon as a redirect leaves those hosts, and `forge-only` refuses a redirect to another host. Tokens, signed addresses and query strings are never logged or shown; failure notes name a host only. Private repositories need a source that can read the attachment, and GitHub may still refuse a token for a `user-attachments` address, in which case the note says your sign-in can't see it and `o` opens it in the browser.
- **Addresses that point at this machine or a private network** (`localhost`, `127.0.0.1`, `10.x`, `192.168.x`, `169.254.x`) are never fetched for a third party. A host name that resolves to a private address is not caught.
- **Limits.** 10 MB per image, 8192 pixels on a side, 40 million pixels, 20 seconds, four redirects. The type is decided by the first bytes, never by the file name or `Content-Type`; PNG, JPEG, GIF (first frame) and WebP are drawn, SVG and everything else get a note.
- **Cache.** Bytes are kept in `images/` under the cache directory, in files named by a hash of the address (mode `0600`, directory `0700`), at most 100 MB with the least recently used removed first. A copy under 7 days old is used without asking the server; an older one is used only if the server can't be reached, so cached images still show offline. A private repository's images stay in this cache until it is trimmed or deleted: it is safe to delete `images/` at any time. Demo mode never reads or writes it.
- **Terminal support.** Sixel, kitty and iTerm2 are used only when the terminal says it supports them in answer to a query at startup (about 0.4 seconds of waiting at most; a terminal that doesn't answer is treated as having no graphics). `REVIEW_BUDDY_IMAGES=halfblocks` skips the query. Over `ssh` or inside `tmux`, graphics may not pass through, in which case use `halfblocks` or `off`.

### `[ui.terminal]`

The terminal pane that `t` opens next to the change (see [keybindings](keybindings.md#terminal-pane)). Every key is optional. None of them is read in `--demo`, which opens a scripted pane that starts nothing.

```toml
[ui.terminal]
command = []            # program and arguments; empty runs $SHELL (%COMSPEC% on Windows). e.g. ["claude"] or ["opencode"]
escape = "ctrl-\\"      # the chord that starts leaving the pane; then esc returns to the app
position = "auto"       # auto | right | left | top | bottom
size = "40%"            # columns or rows, or a percentage; unset is automatic
scrollback = 10000      # lines of history for scrolling back (100 to 200000)
start = "ask"           # ask | worktree | current

[ui.terminal.checkouts] # local clones to make worktrees from, by repository
"acme/widgets" = "~/src/widgets"
```

| Key | Default | Notes |
|---|---|---|
| `command` | `[]` | The program and its arguments. Empty runs `$SHELL` (`/bin/sh` if unset; `%COMSPEC%` or `cmd.exe` on Windows). It runs in a pseudo-terminal with `TERM=xterm-256color` and `COLORTERM=truecolor`, plus `RB_SOURCE` (the source's id), `RB_REPO`, `RB_NUMBER` and `RB_URL` for the change the pane was opened for. The host terminal's own markers (`KITTY_WINDOW_ID`, `ITERM_SESSION_ID`, `WEZTERM_PANE` and similar) are removed, because the pane doesn't provide what they promise |
| `escape` | `"ctrl-\\"` | The chord you press, then `esc`, to give the keyboard back to the app. Written like `ctrl-]`, `alt-x` or `ctrl-alt-space`; it must include `ctrl` or `alt` (or be a function key such as `f12`) so it can never swallow ordinary typing. A value that doesn't parse falls back to the default and the footer says so. `esc esc` (two presses in quick succession) always works too, for terminals and multiplexers that swallow the chord; the first `esc` still reaches the child, so editors keep working |
| `position` | `"auto"` | Where the pane sits. `auto` puts it beside the app at 150 columns or more and below it otherwise. A side placement needs 94 columns and a stacked one 20 rows; when there is no room the pane steps aside (it keeps running) and the footer says so. The chord then `p` cycles it, and it is remembered between runs |
| `size` | automatic | Columns (beside) or rows (stacked), or a percentage; automatic is 40 percent. The app keeps at least 70 columns or 14 rows, and the pane never goes below 24 columns or 6 rows. Drag the seam, or press the chord then `<` or `>`; a double-click on the seam or the chord then `=` goes back to automatic. Remembered between runs |
| `scrollback` | `10000` | Lines of history kept for the scrollback viewport |
| `start` | `"ask"` | `ask` shows a preview with a managed worktree and the current directory as the choices, and **No** is the default. `worktree` offers only the worktree. `current` starts in the directory Review Buddy was started in without asking, since nothing is created |
| `checkouts` | _none_ | Local clones to fetch into, by `owner/name` (GitLab project path). The directory Review Buddy was started in is tried first. `~` is expanded. Without a clone the prompt says so and offers the current directory |

**Managed worktrees.** After you confirm, Review Buddy runs `git fetch <remote> <ref>` in your clone (`pull/N/head` on GitHub, `merge-requests/N/head` on GitLab) and `git worktree add --detach <path> FETCH_HEAD`. The path is `worktrees/<source>/<owner>__<repo>/<number>` in the state directory (`$XDG_STATE_HOME/review-buddy/` on Unix). Your own working copy is left alone. If the worktree is already there it is reused as it is, with nothing fetched. Review Buddy never removes a worktree on its own: the preview ends with the exact command to do it yourself, `git -C <your clone> worktree remove <path>`. Nothing is written in `--demo`.

### Remembered layout

Review Buddy keeps the choices you make with `P`, `S`, `p`, `B`, `z` (soft wrap in the diff), the terminal pane's placement and size, and by dragging or nudging the panes in `$XDG_STATE_HOME/review-buddy/session.toml` (`%LOCALAPPDATA%\review-buddy\state\` on Windows), and picks them up at the next launch. Only what you changed is stored, so a setting you never touched keeps following `config.toml`. The file is written in the background shortly after a change (about half a second) and once more on quit, and only when its content changed. It is created with mode `0600` in the private state directory, written atomically, and never touched in `--demo`. A file that is empty, corrupt or has unknown keys is ignored quietly, and each key that does parse is still used.

```toml
[layout]
detail_position = "left"   # auto | right | left | top | bottom
sources = "top"            # auto | left | top
detail = "closed"          # auto | open | closed
queue_width = 52           # columns, or a percentage like "60%"
queue_height = "55%"
sources_width = 30
background = "yes"         # theme | yes | no
terminal_position = "right"  # the terminal pane: auto | right | left | top | bottom
terminal_size = "40%"
diff_wrap = true           # soft wrap in the diff (z)
```

Precedence at launch, strongest first: a key you press this session, the `REVIEW_BUDDY_*` environment variable (`REVIEW_BUDDY_DETAIL_POSITION`, `REVIEW_BUDDY_BACKGROUND`), the remembered `session.toml`, `config.toml`, then the built-in default. Resetting a split with `=` forgets that size, so the next launch uses `ui.queue_width` or `ui.queue_height` again. `W` still writes sizes to `config.toml`. Set `ui.remember_layout = false` to turn remembering off, in which case the file is neither read nor written. `review-buddy config paths` lists the file and `review-buddy config reset-layout` removes it.

### Review drafts on disk

With `ui.drafts = "local"` (the default) the comments you add in the diff, your review summary and the verdict you chose are kept per change and saved to `$XDG_STATE_HOME/review-buddy/drafts/` (`%LOCALAPPDATA%\review-buddy\state\drafts\` on Windows). Each change has one JSON file named from a stable hash of the source id, repository and number, so no repository name or title shows in a directory listing. The file holds the comments (path, side, `line`, `start_line`, body), the summary, the verdict, the head commit the comments were written against, the change's title and when it was written. It is written atomically with mode `0600` in a `0700` folder, a moment (about half a second) after each change and once more on quit, and only when its content changed. It is deleted after a successful submit, when you discard the draft and when you delete its last comment. A file that is corrupt or from a version this one doesn't understand is ignored quietly. Drafts for changes that no longer appear in any source are kept until you discard them; `review-buddy drafts clear` removes them all. Tokens never go in these files.

A **draft** is your own unsent text on this computer. It is not the pending review a forge may hold for you; that one stays on the forge (see [Review drafts](keybindings.md#review-drafts)). `ui.drafts = "off"` keeps drafts in memory for the session only: nothing is read from or written to the drafts folder. Demo mode is always memory only. `review-buddy config paths` lists the folder.

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

Applied: `tab_width` (1–16) and `wrap`. The diff is always unified, with syntax highlighting on; the other options arrive with side by side in v0.3.

| Key | Default | Notes |
|---|---|---|
| `view` | `"unified"` | `unified` · `side-by-side` |
| `auto_side_by_side` | `true` | Use side by side above `side_by_side_min_cols` |
| `side_by_side_min_cols` | `160` | |
| `context_lines` | `3` | |
| `ignore_whitespace` | `false` | Toggle with `w` |
| `syntax_highlight` | `true` | |
| `tab_width` | `4` | |
| `wrap` | `false` | Soft-wrap long lines in the diff instead of cutting them at the pane edge (see [Soft wrap](keybindings.md#soft-wrap)). `z` toggles it in the diff, and the choice is remembered between runs (see [Remembered layout](#remembered-layout)); the key never edits the file |

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

The dashboard and the commands share one implementation: `noise_authors` and `stale_after` feed the built-in rules, `show` filters the queue by role and drafts, and `bucket_limit` cuts each bucket with a `+N more` row. Under `--demo` the defaults apply. The dashboard starts from the configured `show` and `s` opens a control to change it for the session; those changes aren't written back to `config.toml`. `review-buddy queue --show …` replaces `show` for one run. Each source's `hide_repos` leaves projects out of the dashboard, `queue` and `pr list` in the same way; `--repo` lists one project whatever `hide_repos` says, and the `queue` end note says how many it hid. Items older than `stale_after` that would have been Waiting on you or Worth a look drop to Can wait, measured against the queue's own clock. Headings and the Sources counts reflect what the Show filters let through, and the queue's end note says how many they hide.

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

Settings → Review will show the active rules in order and which rule bucketed the selected change (`bucketed by rule 3 · repo platform/infra, path **/*.tf`), and `review-buddy triage explain <url>` will print the same from the command line. Neither exists yet; both are planned for v0.4.0.

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
| `hide_repos` | no | Projects the queue leaves out, as a list of `owner/repo` (or GitLab project paths such as `platform/infra/terraform`). An entry may end in `*` to cover every project with that prefix, like `"owner/legacy-*"`. Case doesn't matter. `*` anywhere else, spaces and web addresses are config errors that name the source and the key. Applies at launch to the dashboard and to `queue` and `pr list`; `--repo` still lists a hidden project. The Show filters (`s`) change it for the session, and `w` there saves it back through the same write path as Settings, keeping your comments. A source defined outside the write target keeps its own `hide_repos`; edit it where it lives |
| `enabled` | no | Default `true` |

## `[keys]`

Parsed but not applied yet. See `keybindings.md`.

## Environment variables

| Var | Effect |
|---|---|
| `REVIEW_BUDDY_CONFIG` | Path to an alternate config file (replaces the user config layers) |
| `XDG_CONFIG_HOME`, `XDG_CONFIG_DIRS`, `XDG_DATA_HOME`, `XDG_DATA_DIRS`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`, `XDG_RUNTIME_DIR` | Standard base directories; see above |
| `REVIEW_BUDDY_THEME` | Overrides `ui.theme` for this run |
| `REVIEW_BUDDY_BACKGROUND` | `theme`, `yes` or `no` for this run: whether the frame paints a background. Beats the remembered layout, `ui.background` and `[ui.theme_background]`; `B` beats it. Other values are ignored |
| `REVIEW_BUDDY_COLOUR_DEPTH` | Forces the colour depth for this run: `truecolor`, `256` or `16` (overrides detection; `ui.colour_depth` in `config.toml` does the same persistently) |
| `REVIEW_BUDDY_IMAGES` | `auto`, `off`, `halfblocks` or `forge-only` for this run: how pictures in descriptions are drawn and fetched. Beats `ui.images`; other values are ignored |
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
review-buddy config paths|get|list|reset-layout   resolved directories, loaded files and values; forget the remembered layout
review-buddy drafts list|discard <selector>|clear  review drafts saved on this computer
review-buddy theme list|check <id>|export <id>
review-buddy doctor                   check auth, scopes, rate limits, API versions and XDG paths
review-buddy completion <shell>       print a shell completion script

global: --config <path> · -s/--source <name> · -R/--repo <owner/repo> · --json <fields> · -q/--jq <expr>
        -w/--web · --color auto|always|never · --no-color · -y/--yes
        --demo [--demo-scene <name>] [--frozen-time <iso>] [--jax-mood <mood>] [--size <COLSxROWS>]
```

These run: `queue`, `pr list|view|diff|checks|open` (also as `mr`), `open`, `auth status|login|logout|token`, `source list|test|add`, `config paths|get|list|reset-layout`, `drafts list|discard|clear`, `theme list`, `doctor` and `completion`. `triage explain` and `theme check|export` exit `2` with `Not built yet. It's planned for v0.4.0.`, and the write commands in the block above (`pr review|comment|merge|checkout|rerun`) aren't declared yet, so they are a usage error (exit `2`). `--demo-scene`, `--jax-mood` and `--size` are accepted but have no effect yet. `--setup` runs first run (see below).

### First run

With no config file in any location (and no sources), a TTY launch opens first run. `--setup` opens it again, and `--setup --plain` (alias `--no-tui`), or `--setup` without a terminal, runs the same flow as line prompts. Demo mode never shows first run and never touches your real config or keyring.

- **Where it writes.** The layered write target (`config paths` shows it): `--config` or `$REVIEW_BUDDY_CONFIG` if set, else the user `config.toml`. The file is rendered from `config.example.toml` so it keeps its comments, validated, then written atomically with mode `0600` after the confirm step.
- **Existing files.** Never overwritten without a confirmation that defaults to No. A confirmed replace keeps the old file as `config.toml.bak`, and keeps any `api_url` you had set for a host.
- **Tokens.** Reusing `gh`/`glab` writes `auth = "cli"`. A pasted token is tested, then stored in the OS keyring under `review-buddy/<host>` and the source gets `auth = "token"`. Tokens are never written to the config. If the keyring isn't available, the message suggests an `env:VAR` instead.
- **Scope.** One source per host, named after the host. Leave every organisation and "my repositories" unticked to include everything the account can see.
- **Plain exit codes.** `0` written and signed in, `1` couldn't write, `2` usage (for example `--demo`), `3` stopped without changes, `4` a new source couldn't sign in.

`pr` is also available as `mr`. Selectors accept a URL, `owner/repo#N`, `source:owner/repo!N`, a bare number with `--repo`, a branch, or nothing (the current branch).
