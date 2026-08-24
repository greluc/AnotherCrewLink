# AnotherCrewLink Changelog

## Unreleased

### Changed

- **AnotherCrewLink no longer writes anything into Among Us, and no longer asks for
  permission to.** Until now it opened the game with full access — the right to
  change its memory, allocate executable memory inside it, and start threads in it —
  and held that for as long as the game was running. It used a little of it: it
  patched two of the game's functions so its name and address showed in the corner of
  the main menu. That stamp is what you will notice missing. Nothing else about the
  app changes; the whole point of reading the game is to know where everyone is, and
  reading is all it does now. It asks the operating system for permission to read,
  and nothing more.

  If you have ever wondered whether a proximity chat mod could be mistaken for a
  cheat, this is the honest answer to it: there is now no code in this app that can
  change the game, and no permission held that would let it.

- **A second feature went with it.** Clicking a lobby in the browser was once meant to
  make the game join by itself, by writing the code into it. That was switched off
  long before this fork existed and never worked. The code is now shown to you to
  type or paste, which is what already happened in practice.

### Fixed

- **Writing to the game could fail silently.** The library the app used to read Among
  Us never checked whether a write succeeded — it reported success either way. Nothing
  depends on it now, and the functions have been removed so nothing can start.

## v1.0.3

A security release. Update it.

### Fixed

- **Any member of a lobby could read every other player's position and role.**
  Hosting for mobile players is opt-in, through the mobileHost setting, and the
  broadcast did not check it: it depended only on an internal flag that an incoming
  `mobilePlayerInfo` message switched on, and that message was handled before the
  check on who sent it. What it broadcasts is the whole game state — every player's
  coordinates, impostor flag, dead flag and vent state, five times a second — to a
  room named after the lobby code with `_mobile` appended. The server relays a signal
  to whatever target the sender names without checking that they share a lobby, and
  its join handler accepts any string as a room name. Knowing a six-character lobby
  code was therefore enough to switch the broadcast on in someone else's client and
  then read it: a working wallhack, including who the impostors are. The broadcast now
  requires the local setting, and the message is handled below the sender check.
- **The overlay secret was predictable.** It came from
  `Math.random().toString(36).substr(2, 9)`, which is neither unpredictable nor
  reliably nine characters, and it names the room the overlay feed is published to —
  the same positional payload. New secrets come from the platform's cryptographic
  random generator. Existing secrets keep working; regenerating one changes the
  overlay URL with it.

The server-side half — refusing to relay a signal whose target is not in the sender's
lobby — needs a logging period before it can be enforced without breaking older
clients, and is planned for 1.0.5.

This code is inherited from BetterCrewLink, so other forks are likely to be affected.

## v1.0.2

### Fixed

- **Impostors hear ghosts made ghosts inaudible instead of audible.** The setting
  routes the voice through a convolver carrying a reverb impulse response, which is
  fetched and decoded in the background. A convolver whose buffer is still null does
  not pass audio through, it outputs silence: measured in Chromium, a dirac impulse
  renders at peak 0.04 and a null buffer at exactly 0. Connections that came up before
  the file finished decoding held such a convolver for the rest of the session,
  because the buffer was assigned once at creation and never again. The effect is now
  skipped while the impulse response is missing, connections created too early get it
  once it arrives, and a failed decode is reported.
- Applying an effect disconnected the direct path before connecting the effect, so a
  failure in the second step left the player with no output at all, and it reported
  success either way.
- **Hearing through cameras** indexed the camera table of the current map without
  checking that the map or the camera id is in it. It was unreachable while the map
  was undefined; with the map fixed in 1.0.1 it would have thrown out of the audio
  pass and silenced every player at once.

### Note

Two more lobby settings were dead for the same reason as walls block audio, and were
already fixed by 1.0.1: **communications sabotage prevents conversations** and
**hearing through cameras** both branch on the map, and with the map arriving as
undefined neither found a matching branch, so sabotage never registered and the
current camera was always none.

## v1.0.1

### Fixed

- **Walls block audio did nothing.** The setting itself arrived correctly; the map did
  not. The pointer the game options are read through resolves to zero on Among Us
  17.4.0, so the map went out as undefined, and a collider lookup for an undefined map
  reports that no wall is ever in the way, for every pair of players, silently. The map
  now falls back to ShipStatus, which carries the same value behind a different
  signature. Measured against a live session on Polus. **This turns the setting on for
  real:** lobbies whose host has it enabled will notice that walls now block.
- **A speaking ring that stayed lit on a player who had gone offline.** Talking state
  was only written for players still connected, so whoever dropped out mid-sentence
  kept theirs for the rest of the session. A player whose connection died also kept
  counting as connected, instead of falling back to the no-voice marker.
- **Settings were only applied when the panel was closed with the back arrow.** The
  title bar's settings button closes it too, and that path dropped every lobby setting
  changed in the session and skipped the reload a newly picked microphone or speaker
  needs. Both buttons now do the same thing.
- **Error messages on the first start after installing.** The memory offsets are
  fetched on first run, and the fetch made one attempt per host before telling the
  user to check their internet connection. raw.githubusercontent.com rate limits per
  IP, so a household starting the app at the same time could be turned away. Both
  hosts are now retried with a growing pause, and a hanging request is cut off.
- Every failed update check also produced an unhandled promise rejection.
- The loop meant to silence peers who left the game iterated with `for...in` over an
  array and never touched a peer.

### Added

- **A log file**, under `%APPDATA%\AnotherCrewLink\logs`. Both renderer windows, the
  overlay and the main process, with crash and unresponsive events, rotating at four
  megabytes. Reports of "we could not hear him" can now be answered from the state
  that caused it rather than from a description.
- The log records why a specific player cannot be heard, and says so when the game
  lists one player twice.

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
- **Losing a voice connection meant restarting the app.** A peer connection that
  failed was never rebuilt: new ones were only created when a socket joined the lobby
  or sent an offer, and a peer already in the lobby does neither. A connection that
  never completed was invisible on top of that, because an offer nobody answers leaves
  it in the `new` state, where ICE never starts and so can never fail. Connections now
  give up after twenty seconds without coming up and are rebuilt with a growing delay,
  with only one of the two ends offering so the attempts cannot collide.
- Server: departures were never announced, so the only sign that a player had left was
  their connection failing, which is what a broken connection looks like too. A `left`
  event now distinguishes the two. Leaving also ran the lobby cleanup twice, and the
  occupant count was only ever incremented.
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
  action to a commit SHA. Dependabot keeps those pins moving, CodeQL runs on both
  repositories, and the server has tests for the first time.
- The installer no longer carries a second copy of every bundled library. Only the
  native modules and the updater have to be resolvable at runtime; everything else is
  in the bundle already. The app payload dropped from 84 MB to 11 MB.
- Install scripts are an explicit allow-list, so a dependency that starts running code
  at install time is noticed rather than silently trusted.
