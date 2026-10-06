# Architecture

**Status (v0.1.0).** This page describes the target design. Where 0.1 differs it says so: `rb-gitlab` lists merge requests and loads their details, with diffs, threads and review writes still to come, `rb-store` holds the SQLite cache only (no drafts or offline queue yet), the app has two screens (Dashboard and Diff), and there is no tracing or log file yet.

## Workspace

```text
review-buddy/
├─ Cargo.toml                 workspace
├─ crates/
│  ├─ rb-core/                domain types, triage (the built-in rules; the [[triage.rule]] engine is planned), review drafts, the Provider trait
│  ├─ rb-github/              GitHub provider (hand-written GraphQL with typed serde structs + REST via reqwest)
│  ├─ rb-gitlab/              GitLab provider (REST v4). Sign-in, list and detail so far
│  ├─ rb-paths/               XDG resolution and dir creation with 0700 (config layering lives in the binary crate's `config/`)
│  ├─ rb-platform/            open URL, clipboard (OSC 52 first), keyring fallbacks, per-OS cfg
│  ├─ rb-store/               SQLite cache (rusqlite) and ETag store; drafts and the offline queue are planned
│  ├─ rb-theme/               built-in themes, role resolution, colour-depth quantisation
│  ├─ rb-diff/                patch parsing, hunk model, side-by-side pairing, syntect bridge
│  └─ review-buddy/           the binary: app state, event loop, ratatui widgets, and cmd/ (the gh-style command line)
├─ themes/                    built-in theme TOML (embedded with include_str!)
└─ docs/
```

## Key crates

`ratatui` (rendering) · `crossterm` (terminal, input, mouse, focus events) · `tokio` (async runtime) · `reqwest` + `rustls` · hand-written GraphQL with typed `serde` structs · `serde` / `toml` / `toml_edit` · `rusqlite` (bundled) · `keyring` · `syntect` · `nucleo` (fuzzy matching) · `pulldown-cmark` (markdown → spans) · `jaq` (the built-in jq behind `--jq`) · `clap`, `clap_complete` and `clap_mangen` (the command line, shell completions and the man page) · `insta` (snapshot tests of rendered buffers). XDG base directories are resolved by `rb-paths` itself on every Unix, macOS included (`%APPDATA%` on Windows), because crates such as `directories` map macOS to `~/Library`. Planned and not yet in the tree: `nucleo` (fuzzy matching for the palette) and `tracing` (logs to `$XDG_STATE_HOME/review-buddy/logs/`, never to the screen).

## Binary crate layout

The `review-buddy` crate is a library (`src/lib.rs`) with a thin `main.rs`, so integration tests and later features can reach the code. `cli.rs` is shared with `build.rs` through `include!`. The modules:

