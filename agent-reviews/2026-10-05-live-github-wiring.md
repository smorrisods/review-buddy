# Live GitHub wiring (issue 70), 2026-10-05

**What worked**

- Keeping `update` pure paid off: cache-first launch is one `Msg::Cached` (paint, then ask for a refresh), per-source results are `Msg::SourceLoaded`, and lazy details are a post-step in `update` that emits `Cmd::LoadInfo` the first time a change is selected. All of it is unit-testable without a runtime.
- Sharing `load::fetch_info` and `load::fetch_diff` between the demo world and the live backend means both paths run the same `Provider` calls.
- Injecting the runner, secret store and environment lookup into the factory made every auth mode testable without spawning `gh` or touching a keyring.

**Friction**

- The runtime was built with `enable_time()` only, so the first real request panicked in reqwest ("IO is disabled"). The pty test against a stub server caught it; the runtime now uses `enable_all()`.
- The terminal diffing writer splits strings with cursor moves, so pty assertions need short needles that were drawn in one run (`sign-in needed`) rather than whole sentences.
