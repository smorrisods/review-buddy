# Copilot instructions for review-buddy

`review-buddy` is a keyboard-driven Rust terminal UI (`ratatui` + `crossterm`) that shows GitHub pull requests and GitLab merge requests in one queue, with full diff review and an always-available offline demo mode.

## Architecture

Cargo workspace (see `docs/architecture.md`): `rb-core` (domain, triage, `Provider` trait, no I/O), `rb-github`, `rb-gitlab`, `rb-paths` (XDG), `rb-platform` (browser, clipboard, keyring), `rb-store` (SQLite cache, drafts, queue), `rb-theme`, `rb-diff`, `rb-term` (terminal pane: emulator, PTY, encoders; no forge crates), and the `review-buddy` binary. App logic is a pure `update(&mut App, Msg) -> Vec<Cmd>`. `crates/review-buddy/src/cli.rs` is shared by `main.rs` and `build.rs` (man page via `clap_mangen`).

## What to keep true

- **Demo mode never breaks:** zero network and zero credentials must still give a fully explorable UI, without touching real config, cache, or state.
- **Secrets only in the OS keyring, `gh`/`glab` reuse, `env:VAR`, or `token_command`.** Never on disk.
- **UI never branches on forge;** use `Capabilities`.
- **Preview before mutating a forge;** destructive confirmations default to No.
- **Live writes only against a throwaway repository.**
- **XDG paths everywhere,** including macOS.
- **Calm-computing copy:** sentence case, errors say what to do next, colour never the only signal.
- **Canadian spelling** in comments, docs, and UI copy (e.g. "colour"), except for external API fields and crate names.

## Build and test

- Build: `cargo build`. Offline build: `cargo build --no-default-features`.
- Test: `cargo test`. Keep `cargo clippy --workspace --all-targets -- -D warnings` clean under the default, `--no-default-features`, and `--all-features` builds, and run `cargo fmt --all`.

## Writing style

- Don't hard-wrap prose in PRs, issues, or Markdown docs; write each paragraph or bullet as one line. Commit bodies are the exception; what matters there is correct Markdown mechanics. See `AGENTS.md`.

## Commits and PRs

- Conventional Commits (`type(scope): summary`), Markdown bodies **without headings**, using bold labels (**Summary**, **Why**, **Details**, **Validation**, **Risks**). Use `git commit -F <file>`.
- Branches use a `feature/`, `fix/`, `chore/`, `docs/`, `test/`, `ci/`, `build/`, or `design/` prefix. PR titles are human-readable with no `feat:` prefix, and merges use `gh pr merge --merge`. Update a PR branch by rebasing onto `main` and pushing with `--force-with-lease`.

See `AGENTS.md` for the full guidelines.
