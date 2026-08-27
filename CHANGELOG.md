# AnotherCrewLink Changelog

## v1.0.6

This release drops Linux and raises the minimum Windows to 11. If you run AnotherCrewLink
on Linux, this release will not reach you: 1.0.5 was the last one with an AppImage, and
your client will go on reporting that it is up to date. The first note below says what
that means and what your options are.

### Removed

- **Linux is no longer supported.** There is no AppImage in this release and there will
  not be one again. The reason is not that it was broken: it is that nobody working on
  this project has a Linux machine to run it on, and every release was going out with the
  Linux build tested by nothing but the fact that it compiled. Shipping a build under
  those conditions is a promise this project cannot keep.

  What this means if you are on Linux. The 1.0.5 AppImage keeps working exactly as it
  does today — nothing switches off, and the voice server does not stop talking to it.
  What stops is updates. Your client checks a feed of its own that only ever lists Linux
  builds; that feed stops at 1.0.5, so the client will keep saying you are up to date,
  and it will be telling the truth. When a
  future release changes something on the wire, or Among Us moves and the memory offsets
  change, 1.0.5 will stop working and no update will arrive to fix it.

  If you want to keep playing on Linux, [BetterCrewLink](https://github.com/OhMyGuus/BetterCrewLink)
  still publishes Linux builds. It uses a different, incompatible voice server, so
  everyone in your group has to move together.

- **Windows 10 and older are no longer supported, and there is no 32-bit build.**
  Windows 11 is the floor, for the same reason: it is what can actually be tested here.
  The installer is unchanged in name and in how it updates, so nothing about installing
  or updating looks different on a supported machine.

  If you are on Windows 10 this release will very likely still install and run — nothing
  was added that requires 11, and nothing checks your version. It is simply no longer
  tested, so a future release may break it without anyone noticing.

  **On 32-bit Windows the installer now stops and says so.** There is no 32-bit Windows 11,
  so there is no 32-bit build any more — and without one, the update your client fetches is
  the 64-bit installer. It would have run to the end, reported success, and left you with
  files your machine cannot start: an installation that looks finished and does nothing.
  Refusing is not a fix, and nothing here can be one. It is the difference between that and
  a message telling you what happened. 1.0.5 is the last release for those machines and it
  keeps working; your client will go on reporting it is up to date, and that will be true.

### Changed

- **The installer is now built by this project rather than by the packaging tool.** You
  should not be able to tell. It has the same name, installs to the same place, updates the
  same way, and the update from 1.0.5 was tested before this went out.

  It is here because the Rust rewrite has to ship its own installer eventually, and the
  moment to find out whether it handles an update correctly is not the moment several
  hundred machines run it at once. So it is carrying an ordinary release first. If anything
  about installing or updating looks different to you, that is worth reporting — it is the
  one part of this release that is genuinely new.

### Fixed

- **The public lobby list reshuffled itself, and full lobbies could sit above the
  ones you could actually join.** The list is meant to put lobbies you can join
  first, then the fullest of those, because a lobby with eight players is a game
  about to start. One of those rules only worked in one direction, and the result
  was that the same two lobbies came out in a different order depending on the
  order the server happened to send them — so the list rearranged itself between
  refreshes for no reason you could see, and a lobby with no room could appear at
  the top.

  A lobby the server reports as over its own limit now counts as full as well. It
  used to be treated as joinable, and because it also had the most players it went
  straight to the top.

## v1.0.5

A removal release. Two features are gone — hosting for mobile players, and the OBS
browser overlay — and with them the last of the machinery in this app that could write
into another program's memory. If you used the overlay while streaming, this release
changes something for you and the note below says what to do instead. If you did not,
nothing you can see is different.

### Removed

- **Mobile Host is gone.** This was a beta setting that relayed the whole game state —
  every player's position, whether they were dead, in a vent, or an impostor — to a room
  on the voice server named after your lobby code, so that a phone app could follow along
  and place voices for people playing on mobile. That phone app was never released by
  this project. The setting and the broadcast behind it have both been taken out.

- **The OBS browser overlay is gone.** Streamers could switch this on and paste a URL
  into OBS as a browser source to show who was talking, who was dead, and where everyone
  stood. It worked by publishing the same full picture of the lobby through the voice
  server, addressed to a secret string; anyone who had the string could read it. The
  setting, the URL and the feed are all gone, and if you had it in your scene the source
  will now stay blank.

  If you stream and relied on it, the in-game overlay — Settings → Overlay — still shows
  who is talking, and it is captured by capturing the game window.

- **The bundled memory library can no longer write to another process at all.** 1.0.4
  already stopped this app writing into Among Us and stopped asking the operating system
  for permission to. What was left was the ability itself, unused, inside the library
  that does the reading: routines to write memory, to allocate executable memory, to
  change what a page of memory is allowed to do, and to assemble machine code and run it
  inside another program. All of them have been deleted from the library's source. This
  changes nothing you can see; it means there is no longer anything to switch back on.

## v1.0.4

Connections that used to fail quietly now recover, say why when they cannot, and stop
asking for permissions this app has no use for.

### Fixed

- **Some players could not hear anyone, and nobody could hear them, while everything
  else looked fine.** The avatars still lit up when people spoke, because that travels
  over a different connection than the voice does — so from the inside the lobby looked
  perfectly healthy and there was nothing to report but silence. Several things
  contributed and all of them are addressed below.

- **A connection that stalled was left to rot for half a minute.** When the network
  path between two players goes bad, the connection enters a state that often heals by
  itself within a second or two — and when it does not, it takes fifteen to thirty
  seconds before anything notices. This app was not watching that state at all. It now
  waits four seconds and then rebuilds the network path in place, keeping the call
  rather than starting over.

- **The fallback relay was only offered over UDP.** Some networks — school and office
  networks especially, and some mobile providers — block outgoing UDP entirely, and
  those are exactly the networks that need a relay in the first place. Relays are now
  also tried over TCP, and a relay offered over TLS is recognised as one, which it was
  not before.

- **When a direct connection failed, it took a minute per player to try the relay.**
  In a ten-person lobby, a player whose network needs the relay used to rediscover that
  nine more times over. The client now decides from what the failed attempt actually
  found: if the relay answered, it switches at once; if the relay could not be reached
  at all, it says so plainly instead of forcing a setting that would leave the
  connection with nothing to try.

- **Every player was asking the relay for three times what they needed.** Two entries
  in the server's list named the same relay and the client added a third, so every
  connection took three reservations where one would do. With nine other players in the
  lobby that is twenty-seven reservations from a single person — and a relay grants a
  limited number. One player could use up the whole server's supply by themselves, and
  the next person who needed the relay simply got nothing, in a lobby that had worked
  ten minutes earlier. That is exactly what was reported.

- **Writing to the game could fail silently.** The library the app used to read Among
  Us never checked whether a write succeeded — it reported success either way. Nothing
  depends on it any more, and those functions have been removed so nothing can start.

- **A crash on startup left nothing behind to look at.** Two players reported the app
  showing a critical error and closing, with no way to say what it was. It now writes
  the fault to the log and tells you which folder to find it in before it exits.

- **After about ninety seconds of trying, the app gave up on a player for good.** Six
  attempts and then silence for the rest of the round, whatever changed in the meantime.
  The reasons a connection cannot be made are often temporary — a relay with no capacity
  left frees some the moment anybody leaves — and nothing was ever going to ask again.
  It now keeps trying every forty-five seconds, quietly, and says so once rather than
  filling the log.

- **A relay that is simply full now says so.** It is refused with a specific code, and
  told apart from a relay that cannot be reached it is the difference between a network
  problem at your end and a setting on the server. Those are fixed by different people,
  and the log used to give no way to tell which one you had.

- **Assigning the mute or deafen shortcut switched it on immediately.** Both act when
  the key is released, and the setting was saved while it was still held down — so
  letting go of the key you had just pressed to assign mute muted you. There was no
  sound to say so, and the natural conclusion was that the microphone had broken.

- **One use of the impostor radio spoiled every muffled voice afterwards.** The radio
  and the muffling for vents and cameras share one filter, and the radio left it set to
  the wrong kind. From then on a player in a vent, or seen on a camera, had everything
  below 2 kHz stripped out — which is where speech is — so they were thin and hard to
  make out instead of muffled. It lasted until you restarted.

- **Two effects at once made a player far too loud and stuck that way.** A ghost within
  reverb range who then climbed into a vent ended up routed through three paths at once
  and roughly three times as loud, and the app then believed no effect was applied, so
  it never took them out again.

- **The numpad's `+`, `-`, `*`, `/` and `Enter` could be set as shortcuts and did
  nothing.** The settings panel accepted them and the key handler had never heard of
  them.

- **Every player you met left an audio engine behind.** One is created for each person
  and none were ever shut down, so an evening of people coming and going accumulated
  them for as long as the app stayed open.

- **A missing speaker could silence one player and nothing else.** If the speaker you
  had chosen was no longer plugged in, sending a player's voice to it failed and the
  failure went nowhere — no message, no fallback, just one person you could not hear.
  It now falls back to the system default and says so in the log.

- **A failed connection now says why.** It reports what it managed to find — including
  whether the relay could be reached at all — and the app writes down which relays the
  server offered. Two reports of this problem had to be diagnosed by guessing, because
  the log recorded that a connection failed and nothing whatsoever about the reason.

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

- **The library that watches for your push-to-talk key was replaced.** The old one
  carried no licence at all — not a restrictive one, none — which is not something to
  ship inside an installer other people are asked to trust. Your shortcuts are stored
  by name and did not need changing, and two things got slightly better on the way:
  a shortcut typed in lower case now works, where before it silently did nothing, and
  binding plain Shift, Ctrl or Alt again matches either the left or the right key.

  The new one is also told not to report mouse movement at all, so the app never
  receives your cursor position.

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
