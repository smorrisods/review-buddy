# Testing

How the tests are organised, and how to review and accept snapshot changes. Run everything with `cargo test --workspace`.

## Frame snapshots

`crates/review-buddy/tests/frames.rs` renders the canonical demo frames headlessly with ratatui's `TestBackend` and pins them with `insta`:

- **1a Three panes**: the dashboard with the queue, detail and sources visible.
- **1d Diff**: the unified diff of the first demo change, the cursor on a changed line with the existing thread shown beneath it.
- **1f Diff with the composer**: the composer docked under a line that already has a pending suggestion, with some text typed.

Each frame is rendered in `liminal-hq` (the default, transparent background), `dusk` and `afterglow-dark`, at 160×40 and 100×30. A `NO_COLOR` variant of each frame (in `liminal-hq`, at both sizes) snapshots the screen text plus a dump of the bold, dim and reverse runs, so a change to the signals that survive without colour shows up in review. Colour is asserted directly in the same file: the selected row's rule and the composer border use `accent`, added and removed lines carry `added_bg` and `removed_bg`, the wordmark runs the theme's gradient, and `liminal-hq` leaves the background unset.

The frames are deterministic. They use the demo data at its frozen time, a fixed size and truecolour forced in the `AppConfig`, so nothing depends on the terminal, the environment or the clock. Snapshots are plain text, stored with LF line endings (`.gitattributes` marks `*.snap` as `eol=lf`), so they match on Windows too. The older per-screen snapshots in `dashboard.rs`, `diff.rs`, `composer.rs`, `help.rs` and `render.rs` cover other sizes, states and screens, and stay.

## Reviewing and accepting snapshot changes

A failing snapshot prints a diff of the old and new frame. To review the changes interactively, install `cargo-insta` once (`cargo install cargo-insta`) and run:

```sh
cargo insta review
```

It steps through each pending change, and you accept or reject each one. Without `cargo-insta`, the test run writes the new frame beside the old one as `*.snap.new`; compare the two files, then rename the new one over the old one to accept it, or delete it to reject it.

To accept every change at once, which is only appropriate after you've read the diff, re-run the tests with:

```sh
INSTA_UPDATE=always cargo test -p review-buddy
```

`INSTA_UPDATE=no` never writes anything, and CI runs with `INSTA_UPDATE=no` semantics (`CI` is set), so a changed frame fails the build instead of being rewritten. Commit the updated `.snap` files with the change that caused them.

## Snapshot hygiene

`tests/snapshot_hygiene.rs` is a cheap file scan that fails when a `.snap.new` or `.pending-snap` file exists anywhere in the crate, or when a file in `tests/snapshots` has no test that still produces it (its `<test file>__<name>.snap` name must point at an existing `tests/<test file>.rs` that mentions `<name>`). When you rename or delete a snapshot test, delete its `.snap` file too.
