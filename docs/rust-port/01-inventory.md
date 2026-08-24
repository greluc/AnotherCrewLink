# 1. What exists today

A measured inventory of everything a Rust port would have to replace. Figures are
from the `fix/bugs-and-deps-224r05` branch at version 1.0.2 and from
`greluc/AnotherCrewLink-server` at 1.0.0.

## 1.1 Size

| Part | Language | Lines | Notes |
| --- | --- | ---: | --- |
| `src/renderer` | TS/TSX | 7,023 | UI, voice pipeline, overlay |
| `src/main` | TS | 2,903 | Electron main, memory reader, IPC |
| `src/common` | TS | 666 | Shared types, map colliders, mods |
| `native/memoryjs` | C++ | 2,905 | Process memory, both platforms |
| `native/electron-overlay-window` | C | 1,205 | Overlay attach, Win32 + X11 |
| `native/node-keyboard-watcher` | C++ | 280 | Polled key watcher, Win32 + X11 |
| `vendor/structron` | JS | 802 | Binary struct parsing |
| Server `src` | TS | 476 | Signalling relay, lobby browser |
| **Total hand-written** | | **~16,300** | excluding tests, locales, assets |

Assets that carry over unchanged: 4.0 MB images, 4.0 MB sounds, 460 KB of
translations across **37 locales**.

## 1.2 Dependency surface

| | Packages in lockfile | Non-dev |
| --- | ---: | ---: |
| Client | 699 | 118 |
| Server | 267 | 151 |

Three of the client's runtime dependencies are vendored native modules built from
C/C++ at install time. `postinstall` runs `electron-builder install-app-deps`,
which compiles them against Electron's ABI.

## 1.3 Component map

### Client — main process (`src/main`)

| Module | Lines | Responsibility | OS coupling |
| --- | ---: | --- | --- |
| `GameReader.ts` | 1,223 | Reads Among Us state out of process memory; pattern-scans `GameAssembly.dll`; injects x86 shellcode | **Very high** |
| `index.ts` | 489 | Windows, auto-update, protocol handlers, hardening | Electron |
| `hook.ts` | 239 | Global key hooks, game-read loop, settings store | High |
| `offsetStore.ts` | 219 | Fetches and caches memory offsets from a third-party branch HEAD | None |
| `ipc-handlers.ts` | 171 | Main↔renderer bridge | Electron |
| `avatarGenerator.ts` | 135 | Recolours player sprites per colour id (Jimp) | None |
| `windowState.ts` | 101 | Persists window geometry | Electron |
| `logFile.ts` | 89 | Log file capture | None |
| `vdf.ts` | 68 | Parses Steam's VDF to locate the game | None |

`GameReader` is the single most platform-specific file in the project. It:

- enumerates processes and finds `Among Us.exe`;
- resolves `GameAssembly.dll`'s base address;
- pattern-scans the module for ten signatures to derive offsets;
- walks pointer chains to read players, ship status, meeting hud, game options;
- reads .NET dictionaries and arrays out of the target's address space;
- on 32-bit Windows only, allocates RWX memory in the target with
  `VirtualAllocEx` and writes two hand-assembled x86 shellcode stubs plus two
  `JMP` patches, to hook `InnerNetClient.FixedUpdate` and the mod-stamp draw.

### Client — renderer (`src/renderer`)

| Module | Lines | Responsibility |
| --- | ---: | --- |
| `Voice.tsx` | 1,733 | Socket signalling, peer mesh, the whole Web Audio graph, the proximity mix |
| `settings/Settings.tsx` | 1,197 | Settings UI |
| `Avatar.tsx` | 391 | Layered player sprite with hat/visor/skin/pet and speaking ring |
| `Overlay.tsx` | 343 | In-game overlay UI |
| `App.tsx` | 330 | Shell, routing between the three views |
| `settings/SettingsStore.tsx` | 316 | Settings schema and defaults |
| `LobbyBrowser/` | 371 | Public lobby list |
| `peer.ts` | 237 | Minimal `RTCPeerConnection` wrapper (replaced simple-peer) |
| `vad.ts` | 192 | Voice activity detection over `AnalyserNode` |
| `cosmetics.ts` | 152 | Hat collection fetched at runtime, with per-hat CSS geometry |

The renderer runs with `nodeIntegration: true` and `contextIsolation: false`. That
combination disables Chromium's renderer sandbox, so the WebRTC stack, the Opus
decoders, the Web Audio graph and the hat-image decode already run unsandboxed today,
in a process that also holds full Node access. What separates them from the memory
reader is Electron's process split, not a sandbox.

### The audio pipeline in detail

Per remote peer, `Voice.tsx` builds this graph:

```
MediaStreamSource → PannerNode → GainNode ─┬─────────────────────→ MediaStreamDestination → <audio>
                                           ├─ BiquadFilterNode ───→ (vents, cameras, radio)
                                           └─ ConvolverNode ──────→ (haunting reverb)
```

