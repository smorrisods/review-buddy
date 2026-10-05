# Platforms and releases

Review Buddy uses **the same release process as [smorrisods/jira-tui](https://github.com/smorrisods/jira-tui)**: see its `docs/release/distribution-strategy.md`, `.github/workflows/release.yml`, `.github/release.yml` and `scripts/`. This page says what we reuse as-is and where we extend it. jira-tui ships Linux and macOS only; we add Windows and a universal macOS binary.

Linux is the primary platform. Windows and macOS are supported release targets. **No code signing**: macOS gets an ad-hoc signature only, and Windows is unsigned.

Repo: `smorrisods/review-buddy`. Licence: MIT. Channels for 1.0: **GitHub Releases, `install.sh`, `install.ps1`, `.deb` / `.rpm`**. No Homebrew, Scoop, AUR, Nix, crates.io or winget yet; revisit once the release cadence settles, as jira-tui's doc suggests.

## Release flow (identical to jira-tui)

1. Merge the release-ready PR into `main`.
2. Run `scripts/prepare-release-version.sh --version <next>` in a clean tree. It creates `chore/release-v<next>` and bumps the root `[workspace.package]` version and **only our own** `name = "review-buddy"` stanza in `Cargo.lock`. It fails loudly if the stanza isn't found. `--current-version` and `--dry-run` behave the same as in jira-tui.
3. Open a PR from that branch, wait for green CI, and merge to `main`.
4. Tag `vX.Y.Z` on `main`. The tag must match `^v[0-9]+\.[0-9]+\.[0-9]+$`.
5. The `Release` workflow builds every target and creates or reuses the GitHub Release (generated notes; categories from `.github/release.yml`).
6. It attaches binaries, archives, packages and **one `SHA256SUMS`** covering every artefact.
7. Smoke-test from the published assets (checklist below), then edit the generated notes if needed.

**Manual dispatch** (`workflow_dispatch`) takes `release_tag` (`vX.Y.Z`; required unless `dry_run` is on, where it defaults to `v` plus the workspace version) and `dry_run` (default **true**). A dry run can start from any branch: it skips the main-ancestry check, builds and smoke-tests every target, uploads the artefacts and a `SHA256SUMS` as workflow artefacts, and publishes nothing. A real run (`dry_run` off) needs the tag to match the `Cargo.toml` version and the commit to be on `main`. `publish-release` refuses to modify a release that is already published and only reattaches assets to a draft. Concurrency is grouped per tag, with cancel-in-progress. Uploads use `gh release upload --clobber`, so reruns against a draft are idempotent. GitHub only lists a `workflow_dispatch` workflow once the file exists on the default branch, so the very first dry run happens after the workflow merges.

## Release script commands

All scripts live in `scripts/`; shared helpers are in `scripts/lib/release-common.sh`.

```sh
# Print the workspace version, or bump it (clean tree required; creates chore/release-vX.Y.Z)
scripts/prepare-release-version.sh --current-version
scripts/prepare-release-version.sh --version X.Y.Z --dry-run
scripts/prepare-release-version.sh --version X.Y.Z

# tar.gz archive: dist/review-buddy-X.Y.Z-<target>.tar.gz, where `<target>` is a label such as `linux-amd64-musl`, or use `--output-prefix` (man dir found under target/**/build/review-buddy-*/out/man unless --man-dir is given)
cargo build --release --locked --target x86_64-unknown-linux-musl
scripts/build-release-archive.sh --version vX.Y.Z --target linux-amd64-musl --binary target/x86_64-unknown-linux-musl/release/review-buddy

# .deb and .rpm (--arch amd64|arm64, --libc gnu|musl, --format all|deb|rpm); needs dpkg-deb and rpmbuild
scripts/build-linux-packages.sh --version vX.Y.Z --arch amd64 --binary target/release/review-buddy --output-prefix dist/review-buddy-X.Y.Z-linux-amd64-glibc

# one SHA256SUMS over everything in dist/
scripts/generate-checksums.sh dist
```

On Windows, `scripts\build-release-archive.ps1 -Version vX.Y.Z -Target windows-amd64 -Binary target\x86_64-pc-windows-msvc\release\review-buddy.exe` writes `dist\review-buddy-X.Y.Z-windows-amd64.zip` with `review-buddy.exe`, `themes/`, `config.example.toml`, `LICENSE` and `README.md` at the root.

The version bump edits the `version` in the root `[workspace.package]` table (the binary crate inherits it) and our own `name = "review-buddy"` stanza in `Cargo.lock`. The scripts are covered by `bash scripts/tests/run.sh`, which CI runs in the `scripts` job along with `shellcheck`.

## Workflow jobs

| Job | Runner | What it does |
|---|---|---|
| `prepare-release` | `ubuntu-24.04` | Resolves the tag, checks it matches the workspace `Cargo.toml` version, and (unless a dry run) that the commit is on `main`; writes `$GITHUB_STEP_SUMMARY` |
| `build-linux` | `ubuntu-22.04`, `ubuntu-22.04-arm` | Four builds, each on a native runner: `*-unknown-linux-musl` (`musl-tools`, `CC=musl-gcc`; the standalone binary and `.tar.gz`) and `*-unknown-linux-gnu` (`.deb` and `.rpm` via `scripts/build-linux-packages.sh`). Native builds avoid a zig or `cross` toolchain for aarch64. The musl build uses `keyring` with pure-Rust D-Bus (`zbus`, `crypto-rust`), bundled SQLite and rustls, so it is fully static. The job checks `file` for the architecture, `ldd` for "not a dynamic executable" or "statically linked" (glibc's `ldd` reports static-pie the second way), and runs `--version`; the `.deb` is installed and run too |
| `build-macos` | `macos-14` | Builds `aarch64-apple-darwin` natively and `x86_64-apple-darwin` cross-compiled, runs `lipo -create` into one universal2 binary (`lipo -archs` verified), `codesign --sign - --force`, checks `codesign -dv` reports an ad-hoc signature, and smoke-tests `--version` (also under Rosetta when available) |
| `build-windows` | `windows-2022`, `windows-11-arm` | `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`, each built natively. Standalone `.exe` and a `.zip` via `scripts/build-release-archive.ps1`; `--version` is run on the binary and on the unzipped copy (`--demo` needs a terminal, so it isn't run in CI) |
| `publish-release` | `ubuntu-24.04` | Downloads every `review-buddy-*` artefact, writes a single `SHA256SUMS` with `scripts/generate-checksums.sh` and verifies it, then (skipped on a dry run) creates the release with generated notes from `.github/release.yml` and uploads everything |

Fixed runner images, not `-latest`, as in jira-tui. The glibc packages build on Ubuntu 22.04, so they need **glibc ≥ 2.35** (Ubuntu 22.04+, Debian 12+, RHEL 9+, current WSL2). The musl tarballs have no glibc requirement. Caching uses `Swatinem/rust-cache@v2` with per-target keys and a shared key per OS.

## Artefacts

Names are `review-buddy-<version>-<os>-<arch>[-<libc>][.ext]`, where `<version>` is the tag without the `v`.

| Asset | Linux amd64 | Linux arm64 | macOS universal2 | Windows amd64 | Windows arm64 |
|---|---|---|---|---|---|
| Standalone binary | `…-linux-amd64-musl` | `…-linux-arm64-musl` | `…-macos-universal2` | `…-windows-amd64.exe` | `…-windows-arm64.exe` |
| Archive | `…-linux-amd64-musl.tar.gz` | `…-linux-arm64-musl.tar.gz` | `…-macos-universal2.tar.gz` | `…-windows-amd64.zip` | `…-windows-arm64.zip` |
| Packages | `…-linux-amd64-glibc.deb` / `.rpm` | `…-linux-arm64-glibc.deb` / `.rpm` | — | — | — |

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

- `scripts/install.sh`, the same as jira-tui's: POSIX `sh`, safe to pipe from `curl`. It detects OS and arch (Linux `amd64`/`arm64` → the musl tarball; macOS → `universal2`), resolves the latest tag or `--version`, verifies against `SHA256SUMS` and refuses on a mismatch. It installs into `--prefix` / `$PREFIX` (default `/usr/local`, `sudo` only if needed) and supports `--uninstall`. Colour output respects `NO_COLOR` and non-TTY output, and there's a little Jax in the banner.
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
