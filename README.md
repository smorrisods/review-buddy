# review buddy

<p align="center">
  <img src="assets/hero.svg" alt="review buddy — every pull and merge request, in one quiet queue" width="100%">
</p>

Every pull and merge request, in one quiet queue. **review buddy** is a keyboard-driven terminal UI that gathers your pull requests into a single, calm dashboard, with full diff review, inline comments and approvals, without leaving the terminal. Built in Rust with `ratatui` and `crossterm`, in the [Liminal HQ](https://github.com/liminal-hq) style.

> **Status:** nothing is tagged yet. The first release will cover GitHub (github.com and Enterprise) and GitLab (gitlab.com and self-hosted): the queue, diff review with comments and approvals, first run, Settings → Sources and the read-only command line. Merging, suggestions, side-by-side diffs and the command palette follow in v0.3 and v0.4. See [What's included](#whats-included) for the honest list, and [`CHANGELOG.md`](CHANGELOG.md) for the history.

## Highlights

- **One queue.** Your GitHub pull requests (github.com and Enterprise) and GitLab merge requests (gitlab.com and self-hosted) bucketed by what needs you: Waiting on you, Worth a look, Can wait, and a collapsed Noise row for bots. Show filters and per-source tag colours keep a busy queue legible.
- **Quiet by design.** Calm copy, no alarms, pull-only refresh, and `NO_COLOR` support.
- **Review in the terminal.** A unified diff with threads, a line cursor, comments that collect into a pending review, and approval after a preview, on GitHub and GitLab alike. A capability probe hides what an instance can't do.
- **Easy to start.** First run finds the hosts you already use, and Settings → Sources (`,`) tests, adds, edits and removes accounts without leaving the app.
- **Scriptable.** A `gh`-style command line (`queue`, `pr view`, `pr diff`, `pr checks`, `auth`, `source`, `doctor`, `--json` and `--jq`, with `mr` as an alias of `pr`) that never opens the TUI.
- **Always explorable.** `--demo` runs against offline fixtures with no network and no credentials.
- **Your secrets stay put.** Tokens come from `gh`, the OS keyring, an `env:VAR` or a `token_command`. Never from disk.
- **Themeable.** Liminal HQ is the default look, with Dusk, Afterglow Dark and Afterglow Light built in.

## What's included

| Area | Included | Planned |
|---|---|---|
| Forges | GitHub, including Enterprise Server, and GitLab, including self-hosted. Sources are aggregated into one queue, with a capability probe per source | |
| First run and sources | First run (`--setup`) that finds your hosts and writes a commented config. Settings → Sources (`,`) to test, edit, add, switch and remove sources | The rest of Settings: Review, Keys, Theme and Jax (v0.4) |
| Dashboard | The three-pane layout (sources, queue, detail) with Overview, Files, Checks and Conversation tabs, Show filters, `+N more` rows and a refresh engine with per-source state, backoff and rate-limit pauses | Split and one-at-a-time layouts, command palette, search (v0.4) |
| Diff | Unified diff, file list, line cursor, inline threads, hunk and file jumps, syntax highlighting with tinted added and removed lines | Ranges from the keyboard, suggestions, side by side, merge, re-run CI, checkout (v0.3) |
| Review | Line comments that collect into a pending review, replies, approve with a preview, open and copy the URL | Request changes in the interface (v0.3), draft persistence (1.0) |
| Mouse | Click, double-click, drag a range, wheel, tabs and chips. Shift always falls through to the terminal | |
| Command line | `queue`, `pr list`, `pr view`, `pr diff`, `pr checks`, `pr open` (all also as `mr`), `open`, `auth status|login|logout|token`, `source list|test|add`, `config paths|get|list`, `theme list`, `doctor`, `completion`, with `--json` and `--jq` | Writes (v0.3), `triage explain`, `theme check|export` |
| Demo | `--demo` with frozen time, writes labelled `(demo)` | `--demo-scene`, `--jax-mood` and `--size` are accepted but do nothing yet |
| Themes | Four built-ins, `NO_COLOR`, colour-depth detection | User theme files (v0.4) |
| Install | `install.sh`, `install.ps1`, archives, `.deb` and `.rpm`, shell completions and a man page | Homebrew, Scoop and friends are not planned yet |

## Quick start

Try it with no network and no credentials:

```bash
review-buddy --demo
```

The terminal needs to be at least 100×30. Press `?` for the keys on the current screen, `⏎` to open a diff, `q` to quit. Nothing in demo mode touches your config, cache or tokens.

### First run

When you're ready to use your own accounts, just run `review-buddy`. With no config file it opens a calm first-run screen that finds the hosts you already use (from `gh` and `glab` sign-ins, your `~/.gitconfig` `insteadOf` rewrites and repositories under `~/src`), lists your accounts and GitHub organisations, lets you reuse a CLI sign-in or paste a token (tested live for GitHub, then kept in your OS keyring and never in `config.toml`; a GitLab token is kept without a live check, and `review-buddy source test` checks it afterwards), previews a theme, and writes a commented `config.toml` only when you confirm. Press `esc` to skip it. Run `review-buddy --setup` any time to go through it again (an existing file is replaced only if you say yes, and a copy is kept as `config.toml.bak`), or `review-buddy --setup --plain` for a line-based version that also runs when there's no terminal to draw in.

Add another account later with `review-buddy source add --host ghe.example.com --org my-team` (it previews the `[[source]]` block, keeps your comments, and checks the sign-in), store a token with `review-buddy auth login --host ghe.example.com`, and see what each source can do with `review-buddy source test`. `review-buddy config get ui.theme` shows a setting and the file it came from.

You can still write the config by hand: copy [`config.example.toml`](config.example.toml) to `~/.config/review-buddy/config.toml` (or `$XDG_CONFIG_HOME/review-buddy/config.toml`). Press `,` in the queue to manage sources from inside the app. `review-buddy doctor` checks sign-in, rate limits, API versions, endpoints, clock skew, capabilities and the paths in use, for GitHub Enterprise and self-hosted GitLab as well as the public hosts.

### Command line

With no command, `review-buddy` opens the TUI. With a command it is non-interactive and pipe-friendly, like `gh`. The examples below run on the demo fixtures, so you can try them verbatim; drop `--demo` and `--frozen-time` to use your own sources.

```bash
review-buddy --demo --frozen-time 2026-10-05T10:00 queue
review-buddy --demo --frozen-time 2026-10-05T10:00 -s liminal-hq -R liminal-hq/review-buddy pr view 214
review-buddy --demo --frozen-time 2026-10-05T10:00 -s liminal-hq -R liminal-hq/review-buddy pr diff 214 --stat
review-buddy --demo --frozen-time 2026-10-05T10:00 -s liminal-hq -R liminal-hq/review-buddy pr checks 214
review-buddy --demo --frozen-time 2026-10-05T10:00 -s platform mr view '!1182'
review-buddy --demo --frozen-time 2026-10-05T10:00 -s platform mr diff '!1182' --stat
review-buddy --demo auth status
review-buddy --demo doctor
```

GitLab works the same way: `mr` is a quiet alias of `pr`, refs are `group/project!12` (subgroups included), and any merge request URL is a selector. On a live GitLab source, `mr diff` rebuilds a `git apply`-ready patch and `mr checks` lists pipeline jobs. See `docs/cli.md`.

Without `--demo`, a number needs `--repo`, or a git clone whose remote matches one of your sources (`review-buddy pr view 214`).

Piped output is stable, tab-separated and uncoloured. `queue` prints `bucket  source  ref  ci  author  updatedAt  title`:

```text
$ review-buddy --demo --frozen-time 2026-10-05T10:00 queue
wait	liminal-hq	liminal-hq/review-buddy#214	running	ada	2026-10-05T08:00:00Z	Add a menu bar and keyboard-driven menus
wait	liminal-hq	liminal-hq/review-buddy#209	pass	jo	2026-10-05T05:00:00Z	Raise muted text contrast in the light theme
look	platform	platform/flow!1182	fail	priya	2026-10-05T03:00:00Z	Cache pipeline status lookups
look	smorris	smorris/dotfiles#31	pass	smorris	2026-10-04T10:00:00Z	Tidy zsh startup
later	gitlab-com	kai-codes/notes!12	none	kai	2026-10-02T10:00:00Z	Clarify the install steps
```

`pr view` summarises one change:

```text
$ review-buddy --demo --frozen-time 2026-10-05T10:00 -s liminal-hq -R liminal-hq/review-buddy pr view 214
Add a menu bar and keyboard-driven menus
liminal-hq/review-buddy#214 · open
ada wants ada/menus → main
+32 −1 · 3 files · opened 3d ago

Bucket    Waiting on you · your review is requested
Reviewers smorris (requested), jo (commented)
Checks    1 running · 2 passing · 1 neutral · 1 skipped · 1 cancelled
          running   test (linux)
          skipped   test (windows)
          neutral   coverage
          cancelled bench
Merge     mergeable, though a check isn't passing
Labels    enhancement, ui
Link      https://github.com/liminal-hq/review-buddy/pull/214

Adds a menu bar along the top and a `Menu` type that tracks the selected item.
...
```

`pr diff` prints the patch (`--stat` and `--name-only` summarise it), and `pr checks` exits `0` when everything passed, `1` when something failed and `8` while checks are still running:

```text
$ review-buddy --demo --frozen-time 2026-10-05T10:00 -s liminal-hq -R liminal-hq/review-buddy pr diff 214 --stat
 src/ui/menus.rs   |  23 ++++++++++++++++++++++-
 src/ui/menubar.rs |   8 ++++++++
 src/ui/mod.rs     |   2 ++
 3 files changed, 32 insertions(+), 1 deletion(-)
```

Add `--json <fields>` for scripts (with no fields it lists the ones available), and `--jq <expr>` to filter with a built-in jq, so you don't need the `jq` binary:

```text
$ review-buddy --demo --frozen-time 2026-10-05T10:00 queue --json number,title --jq '.[0]'
{"number":214,"title":"Add a menu bar and keyboard-driven menus"}

$ review-buddy --demo --frozen-time 2026-10-05T10:00 queue --json ref,ci --jq '[.[]|select(.ci=="fail")|.ref]'
["platform/flow!1182"]

$ review-buddy --demo --frozen-time 2026-10-05T10:00 auth status --jq '.[0].user'
smorris
```

`review-buddy --help` lists everything, and `docs/cli.md` has the selectors, output rules and exit codes. `triage explain` and `theme check|export` are declared but not built yet: they exit `2` and say so. The write commands (`pr review`, `pr merge` and friends) aren't declared until v0.3.

### Keys

| Key | Where | Does |
|---|---|---|
| `j` `k` / `↑` `↓` | Everywhere | Move |
| `g` `G` | Everywhere | First / last row or line |
| `tab` `h` `l` | Dashboard | Next / previous pane |
| `1`–`9` | Dashboard | Switch source |
| `[` `]` | Dashboard | Previous / next detail tab |
| `⏎` or `d` | Dashboard | Open the diff |
| `s` | Dashboard | Show filters |
| `,` | Dashboard | Settings → Sources |
| `r` | Dashboard | Refresh now |
| `n` `p` | Diff | Next / previous hunk |
| `→` `←` or `]` `[` | Diff | Next / previous file |
| `⇧↑` `⇧↓` or `V` then `j` `k` | Diff | Select a range of lines (`c` comments on it, `esc` clears) |
| `⌃Z` | Everywhere (macOS, Linux) | Suspend to the shell |
| `tab` | Diff | Switch between files and diff |
| `c` / `r` | Diff | Comment on the line / reply to the thread |
| `a` | Diff | Approve, after a preview |
| `o` / `y` | Everywhere | Open in the browser / copy the URL |
| `T` | Everywhere | Cycle theme |
| `?` | Everywhere | Help for the current screen |
| `esc` / `q` | Diff / dashboard | Back / quit |

The full keymap, including what is planned, is in [`docs/keybindings.md`](docs/keybindings.md).

### A frame from the demo

Screenshots and recordings aren't checked in as binary files. This is one real frame, captured from `review-buddy --demo --frozen-time 2026-10-05T10:00` in a 130×30 terminal, and the snapshots under `crates/review-buddy/tests/snapshots/` pin the canonical frames in every theme.

<details>
<summary>Three-pane dashboard (demo)</summary>

```text
 review buddy  ·  demo · 4 sources · 7 changes                                                                         Liminal HQ
╭ sources ───────────────╮╭ queue ───────────────────────────────────────╮╭ liminal-hq/review-buddy#214 ─────────────────────────╮
│▌ ● All               7 ││  Waiting on you · 2                          ││ Add a menu bar and keyboard-driven menus             │
│    every source        ││▌ ◐ Add a menu bar and keyboard-driven me… 2h ││ ada wants ada/menus → main                           │
│  ● liminal-hq        3 ││▌ GH review-buddy#214 · ada · review requested││ +32 −1 · 3 files · opened 3d ago                     │
│    github.com          ││  ● Raise muted text contrast in the ligh… 5h ││                                                      │
│  ● smorris           1 ││  GH review-buddy#209 · jo · approved by you  ││ a Approve  x Request changes  c Comment  ⏎ Diff      │
│    github.com          ││                                              ││                                                      │
│  ● platform          2 ││  Worth a look · 2                            ││  Overview   Files 3   Checks 6   Conversation 3      │
│    gitlab.platform.ex… ││  ✕ Cache pipeline status lookups          7h ││ ──────────────────────────────────────────────────── │
│  ● gitlab.com        1 ││  GL flow!1182 · priya · you're mentioned     ││ Adds a menu bar along the top and a Menu type that   │
│    gitlab.com          ││  ● Tidy zsh startup                       1d ││ tracks the selected item.                            │
│                        ││  GH dotfiles#31 · smorris · yours            ││                                                      │
│  Show  s to change     ││                                              ││ • Menu::select_next and Menu::select_prev move the   │
│  [x] reviewing         ││  Can wait · 1                                ││ highlight                                            │
│  [x] assigned          ││  · Clarify the install steps              3d ││ • Menu::activate returns the action for the          │
│  [x] authored          ││  GL notes!12 · kai · approved by you         ││ highlighted item                                     │
│  [ ] drafts            ││                                              ││ • The bar itself is a plain Line, so it picks up the │
│  [ ] noise             ││  ▸ Noise · 2 bot updates · ⏎ to expand       ││ …                                                    │
│                        ││                                              ││                                                      │
│                        ││  ── That's everything.                       ││ Reviewers                                            │
│                        ││  7 in this view.                             ││ ◌ smorris requested                                  │
│                        ││                                              ││ ✎ jo commented                                       │
│                        ││                                              ││                                                      │
│                        ││                                              ││ Checks                                               │
│                        ││                                              ││ ● 2 passing  ◐ 1 running                             │
│                        ││                                              ││ ○ 1 neutral  ↷ 1 skipped  ⊘ 1 cancelled              │
│                        ││                                              ││                                                      │
╰────────────────────────╯╰──────────────────────────────────────────────╯╰──────────────────────────────────────────────────────╯
 ⏎ diff  o open  y copy  s show filters  ? help  T theme  q quit
```

</details>

## Install

Linux and macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/smorrisods/review-buddy/main/scripts/install.sh | sh
```

The installer picks the static musl build on Linux (amd64 or arm64) and the universal binary on macOS, verifies the download against `SHA256SUMS`, and refuses on a mismatch. It installs into `/usr/local` when that is writable, and otherwise into `~/.local` with a note; it never runs `sudo` for you, and prints the command instead. The man page, bundled themes and shell completions go alongside the binary. Options go after `sh -s --`:

```bash
curl -fsSL https://raw.githubusercontent.com/smorrisods/review-buddy/main/scripts/install.sh | sh -s -- --dry-run
curl -fsSL https://raw.githubusercontent.com/smorrisods/review-buddy/main/scripts/install.sh | sh -s -- --version v0.1.0 --prefix "$HOME/.local"
curl -fsSL https://raw.githubusercontent.com/smorrisods/review-buddy/main/scripts/install.sh | sh -s -- --uninstall
```

Windows (PowerShell 5.1 or 7):

```powershell
irm https://raw.githubusercontent.com/smorrisods/review-buddy/main/scripts/install.ps1 | iex
```

It installs to `%LOCALAPPDATA%\Programs\review-buddy`, verifies `SHA256SUMS`, and offers to add that folder to your user `PATH`. Download `install.ps1` first if you want `-Version`, `-InstallDir`, `-DryRun`, `-Yes` or `-Uninstall`.

By hand: download an archive and `SHA256SUMS` from the [releases page](https://github.com/smorrisods/review-buddy/releases), check it with `sha256sum -c SHA256SUMS --ignore-missing`, and unpack it into a prefix. Debian and Ubuntu users can install the `.deb`, and Fedora and RHEL users the `.rpm` (glibc 2.35 or newer). The binaries are unsigned: macOS gets an ad-hoc signature only, and Windows may show a SmartScreen prompt for browser downloads. `docs/release.md` covers both.

To build from source you need Rust 1.87 or newer:

```bash
cargo build --release          # target/release/review-buddy
cargo build --no-default-features   # no demo fixtures, no network
```

## Configuration

Config lives at `$XDG_CONFIG_HOME/review-buddy/config.toml` (default `~/.config/review-buddy/`, on macOS too, and `%APPDATA%\review-buddy\` on Windows). Start from [`config.example.toml`](config.example.toml), which is complete and commented. Not every option is applied yet; `docs/configuration.md` marks which ones are.

## Documentation

- [`docs/SPEC.md`](docs/SPEC.md): the product spec, with the release plan
- [`docs/cli.md`](docs/cli.md): the command line, selectors, output and exit codes
- [`docs/architecture.md`](docs/architecture.md): workspace layout and the provider trait
- [`docs/configuration.md`](docs/configuration.md): config files, environment variables, and the command line
- [`docs/keybindings.md`](docs/keybindings.md): the keymap
- [`docs/theming.md`](docs/theming.md): themes and roles
- [`docs/integrations.md`](docs/integrations.md): forge APIs and auth
- [`docs/release.md`](docs/release.md): targets and the release process
- [`docs/testing.md`](docs/testing.md): running tests and reviewing snapshots
- [`CHANGELOG.md`](CHANGELOG.md): what changed in each release

## Contributing

Read `AGENTS.md` for conventions: Conventional Commits, branch prefixes, merge-commit-only PRs, and Canadian English. Enable the pre-push hook with `git config core.hooksPath scripts/hooks`. Run `cargo test --workspace` before you push, and see [`docs/testing.md`](docs/testing.md) for how the frame snapshots work and how to review them with `cargo insta review`. Tests use demo mode and recorded fixtures only; live write tests run against throwaway repositories, never a real project.

## Licence

MIT. See [`LICENSE`](LICENSE).
