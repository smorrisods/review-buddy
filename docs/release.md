# Platforms and releases

Review Buddy uses **the same release process as [smorrisods/jira-tui](https://github.com/smorrisods/jira-tui)**: see its `docs/release/distribution-strategy.md`, `.github/workflows/release.yml`, `.github/release.yml` and `scripts/`. This page says what we reuse as-is and where we extend it. jira-tui ships Linux and macOS only; we add Windows and a universal macOS binary.

Linux is the primary platform. Windows and macOS are supported release targets. **No code signing**: macOS gets an ad-hoc signature only, and Windows is unsigned.

Repo: `smorrisods/review-buddy`. Licence: MIT. Channels for 1.0: **GitHub Releases, `install.sh`, `install.ps1`, `.deb` / `.rpm`**. No Homebrew, Scoop, AUR, Nix, crates.io or winget yet; revisit once the release cadence settles, as jira-tui's doc suggests.

## Release flow (identical to jira-tui)

1. Merge the release-ready PR into `main`.
2. Run `scripts/prepare-release-version.sh --version <next>` in a clean tree. It creates `chore/release-v<next>` and bumps **only our own** `name = "review-buddy"` stanza in `Cargo.toml` and `Cargo.lock`. It fails loudly if the stanza isn't found. `--current-version` and `--dry-run` behave the same as in jira-tui.
3. Open a PR from that branch, wait for green CI, and merge to `main`.
4. Tag `vX.Y.Z` on `main`. The tag must match `^v[0-9]+\.[0-9]+\.[0-9]+$`.
5. The `Release` workflow builds every target and creates or reuses the GitHub Release (generated notes; categories from `.github/release.yml`).
6. It attaches binaries, archives, packages and **one `SHA256SUMS`** covering every artefact.
7. Smoke-test from the published assets (checklist below), then edit the generated notes if needed.

**Manual dispatch** (`workflow_dispatch`) is the same as jira-tui: `release_tag` is a **required** input and is never derived. There's an optional `release_draft`. The workflow refuses to reuse a matching release that is already published, and only reattaches to a release that is still a draft. Concurrency is grouped per tag, with cancel-in-progress. Uploads use `gh release upload --clobber`, so reruns against a draft are idempotent.

## Workflow jobs

| Job | Runner | What it does |
|---|---|---|
| `prepare-release` | `ubuntu-24.04` | Same as jira-tui: resolves and validates the tag, creates or reuses the release, writes `$GITHUB_STEP_SUMMARY` |
| `build-linux` | `ubuntu-22.04`, `ubuntu-22.04-arm` | jira-tui's job, **extended with musl**. Each runner builds two targets: `*-unknown-linux-gnu` → `.deb` / `.rpm` via `scripts/build-linux-packages.sh`; and `*-unknown-linux-musl` (`apt install musl-tools`, native on each arch) → the standalone binary and `.tar.gz` via `scripts/build-release-archive.sh`. The musl build uses `keyring` with pure-Rust D-Bus (`zbus`, `crypto-rust`), bundled SQLite and rustls, so it is fully static (`ldd` reports "not a dynamic executable"; checked in the job) |
| `build-macos` | `macos-14` | **Extended:** builds `aarch64-apple-darwin` natively and `x86_64-apple-darwin` cross-compiled (same as jira-tui), then a new step runs `lipo -create` to make one universal2 binary, then `codesign --sign - --force` on the result. Archive via `build-release-archive.sh --arch universal` |
| `build-windows` | `windows-2022`, `windows-11-arm` | **New:** `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`. Standalone `.exe` and a `.zip` via `scripts/build-release-archive.ps1` (mirrors the shell script's layout) |
| `publish-release` | `ubuntu-24.04` | Same as jira-tui: downloads every `review-buddy-*` artefact, writes a single `SHA256SUMS` (`sha256sum -- *`, no `./` prefix) and uploads |

Fixed runner images, not `-latest`, as in jira-tui. The glibc packages build on Ubuntu 22.04, so they need **glibc ≥ 2.35** (Ubuntu 22.04+, Debian 12+, RHEL 9+, current WSL2). The musl tarballs have no glibc requirement. Caching uses `Swatinem/rust-cache@v2` with per-target keys and a shared key per OS.

## Artefacts

Same naming as jira-tui: `review-buddy-<tag>-<os>-<arch>[.ext]`.

| Asset | Linux amd64 | Linux arm64 | macOS universal | Windows amd64 | Windows arm64 |
|---|---|---|---|---|---|
| Standalone binary | `…-linux-amd64` | `…-linux-arm64` | `…-macos-universal` | `…-windows-amd64.exe` | `…-windows-arm64.exe` |
| Archive | `.tar.gz` | `.tar.gz` | `.tar.gz` | `.zip` | `.zip` |
| Packages | `.deb`, `.rpm` (glibc) | `.deb`, `.rpm` (glibc) | — | — | — |

Linux standalone binaries and tarballs are **static musl**, so they run on any distro, including Alpine and older glibc. Plus one `SHA256SUMS`.

**Archive layout** (as in jira-tui, so the archive drops into any prefix):

```text
bin/review-buddy                       (review-buddy.exe on Windows, at the zip root instead)
share/man/man1/review-buddy.1.gz       (not in the Windows zip)
share/review-buddy/themes/*.toml
share/review-buddy/config.example.toml
share/doc/review-buddy/LICENSE
```

`share/review-buddy/themes/` lines up with the `$XDG_DATA_DIRS` theme search path, so a `/usr/local` install makes the bundled themes discoverable with no extra step.

**Linux packages** (metadata pattern from jira-tui): package `review-buddy`, version = tag without the `v`, licence `MIT`, homepage `https://github.com/smorrisods/review-buddy`, summary `Every pull and merge request, in one quiet queue`, vendor `Liminal HQ`, maintainer `Liminal HQ <contact@liminalhq.ca>`. Debian arch `amd64` / `arm64`; RPM `x86_64` / `aarch64`. They install `/usr/bin/review-buddy`, `/usr/share/man/man1/review-buddy.1.gz` and `/usr/share/review-buddy/themes/`. GNU-linked: `Depends: libc6` on Debian; RPM relies on auto-detected requirements.

## Man page

Generated at build time with `clap_mangen` from one shared `src/cli.rs`, `include!`d by both `main.rs` and `build.rs`. This is jira-tui's pattern, and it makes CLI/man-page drift structurally impossible. The packaging scripts find the newest `*/build/*/out/man` directory next to the release binary, using the OS-aware `stat` helper from jira-tui's script.

## Install scripts

- `scripts/install.sh`, the same as jira-tui's: POSIX `sh`, safe to pipe from `curl`. It detects OS and arch (Linux `amd64`/`arm64` → the musl tarball; macOS → `universal`), resolves the latest tag or `--version`, verifies against `SHA256SUMS` and refuses on a mismatch. It installs into `--prefix` / `$PREFIX` (default `/usr/local`, `sudo` only if needed) and supports `--uninstall`. Colour output respects `NO_COLOR` and non-TTY output, and there's a little Jax in the banner.
- `scripts/install.ps1` **(new)**: the PowerShell equivalent. It detects `AMD64` / `ARM64`, verifies against `SHA256SUMS`, installs to `%LOCALAPPDATA%\Programs\review-buddy\`, adds it to the user `PATH`, supports `-Uninstall`, and runs `Unblock-File` on what it installs.

## Running unsigned builds

**macOS.** The universal binary is ad-hoc signed after `lipo`; Apple Silicon won't run arm64 code without at least that. It is not notarised (no Apple Developer account, the same as jira-tui). If you downloaded through a browser, the first run needs either:

```sh
xattr -d com.apple.quarantine ./review-buddy
```

or right-click → Open in Finder once. `install.sh` downloads with `curl`, which doesn't set the quarantine flag. Its output still prints this note, as jira-tui's does.

**Windows.** Browser-downloaded files carry Mark of the Web, so SmartScreen may say "Windows protected your PC". Choose **More info → Run anyway**, or run `Unblock-File .\review-buddy.exe`. `install.ps1` unblocks for you.

**Linux.** There's nothing to do. Verify with `sha256sum -c SHA256SUMS`.

## CI (same as jira-tui, plus OS coverage)

`ci.yml` keeps jira-tui's shape: no `branches:` filter on `pull_request` (for stacked PRs), `push` on `main`, concurrency per ref. Jobs:

- **Format:** `cargo fmt --all -- --check`
- **Clippy:** `-D warnings`, across a feature matrix (`default`, `no-default-features` (offline / demo mode), `all-features`)
- **Test:** `cargo nextest run --profile ci` with JUnit output and published results
- **Security audit:** `actions-rust-lang/audit`
- **PR summary:** one updatable comment
- **New:** a `test-platforms` job runs the default feature set on `windows-2022` and `macos-14`, so path, keyring and key-chord code is exercised before release day rather than only in the release workflow.

## Platform differences

| Area | Linux | Windows | macOS |
|---|---|---|---|
| Paths | XDG | `%APPDATA%` / `%LOCALAPPDATA%`; XDG variables honoured if set | XDG (not `~/Library`) |
| Secrets | Secret Service (GNOME Keyring, KWallet, KeePassXC) via `keyring` | Credential Manager | Keychain |
| No keyring (SSH, headless) | `auth = "cli"`, `auth = "env:VAR"` or `token_command`. Never plain text | n/a | n/a |
| Open URL | `$BROWSER`, then `xdg-open` | `ShellExecuteW` | `open` |
| Clipboard (`y`) | OSC 52, then `wl-copy` / `xclip` | OSC 52, then Win32 | OSC 52, then `pbcopy` |
| Terminal | any VT; truecolour from `COLORTERM` | Windows Terminal recommended; conhost works but is limited to 256 colours | Terminal.app is 256-colour (roles quantise); iTerm2, Ghostty, WezTerm, kitty and Alacritty give full colour |
| Keys | — | `⌃⏎` needs Windows Terminal; fall back to `alt-enter` | `⌘K` where the terminal passes it through; `⌥` acts as Alt only with "Option as Meta" |
| git | `git` | Git for Windows; paths normalised to `/` for the APIs | `git` (Xcode CLT or Homebrew) |

Platform code lives behind `cfg` in `rb-paths` and `rb-platform`.

## First release checklist (adapted from jira-tui's)

- [ ] `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and `cargo nextest run` pass across all three feature sets
- [ ] `scripts/prepare-release-version.sh --version 0.1.0`; review, then merge the bump PR
- [ ] Tag `v0.1.0` on `main`; let `Release` build and publish
- [ ] Linux, both arches: extract, `sha256sum -c SHA256SUMS`, install the `.deb` / `.rpm` in containers, `review-buddy --demo`, `man review-buddy`, `review-buddy config paths`
- [ ] macOS: `lipo -archs` shows `x86_64 arm64`, `codesign -dv` shows an ad-hoc signature, Gatekeeper behaves as documented, `--demo` runs under Rosetta and natively
- [ ] Windows, both arches: unzip, check SmartScreen behaviour, `install.ps1` then `-Uninstall`, `--demo` in Windows Terminal and conhost
- [ ] `install.sh` against the real release on one Linux and one macOS machine
- [ ] Release notes look right
