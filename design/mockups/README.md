# Client mockups — a visual reference to implement against

Every view of the AnotherCrewLink client, at the size it really is, built from the same
components as the rest of this design system. They exist for one job: **an implementation
can be held up against them and checked.**

Open `index.html` for the contact sheet.

## The window is resizable

Frameless, **250px minimum**, ~400 typical, no maximum. So no window mockup is one fixed
picture, and a layout that only works at 400px is not finished. Every window view here can
be dragged by the purple grip, jumped to a preset, or requested at any size:

| | |
| --- | --- |
| `?bare=1` | No toolbar, no callouts — the frame alone at exactly the stated size. **This is the reference to screenshot against your build.** |
| `?w=250&h=520` | Any size. Verify the minimum with a full fifteen-player lobby, not just the default width. |
| `?annotate=0` | Hide the measurement overlay (on by default). |
| `?grid=1` | 8px grid — the client's spacing step. |
| `a` `g` `[` `]` | Toggle measures, toggle grid, step the width by 10px. |

Callouts are measured off the live DOM, not written down, so they cannot drift from the
layout they describe.

## The views

| File | View | Recreates |
| --- | --- | --- |
| `window-waiting.html` | MENU — no game running | `Menu.tsx`, `LaunchButton.tsx` |
| `window-error.html` | The error column | `Menu.tsx`, `SupportLink.tsx` |
| `window-lobby.html` | VOICE — in a lobby. **The reflow view** | `Voice.tsx`, `Avatar.tsx` |
| `window-self-states.html` | Live · muted · deafened, side by side | `Voice.tsx` |
| `window-settings.html` | The settings sheet, with section jumps | `settings/Settings.tsx` |
| `lobby-browser.html` | The separate lobby-browser window | `LobbyBrowser/LobbyBrowser.tsx` |
| `overlay-positions.html` | Top · bottom left · left · right | `Overlay.tsx`, `css/overlay.css` |
| `overlay-compact.html` | Compact — talkers only | `Overlay.tsx` |
| `overlay-meeting.html` | The discussion-tablet layer | `Overlay.tsx` |
| `player-states.html` | Every player state and its words | `Avatar.tsx`, `views/main.rs` |
| `crew-colours.html` | All twelve pairs, alive and dead | `src/common/playerColors.ts` |

`reference.json` is the same information machine-readable — window metrics, state
precedence, crewmate geometry, overlay veils, settings ranges and rules, motion durations.
Assert against it rather than against a screenshot where you can.

## What to check

- **What every control does**, and what every state communicates.
- **The order of things** — the settings sections, the state precedence.
- **The reflow.** Step the width and watch the roster change count per row, and the lobby
  table start scrolling sideways instead of crushing its columns.
- **The colour meanings.** `#00ff00` is only ever a hovered border; `#2ecc71` is only ever
  someone speaking.
- **That nothing is drawn for a healthy state.** An indicator that is always on is one
  nobody reads.
- **That every colour-coded state is also words on hover.** egui has no `title` attribute
  to inherit: if you do not call `on_hover_text`, the information is gone.

## What not to check

**Pixel parity.** The client is being reimplemented in Rust with egui, and the spec grants
that layout, spacing and control affordances will differ — what must not differ is what
every control does. Numbers here are intent, not contract.

See `../guidelines/client-gui-spec.md` for the written specification and
`../guidelines/egui-implementation.md` for the egui translation.

## Notes

- The gradient behind the overlays stands in for the running game. It is not part of the
  design; do not sample colours from it.
- Crewmate bodies are local and recoloured on a canvas; hats stream from the pinned
  `AnotherCrewLink-Hats` CDN commit. Cosmetics need a network, bodies do not.
- Talking states in the window mockups are on a timer. There is no audio, no memory
  reading and no sockets anywhere in here.
