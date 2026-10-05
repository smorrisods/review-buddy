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
| `?` | Help overlay listing the keys for the current screen |
| `q` | Quit (asks first if there are unsent drafts) |
| `esc` | Close overlay → clear selection → back one screen |

## Dashboard

| Key | Action |
|---|---|
| `↑↓` / `j k` | Move in the focused pane |
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
| `↑↓` | Files pane: choose a file · Diff pane: move the line cursor |
| `⏎` | Files pane: open the file · on a thread marker: expand it |
| `⇧↑↓` | Extend the line range |
| `V` | Toggle the range anchor at the cursor |
| `esc` | Clear the range, then go back to the dashboard |
| `{` `}` | Previous / next hunk |
| `⇧[` `⇧]` | Previous / next file |
| `v` | Unified ↔ side by side |
| `w` | Toggle whitespace-only changes |
| `c` | Comment on the line or range |
| `s` | Comment with a suggestion prefilled from the line or range |
| `r` | Reply to the thread at the cursor |
| `e` | Resolve / unresolve the thread at the cursor |
| `f` | Mark the file as viewed |
| `a` / `x` | Submit your review as approve / request changes |
| `b` / `o` | Check out / open in browser |

**Mouse:** click to place the cursor · press and drag to select a range · shift-click to extend · click the `unified │ side by side` toggle · click a file to open it.

## Composer

| Key | Action |
|---|---|
| `⌃S` | Insert a ` ```suggestion ` block from the selected lines |
| `⏎` | Add to your pending review |
| `⌃⏎` | Post now as a standalone comment |
| `⇧⏎` | Newline |
| `⌃E` | Edit the draft in `$EDITOR` |
| `esc` | Discard (asks first if longer than one line) |

## Command palette and search

| Key | Action |
|---|---|
| type | Filter (fuzzy) |
| `↑↓` | Choose |
| `⏎` | Run the command / open the change |
| `⌫` | Delete a character |
| `esc` | Close |

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