`calculateVoiceAudio()` runs on every game-state frame (5 Hz) and, per peer,
decides gain and pan from: game state, distance, `maxDistance`, wall colliders,
closed doors, vents, cameras, light radius, comms sabotage, dead/alive, impostor
role, impostor radio, and eleven host-controlled lobby settings.

Local capture uses `getUserMedia` with `echoCancellation` and `noiseSuppression`
driven by user settings, then optionally a `GainNode` for microphone gain and a
custom VAD built on `AnalyserNode` + `ScriptProcessorNode`.

Everything below the Web Audio layer — Opus encode and decode, RTP/RTCP, NACK,
FEC, DTX, the NetEQ adaptive jitter buffer, packet loss concealment, device
enumeration, resampling, acoustic echo cancellation, noise suppression and
automatic gain control — is provided by Chromium and never appears in this
repository.

### Native modules

| Module | What it actually calls |
| --- | --- |
| `memoryjs` | `CreateToolhelp32Snapshot`, `OpenProcess`, `ReadProcessMemory`, `WriteProcessMemory`, `VirtualAllocEx`, `EnumProcessModules`, `QueryFullProcessImageName`; on Linux `process_vm_readv` and `/proc/<pid>/maps` |
| `node-keyboard-watcher` | `GetAsyncKeyState` on a dedicated thread, polled every 60 ms; on X11 the same loop with `GetAsyncKeyState` aliased to `XQueryKeymap`, opening and closing the display on every check |
| `electron-overlay-window` | `SetWinEventHook` to follow the game window, `SetWindowLong` for `WS_EX_LAYERED\|WS_EX_TRANSPARENT`, `SetWindowPos` for z-order, and a `PostMessage` probe for UIPI access; on X11 plain `xcb` only, tracking `_NET_ACTIVE_WINDOW`, `_NET_WM_NAME` and `_NET_WM_STATE` |

There is no low-level keyboard hook anywhere in the tree: `SetWindowsHookEx`,
`WH_KEYBOARD_LL` and `LowLevelKeyboardProc` do not appear in `native/` or `src/`. The
watcher polls, so it never intercepts a keystroke and never sits in another process's
input path.

Click-through is Electron's, not the native module's — `overlay.setIgnoreMouseEvents(true)`
at `src/main/index.ts:224`. On Windows the module additionally sets `WS_EX_TRANSPARENT`
as part of its transparency fix; on Linux `x11.c` includes only `<xcb/xcb.h>`, links
only `-lxcb`, and contains no XFixes and no Shape code, so nothing native sets an input
region there. What the X11 half of the module actually does is EWMH tracking: follow the
active window, read its title, and watch `_NET_WM_STATE` for fullscreen.

### Data fetched at runtime

Two datasets are downloaded at start-up, pinned very differently.

| Source | Pinned to | Supplies |
| --- | --- | --- |
| `greluc/AnotherCrewlink-Offsets` | nothing — `main` branch HEAD over `raw.githubusercontent.com`, with a jsDelivr mirror of the same branch as the fallback host | `lookup.json` and the per-build offsets file |
| `OhMyGuus/BetterCrewLink-Hats` | commit `3d2cc7de`, through a jsDelivr `/gh/…@<sha>/` path | `hats.json` and every hat, visor and pet image |

The offsets moved to a fork under this project's own account on 2026-08-24. That
changes who can alter them and nothing else: the branch is still unpinned, the
contents are still used without validation, and there is still no signature. It
also creates an obligation that did not exist while the client followed upstream —
the fork has to be kept in sync, because a client pointed at a mirror nobody
updates cannot read a newly patched game at all.

The hat collection is content-addressed and immutable for the life of a release;
bumping `HAT_COLLECTION_COMMIT` in `src/common/hatCollection.ts` is a deliberate,
reviewable act. The offsets are whatever that branch holds at the moment the request
lands. There is no hash, no signature and no structural validation on them, and a
failed fetch falls back to an unauthenticated copy in `userData` that is likewise never
checked. Those numbers drive every pointer chain in `GameReader`, the buffer lengths it
allocates, and — on 32-bit Windows — the addresses the `JMP` patches are written to.

### Server

476 lines of TypeScript. An Express app serving one Pug status page, a
`/health` and a `/lobbies` JSON endpoint, and a Socket.IO namespace with eleven
events: `join`, `leave`, `id`, `setHost`, `signal`, `VAD`, `lobby`,
`remove_lobby`, `join_lobby`, `lobbybrowser`, `disconnect`. It relays opaque
signalling blobs between peers and keeps an in-memory lobby registry. No
database, no authentication, no persistence. TURN is a separate coturn service;
the server only advertises its credentials.

## 1.4 Build and release

- `electron-vite` builds three bundles (main, preload, renderer).
- `electron-builder` produces an NSIS installer for Windows x64 and ia32 and an
  AppImage for Linux x64.
- `electron-updater` checks GitHub Releases and applies deltas.
- CI: four GitHub Actions workflows, every action pinned to a commit SHA, with
  a Windows and a Linux matrix leg, CodeQL, and a PR artifact comment.
