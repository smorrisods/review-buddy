# Architecture

## Workspace

```text
review-buddy/
├─ Cargo.toml                 workspace
├─ crates/
│  ├─ rb-core/                domain types, triage (built-ins + [[triage.rule]] engine), review drafts, the Provider trait
│  ├─ rb-github/              GitHub provider (GraphQL via graphql_client + REST via reqwest)
│  ├─ rb-gitlab/              GitLab provider (REST v4)
│  ├─ rb-paths/               XDG resolution, config layering, dir creation with 0700
│  ├─ rb-platform/            open URL, clipboard (OSC 52 first), keyring fallbacks, per-OS cfg
│  ├─ rb-store/               SQLite cache (rusqlite), ETag store, drafts, offline queue
│  ├─ rb-theme/               theme loading, role resolution, colour-depth quantisation
│  ├─ rb-diff/                patch parsing, hunk model, side-by-side pairing, syntect bridge
│  └─ review-buddy/           the binary: app state, event loop, ratatui widgets, and cmd/ (the gh-style command line)
├─ themes/                    built-in theme TOML (embedded with include_str!)
└─ docs/
```

## Key crates

`ratatui` (rendering) · `crossterm` (terminal, input, mouse, focus events) · `tokio` (async runtime) · `reqwest` + `rustls` · hand-written GraphQL with typed `serde` structs · `serde` / `toml` / `toml_edit` · `rusqlite` (bundled) · `keyring` · `syntect` · `nucleo` (fuzzy matching) · `pulldown-cmark` (markdown → spans) · `etcetera` (XDG base directories on every Unix, macOS included; `%APPDATA%` on Windows. We don't use `directories`, because it maps macOS to `~/Library`) · `tracing` + `tracing-appender` (logs to `$XDG_STATE_HOME/review-buddy/logs/`, never to the screen) · `insta` (snapshot tests of rendered buffers).

## Binary crate layout

The `review-buddy` crate is a library (`src/lib.rs`) with a thin `main.rs`, so integration tests and later features can reach the code. `app/` holds the `App` state, `Msg`, `Cmd`, `Action` and the pure `update`. `ui/` holds `draw(frame, &App) -> HitMap` (top bar, body placeholder per `Screen`, footer hints, toast stack, the 100×30 minimum-size notice), the `HitMap`, and `ui::style`, the adapter from `rb-theme`'s framework-neutral colours onto ratatui. `runtime/` owns the terminal modes (alternate screen, mouse, bracketed paste, focus events, restored on drop and from a panic hook) and the tokio loop that selects over crossterm's `EventStream`, a tick, and the `Cmd` results channel, and redraws only when the app is dirty and at most about 30 times a second. `cli.rs` is shared with `build.rs` through `include!`.

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
2. Global keys (`⌃K`, `/`, `tab`, `,`, `T`, `L`, `J`, `q`).
3. Focused-pane keys (`↑↓`, `space`, `⏎`, `← →`).
4. Screen action keys (`a x c s m R b o v …`).

Mouse events are hit-tested against the `Rect`s recorded during the last draw (`HitMap`). Line drag-select is `Down(Left)` → anchor, `Drag(Left)` → cursor, `Up(Left)` → finish (and clear if anchor == cursor).

## Command line

With a command, the binary skips the TUI and runs one module under `crates/review-buddy/src/cmd/`. It builds the same sources, providers (or `DemoProvider` under `--demo`), cache and theme as the TUI, then resolves a selector, calls `Provider` and formats the result as a table, tab-separated lines or `--json`. Selector parsing and output formatting are pure and unit tested. See `docs/cli.md`.

## Rendering notes

- Pane titles are drawn into the top border with `Block::title`. The focused pane uses `theme.accent` for the border and title.
- Diff lines are a custom `StatefulWidget` that renders only visible rows, from a pre-highlighted `Vec<StyledLine>` cache keyed by `(file, theme_id, tab_width)`.
- `transparent` background → no `bg` is set on any cell outside overlays. Overlays call `Clear` and paint `raised`.
- Alpha colours are pre-blended at theme load against `background`, or `raised` when it is transparent.
- Wide characters and emoji (Jax captions) are measured with `unicode-width`. Jax's box reserves two cells for the emoji.

## Persistence

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
- UI: `insta` snapshots of `TestBackend` buffers for every frame on the design board (1a–1m) in all three themes, at 160×40 and 100×30.
- Cargo features: `live` (HTTP providers, keyring) and `demo` (fixtures, `--demo`), both on by default. `--no-default-features` builds the core with neither and must stay warning-free (the CI clippy and test matrix: default, no-default-features, all-features, as in jira-tui).
- Demo fixtures live in `crates/review-buddy/src/demo/` as TOML plus patch files. A `DemoProvider` implements `Provider`, so the UI code path is identical; write calls mutate in-memory state only.
- CI runs tests on Linux x64 and ARM64, Windows x64 and macOS ARM64; release builds cover Linux amd64/arm64, macOS universal and Windows amd64/arm64 (see `release.md`).
- CLI: `assert_cmd` runs every command under `--demo --frozen-time` with temp `XDG_*` dirs, piped and with `--json`, pinned with `insta` (see `docs/cli.md`).
- `cargo test --features live` runs against a throwaway GitHub repo and a GitLab project (CI only, behind secrets).
