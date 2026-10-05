# 2026-10-05: pr view, pr open and open

**What worked.** Building on the CLI core meant the new command was mostly resolve, call the provider, format. Keeping `execute` returning an `Outcome` (stdout, stderr note, paged flag) made the browser and pager paths testable without spawning anything, and the browser opener goes through an injected `CommandRunner`.

**Friction.** In demo mode a bare `liminal-hq/review-buddy` repository matches two GitHub sources (`liminal-hq` and the user-scoped `smorris`), so selectors report an ambiguity unless `-s` is given. The tests pass `-s liminal-hq`; the selector's `covers` rule for user-scoped sources may deserve a look. Running the binary in the agent shell also hung on the pager until stdout was piped, which is the intended TTY behaviour.

**Decisions.** The Markdown renderer is a small `pulldown-cmark` walker in `cmd/markdown.rs`. `open` is handled inside `cmd::execute` and starts the TUI through a new `RunOptions::open`, which switches to the diff after the first load.