- `app/`: the `App` state, `Msg`, `Cmd`, `Action` and the pure `update`, split by concern (`dashboard`, `diff`, `diffview`, `composer`, `editor`, `queue`, `range`, `mouse`, `live`, `links`, `failure`).
- `ui/`: `draw(frame, &App) -> HitMap`: the top bar and footer hints (`chrome`, which also holds the key registry the footer and the help overlay share), the three-pane dashboard and detail pane, the diff and composer, the help overlay, the `HitMap`, layout and size helpers, and `ui::style`, the adapter from `rb-theme`'s framework-neutral colours onto ratatui. A terminal below 100×30 shows a "make me a little wider" notice.
- `runtime/`: the terminal modes (alternate screen, mouse, bracketed paste, focus events, restored on drop and from a panic hook), the tokio loop that selects over crossterm's `EventStream`, a tick and the `Cmd` results channel (redrawing only when the app is dirty, at most about 30 times a second), and `effects` that run `Cmd`s.
- `cmd/`: the `gh`-style command line, one module per command (`queue`, `pr_list`, `pr_view`, `pr_diff`, `pr_checks`, `auth`, `source`, `config`, `theme`, `doctor`, `completion`, `open`) plus shared plumbing (`context`, `selector`, `output`, `prompt`, `error` for exit codes, `git`, `markdown`, and `stub` for declared-but-unbuilt commands).
- `config/`: the layered `config.toml` (`schema`, `sources`, `edit`, `error`).
- `providers/`: the live backend that builds one `Provider` per source and refreshes them concurrently, with cached rows painted first.
- `demo/`: the offline fixtures, the `DemoProvider`, the frozen clock and the throwaway environment behind `--demo`.

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
}
```

`Capabilities` drives the UI: actions an instance can't do are hidden from the palette, and their key shows an explanation instead of failing.

## GraphQL typing

**Decision:** `rb-github` uses hand-written query strings with typed `serde` response structs behind a small `graphql::query<T>(client, query, variables)` helper, not `graphql_client`.

**Why:** `graphql_client` generates types from a vendored copy of GitHub's schema. That file is huge, goes stale, differs between github.com and each Enterprise version, and adds a code-generation step to every build. Review Buddy uses a handful of queries (the five `search` queries, PR detail, threads, and a few mutations), so each response struct only declares the fields we read. A mistyped field fails in a wiremock fixture test instead of at compile time, which is an acceptable trade for a much lighter build and no schema to keep in sync.

**What the helper does:** posts to `/graphql` (`/api/graphql` on Enterprise), reuses the shared auth, rate-limit tracking and error mapping, turns the `errors` array into forge-neutral errors (`NOT_FOUND`, `RATE_LIMITED`, `FORBIDDEN`), keeps partial errors next to usable data, and reads the `rateLimit { cost remaining resetAt }` block when a query asks for it.

**Revisiting:** if the query set grows past what is comfortable to maintain by hand, or Enterprise schema drift causes repeated breakage, switch to `graphql_client` with a trimmed schema (introspected and pruned to the types we use) checked in under `crates/rb-github/graphql/`. Only `graphql::query` callers change; the `Provider` surface and `rb-core` models don't.

## HTTP stack

`reqwest` is built with `default-features = false` and `rustls-tls` plus `gzip`, so there is no OpenSSL or native-tls anywhere in the tree and musl static builds stay possible. Check with `cargo tree -p rb-github | grep -i -E 'openssl|native-tls'`, which must print nothing.

## App state (Elm-style)

The struct below is the target shape. In 0.1 `Screen` is `Dashboard | Diff`, the dashboard layout is fixed at three panes, and the overlays are the help overlay, the composer and the approve/post/discard confirm; there is no palette, search, merge confirm or settings screen yet. First run adds `Screen::FirstRun`, `Msg::Setup`, `Cmd::Setup` and `Cmd::FinishSetup`; its flow is the pure state machine in `crates/review-buddy/src/setup/`. `Cmd`s today are `LoadChanges`, `LoadInfo`, `LoadDiff`, `SubmitReview`, `Reply`, `OpenUrl`, `Copy` and `After`.

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
2. Global keys (`?`, `o`, `y`, `T`, `q`, and `r` on the dashboard; `⌃K`, `/`, `,`, `L` and `J` are planned).
3. Focused-pane keys (`↑↓`, `⏎`, `← →`, `tab`).
4. Screen action keys (the dashboard and diff handlers; see `keybindings.md`).

Mouse events are hit-tested against the `Rect`s recorded during the last draw (`HitMap`). Line drag-select is `Down(Left)` → anchor, `Drag(Left)` → cursor, `Up(Left)` → finish (and clear if anchor == cursor). In 0.1 a range can be selected but only the cursor line is commented on.

## Command line

With a command, the binary skips the TUI and runs one module under `crates/review-buddy/src/cmd/`. It builds the same sources, providers (or `DemoProvider` under `--demo`), cache and theme as the TUI, then resolves a selector, calls `Provider` and formats the result as a table, tab-separated lines or `--json`. Selector parsing and output formatting are pure and unit tested. See `docs/cli.md`.

## Rendering notes

- Pane titles are drawn into the top border with `Block::title`. The focused pane uses `theme.accent` for the border and title.
- Diff lines are a custom `StatefulWidget` that renders only visible rows, from a pre-highlighted `Vec<StyledLine>` cache keyed by `(file, theme_id, tab_width)`.
- `transparent` background → no `bg` is set on any cell outside overlays. Overlays call `Clear` and paint `raised`.
- Alpha colours are pre-blended at theme load against `background`, or `raised` when it is transparent.
- Wide characters and emoji (Jax captions) are measured with `unicode-width`. Jax's box reserves two cells for the emoji.

## Persistence

In 0.1 only the config file and the SQLite cache exist. The drafts, offline queue, session and lock rows are planned.

| Store | Contents | Format |
|---|---|---|
| `$XDG_CONFIG_HOME/review-buddy/config.toml` (+ `config.d/`, `$XDG_CONFIG_DIRS`) | user settings, layered | TOML, edited with `toml_edit` |
| `$XDG_CACHE_HOME/review-buddy/cache.sqlite` | summaries, details, patches, threads, ETags | SQLite, WAL mode, versioned migrations; safe to delete |
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
- `#[ignore]`d live tests run only against a throwaway GitHub repository you name yourself (see `docs/integrations.md`); GitLab live tests arrive with GitLab.
