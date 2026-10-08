# Architecture

**Status.** This page describes the target design and marks where the code is behind it. `rb-github` and `rb-gitlab` both implement the `Provider` trait for listing, details, diffs, threads, checks and review writes, with merge, re-run and checkout still to come. `rb-store` holds the SQLite cache (changes, ETags and saved capability probes) but no drafts or offline queue yet. The app has four screens (Dashboard, Diff, Settings and FirstRun), and there is no tracing or log file yet.

## Workspace

```text
review-buddy/
├─ Cargo.toml                 workspace
├─ crates/
│  ├─ rb-core/                domain types, triage (the built-in rules; the [[triage.rule]] engine is planned), review drafts, the Provider trait and capability probe types
│  ├─ rb-github/              GitHub provider (hand-written GraphQL with typed serde structs + REST via reqwest)
│  ├─ rb-gitlab/              GitLab provider (REST v4): sign-in, lists, details, diffs, threads, pipeline jobs, draft-note review writes and the capability probe
│  ├─ rb-paths/               XDG resolution and dir creation with 0700 (config layering lives in the binary crate's `config/`)
│  ├─ rb-platform/            open URL, clipboard (OSC 52 first), keyring fallbacks, per-OS cfg
│  ├─ rb-store/               SQLite cache (rusqlite): changes, ETags and saved capability probes (`probes`); drafts and the offline queue are planned
│  ├─ rb-theme/               built-in themes, role resolution, colour-depth quantisation
│  ├─ rb-diff/                patch parsing, hunk model, side-by-side pairing, syntect bridge
│  ├─ rb-term/                the terminal pane: emulator boundary over alacritty_terminal, PTY over portable-pty, key and mouse encoders, focus chord, scripted demo pane, worktree planning, optional ratatui widget (no forge crates)
│  └─ review-buddy/           the binary: app state, event loop, ratatui widgets, and cmd/ (the gh-style command line)
├─ themes/                    built-in theme TOML (embedded with include_str!)
└─ docs/
```

## Key crates

`ratatui` (rendering) · `crossterm` (terminal, input, mouse, focus events) · `tokio` (async runtime) · `reqwest` + `rustls` · hand-written GraphQL with typed `serde` structs · `serde` / `toml` / `toml_edit` · `rusqlite` (bundled) · `keyring` · `syntect` · `nucleo` (fuzzy matching) · `pulldown-cmark` (markdown → spans, and finding images) · `ratatui-image` and `image` (pictures in descriptions: sixel, kitty, iTerm2 and halfblocks; PNG, JPEG, GIF and WebP decoding) · `jaq` (the built-in jq behind `--jq`) · `clap`, `clap_complete` and `clap_mangen` (the command line, shell completions and the man page) · `insta` (snapshot tests of rendered buffers). XDG base directories are resolved by `rb-paths` itself on every Unix, macOS included (`%APPDATA%` on Windows), because crates such as `directories` map macOS to `~/Library`. Planned and not yet in the tree: `nucleo` (fuzzy matching for the palette) and `tracing` (logs to `$XDG_STATE_HOME/review-buddy/logs/`, never to the screen).

## Binary crate layout

The `review-buddy` crate is a library (`src/lib.rs`) with a thin `main.rs`, so integration tests and later features can reach the code. `cli.rs` is shared with `build.rs` through `include!`. The modules:

