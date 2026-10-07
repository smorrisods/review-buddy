# Resizable panes

**What worked:** keeping the sizes in `layout::Options` (as `Split`) meant drawing, scrolling and hit-testing all picked up an override with no extra plumbing. The seam hit targets come from the same layout function, so they can't drift from what is drawn.

**Friction:** `[` and `]` were already the Detail tab keys, so the resize keys are `<` and `>`. In the pty test a click inside Detail moves focus there, which made `>` act on the wrong pane until the test clicked the Queue first.

**Decisions:** a double-click resets to automatic, not to the config value. `W` writes both Queue sizes and removes a key whose size is automatic.
