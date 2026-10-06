# Key bindings

Review Buddy uses arrows plus mnemonic letters. Vim-style `j`/`k` also work for movement. Arrows, `tab`, `⏎` and `esc` always work.

**Status.** This page has two halves. The first covers what the binary does today. It matches the key handlers in `app/` and, with two exceptions, the registry in `ui::chrome` that the footer hints and the `?` overlay are built from: `r` (refresh) and `d` (open the diff) work on the dashboard but aren't listed in that registry, so the overlay doesn't show them. [Planned keys](#planned-keys) lists what the spec describes for later milestones. Press `?` in the app for the keys on the current screen.

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
| `,` | Open Settings → Sources (see [Settings](#settings)) |
| `s` | Open the Show filters control: `reviewing`, `assigned`, `authored`, `drafts` and `noise`. Changes apply to the queue at once and last for the session |
| `p` | Close or reopen the Detail pane (`ui.detail`). Closed, the Queue takes the full width and the footer adds `p show detail`. Selection and `⏎` are unchanged. Lasts for the session |
| `S` | Cycle where the Sources sit: `auto` (pane at 130 columns or more, tabs below), `left` (always the pane) and `top` (always the tab strip). Lasts for the session (`ui.sources`) |
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

`s` opens a control over the queue. The kinds sit at the top: tick a filter to let those changes in, clear it to hide them. Below them is the **Projects** section, which lists every project (GitHub `owner/repo`, GitLab project path) that has an open change in the loaded or cached queue, grouped under its source with a count of changes, and says `N of M projects shown`. The list scrolls when it is long, and the control fits a 100×30 terminal.

| Key | Action |
|---|---|
| `↑↓` / `j` `k` | Move the cursor through the kinds, then the projects |
| `g` `G` / `Home` `End`, `PgUp` `PgDn` | First or last row, or a page at a time |
| `space` / `⏎` | Toggle the filter or project under the cursor |
| `1`–`5` | Toggle `reviewing`, `assigned`, `authored`, `drafts` or `noise` directly |
| `/` | Focus the project search. Typing narrows the list to matching project names, `⏎` returns to the list and keeps the search, `esc` clears it |
| `a` / `n` | Show all or none of the projects the search lets through (every project when there is no search) |
| `w` | Save the project choice to `hide_repos` in your config (see below). Only offered when there is a config file to write to; in demo mode it explains that nothing is written |
| `esc` / `s` / `q` | Clear the search first, then close |

Clicking a project row toggles it, clicking the search line focuses it, and the mouse wheel scrolls the list.

The kinds start from `triage.show` and, when the Sources pane is showing, are also listed under the sources. The project filter starts from each source's `hide_repos`. Every change applies at once and lasts for the session; `w` is the only thing that writes to `config.toml`, and it writes only `hide_repos`. The queue, the bucket headings, the Sources counts, the `+N more` rows and the end note all honour both filters, and the end note says what each one hides (`2 hidden · 1 by kind, 1 by project`). The selection stays on the same change when it is still listed, and otherwise settles on the nearest row. Noise stays expanded or collapsed as you left it.

**Projects that appear later.** The filter remembers what you hid, not what you showed. A project that first appears after a refresh is shown, even when you had narrowed the list, until you untick it yourself. A `hide_repos` entry with a trailing `*` hides any project that matches it, including new ones; ticking one matching project switches that one back on without touching the rest of the pattern.

## Settings

`,` opens Settings from the queue. Only **Sources** is built; Review, Keys, Theme and Jax arrive with the rest of Settings in v0.4. Changes save to your config at once, and the queue reloads from it.

| Key | Action |
|---|---|
| `j` `k` / `↑` `↓` · `g` `G` | Move between sources · first / last |
| `t` | Test the selected source's token: who it signs in as, its scopes and any expiry |
| `e` / `⏎` | Edit the source in a form |
| `a` / `n` | Add a source. Hosts found on this machine are offered first, then **Another host…** |
| `space` | Switch the source on or off (it stays in the config) |
| `x` / `del` | Remove the source, after a confirm that starts on **No, keep it** |
| `?` · `T` · `esc` | Help · theme · back to the queue |

In the form, `tab` / `⇧tab` (or `↓` `↑`) move between fields, `← →` or `space` change a choice, `⏎` saves and `esc` cancels. A token typed in the form is hidden, is tested before anything is written, and goes only to your OS keyring; the config file never holds it. Leave it blank to keep the token already saved.

The remove confirm names the source and the file it comes out of. `⏎` chooses the highlighted option, `y` removes the source, `d` removes it and its keyring token (offered only when no other source shares that token), and `esc` or `n` keeps it. The keyring token stays unless you choose `d`.

Sources that come from `config.d/*.toml`, `$XDG_CONFIG_DIRS` or a `--config` file other than the write target are shown with where they come from (`from config.d/10-work.toml`). Because a later file's `[[source]]` list replaces an earlier one, they can't be overridden from your own `config.toml`, so edit, add, switch and remove explain this and leave every file alone; testing a token still works. In demo mode Settings lists the demo sources read-only and says `(demo)`.

## Mouse

Every mouse target is registered while drawing and resolved in `update`, so it behaves like the key it mirrors. Turn the mouse off with `ui.mouse = false`; then the terminal's own selection and wheel work everywhere.

| Where | What it does |
|---|---|
| Top bar theme name | Cycles the theme (`T`) |
| Footer hints | Run the hint's action |
| Sources rows, or the source tabs when Sources is collapsed | Show that source. Tab names are shortened to share the width (the active one stays whole while it can, long names lose their middle, counts always show). When the tabs still overflow they scroll to keep the active one in view, and `‹` / `›` step to the previous or next source |
| `⟩` on the Detail pane's top border / `⟨ detail` on the Queue's | Closes / reopens the Detail pane (`p`) |
| Queue row | Selects it. A double-click (two clicks within about half a second) opens its diff. Clicking a `+N more` row expands or collapses its bucket |
| `N hidden by your Show filters` (the queue's end note) | Opens the Show filters control |
| Show filters (the checkboxes under the sources, or the control) | Tick or clear a filter or a project. The search line focuses the search, and the wheel scrolls the project list. A click outside the control closes it |
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

## First run

The first-run screen (a launch with no config, or `--setup`) has two focus zones: the list for the step, and the button row under it (`Back`, `Continue`, `Skip for now`). `↑ ↓` always work the list, and `tab` / `shift-tab` cycle list → Back → Continue → Skip.

| Key | Action |
|---|---|
| `⏎` | Continue. With a button focused, activates that button (so `⏎` on a focused Back goes back and on Skip skips) |
| `← →` (`h` `l`) | Move focus along the buttons, starting from Continue. Back is skipped on the first step. Not while the token field is open, where `tab` leaves it |
| `tab` / `shift-tab` | Cycle list → Back → Continue → Skip. On the look step `← →` change the theme live, so use these to reach the buttons |
| `space` | Pick the row (`J` also toggles Jax on its step) |
| `⌫` or `b` | Go back (in the token field `⌫` deletes and `b` types) |
| `esc` | Skip for now (closes the token field first) |

The focused button is drawn as `[› Back ‹]` in reverse video, so it shows without colour. Clicking a button moves focus to it. `--setup --plain` is unchanged.

## Planned keys

These are in the spec and not built yet. Pressing them does nothing, or says it isn't available yet.

| Key | Planned for | Action |
|---|---|---|
| `⌃K` · `/` | v0.4 | Command palette · search all sources |
| `L` · `J` | v0.4 | Cycle layout · toggle Jax |
| `x` `m` on the dashboard | v0.3 | Request changes and merge |
| `m` | v0.3 | Merge, with a confirm that defaults to **No, not yet** |
| `R` · `b` | v0.3 | Re-run failed CI · check out the branch |
| `n` `p` (one at a time) | v0.4 | Skip / previous in the `queue` layout |
| `v` · `w` | v0.3 | Unified ↔ side by side · toggle whitespace-only changes |
| `⌃S` | v0.3 | Insert a suggestion block in the composer (on the dashboard `s` already opens Show filters, so a suggestion shortcut in the diff will need its own key) |
| `⇧↑↓` · `V` | v0.3 | Extend the range from the keyboard (drag and shift-click select a range today) |
| `e` · `f` | v0.3 | Resolve / unresolve a thread · mark a file viewed |
| `⌃E` | v0.3 | Edit the draft in `$EDITOR` |
| `x` in the diff | v0.3 | Request changes, with a required summary |

The merge confirm modal (`← →` / `tab` switch between **No, not yet** and **Merge**, `⏎` confirms, `esc` cancels) arrives with merge.

## Remapping

`[keys]` in `config.toml` is parsed and kept, but not applied yet: every key is fixed today. The table below is the planned shape.

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
