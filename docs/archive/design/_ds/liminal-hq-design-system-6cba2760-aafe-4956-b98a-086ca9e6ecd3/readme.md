# Liminal HQ Design System — "Afterglow"

> Digital tools for the spaces in between.

Liminal HQ is an independent studio in Kitchener, Ontario (est. 2025, Scott Morris) building **local-first software for calm, durable personal computing**. Its products span desktop, terminal and Android, and share one brand — **Afterglow**: the soft orange, rose and purple that linger on the horizon after the light goes, set against a near-black void. Dark-first.

Principles: **local first** (the cloud is optional, not foundational) · **sovereignty** (open formats, portable data — you can always leave) · **calm computing** (bounded, pull-only, pre-triaged, low-fidelity glances) · **craftsmanship over novelty**. Licensing is Apache-2.0 OR MIT.

## Sources

Built by reading these repositories (explore them for anything not captured here — they are the ground truth):

- https://github.com/liminal-hq/liminal-hq.github.io — liminalhq.ca (Next.js + Tailwind 4). `src/app/globals.css`, `src/components/*`. **Primary source.**
- https://github.com/liminal-hq/afterglow-themes — Afterglow colour themes (OpenCode, VS Code, Midnight Commander, Claude Code, Codex, browsers). `themes/*.json` nine-step ramps, `docs/theme-format.md`. *(The brief called it "afterglow-thmems".)*
- https://github.com/liminal-hq/spindle — DVD/Blu-ray authoring studio (Tauri + React). `apps/spindle/src/design-system.css` is the canonical desktop token sheet.
- https://github.com/liminal-hq/cadence — Android training journal + Wear OS (Tauri + React, Material 3). `apps/cadence/src/styles/tokens.css`, `components/ui/*`.
- https://github.com/liminal-hq/waypoint — tabbed file manager (Tauri + React). `packages/chrome/src/tokens.css` (native-flex `--wp-*` tokens).
- Also reviewed (READMEs, hero art, themes): [threshold](https://github.com/liminal-hq/threshold), [flow](https://github.com/liminal-hq/flow), [foyer](https://github.com/liminal-hq/foyer) (`apps/foyer/src/theme/theme.ts`), [mudroom](https://github.com/liminal-hq/mudroom), [jar](https://github.com/liminal-hq/jar), [flicker](https://github.com/liminal-hq/flicker), [emoji-nook](https://github.com/liminal-hq/emoji-nook), [smdu](https://github.com/liminal-hq/smdu).

## The product family

| Product | What | Platform | Visual flex |
|---|---|---|---|
| **liminalhq.ca** | Studio site + blog | Web | Pure Afterglow: void, ambient light, glass, gradient hero |
| **Afterglow** | Colour themes for editors/terminals | Many | The palette itself — Afterglow / Dark / Light / see-through |
| **Spindle** | DVD / Blu-ray / UHD authoring workstation | Desktop (Linux) | Afterglow desktop shell: dark, orange accent, glass cards |
| **Threshold** | Flexible-window alarm clock ("About, not at") | Android + desktop | MUI + Material You; origin of the shared title bar |
| **Cadence** | Local-first training journal + Wear OS | Android | Material 3, Roboto Flex, seed #7C5CFF, light + dark |
| **Waypoint** | Tabbed, extensible file manager | Linux, Windows 11 | Native: follows system colour scheme, accent and icon theme |
| **Emoji Nook** | Native Linux emoji picker | Linux | Native: Adwaita / Breeze tokens from xdg-desktop-portal |
| **Flow** (`flo`) | Terminal working-memory sidecar | Terminal | Truecolour, uses the terminal's own background |
| **Mudroom** / **Foyer** | Calm triage for files / messages | TUI / Android | Calm palette: #14161a, dusty blue, warm clay, radius 16 |
| **Flicker** | Homelab ops console (mascot Jax 🦦) | Terminal | Cinema metaphors, "curtain-red" confirm only for destructive |
| **SMDU** | Disk usage analyser | Terminal | Themes (Default, Classic, Dracula), heatmap bars |
| **Jar** | Desktop aquarium toy | Desktop | Playful exception: Classic 98, Paper Notebook, Handheld LCD, Neon Terminal dialog themes |

---

## Content fundamentals

**Voice:** quiet, human, craftsmanlike. Confident but never loud. Writes like a thoughtful maker explaining a decision, not a marketer selling one.

- **Canadian English, always** (house rule — US spelling only where an API demands it, e.g. CSS `color`, `center`): colour, behaviour, centre, organisation, prioritise, analyser, licence (noun), artefact, favourite, grey, minimise/maximise. Liminal uses **-ise** forms (organised, prioritise), matching its repos. The site's hero line "We prioritize…" is written "We prioritise…" here. Dates in `en-CA` ("Mon, 5 Oct", "Feb 24").
- **Person:** the studio speaks as **"we"** ("We build software that feels more human without becoming less powerful"); the reader is **"you"** ("You should be able to leave any tool without losing your work"). Product UI addresses the user plainly and rarely says "I".
- **Taglines are short, aphoristic, often a twist on a phrase**: "Digital tools for the spaces in between." · "About, not at." (Threshold) · "Catch the pile before it catches you." (Mudroom) · "See everything at a glance." (Foyer) · "The space between frames." (Flicker) · "A tiny simulation in a box." (Jar).
- **Explain the why, kindly.** Empty states and errors say what happened and offer the next step: "No workout yet today — A workout is created the moment you log a set, or you can start one now." · "No issues found. Project looks ready to build." · "Pick up where you left off."
- **Fix-oriented diagnostics.** Validation messages carry a suggested fix and are specific about cause ("…since dvdauthor authors that declaration once per titleset, not per title").
- **No guilt, no alarm.** Calm apps avoid urgency words; buckets are "Waiting on you / Worth a look / Can wait / Noise". The Sill reads "quiet · 2 worth a look · nothing on fire." Always provide a "you're caught up" closure ("That's everything").
- **Casing:** Title Case for page/section headings and buttons on the site ("Selected Work", "Our Approach", "Visit Site →", "Play Now"); sentence case for app actions and descriptions ("Start workout", "Continue workout", "Generate ISO image"). **House rule: no all-caps titles, headings or labels** — section labels, badges and eyebrows are sentence or title case ("Project", "Flagship", "↑ Top"). The live site still uses some uppercase labels; this system deliberately does not. The wordmark is always lowercase **liminal hq** in small caps.
- **Parenthetical honesty**: settings describe trade-offs inline — "Two-pass encoding (slower, more accurate sizing & quality)".
- **Emoji:** sparing, decorative, never semantic. The site uses one emoji per principle tile (💾 🛡️ 🧶) and 🍁 in the footer ("Designed & Coded in Canada 🍁"); Flicker's README uses 🎬 🦦. Never in app chrome or buttons.
- **Unicode arrows** as affordances: "Visit Site →", "↑ Top", "Watch ↑".
- **Middots** separate metadata: "DVD-Video · NTSC", "4 titles · 2 menus", "Chest · Barbell".

## Visual foundations

**Colour.** A near-black void (`#050507`) with text at `#e0e0e0` (body), `#b4bfce` (secondary/lede) and `#7c8796` (muted labels); headings go pure white. Accents: **orange `#ffaa40` (primary, focus rings, active nav)**, rose `#f43f5e`, purple `#a78bfa` (interactive in the base theme, hover border on site cards), cyan `#22d3ee` (blog dates/hover rules), blue `#60a5fa` (app links, info). Semantic: success `#2ec66a`, warning `#fbbf24`, error `#f43f5e`, info `#60a5fa`. Every hue is a nine-step ramp with the brand colour on **step 200** (`tokens/colors.css`). Status is shown as **15%-alpha tint + full-hue text** (badges) or a **6px dot** (issue rows, status bar) — never a solid red block. Each product picks one accent for its project label and card outline.

**Variants.** *Afterglow* (void + orange accent, purple interactive) · *Afterglow Dark* (indigo-black `#0f0e1a`, purple accent, blue interactive) · *Afterglow Light* (warm paper `#fbfaf6`, deeper accents: `#b45309` orange, `#5e44cc` purple, `#2563eb` blue). All text ≥ 4.5:1. Apply with `data-theme="afterglow-dark|afterglow-light"`.

**Signature gradient.** `linear-gradient(135deg, #ffaa40, #f43f5e, #a78bfa)` — used sparingly: app wordmark text in title bars, capacity meters. Hero headline uses `to right, #fff 20%, #ffaa40, #f43f5e` clipped to text. The tagline pill is `90deg, #a78bfa → #f43f5e`. Never as a large page background.

**Backgrounds.** Solid void plus the **ambient light**: three radial washes (purple 10%/20%, rose 90%/60%, cyan 50%/90%) blurred 80px at 0.8 opacity, pulsing opacity 0.6→1 over 10s alternate. The hero adds a 40px hairline grid masked to a radial fade. No photography, no textures, no repeating patterns. Product READMEs each carry an SVG hero banner (stars, horizon glow, a mock window) — see `assets/heroes/`.

**Surfaces & transparency.** "Glass": `rgba(20,20,25,0.4)` (site, with `backdrop-filter: blur(10px)`) to `rgba(20,20,25,0.55)` (app cards), hover `rgba(30,30,38,0.7)`. Hero panel `rgba(255,255,255,0.03)` + blur 20px. Blur is reserved for floating/marketing surfaces; app chrome is solid (`#0c0c10` sidebar/title bar, `#16161c` menus/modals).

**Borders.** Hairline white: 8% (subtle, app default), 10% (site cards), 14% (default/strong UI, footer rule), 22% (strong hover). Focus border = orange. On hover, site cards take their **accent colour** as the border; featured cards swap the border for a 1px accent ring via box-shadow.

**Shadows.** Soft and dark: card `0 2px 12px rgba(0,0,0,.35)`, panel `0 4px 24px .45`, modal `0 20px 60px .6`, lift `0 10px 30px .3`. Warmth arrives as a **faint orange glow** on hover/focus: `0 0 20px rgba(255,170,64,.15)` (stat cards, build button), `0 0 16px rgba(255,170,64,.3)` (primary button hover). No inner shadows except the back-to-top orb.

**Corner radii.** 4px inputs/menu rows/project labels · 8px buttons, menus, chips · 12px app cards · 16px site cards, calm apps · 20px featured cards · 24px hero · pill for badges, CTAs, nav pills.

**Cards.** App: glass fill, 8% border, 12px radius, 20px padding, header row (Space Grotesk 14/600 title + muted meta). Site: 16px radius, 2rem padding, 10% border, `rgba(255,255,255,.02)` fill; featured: 20px radius, 3.5rem padding, 160° white-alpha gradient, accent radial bloom in the top-right corner.

**Type.** **Space Grotesk** for headings (700/600, −0.02em on the site, −0.01em in apps); **Inter** for body/UI (300 for ledes, 400–600 UI); **JetBrains Mono** for code, paths, diagnostic codes. App density is 13px base (12px controls, 11px labels/status). Android apps use **Roboto Flex** with tabular numerals. Never all caps for titles, headings or labels; the small-caps wordmark is the only tracked caps.

**Spacing & layout.** 4px grid (`--space-1…14`), generous: site sections 6rem (4rem mobile), 1200px container with 2rem gutters (1.25rem mobile), card grids `repeat(auto-fit, minmax(280px, 1fr))` with 2rem gaps. Desktop apps: CSS grid shell — 40px title bar, 220px sidebar, 28px status bar, 960px max content column centred. Fixed elements: only the back-to-top pill (bottom-right 1.7rem) and app chrome.

**Motion.** Slow and breathing, never bouncy: transitions 0.15s / 0.25s / 0.4s `ease`; page content fades in 4px over 0.2s; the hero slit **shimmers** (3s); the back-to-top orb **portal-pulses** (6.4s `cubic-bezier(.22,1,.36,1)`); ambient light pulses (10s). Android uses M3 emphasised easing `cubic-bezier(.2,0,0,1)` at 200ms. All motion is disabled under `prefers-reduced-motion`.

**Hover states.** Lighten, don't darken: fills go from 2–4% to 5–7% white; text goes muted → primary/white; borders strengthen or take the accent; the primary orange button darkens slightly to `#e89930` and gains a warm glow. Site principle cards lift `translateY(-5px)`; featured CTA pills slide `translateX(5px)` and fill with the accent (black text). Blog rows gain a 3px cyan left rule + fade and indent 1rem.

**Press/active.** Desktop: no shrink; active nav rows get an orange 10% tint + orange text. Android: Material state layers (8% hover, 10% pressed, 105ms in / 375ms out). Focus is a 2px outline in the accent.

**Imagery vibe.** Cool void with warm horizon light — orange/rose/purple glows, faint stars, thin luminous lines. No photos of people; illustrations are flat vector SVG heroes per product.

**Calm computing rules.** No numeric unread badges (counts only for the user's own objects, e.g. "4 titles"). No alarm red by default — errors use the warm attention role or a small dot. Lists are bounded ("+N more"). Always design the "you're caught up" state. Destructive actions confirm and default to *no*.

**Platform flex.** Android (Cadence, Threshold): Material 3 / Material You, Roboto Flex, 4dp grid, light + dark (`tokens/material.css`, `data-md-theme="dark"`). Desktop native (Waypoint, Emoji Nook): follow the system colour scheme. Waypoint's own theme (`tokens/waypoint.css`, `--wp-*`) is warm stone — dark window `#1c1917`, sidebar `#211e1b`, content `#171412`, raised `#292524`, selected `#4a2a1a`, accent `#f97316` (light: paper `#f4f1ee`, accent `#c2410c`) — with 28px rows and controls and a 12px window radius. The shared `TitleBar` draws Linux, Windows and macOS (traffic-light) controls. Waypoint's own chrome (recreated in its kit; no macOS, as Waypoint does not target it) keeps the title exactly centred and draws GNOME, KDE or Windows 11 controls (Cinnamon desktops use the GNOME ones). Terminal (Flow, Flicker, Mudroom, SMDU): truecolour palettes that **respect the terminal background** (no forced background fill). Flicker's projector-booth palette (`--booth-*`: marquee amber #ffb454, curtain crimson #e25d75, projector cyan #6ecddc) is Jax's home. Flow's palette (`tokens/terminal.css`, from SMDU's default theme): text `#d2d8e1`, muted `#7c8796`, accent `#5aa2ff`, active `#2ec66a`, done `#e6b450`, error `#fca5a5`, lines `#2e3540`, rounded box-drawing panes; brand orange and purple appear only in the `< flo >` header mark. Calm apps (Foyer, Mudroom): `#14161a` bg, `#1b1e24` paper, `#e6e9ef`/`#9aa3b2` text, dusty blue `#9bb4d8`, warmest `#d8a48f`, radius 16, system-ui font.

## Jax 🦦 — the companion

Jax is the Liminal HQ mascot: a tiny box-drawn character who lives in a rounded crimson box in the corner of a terminal app, cycling through little animated scenes. He was born in [smorrisods/jira-tui](https://github.com/smorrisods/jira-tui) (`src/ui/jax_companion.rs`) and moved into the Liminal HQ home with **Jax 2.0** in [Flicker](https://github.com/liminal-hq/flicker) (`src/ui/jax.rs`, `src/plugin/jax.rs`), where he gained **moods** that react to what the tool is doing.

- **Look:** `.---.` / `|●‿●|` / `'--'` in amber (`--booth-accent` #ffb454) with a white face, inside a rounded box in curtain crimson (`--booth-accent2` #e25d75), titled `jax 2.0 · {emoji caption}`. He blinks now and then (`- ‿ -`).
- **Moods:** party 🎉 (an action just succeeded), alarm 😰 (something is erroring: at the splice bench), showtime 🎬 (running the projector), hauling 📦 (shifting crates), and chill, which rotates the classic hobbies: wave 👋, nap 😴, reading the spec 🤓, fishing 🎣, otter break 🦦.
- **Presentations:** a floating 30×8 box; a mini footer dock `●‿● jax 🦦` at narrow widths; and in Flicker his own booth panel with a rolling shift log, a snack counter, and *pet Jax* / *toss a snack* actions.
- **Voice:** lowercase, wry, affectionate. "rewound reel 3 by hand, character building", "told the disks a bedtime story", "Jax 2.0 — now with object permanence".
- **Rules:** always optional and toggleable (`J` in Flicker), never covers content or modals, hidden on welcome, edit and about screens, and frozen under reduced motion. He's the one place emoji are used freely.
- Use the `Jax` component (`components/companion/`).

## Iconography

- **Desktop apps (Spindle, Threshold, Cadence desktop)** use **hand-authored inline SVG line icons** on a 16px grid, `stroke="currentColor"`, `stroke-width 1.5`, no fill, rendered at 16px with 0.7 opacity (1.0 when active). The set is lifted verbatim into the `Icon` component (`components/core/Icon.jsx`): overview, assets, titles, chapters, menus, planner, build, logs, settings, save, play, project, chevron-down, disc (64px welcome glyph). Window controls (minimise/maximise/restore/close) are shared 10px 1px-stroke glyphs.
- **Waypoint** uses its own outline set (16px grid, 1.3px stroke, round caps; folders tinted with 25% accent) for toolbar, file and menu glyphs, and can follow the system icon theme. The kit's `ui_kits/waypoint/Glyphs.jsx` lifts these verbatim from `AppIcons.tsx`, `waypointFileIcons.tsx` and `MenuIcons.tsx`.
- **Android apps (Cadence)** use **Material Symbols Rounded** (Google Fonts icon font, loaded in `tokens/fonts.css`), filled variant (`.is-filled`) for active nav and status (trophy, check_circle). Names in use: today, history, list_alt, monitoring, settings, straighten, trophy, check_circle, circle, sticky_note_2, watch, more_vert, add, remove, close, arrow_back.
- **liminalhq.ca** uses **emoji** inside 40px tinted tiles for its three principles (💾 🛡️ 🧶), 🍁 in the footer, and **unicode arrows** (→ ↑) in CTAs and the back-to-top orb. No icon font on the site.
- No PNG icon sprites. **App marks** (all copied from their repos into `assets/app-icons/`): studio `../liminalhq-mark.svg` (dark rounded tile, two bracket strokes in plum→ember→amber, a pale slit of light between — "the space between"); `spindle-mark.svg` (title-bar brackets mark) + `spindle-icon.png` (app icon: brackets around a disc); `waypoint-mark.svg` + `waypoint-icon.png` (amber folder with a waypoint ring); `threshold-icon.svg`, `threshold-icon-android.svg`; `cadence-icon.svg`; `emoji-nook-icon.svg`; `jar-fish-mark.svg`, `jar-gecko-mark.svg`; `afterglow-icon.png`. Foyer's repo icon is a placeholder copy of Threshold's, and Flow, Mudroom, Flicker and SMDU have no app icon — use the wordmark in plain type for those.

---

## Index

**Root**
- `styles.css` — entry point (imports only). `tokens/fonts.css` · `colours.css` · `typography.css` · `spacing.css` · `material.css` (Android) · `waypoint.css` (native desktop) · `terminal.css` (TUI) · `base.css`
- `thumbnail.html` · `SKILL.md` · `github.md`
- `assets/` — `liminalhq-mark.svg`, `app-icons/`, `heroes/` (README hero SVGs for afterglow, spindle, waypoint, cadence, flow, mudroom, foyer, jar, flicker, emoji-nook + Afterglow banner PNG)
- `foundations/` — specimen cards: `brand/`, `colours/`, `type/`, `spacing/` (spacing, radii, shadows, motion)
- `components/` — React primitives (below)
- `ui_kits/website/` · `spindle/` · `waypoint/` · `cadence/` · `flow/` — click-through recreations

### Components

- **core/** — Button, Badge, Card, Icon
- **forms/** — TextInput, Select, Checkbox
- **feedback/** — StatCard, CapacityBar, IssueRow, EmptyState
- **desktop/** — TitleBar (Linux, Windows, macOS), Sidebar, Statusbar
- **menus/** — ContextMenu (the shared Liminal menu, from Waypoint)
- **marketing/** — AmbientLight, SiteHeader, Wordmark, HeroBanner (+ ThresholdGate), TaglineBadge, SectionHeading, ProjectCard, ProjectLabel, PhilosophyCard, PostList, SiteFooter, BackToTop
- **companion/** — Jax (Jax 2.0, the Liminal HQ mascot 🦦: box, mini dock, booth panel)
- **material/** (Android / Material 3) — MdButton, MdIconButton, MdChip, MdTag, MdSwitch, MdSegmentedControl, MdBanner, MdTextField, MdAppBar, MdBottomNav, MdEmptyState

**Intentional additions:** `Icon` (wraps Spindle's inline SVGs so they're reusable), `Wordmark` and `SectionHeading` (extracted from repeated site markup), `ThresholdGate` (the hero visual as a standalone piece). Spindle's validation row, stat tile and capacity meter are page-level markup in the source, promoted here as `IssueRow`, `StatCard`, `CapacityBar`.

### UI kits
- `ui_kits/website/` — liminalhq.ca home
- `ui_kits/spindle/` — Spindle desktop shell: welcome → overview, logs
- `ui_kits/waypoint/` — Waypoint main window: shared chrome in four control styles, tabs, path bar, Places sidebar, list/grid view; dark/light
- `ui_kits/cadence/` — Cadence Android: Today, exercise logging, history; light/dark
- `ui_kits/flow/` — `flo` TUI: threads, status, capture with slash-command palette and Normal mode

## Notes & caveats
- **Fonts** load from the Google Fonts CDN (the repos ship no binaries): Space Grotesk, Inter, JetBrains Mono, Roboto Flex, Material Symbols Rounded.
- Threshold, Emoji Nook, Jar, Foyer, Mudroom, Flicker and SMDU are documented but have no UI kit.
- The `grey` ramp is named with Canadian spelling in CSS (`--grey-*`); upstream theme JSON uses `gray` because the OpenCode schema requires it.
