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

**Manual dispatch** (`workflow_dispatch`) takes `release_tag` (`vX.Y.Z`; required unless `dry_run` is on, where it defaults to `v` plus the workspace version) and `dry_run` (default **true**). A dry run can start from any branch: it skips the main-ancestry check, builds and smoke-tests every target, uploads the artefacts and a `SHA256SUMS` as workflow artefacts, and publishes nothing. A real run (`dry_run` off) needs the tag to match the `Cargo.toml` version and the commit to be on `main`. `publish-release` refuses to modify a release that is already published and only reattaches assets to a draft. Concurrency is grouped per tag, with cancel-in-progress. Uploads use `gh release upload --clobber`, so reruns against a draft are idempotent. A dry run from `main` has already succeeded, so the workflow is listed and works.

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
share/bash-completion/completions/review-buddy   (not in the Windows zip)
share/zsh/site-functions/_review-buddy           (not in the Windows zip)
share/fish/vendor_completions.d/review-buddy.fish (not in the Windows zip)
share/review-buddy/themes/*.toml
share/review-buddy/config.example.toml
share/doc/review-buddy/LICENSE
```

`share/review-buddy/themes/` lines up with the `$XDG_DATA_DIRS` theme search path, so a `/usr/local` install makes the bundled themes discoverable with no extra step.

**Linux packages** (metadata pattern from jira-tui): package `review-buddy`, version = tag without the `v`, licence `MIT`, homepage `https://github.com/smorrisods/review-buddy`, summary `Every pull and merge request, in one quiet queue`, vendor `Liminal HQ`, maintainer `Liminal HQ <contact@liminalhq.ca>`. Debian arch `amd64` / `arm64`; RPM `x86_64` / `aarch64`. They install `/usr/bin/review-buddy`, `/usr/share/man/man1/review-buddy.1.gz`, the bash, zsh and fish completions and `/usr/share/review-buddy/themes/`. GNU-linked: `Depends: libc6` on Debian; RPM relies on auto-detected requirements.

## Man page

Generated at build time with `clap_mangen` from one shared `src/cli.rs`, `include!`d by both `main.rs` and `build.rs`. This is jira-tui's pattern, and it makes CLI/man-page drift structurally impossible. The packaging scripts find the newest `*/build/*/out/man` directory next to the release binary, using the OS-aware `stat` helper from jira-tui's script.

## Shell completions

`build.rs` also writes `review-buddy.bash`, `_review-buddy`, `review-buddy.fish`, `review-buddy.elv` and `_review-buddy.ps1` to `OUT_DIR/completions` with `clap_complete`, from the same `cli.rs`. The packaging scripts find them like the man page (`--completions-dir`, else the `completions` directory beside `--man-dir`, else the newest `*/build/review-buddy-*/out/completions` next to the binary). Archives and `.deb`/`.rpm` ship bash, zsh and fish; elvish and PowerShell users, including the Windows zip, run `review-buddy completion <shell>` instead. The binary prints the same scripts without touching config or the network.

## Install scripts

- `scripts/install.sh`: POSIX `sh`, safe to pipe from `curl`. It detects OS and arch (Linux `amd64`/`arm64` → the musl tarball; macOS → `universal2`), resolves the latest tag or `--version`, verifies against `SHA256SUMS` and refuses on a mismatch. It installs into `--prefix` / `$PREFIX` (default `/usr/local`, falling back to `~/.local` with a note when that isn't writable and you aren't root; it never runs `sudo` itself and prints the command instead). It also supports `--libc glibc`, `--dry-run` and `--uninstall`, which removes exactly the files listed in `share/review-buddy/install-manifest`. Man page, themes and any completions in the archive are installed alongside the binary. `GITHUB_TOKEN` is used for the release lookup only if set. For tests, `RB_INSTALL_BASE_URL` (a directory or `file://` URL holding the assets and a `VERSION` file), `RB_INSTALL_DEFAULT_PREFIX`, `RB_INSTALL_UNAME_S` and `RB_INSTALL_UNAME_M` override discovery; they're testing-only and not a supported interface. Colour output respects `NO_COLOR` and non-TTY output, and there's a little Jax in the banner.
- `scripts/install.ps1`: the PowerShell equivalent (5.1 and 7), with `-Version`, `-InstallDir`, `-DryRun`, `-Yes` and `-Uninstall`. Without `-Yes` the PATH change is only offered in an interactive session, defaulting to No. It detects `AMD64` / `ARM64`, verifies against `SHA256SUMS`, installs to `%LOCALAPPDATA%\Programs\review-buddy\`, adds it to the user `PATH`, supports `-Uninstall`, and runs `Unblock-File` on what it installs.

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

## First release checklist (v0.1.0)

Adapted from jira-tui's. Tagging and publishing are a deliberate, separate step after the milestone review; everything above the tag line can be done and re-run safely.

### 1. Pre-flight (on a clean `main`)

- [ ] The milestone's issues are closed and the docs match behaviour (`README.md`, `docs/`, `CHANGELOG.md`)
- [ ] CI is green on `main` (format, clippy and test across the three feature sets, `test-platforms`, `scripts`, audit)
- [ ] The same checks pass locally:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace                      # or: cargo test --workspace
cargo nextest run --workspace --no-default-features
cargo nextest run --workspace --all-features
bash scripts/tests/run.sh                          # the release and install script tests
```

