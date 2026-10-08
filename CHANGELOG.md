# Changelog

All notable changes to review buddy are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Soft wrap in the diff.** `diff.wrap = true` (default `false`) or `z` in the diff wraps lines that are wider than the pane onto the next rows, under the code and not the line numbers. Continuation rows show a dim `↪` in the number columns and keep the sign and the added or removed tint, line numbers appear only on the first row, syntax highlighting carries across the break, and wide characters, combining marks and tab expansions never split. The cursor, ranges, `j` `k`, `c` and the mouse still work on whole lines; scrolling and paging count screen rows; resizing the terminal or opening or closing the terminal pane re-wraps and keeps the cursor line. The toggle says `Wrap on` or `Wrap off`, the pane's bottom border shows `wrap`, and the choice is remembered in `session.toml`. With wrap off nothing changes.

## [0.1.0] - 2026-10-08

### Added

- **First release contents.** 0.1.0 is the first release. It rolls up everything that was planned as v0.1.0 (GitHub, panes, unified diff) and v0.2.0 (GitLab, aggregation, first run), so there is no separate v0.2.0.
- **Three-pane dashboard.** Sources, queue and detail panes, with your pull requests bucketed into Waiting on you, Worth a look and Can wait, a collapsed Noise row for bots, and the Overview, Files, Checks and Conversation tabs.
- **Unified diff review.** A files pane, line cursor, syntax highlighting, tinted added and removed lines, inline threads, and hunk and file jumps.
- **Comments and approval.** Line comments and replies collect into a pending review, `⌃⏎` posts one now after a preview, and `a` approves after a preview of the verdict and pending comments. Discarding a draft asks first and defaults to No.
- **Submit a review with a verdict.** `a`, `x` and `R` open a review modal in the diff with Approve, Request changes or Comment selected. It previews what will be sent, lists the pending comments, takes an optional summary that becomes the review body (required for Request changes, and for a Comment review with no pending comments), and hides verdicts the source doesn't support. The submitted verdict shows as `Your review: …` and updates the queue row. The dashboard chips and keys lead into it.
- **Terminal pane.** `t` opens a terminal next to the change, in a new `rb-term` crate (a real PTY, an `alacritty_terminal` emulator, colours, wide characters, scrollback, the child's queries answered, OSC 52 copy, bracketed paste, mouse reporting and the kitty keyboard protocol). It starts in a managed worktree of the change's head, made only after a preview and a confirm that defaults to No, or in the current directory, with `RB_SOURCE`, `RB_REPO`, `RB_NUMBER` and `RB_URL` set. A chord then `esc` (default `⌃\`) or `esc esc` returns focus, Shift always stays with your terminal, and the placement and size are remembered. `[ui.terminal]` configures it. Under `--demo` it replays a scripted shell and starts nothing. Images inside the pane are not supported.
- **Review drafts that stay.** Comments, the review summary and your chosen verdict now belong to the change: leaving a diff asks `Keep these 3 comments as a draft?` (Keep is the default), quitting saves them, and reopening restores them with the cursor where you left it. They are autosaved under the state directory's `drafts/` folder (`ui.drafts = "local"`, or `"off"` for memory only; never in demo mode) with `0600` files named from a hash. A restored draft says `Draft restored · 3 comments`, flags a moved head (`the code changed since you wrote this`) and keeps a comment whose line is gone as `outdated` with the choice to add it to the summary or discard it.
- **Pending reviews list.** Queue rows with a draft show `✎ 3`, `D` lists every change with a draft (source, change, title, comments, age, `outdated`) and opens or discards one, and `review-buddy drafts list`, `drafts discard <selector>` and `drafts clear` do the same from the shell. `config paths` lists the drafts folder.
- **Edit and delete pending comments.** `e` (or `⏎`) on a line reopens its pending comment in the composer with its range, `d` deletes it after a confirm that defaults to No, `,` and `.` step between several on one line, and the Files pane lists each one with a jump. Comments restored from disk and pending comments the forge holds can be edited and deleted too: GitHub through `updatePullRequestReviewComment` and `deletePullRequestReviewComment`, GitLab through its draft notes, behind a confirm.
- **Open and copy.** `o` opens the change in the browser and `y` copies its URL, with OSC 52 and native clipboard fallbacks.
- **Mouse support.** Click, double-click, drag to select a range of lines, shift-click to extend it, and the wheel. Shift-drag always falls through to the terminal's own selection, and `ui.mouse = false` turns capture off.
- **Live GitHub sources.** github.com and Enterprise, signed in through `gh`, the OS keyring, `env:VAR` or a `token_command`. Cached rows paint first, then every source refreshes at once on launch, on `r` and when the terminal regains focus.
- **Demo mode.** `--demo` runs on offline fixtures with no network and no credentials, in a throwaway directory, with `--frozen-time` for repeatable ages and writes labelled `(demo)`.
- **Themes.** Liminal HQ (the default, on your terminal's own background), Dusk, Afterglow Dark and Afterglow Light, with `T` to cycle, `ui.theme` and `REVIEW_BUDDY_THEME` to choose, colour-depth detection and `NO_COLOR` support.
- **Command line.** `queue`, `pr list`, `pr view`, `pr diff`, `pr checks` (with `--watch`, `--fail-fast` and `--required`), `pr open`, `open`, `auth status`, `source list`, `config paths`, `theme list`, `doctor` and `completion`, with `--json` and a built-in `--jq`, selectors, tab-separated output when piped, and documented exit codes. `triage explain` and `theme check|export` are declared but not built yet (planned for v0.4.0).
- **Configuration.** A layered `config.toml` (system, user, `config.d/` and `--config`) that follows XDG on every Unix, including macOS, with a complete `config.example.toml`.
- **Installers and packaging.** `install.sh` and `install.ps1` that verify `SHA256SUMS`, static musl tarballs for Linux, a universal macOS binary, Windows archives, `.deb` and `.rpm` packages, a generated man page, and bash, zsh and fish completions.
- **Release process.** A `Release` workflow with a dry-run mode, a version bump script, and a first-release checklist in `docs/release.md`.
- **GitLab.** gitlab.com and self-hosted GitLab behind the same `Provider` trait as GitHub: sign-in (`glab`, keyring, `env:VAR` or `token_command`), merge request lists scoped by group or project, details, diffs, threads, pipeline jobs, and review writes (comments, replies, approvals and resolve) through draft notes.
- **Source aggregation.** One queue across every source, with `GH` and `GL` tags, per-source `tag_colour`, `in_all` to leave a source out of All, and a source tab strip that scrolls when it overflows.
- **First run.** A launch with no config, or `review-buddy --setup`, finds hosts from `gh` and `glab` sign-ins, `insteadOf` rewrites and repositories under `~/src`, lists accounts and GitHub organisations, tests a GitHub token live and keeps any pasted token in the keyring (a GitLab token is kept as it is; `source test` checks it afterwards), previews a theme, and writes a commented `config.toml` only when you confirm. `--setup --plain` runs the same flow as prompts.
- **Settings → Sources.** `,` opens a table of sources where you can test a token, edit, add, switch on or off, and remove a source, with comments in `config.toml` kept. Sources defined in other config layers are shown read-only.
- **Show filters.** `s` opens a control over `reviewing`, `assigned`, `authored`, `drafts` and `noise`, starting from `triage.show`, with `triage.bucket_limit` and expandable `+N more` rows. Counts reflect what the filters let through.
- **Refresh engine.** Per-source state, conditional requests, backoff with jitter on transient failures, per-host rate-limit pauses, `refresh.interval`, `refresh.on_focus` and `refresh.max_concurrency_per_host`, with a calm offline banner and toasts only on state changes.
- **Capability probe.** Each source is asked what it can do on connect (GitLab's version and token scopes, GitHub's token scopes), cached per host for 24 hours. Actions an instance can't do, such as request changes before GitLab 17.3, are hidden and explained.
- **GitHub Enterprise and self-hosted GitLab.** Hosts with ports, path prefixes and plain `http` through `api_url`, with web links and selectors that keep the scheme, port and prefix. `doctor` reports API versions, endpoints, clock skew and capabilities.
- **Command line parity.** Every read command works on GitLab, with `mr` as a hidden alias of `pr`; `auth login`, `auth logout` and `auth token`; `source test` (with `--require`) and `source add`; and `config get` and `config list`.
- **Images in descriptions.** Markdown and `<img>` images in a change's description are drawn inline in the Overview in sixel, kitty or iTerm2 where the terminal supports them, in halfblocks on a truecolour terminal, and as a one-line note anywhere else. They load in the background within strict limits (10 MB, pixel caps, redirects checked at every hop, type decided by the bytes), send the source's token only to its own hosts, and are cached on disk so they still show offline. `i` selects the next image and `o` / `y` open or copy it. `ui.images` (`auto`, `off`, `halfblocks`, `forge-only`) and `REVIEW_BUDDY_IMAGES` control it, and demo mode draws generated pictures offline.
- **Test infrastructure.** Frame snapshots for the v0.2 screens, snapshot and fixture hygiene checks, a `vt100`-based pseudo-terminal helper, and a CI retry for the pseudo-terminal tests.
- **Documentation.** A README with a real quick start, a spec, and documents for architecture, configuration, key bindings, theming, integrations, the command line, testing and releases, each marking what is built and what is planned.

### Changed

- **`doctor` shows each server version once.** It appears under API versions only, and the Capabilities section gives when the source was probed without repeating it. The `--json` fields are unchanged.
- **Source tests are shared.** Settings reuses the command line's token test, so `source test`, `auth login` and Settings → Sources agree.

### Fixed

- **Review modal is reachable on every terminal.** `tab` from the summary now lands on Submit, `⌃P` and `⌥⏎` submit as well as `⌃⏎`, and the hint under the buttons only mentions `⌃⏎` when the terminal can report it. Clicks arm on press and fire on release (a lone release fires too), a failed submit keeps the modal open with the reason shown under the buttons, and the Submit button reads `sending…` while it works.
- **Replying to a thread is discoverable.** On a line with a thread the footer shows `r reply`, the thread block's bottom border carries a clickable `r reply`, and the help overlay explains it.
- **The diff names the change under review.** A header row above the Files and Diff panes shows the forge badge, `owner/repo#number`, source label and title (cut with an ellipsis when long), plus author and branches where there's room.
- **Web links and selectors honour the host's address.** A scheme, port or path prefix on an Enterprise or self-hosted source is kept in links, copied URLs and pasted selectors.
- **Footer hints stay ahead of the refreshed time.** The passive `refreshed HH:MM` is the first thing to drop when space is short.
- **v0.1.0 review polish.** Wording, spacing and CI-state output across the TUI and command line.

[Unreleased]: https://github.com/smorrisods/review-buddy/compare/v0.1.0...main
[0.1.0]: https://github.com/smorrisods/review-buddy/releases/tag/v0.1.0
