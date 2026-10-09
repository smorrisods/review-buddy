# Key bindings

Review Buddy uses arrows plus mnemonic letters. Vim-style `j`/`k` also work for movement. Arrows, `tab`, `⏎` and `esc` always work.

**Status.** This page has two halves. The first covers what the binary does today. It matches the key handlers in `app/` and, with a few exceptions, the registry in `ui::chrome` that the footer hints and the `?` overlay are built from. The main two: `r` (refresh) and `d` (open the diff) work on the dashboard but aren't listed in that registry, so the overlay doesn't show them. They stay out on purpose for now: the dashboard's help list already fills a 160×40 terminal, and two more rows would push `⌃Z suspend` off the bottom. The registry also leaves out a few aliases, which appear only on this page: `}` and `{` for hunks, `⌃U` and `⌃D` for half pages, and `home` and `end`. [Planned keys](#planned-keys) lists what the spec describes for later milestones. Press `?` in the app for the keys on the current screen.

Notation: `⌃` Ctrl · `⇧` Shift · `⌥` Alt · `⏎` Enter.

## Dashboard

| Key | Action |
|---|---|
| `↑↓` / `j` `k` | Move in the focused pane |
| `g` `G` / `home` `end` | Jump to the first / last row in the focused pane |
| `tab` / `⇧tab` | Next / previous pane, in the order they appear: left to right side by side, top to bottom when the Detail is stacked. `h` / `l` and `← →` do the same; `j` / `k` always move inside the focused pane |
| `h` `l` | Previous / next pane |
| `← →` | Previous / next detail tab when the detail pane is focused, otherwise previous / next pane |
| `[` `]` | Previous / next detail tab (Overview, Files, Checks, Conversation) |
| `1`–`9` | Switch source (`1` is All, then sources in config order) |
| `⏎` / `d` | Open the diff for the selected change |
| `⏎` on the Noise row | Expand or collapse the bot updates |
| `⏎` on a `+N more` row | Show the rest of that bucket, past `triage.bucket_limit`. `⏎` again on the `show fewer` row collapses it |
| `,` | Open Settings → Sources (see [Settings](#settings)) |
| `D` | Open the **Pending reviews** list: every change you have a saved draft for (see [Review drafts](#review-drafts)). A `✎ 3` marker on a queue row says the same thing. The footer adds `D pending` while there are drafts |
| `s` | Open the Show filters control: `reviewing`, `assigned`, `authored`, `drafts` and `noise`. Changes apply to the queue at once and last for the session |
| `p` | Close or reopen the Detail pane (`ui.detail`). Closed, the Queue takes the full width and the footer adds `p show detail`. Selection and `⏎` are unchanged. Remembered between runs |
| `S` | Cycle where the Sources sit: `auto` (pane at 130 columns or more, tabs below), `left` (always the pane) and `top` (always the tab strip). Remembered between runs (`ui.sources`) |
| `P` | Rotate the panes: the list (Queue) moves left → top → right → bottom, then back to `auto`. As `ui.detail_position` values that is `auto` → `bottom` → `left` → `top` → `right` → `auto`. `auto` puts Detail on the right when the terminal is at least 110 columns wide and the Queue and Detail have 98 between them, otherwise below the Queue. A stop that would look the same as what is on screen is skipped (see the table below). Stacked, the Queue gets about 55 percent of the height (at least 8 rows) and Detail the rest, and Detail drops to a compact header so the action chips and tabs stay in view. The toast says what is on screen, for example `Layout: list on top, details below`. Remembered between runs (`ui.detail_position`, `REVIEW_BUDDY_DETAIL_POSITION`) |
| `<` / `>` | Shrink or grow the focused pane by 2 columns (or rows, when stacked): the Queue/Detail split for those panes, or the Sources width when Sources is focused (or when Detail is closed). Remembered between runs; `[` and `]` stay on the Detail tabs |
| `=` | Reset the split next to the focused pane to its automatic size |
| `W` | Save the current Queue width and height to `ui.queue_width` and `ui.queue_height` in the config file. Needs a config file to write to; under `--demo` nothing is written |
| `n` / `N` | On the Conversation tab with the Detail pane focused: move to the next / previous thread (marked `›`), scrolling it into view |
| `z` / `Z` | On the Conversation tab with the Detail pane focused: fold or unfold the thread under the cursor / fold every thread, or unfold them all when all are folded. Resolved threads start folded. Clicking a thread's header does the same as `z` |
| `c` / `⏎` | On the Conversation tab with the Detail pane focused: reply to the thread under the cursor (opens the diff on its line with the reply box ready) / jump to its line in the diff. A general comment has no line, so `⏎` says so and `c` still replies |
| `v` | On the Conversation tab with the Detail pane focused: start copy mode on the cursor thread's comments (`hjkl`, arrows, `w` `b`, `0` `$`, `tab` for the next comment on screen, `y`, `esc`) |
| `PgUp` / `PgDn` | Scroll the Detail pane when it has focus (so does `j` / `k`, the wheel and `g` / `G`) |
| `r` | Refresh now (live sources; also refreshes when the terminal regains focus, if `refresh.on_focus` is on) |
| `o` / `y` | Open the change in the browser / copy its URL. With text selected in the Detail pane, `y` copies that text instead (see [Copying text](#copying-text)). While an image is selected (`i`), they open or copy that image's address instead. Under `--demo` nothing is opened: the footer says `Would open <url> (demo)` |
| `i` | Select the next image in the description, scrolling it into view: its caption gains `▸` and `(open with o)`. After the last, none is selected. Works with images off too, so `o` can still open one. See [Images in descriptions](SPEC.md#images-in-descriptions) |
| `t` | Open the terminal pane next to the change, or focus it if it is open (also in the Diff). See [Terminal pane](#terminal-pane) |
| `T` | Cycle theme |
| `B` | Cycle the background: theme → yes → no, and remember it between runs (not on first run) |
| `M` | Turn mouse capture off or on for this session, so your terminal's own selection and wheel work everywhere. The footer shows `mouse off · M turns it on` while it is off. Also on the Diff screen |
| `?` | Help overlay for the current screen (`esc` or `?` closes it) |
| `q` / `⌃C` | Quit. If there are unsent comments, it asks you to quit again |
| `⌃Z` | Suspend to the shell on macOS and Linux, on every screen. The terminal is restored first (main screen, cooked mode, mouse, paste and focus reporting off, cursor shown); `fg` re-enters, repaints everything and refreshes as if the window had just regained focus. Unsent composer text is kept. On Windows there is no job control: the footer says `Suspend isn't available on Windows. Use ⌃C or q to quit.` |
| `esc` | Clear selected text, then dismiss status messages |

The action chips in the detail pane (`a Approve`, `x Request changes`, `c Comment`, `m Merge`) are clickable. `a`, `x` and `c` (or their chips) open the diff for the selected change and start the review modal (Approve or Request changes selected) or the comment composer there, with the cursor on the first changed line. Where the source can't request changes, `x` explains why in one line instead. `m` explains that merging is planned for v0.3.

**Rotating with `P`.** The cycle is `auto` → `bottom` (list on top, details below) → `left` (list on the right, details left) → `top` (list on the bottom, details above) → `right` (list on the left, details right) → `auto`. A stop that looks the same as the layout showing now is skipped, and so is a fixed `right` that looks the same as the `auto` after it:

| Now | `P` goes to | Note |
|---|---|---|
| `auto` showing details right (110 columns or more) | `bottom` | |
| `auto` showing details below (narrower) | `left` | `bottom` already looks like this, so it is skipped |
| `bottom` | `left` | |
| `left` | `top` | |
| `top` | `right`, or `auto` when `auto` shows details right | At wide sizes `right` looks like `auto`, so the cycle goes `top` → `auto` |
| `right` | `auto`, or `bottom` when `auto` shows details right | A remembered `right` at a wide size: `auto` would look the same, so it goes on to `bottom` |

**Remembered layout.** `P`, `S`, `p`, `B` and the dragged or nudged sizes are remembered between runs in `session.toml` in the state directory (see `docs/configuration.md#remembered-layout`). Precedence at launch, strongest first: a key you press this session, the `REVIEW_BUDDY_*` environment variable, the remembered file, `config.toml`, the built-in default. `ui.remember_layout = false` turns it off, and `review-buddy config reset-layout` forgets it. Demo mode never reads or writes it.

### Queue marks

The `?` overlay on the queue ends with this table, so it matches the app. The marks sit at the right of each row's second line when the Queue is wide enough (close Detail with `p` to see them). See [the status cluster](SPEC.md#queue-status-cluster).

| Mark | Meaning |
|---|---|
| `✓N` | Approvals from other people |
| `✕N` | Reviewers asking for changes |
| `○N` | Reviewers asked and not done yet |
| `○` | An approval is required and nobody was asked |
| `¶N` | Comments on the change: general comments and every comment in review threads (`¶N+` is at least N: the list can't see every thread's comments, so it is exact once you select the change) |
| `N open` | Unresolved review threads |
| `CI running`, `CI failing` | Checks still running; a check has failed |
| `+N −M` | Lines added and removed (`N files` when the forge's list has no line counts) |

## Diff

Open with `⏎` or `d` from the dashboard. A header row above the panes names the change under review (forge badge, `owner/repo#number`, source and title), so `o` and `y` visibly act on that change.

| Key | Action |
|---|---|
| `tab` / `⇧tab` | Switch focus between Files, Diff and (when there are pending comments) the pending-comment list under the files |
| `↑↓` / `j` `k` | Files pane: choose a file · Diff pane: move the line cursor |
| `g` `G` / `home` `end` | First / last line (Files pane: first / last file) |
| `PgUp` `PgDn` · `⌃U` `⌃D` | Move a page or half a page |
| `⏎` | Files pane: focus the diff · Diff pane, on a line with a pending comment: edit it · pending-comment list: jump to the comment |
| `n` `}` / `p` `{` | Next / previous hunk |
| `→` / `←` · `]` / `[` | Next / previous file. The footer says `That's the last file.` or `That's the first file.` at the ends. While the composer or a confirmation is open, `←` and `→` keep their own meaning there |
| `⇧↑` `⇧↓` · `⇧PgUp` `⇧PgDn` | Extend a range from the cursor by a line or a page. The first press anchors the range at the cursor line; the footer counts the lines (`3 lines selected`) |
| `V` | Start a line-wise selection, for terminals that don't report Shift+arrows: plain `↑` `↓` `j` `k` `PgUp` `PgDn` (and `g` `G` `n` `p`) extend the range until you press `V` again, which ends the mode and keeps the range, or `esc`, which clears it |
| `c` | Comment on the cursor line, or on the selected range (`start_line` to `line`, on the side of the last line) after a drag, shift-click or keyboard selection; the selection clears once the comment is added |
| `r` | Reply to the thread on the cursor line. When the line has one, the footer shows `r reply` and the thread block's bottom border carries a clickable `r reply` hint. The cursor never rests on the thread block itself (`j`/`k` step over it), so `⏎` isn't a reply key |
| `e` | Edit the pending comment on the cursor line (see [Editing pending comments](#editing-pending-comments)) |
| `d` / `del` | Delete it, after a confirm that defaults to **No** |
| `,` / `.` | Previous / next pending comment on the cursor line, when it has several (`Comment 2 of 3 on this line`) |
| `a` | Review modal with **Approve** selected (see [Review modal](#review-modal)) |
| `x` | Review modal with **Request changes** selected. Where the source can't request changes the key stays on screen and says why in one line instead |
| `R` | Review modal with **Comment** selected: submits the pending comments without approving |
| `v` | Copy mode: start a text selection at the cursor line and move its end with `h` `j` `k` `l` or the arrows, `w` / `b` by word, `0` / `$` to the start / end of the row. `tab` moves between the code and the comments under the line. `y` copies and ends the mode; `esc` or `v` cancels. See [Copying text](#copying-text) |
| `y` | With text selected: copy exactly that text (otherwise copies the change's URL, as below) |
| `Y` | Copy the whole comment or thread under the cursor line (the one picked with `,` / `.` when the line has several) |
| `M` | Turn mouse capture off or on for this session |
| `z` | Turn soft wrap on or off (`Wrap on` / `Wrap off`; `diff.wrap`). See [Soft wrap](#soft-wrap) |
| `o` / `y` | Open the change's files page in the browser / copy its URL |
| `T` / `B` / `?` | Cycle theme / cycle the background / help |
| `esc` / `q` | Clear the selected text or range (and end `V` and copy modes), then go back to the dashboard. If you added or changed comments since you opened the change it asks `Keep these 3 comments as a draft?` first (see [Review drafts](#review-drafts)) |

A keyboard range is the same range a drag makes: the same highlight, the same `c` comment, and it stays inside one hunk (the extension stops at the hunk edge) and, for the comment, on one side. A plain move without Shift outside `V` mode clears the range, just as a click does.

**Mouse:** click to place the cursor · press and drag on code or a comment to copy text · press and drag on the line numbers to select a range of lines · shift-click to extend the range · click a file to open it · the wheel scrolls the pane under the pointer. See [Mouse](#mouse).

### Copying text

Where a drag starts decides what it does, so neither use is lost:

- **Start on code, a comment or a thread, the description, or a path in the Files pane:** the drag selects text. It stays inside the block of text it began in, even if the pointer leaves it (a drag that starts in a thread never picks up the code beside it, and the other way round), is highlighted cell by cell in a stronger tint than the cursor line (reversed under `NO_COLOR`), and is copied when you let go. The toast says `Copied 42 characters`, never the text. Dragging past the top or bottom of the diff scrolls it.
- **Start in the line-number gutter or on the `+` / `-` sign:** the drag selects whole lines for `c`, exactly as before. Shift-click, `⇧↑` `⇧↓` and `V` are unchanged.

What is copied is the logical text. For the diff that is the code without the gutter, with tabs expanded as shown; for a thread or comment it is the text inside the border, without the `│` padding or the `-` / `+` marks of a suggestion; a soft-wrapped line (the diff with `z`, a long comment, the description) comes back as one line, wide characters whole; rows are joined with newlines and trailing spaces are trimmed. Selecting across code lines skips the comments between them. A path shortened to fit (`…ui/menus.rs`) copies in full. Image rows have no text to select.

**Double-click** selects a word, **triple-click** a line (a whole wrapped line, a whole paragraph); dragging after either grows the selection by words or lines. `esc`, or a click anywhere else, clears the highlight. Changing file, toggling wrap, resizing or opening a composer also drops it, since the rows it pointed at have moved. The first time you press on text in the diff the footer reminds you of the split.

**With the keyboard:** `v` starts a selection at the cursor line, `h` `j` `k` `l` and the arrows move its end, `w` and `b` jump by word, `0` and `$` go to the ends of the row, `tab` moves between the line's code and the comments under it, and `y` copies and ends the mode. `esc` or `v` cancels. Other keys do nothing in this mode. `y` outside copy mode copies a mouse selection when there is one, and otherwise keeps its meaning (the change's URL, or the selected image's address). `Y` copies a whole comment or thread, with the author lines, exactly as written.

**When the terminal's own selection is what you want:** Shift-drag always works (most terminals keep it for themselves; Windows Terminal and some others want their own modifier). `M` turns mouse capture off for the rest of the session so a plain drag is the terminal's too, and on again; the footer shows `mouse off`. `ui.mouse = false` starts that way. The copy goes through OSC 52 first and the operating system's clipboard tool second, so it works over SSH in terminals that allow OSC 52.

### Soft wrap

Off by default, a line that is wider than the Diff pane is cut at the edge and ends in `…`. Press `z` (or set `diff.wrap = true`) to wrap it instead: the line continues on the next rows, under the code and not under the line numbers. Each continuation row shows a dim `↪` where the line numbers are (in the old column, the new column, or both, matching the line), repeats the `+` or `-` sign, and carries the same added or removed tint and cursor highlight; line numbers and the `›` cursor marker appear only on the first row. Syntax highlighting carries across the break, and a wide character (CJK, emoji), a character with its combining marks, or the spaces a tab expands to are never split, so a row can end a cell early. Thread and pending-suggestion blocks sit below the last row of their line.

The cursor, a selected range, `j` `k`, `n` `p`, `c` and the mouse all work on whole lines: `j` moves one line, not one row, and clicking or dragging on any row of a wrapped line means that line. Scrolling, `PgUp` `PgDn`, `⌃U` `⌃D` and the wheel count screen rows. Resizing the terminal, or opening, closing or resizing the terminal pane, re-wraps the lines and keeps the cursor line and the top of the view. While wrap is on, the Diff pane's bottom border ends with `wrap`, and the toggle says `Wrap on` or `Wrap off`. The choice is remembered between runs with the rest of the layout (see [Remembered layout](#remembered-layout) in the configuration reference). The Files pane, the composer and the other screens are not affected.

## Terminal pane

`t` opens a terminal beside the selected change (or the change whose diff is open): a shell, or whatever `ui.terminal.command` runs, for `claude`, `opencode`, `lazygit`, `vim` and the like. It lives in a box on the Dashboard and Diff screens; the app keeps the rest of the screen.

**Starting.** Under `--demo` the pane opens at once and replays a scripted shell (`help`, `ls`, `git log`, `env` and a few more are answered; nothing is started, read or written). Otherwise Review Buddy looks for a local clone of the change's repository (the directory it was started in, then `ui.terminal.checkouts`) and shows a preview before anything is created:

| Key | Action |
|---|---|
| `←` `→` / `tab` / `h` `l` | Move between **No, cancel** (the default), **Create the worktree** and **Use the current directory** |
| `⏎` | Choose the highlighted button |
| `y` / `w` | Create the worktree now (only when a clone was found) |
| `c` | Use the current directory |
| `n` / `esc` / `q` | Cancel; nothing is created |

There is one pane at a time. It stays with the change it was opened for (its title says which) until you close it, so `t` on another change focuses the same pane; close it with the chord then `x` to start one for the new change.

The worktree is a separate, detached checkout of the change's head under `worktrees/` in the state directory; your own working copy is untouched, and Review Buddy never deletes it (the preview shows the `git worktree remove` command). The child gets `RB_SOURCE`, `RB_REPO`, `RB_NUMBER` and `RB_URL` either way.

**While the pane has focus** (its border is thick and its title says `focused`), every key goes to the child, including `⌃C`, `⌃Z`, `q` and `esc`. The pane's bottom border and the footer say how to get out.

| Key | Action |
|---|---|
| chord, then `esc` | Return focus to the app. The chord is `⌃\` unless `ui.terminal.escape` says otherwise |
| `esc` `esc` | The same, where a terminal or multiplexer swallows the chord. The first `esc` still reaches the child |
| chord, then `t` | Hide the pane. The child keeps running; `t` brings it back |
| chord, then `x` | Close the pane and stop the child. A running child is only stopped on the second request, and the footer says so |
| chord, then `p` | Move the pane: auto → bottom → left → top → right. Remembered |
| chord, then `>` `<` / `=` | Grow or shrink it by two columns or rows / back to automatic. Remembered |
| chord twice | Send the chord's own key to the child |
| paste | Sent to the child, bracketed if it asked for that |

A half-typed chord is forgotten after about two seconds. When the child exits the pane keeps its last screen and a line saying so; any key closes it, and `t` starts a fresh one. If the window gets too small for both, the pane steps aside (the child keeps running) and `t` brings it back at a larger size.

**Keys the child sees.** Keys are encoded the way a terminal would: application cursor keys when it asked for them, and the kitty keyboard protocol only when the child switched it on and your terminal can tell the keys apart (kitty, WezTerm, Ghostty, foot, Alacritty and recent iTerm2; the terminal is asked at launch, and `REVIEW_BUDDY_KITTY_KEYS=1` or `0` skips the question). Without it, `⏎` and `⇧⏎` both send a carriage return, as they do on a Windows console.

**Mouse.** Clicking the pane focuses it; clicking the app gives focus back. When the child turns mouse reporting on, clicks, drags and the wheel go to it. **Shift always belongs to your terminal**: a Shift-click or Shift-drag is never sent to the child, so your terminal's own selection works. When the child isn't listening, the wheel scrolls the pane's history (the title says `↑ N lines back`; typing snaps back to the bottom), or sends arrow keys on the alternate screen, as `less` expects. Press and drag the seam to resize; double-click it to go back to automatic.

## Review modal

`a`, `x` and `R` open it over the diff, with Approve, Request changes or Comment selected. It previews exactly what will be sent (for example `Request changes with 2 comments`, from the same plan the forge call uses), lists the pending comments with their file and line, and has an optional **summary**, which becomes the review body whatever the verdict. Verdicts the source doesn't support are left out (GitLab shows Request changes only where the capability probe allows it).

- **Request changes** needs a summary. **Comment** needs at least one pending comment or a summary. **Approve** goes with both empty. Until the verdict is valid the modal says what is missing and **Submit** does nothing.
- **Cancel** is the default focus for Request changes (or the summary field while it is empty), so ⏎ never sends one by accident. Changing the verdict never moves focus onto Submit by itself, and nothing is sent until you press Submit.
- **Submitting:** `tab` from the summary lands on Submit, and `⏎` there sends. `⌃P` submits from anywhere, as do `⌥⏎` and, where the terminal can report it, `⌃⏎`. Many terminals (Windows Terminal among them) can't tell `⌃⏎` from `⏎`, so the line under the buttons shows `⌃⏎` only when the terminal answered the kitty keyboard protocol query at startup, and otherwise says `tab then ⏎ submits · ⌃P submits now`. `REVIEW_BUDDY_KITTY_KEYS=1` or `0` overrides the detection.
- **Tab order** is Summary → Submit → Cancel → verdict row → Summary; `⇧tab` walks it backwards.
- **While sending** the button reads `sending…`. If the forge refuses (for example it won't let you request changes on your own pull request), the reason shows in red under the buttons with a `✗`, the modal stays open with focus on Submit, and a toast repeats it. The next key clears the message; `⏎` retries, or change the verdict first.
- Closing the modal keeps the pending comments and the summary; they are there next time. After a successful submit they clear, a toast says `Approved`, `Changes requested` or `Review posted` (with `(demo)` under `--demo`), and the Files pane and the queue row show `Your review: …`. On failure everything stays and the toast says what to do next.

| Key | Action |
|---|---|
| `←` `→` / `h` `l` (verdict row) · `1` `2` `3` | Choose Comment, Approve or Request changes |
| `↓` / `⏎` / `tab` (verdict row) | Move to the summary |
| Typing, `⇧⏎` `⌃J` for a new line (summary) | Edit the summary. `⏎` adds a line here and never submits |
| `tab` (summary) · `↓` on the last line | Move to Submit · Cancel |
| `↑` on the first line · `⇧tab` (summary) | Back to the verdict row |
| `⌃P` · `⌥⏎` · `⌃⏎` (where reported), from anywhere in the modal | Submit |
| `tab` (buttons) | Submit → Cancel → verdict row |
| `←` `→` (buttons) | Switch between Cancel and Submit |
| `⏎` (buttons) | Press the focused button |
| `↑` / `e` (buttons) | Back to the summary |
| `esc` / `n` | Cancel |

**Mouse:** click a verdict, the summary (the caret goes under the pointer), Cancel or Submit. A press on a button arms it and releasing on the same button presses it; a release with no press before it also presses it, for terminals that only report releases. A click on no control, or outside the modal, does nothing.

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

Comments you add are kept as a draft per change and saved between sessions (see [Review drafts](#review-drafts)). `⌃⏎` and `⇧⏎` need a terminal that reports those modifiers (kitty, WezTerm, foot, Ghostty and recent iTerm2 do); `⌥⏎`, `⌃J` and `⌃P` work everywhere.

## Review drafts

Everything you write in a review (the comments you add with `c`, the summary and the verdict you chose in the review modal) belongs to the **change**, not to the screen. It stays when you leave the diff and when you quit, and comes back when you open the change again, with the cursor on the file and line you left it on.

**A local draft is not a pending review on the forge.** The draft is your own unsent text, kept on this computer; the forge doesn't know about it until you submit the review. GitHub and GitLab can also hold a *pending review* of their own (started on the web, or left behind by a submit that stopped part way). Those show up in the diff as threads marked `◌ pending`, exactly as before, and are edited through the forge (see below). Submitting sends the local draft; it never touches the forge's pending threads except to publish them with it.

- **Leaving a diff** (`esc`, `q`, or the mouse) asks `Keep these 3 comments as a draft?` when you added or changed comments since you opened the change. **Keep** (the default, so `⏎` is safe) saves and leaves, **Discard** throws them away, **Cancel** stays. `k`, `d` and `c` pick them directly, `tab` and `←` `→` move between the buttons, and `esc` is Cancel. With nothing new it just leaves.
- **Quitting** saves drafts instead of discarding them, so there's nothing to confirm. Text still open in the composer is saved as a comment. With `ui.drafts = "off"` (or under `--demo`) drafts live in memory only, and quitting warns first.
- **Autosave** writes the draft under `$XDG_STATE_HOME/review-buddy/drafts/`, one file per change, a moment after each change. Submitting a review, discarding a draft or deleting its last comment removes the file. See [configuration](configuration.md#review-drafts-on-disk).
- **Restoring** says `Draft restored · 3 comments`. If the change's head moved since you wrote the draft the comments are kept and marked `code changed`, with `Code changed since you wrote this` in the Files pane. A comment whose line is no longer in the patch stays in the review block marked `outdated` (and in the pending-comment list); pick it in the list and choose **Add to summary** (it goes into your review summary as a general comment, quoting where it was), **Discard**, or **Leave it**. Nothing is dropped silently.

### Pending reviews

`D` on the dashboard lists every change with a draft: source, `repo#number`, title, comment count (`✎ 3`), age, `outdated` when the head moved, and `not in queue` for a change no source lists any more (kept until you discard it). Queue rows with a draft show the same `✎ 3` marker.

| Key | Action |
|---|---|
| `↑↓` / `j` `k` · `g` `G` | Move |
| `⏎` | Open the change's diff with the draft restored |
| `x` / `del` | Discard that draft, after a confirm that defaults to **No, keep it** |
| `esc` / `q` / `D` | Close |

**Mouse:** click a row to select it, click it again to open it. Click outside the list to close it. `review-buddy drafts list`, `drafts discard` and `drafts clear` do the same from the shell (see [the CLI](cli.md#review-buddy-drafts)). The existing `drafts` Show filter means *draft pull requests* (authored as draft), not your pending comments.

### Editing pending comments

With the cursor on a line that has a pending comment (one of yours from a draft, one restored from disk, or a pending one on the forge):

- `e`, or `⏎` on the line, reopens it in the composer with its text and its anchor (a range stays a range). `⏎` saves it in place, in the same position and order. `esc` closes, and asks first only if you changed the text. The title says `Edit comment · a.rs lines 3–5`.
- `d` or `del` deletes it after a confirm that names what goes (`a.rs lines 3–5 · “first words”`) and defaults to **No, keep it**.
- When a line has several pending comments `,` and `.` step between them (`Comment 2 of 3 on this line`, the picked one is marked `▸`), and `e` and `d` act on that one. (`n` and `p` stay on hunks.)
- The Files pane's **Your review** block lists each pending comment as `file:line first words`. `tab` focuses the list, `↑↓` choose, `⏎` jumps to it, `e` and `d` edit and delete it. Clicking an entry jumps to it.
- The summary and verdict are edited in the review modal (`a`, `x`, `R`) and are kept with the draft.
- A **pending comment on the forge** (started on the web) can be edited and deleted too, where the forge allows it (GitHub through `updatePullRequestReviewComment` and `deletePullRequestReviewComment`, GitLab through the draft notes). Because that changes the forge, editing asks `Update this pending comment?` and deleting asks `Delete this pending comment on the forge?`, and a failure says what to do next and leaves your text where it was. Nothing is published until you submit the review. Where the forge can't do it, those comments aren't offered for editing.

## Preview and discard confirms

The same two-button modal is used for approving, posting now, discarding a draft, deleting a pending comment and updating one on the forge (the default is **No** for the destructive ones). Leaving a diff with new comments, and a comment whose line is gone, use a three-button version whose first button is the safe default.

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
| `?` · `T` · `B` · `esc` | Help · theme · background · back to the queue |

In the form, `tab` / `⇧tab` (or `↓` `↑`) move between fields, `← →` or `space` change a choice, `⏎` saves and `esc` cancels. A token typed in the form is hidden, is tested before anything is written, and goes only to your OS keyring; the config file never holds it. Leave it blank to keep the token already saved.

The remove confirm names the source and the file it comes out of. `⏎` chooses the highlighted option, `y` removes the source, `d` removes it and its keyring token (offered only when no other source shares that token), and `esc` or `n` keeps it. The keyring token stays unless you choose `d`.

Sources that come from `config.d/*.toml`, `$XDG_CONFIG_DIRS` or a `--config` file other than the write target are shown with where they come from (`from config.d/10-work.toml`). Because a later file's `[[source]]` list replaces an earlier one, they can't be overridden from your own `config.toml`, so edit, add, switch and remove explain this and leave every file alone; testing a token still works. In demo mode Settings lists the demo sources read-only and says `(demo)`.

## Mouse

Every mouse target is registered while drawing and resolved in `update`, so it behaves like the key it mirrors. Turn the mouse off with `ui.mouse = false`, or with `M` for the rest of the session; then the terminal's own selection and wheel work everywhere.

| Where | What it does |
|---|---|
| Seam between Queue and Detail, or at the Sources pane's edge (the two border columns or rows next to it) | Press and drag to resize; the seam shows in the accent colour and the footer shows the size (`queue 52 columns`). Double-click resets it to automatic. Only a press that starts on the seam resizes; Shift-drag stays the terminal's |
| Top bar theme name | Cycles the theme (`T`) |
| Footer hints | Run the hint's action |
| Sources rows, or the source tabs when Sources is collapsed | Show that source. Tab names are shortened to share the width (the active one stays whole while it can, long names lose their middle, counts always show). When the tabs still overflow they scroll to keep the active one in view, and `‹` / `›` step to the previous or next source |
| `⟩` on the Detail pane's top border / `⟨ detail` on the Queue's | Closes / reopens the Detail pane (`p`) |
| Queue row | Selects it. A double-click (two clicks within about half a second) opens its diff. Clicking a `+N more` row expands or collapses its bucket |
| `N hidden by your Show filters` (the queue's end note) | Opens the Show filters control |
| Show filters (the checkboxes under the sources, or the control) | Tick or clear a filter or a project. The search line focuses the search, and the wheel scrolls the project list. A click outside the control closes it |
| Tabs (Overview, Files, Checks, Conversation) | Switch the detail view |
| Action chips (Approve, Request changes, Comment, Diff) | Diff opens it; Approve, Request changes and Comment open the diff and start there (Request changes explains why when the source can't do it); Merge explains it is planned for v0.3 |
| Terminal pane | Click to focus it. With mouse reporting on in the child, clicks, drags and the wheel go to it; otherwise the wheel scrolls its history. Shift is never sent to the child. Drag its seam to resize, double-click to reset |
| Any pane | Click empty space to focus it. The wheel scrolls the pane under the pointer, whichever is focused |
| Diff: files | Click a file to open it |
| Diff: rows and thread blocks | Click to place the cursor; a block puts it on the line it hangs from. With soft wrap on, any row of a wrapped line means that line |
| Diff: press and drag on the line numbers or the `+` / `-` sign | Selects a range of lines, shown with the selection colour and a `▌` marker. Dragging past the top or bottom scrolls. `c` comments on the range |
| Diff: press and drag on code, a comment or a thread, the Files paths, or the Detail description and latest comment | Selects text inside that block and copies it when you let go (`Copied 42 characters`); `esc` or a click elsewhere clears it. Double-click a word, triple-click a line. See [Copying text](#copying-text) |
| Diff: shift-click | Extends the range from the cursor (or the range's anchor) to the line clicked |
| Pending reviews list | Click a row to select it, click again to open it. The confirm's buttons answer it. A click outside closes the list |
| Diff: Your review list | Click a pending comment to jump to it |
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
| `m` on the dashboard | v0.3 | Merge |
| `m` | v0.3 | Merge, with a confirm that defaults to **No, not yet** |
| `R` (dashboard) · `b` | v0.3 | Re-run failed CI · check out the branch (`R` already submits a review in the diff) |
| `n` `p` (one at a time) | v0.4 | Skip / previous in the `queue` layout |
| `v` · `w` | v0.3 | Unified ↔ side by side · toggle whitespace-only changes |
| `⌃S` | v0.3 | Insert a suggestion block in the composer (on the dashboard `s` already opens Show filters, so a suggestion shortcut in the diff will need its own key) |
| `e` · `f` | v0.3 | Resolve / unresolve a thread · mark a file viewed (`e` already edits a pending comment in the diff, so resolving will need its own key) |
| `⌃E` | v0.3 | Edit the draft in `$EDITOR` |

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
