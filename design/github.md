repo: greluc/AnotherCrewLink
branch: nightly

## Direction

The client is being reimplemented in Rust (`crates/acl-*`, GUI in `crates/acl-ui`).
The Electron / Node + TypeScript client under `src/` is being wound down and is the
reference implementation for this design system, not its target. The server is already
Rust; its Node implementation was removed on 2026-08-24.

## Related repositories

- greluc/AnotherCrewLink-Server (branch master) — status page, wire protocol
- greluc/AnotherCrewlink-Offsets (branch main) — data only
- greluc/AnotherCrewlink-Offsets-Generator (branch main) — CLI only
- greluc/AnotherCrewLink-Hats (branch main) — sprite data, not copied here
- greluc/AnotherCrewlink-HatTools (branch main) — CLI only

## Last sync

date: 2026-08-28T11:04:52Z

### Updated in this project

- Added `mockups/`: every client view at real size, resizable, with DOM-measured callouts and `reference.json`.
- Read `crates/acl-ui/{Cargo.toml, renderer.rs, lib.rs}` — egui 0.36.1 on wgpu; wrote `guidelines/egui-implementation.md`.
- Recorded the Rust direction across readme, spec, both kit READMEs and 13 component prompts.
- Server status page rebuilt in the brand's language; the shipped page kept as `verbatim.html`.

## Sync history

### 2026-08-27T18:35:19Z

- Built the design system from the client's renderer and the Rust port's views.
- Tokens lifted verbatim: shell greys, MUI purple/red accents, signal colours, the twelve crew pairs.
- 23 components across core, forms, feedback, game and navigation.
- Client UI kit (waiting, in-lobby, settings, lobby browser, overlay) plus the server status page.

## Screen map

| Screen / file | Built from |
| --- | --- |
| ui_kits/client/index.html | src/renderer/App.tsx, src/renderer/index.html |
| ui_kits/client/WaitingScreen.jsx | src/renderer/Menu.tsx, LaunchButton.tsx, SupportLink.tsx |
| ui_kits/client/VoiceScreen.jsx | src/renderer/Voice.tsx, Avatar.tsx |
| ui_kits/client/SettingsScreen.jsx | src/renderer/settings/Settings.tsx, MicrophoneSoundBar.tsx, ServerURLInput.tsx |
| ui_kits/client/LobbyBrowserScreen.jsx | src/renderer/LobbyBrowser/LobbyBrowser.tsx |
| ui_kits/client/GameOverlay.jsx | src/renderer/Overlay.tsx, src/renderer/css/overlay.css |
| ui_kits/server/index.html | AnotherCrewLink-Server src/web.rs |
| tokens/*.css | src/renderer/theme.ts, css/index.css, css/overlay.css, src/common/playerColors.ts |
| components/game/Crewmate.jsx | crates/acl-ui/src/views/main.rs, views/colour.rs |
| guidelines/client-gui-spec.md | all of the above, plus crates/acl-ui/src/views/settings.rs |
| guidelines/egui-implementation.md | crates/acl-ui/{Cargo.toml, renderer.rs, lib.rs}, views/settings.rs |
| ui_kits/client/MeetingOverlay.jsx | src/renderer/Overlay.tsx, css/overlay.css |
| mockups/*.html | the ui_kits/client screens they mount; reference.json from all sources above |
