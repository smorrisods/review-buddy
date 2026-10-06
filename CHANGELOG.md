# Changelog

All notable changes to review buddy are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **v0.1.0, GitHub, panes, unified diff.** The first release. GitHub only; GitLab arrives in v0.2. The release is not tagged yet, so these entries stay under Unreleased until it is.
- **Three-pane dashboard.** Sources, queue and detail panes, with your pull requests bucketed into Waiting on you, Worth a look and Can wait, a collapsed Noise row for bots, and the Overview, Files, Checks and Conversation tabs.
- **Unified diff review.** A files pane, line cursor, syntax highlighting, tinted added and removed lines, inline threads, and hunk and file jumps.
- **Comments and approval.** Line comments and replies collect into a pending review, `⌃⏎` posts one now after a preview, and `a` approves after a preview of the verdict and pending comments. Discarding a draft asks first and defaults to No.
- **Open and copy.** `o` opens the change in the browser and `y` copies its URL, with OSC 52 and native clipboard fallbacks.
- **Mouse support.** Click, double-click, drag to select a range of lines, shift-click to extend it, and the wheel. Shift-drag always falls through to the terminal's own selection, and `ui.mouse = false` turns capture off.
- **Live GitHub sources.** github.com and Enterprise, signed in through `gh`, the OS keyring, `env:VAR` or a `token_command`. Cached rows paint first, then every source refreshes at once on launch, on `r` and when the terminal regains focus.
- **Demo mode.** `--demo` runs on offline fixtures with no network and no credentials, in a throwaway directory, with `--frozen-time` for repeatable ages and writes labelled `(demo)`.
- **Themes.** Liminal HQ (the default, on your terminal's own background), Dusk, Afterglow Dark and Afterglow Light, with `T` to cycle, `ui.theme` and `REVIEW_BUDDY_THEME` to choose, colour-depth detection and `NO_COLOR` support.
- **Read-only command line.** `queue`, `pr list`, `pr view`, `pr diff`, `pr checks`, `pr open`, `open`, `auth status`, `source list`, `config paths`, `theme list`, `doctor` and `completion`, with `--json` and a built-in `--jq`, selectors, tab-separated output when piped, and documented exit codes.
- **Configuration.** A layered `config.toml` (system, user, `config.d/` and `--config`) that follows XDG on every Unix, including macOS, with a complete `config.example.toml`.
- **Installers and packaging.** `install.sh` and `install.ps1` that verify `SHA256SUMS`, static musl tarballs for Linux, a universal macOS binary, Windows archives, `.deb` and `.rpm` packages, a generated man page, and bash, zsh and fish completions.
- **Release process.** A `Release` workflow with a dry-run mode, a version bump script, and a first-release checklist in `docs/release.md`.
- **Documentation.** A README with a real quick start, a spec, and documents for architecture, configuration, key bindings, theming, integrations, the command line, testing and releases, each marking what is in 0.1 and what is planned.

[Unreleased]: https://github.com/smorrisods/review-buddy/commits/main
