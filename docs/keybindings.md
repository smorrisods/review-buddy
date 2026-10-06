# Key bindings

Review Buddy uses arrows plus mnemonic letters. Vim-style `j`/`k` also work for movement in lists. Every binding can be changed under `[keys]` in `config.toml`. Arrows, `tab`, `⏎` and `esc` always work, even if you rebind something on top of them.

Notation: `⌃` Ctrl · `⇧` Shift · `⏎` Enter. On macOS, `⌘K` also opens the palette.

## Global

| Key | Action |
|---|---|
| `⌃K` | Command palette |
| `/` | Search all sources |
| `tab` / `⇧tab` | Next / previous pane (the focused pane gets the accent border) |
| `1`–`9` | Switch source (`1` is always All) |
| `,` | Settings |
| `L` | Cycle layout: three panes → list + diff → one at a time |
| `T` | Cycle theme |
| `J` | Toggle Jax |
| `r` | Refresh now |
| `?` | Help overlay listing the keys for the current screen (`esc` or `?` closes it; it is built from the same registry as the footer hints) |
| `o` / `y` | Open the current change in the browser / copy its URL. In the diff they use the change's files page. Under `--demo` nothing is opened: the footer says `Would open <url> (demo)` |
| `q` | Quit from the queue; in the diff, go back (asks first if there are unsent drafts) |
| `esc` | Close overlay → clear selection → back one screen |

## Dashboard

| Key | Action |
|---|---|
| `↑↓` / `j k` | Move in the focused pane |
| `g` `G` | Jump to the first / last row in the focused pane |
| `h` `l` | Previous / next pane (same as `⇧tab` / `tab`) |
| `⏎` on the Noise row | Expand or collapse the bot updates |
| `⏎` / `d` | Open the diff for the selected change |
| `[` `]` | Previous / next detail tab (Overview, Files, Checks, Conversation) |
| `← →` | Same as `[` `]` when the detail pane is focused |
| `space` | Toggle the Show filter under the cursor (Sources pane) |
| `a` | Approve |
| `x` | Request changes (opens the composer for the required summary) |
| `c` | Comment on the change |
| `m` | Merge… (confirm defaults to No) |
| `R` | Re-run failed CI |
| `b` | Check out the branch locally |
| `o` | Open in browser |
| `y` | Copy the change URL |

### One at a time (1c)

| Key | Action |
|---|---|
| `a` | Approve and move to the next change |
| `n` | Skip for now (next) |
| `p` | Previous |

## Diff

| Key | Action |
|---|---|
| `tab` | Switch focus between Files and Diff |
| `↑↓` / `j` `k` | Files pane: choose a file · Diff pane: move the line cursor |
| `g` `G` | First / last line (Files pane: first / last file) |
| `PgUp` `PgDn` · `⌃U` `⌃D` | Move a page or half a page |
| `⏎` | Files pane: open the file · on a thread marker: expand it |
| `⇧↑↓` | Extend the line range |
| `V` | Toggle the range anchor at the cursor |
| `esc` / `q` | Clear the range, then go back to the dashboard |
| `{` `}` · `p` `n` | Previous / next hunk |
| `[` `]` | Previous / next file |
| `v` | Unified ↔ side by side |
| `w` | Toggle whitespace-only changes |
| `c` | Comment on the line or range |
| `s` | Comment with a suggestion prefilled from the line or range |
| `r` | Reply to the thread at the cursor |
| `e` | Resolve / unresolve the thread at the cursor |
| `f` | Mark the file as viewed |
| `a` | Approve: opens a preview (`Approve with N comments`, the pending comments listed, the verdict) with **Cancel** and **Approve**. `⏎` confirms the focused button (Approve by default), `esc` cancels. On success your pending comments are cleared and a toast says `Approved` (with `(demo)` under `--demo`); on failure they stay pending and the toast says what to do next |
| `x` | Request changes. Not built yet; where the forge can't do it at all the footer says so and points to `c` |
| `b` / `o` | Check out / open in browser |

