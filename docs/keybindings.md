# Key bindings

Review Buddy uses arrows plus mnemonic letters. Vim-style `j`/`k` also work for movement. Arrows, `tab`, `⏎` and `esc` always work.

**Status (v0.1.0).** This page has two halves. The first covers what the 0.1 binary does today, and it matches the registry in `ui::chrome` (the footer hints and the `?` overlay are built from it) and the key handlers in `app/`. [Planned keys](#planned-keys) lists what the spec describes for later milestones. Press `?` in the app for the keys on the current screen.

Notation: `⌃` Ctrl · `⇧` Shift · `⌥` Alt · `⏎` Enter.

## Dashboard

| Key | Action |
|---|---|
| `↑↓` / `j` `k` | Move in the focused pane |
| `g` `G` / `home` `end` | Jump to the first / last row in the focused pane |
| `tab` / `⇧tab` | Next / previous pane |
| `h` `l` | Previous / next pane |
| `← →` | Previous / next detail tab when the detail pane is focused, otherwise previous / next pane |
| `[` `]` | Previous / next detail tab (Overview, Files, Checks, Conversation) |
| `1`–`9` | Switch source (`1` is All, then sources in config order) |
| `⏎` / `d` | Open the diff for the selected change |
| `⏎` on the Noise row | Expand or collapse the bot updates |
| `⏎` on a `+N more` row | Show the rest of that bucket, past `triage.bucket_limit`. `⏎` again on the `show fewer` row collapses it |
| `s` | Open the Show filters control: `reviewing`, `assigned`, `authored`, `drafts` and `noise`. Changes apply to the queue at once and last for the session |
| `r` | Refresh now (live sources; also refreshes when the terminal regains focus, if `refresh.on_focus` is on) |
| `o` / `y` | Open the change in the browser / copy its URL. Under `--demo` nothing is opened: the footer says `Would open <url> (demo)` |
| `T` | Cycle theme |
| `?` | Help overlay for the current screen (`esc` or `?` closes it) |
| `q` / `⌃C` | Quit. If there are unsent comments, it asks you to quit again |
| `esc` | Dismiss status messages |

The action chips in the detail pane (`a Approve`, `x Request changes`, `c Comment`, `m Merge`) are clickable. `a` and `c` (or their chips) open the diff for the selected change and start the approve preview or the comment composer there, with the cursor on the first changed line. `x` and `m` explain that they are planned for v0.3.

## Diff

Open with `⏎` or `d` from the dashboard.

| Key | Action |
|---|---|
| `tab` / `⇧tab` | Switch focus between Files and Diff |
| `↑↓` / `j` `k` | Files pane: choose a file · Diff pane: move the line cursor |
| `g` `G` / `home` `end` | First / last line (Files pane: first / last file) |
| `PgUp` `PgDn` · `⌃U` `⌃D` | Move a page or half a page |
| `⏎` | Files pane: focus the diff |
| `n` `}` / `p` `{` | Next / previous hunk |
| `]` / `[` | Next / previous file |
| `c` | Comment on the cursor line, or on the selected range (`start_line` to `line`, on the side of the last line) after a drag or shift-click; the selection clears once the comment is added |
| `r` | Reply to the thread at the cursor |
| `a` | Approve: opens a preview (`Approve with N comments`, the pending comments listed, the verdict) with **Cancel** and **Approve**. On success your pending comments are cleared and a toast says `Approved` (with `(demo)` under `--demo`); on failure they stay pending and the toast says what to do next |
| `x` | Request changes. Not built yet: the footer says so and points to `a` and `c`. Where the forge can't do it at all, it says that instead |
| `o` / `y` | Open the change's files page in the browser / copy its URL |
| `T` / `?` | Cycle theme / help |
| `esc` / `q` | Clear the selected range, then go back to the dashboard (`q` also asks first if there are unsent drafts) |

**Mouse:** click to place the cursor · press and drag to select a range · shift-click to extend it · click a file to open it · the wheel scrolls the pane under the pointer. See [Mouse](#mouse).

## Composer

Docked at the bottom of the diff. It opens with `c` on the cursor line, or with `r` on a line that has a thread.

| Key | Action |
|---|---|
| `⏎` | Add to your pending review. On a reply, post the reply |
| `⌃⏎` | Post now as a standalone comment, after a preview (turn the preview off with `review.confirm_post_now = false`). `⌃P` does the same on terminals that can't tell `⌃⏎` from `⏎` |
| `⇧⏎` · `⌥⏎` · `⌃J` | Newline |
| `← → ↑ ↓` · `home` `end` | Move the cursor |
| `⌥←` `⌥→` · `⌃←` `⌃→` · `⌥b` `⌥f` | Move by word |
| `⌃home` `⌃end` | Start / end of the draft |
| `⌫` · `del` · `⌃W` · `⌃⌫` · `⌥⌫` | Delete back / forward / the word before the cursor |
| `tab` | Insert a tab |
| paste | Bracketed paste inserts the text as typed |
| `esc` | Close. If you've typed anything it asks first, and the answer defaults to **No, keep editing** |

Drafts live in memory for now; they are not saved between sessions, and quitting asks first if any are unsent. `⌃⏎` and `⇧⏎` need a terminal that reports those modifiers (kitty, WezTerm, foot, Ghostty and recent iTerm2 do); `⌥⏎`, `⌃J` and `⌃P` work everywhere.

## Preview and discard confirms

The same two-button modal is used for approving, posting now, and discarding a draft.

| Key | Action |
|---|---|
| `← →` / `tab` / `h` `l` | Switch buttons. Approve and post now start on the action; discard starts on **No, keep editing** |
| `y` / `n` | Answer yes / no directly |
| `⏎` | Confirm the highlighted button |
| `esc` | Cancel |

## Footer

The footer lists the key hints on the left and a status on the right. When space is short, a status message you triggered keeps its place and the later hints drop first. The passive `refreshed HH:MM` time is the first to go: it shows only when every hint still fits, so `s show filters` and the hints before it are never cut for it.

## Show filters

`s` opens a small control over the queue. Tick a filter to let those changes in, clear it to hide them.

| Key | Action |
|---|---|
| `↑↓` / `j` `k` | Move the cursor |
| `space` / `⏎` | Toggle the filter under the cursor |
| `1`–`5` | Toggle `reviewing`, `assigned`, `authored`, `drafts` or `noise` directly |
| `esc` / `s` | Close |

The filters start from `triage.show` and, when the Sources pane is showing, are also listed under the sources. Changes are kept for the session only and are never written to `config.toml`. The selection stays on the same change when it is still listed, and otherwise settles on the nearest row. Noise stays expanded or collapsed as you left it.

## Mouse

Every mouse target is registered while drawing and resolved in `update`, so it behaves like the key it mirrors. Turn the mouse off with `ui.mouse = false`; then the terminal's own selection and wheel work everywhere.

| Where | What it does |
|---|---|
| Top bar theme name | Cycles the theme (`T`) |
| Footer hints | Run the hint's action |
| Sources rows, or the source tabs when Sources is collapsed | Show that source. When the tabs overflow they scroll to keep the active one in view, and `‹` / `›` step to the previous or next source |
| Queue row | Selects it. A double-click (two clicks within about half a second) opens its diff. Clicking a `+N more` row expands or collapses its bucket |
| `N hidden by your Show filters` (the queue's end note) | Opens the Show filters control |
| Show filters (the checkboxes under the sources, or the control) | Tick or clear a filter. A click outside the control closes it |
| Tabs (Overview, Files, Checks, Conversation) | Switch the detail view |
| Action chips (Approve, Request changes, Comment, Diff) | Diff opens it; Approve and Comment open the diff and start there; Request changes and Merge explain they are planned for v0.3 |
| Any pane | Click empty space to focus it. The wheel scrolls the pane under the pointer, whichever is focused |
| Diff: files | Click a file to open it |
| Diff: rows and thread blocks | Click to place the cursor; a block puts it on the line it hangs from |
| Diff: press and drag | Selects a range of lines, shown with the selection colour and a `▌` marker. Dragging past the top or bottom scrolls. `c` comments on the range |
| Diff: shift-click | Extends the range from the cursor (or the range's anchor) to the line clicked |
| Composer text | Click to place the caret. The wheel moves the caret a line. Clicks outside the composer are ignored and never discard the draft |
| Preview and discard confirms | Click **Cancel** or the confirm button. Clicks outside are ignored |
| Toast | Click to dismiss |
| Help overlay | The wheel scrolls it; a click closes it |

**Shift always belongs to the terminal.** With mouse capture on, terminals keep Shift-drag for their own text selection and don't send it to the app. If a terminal forwards Shift events anyway, Review Buddy ignores all of them except a Shift-click in the diff, which extends the selected range. Shift-drag and Shift-wheel are never consumed.

## Planned keys

These are in the spec and not built in 0.1. Pressing them does nothing, or says it isn't available yet.

| Key | Planned for | Action |
|---|---|---|
| `⌃K` · `/` | v0.4 | Command palette · search all sources |
| `,` | v0.4 | Settings |
| `L` · `J` | v0.4 | Cycle layout · toggle Jax |
| `x` `m` on the dashboard | v0.3 | Request changes and merge |
| `m` | v0.3 | Merge, with a confirm that defaults to **No, not yet** |
| `R` · `b` | v0.3 | Re-run failed CI · check out the branch |
| `n` `p` (one at a time) | v0.4 | Skip / previous in the `queue` layout |
| `v` · `w` | v0.3 | Unified ↔ side by side · toggle whitespace-only changes |
| `s` · `⌃S` | v0.3 | Comment with a suggestion · insert a suggestion block in the composer |
| `⇧↑↓` · `V` | v0.3 | Extend the range from the keyboard, (drag and shift-click select a range today) |
| `e` · `f` | v0.3 | Resolve / unresolve a thread · mark a file viewed |
| `⌃E` | v0.3 | Edit the draft in `$EDITOR` |
| `x` in the diff | v0.3 | Request changes, with a required summary |
| First run (`--setup`) | Shipped in v0.2 | `⏎` continue, `space` pick, `←→` theme, `J` Jax, `esc` skip, `⌫` back |

The merge confirm modal (`← →` / `tab` switch between **No, not yet** and **Merge**, `⏎` confirms, `esc` cancels) arrives with merge.

## Remapping

`[keys]` in `config.toml` is parsed and kept, but not applied yet: every key in 0.1 is fixed. The table below is the planned shape.

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
