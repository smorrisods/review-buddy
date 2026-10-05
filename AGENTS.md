# AGENTS

Guidelines for humans and AI agents working in `review-buddy`.

## Project

`review-buddy` is a keyboard-driven terminal UI that puts every GitHub pull request and GitLab merge request in one quiet queue, with full diff review, written in Rust (`ratatui` + `crossterm`). It should feel calm, fast, and legible, with a little personality. The product spec is `docs/SPEC.md`; the other files in `docs/` cover architecture, keybindings, configuration, theming, integrations, and releases. `docs/archive/design/` is the original design prototype, kept for reference only.

## Core Principles

- **Demo mode always works.** The TUI must be fully explorable with zero network and zero credentials (`--demo`, `demo` feature on by default). Demo mode never touches the real config, cache, or state directories, and its actions mutate memory only and are labelled `(demo)`.
- **Provider trait first.** GitHub and GitLab sit behind the `Provider` trait in `rb-core`. UI code never branches on forge; it asks `Capabilities`. Hide unsupported actions and explain why.
- **Secrets never touch disk.** Tokens live only in the OS keyring, reused `gh`/`glab` auth, an `env:VAR` reference, or a `token_command`. Never write a token to `config.toml`, logs, the cache, or a fixture.
- **XDG everywhere.** Config, data, cache, and state follow XDG on every Unix including macOS (`%APPDATA%` on Windows). Create directories `0700` and secret-adjacent files `0600`.
- **Pure update.** App logic is `update(&mut App, Msg) -> Vec<Cmd>`: no I/O inside `update`, so it stays unit-testable. Effects run as `Cmd`s on the async runtime and report back as `Msg`s.
- **Preview before mutate.** Anything that changes a forge (approve, comment, merge) is legible and confirmable first. Destructive confirmations default to **No**.
- **Calm computing.** Sentence-case copy, no shouting, no alarm colours for routine states, and errors say what to do next. Colour is never the only signal. Honour `NO_COLOR` and reduced motion.
- **Writes only against throwaway repos.** Live write testing (approve, comment, merge) happens only against a repository created for that purpose, never a real project. Tests and CI use demo mode and recorded `wiremock` fixtures.
- **Mouse is polite.** Shift-drag must always fall through to the terminal's native selection.

## Rust Conventions

- Cargo workspace of nine crates (`rb-core`, `rb-github`, `rb-gitlab`, `rb-paths`, `rb-platform`, `rb-store`, `rb-theme`, `rb-diff`, `review-buddy`). Put code in the crate that owns the concern; `rb-core` stays free of I/O.
- Keep fast UI state separate from slow remote state.
- Domain models in `rb-core` stay stable even if forge API shapes vary.
- Prefer small, readable functions; avoid clever borrows that hurt legibility.
- `unsafe_code` is forbidden workspace-wide.
- Run `cargo fmt --all` and keep clippy clean with `-D warnings` under the default, `--no-default-features`, and `--all-features` builds before committing.
- Add tests for pure logic and rendering: unit tests live beside the code, integration tests in `tests/` cover the CLI and headless rendering via `ratatui`'s `TestBackend`, and `insta` snapshots cover the screens.

## Canadian English

- Use Canadian spelling in comments, docs, and user-facing copy where technically valid (e.g. "colour", "behaviour").
- Keep required external spellings unchanged for API fields, crate names, and third-party schema keys.

## Writing style (commits, PRs, issues, docs)

- **Don't hard-wrap prose at a fixed column** in anything that gets rendered somewhere that reflows it: PR descriptions, GitHub issues, and Markdown docs (`README.md`, `AGENTS.md`, `docs/**`). Write each paragraph, bullet, or list item as one continuous line. Hard-wrapped lines render as awkward breaks in GitHub's viewer and in editors with soft-wrap.
- **Commit bodies are the exception.** They're read in `git log`/`git show`, a terminal context, so line length there is a readability choice. What matters is correct Markdown mechanics: backticks around identifiers and code, matched formatting, no mangled quoting. Use `git commit -F <message-file>` so backticks survive verbatim.
- This doesn't apply to fixed-width content where line length is part of the format, such as code blocks and tables.

