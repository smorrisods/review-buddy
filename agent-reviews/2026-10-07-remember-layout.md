# 2026-10-07: remember the layout, rotate Detail counter-clockwise

**Worked:** a pure `Tracker` that diffs layout snapshots after each `update` kept the key handlers untouched, and made the debounce unit-testable. Seeding the precedence in `Settings::with_session` kept env and config layering in one place.

**Friction:** `auto` at wide sizes already looks like `right`, so "auto then right" would not move anything. From `auto` the rotation goes to the successor of the resolved place, so at wide sizes `auto` leads to `top`. Documented in the keybindings table. One `pty_show` run flaked under a loaded machine and passed on rerun.
