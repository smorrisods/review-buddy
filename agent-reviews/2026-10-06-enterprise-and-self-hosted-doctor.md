# Enterprise and self-hosted forges, and a fuller doctor

Date: 2026-10-06. Issue #98.

**What worked.** Auditing URL construction first found the real bugs quickly: `web_url` ignored the `api_url` scheme, port and path prefix; GitHub URL selectors rejected a path prefix; remotes and URLs disagreed about ports. Deriving the web base from the API base (one helper in `rb-core::http`) fixed all the link paths at once. A `Versions`, `Endpoints` and `Probe` trio in `cmd::doctor` keeps probing separate from rendering.

**Friction.** Worktrees share one target directory, and cargo hashes workspace crates by relative path, so two worktrees building `rb-core` overwrite each other's artefacts (and the shared `review-buddy` binary). The symptom was `cannot find http in rb_core` right after a successful build, and a test binary that printed another branch's output. Setting `RUSTFLAGS="--cfg wt<issue>"` per command gave this branch its own artefacts without a separate target directory. The shared binary can still flip under a running test, so a surprising failure in an integration test is worth one rerun.

**Decision.** reqwest is built with bundled public roots, so the TLS hint asks for a publicly trusted certificate rather than telling people to trust a private authority on their machine, which wouldn't work. Switching to native roots is a separate change.
