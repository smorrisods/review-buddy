# 2026-10-06 Settings → Sources

**Worked**
- Splitting the screen into a pure state machine (`settings::state`), effects behind injected services (`settings::effects`) and config edits (`settings::edit`) made nearly everything unit-testable without a terminal.
- Reusing first run's `probe` for a typed token meant the test-then-keep-in-keyring behaviour came for free, and `check_source` from `auth status` gave the token check with scopes and expiry.
- A small screen grid in the pty test (replaying cursor moves) made text assertions reliable. Matching raw pty output fails because only changed cells are written.

**Friction**
- Layers replace `[[source]]` lists wholesale, so the spec's "write an override" can't work for sources from `config.d` or `$XDG_CONFIG_DIRS`. Settings shows them read-only with their origin and says where to change them. Documented in SPEC §4.7 and `docs/configuration.md`.
- Worktrees share a `.shared-target` that produced a stale `rb-github` build; building with a per-worktree `CARGO_TARGET_DIR` fixed it.
- The first pty run raced the settings load: a dashboard toast already contained `ghe.test`, so the test pressed `t` before the table existed. Wait for something only the loaded table shows.
