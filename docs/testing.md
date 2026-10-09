# Testing

How the tests are organised, and how to review and accept snapshot changes. Run everything with `cargo test --workspace`.

## Layout option snapshots

`tests/layout_options.rs` renders the dashboard at 160×40, 129×40 and 100×30 for each combination of the `ui.sources` and `ui.detail` options (the default, Sources on top, Detail closed, both) in `liminal-hq`, plus `dusk` for two of them, and checks the toggle keys, the clickable markers and the footer hint. `tests/detail_position.rs` renders the five `ui.detail_position` values at the same sizes (plus Dusk, sources on top and a closed Detail) and tests hits in the stacked layouts, and `tests/pty_detail_position.rs` cycles `P` in the real binary. `tests/resizable_panes.rs` snapshots dragged layouts at 160×40 and 100×30 (side by side and stacked) and tests the accent cue, hit-testing and the minimums; `tests/pty_resizable_panes.rs` sends SGR mouse press, drag and release sequences to the real binary. `tests/pty_layout.rs` presses `p` and `S` in the real binary under `--demo`.

## Frame snapshots

`crates/review-buddy/tests/frames.rs` renders the canonical demo frames headlessly with ratatui's `TestBackend` and pins them with `insta`:

- **1a Three panes**: the dashboard with the queue, detail and sources visible.
- **1d Diff**: the unified diff of the first demo change, the cursor on a changed line with the existing thread shown beneath it.
- **1f Diff with the composer**: the composer docked under a line that already has a pending suggestion, with some text typed.

Each frame is rendered in `liminal-hq` (the default, transparent background), `dusk` and `afterglow-dark`, at 160×40 and 100×30. A `NO_COLOR` variant of each frame (in `liminal-hq`, at both sizes) snapshots the screen text plus a dump of the bold, dim and reverse runs, so a change to the signals that survive without colour shows up in review. Colour is asserted directly in the same file: the selected row's rule and the composer border use `accent`, added and removed lines carry `added_bg` and `removed_bg`, the wordmark runs the theme's gradient, and `liminal-hq` leaves the background unset.

The frames are deterministic. They use the demo data at its frozen time, a fixed size and truecolour forced in the `AppConfig`, so nothing depends on the terminal, the environment or the clock. Snapshots are plain text, stored with LF line endings (`.gitattributes` marks `*.snap` as `eol=lf`), so they match on Windows too. The older per-screen snapshots in `dashboard.rs`, `diff.rs`, `composer.rs`, `help.rs` and `render.rs` cover other sizes, states and screens, and stay.

## v0.2 frames

`crates/review-buddy/tests/frames_v02.rs` pins the v0.2 screens the same way, sharing its render helpers with `frames.rs` through `tests/support/render.rs`:

- **1i First run**: each step (welcome, connect accounts, a masked token entry, scope, look, Jax, summary).
- **1j Settings → Sources**: the table with check results, the edit form with a typed token, the add picker and form, the remove confirm, and a table whose file is read-only because it comes from another config file.
- **Mixed dashboard**: the All view, a GitLab source tab, and the collapsed source strip below 130 columns (129×40).

Every frame is rendered in `liminal-hq`, `dusk` and `afterglow-dark` at 160×40 and 100×30, with `NO_COLOR` text-and-modifier variants for `liminal-hq`. Colour assertions live in the same file: the first-run card border uses `accent`, source tags take their forge colour from `style::tag_fg`, the selected Settings row carries a background the others lack, and the wordmark keeps its gradient. The offline banner, rate-limit note and refreshing states are pinned in `refresh_state.rs` in all three themes. The first-run and Settings behaviour tests (`first_run.rs`, `settings.rs`) no longer keep their own screen snapshots: add new screens to `frames_v02.rs` instead of duplicating them.

## v0.1 additions

Everything below lives in `crates/review-buddy/tests/` and follows the rules above: demo or stub-server data only, no network, a sandboxed home.

