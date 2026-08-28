# Design specification — AnotherCrewLink client GUI

**This specification is written to be implemented in Rust.** `crates/acl-*` with a native
GUI in `crates/acl-ui` is the client going forward; the Electron / Node + TypeScript
client in `src/renderer` is being wound down and appears here as the reference
implementation — it is where every value below was measured, and it is not the thing to
extend.

Two consequences for anyone building against this document:

- Numbers in px are the **behavioural intent**, not a pixel contract. Match the ratios and
  the meanings; egui's own affordances (combo boxes, sliders, hover text) replace MUI's.
- Where the Electron client and `crates/acl-ui` disagree, the Rust file wins, and the
  disagreement is called out in the section.

§-references point at the client's own `docs/rust-port/03-target-architecture.md`.
For the egui side of every value below — `Style`, fonts, widget mapping, and what egui
cannot do — see `egui-implementation.md` beside this file.

The Rust port grants itself one licence, and it is worth restating here because it governs
every decision below: *"the Rust UI will not be pixel-identical to the React one. Layout,
spacing and control affordances will differ. What must not differ is what every control
does."*

---

## 1. Window

| Property | Value | Source |
| --- | --- | --- |
| Frame | Frameless, always-on-top optional | `main/index.ts` → `acl-ui/window_state.rs`, `alwaysOnTop` setting |
| Width | 400px typical, **250px minimum** | `acl-ui` main view arithmetic |
| Height | Resizable; content is a flex column | `Voice.tsx` `classes.root` |
| Background | `#25232a` | `css/index.css` |
| Title bar | 24px tall, `#1d1a23`, whole strip is the drag region | `App.tsx` |
| Resize strip | 4px, `-webkit-app-region: no-drag`, above the title bar | `App.tsx` `resizeStrip` |
| Scrollbars | 8px, transparent track, `rgba(255,255,255,.2)` thumb, radius 5 | `css/index.css` |

**Title bar contents.** App name plus version, centred, in `#ba68c8`. Three 30px icon
buttons at `#777`: settings and reload flush left (offsets 0 and 22px), close flush right.
The settings button stays clickable while the settings sheet is open — both it and the
sheet's back arrow must perform the same commit-and-reload work.

## 2. States

The window has exactly two states, chosen by whether the game is running:

**MENU** — "Waiting for Among Us" (20px), a 40px purple circular progress, "Open via"
(24px), then the split launch control. On an error, the whole column is replaced by `ERROR`
in red (h6), the error text with `white-space: pre-wrap`, a "Get support" link to Discord,
and a Reload button.

**VOICE** — in a lobby or a game:

- Top row: your crewmate (left, ~100px column), your name (20px, ellipsised) and the lobby
  code stacked centre, mute and deafen icon buttons in a 2-row grid at the right with 26px
  of top padding.
- A divider.
- The roster: a wrapped, scrollable grid of `32%`-wide cells (`min 60px`, `max 120px`,
  8px padding). It takes all remaining height (`flex: 1 1 auto; min-height: 0`).
- Each cell is a crewmate with, on hover, a tooltip holding the player's name, a per-peer
  mute toggle and a 0–200% volume slider. The slider commits on mouse-leave.

**Your own slot is not in the wrapped list.** It sits above it, larger. This is where you
check the two things only you can be — muted and deafened — and the Rust port records a
regression from omitting it.

## 3. The lobby code

Source Code Pro 500 at 28px, `padding: 5px`, `border-radius: 5px`, tinted with the local
player's crew colour, centred with `margin: 5px auto`. When "Show Lobby Code" is off it
reads `LOBBY`. A code is the credential that gates entry to a game: treat it as such in
any design that might appear on a stream.

## 4. What a player must communicate

Four facts, in this order of precedence, and none of them may depend on artwork having
downloaded:

1. **Bugged** — red `error` badge.
2. **Disconnected** — `wifi_off` badge; in the Rust port a `#d25a5a` ring.
3. **Connected, no audio** — `link_off` badge on `#e67e22`/`#694900`; Rust ring `#dcb450`.
4. **Muted / deafened** — `mic_off` / `volume_off` on `#ea3c2a`/`#690a00`.

Plus two continuous states: **talking** (a green ring *outside* the body — never a fill,
because it has to stay legible against all twelve crew colours) and **dead** (the whole
avatar at 35% opacity, still recognisable, because knowing *who* is dead is the point).

A healthy connection draws nothing. An indicator that is always on is one nobody reads.

Every one of these must also be available as words on hover. The Rust port spells this out:
`"Red — connected"`, `"deafened — you cannot hear anybody and nobody can hear you"`.

## 5. Crewmate geometry

The Rust GUI draws the crewmate **uncropped and square** (`views/main.rs`: `SLOT` 76,
`AVATAR` 52, `OWN_SLOT` 96, `OWN_AVATAR` 68) where Electron clips it to a circle with a
border. Both are supported by `Crewmate` — `shape="sprite"` and `shape="circle"` — and
new work should draw the sprite.

When artwork is available: a square sprite, because cosmetics are positioned as fractions
of its width — fitting it into a non-square box moves every hat.

When it is not (and on the first frame of every session it is not), draw three circles at
`half = size / 2`:

```
radius = half - 5          shadow, at centre, colour = pair[1]
body   = radius - 2        offset (-1.5, -1.5), colour = pair[0]
visor  = radius * 0.42     at (+radius * 0.35, -radius * 0.25), #BEE3F5
talking ring               stroke 3px, #2ecc71 / rgb(80,220,120), outside the body
```

Scale every number by `half / 26` when drawing at another size. Border width elsewhere is
`max(2, size / 40)`.

## 6. Settings sheet

Not a route. A full-height sheet at `z-index: 99`, below the title bar, `#171717ad` with
`backdrop-filter: blur(4px)`, sliding `translateX(-100%) → 0` in 100ms ease-in-out.
Header: 40px, centred "Settings" (h6), back arrow pinned right.

Body: a single scroll column, 8px top / 16px sides / 56px bottom padding, sections
separated by full-width dividers with 16px above and below. Section order, and it is
deliberate — the things a host changes for everybody come first:

1. **Lobby Settings** — voice distance slider (1–10, step 0.1) then eleven checkboxes.
2. **Audio** — microphone select, 200×8 level meter, speaker select, test-speaker button,
   then the three-way radio group (Voice Activity / Push to Talk / Push to Mute).
3. Microphone volume (0–300, step 2) and sensitivity (0–1, step 0.05), each gated by its
   own checkbox at 3 grid columns against the slider's 8. Then master volume (0–200), crew
   volume as ghost, ghost volume as impostor.
4. **Keyboard Shortcuts** — four read-only fields in a 2×2 grid; they capture the next key
   or mouse button rather than accepting text.
5. **Overlay** — always-on-top, enable, and — only when enabled — compact, meeting, and the
   seven-way position select.
6. **Advanced** — NAT fix, and the voice-server dialog.
7. **BETA/DEBUG** — VAD, hardware acceleration, echo cancellation, spatial audio, noise
   suppression, samplerate debug. Then the language select.
8. **Streaming** — show lobby code. **Troubleshooting** — restore defaults, reset offsets.

**Rules for this screen.**

- A control whose consequence is not obvious raises a confirmation first: title
  "Are you sure?", body = the specific consequence, actions Confirm / Cancel.
- A lobby rule is only editable when this client is host *and* in a lobby or menu.
  Disabled rows carry a tooltip that says which condition failed.
- **Nothing in the panel writes a setting.** Controls emit a change and the caller applies
  it (`views/settings.rs` `Change::{Set, Run, Capture}`), because a warning outlives the
  frame that raised it and the lobby rules are not always this client's to write.
- Settings live in `config.json` at the paths `SettingsStore.tsx` used — lobby rules under
  `localLobbySettings` — so a machine can move between the two clients without losing
  anything.
- Changing a device or the server URL requires a reload; the panel says so with a pinned
  info alert ("Exit settings to apply changes") and reloads on close.

## 7. In-game overlay

Drawn over the running game, so: transparent, non-interactive, and never a surface.

| Position | Veil | Shape |
| --- | --- | --- |
| Menu (game not in a round) | `rgba(0,0,0,.85)` | 100px wide, radius 8, offset 8/60 |
| Top centre | `rgba(0,0,0,.5)` | 800px wide, 8px top padding |
| Bottom left | `rgba(0,0,0,.35)` | auto width |
| Left / Right | none, or `#25232ac0` capsule in the `1` variants | 300px, 10px padding, column |
| Compact | none | 9vh wide; a capsule with one rounded end (radius 25) |

Avatars are 100px nominal, sized to `7.5 * (10 / count) vh` on the side positions and
capped at `7.5vh`. Names are 11px, bold, in a `rgba(0,0,0,.32)` pill (radius 40) on the
side positions, fading over 400ms. On the right-hand positions the crewmate is mirrored.

**Compact mode draws only players who are talking** — the overlay disappears entirely when
nobody speaks. That is the feature, not a bug to fix.

**Meeting overlay** is a separate layer that positions itself against the game's own
discussion tablet (two ratio regimes: the old iPad hud at 854/579, and the current one at
~1.72). Each talking player's tile gets `box-shadow: 0 0 h/100 h/100 <crew colour>` and
fades in over 400ms; nothing else is drawn.

## 8. Lobby browser

A separate window. 15px top padding, 20px body padding, `Public Lobbies` in bold 14px, then
a sticky-header table: head `#1d1a23`, rows alternating `#25232a` / `#1d1a23`, 14px body,
`max-height: calc(100vh - 130px)`. Columns: Title, Host, Players, Mods, Language, Status
(`Lobby`/`In game` + elapsed `mm:ss`), then a right-aligned contained-secondary
**Show code** button.

The button is disabled — with a tooltip naming the reason — when the game is in progress,
the lobby is full, or the mods do not match. Choosing it opens a dialog with the code and
region; the client no longer writes the code into the game.

## 9. Accessibility and honesty

- Every colour-coded state is also words on hover.
- Every disabled control explains itself.
- 14px is the floor for body text in the window; 11px only for overlay names drawn over
  the game.
- The window must work at 250px wide: fifteen players wrap to five rows there.
- Do not add an indicator for a healthy state, and do not use a signal colour decoratively.
  Both make the real signals harder to see.