- [ ] `cargo tree -p rb-github | grep -i -E 'openssl|native-tls'` prints nothing, so the musl build stays static
- [ ] `target/release/review-buddy --demo` opens and `review-buddy --version` prints the version you expect

### 2. Version bump

- [ ] `scripts/prepare-release-version.sh --version 0.1.0 --dry-run`, then without `--dry-run` in a clean tree. It creates `chore/release-v0.1.0` and bumps the workspace `version` and our `Cargo.lock` stanza
- [ ] In `CHANGELOG.md`, rename `Unreleased` to `0.1.0 - <date>` and add a fresh empty `Unreleased` heading above it, in the same branch
- [ ] Open the PR (label `release`), wait for green CI and merge it with a merge commit

### 3. Dry run from `main`

- [ ] Actions → Release → Run workflow on `main` with `dry_run` on (the default) and `release_tag` empty, or `gh workflow run release.yml --ref main -f dry_run=true`
- [ ] Every job is green. Download the workflow artefacts and check `SHA256SUMS` against them: `sha256sum -c SHA256SUMS --ignore-missing`
- [ ] Spot-check one musl tarball: `tar tzf` shows the layout under [Artefacts](#artefacts), including the man page, the three completions, `themes/`, `config.example.toml` and `LICENSE`

### 4. Tag (only after the milestone review)

- [ ] On `main`: `git tag v0.1.0 && git push origin v0.1.0`. The tag must match the `Cargo.toml` version and the commit must be on `main`
- [ ] The `Release` workflow publishes the release with generated notes and one `SHA256SUMS`

### 5. Post-release verification

- [ ] **Checksums:** download the assets and `SHA256SUMS`, then `sha256sum -c SHA256SUMS`
- [ ] **Static check (Linux, both arches):** extract each musl tarball. `file bin/review-buddy` names the right architecture, and `ldd bin/review-buddy` says `not a dynamic executable` (or `statically linked`)
- [ ] **`--version`:** `review-buddy --version` prints `review-buddy 0.1.0` from each Linux tarball, the macOS binary and the Windows `.exe`
- [ ] **Linux install script:** `curl -fsSL https://raw.githubusercontent.com/smorrisods/review-buddy/main/scripts/install.sh | sh`, then `review-buddy --version`, `man review-buddy`, `review-buddy config paths` and `review-buddy --demo`. Run it once with `--prefix "$HOME/.local"` and once with `--uninstall` afterwards
- [ ] **Linux packages:** install the `.deb` in a Debian or Ubuntu 22.04+ container and the `.rpm` in a Fedora or RHEL 9+ container on both arches, and run `review-buddy --version` and `review-buddy --demo`
- [ ] **macOS:** the install script on one Mac; `lipo -archs $(which review-buddy)` shows `x86_64 arm64`, `codesign -dv` shows an ad-hoc signature, and `--demo` runs natively and under Rosetta. Check the Gatekeeper behaviour for a browser download is as documented
- [ ] **Windows (both arches where available):** run `install.ps1` from a PowerShell prompt, check `review-buddy --version`, then `install.ps1 -Uninstall`. Check the SmartScreen behaviour for a browser download, and `--demo` in Windows Terminal and conhost
- [ ] **Completions:** `review-buddy completion bash | head` works, and the installed bash, zsh and fish completions load
- [ ] The release notes look right; edit the generated notes if needed

### Known gaps for v0.1.0

- **PowerShell in CI:** nothing in CI lints or runs `install.ps1` or `build-release-archive.ps1` yet. A `pwsh` check is pending, because adding it means editing `.github/workflows/`, which needs a token with workflow scope. Until it lands, the Windows steps above are the only check
- **The real publish path is untested:** dry runs build and verify everything, but the step that creates the GitHub Release (`publish-release` with `dry_run` off) has not run. The first real tag is its first test, so keep an eye on it, and remember it refuses to modify a release that is already published
- **glibc has only `.deb` and `.rpm`:** there is no glibc tarball. The musl tarballs are static and run on any Linux. `install.sh --libc glibc` asks for a glibc tarball that isn't published, so it will fail until one is added
- **No signing or notarisation:** macOS is ad-hoc signed only and Windows is unsigned, as described under [Running unsigned builds](#running-unsigned-builds)
- **Bundled themes aren't read from disk yet:** archives and packages install `themes/` where the `$XDG_DATA_DIRS` search path will find it, but 0.1 only loads the built-in themes
