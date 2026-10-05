# review buddy

<p align="center">
  <img src="assets/hero.svg" alt="review buddy — every pull and merge request, in one quiet queue" width="100%">
</p>

Every pull and merge request, in one quiet queue. **review buddy** is a keyboard-driven terminal UI that gathers your GitHub pull requests and GitLab merge requests into a single, calm dashboard, with full diff review, inline comments, suggestions, and merging, without leaving the terminal. Built in Rust with `ratatui` and `crossterm`, in the [Liminal HQ](https://github.com/liminal-hq) style.

> **Status:** early development. The workspace skeleton is in place and the TUI is being built milestone by milestone. See `docs/SPEC.md` for the product spec.

## Highlights

- **One queue, many forges.** GitHub (including Enterprise) and GitLab (including self-hosted) side by side, bucketed by what needs you.
- **Quiet by design.** Calm copy, no alarms, bots tucked into a collapsed Noise bucket, and reduced-motion and `NO_COLOR` support.
- **Full review in the terminal.** Unified and side-by-side diffs, line ranges, suggestions, threads, approvals, and merge pre-checks.
- **Always explorable.** `--demo` runs against offline fixtures with no network and no credentials.
- **Your secrets stay put.** Tokens live in the OS keyring, or reuse `gh`/`glab`, an `env:VAR`, or a `token_command`. Never on disk.
- **Themeable.** Liminal HQ is the default look, with Afterglow Dark and Light, and your own TOML themes.

## Getting started

```bash
cargo run -- --version   # the skeleton builds today
cargo run -- --help      # the full command-line surface
```

Build a release binary with `cargo build --release`, or an offline-only build with `cargo build --no-default-features`.

## Documentation

- [`docs/SPEC.md`](docs/SPEC.md): the product spec
- [`docs/architecture.md`](docs/architecture.md): workspace layout and the provider trait
- [`docs/configuration.md`](docs/configuration.md): config files, environment variables, and the command line
- [`docs/keybindings.md`](docs/keybindings.md): the keymap
- [`docs/theming.md`](docs/theming.md): themes and roles
- [`docs/integrations.md`](docs/integrations.md): forge APIs and auth
- [`docs/release.md`](docs/release.md): targets and the release process

## Contributing

Read `AGENTS.md` for conventions: Conventional Commits, branch prefixes, merge-commit-only PRs, and Canadian English. Enable the pre-push hook with `git config core.hooksPath scripts/hooks`.

## Licence

MIT. See [`LICENSE`](LICENSE).