- `app/`: the `App` state, `Msg`, `Cmd`, `Action` and the pure `update`, split by concern (`dashboard`, `diff`, `diffview`, `composer`, `editor`, `queue`, `range`, `mouse`, `live`, `links`, `failure`, `refresh`, `show`, `settings`, `setup`, `terminal`).
- `ui/`: `draw(frame, &App) -> HitMap` (including `terminal`, the pane's box, its seam geometry and the start prompt): the top bar and footer hints (`chrome`, which also holds the key registry the footer and the help overlay share), the three-pane dashboard and detail pane, the diff and composer, the Show filters control (`show`), Settings (`settings`), first run (`first_run`), the help overlay, the `HitMap`, layout and size helpers, and `ui::style`, the adapter from `rb-theme`'s framework-neutral colours onto ratatui. A terminal below 100×30 shows a "make me a little wider" notice.
- `runtime/`: `pane` runs the terminal pane's effects (the PTY, finding a clone, creating a worktree); the terminal modes (alternate screen, mouse, bracketed paste, focus events, restored on drop and from a panic hook), the tokio loop that selects over crossterm's `EventStream`, a tick and the `Cmd` results channel (redrawing only when the app is dirty, at most about 30 times a second), and `effects` that run `Cmd`s.
- `cmd/`: the `gh`-style command line, one module per command (`queue`, `pr_list`, `pr_view`, `pr_diff`, `pr_checks`, `auth*`, `source*`, `config*`, `theme`, `doctor`, `completion`, `open`, `probe`) plus shared plumbing (`context`, `selector`, `output`, `prompt`, `error` for exit codes, `git`, `markdown`, and `stub` for declared-but-unbuilt commands).
- `config/`: the layered `config.toml` (`schema`, `sources`, `edit`, `error`).
- `setup/`: first run as a pure state machine (`flow`) with its effects (`effects`: detection, token checks, the config write), host and account detection (`detect`), the rendered config (`write`, `source`) and the line-based `plain` mode behind `--setup --plain`.
- `settings/`: Settings → Sources as a state machine (`state`), its config edits through `toml_edit` (`edit`) and its effects (`effects`: read the config, test a token, write a change).
- `providers/`: the live backend (`live`) that builds one `Provider` per source, the refresh engine (`refresh`: per-source state, ETag short-circuit, backoff, per-host concurrency caps and rate-limit pauses) and the capability probe (`probe`: run once per source per session and cached for 24 hours), with cached rows painted first.
- `images/`: pictures in descriptions. `extract` finds Markdown and `<img>` images and splits the text around them; `hosts` decides which hosts are a source's own and where its token may go; `fetch` is the limited fetcher (manual redirects, size and pixel caps, magic-byte sniffing) and decoder; `cache` is the hashed, size-bounded disk cache; `layout` is the sizing maths; `detect` asks the terminal what it can draw, once, with a deadline; `state` holds each address's progress and the encoded pictures. `app/images.rs` asks for pictures once a description loads (`Cmd::FetchImage`, answered by `Msg::ImageLoaded`) and handles `i`; `ui/detail.rs` reserves rows for each picture and draws it, clipped, after the text.
- `demo/`: the offline fixtures, the `DemoProvider`, the frozen clock and the throwaway environment behind `--demo`.

## Terminal pane

`t` opens a terminal next to the change, so `claude`, `opencode`, `lazygit`, `vim` or a plain shell can run beside the diff. It follows the same Elm shape as everything else, with the system on the far side of a `Cmd`/`Msg` boundary:

```text
keys, paste, mouse ──▶ update ──▶ Cmd::Term(Write | Resize | Spawn | Plan | CreateWorktree | Close | Copy)
                         ▲                                   │
                         └── Msg::Term(Output | Exited | Planned | WorktreeReady | SpawnFailed) ◀── runtime/pane.rs
```

- **`rb-term` holds the logic.** `Emulator` is the only code that names `alacritty_terminal` (pinned with `=0.26.0`, because its API shifts between releases): bytes in, a `Screen` snapshot and a few `Event`s out, with the child's queries (device attributes, status and size reports, colour queries, the kitty keyboard query) answered as bytes to write back. `Pty` wraps `portable-pty` (ConPTY on Windows) and calls back with output and exit from helper threads. `Pane` is an emulator plus, in demo mode, a `Script`. `encode_key`, `encode_mouse` and `encode_paste` are pure tables. `Chord` and `EscapeState` are the pure focus model. `worktree` plans a checkout and previews it before anything runs. The ratatui `TerminalWidget` is behind the `widget` feature and is the only module that touches ratatui.
- **The `App` owns the emulator, the runtime owns the PTY.** Output arrives as `Msg::Term(Output)` and is fed to the emulator inside `update` (parsing, no I/O); whatever the child expects back leaves as `Cmd::Term(Write)`. Each child has a generation number, so a closed pane's last bytes are ignored. Keys while the pane has focus are encoded and sent as `Write`, with the child's own modes applied (application cursor keys, bracketed paste, mouse reporting, the kitty keyboard flags).
- **Keys.** The kitty encoding is used only when the child switched it on and the host reported that it can tell the keys apart (`App::kitty_keys`, answered by the terminal's keyboard-enhancement query at launch, or `REVIEW_BUDDY_KITTY_KEYS`); otherwise the legacy encoding is used. Enter and Shift-Enter both send a carriage return in the legacy encoding, which is also what a Windows console delivers.
- **Placement** reuses the dashboard's `DetailPosition` values and `Size`, and the pane is a dock around the body of the Dashboard and Diff screens: `app::terminal::app_body` is the body less the pane, so the screens' own layout, hit-testing and viewport maths shrink with it. The seam is draggable like the others, and the placement and size are remembered in `session.toml`.
- **Demo mode** opens a scripted, in-process pane. Nothing in the demo path reaches the runtime, and the runtime ignores every terminal effect when the backend is demo as a second line of defence.
- **Not supported.** Images in the pane (sixel, kitty graphics, iTerm2) are dropped. OSC 52 reads are never answered; writes go to the clipboard through `rb-platform`. Focus-in and focus-out reports are not forwarded to the child yet.

## Provider trait

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn kind(&self) -> ForgeKind;
    async fn whoami(&self) -> Result<User>;
    async fn list_changes(&self, scope: &Scope, since: Option<Etag>) -> Result<Page<ChangeSummary>>;
    async fn change_detail(&self, id: &ChangeId) -> Result<ChangeDetail>;
    async fn files(&self, id: &ChangeId) -> Result<Vec<FilePatch>>;
    async fn threads(&self, id: &ChangeId) -> Result<Vec<Thread>>;
    async fn checks(&self, id: &ChangeId) -> Result<Vec<Check>>;
    async fn submit_review(&self, id: &ChangeId, review: &ReviewDraft, verdict: Verdict) -> Result<()>;
    async fn reply(&self, thread: &ThreadId, body: &str) -> Result<Comment>;
    async fn resolve(&self, thread: &ThreadId, resolved: bool) -> Result<()>;
    async fn merge(&self, id: &ChangeId, opts: &MergeOpts) -> Result<MergeOutcome>;
    async fn rerun_failed(&self, id: &ChangeId) -> Result<()>;
    fn checkout_refspec(&self, id: &ChangeId) -> String;
    fn web_url(&self, id: &ChangeId) -> Url;
    fn capabilities(&self) -> Capabilities; // e.g. request_changes, viewed_files, range_comments
    async fn probe(&self) -> Result<ProbeOutcome>; // what this instance can do; defaults to the static capabilities
}
```

`Capabilities` drives the UI: actions an instance can't do are hidden from the chips and the review block, and their key shows an explanation instead of failing. `probe()` asks the instance on connect (GitLab's version and the token's scopes, GitHub's token scopes) and returns a `ProbeOutcome`: the capabilities, the version when there is one, whether the instance answered and a reason for each action that's off. It never fails because a version check did. The binary caches the outcome in `rb-store` per forge and host.

## GraphQL typing

**Decision:** `rb-github` uses hand-written query strings with typed `serde` response structs behind a small `graphql::query<T>(client, query, variables)` helper, not `graphql_client`.

**Why:** `graphql_client` generates types from a vendored copy of GitHub's schema. That file is huge, goes stale, differs between github.com and each Enterprise version, and adds a code-generation step to every build. Review Buddy uses a handful of queries (the five `search` queries, PR detail, threads, and a few mutations), so each response struct only declares the fields we read. A mistyped field fails in a wiremock fixture test instead of at compile time, which is an acceptable trade for a much lighter build and no schema to keep in sync.

**What the helper does:** posts to `/graphql` (`/api/graphql` on Enterprise), reuses the shared auth, rate-limit tracking and error mapping, turns the `errors` array into forge-neutral errors (`NOT_FOUND`, `RATE_LIMITED`, `FORBIDDEN`), keeps partial errors next to usable data, and reads the `rateLimit { cost remaining resetAt }` block when a query asks for it.

**Revisiting:** if the query set grows past what is comfortable to maintain by hand, or Enterprise schema drift causes repeated breakage, switch to `graphql_client` with a trimmed schema (introspected and pruned to the types we use) checked in under `crates/rb-github/graphql/`. Only `graphql::query` callers change; the `Provider` surface and `rb-core` models don't.

## HTTP stack

`reqwest` is built with `default-features = false` and `rustls-tls` plus `gzip`, so there is no OpenSSL or native-tls anywhere in the tree and musl static builds stay possible. Check with `cargo tree -p rb-github | grep -i -E 'openssl|native-tls'`, which must print nothing.

## App state (Elm-style)

The struct below is the target shape. Today `Screen` is `Dashboard | Diff | FirstRun | Settings`, the dashboard layout is fixed at three panes, and the overlays are the help overlay, the Show filters control, the composer and the approve/post/discard confirm; there is no palette, search or merge confirm yet. First run is `Screen::FirstRun` with `Msg::Setup` and `Cmd::Setup`, driven by the pure state machine in `crates/review-buddy/src/setup/`; Settings → Sources is `Screen::Settings` with `Cmd::Settings`, driven by `crates/review-buddy/src/settings/`. `Cmd`s today are `LoadChanges`, `LoadChangesNow` (`r`), `LoadChangesOnFocus`, `LoadInfo`, `LoadDiff`, `SubmitReview`, `Reply`, `Setup`, `Settings`, `FinishSetup`, `OpenUrl`, `Copy` and `After`.

```rust
struct App {
    screen: Screen,                 // FirstRun | Dashboard | Diff | Settings
    layout: Layout,                 // Panes | Split | Queue
    focus: HashMap<ScreenKey, PaneId>,
    source: SourceFilter,           // All | One(SourceId)
    show: ShowFilters,              // reviewing, assigned, authored, drafts
    selected: Option<ChangeId>,
    detail_tab: DetailTab,          // Overview | Files | Checks | Conversation
    diff: DiffState,                // file idx, cursor, anchor, view, scroll, collapsed threads
    overlay: Option<Overlay>,       // Palette | Search | Composer | ConfirmMerge | Help
    drafts: DraftStore,
    toast: Option<Toast>,
    jax: JaxState,
    theme: ResolvedTheme,
    net: NetStatus,                 // per source: ok | refreshing | offline(since) | error
}
```

- `update(&mut App, Msg) -> Vec<Cmd>` is pure and unit tested. `Msg` comes from input (`Key`, `Mouse`, `Resize`, `FocusGained`), timers (`Tick`, `RefreshDue`) and network results.
- `Cmd`s are async jobs run on tokio (`Fetch`, `Submit`, `Merge`, `Checkout`, `OpenUrl`, `WriteConfig`). Results come back as `Msg` over an `mpsc` channel.
- The render loop draws at most 30 fps, and only when something is dirty. Jax ticks at 6 s and the blink at ~4 s; both are off under reduced motion.

### Input routing

1. An open overlay gets the key first (palette, then confirm, then composer).
2. Global keys (`?`, `o`, `y`, `T`, `q`, and `r` and `,` on the dashboard; `⌃K`, `/`, `L` and `J` are planned).
3. Focused-pane keys (`↑↓`, `⏎`, `← →`, `tab`).
4. Screen action keys (the dashboard and diff handlers; see `keybindings.md`).

Mouse events are hit-tested against the `Rect`s recorded during the last draw (`HitMap`). Line drag-select is `Down(Left)` → anchor, `Drag(Left)` → cursor, `Up(Left)` → finish (and clear if anchor == cursor). A range can be selected and commented on; ranges from the keyboard are planned.

## Command line

With a command, the binary skips the TUI and runs one module under `crates/review-buddy/src/cmd/`. It builds the same sources, providers (or `DemoProvider` under `--demo`), cache and theme as the TUI, then resolves a selector, calls `Provider` and formats the result as a table, tab-separated lines or `--json`. Selector parsing and output formatting are pure and unit tested. See `docs/cli.md`.

## Rendering notes

- Pane titles are drawn into the top border with `Block::title`. The focused pane uses `theme.accent` for the border and title.
- Diff lines are a custom `StatefulWidget` that renders only visible rows, from a pre-highlighted `Vec<StyledLine>` cache keyed by `(file, theme_id, tab_width)`.
- `transparent` background → no `bg` is set on any cell outside overlays. Overlays call `Clear` and paint `raised`.
- Alpha colours are pre-blended at theme load against `background`, or `raised` when it is transparent.
- Wide characters and emoji (Jax captions) are measured with `unicode-width`. Jax's box reserves two cells for the emoji.

## Persistence

Today only the config file and the SQLite cache exist (migrations create `changes`, `etags` and `probes`). The drafts, offline queue, session and lock rows are planned.

| Store | Contents | Format |
|---|---|---|
| `$XDG_CONFIG_HOME/review-buddy/config.toml` (+ `config.d/`, `$XDG_CONFIG_DIRS`) | user settings, layered | TOML, edited with `toml_edit` |
| `$XDG_CACHE_HOME/review-buddy/cache.sqlite` | summaries, details, ETags, capability probes | SQLite, WAL mode, versioned migrations; safe to delete |
| `$XDG_STATE_HOME/review-buddy/drafts/` | composer text per change | Markdown files, so you can recover them by hand |
| `$XDG_STATE_HOME/review-buddy/queue.jsonl` | actions made while offline | JSON lines, replayed after you confirm |
| `$XDG_STATE_HOME/review-buddy/session.toml` | last source, selection, layout, diff view | TOML |
| `$XDG_RUNTIME_DIR/review-buddy/instance.lock` | single-writer lock for the cache | flock |
| keyring | tokens | OS keyring |

## Testing

- `rb-core`: triage rules and the draft → API payload mapping, tested against fixtures.
- Providers: recorded HTTP fixtures (`wiremock`) for each endpoint in `integrations.md`, including 304, 401, 403-SSO, 409 and rate-limit cases.
- UI: `insta` snapshots of `TestBackend` buffers for the canonical frames (1a three panes, 1d diff, 1f composer) in three themes, at 160×40 and 100×30, plus per-screen snapshots; the other board frames are snapshotted as they are built. See `docs/testing.md` for running, reviewing and accepting snapshots.
- Cargo features: `live` (HTTP providers, keyring) and `demo` (fixtures, `--demo`), both on by default. `--no-default-features` builds the core with neither and must stay warning-free (the CI clippy and test matrix: default, no-default-features, all-features, as in jira-tui).
- Demo fixtures live in `crates/review-buddy/src/demo/` as TOML plus patch files. A `DemoProvider` implements `Provider`, so the UI code path is identical; write calls mutate in-memory state only.
- CI runs the tests on Linux x64 (ubuntu-24.04) across the three feature sets, plus a default-features run on Windows (windows-2022) and macOS ARM64 (macos-14); release builds cover Linux amd64/arm64, macOS universal and Windows amd64/arm64 (see `release.md`).
- CLI: `assert_cmd` runs every command under `--demo --frozen-time` with temp `XDG_*` dirs, piped and with `--json`, pinned with `insta` (see `docs/cli.md`).
- `#[ignore]`d live tests run only against a throwaway GitHub repository or GitLab project you name yourself (see `docs/integrations.md`).
