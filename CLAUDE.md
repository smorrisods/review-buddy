# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

`review-buddy` is a keyboard-driven terminal UI for GitHub pull requests and GitLab merge requests in one queue, written in Rust (`ratatui` + `crossterm`), with an always-available offline demo mode.

`AGENTS.md` holds the detailed conventions (writing style, commit format, PR rules, releases, Canadian spelling). Read it in full before making changes. `.github/copilot-instructions.md` is a condensed mirror. Follow `AGENTS.md` as the source of truth; this file focuses on commands and architecture. The product spec is `docs/SPEC.md`.

## Commands

```bash
cargo build                        # default build (live + demo features)
cargo build --no-default-features  # offline build
cargo run -- --version
cargo run -- --demo                # offline fixtures

cargo test                         # unit and integration tests
cargo nextest run --workspace      # matches CI's runner (see .config/nextest.toml)

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Match the three-feature-set clippy matrix locally before pushing. Enable the pre-push hook once per clone: `git config core.hooksPath scripts/hooks`.

## Architecture

A Cargo workspace; see `docs/architecture.md` for detail.

- `rb-core`: domain types, triage, review drafts, the `Provider` trait and `Capabilities`. No I/O.
- `rb-github`, `rb-gitlab`: forge providers.
- `rb-paths`: XDG resolution, config layering, directory creation.
- `rb-platform`: open URL, clipboard (OSC 52 first), keyring, per-OS code.
- `rb-store`: SQLite cache, ETag store and capability probes. Review drafts are files written by the binary crate (`drafts.rs`), and the offline queue is planned.
- `rb-theme`: theme loading, role resolution, colour-depth quantisation. Built-in themes live in `themes/`.
- `rb-diff`: patch parsing, hunk model, side-by-side pairing, syntax highlighting bridge.
- `rb-term`: the terminal pane (emulator boundary, PTY, key and mouse encoders, worktree planning). No forge crates.
- `rb-term`: the terminal pane: emulator boundary, PTY, key and mouse encoders, focus chord, scripted demo pane, worktree planning. No forge crates.
- `review-buddy`: the binary. `src/cli.rs` is shared with `build.rs` for the man page.

The app is Elm-style: a pure `update(&mut App, Msg) -> Vec<Cmd>`, with effects run as `Cmd`s and results returned as `Msg`s.

## What to keep true

- **Demo mode never breaks:** no credentials and no network must still yield a fully explorable UI, and demo never touches real config, cache, or state.
- **Secrets only in the OS keyring, `gh`/`glab` reuse, `env:VAR`, or `token_command`.** Never on disk, in logs, or in fixtures.
- **UI never branches on forge.** Use `Capabilities`.
- **Preview before mutating a forge; destructive confirms default to No.**
- **Live writes only against a throwaway repository.**
- **Calm-computing copy:** sentence case, errors say what to do next, colour is never the only signal.
- **Canadian spelling** in comments, docs, and UI copy, except for external API fields and crate names.
- **Update PR branches by rebasing onto `main`** (`git push --force-with-lease`), not by merging `main` in.
- **Branch names start with a `prefix/`** (`feature/`, `fix/`, `chore/`, `docs/`, `test/`, `ci/`, `build/`, `design/`); see `AGENTS.md`.
- **Leave a dated note in `agent-reviews/`** after a substantial session (what worked, what caused friction, how it was handled), and read the existing ones first.
