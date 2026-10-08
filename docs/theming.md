# Theming

**Status.** The four built-in themes work, `T` cycles through them, the frame paints an opaque theme background (`ui.background`, `B`), `ui.theme` (and `REVIEW_BUDDY_THEME`) picks one, colour depth is detected and `NO_COLOR` is honoured. `review-buddy theme list` lists the built-ins. **Not built yet:** loading theme files from disk (the search order below is the plan), hot reload, the Settings → Theme screen and palette commands, `theme check` and `theme export` (both exit `2` for now, planned for v0.4.0), and the Jax roles, since Jax isn't drawn yet. The format and roles below are what user themes will use, and the built-in files in `themes/` follow it.

A theme is a TOML file that assigns colours to **roles**. Widgets only ever ask for roles, never raw colours, so a theme can restyle the whole app without touching code.

## Where themes live

- Built-in (the only ones loaded today): Liminal HQ (`liminal-hq`, default), Dusk (`dusk`, the Review Buddy signature look), Afterglow Dark (`afterglow-dark`), Afterglow Light (`afterglow-light`). They are embedded in the binary and also shipped as `themes/*.toml` for reference.
- Your own (planned): `$XDG_CONFIG_HOME/review-buddy/themes/*.toml` (default `~/.config/review-buddy/themes/`). The file name without `.toml` is the theme id.
- Installed packs (planned): `$XDG_DATA_HOME/review-buddy/themes/` (default `~/.local/share/…`), then each `$XDG_DATA_DIRS/review-buddy/themes/` (distro packages, e.g. `/usr/share/review-buddy/themes/`).
- Search order is config home → data home → data dirs → built-ins; the first matching id wins, so you can shadow a built-in by copying it into your config dir. See `configuration.md` → File locations.
- Choose one with `ui.theme = "<id>"` in `config.toml` or `T` to cycle (the Settings → Theme screen and the `Theme: …` palette commands are planned).
- Planned: theme files hot-reload, so saving the file repaints the running app.

## Format

```toml
[theme]
name    = "Harbour"            # shown in Settings and the top bar
extends = "liminal-hq"         # optional; defaults to liminal-hq
appearance = "dark"            # dark | light (used for syntect and 256-colour fallback)
paint_background = true        # optional; true | false overrides what `background` implies (see Background)

[colours]
background = "transparent"     # "transparent" = never paint the background
text       = "#e0e0e0"
accent     = "#ffaa40"
# … any roles below; omitted roles inherit from `extends`

[syntax]
keyword = "#a78bfa"

[ansi]                          # optional overrides for 16-colour terminals
accent = "yellow"
```

Colours are `#rrggbb`, `#rrggbbaa` (alpha is blended against `background`, or against `raised` when `background` is transparent), a 256-colour index (`"237"`), an ANSI name (`"bright-black"`), or `"transparent"` / `"reset"`.

## Background

Themes either paint a background or leave the terminal's own showing. Liminal HQ and Dusk are `transparent`, so they use your terminal's background. Afterglow Dark (`#0f0e1a`) and Afterglow Light (`#fbfaf6`) have an opaque `background`, and the interface fills the whole frame with it, including the cells behind text, the Detail pane and the margins, so those themes look the same whatever your terminal is set to.

Three things decide whether the frame is painted:

- **`ui.background`** is `theme` (the default), `yes` or `no`. `theme` follows the theme: an opaque `background` paints, a transparent one doesn't. `yes` paints even a transparent theme, using the theme's effective background (`#050507` for a dark theme, `#fbfaf6` for a light one, chosen from `appearance`). `no` never paints.
- **`[ui.theme_background]`** maps a theme id to `theme`, `yes` or `no`, and beats `ui.background` for that theme (for example `dusk = "yes"` or `afterglow-light = "no"`).
- **`[theme] paint_background = true | false`** in a theme file sets what `theme` means for that theme, instead of inferring it from `background`. It is inherited through `extends`, like the other `[theme]` keys. `true` on a transparent theme paints the effective background; `false` on an opaque one leaves the terminal's own.

`REVIEW_BUDDY_BACKGROUND=theme|yes|no` overrides the settings for one run, and `B` cycles theme → yes → no for the session (with a toast). From strongest to weakest, the order is: the `B` key, `REVIEW_BUDDY_BACKGROUND`, the theme's `[ui.theme_background]` entry, `ui.background`, then the theme's own default. Neither the key nor the variable is saved to the file.

Two rules sit above all of that. `NO_COLOR` never paints. At 16 colours nothing is painted unless the setting is `yes` (a theme's hex background maps poorly onto the terminal's own 16-colour palette, and `yes` is an explicit ask). At 256 colours the background is quantised to the nearest palette entry like every other role.

Overlays (Show, help, the composer, confirmations, toasts and Settings dialogs) keep their `raised` surface on top of the painted frame, so nothing is left on the terminal's own colour. The selection and the diff add/remove tints are blended against `background` (or `raised` when it's transparent) when the theme loads. Forcing `yes` on a transparent theme therefore uses `raised` for those blends, which is within a shade of the dark fallback.

