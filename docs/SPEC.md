# Review Buddy — product and technical spec

> Every pull and merge request, in one quiet queue.

Review Buddy is a terminal dashboard (Rust + [ratatui](https://ratatui.rs)) that brings GitHub pull requests and GitLab merge requests from any number of hosts into one place. You can read the diff, leave line comments and suggestions, approve, request changes and merge, all without leaving the terminal. It is Release Buddy's sibling, and the default look is Liminal HQ's Afterglow.

## Implementation status

This spec describes the whole product. **v0.1.0 is released and covers the v0.1 and v0.2 scope** (see §14): GitHub and GitLab, the three-pane dashboard, source aggregation, the unified diff with comments and approve, first run, Settings → Sources, the refresh engine, open and copy, the mouse, demo mode, four built-in themes and the command line. Each section below is the target design; this table says where the built code stops.

| Section | Built | Planned |
|---|---|---|
| §4.1 First run | Built (v0.2). A launch with no config file, and `--setup`, open the first-run screen: host detection from `gh`/`glab` sign-ins (`gh auth status`), git config `insteadOf` and shallow `~/src` remotes; accounts and GitHub organisations from `/user/orgs`; token reuse or inline entry saved to the keyring with a live test and scope hints; scope per host; look picker with live preview; Jax toggle; summary, then an atomic `0600` commented `config.toml`. `--setup --plain` (or no TTY) runs the same flow as prompts. Order differs slightly from the list below: tokens come before scope, because listing organisations needs a working token. Not yet: GitLab token checks and group listing (a pasted GitLab token is kept in the keyring but taken as it is, and a GitLab host is one source for everything the account can see; `source test` and Settings → Sources do check it), reading `hosts.yml` directly, a configurable scan root, and auto-detected sources without saving. After `esc` the empty queue points to `review-buddy --setup` | Live GitLab token check and group listing in first run |
| §4.2 Dashboard | 1a three panes, with the Overview, Files, Checks and Conversation tabs, the collapsed Noise row and "That's everything." The Show filters start from `triage.show`; `s` opens a control to change them for the session, and the Sources pane lists them. The same control has a Projects section (searchable, scrolling, grouped by source) to choose which projects the queue shows, starting from each source's `hide_repos`, with `w` (inside that control) saving the choice to config. `+N more` rows expand and collapse. Approve, Request changes and Comment start in the diff (Request changes explains itself where the source can't do it); Merge says it's planned for v0.3 | 1b and 1c layouts and `L` (v0.4), saving the kind Show filters to config (the project choice saves as `hide_repos`). A smaller step toward them is built: `ui.sources = auto/left/top` (`S`) puts Sources on the top strip at any width, `ui.detail = auto/open/closed` (`p`, or the `⟩` / `⟨ detail` markers) closes the Detail pane so the Queue takes the full width, and the strip shares its width between tabs so names are elided in the middle rather than cut. `ui.detail_position = auto/right/left/top/bottom` (`P` rotates it: auto, bottom, left, top, right (the list moves left, top, right, bottom); `REVIEW_BUDDY_DETAIL_POSITION`) puts Detail beside or above/below the Queue, `auto` choosing right at 110 columns or more and below otherwise. `P`, `S`, `p`, `B` and the dragged sizes are remembered between runs in the state directory's `session.toml` (`ui.remember_layout`, `config reset-layout`; never in demo mode), and none of them is saved to `config.toml` from those keys. Pane sizes: dragging the seam between Queue and Detail (or the Sources pane edge) with the mouse, double-clicking it to reset, `<` / `>` / `=` from the keyboard, `ui.queue_width` and `ui.queue_height` for the starting sizes and `W` to save the current ones |
| §4.3 Diff | Unified diff with syntax highlighting, soft line wrap (`z`), a header row naming the change under review (forge badge, `owner/repo#number`, source, title), files pane, line cursor, inline threads, hunk and file jumps, mouse cursor, drag, shift-click and keyboard (`⇧↑` `⇧↓`, `V`) range selection, with multi-line comments on a range, and text selection and copy by mouse (drag, double- and triple-click) and keyboard (`v`, `y`, `Y`), with `M` to hand the mouse back to the terminal | Side by side, resolve, viewed marks (v0.3) |
| §4.4 Composer | Comment and reply on a line, add to the pending review, post now with a preview, word-wise editing, discard confirm. Review drafts belong to the change: they are kept when you leave the diff (`Keep these 3 comments as a draft?`), autosaved to `$XDG_STATE_HOME/review-buddy/drafts/` (`ui.drafts = "local"`, the default; `off` is memory only, and demo is always memory only) and restored with their cursor position, flagging a moved head and keeping a comment whose line is gone as `outdated`. `D` on the dashboard lists every pending draft (`✎ N` marks queue rows), `drafts list/discard/clear` do the same from the shell, and pending comments (local, restored, or pending on the forge) can be edited with `e` and deleted with `d`, the forge ones through the provider | Suggestions and `⌃S`, `⌃E` (v0.3) |
| §4.8 Terminal pane | `t` opens a terminal beside the change (Dashboard and Diff), in its own crate `rb-term`: a real PTY (ConPTY on Windows) feeding an `alacritty_terminal` emulator, drawn with colours, attributes, wide characters, a cursor and a scrollback viewport. It starts in a managed worktree of the change's head under the state directory (created only after a preview and a confirm that defaults to No) or in the current directory, with `RB_SOURCE`, `RB_REPO`, `RB_NUMBER` and `RB_URL` set. The child's queries are answered, OSC 52 copies go through the clipboard path, bracketed paste and mouse reporting are forwarded, and the kitty keyboard protocol is used when both sides want it. One escape chord (default `⌃\`) then `esc` returns focus, with `esc esc` as a second way out; a click focuses the pane and Shift stays with the terminal. Placement (`auto`, `right`, `left`, `top`, `bottom`) and size reuse the pane layout and seams and are remembered. `[ui.terminal]` sets the command, chord, placement, size, history and start. Demo mode opens a scripted pane that spawns nothing | Windows ConPTY is built but untested here; images in the pane are dropped; focus-in and focus-out reports are not forwarded; worktree cleanup is manual by design |
| §4.5 Palette and search | No | v0.4 |
| §4.6 Merge confirm | No | v0.3 |
| §4.7 Settings | Sources only (v0.2): `,` opens Settings with a Sources table (name, kind, host, scope, sign-in, in All, last token check and expiry), `t` tests a token, `e` edits and `a` adds through a form (hosts found on the machine are suggested; a typed token is tested, hidden and kept only in the keyring), `space` switches a source on or off, `x`/`del` removes after a confirm that starts on No and says the keyring token stays unless you also choose to remove it. Writes go through `toml_edit` to the write target with comments kept. Sources defined in `config.d`, `$XDG_CONFIG_DIRS` or another `--config` file show their origin and are read-only (a later layer replaces the `[[source]]` list wholesale, so an override in the user file can't work); demo mode lists demo sources read-only | Review, Keys, Theme and Jax sections (v0.4) |
| §6 Actions | Submit a review in the diff with a verdict (`a` approve, `x` request changes with a required summary, `R` comment) and an optional summary, on GitHub and GitLab (Request changes where the capability probe allows it); open `o` and copy `y` everywhere | Merge, re-run, checkout, and `pr review` on the command line (v0.3) |
| §7 Feedback | Toasts (on state changes only) and footer status, last-refreshed time, the `offline · cached HH:MM` banner, errors with a next step, per-source failures keep cached rows | Offline action queue (1.0); banner times are UTC until local time lands |
| §8 Demo | `--demo`, `--frozen-time`, offline fixtures (all four sources, GitHub and GitLab, seven changes), writes labelled `(demo)` | `--demo-scene`, `--jax-mood` and `--size` are accepted but have no effect yet |
| §9 Jax | No | v0.4 |
| §10 Theming | Built-ins, `T`, `ui.theme`, colour depth, `NO_COLOR`, the painted background (`ui.background`, `[ui.theme_background]`, `[theme] paint_background`, `REVIEW_BUDDY_BACKGROUND`, `B`) | User theme files, hot reload (v0.4) |
| §11 Performance | Cached rows paint first; refresh engine: launch, `r`, `refresh.interval` and focus, per-source state, per-host concurrency cap, ETag/not-modified, backoff with jitter, rate-limit pause | Lazy huge diffs |
| §4.2a Images | Images in a description (`![alt](url)`, reference-style, and `<img>` tags with `width` and `height`) drawn inline in the Overview in sixel, kitty or iTerm2 where the terminal answers the capability query, else in halfblocks on a truecolour terminal, else a one-line note; `i` selects the next image and `o` / `y` open or copy it; `ui.images` and `REVIEW_BUDDY_IMAGES`; a disk cache; demo pictures | Images in Conversation comments (the latest comment shows a note), a larger view, animated GIFs (first frame only), SVG |
| §12 Accessibility | Glyphs plus colour, `NO_COLOR`, `ui.reduced_motion` (a still glyph for the refresh spinner) | `--no-unicode`, and reduced motion for Jax and cursors (nothing else animates yet) |
| §13 Platforms | All release targets built by the `Release` workflow | |

The interactive design lives alongside this file:

- `archive/design/Review Buddy Board.dc.html`: every layout and screen side by side (frames 1a–1m)
- `archive/design/ReviewBuddy.dc.html`: a single working prototype; click into it and use the keys

The prototype is kept for reference only. Where it and the shipped app disagree, the app and this spec win.

Supporting docs:

- `docs/keybindings.md`: the key map (what works today, and what is planned)
- `docs/theming.md`: theme file format, roles and the built-in themes
- `docs/configuration.md`: `config.toml` reference
- `docs/cli.md`: the `gh`-style command line (commands, selectors, output, exit codes)
- `docs/integrations.md`: GitHub and GitLab API mapping
- `docs/architecture.md`: crate layout, state model, rendering, caching
- `docs/release.md`: platforms, build targets, distribution, running unsigned builds, the release checklist
- `docs/testing.md`: running tests and reviewing snapshots
- `CHANGELOG.md`: what changed in each release
- `config.example.toml`, `themes/*.toml`: ready-to-copy files

---

## 1. Goals and non-goals

**Goals**

1. One queue for all review work across GitHub (github.com and Enterprise) and GitLab (gitlab.com and self-hosted).
2. Show sources one at a time **or** aggregated, with a single key to switch.
3. Review fully from the keyboard: diff, range comments, suggestions, approve, request changes, merge, re-run CI, check out locally, open in the browser.
4. Mouse support on par with the keyboard: click, shift-click, drag to select lines, click tabs and chips.
5. Themeable by the user. Liminal HQ is the default and is transparent, so it uses the terminal's own background.
6. Calm by default: pre-triaged buckets, pull-only refresh, no unread counts, a clear "you're caught up" state.
7. Scriptable without the TUI. With no arguments it opens the TUI; with a command it behaves like `gh` (`review-buddy pr list`, `pr view`, `pr diff --json …`), pipe-friendly and non-interactive, across every source. See `docs/cli.md`.

**Non-goals (v1)**

- Not a full Git client. Checkout shells out to `git`; there is no staging or committing.
- No editing of PR or MR metadata beyond review actions (titles, labels, milestones are out of scope).
- No push notifications or background daemon. Refresh happens while the app is open.
- No MCP server or agent API (unlike jira-tui's `jira-mcp`). Review Buddy is for people.
- No Bitbucket, Gitea or Forgejo in v1. The provider trait is designed so they can be added later (see `docs/architecture.md`).

## 2. Principles (from the Liminal HQ house style)

- **Local first.** Config, cache, state and drafts live on disk in the standard XDG base directories (`~/.config`, `~/.cache`, `~/.local/state`, `~/.local/share`, macOS included); tokens live in the OS keyring. The app works offline from the last cache.
- **Calm computing.** Buckets, not badges. Bots go to Noise. Every list ends with "That's everything." Destructive actions confirm and default to **No**.
- **Explain the why, kindly.** Errors say what happened and what to do next ("Token for gitlab.work.ca expired on 12 Jan. Press `e` to paste a new one.").
- **Canadian English** in all copy: colour, behaviour, prioritise, licence (noun).
- **Sentence case** for labels and actions; no all-caps anywhere. The wordmark is lowercase: `review buddy`.
- **Unicode affordances** (`→ ⏎ ⇧ ⌃ ↑↓`) and middots (`·`) for metadata.

## 3. Glossary

| Term | Meaning |
|---|---|
| Source | One authenticated scope on one host: an org, group, user or whole instance. e.g. `liminal-hq` on github.com, `platform` on gitlab.work.ca |
| All | The aggregated view across every source with `in_all = true` |
| Change | A GitHub pull request or GitLab merge request. The UI says "PR" generically; numbers keep their native prefix (`#214`, `!1182`) |
| Bucket | Triage group: **Waiting on you**, **Worth a look**, **Can wait**, plus hidden **Noise** |
| Review | Your pending set of comments and suggestions on one change, submitted together with an approve, request-changes or comment verdict |
| Suggestion | A comment containing a ` ```suggestion ` block that the author can apply in one click on the forge |

## 4. Screens

All screens share the **top bar** (wordmark · source tabs `1`–`5` · sync status and theme name) and the **footer** (context-sensitive key hints · transient status message · Jax mini dock where relevant).

Panes use rounded borders (`BorderType::Rounded`) with the title set into the top border. The **focused pane** draws its border and title in the theme's `accent`; other panes use `line` and `text_secondary`.

### 4.1 First run (frame 1i)

Shown when no `config.toml` is found in any XDG config location, or when it is launched with `--setup`.

1. **Connect your sources.** On start, detect:
   - `gh auth status` / `gh auth token` for each GitHub host in gh's `hosts.yml` (`$GH_CONFIG_DIR`, else `$XDG_CONFIG_HOME/gh/`)
   - `glab auth status` for each GitLab host in glab's config (`$GLAB_CONFIG_DIR`, else `$XDG_CONFIG_HOME/glab-cli/`)
   - Remote hosts in git config (`~/.gitconfig` and `$XDG_CONFIG_HOME/git/config`) `url.*.insteadOf` entries and in recently used repos under `~/src` (configurable) that have no credentials yet

   Each row reads `✓ GH github.com  signed in as smorris via gh · 2 orgs` or `○ GL gitlab.work.ca  found in ~/.gitconfig, no credentials yet  › add token`. Choosing a missing host expands an inline token field. Test the token with `⏎`; it is saved to the OS keyring under `review-buddy/<host>`. The field lists the required scopes (GitHub: `repo`, `read:org`; GitLab: `api`, `read_user`).
2. **Pick a look.** Liminal HQ / Afterglow Dark / Afterglow Light. Each shows swatches and changes the screen live (`← →`).
3. **A little company.** `[x] Keep Jax around` (`J`).

Keyboard focus has two zones: the step's list and the button row (`[ Back ]  [ Continue ⏎ ]  [ Skip for now · esc ]`). `tab` / `shift-tab` cycle list → Back → Continue → Skip, `← →` (or `h` `l`) walk the buttons on every step except the look step (where they change the theme and `tab` reaches the buttons) and while the token field is open, and `⏎` activates the focused button (Continue by default). The focused button is bracketed `[› Back ‹]` and reversed, so it reads without colour. Back is absent on the welcome step, and `⌫` or `b` goes back outside the token field. The footer reads `⏎ continue  ← → buttons  ⌫ back  space pick  esc skip for now`.

`⏎` writes the config and opens the queue. `esc` skips; the app then runs with whatever was auto-detected and shows a one-line hint in the footer.

### 4.2 Dashboard (three layouts)

The layout is a user setting (`ui.layout`), and `L` cycles it at runtime. All three share the same data and actions.

#### 1a · Three panes (`panes`, default)

| Pane | Width | Contents |
|---|---|---|
| Sources | 26 cols | All sources, then each source (dot in the source's tag colour, host line underneath). Below that, **Show** filters: `[x] reviewing`, `[x] assigned`, `[x] authored`, `[ ] drafts`. The last Show filter is `[ ] noise`, which mixes Noise items into their natural buckets |
| Queue | 48 cols | Bucket headings, then two-line rows: `CI glyph · title · age` / `GH|GL · repo#num · author · status tag`. Selected row: `sel` background and a 2-col `accent` left rule. After Can wait comes a collapsed **Noise** row (`Noise · 2 bot updates · ⏎ to expand`, muted); expanded, it lists them like any bucket. Ends with `── That's everything.` and a note (e.g. "1 hidden by your Show filters.") |
| Detail | rest | Title (bold, `text_bright`), `author wants branch → base`, `+adds −dels · N files · opened 2h ago`. Tabs: **Overview · Files · Checks · Conversation** (`[` `]`). Action chips: `a Approve`, `x Request changes`, `c Comment`, `⏎ Diff`, `m Merge`. Jax box bottom-right when enabled |

Detail tab contents:

- **Overview**: description (rendered markdown → styled text, with its images drawn in place where the terminal can, see [Images in descriptions](#images-in-descriptions)), Reviewers (`✓` approved, `◌` requested, `✎` commented or requested changes) and Checks side by side, the latest comment, and your review state.
- **Files**: changed files with `+/−` counts. `↑↓` picks, `⏎` opens that file in the diff.
- **Checks**: every check run or pipeline job with duration. `R` re-runs failed jobs, `o` opens the logs in the browser.
- **Conversation**: general and line comments in time order, each with its location (`menus.rs:43`). `c` replies, `⏎` jumps to the line in the diff.

##### Images in descriptions

The Overview draws the pictures a description refers to where they sit in the text, so the screenshot that explains a change is in front of you without leaving the terminal.

- **What counts.** Markdown images (`![alt](url)`, including reference-style `![alt][ref]`), and HTML `<img src alt width height>` tags, also inside links. GitHub `user-attachments` addresses are ordinary images. A relative address resolves against the change's page. Only `http` and `https` addresses are fetched; the first eight images of a description are drawn and a note counts the rest.
- **How they are drawn.** At startup Review Buddy asks the terminal once (with a short deadline) what it can do. Sixel, kitty and iTerm2 are used as the terminal reports them; Windows Terminal supports sixel and is detected the same way. Without a graphics protocol, a truecolour terminal gets halfblocks; any other terminal, `NO_COLOR`, `ui.images = "off"` and a terminal that doesn't answer on a colour-poor setup get a one-line note instead: `▣ image: alt text – can't be drawn in this terminal`. The word `image` and the alt text carry the meaning, never colour alone.
- **Size and scrolling.** An image keeps its aspect ratio and is never drawn larger than its natural size (or the `width` and `height` the tag asks for), at most 60 percent of the pane wide and 14 rows tall. It scrolls with the text, and a picture that is partly scrolled off is clipped row by row on every protocol.
- **Selecting.** `i` selects the next image (and then none after the last), scrolling it into view and marking its caption with `▸` and `(open with o)`. While one is selected, `o` opens it and `y` copies its address instead of the change's.
- **Fetching.** Images load in the background with a note while they do. Each is limited to 10 MB, 8192 pixels on a side and 40 million pixels, 20 seconds and four redirects, and is recognised by its first bytes: PNG, JPEG, GIF (first frame) and WebP. SVG is never drawn.
- **Privacy.** The source's token goes only to that source's own web and API hosts, over https, and is dropped on any redirect that leaves them. Other hosts are fetched anonymously, and `ui.images = "forge-only"` refuses them. See [Images](configuration.md#images) for what a remote image can tell its host.
- **Cache.** Fetched bytes are kept under the cache directory (100 MB at most, least recently used first), so reopening a change doesn't fetch again and cached images show offline. Demo mode draws its own generated pictures and never touches the network or the real cache.

#### 1b · List + diff (`split`)

- The left list (46%) has one-line rows: `CI · GH/GL · repo#num · title · age`. Sources are only in the top tabs.
- The right pane always shows the first changed file of the selected change, with a compact header. Threads collapse to `╰ 2 comments from tess, mira-k · ⏎ to expand`.

#### 1c · One at a time (`queue`)

- A strip across the top: bucket name, progress dots (`● current`, `○ pending`, `✓ reviewed`), `1 of 3`, then "then Worth a look, then Can wait" or "then you're caught up".
- Left: a context pane (description, a "Before you approve" list of checks and reviewers, actions `a Approve & next`, `x Request changes`, `n Skip for now`). Right: the diff.
- `a` approves and advances. `n` / `p` move next / previous.

### 4.3 Diff (frames 1d, 1e, 1f)

Opened with `⏎` (or `d`) from any dashboard.

- **Change header**: one row under the top bar, over both panes, always naming the change under review: the forge badge (`GH` or `GL`), the native reference (`owner/repo#214` or `group/project!1182`), the source label, the title, and `author · branch → base` when the whole line fits. The title is cut with an ellipsis before anything else gives way, and the footer hints and refresh time sit on their own row, so a long title can't push them off. It reads the same for GitHub and GitLab and relies on text, not colour. When the diff was opened by `review-buddy open <url>` before the queue knows the change, it shows the reference and `title not loaded yet`. Bodies shorter than eight rows leave the header out.
- **Files pane** (34 cols): the file tree with `+/−` counts, the current file marked `›`. The bottom block shows "Your review · pending": the count of pending comments and suggestions, files viewed, and `a approve with these · x request changes · R review`, with `Your review: …` showing the verdict you've submitted (`approved`, `changes requested`, `commented`) and `Choosing: …` while the review modal is open.
- **Diff pane**: the file path in the top border; `z` toggles soft wrap (below) and the bottom border says `wrap` while it is on; the view toggle `v unified │ side by side` top-right; the range status in the bottom border (`lines 43–47 selected · c comment · s suggest · esc clear`).

**Unified** columns: `cursor (2) · old no. (5) · new no. (5) · sign (2) · code`. Additions get an `added_bg` tint, deletions a `removed_bg` tint. Hunk headers use `cyan` on `raised`.

**Soft wrap** (`diff.wrap`, default off; `z`, remembered with the layout): off, a line wider than the code column is cut with `…`. On, it continues on the next screen rows under the code column. The cursor, ranges, navigation and the mouse stay on logical lines; a separate screen-row layer holds each line's height and drives scrolling, paging, revealing the cursor and mapping a click or drag back to a line. Continuation rows show a dim `↪` in the number columns, a blank cursor column, the sign and the tint. Wrapping is by display width and grapheme, so wide characters, combining marks and tab expansions never split. Threads and pending suggestions sit under the last row of their line. Resizing, and opening or closing the terminal pane, re-wraps and keeps the cursor line.

**Side by side**: two halves, each `no. (5) · sign (2) · code`, divided by a `line`-coloured rule. Runs of deletions and additions are zipped row by row; the shorter side is padded with empty rows. Threads and suggestions show as one-line markers on the right half.

**Syntax highlighting**: `syntect` with a theme generated from the active theme's syntax roles (keyword → `interactive`, string → `success`, type → `cyan`, comment → `muted`).

**Inline threads**: a bordered block indented under the line it belongs to, titled `thread · line 43`, showing author, age and body. `r` replies, `e` resolves.

**Pending suggestions**: a bordered block in `accent`, showing the `−` / `+` lines and a note.

**Line cursor and ranges**

- `↑↓` moves the cursor (`›` in the gutter).
- `⇧↑↓` (and `⇧PgUp`/`⇧PgDn`) extends a range from the cursor, the first press anchoring it; `V` toggles a line-wise selection in which plain moves extend it; `esc` clears; a plain move outside that mode clears it like a click. The footer counts the selected lines. `←`/`→` (like `]`/`[`) flip files, with a calm note at the first and last file.
- `⌃Z` suspends on Unix: the terminal is restored, the process stops, and on `fg` it re-enters, repaints and refreshes; on Windows it shows a status and does nothing.
- Mouse: click to place the cursor; **press and drag on the line numbers or sign** to select a range; shift-click to extend.
- **Copying text.** A drag that starts on code, a comment or thread, a Files path, or the Detail description selects text instead: it stays inside the block it started in, is highlighted per cell in a tint stronger than the cursor line (reversed under `NO_COLOR`), and on release the logical text (no gutter, border, padding or `-`/`+` marks; soft-wrapped rows joined; wide characters whole; rows joined by newlines with trailing spaces trimmed) is copied through OSC 52 and then the OS tool, with a `Copied N characters` toast that never shows the text. Double-click selects a word, triple-click a line; `esc` or a click elsewhere clears it. `y` copies the selection (otherwise the URL), `Y` the whole comment or thread under the cursor, `v` starts a keyboard copy mode (`hjkl`, arrows, `w` `b`, `0` `$`, `tab` between code and comments, `y`, `esc`). `M` turns mouse capture off and on for the session, shown in the footer. The draw records each region's rows as logical text next to the hit map, so nothing is scraped from the buffer.
- Lines in the range get the `sel` background, `▌` in the gutter and `accent` line numbers.
- A range maps to the forge's multi-line comment (GitHub `start_line` + `line`; GitLab `position.line_range`). Ranges can't cross hunks; selection clamps at the hunk edge.

**Wide terminals**: with `diff.auto_side_by_side = true`, side by side becomes the default above 160 columns.

### 4.4 Composer (frame 1f)

There is one composer for comments and suggestions, docked over the bottom of the body.

- Title: `Comment · menus.rs lines 43–47`, plus `· with suggestion` when the draft contains a suggestion block.
- `c` opens it empty. `s` opens it with a suggestion block already in.
- **`⌃S` inserts a ` ```suggestion ` block** prefilled with the selected lines (added and context lines; deleted lines left out), so you edit the replacement in place. You can insert it at any point in a draft.
- `⏎` adds to your pending review. `⌃⏎` posts at once as a standalone comment. `⇧⏎` inserts a newline. `esc` discards; it asks first if the draft is longer than one line.
- `r` replies to the thread on the cursor line; the footer and the thread block's bottom border show `r reply` when there is one.
- In the review modal, `tab` reaches Submit, `⌃P` and `⌥⏎` submit from anywhere, `⌃⏎` is advertised only where the terminal reports it, and a failed submit shows its reason inside the modal. See `docs/keybindings.md`.
- `⌃E` opens the draft in `$EDITOR`.
- Drafts autosave to `$XDG_STATE_HOME/review-buddy/drafts/`, one JSON file per change named from a hash of source, repository and number (so no path or title leaks), and come back when you return to the change. As built, the file holds the comments, summary, verdict and head SHA rather than markdown. See `docs/keybindings.md#review-drafts`.
- A local draft is not a pending review on the forge. Pending comments from either (a draft comment, one restored from disk, or a forge-side pending thread) are edited in place with `e` and removed with `d`; the forge ones go through the provider and ask first.

### 4.5 Command palette (frame 1g)

- `⌃K` opens **Commands**; `/` opens the same box as **Search all sources**.
- Commands are fuzzy-matched (`nucleo`), eight results at most, each with its direct key on the right. They cover every action in `docs/keybindings.md` plus `Source: …`, `Theme: …`, `Cycle layout`, `Toggle Jax` and `Settings`.
- Search matches title, repo, number and author across every source (cached data, then a live forge search if fewer than three local hits). `⏎` opens the result's diff.
- `↑↓` choose · `⏎` run · `esc` close.

### 4.6 Merge confirm (frame 1h)

- A modal with a `danger`-coloured border: `Merge spindle#214?`, then the method and target ("Squash and merge feat/titleset-menus into main, then delete the branch."), then any caveats ("1 suggestion is still pending. It will be posted first.").
- Buttons: `No, not yet` (selected by default) and `Merge`. `← → / tab` switch, `⏎` confirms, `esc` cancels.
- The merge is blocked, with an explanation, when required checks or approvals are missing. `--force` doesn't exist; open the change in the browser instead.

### 4.7 Settings (frames 1j, 1k)

`,` opens Settings. The nav on the left has: **Sources · Review · Keys · Theme · Jax**.

- **Sources**: a table (`enabled · GH/GL · name · host · auth · in All`) with the selected source's details below: host, sign-in method (CLI or token), masked token with `e` edit / `t` test and its expiry, scope (orgs or groups or `*`), include drafts, show in All, tag colour. `n` adds a source, `space` enables or disables, `del` removes (asks first, defaults to No).
- **Review**: merge method, confirm-before-merge, diff default, mark files viewed on open, refresh interval, Noise authors, checkout location.
- **Keys**: a read-only map with a pointer to `[keys]` in config.
- **Theme**: built-in and user themes with swatches; `↑↓` previews live; the user theme directory and an example are shown.
- **Jax**: show Jax, mood reactions, shift log.

Changes write at once to `$XDG_CONFIG_HOME/review-buddy/config.toml` (atomic write via a temp file + rename, comments preserved via `toml_edit`). Values that come from `$XDG_CONFIG_DIRS` or `config.d/` show their origin (e.g. `from config.d/10-work.toml`). In 0.2 Sources shows those read-only with an explanation instead of writing an override: `[[source]]` lists replace each other wholesale across layers, so an override in the user file would either be ignored (a drop-in wins) or hide the other list.

## 5. Data model and triage

```text
Source { id, kind: GitHub|GitLab, host, label, scope, auth: Cli|Token, in_all, include_drafts, tag_colour }
Change { source_id, kind, repo, number, title, author, state, draft, created_at, updated_at,
         branch, base, head_sha, base_sha, adds, dels, files, ci: Pass|Running|Fail|None,
         reviewers: [Reviewer{login, state}], my_role: Reviewing|Assigned|Authored|Mentioned,
         my_review: None|Approved|ChangesRequested|Commented, bucket }
```

**Bucket rules** (first match wins; all configurable in `[triage]`):

0. **Your rules first.** `[[triage.rule]]` entries are tried in order (global, or scoped with `source = …`). The first rule that matches sets the bucket. See `docs/configuration.md`.
1. Author is in `noise_authors`, or matches a bot pattern → **Noise** (a collapsed bucket at the end of the queue, plus the optional `noise` Show filter)
2. Your review is requested, or you are assigned and haven't reviewed since the last push → **Waiting on you**
3. You are mentioned, you have commented before, or you authored it and it has new activity → **Worth a look**
4. Everything else in scope (your own approved changes, drafts) → **Can wait**

Within a bucket, sort by `updated_at` descending, with CI failures on your own changes first. Lists are bounded: each bucket shows 20 rows and then `+N more · ⏎ to expand`.

**Show filters** narrow by `my_role`; `drafts` and `noise` are off by default. With `noise` ticked, Noise items appear in their natural bucket instead of the collapsed row. The queue's end note reports how many the filters hide, by kind and by project. The project filter (`[[source]] hide_repos`, or the Projects section of the control) stores what is hidden, so new projects are shown until you hide them.

## 6. Actions → forge mapping (summary)

| Action | Key | GitHub | GitLab |
|---|---|---|---|
| Approve | `a` | Submit review `event: APPROVE` | `POST …/merge_requests/:iid/approve`, then publish draft notes |
| Request changes | `x` | Submit review `event: REQUEST_CHANGES` (needs a body) | Publish draft notes, then set the reviewer state to "requested changes". **Probed on connect** (GitLab 17.3 or newer on an enterprise build): on instances without it, `x` is hidden (chips and the review block) and pressing it says "GitLab 16.9 on gitlab.work.ca doesn't support requesting changes (it needs 17.3 or newer). Approve or comment instead." |
| Comment | `c` | Pending review comment (`line`, `side`, `start_line`) | Draft note with `position` |
| Suggest | `s` / `⌃S` | Same, body contains a ` ```suggestion ` block | Same, body contains ` ```suggestion:-N+M ` |
| Merge | `m` | `PUT /pulls/:n/merge` with `merge_method` | `PUT …/merge_requests/:iid/merge` with `squash`, `should_remove_source_branch` |
| Re-run CI | `R` | `POST /actions/runs/:id/rerun-failed-jobs` | `POST /projects/:id/pipelines/:pid/retry` |
| Check out | `b` | `git fetch origin pull/N/head:<branch>` | `git fetch origin merge-requests/IID/head:<branch>` |
| Open | `o` | `html_url` via `open` / `xdg-open` | `web_url` |

Full endpoint details, pagination, rate limits and error mapping are in `docs/integrations.md`.

## 7. Feedback and states

- **Status messages** sit at the right of the footer for 4 s: `✓ Approved spindle#214`, `↻ Re-running failed jobs on flow#88`, `⎇ Checked out feat/titleset-menus in ~/src/spindle`.
- **Errors** use the same slot in `warning` (never a full-screen alarm), with a fix: `Couldn't reach gitlab.work.ca (timed out). Showing cached data from 10:42 · r to retry`.
- **Offline**: the top bar reads `offline · cached 10:42`. Actions queue up locally and are sent on reconnect, after you confirm the list.
- **Empty states**:
  - No sources: "Nothing connected yet. Press , to add GitHub or GitLab."
  - Caught up: "That's everything. Nothing is waiting on you." with Jax napping.
  - Filtered to empty: "Your Show filters hide all N. Press space on a filter to widen it."
- **Loading**: per-pane `·  ·  ·` placeholders. Never a spinner over the whole screen.

## 8. Demo mode

`review-buddy --demo` runs fully offline on built-in fixtures. It is used for first impressions, screenshots, docs and release smoke tests.

- **Fixtures:** the same data as the design board: GitHub `liminal-hq` and `smorris`, GitLab `platform` (self-hosted) and `gitlab.com`, seven changes across every bucket, plus two Noise bots, the `menus.rs` diff with a thread and a pending suggestion, and checks in pass, running and fail states.
- **Actions animate but don't send:** approve, request changes, comment, merge, re-run, checkout and open all update local state, show their status message and trigger Jax's mood, then append `(demo)`. No network sockets are opened (the `live` feature's HTTP client isn't constructed), and checkout prints the git command it would have run.
- **Jax** is on by default in demo mode.
- **Screenshot-ready:**
  - `--demo-scene <name>` opens a specific frame (`panes`, `split`, `queue`, `diff`, `diff-split`, `suggest`, `palette`, `merge`, `firstrun`, `settings-sources`, `settings-theme`), matching board frames 1a–1m.
  - `--frozen-time 2026-10-05T10:00` pins relative ages.
  - `--jax-mood <mood>` and `REVIEW_BUDDY_SEED` make Jax deterministic.
  - `--size 160x40` sets the reported size when it is run under a recorder (vhs, asciinema).
- **Feature-gated:** fixtures live behind the `demo` Cargo feature (on by default). `cargo build --release --no-default-features --features live` drops them. Release builds keep `demo` on, because the smoke tests rely on it.
- Demo never reads or writes the user's config, cache or state. It uses a throwaway temp dir, so it's safe to run on any machine.

## 9. Jax

Jax is optional (`J` toggles; persisted). He appears in the 1a detail pane and the 1c context pane, and as a mini dock (`●‿● jax 🦦`) in the diff footer. He is never drawn over content or modals, and is hidden on first run and in Settings.

- Moods: 🎉 party for 4 s after approve or merge; 😰 alarm while the selected change has failing CI; otherwise chill, rotating every 6 s (🤓 reading the diff, slowly · 🎣 fishing for nits · 😴 napping until CI finishes · 👋 hi. a few things want you.).
- He blinks (`- ‿ -`) every ~4 s. Everything freezes when `ui.reduced_motion = true` or `REVIEW_BUDDY_REDUCED_MOTION=1`.
- The box title is `jax · {emoji}`.

## 10. Theming (summary)

Themes are TOML files of named colour roles. Built-ins: **Liminal HQ** (default, transparent background), **Afterglow Dark**, **Afterglow Light**. User themes go in `$XDG_CONFIG_HOME/review-buddy/themes/*.toml`, with installed packs under `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`; missing keys fall back to Liminal HQ. `T` cycles at runtime. Truecolour is used when `COLORTERM` is `truecolor`/`24bit` or the terminal is a known 24-bit one (Windows Terminal, iTerm2, kitty, WezTerm and others; see `docs/theming.md`); otherwise roles are quantised to the 256-colour palette, and below that to 16 named ANSI colours via each theme's `[ansi]` table. Full spec: `docs/theming.md`.

## 11. Performance and limits

- Cold start to the first painted queue from cache: < 150 ms. A full refresh across 5 sources: < 3 s on a typical connection (requests run in parallel per source, capped at 4 concurrent per host).
- Diffs over 3,000 lines render lazily by hunk; files over 1 MB or binary files show `binary or very large · o to open in browser`.
- Refresh is pull-only: on launch, on `r`, and every `refresh.interval` (default 5 min) while focused. ETags and `If-None-Match` keep it cheap.
- Minimum terminal size is 100×30. Below 130 cols, 1a collapses the Sources pane into the top tabs; below 100, a "make me a little wider" notice is shown.

## 12. Accessibility

- Every action has a key; every key action has a mouse equivalent.
- Colour is never the only signal: CI states use glyphs (`● ◐ ✕`), review states use `✓ ◌ ✎`, and diff lines keep `+`/`−` signs.
- All built-in themes keep text ≥ 4.5:1 against their background (Liminal HQ measured against `#050507`).
- `--no-unicode` swaps glyphs for ASCII (`*`, `~`, `x`, `>`) and rounded borders for plain ones.

## 13. Platforms

- **Linux is the primary platform**: designed, dogfooded and tested there first. Windows and macOS are supported release targets with CI coverage.
- **Architectures:** x86-64 and ARM64 on every OS. **macOS ships one universal2 binary** (no per-arch builds). **Linux:** tarballs, standalone binaries and `install.sh` use **static musl** builds (any distro); `.deb` / `.rpm` use glibc builds (≥ 2.35, built on Ubuntu 22.04).
- **Channels for 1.0:** GitHub Releases, `install.sh`, `install.ps1`, `.deb` / `.rpm`. Nothing else yet.
- **Repo:** `smorrisods/review-buddy`.
- **Release process:** the same as smorrisods/jira-tui (version-bump script → PR → tag → `Release` workflow → one `SHA256SUMS`), extended with Windows jobs and a `lipo` step.
- **No code signing.** macOS binaries carry only the ad-hoc signature Apple Silicon needs to run; Windows binaries are unsigned. Integrity comes from a single `SHA256SUMS`. `install.sh` / `install.ps1` verify it and avoid the Gatekeeper and SmartScreen prompts.
- Platform differences (keyring, clipboard, URL opening, terminal capability, key chords) are isolated in `rb-paths` and `rb-platform`. The full matrix is in `docs/release.md`.

## 14. Release plan

| Milestone | Scope |
|---|---|
| 0.1 (implemented; released as v0.1.0) | GitHub, 1a layout, unified diff, approve, comment, open in browser, the Liminal HQ theme. Read-only command line (`queue`, `pr list/view/diff/checks/open`, `auth status`, `--json`/`--jq`). Releases for every target from day one (Linux amd64/arm64 musl + glibc packages, macOS universal, Windows amd64/arm64), with `--demo` for smoke tests (Linux tested by hand; Windows and macOS smoke-tested in CI). Also built: `open`, `config paths`, `theme list`, `doctor`, shell completions and a man page. Declared for 0.1 but still not built (planned for 0.4, with triage rules and user themes): `triage explain`, `theme check` and `theme export` |
| 0.2 (implemented; released as v0.1.0, so there is no separate v0.2.0 tag) | GitLab (gitlab.com and self-hosted) listing, details, diffs, threads, pipelines and review writes; source aggregation with tag colours; Show filters and bucket limits; first-run detection and `--setup`; Settings → Sources; the refresh engine; the capability probe; GitHub Enterprise and self-hosted GitLab addressing with a fuller `doctor`; GitLab parity for the command line, `mr`, `auth login|logout|token`, `source test|add` and `config get|list` |
| 0.3 | Ranges, suggestions, side-by-side diff, merge, re-run CI, checkout, and the command line's `pr review`, `pr merge`, `pr checkout`, `pr rerun` |
| 0.4 | Layouts 1b and 1c, command palette and search, user themes, Jax, triage rules, demo scenes |
| 1.0 | Offline queueing (draft persistence was pulled forward to v0.2), `--no-unicode`; release channels per `docs/release.md` |

## 15. Decisions log

| Question | Decision |
|---|---|
| GitLab instances without request changes | Probe on connect (`GET /version`, token scopes), cached for 24 hours per host; hide `x` there and explain if it's pressed. See "Capability probe" in `docs/integrations.md` |
| Linux with no Secret Service | `auth = "cli"`, `env:VAR` or `token_command` is enough; no file store |
| Noise | A reachable collapsed bucket at the end of the queue **and** a `noise` Show filter |
| Per-source triage | Yes, as an ordered rule list with match conditions (see `docs/configuration.md`) |
| Demo mode | Yes, offline fixtures, feature-gated (`demo`), screenshot-ready |
| MCP server | No |
| Command line | `gh`-style: no arguments opens the TUI, any command is non-interactive and scriptable (`docs/cli.md`) |
| macOS artefacts | Universal only |
| musl | Ships in 1.0, for tarballs and `install.sh`; glibc for `.deb` / `.rpm` |
| Channels | GitHub Releases, `install.sh`, `install.ps1`, `.deb` / `.rpm` |
| GraphQL client | Hand-written queries with typed `serde` responses, not `graphql_client` (see `docs/architecture.md`) |
| Demo fixtures | Include GitHub and GitLab sources, so the dashboard and `--json` shapes can be explored with no network |
| Licence | MIT |
| Repo | `smorrisods/review-buddy` |

Licence: MIT.
