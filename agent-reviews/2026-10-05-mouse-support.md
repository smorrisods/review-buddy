# Mouse support (issue 24)

**What worked**

- Most targets were already registered through `HitMap`, so the audit was mostly about the gaps: the composer, confirm buttons, pane focus in the diff, drag ranges, double-click and the help wheel.
- Finding click positions by searching the rendered buffer for text kept the headless tests honest about what draw registered. The pty test does the same with an in-process render to learn where to click.

**Friction**

- `update` has no clock, so a double-click is measured in ticks (250 ms each). It is coarse, so the window is documented as about half a second.
- A drag aimed at rows inside a thread block collapses to a single line, because blocks resolve to the line they hang from. Tests aim at plain diff rows.
- Terminal output is a stream of cell updates, so pty needles must be text a redraw emits contiguously. Syntax-coloured code splits across escape sequences, so a plain span such as a line number made a better needle.