## Roles

### Surfaces

| Role | Used for |
|---|---|
| `background` | The whole frame. `transparent` leaves the terminal's own background showing |
| `raised` | Overlays (palette, confirm, composer), hunk header background |
| `selection` | Selected row, cursor range, active tab background |
| `line` | Unfocused pane borders, rules, dividers |

### Text

| Role | Used for |
|---|---|
| `text` | Body text, code |
| `text_bright` | Titles, the selected row's title |
| `text_secondary` | Descriptions, unfocused pane titles, section labels |
| `muted` | Metadata, key hint labels, line numbers |

### Accents and state

| Role | Used for |
|---|---|
| `accent` | Focus border and title, key letters in hints, bucket headings, primary chip, cursor |
| `interactive` | Usernames, links, `keyword` syntax by default |
| `cyan` | Branch names, hunk headers, `type` syntax |
| `success` | CI pass `●`, `+` signs, approvals, strings |
| `warning` | CI running `◐`, recoverable errors in the footer |
| `danger` | CI fail `✕`, `−` signs, the merge-confirm border |
| `added_bg` | Background of added lines (keep it subtle, 8–12% alpha) |
| `removed_bg` | Background of removed lines |
| `github` | Source tag `GH` and dot |
| `gitlab` | Source tag `GL` and dot |

### Wordmark and Jax

`wordmark` is used in the top bar today. `jax_body` and `jax_box` are defined but unused until Jax arrives.

| Role | Used for |
|---|---|
| `wordmark` | List of 2–4 colours; the `review buddy` wordmark is drawn per character along this gradient |
| `jax_body` | Jax's outline |
| `jax_box` | Jax's box border and title |

### Syntax (`[syntax]`)

`keyword`, `string`, `number`, `type`, `function`, `comment`, `punctuation`, `macro`. Missing keys fall back to the mapping in the spec (keyword → `interactive`, string → `success`, type → `cyan`, comment → `muted`, everything else → `text`).

## Built-in themes

### Liminal HQ (default)

The Afterglow palette on whatever background your terminal already uses. The values below are tuned for near-black terminals; on light terminals, use Afterglow Light.

| Role | Value |
|---|---|
| background | transparent (mocked as `#050507`) |
| text / bright / secondary / muted | `#e0e0e0` / `#ffffff` / `#b4bfce` / `#7c8796` |
| line / raised / selection | `#2a2d36` / `#111116` / `#ffaa40` @ 12% |
| accent / interactive / cyan | `#ffaa40` / `#a78bfa` / `#22d3ee` |
| success / warning / danger | `#2ec66a` / `#fbbf24` / `#f43f5e` |
| github / gitlab | `#60a5fa` / `#a78bfa` |
| wordmark | `#ffaa40 → #f43f5e → #a78bfa` |
| jax body / box | `#ffb454` / `#e25d75` |

### Afterglow Dark

Indigo-black, purple accent, blue interactive. `background #0f0e1a`, `raised #1a1828`, `line #2a2840`, `text #e4e2ec`, `secondary #b8b6c8`, `muted #8f8da6`, `accent #a78bfa`, `interactive #60a5fa`, `gitlab #ffaa40`.

### Afterglow Light

Warm paper with deeper accents for ≥ 4.5:1 contrast. `background #fbfaf6`, `raised #f2eff4`, `line #ddd6c8`, `text #2b2722`, `bright #15120e`, `secondary #4a453d`, `muted #6b655b`, `accent #b45309`, `interactive #5e44cc`, `cyan #0e7490`, `success #15803d`, `warning #a16207`, `danger #be123c`, `github #2563eb`, `gitlab #5e44cc`.

## Colour depth

Detected once at start-up:

1. `REVIEW_BUDDY_COLOUR_DEPTH=truecolor|256|16` forces a depth for this run.
2. `COLORTERM=truecolor|24bit`, or a `TERM` that names 24-bit colour (`*-direct`, kitty, alacritty, wezterm, ghostty) → 24-bit colour.
3. Terminals that support 24-bit colour but don't always set `COLORTERM` → 24-bit colour: Windows Terminal (`WT_SESSION`, which WSL passes through), iTerm2, VS Code, WezTerm, Ghostty, kitty and VTE 0.36 or newer. These hints are ignored under tmux or screen, which only pass 24-bit colour through when they're configured to; set `COLORTERM=truecolor` there.
4. A `TERM` containing `256color` → each role quantised to the nearest xterm-256 colour (in OKLab, not RGB). Similar themes (Liminal HQ and Dusk, for example) can look almost identical here, because their colours land on the same 256-colour entries.
5. Otherwise → the theme's `[ansi]` table, falling back to the built-in 16-colour mapping (accent → yellow, interactive → magenta, success → green, danger → red, muted → bright-black).

Force it with `ui.colour_depth = "truecolor" | "256" | "16"` in `config.toml`.

## Checking contrast

Planned: `review-buddy theme check <id>` will print each text role's contrast against `background` (or `#050507` / `#fbfaf6` for transparent themes, depending on `appearance`). It warns below 4.5:1, or 3:1 for `muted`.