## Commits

- Use Conventional Commits: `type(scope): short summary`.
- Bodies are Markdown, but **do not use Markdown headings (`#`)**. Use **bold** for section titles instead.
- Recommended body sections (as bold labels): **Summary**, **Why**, **Details**, **Validation**, **Risks**.
- Do not pass unescaped backticks to `git commit -m`. Prefer `git commit -F <message-file>`.
- Commit at meaningful milestones with a clear, detailed body.
- Use `test:` for test-only changes (including fixes to tests themselves). Reserve `fix:` for application-code bugs.
- The very first commit on `main` is the only commit made directly to `main`.

## Pull Requests

- **Branching:** work happens on a branch, not directly on `main`. Every branch name starts with a `prefix/` matching its purpose, followed by a short kebab-case description (add an issue number when one exists, e.g. `fix/issue-19-keyring-fallback`):
  - `feature/` new functionality (mirrors the `enhancement` label)
  - `fix/` bug fixes (mirrors `bug`)
  - `chore/` maintenance, dependency bumps, release prep (mirrors `chore`; release branches are `chore/release-vX.Y.Z`)
  - `docs/` documentation-only changes (mirrors `documentation`)
  - `test/` test-only changes (mirrors `testing`)
  - `ci/` CI/workflow changes (mirrors `ci`)
  - `build/` build/packaging changes (mirrors `build`)
  - `design/` design docs and mockups with no code change
  - `dependabot/` Dependabot's own branches, which it names itself

  `main`/`master` are exempt. Enable the `pre-push` hook once per clone with `git config core.hooksPath scripts/hooks`; it rejects a push from a branch that doesn't match this pattern.
- **Titles:** human-readable summaries starting with a capital letter, with no Conventional Commit prefixes (`feat:`, `fix:`) in the title. Describe the outcome, not internal process.
- **Content:** PR descriptions must not mention internal workflow artefacts (session notes, todo tracking, agent planning chatter). Keep that in `agent-reviews/`.
- **Description format:** compact Markdown with `## Summary` and `## Test plan`. Use `###` sub-sections under Summary when it helps. Flat bullets with bold lead-ins. Under `## Test plan`, use checklist bullets (`- [x]`/`- [ ]`) naming the concrete commands run, and state plainly anything that couldn't be verified.
- **Labels:** every PR gets at least one primary category label (`enhancement`, `bug`, `documentation`, `testing`, `ci`, `build`, `chore`) plus scope labels where useful (`rust`, `dependencies`, `github_actions`, `github`, `gitlab`, `ui`, `diff`, `theme`, `demo`, `platform`, `release`). Use `skip-changelog` for changes that shouldn't appear in release notes.
- **Readiness:** open PRs ready for review by default; draft only when asked or when there's a clearly communicated blocker.
- **Merging:** use a real merge commit (`gh pr merge --merge`), not squash or rebase, so the per-commit history survives onto `main`.
- **Force-pushing** an open PR branch requires explicit user confirmation first.
- Prefer the `gh` CLI for PR, issue, label, and workflow-run work.

## Releases

- **Tag format:** `vX.Y.Z`, matching the workspace `version` in `Cargo.toml`.
- **Version bump flow:** run `scripts/prepare-release-version.sh --version X.Y.Z` in a clean working tree. It updates `Cargo.toml`/`Cargo.lock` and creates a `chore/release-vX.Y.Z` branch. Open a PR, merge to `main`, then tag on `main`. Never tag from another branch.
- See `docs/release.md` for targets, packaging, and signing.
- **Man page:** generated at build time from `crates/review-buddy/src/cli.rs` via `clap_mangen` (`build.rs`). Never hand-edit it. `cli.rs` is the single source of truth, shared by `main.rs` and `build.rs`; don't duplicate the `Cli` definition.

## Reviews

- Leave a dated note in `agent-reviews/` after a substantial session: what worked, what caused friction, and how it was handled. Read existing reviews first.
