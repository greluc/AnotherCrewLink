# UI kit — AnotherCrewLink client

> **The client is being reimplemented in Rust.** `crates/acl-*` in
> [greluc/AnotherCrewLink](https://github.com/greluc/AnotherCrewLink) is the
> implementation going forward, with a native GUI (egui) in `crates/acl-ui`. The
> Electron / Node + TypeScript client in `src/` is being wound down: it is the reference
> for behaviour and for every value in this design system, not the target to build on.
>
> So read this kit as **the specification the Rust GUI has to satisfy**, screen by
> screen — what each control does, what each state must communicate — rather than as
> markup to port. `docs/rust-port/03-target-architecture.md` §4.8 grants the licence:
> layout, spacing and control affordances will differ; what every control *does* must
> not.

A recreation of the client's five surfaces, built from `src/renderer` (branch
`nightly`) and cross-checked against `crates/acl-ui/src/views/**`.

| File | Recreates | Rust counterpart |
| --- | --- | --- |
| `WaitingScreen.jsx` | `Menu.tsx`, `LaunchButton.tsx`, `SupportLink.tsx` | `views/main.rs` (empty roster state) |
| `VoiceScreen.jsx` | `Voice.tsx`'s UI half and `Avatar.tsx` | `views/main.rs`, `roster.rs`, `worn.rs` |
| `SettingsScreen.jsx` | `settings/Settings.tsx` | `settings_screen.rs` + `views/settings.rs` |
| `LobbyBrowserScreen.jsx` | `LobbyBrowser/LobbyBrowser.tsx` | `lobby_list.rs`, `views/lobby_browser.rs` |
| `GameOverlay.jsx` | `Overlay.tsx` + `css/overlay.css` | `overlay_layout.rs` |
| `index.html` | The window shell and the view switch | `window_state.rs`, `renderer.rs` |

## What is faithful, and what is not

Faithful: window and title-bar metrics, the 8px spacing scale, every colour, the
32%/min 60/max 120 avatar grid, the settings section order and control types, the
striped lobby table, and the overlay's four veils.

Not faithful, deliberately:

- **The game behind the overlay** is a flat gradient, not a screenshot.
- No audio, no memory reading, no sockets. Talking states are on a timer.
- Avatars are real: `Crewmate` recolours the client's body template and composites
  cosmetics from the pinned Hats CDN. `shape="circle"` is the Electron frame;
  `shape="sprite"` is what the Rust GUI draws.
