# Theming

**Status.** The four built-in themes work, `T` cycles through them, `ui.theme` (and `REVIEW_BUDDY_THEME`) picks one, colour depth is detected and `NO_COLOR` is honoured. `review-buddy theme list` lists the built-ins. **Not built yet:** loading theme files from disk (the search order below is the plan), hot reload, the Settings → Theme screen and palette commands, `theme check` and `theme export` (both exit `2` for now), and the Jax roles, since Jax isn't drawn yet. The format and roles below are what user themes will use, and the built-in files in `themes/` follow it.

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

1. `COLORTERM=truecolor|24bit` → 24-bit colour.
2. A `TERM` containing `256color` → each role quantised to the nearest xterm-256 colour (in OKLab, not RGB).
3. Otherwise → the theme's `[ansi]` table, falling back to the built-in 16-colour mapping (accent → yellow, interactive → magenta, success → green, danger → red, muted → bright-black).

Force it with `ui.colour_depth = "truecolor" | "256" | "16"`.

## Checking contrast

Planned: `review-buddy theme check <id>` will print each text role's contrast against `background` (or `#050507` / `#fbfaf6` for transparent themes, depending on `appearance`). It warns below 4.5:1, or 3:1 for `muted`.