**Mouse:** click to place the cursor · press and drag to select a range · shift-click to extend it · click a file to open it · click the Files pane or its review block to focus it · the wheel scrolls the pane under the pointer · `esc` clears a selected range before it leaves the diff. See [Mouse](#mouse) for the full list.

## Composer

| Key | Action |
|---|---|
| `⌃S` | Insert a ` ```suggestion ` block from the selected lines |
| `⏎` | Add to your pending review. On a reply, post the reply |
| `⌃⏎` | Post now as a standalone comment, after a preview (turn the preview off with `review.confirm_post_now = false`). `⌃P` does the same on terminals that can't tell `⌃⏎` from `⏎` |
| `⇧⏎` · `⌥⏎` · `⌃J` | Newline |
| `← → ↑ ↓` · `home` `end` | Move the cursor |
| `⌥←` `⌥→` · `⌃←` `⌃→` · `⌥b` `⌥f` | Move by word |
| `⌃home` `⌃end` | Start / end of the draft |
| `⌫` · `del` · `⌃W` | Delete back / forward / the word before the cursor |
| paste | Bracketed paste inserts the text as typed |
| `⌃E` | Edit the draft in `$EDITOR` |
| `esc` | Close. If you've typed anything it asks first, and the answer defaults to **No, keep editing** |

The composer is docked at the bottom of the diff and opens with `c` on the cursor line, or with `r` on a line that has a thread (`r` replies to that thread). Drafts live in memory for now; they are not saved between sessions, and quitting asks first if any are unsent. `⌃⏎` and `⇧⏎` need a terminal that reports those modifiers (kitty, WezTerm, foot, Ghostty and recent iTerm2 do); `⌥⏎`, `⌃J` and `⌃P` work everywhere.

## Command palette and search

| Key | Action |
|---|---|
| type | Filter (fuzzy) |
| `↑↓` | Choose |
| `⏎` | Run the command / open the change |
| `⌫` | Delete a character |
| `esc` | Close |

## Preview and discard confirms

The same two-button modal is used for approving, posting now, and discarding a draft.

| Key | Action |
|---|---|
| `← →` / `tab` | Switch buttons. Approve and post now start on the action; discard starts on **No, keep editing** |
| `y` / `n` | Answer yes / no directly |
| `⏎` | Confirm the highlighted button |
| `esc` | Cancel |

## Merge confirm

| Key | Action |
|---|---|
| `← →` / `tab` | Switch between **No, not yet** (default) and **Merge** |
| `⏎` | Confirm the highlighted button |
| `esc` | Cancel |

## First run

| Key | Action |
|---|---|
| `↑↓` | Move between sources |
| `⏎` | Add a token / test and save / continue |
| `← →` / `t` | Change theme (live preview) |
| `J` | Toggle Jax |
| `esc` | Skip for now |

## Settings

| Key | Action |
|---|---|
| `tab` | Switch between nav and content |
| `↑↓` | Nav: section · Content: rows (Theme: preview each theme) |
| `space` | Toggle / enable |
| `e` / `t` | Edit / test a token |
| `n` / `del` | New source / remove (asks first) |
| `esc` / `,` | Back to the queue |

## Mouse

Every mouse target is registered while drawing and resolved in `update`, so it behaves like the key it mirrors. Turn the mouse off with `ui.mouse = false`; then the terminal's own selection and wheel work everywhere.

| Where | What it does |
|---|---|
| Top bar theme name | Cycles the theme (`T`) |
| Footer hints | Run the hint's action |
| Sources rows, or the source tabs when Sources is collapsed | Show that source |
| Queue row | Selects it. A double-click (two clicks within about half a second) opens its diff |
| Tabs (Overview, Files, Checks, Conversation) | Switch the detail view |
| Action chips (Approve, Request changes, Comment, Diff) | Same as their keys |
| Any pane | Click empty space to focus it. The wheel scrolls the pane under the pointer, whichever is focused |
| Diff: files | Click a file to open it |
| Diff: rows and thread blocks | Click to place the cursor; a block puts it on the line it hangs from |
| Diff: press and drag | Selects a range of lines, shown with the selection colour and a `▌` marker. Dragging past the top or bottom scrolls. Groundwork only: nothing is commented on yet |
| Diff: shift-click | Extends the range from the cursor (or the range's anchor) to the line clicked |
| Composer text | Click to place the caret. The wheel moves the caret a line. Clicks outside the composer are ignored and never discard the draft |
| Preview and discard confirms | Click **Cancel** or the confirm button. Clicks outside are ignored |
| Toast | Click to dismiss |
| Help overlay | The wheel scrolls it; a click closes it |

**Shift always belongs to the terminal.** With mouse capture on, terminals keep Shift-drag for their own text selection and don't send it to the app. If a terminal forwards Shift events anyway, Review Buddy ignores all of them except a Shift-click in the diff, which extends the selected range. Shift-drag and Shift-wheel are never consumed.

## Remapping

```toml
[keys]
approve         = "a"
request_changes = "x"
comment         = "c"
suggest         = "s"
merge           = "m"
rerun_ci        = "R"
checkout        = "b"
open_browser    = "o"
palette         = "ctrl-k"
search          = "/"
toggle_view     = "v"
insert_suggestion = "ctrl-s"
```

Key strings follow crossterm names: `ctrl-`, `alt-`, `shift-` prefixes; `enter`, `esc`, `tab`, `backspace`, `up`, `down`, `left`, `right`, `space`, `f1`–`f12`.
