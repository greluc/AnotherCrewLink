# AnotherCrewLink Changelog

## v1.0.0

First release under this name. AnotherCrewLink forks
[BetterCrewLink](https://github.com/OhMyGuus/BetterCrewLink) at v3.1.4 and modernises
it from the toolchain up.

### Breaking

- **The official BetterCrewLink server no longer works.** The client speaks
  socket.io 4 (Engine.IO v4); `bettercrewl.ink` runs socket.io 2 and the two are not
  interoperable. The default server is now `https://aucl.greluc.me`.
- Settings live in a new directory, because the application name changed. Existing
  BetterCrewLink settings are not carried over; window size is.
- The GitHub, Discord and donate buttons in the footer were removed.

### Added

- The main window is resizable. It was previously locked to 250x350 by matching
  minimum and maximum bounds, and the saved size was never read back. The layout is
  fluid: the avatar grid and avatar sizes follow the window, and the footer no longer
  had to be hidden above six players.

### Fixed

- **Hot microphone.** The push-to-talk counter only counted down while the local
  player was an impostor, so a state change between pressing and releasing left the
  microphone open until restart.
- **One player unable to hear one specific other player.** Four separate causes:
  trickled ICE candidates were dropped because only signals carrying a `type` were
  forwarded; a race dropped signals arriving in the same tick as the join; an orphaned
  connection tore down its own replacement on offer glare; and `natFix` discarded the
  server's ICE configuration in favour of a hardcoded foreign relay.
- **Microphone and volume settings not surviving a restart.** Per-player volumes were
  deleted wholesale once the map passed fifty entries, and the microphone was stored
  as a Chromium device id, which changes across driver updates and re-plugging.
- The overlay renderer could crash: a hook was called after an early return.
- IPC listeners, the microphone stream and the AudioContext leaked one set per game
  session.
- The hats download had no error handling, so one failed request disabled hats for
  the rest of the session.
- The old meeting overlay used its width as its height.
- The lobby browser never closed its socket.
- Left arrow was mapped to the Home key. CapsLock could be bound but never worked.
- Content-hashed hats were re-rendered every five minutes.
- Server: a new host was announced to the room the socket was in *before* joining.

### Changed

- Electron 11 to 43, and the build moved from the unmaintained electron-webpack and
  webpack 4 to electron-vite. React 17 to 19, MUI 5 to 9, TypeScript 4.6 to 6.
- socket.io 2 to 4 on both client and server.
- The three native modules are vendored under `native/` at pinned commits instead of
  being pulled from unpinned GitHub branches. memoryjs additionally did not compile
  with any current toolchain and handed V8 an external buffer, which current Electron
  rejects outright.
- Dropped unmaintained dependencies: simple-peer (replaced by a native WebRTC
  wrapper), @mui/styles, react-tooltip-lite, typeface-varela, valid-url, node-fetch,
  electron-window-state. structron is vendored.
- 134 dependency advisories down to zero, on both client and server.
- The hat collection is pinned to a commit rather than tracking a branch.
- CI installs with `npm ci`, fails on high or critical advisories, and pins every
  action to a commit SHA.
