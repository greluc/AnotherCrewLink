# AnotherCrewLink — design system

Free, open proximity voice chat for Among Us. AnotherCrewLink reads the game's state out
of memory and mixes every other player's voice by how far away they are, whether a wall is
between you, whether you are dead, and whether the lights are out. It runs alongside the
game; nothing is injected into your account and no game files are modified.

It is a fork of [BetterCrewLink](https://github.com/OhMyGuus/BetterCrewLink) by OhMyGuus,
which in turn forked [CrewLink](https://github.com/ottomated/CrewLink) by ottomated.

> **Implementation direction.** The client is being reimplemented in **Rust**
> (`crates/acl-*`, native GUI in `crates/acl-ui`). The **Electron / Node + TypeScript
> client is being wound down** — it stays the reference for behaviour and for every
> value in this design system, but it is not the target to build on. The server was
> already rewritten in Rust on 2026-08-24 and its Node implementation is gone. When you
> design a new client surface here, design it for the Rust GUI: the Electron screens are
> the specification it has to satisfy, not markup to port. `guidelines/egui-implementation.md`
> is the translation: one `Style` built from these tokens, the widget-by-widget mapping, and
> the four places CSS did something egui cannot.

This folder is the design system for the whole family: tokens taken out of the shipping
client, the reusable primitives it is built from, a recreation of every client surface, a
written specification for the client GUI, and repository marks.

## Sources this was built from

Everything here was read out of the repositories below. Nothing was inferred from
screenshots. Explore them further — they are the ground truth, and the client's own
`docs/rust-port/` is unusually explicit about *why* the UI is the way it is.

| Repository | What it is | What was read |
| --- | --- | --- |
| [greluc/AnotherCrewLink](https://github.com/greluc/AnotherCrewLink) (`nightly`) | The client: Rust (`crates/acl-*`, current direction) and the Electron/TS implementation being retired | `src/renderer/**`, `src/common/**`, `crates/acl-ui/src/views/**`, `README.md`, `CLAUDE.md`, `static/locales/en` |
| [greluc/AnotherCrewLink-Server](https://github.com/greluc/AnotherCrewLink-Server) (`master`) | Rust signalling + lobby-list server | `README.md`, `src/web.rs` |
| [greluc/AnotherCrewlink-Offsets](https://github.com/greluc/AnotherCrewlink-Offsets) | Memory offsets every client fetches | `README.md` |
| [greluc/AnotherCrewlink-Offsets-Generator](https://github.com/greluc/AnotherCrewlink-Offsets-Generator) | Generates those offsets from an installed game | `README.md` |
| [greluc/AnotherCrewLink-Hats](https://github.com/greluc/AnotherCrewLink-Hats) | Hat/skin/visor sprites + `hats.json`, served over jsDelivr | `README.md` |
| [greluc/AnotherCrewlink-HatTools](https://github.com/greluc/AnotherCrewlink-HatTools) | `hat-exporter`, which produces the above | `README.md` |

The products with a visual surface are exactly two: **the client** (a frameless window, an
in-game overlay, and a separate lobby-browser window — today Electron, next a native Rust
GUI) and **the server status page**. The other four repositories are data and command-line tools; they have marks and
nothing else.

## Index

| Path | What it holds |
| --- | --- |
| `styles.css` | The single stylesheet a consumer links. `@import`s only. |
| `tokens/` | `colors.css`, `crew.css`, `typography.css`, `spacing.css`, `radius.css`, `elevation.css`, `motion.css`, `fonts.css` |
| `components/core/` | `Button`, `OutlineButton`, `LaunchButton`, `IconButton`, `Icon`, `SectionHeading`, `Divider` |
| `components/forms/` | `Checkbox`, `RadioOption`, `Slider`, `SelectField`, `TextField` |
| `components/feedback/` | `Alert`, `Tooltip`, `Dialog`, `MeterBar`, `StatusBadge` |
| `components/game/` | `Crewmate`, `PlayerSlot`, `LobbyCode`, `OverlayCapsule` |
| `components/navigation/` | `TitleBar`, `LobbyTable` |
| `ui_kits/client/` | The client window, click-through: waiting, in-lobby, settings, lobby browser, overlay — the spec the Rust GUI has to satisfy |
| `ui_kits/server/` | The server status page: `verbatim.html` as it ships, `index.html` as a branded proposal |
| `mockups/` | Every client view at real size, resizable, with measured callouts and `reference.json` — the visual reference an implementation is checked against |
| `guidelines/` | Foundation specimen cards, `client-gui-spec.md` (the written GUI specification) and `egui-implementation.md` (how it is built in egui 0.36) |
| `assets/logos/` | The mark, two lockups, and one tile per repository |
| `assets/crewmates/` | The client's body templates + `README.md` on how the recolour works |
| `assets/icons/radio.svg` | The client's only bespoke icon |
| `templates/client-window/` | `ClientWindow.dc.html` — the client window as a reusable starting template |
| `SKILL.md` | Agent-skill entry point |

Every component ships `<Name>.jsx`, `<Name>.d.ts` and `<Name>.prompt.md`. Read the
`.prompt.md` first: it says what the thing is for and when *not* to use it.

## CONTENT FUNDAMENTALS

The writing is the most distinctive thing about this project. It is plain, British, and
unusually willing to say what something costs.

**Register.** Short declarative sentences. No marketing voice, no second-person
cheerleading, no exclamation marks outside two inherited UI strings. The README does not
say "easy to use"; it says what happens and what it cannot do.

**Person.** Mostly impersonal — "It runs alongside the game", "The default server is…".
Second person appears for the user's own actions and states: "whether you are dead",
"nobody can hear you". First person plural is almost absent; when the Hats repository has
to speak for the project it says "We are not taking any credits for the images in this
repository."

**Consequences before instructions.** The pattern is: name the thing, say what it costs,
then tell you how. From the client README: "**Smart App Control cannot be switched back on
afterwards without reinstalling Windows.** That is how Microsoft designed it, and it
applies to the whole machine, not to this app."

**Honesty about limits is a feature.** "AnotherCrewLink does not work with the official
BetterCrewLink server." · "Without a relay the server still works, and most players will
still connect directly." · "Short answer: yes, with no hard blockers, but…"

**UI strings.** Title Case for settings labels ("Walls Block Audio", "Hear People in
Vision Only", "Impostors Hear Dead"). ALL CAPS for exactly two states: `MENU` and `ERROR`.
Sentence case for warnings, and warnings are specific: "This will reset ALL settings to
their default values." Buttons are one or two words: Confirm, Cancel, Close, Delete,
Default, Show code, Select File.

**Errors name the fix.** "Couldn't connect to Among Us.\nPlease re-open AnotherCrewLink as
Administrator." Where there is no fix, the string says so: "Your version of Among Us is
unsupported by AnotherCrewLink."

**Code comments.** In the source, comments explain *why*, and record the bug that made a
line necessary. If you write copy for this project, match that instinct: prefer the
sentence that removes a future question.

**No emoji.** One exception exists in the codebase — a 📻 glyph the Rust radio indicator
draws — and it is a substitute for an icon, not decoration. Do not introduce emoji.

**Numbers and units.** Sizes in px, versions as `1.0.6`, dates as ISO (`2026-08-25`),
distances unitless (voice distance 1.0–10.0, default 5.32).

## VISUAL FOUNDATIONS

**The mood.** A small dark tool window that sits beside a bright cartoon game. It is
almost entirely two dark violet-tinted greys, and the only saturated colour on screen is
information: who is speaking, who cannot be heard, which crew colour you are.

**Colour.** `#25232a` body, `#1d1a23` title bar and table head, `#272727` paper, `#313135`
hairline. All four are violet-leaning — never `#000` and never a neutral grey. The accents
are MUI's `purple[300]` (`#ba68c8`, the app name and every primary control) and MUI red
(`#f44336`, contained buttons and the mic meter). Two greys (`#e0e0e0`, `#bdbdbd`) exist
for the updater's dismissive buttons. Icons in chrome are `#777` and nothing else.

**Signal colours are not a palette.** `#00ff00` means "this border is hovered" and appears
nowhere else. `#2ecc71` means "this player is speaking". `#ea3c2a` over `#690a00` means
"muted or deafened". `#e67e22` over `#694900` means "connected but no audio". Do not
reuse any of them decoratively; each is load-bearing. The one alias is `--state-online`,
server liveness on the status page, which shares the talking hex on purpose: a page and a
roster can never appear together.

**The crew palette is the game's, not ours.** Twelve body/shadow pairs in colour-id order
(`tokens/crew.css`). Always use both halves: the client's own source says using the body
colour for the shadow "gives a flat sticker".

**Type.** The window scale is small and coarse: 28 / 24 / 20 / 19 / 14 / 12 / 11 / 10, and
nothing in the client goes outside it. Full-page surfaces read in a browser — the server
status page — add four display steps (`--size-stat` 44, `--size-hero` 34, `--size-lede` 18,
`--size-meta` 13); do not bring them into the client window.

Varela Round for everything — a rounded geometric sans that matches the game's
own lettering without imitating it. Source Code Pro, weight 500, for exactly one element:
the lobby code, at 28px. The scale is small and coarse: 28 / 24 / 20 / 19 / 14 / 12 / 11 /
10. There is one heading level (MUI `h6`, 20px). No letterspacing except a hair on the
code.

**Avatars are real crewmates.** `Crewmate` recolours the client's own body template
(`assets/crewmates/player-base.png`, ported from `src/main/avatarGenerator.ts`) on a
canvas and composites hat / skin / visor artwork from the pinned `AnotherCrewLink-Hats`
CDN commit over it, exactly as `src/renderer/cosmetics.ts` does. `shape="circle"` is the
Electron client's round frame; `shape="sprite"` is the Rust GUI's uncropped crewmate.
Hats need a network; bodies do not.

**Backgrounds.** Flat fills. No gradients anywhere in the product, no images, no
patterns, no textures, no noise. The only "imagery" is player artwork: 270×428 PNG
crewmate sprites fetched at run time from the Hats repository over jsDelivr, composited
front/back around the body. When artwork has not arrived, players are drawn as three
circles — shadow, body, visor — and that drawn form is a permanent fallback, not a
placeholder.

**Layout.** A frameless window, 250px minimum and 400px typical, with a 24px draggable
title bar and a 4px non-draggable strip above it so the top edge stays resizable. Content
is a flex column: the roster takes the slack, footers stay pinned. The roster is a wrapped
grid of `32%`-wide cells, `min 60px` / `max 120px`. The settings panel is not a route —
it is a full-height sheet at `z-index: 99` that slides in from the left over the whole
window.

**Transparency and blur.** Used in two places only. The settings sheet is `#171717ad` with
`blur(4px)` — the one blur in the product. The in-game overlay is transparent by
definition: `rgba(0,0,0,.85)` in menus, `.5` in game, `.35` bottom-left, and `#25232ac0`
for the compact capsule.

**Corners.** 5px (lobby code, tooltip, scrollbar thumb), 8px (overlay), 10px (buttons),
25px (compact overlay capsule, one side only), 40px (overlay name plate), 50% (avatars,
badges). MUI's own controls keep their 4px. Nothing is 6px, 12px or 16px.

**Borders instead of shadows.** There is no shadow system: no `box-shadow` in the client's
own CSS. Depth comes from a flat value step and a border — 4px white on the launch group
(2px where the two halves meet), 2px white on the outline button, 1px `#313135` above each
settings row, 1px `rgba(255,255,255,.3)` around dropdowns. The one glow in the product is
in the in-game meeting overlay: `0 0 h/100px h/100px <crew colour>` around a talking
player's tile, and it is information.

**Cards.** There are none in the marketing sense. `Paper` is a flat `#272727` rectangle at
MUI's 4px radius, and that is the whole card language.

**Hover, press, disabled.** Hover on the client's own buttons recolours the border to
`#00ff00`; nothing moves, scales or fills. Hover on MUI controls is a 12% tint wash.
There is no distinct pressed state — a toggle's selected state *is* the green border.
Disabled is `opacity: .38` plus a tooltip that says why ("Only the game host can change
this!" / "You can only change this in the lobby!").

**Motion.** Four durations: 50ms linear (the mic meter — anything slower reads as lag on
your own voice), 100ms ease-in-out (the settings sheet, overlay opacity), 200ms ease-out
(avatar border colour), 400ms (overlay name and meeting-tile fades). No entrances, no
staggering, no bounces, no spring curves. The one looping animation is MUI's circular
progress on the waiting screen.

**Scrollbars are styled**, because the window is small: 8px wide, transparent track,
`rgba(255,255,255,.2)` thumb at 5px radius, `.3` on hover.

**Imagery temperature.** Cool. Everything is violet-black with cyan-white visors; the
warm colours in view belong to the game's own crew palette.

## ICONOGRAPHY

**The client uses Material Design icons, through `@mui/icons-material`.** Filled style,
24px nominal, drawn at `#777` in chrome and white on avatars. The exact glyphs in the
shipping UI: `Settings`, `RefreshSharp`, `Close`, `ArrowBack`, `ArrowDropDown`, `Mic`,
`MicOff`, `VolumeUp`, `VolumeOff`, `WifiOff`, `LinkOff`, `ErrorOutlined`.

There is **one bespoke SVG** in the whole client: `static/radio.svg`, drawn white at 30px
over the lower-right of an avatar when that impostor is holding the radio. It is game
artwork-adjacent and is not copied here.

Emoji: not used, except the Rust port's temporary `📻` where the radio SVG is not wired up
yet. Unicode characters as icons: only `&rarr;` in the server's status-page prose.

**Substitution — please review.** The icon set could not be copied: `@mui/icons-material`
is an npm package, not files in the repository, and this project has no icon sprite of its
own. This system links **Material Symbols Rounded** from Google Fonts and exposes it as
`<Icon name="mic_off">`. Same family lineage, same ligature names, but Symbols is the newer
variable-axis set — a Rounded/filled-off rendering is very slightly lighter than the filled
Material Icons the client ships. If you want exact parity, drop the MUI SVGs into
`assets/icons/` and I will point `Icon` at them.

**Player artwork is referenced, not copied.** The body templates in `assets/crewmates/`
are the client's own files. Hats, skins and visors stay on the pinned
`AnotherCrewLink-Hats` CDN commit — that repository is extracted from an Among Us
installation and says of itself: "We are not taking any credits for the images in this
repository." `assets/icons/radio.svg` is the client's one bespoke icon. The inherited
`BCL-*` logo art is not copied — see below.

## Logos

AnotherCrewLink ships **no mark of its own**. Everything under
`static/images/logos/` is BetterCrewLink's artwork, inherited through the fork and still
named `BCL-*`; the README renders `256-BCL-Logo-shadow.png` as the project's logo.

So the marks in `assets/logos/` are **new, and abstract on purpose**: a filled centre with
two receding arcs — near, further, furthest — which is what the product does to a voice.
No crewmate silhouette, no game artwork, no reuse of BetterCrewLink's mark.

| File | Use |
| --- | --- |
| `acl-mark.svg` | The ring mark alone |
| `acl-lockup.svg` | Mark + "AnotherCrewLink", for dark surfaces |
| `acl-lockup-dark-bg.svg` | The same on its own `#1d1a23` plate |
| `acl-client.svg` · `acl-server.svg` · `acl-offsets.svg` · `acl-offsets-generator.svg` · `acl-hats.svg` · `acl-hattools.svg` | One 256px tile per repository: same geometry, one accent each (purple / green / gold / orange / pink / cyan) and a 3–5 letter tag |

The wordmark is set in Varela Round, the client's own UI font. **If you would rather keep
the inherited BCL crewmate mark, or have real ACL artwork, send it and I will replace
these.**

## Fonts

Varela Round 400 and Source Code Pro 500 — exactly what the client loads, via
`@fontsource`. No binaries exist in the repositories, so `tokens/fonts.css` pulls both from
Google Fonts. This is a delivery change, not a substitution. Drop `.woff2` files into
`assets/fonts/` if you want the system to be offline-complete.

## Intentional additions

- **`Icon`** — a wrapper for the glyph set, because the client's icons come from a package
  rather than from files. One line of API so consumers do not hand-roll `<span>`s.
- **`OutlineButton`** — the client draws this button inline in `SupportLink.tsx` rather
  than as a component; it appears in two places and needed a name.
- **`PlayerSlot`** and **`OverlayCapsule`** — the roster cell and the overlay container are
  CSS classes in `css/overlay.css`, not components. They are the units every overlay
  layout is built from, so they are components here.

Nothing else was invented: the inventory above is the set of MUI primitives the client
actually configures, plus the drawn crewmate the Rust port defines.