- **Terminal pane:** `terminal_pane.rs` drives the pane through `update` and pins frames with the scripted pane, and `pty_terminal.rs` runs the real binary (demo mode's scripted pane, and live mode against a stub server with a real shell, clone and worktree). See [Terminal pane tests](#terminal-pane-tests).
- **Review drafts:** `drafts.rs` renders the queue marker, the Pending reviews list, the leave confirm, a restored draft, and editing and deleting a pending comment from the frozen demo data. `pty_drafts.rs` runs the real binary twice in one sandbox against a stub GitHub server (comment, leave keeping the draft, quit, relaunch, see it restored, then edit and delete) and checks that nothing is sent and the draft file is private. `cli_drafts.rs` covers `drafts list|discard|clear` and the drafts folder in `config paths`.
- **Images in descriptions:** `images.rs` renders the Overview headlessly as halfblocks (the one renderer whose output is plain text and identical everywhere), including scrolling, clipping and selection; real sixel, kitty and iTerm2 bytes are never snapshotted. `live_images.rs` fetches through the live backend against a stub server and checks the limits, the disk cache (including offline), `forge-only` and that the token never goes over plain http. `pty_images.rs` runs the real binary in demo mode with terminals that answer the capability query, ones that don't, and `NO_COLOR`.
- **Diff header and review modal:** `pty_diff_header.rs` checks the header row naming the change, in the real binary. `review_submit.rs` drives the review modal's keys, buttons and the thread reply hint through the real hit map.
- **Resizable panes and the remembered layout:** `resizable_panes.rs` snapshots dragged layouts and tests hit-testing and the minimums, and `pty_resizable_panes.rs` sends SGR mouse sequences to the real binary. `session.rs` tests which keys schedule a debounced write of `session.toml`, the write on quit and the toast. `pty_session.rs` rotates the Detail pane, quits, relaunches and checks the remembered arrangement, the file's mode, `ui.remember_layout = false`, the environment taking precedence and that `--demo` leaves nothing behind. `cli_reset_layout.rs` covers `config reset-layout` and the session file in `config paths`.

- **Text selection:** the selection geometry is unit-tested in `app/selection.rs` (constraint to the region a drag started in, reading-order normalisation, wide characters and combining marks, soft-wrap joins, word and line selection, keyboard movement, rows that scrolled out of view), the recorded text map in `ui/textmap.rs`, the wrap row joins in `ui/text.rs`, and the clipboard notices (characters counted, never the text) in `runtime/effects.rs` and `runtime/mod.rs`. `mouse.rs` drives drags, double and triple clicks, `esc`, `y`, Shift and the gutter split against the demo data (the Files paths, the Detail description, `NO_COLOR`) and snapshots an active selection at 160x40 and 100x30 in the default theme and Dusk, with the selected cells wrapped in `⟦ ⟧` so a snapshot shows exactly what is highlighted. `text_selection.rs` uses a dedicated patch for wrapped lines, CJK, threads, suggestions, `Y`, copy mode, `M` and the selection being dropped when the screen changes. `pty_selection.rs` sends SGR drag sequences to the real binary under `--demo` and checks the OSC 52 sequence, the highlighted cells with the vt100 helper, `esc`, and `M` switching mouse reporting off and on.
- **Diff soft wrap:** `diff_wrap.rs` builds its own long-line patch through `DiffData::new` (the demo patches stay short, so no other diff snapshot moves) and covers the toggle and its status, the `↪` continuation rows, the tint and sign on every row, tabs and wide characters, cursor and range movement by logical line, clicks and drags on continuation rows, thread and suggestion blocks under the last row, paging and jumps by screen rows, resize and terminal-pane re-wrapping, the windowed 6,000-line case, the screen-to-logical mapping, and snapshots at 160x40 and 100x30 in the default theme and Dusk. The wrapping maths (graphemes, widths, tabs, long tokens) is unit-tested in `ui/text.rs`, the screen-row layer in `app/diffview.rs`, and the remembered `diff_wrap` in `session.rs` and `runtime/mod.rs`. `pty_diff_wrap.rs` presses `z` in the real binary, resizes and checks the cursor line survives.
- **Conversation tab:** `conversation.rs` drives the demo conversation (three threads on the menus change, replies, a resolved one that starts folded, a long body with a list and a fence) at 160x40 and 100x30 in the default theme and Dusk, plus a `NO_COLOR` text-and-modifier dump, and builds its own threads for what the demo does not hold (outdated and pending labels, a range, wide characters, an image note, a 400-comment conversation and its scroll limit). It checks the layout rows, `n` `N` `z` `Z`, `c` and `⏎` leading into the diff on the right line, header clicks, selection rows that join wrapped lines, and copy mode. `pty_conversation.rs` presses the keys in the real binary under `--demo`.
- **Queue status cluster:** `queue_status.rs` renders the demo queue at 160x40 (Detail closed, so the queue has the width) and 100x30 in the default theme, Dusk and a `NO_COLOR` text-and-modifier dump, and checks that every state shows (approved, changes requested, outstanding reviewer, comments, open threads, CI running and failing, size), that `CI failing` is not repeated beside a reason that says it, the drop order at 61 to 101 columns of Queue, that the title and age keep their place, that columns line up, `ui.queue_status` choices and rows from an old cache. The layout, widths, truncation and wording are unit-tested in `ui/queue_status.rs`; `cli_queue_status.rs` covers the setting; `pty_queue_status.rs` presses `p` in the real binary under `--demo`. The provider mapping is covered with wiremock in `rb-github/tests/changes.rs` and `rb-gitlab/tests/changes.rs`, the query's size in a unit test in `rb-github/src/changes.rs`, and an older cached summary without the new fields in `rb-store` and `rb-core`. Comment totals are pinned for both forges (threads with several comments, resolved and open, only general comments, and past 50 threads as a floor in `rb-github/tests/changes.rs` with the `list_many_threads` fixture; system notes left out in `rb-gitlab/tests/reads.rs`), the demo has a test that no row's `¶N` sits below its open threads, and `conversation.rs` checks the tab and the row show the same number. `help.rs` scrolls the queue's help to the end at 100x30 and 160x40 (snapshots included) and checks every legend mark, the suspend row and the general keys are reachable. Adding the cluster moved most dashboard snapshots (the queue rows), and the demo reviewers of two changes moved the Reviewers list in the detail pane of "Raise muted text contrast" and "Tidy zsh startup".

## Reviewing and accepting snapshot changes

A failing snapshot prints a diff of the old and new frame. To review the changes interactively, install `cargo-insta` once (`cargo install cargo-insta`) and run:

```sh
cargo insta review
```

It steps through each pending change, and you accept or reject each one. Without `cargo-insta`, the test run writes the new frame beside the old one as `*.snap.new`; compare the two files, then rename the new one over the old one to accept it, or delete it to reject it.

To accept every change at once, which is only appropriate after you've read the diff, re-run the tests with:

```sh
INSTA_UPDATE=always cargo test -p review-buddy
```

`INSTA_UPDATE=no` never writes anything, and CI runs with `INSTA_UPDATE=no` semantics (`CI` is set), so a changed frame fails the build instead of being rewritten. Commit the updated `.snap` files with the change that caused them.

## Snapshot hygiene

`tests/snapshot_hygiene.rs` is a cheap file scan that fails when a `.snap.new` or `.pending-snap` file exists anywhere in the crate, or when a file in `tests/snapshots` has no test that still produces it (its `<test file>__<name>.snap` name must point at an existing `tests/<test file>.rs` that mentions `<name>`). When you rename or delete a snapshot test, delete its `.snap` file too.

## Recorded fixtures

`crates/rb-gitlab/tests/fixtures` and `crates/rb-github/tests/fixtures` hold the JSON the wiremock stubs serve. They are hand-written to match the documented API shapes (GitLab 16.x and 17.x, GitHub GraphQL and REST), not captured from live accounts, and each directory's `README.md` says which endpoint each file stands for. `tests/fixtures_hygiene.rs` in each crate fails when a fixture is invalid JSON, is not named by any test, or is missing from the README table. Fixtures are stored with LF endings (`.gitattributes`).

## Terminal pane tests

`rb-term` has unit tests beside its code (the emulator boundary, key and mouse encoder tables for the legacy and kitty encodings, query replies, the focus chord, the scripted pane, worktree planning against real local git repositories, and the widget) and `crates/rb-term/tests/pty_shell.rs`, which runs real `/bin/sh` children in a PTY (unix only) and checks output, input, environment, resizing, query answers and exit. In the binary, `tests/terminal_pane.rs` drives the pane through `update` (the start prompt and its default of No, focus and the chord, double `esc`, what reaches the child, the mouse, the seam, resizing, remembered placement) and pins frames at 160×40 and 100×30 with the scripted pane in `liminal-hq` and `dusk` plus the start prompt; `tests/pty_terminal.rs` runs the real binary: demo mode's scripted pane (which must write no state), and live mode against a stub server with a real shell, a real local clone and a real managed worktree. Nothing in them needs the network, and none touches a real home. The Windows ConPTY path compiles but is not run by any test here.

## Pseudo-terminal tests

`crates/review-buddy/tests/support/pty.rs` (include it with `#[path = "support/pty.rs"] mod pty;`) runs the real binary in a pty and feeds its output into a `vt100` parser, so assertions run against the rendered screen rather than raw escape-sequence bytes. `pty::command(home)` returns a command with a cleared environment, a colour terminal and a sandboxed home with the XDG directories under it (unix only); `Pty::spawn(cmd, cols, rows)` starts it; `wait_for_screen(needle, timeout)` polls the parsed screen, `expect` and `send_expect` do the same with the default limit and print the screen on failure, and `send_until` resends an idempotent key until its effect shows, for the first key after start-up on a loaded runner. There are no repaint nudges or fixed sleeps. `pty_show.rs`, `pty_setup.rs` and `pty_composer.rs` use it; the other pty tests still match raw bytes and can move over as they prove flaky.

The pseudo-terminal tests depend on terminal timing, which varies on shared CI runners. CI runs `cargo nextest run --profile ci` (see `.config/nextest.toml`), which retries any test in a binary whose name contains `pty` up to twice and every other test not at all, so a flaky terminal test is retried but a logic failure still fails the build. Locally `cargo test` doesn't retry, so a pty test that fails once and passes on a rerun is worth fixing, not ignoring.

## Provider, refresh and live tests

Provider behaviour is tested against `wiremock` stubs serving the recorded fixtures: `crates/rb-github/tests` and `crates/rb-gitlab/tests` cover lists, details, diffs, threads, checks, review writes, rate limits and self-hosted layouts. In the binary crate, `live_gitlab.rs`, `live_gitlab_writes.rs`, `live.rs` and `live_writes.rs` build the live factory against a stub server, `refresh_engine.rs` drives the refresh engine with a clock that only moves when told to (no real waiting), `cli_gitlab.rs`, `cli_gitlab_demo.rs` and `cli_enterprise.rs` run the read commands on GitLab, Enterprise and self-hosted layouts, and `capabilities.rs` checks that actions a source can't do leave the chips and the review block. These are gated with `#[cfg(feature = "live")]` or `"demo"`, so they pass under every feature set. Live write tests are `#[ignore]`d and only run against a throwaway repository or project you name (see `docs/integrations.md`); nothing in CI writes to a real forge.

## CLI end-to-end tests

The consolidated command layer runs the real `review-buddy` binary through `assert_cmd`:

- **`tests/cli_e2e.rs`** runs the v0.1.0 commands in demo mode, piped, with `--demo --frozen-time 2026-10-05T10:00`, and pins stdout with `insta` (`queue`, `pr list`, `pr view`, `pr diff` with `--stat` and `--name-only`, `pr checks`, `pr open`, `auth status`, `source list`, `theme list`, and the non-path parts of `doctor` and `config paths`). It also pins the `--json` field list for each command, a representative `--json` payload, and `--jq` examples. `open` (needs a terminal), `triage explain` (not built) and `completion` (a script when built, a calm exit `2` when not) are asserted directly.
- **`tests/cli_e2e_exit.rs`** covers exit codes `0`, `1`, `2`, `4`, `5` and `8` through the binary, and the commands that work without `--demo` or a network. Demo-only and live-only cases are gated by feature, so the file passes under the default, `--no-default-features` and `--all-features` builds. Exit `3` (a write refused without `--yes` on a non-terminal) is covered by the unit tests in `cmd::prompt` and `cmd::error`, because no command writes to a forge yet.
- **`tests/cli_e2e_tty.rs`** (unix only) runs `queue`, `pr view`, `pr checks` and `theme list` in a pseudo-terminal and asserts headers, glyphs and colour, then repeats with `NO_COLOR` to check the same text survives without colour.

All three include the shared sandbox helper with `#[path = "support/cli.rs"] mod sandbox;`. `Sandbox::cmd()` returns a command with a cleared environment and a throwaway home: `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA` and every `XDG_*` variable point into a temp directory, `NO_COLOR` is set, and no `REVIEW_BUDDY_*` variable leaks in. Only `PATH` and, on Windows, `SystemRoot`, `windir`, `PATHEXT`, `COMSPEC` and the temp variables are passed through: a fully empty environment breaks sockets and home resolution on Windows. `Sandbox::demo()` adds `--demo --frozen-time`, `with_colour` forces colour on a pipe, and `normalise` makes output comparable across machines (LF endings, the demo temp directory replaced with `<demo>`, the crate version replaced with `<version>`).

Tests of the commands that touch the keyring (`auth login|logout|token`, `source add|test`) use a test-only seam: when `REVIEW_BUDDY_TEST_KEYRING` is set, an in-memory store replaces the OS keyring, seeded from its value as `host=token,host=token` (an empty value is an empty store). It lives in `providers::system_store`, is never read from a config file, and exists so no test needs, or ever touches, a real keyring. `tests/cli_auth_source.rs` runs these commands against wiremock stubs and checks that the token appears in no output or file; `tests/pty_auth_source.rs` (unix only) covers the terminal refusals and default-No prompts.

Snapshot names are plain (`cli_e2e__pr_view.snap`) so the hygiene check can find them: keep snapshot tests at the top level of the file rather than in nested modules.
