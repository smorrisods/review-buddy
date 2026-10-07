# Terminal pane

**What worked:** keeping the emulator behind one file (`rb-term/src/emulator.rs`) and giving the app only a `Screen` snapshot, a few events and "bytes to write back". That made the pane testable three ways without a real terminal: unit tests on the boundary, `update` tests that feed `Msg::Term(Output)`, and a scripted pane for demo mode that uses the same emulator. Letting the `App` own the emulator and the runtime own only the PTY kept `update` pure (parsing is CPU work, not I/O), and a generation number on each child made "output from a closed pane" a non-problem. Running a real shell inside the real binary inside a pty (`tests/pty_terminal.rs`) found two bugs unit tests could not.

**Friction:**

- `portable-pty` starts a child in the home directory when no working directory is set, so "use the current directory" opened a shell in `$HOME`. The pty test caught it; `Pty::spawn` now passes `current_dir()` when the caller gives none.
- `git remote -v` prints a URL after `insteadOf` has rewritten it, so a clone whose `origin` is rewritten to a local path never matched its repository. Matching now also reads `remote.<name>.url` as written. The same quirk is what lets the worktree test fetch from a local bare repository with no network.
- A legacy terminal reports `Ctrl-\` as `Ctrl-4` (the byte is 0x1c), so the escape chord compares control codes, not key names.
- The brief mentioned `App::kitty_keys`; it did not exist in this tree. It is now an `App` field, seeded from the environment (`rb_term::host_supports_kitty`) and raised by the first key only the kitty protocol can produce. A startup `supports_keyboard_enhancement()` query would have been exact but waits on a reply that the `vt100`-hosted pty tests never send.
- The shared cargo target directory gave a stale "method not found" for a function that existed; touching the crate roots and rebuilding once fixed it, as the brief said.

**Decisions:**

- `alacritty_terminal` is pinned with `=0.26.0` (Apache-2.0, MSRV 1.85, which fits the workspace's MIT licence and MSRV 1.87). There is no `deny.toml`; CI runs `cargo-audit` only.
- The pane is a dock around the body of the Dashboard and Diff screens rather than a new `SeamKind`, so existing `layout::Options` literals and the resize code stay untouched. The four screens' layout code reads `app::terminal::app_body` instead of `layout::body(app.size)`.
- Legacy Enter and Shift-Enter both send a carriage return; Shift-Enter is only distinguished under the kitty encoding.
- Focus-in and focus-out reports are not forwarded to the child yet.
- Windows: ConPTY goes through `portable-pty` and `CSI ? 9001 l` is written on pane close and on terminal restore, but nothing here can run it. The PR says so.
