# 3. Target architecture

## 3.1 Workspace layout

One Cargo workspace, one repository. Crates are split so that the risky parts are
testable without a GUI and without a game running.

```
AnotherCrewLink/
├── Cargo.toml                  # workspace, shared lints, shared dep versions
├── rust-toolchain.toml         # pinned stable toolchain
├── deny.toml                   # cargo-deny: licences, advisories, bans
├── crates/
│   ├── aucl-types/             # AmongUsState, Player, settings, map data, mods
│   ├── aucl-game/              # process memory, pattern scan, offsets, injection
│   ├── aucl-audio/             # capture, APM, codec, jitter buffer, DSP graph, mix
│   ├── aucl-net/               # socket.io client, WebRTC peers, signalling
│   ├── aucl-platform/          # keyboard hook, overlay window, autostart, paths
│   ├── aucl-app/               # orchestration: state machine wiring the above
│   ├── aucl-ui/                # egui views: main, settings, lobby browser, overlay
│   └── aucl-client/            # the binary: winit + eframe + tokio, packaging
├── server/                     # separate crate (may stay its own repository)
│   └── src/                    # socketioxide + axum
├── xtask/                      # build/release automation as Rust, not shell
├── static/                     # unchanged: images, sounds, locales
└── tests/
    ├── golden/                 # vectors captured from the Electron build
    └── interop/                # 1.x ↔ 2.x connection tests
```

`aucl-types`, `aucl-game`, `aucl-audio` and `aucl-net` must all build and test
with no GUI dependency. That is what makes the go/no-go gates possible.

## 3.2 Threading and data flow

Electron's process split (main = privileged, renderer = UI) is replaced by a
thread split. Nothing shares mutable state across a lock in the audio path.

```
┌─ game thread (5 Hz, blocking) ────────────────────────────────┐
│  aucl-game: read process memory → AmongUsState                │
└──────────────────────────┬────────────────────────────────────┘
                           │ watch::Sender<AmongUsState>
                           ▼
┌─ tokio runtime (multi-thread) ────────────────────────────────┐
│  aucl-net:  socket.io client ── signalling ──► WebRTC peers    │
│  aucl-app:  state machine, lobby settings, reconnect policy    │
└───────┬──────────────────────────────────┬────────────────────┘
        │ mix parameters (lock-free SPSC)  │ decoded frames
        ▼                                  ▼
┌─ audio render thread (cpal callback, real-time) ──────────────┐
│  per peer: jitter buffer → opus decode → pan → filter →       │
│            reverb → gain ──┐                                  │
│                            └──► sum ──► output device         │
└───────────────────────────────────────────────────────────────┘
┌─ audio capture thread (cpal callback, real-time) ─────────────┐
│  input device → APM (AEC/NS/AGC) → VAD → gain → opus encode ──┼─► RTP
└───────────────────────────────────────────────────────────────┘
┌─ UI thread (winit event loop, 60 Hz when visible) ────────────┐
│  eframe: main window, settings, lobby browser                 │
└───────────────────────────────────────────────────────────────┘
┌─ overlay thread (winit, separate window) ─────────────────────┐
│  transparent, click-through, follows the game window          │
└───────────────────────────────────────────────────────────────┘
```

Three rules keep this honest:

1. **The audio callback never allocates, never locks, never logs.** Parameters
   reach it through a lock-free ring buffer written by the game/app threads.
   Violations are caught in CI by a debug allocator that panics if the render
   thread allocates.
2. **`AmongUsState` is produced in exactly one place** and broadcast with
   `tokio::sync::watch`. Consumers get the latest, never a queue.
3. **The UI is a pure function of state.** No UI thread ever writes voice state.

## 3.3 Crate-by-crate

### `aucl-types`

Direct ports of `src/common`. `AmongUsState`, `Player`, `GameState`, `MapType`,
`CameraLocation`, the collider tables in `ColliderMap.ts`, `AmongusMap.ts`,
`playerColors.ts`, `Mods.ts`, and the settings schema.

The collider maps are static geometry — port them as `const` arrays and keep
`ColliderMap.test.ts` as the parity reference; it already exists and passes.

### `aucl-game`

```rust
pub trait ProcessMemory {
    fn read(&self, addr: u64, buf: &mut [u8]) -> Result<()>;
    fn write(&self, addr: u64, buf: &[u8]) -> Result<()>;
    fn alloc(&self, size: usize, prot: Protection) -> Result<u64>;
}
```

Two implementations: `WindowsProcess` (`OpenProcess` +
`ReadProcessMemory`/`WriteProcessMemory`/`VirtualAllocEx` via the `windows`
crate) and `LinuxProcess` (`process_vm_readv`, `/proc/<pid>/maps`). Plus
`ReplayProcess`, which serves a recorded memory snapshot — that is what makes
the game reader testable in CI with no Among Us installed. See
[05-regression-strategy.md](05-regression-strategy.md) §5.3.

Above the trait: pattern scanning, pointer-chain resolution, .NET dictionary and
array walking, offset fetching and caching, mod detection, and the injection
module. The x86 shellcode byte arrays transfer verbatim — they are already
literal `u8` values in `GameReader.ts`, and Rust's `const` arrays express them
more clearly than the current JavaScript does.

Injection is feature-gated (`--features injection`, on by default on Windows)
and stays 32-bit-only, matching today's behaviour.

### `aucl-audio`

The crate that decides the project. Structured so each stage is independently
testable against golden vectors:

```
capture:  cpal::Stream → Resampler(rubato) → Apm(webrtc-audio-processing)
                       → Vad → Gain → OpusEncoder → EncodedFrame

playback: RtpPacket → NetEq(neteq) → OpusDecoder → Panner → Biquad
                    → Convolver → Gain → Mixer → cpal::Stream
```

The DSP nodes live in `aucl-audio::graph` and are deliberately *not* a general
Web Audio implementation. They are the exact subset the app uses, each written
against the formula in the specification, each with a golden-vector test:

| Node | Configuration actually used |
| --- | --- |
| `Panner` | `equalpower`, `linear`, `refDistance` 0.1, `rolloffFactor` 1, `maxDistance` from lobby settings |
| `Biquad` | lowpass 2000 Hz Q 20; lowpass 2300 Hz Q −15; highpass 1000 Hz Q 10 |
| `Convolver` | one impulse response, `static/sounds/reverb.ogx`, normalised per spec |
| `Gain` | scalar, `set_value_at_time` semantics |
| `Analyser` | 1024-point FFT, Blackman window, `smoothingTimeConstant` 0.2, byte output |

`calculateVoiceAudio()` ports to a pure function:

```rust
pub fn voice_params(
    state: &AmongUsState,
    settings: &Settings,
    lobby: &LobbySettings,
    me: &Player,
    other: &Player,
) -> VoiceParams;   // { gain, pan: [f32; 3], filter: Option<Filter>, reverb: bool }
```

Pure, total, no I/O — so the entire proximity ruleset (eleven lobby settings,
walls, doors, vents, cameras, lights, sabotage, roles, radio) becomes a table
test. This is the single highest-value structural change in the port: today that
logic is 180 lines inside a React component and cannot be tested at all.

### `aucl-net`

Two halves.

**Signalling.** A typed Socket.IO client. `rust_socketio` 0.6.0 speaks Socket.IO
protocol rev 5 / Engine.IO rev 4, which is what the 4.x server uses — but it was
last released in April 2024. The client's protocol surface here is eleven events
over one namespace with no binary payloads and no acknowledgements except
`join_lobby`. **Plan for `rust_socketio`, budget for replacing it** with a direct
Engine.IO v4 implementation over `tokio-tungstenite` — roughly 400 lines, and it
removes the project's one stale dependency. Either way the wire format is
unchanged, so the existing server and existing 1.x clients keep working.

**Peers.** A `Peer` type mirroring `peer.ts`: initiator/answerer, one audio track
each way, one data channel (`anothercrewlink`) for lobby settings and impostor
radio, trickle ICE with candidates queued until the remote description lands,
and the 20-second connect timeout that exists because a connection stuck in
`new` never fails on its own. All four connection bugs fixed in 1.0.0 are
carried over as named tests.

### `aucl-platform`

| Concern | Windows | Linux |
| --- | --- | --- |
| Key hook | `SetWindowsHookEx(WH_KEYBOARD_LL)` on an owned thread | `XQueryKeymap` poll, as today |
| Overlay attach | `SetWinEventHook` + `SetWindowLongPtrW` for `WS_EX_LAYERED\|WS_EX_TRANSPARENT\|WS_EX_TOPMOST` | `XFixes` input region + `_NET_WM_STATE_ABOVE` |
| Paths | `directories` | `directories` |
| Single instance | named mutex | abstract socket |

`winit`'s `set_cursor_hittest(false)` handles click-through on Windows and
Wayland. On X11 it is window-manager dependent and known to be unreliable, so the
Linux overlay keeps the explicit `XFixes` path ported from
`native/electron-overlay-window/src/lib/x11.c`.

### `aucl-ui` and `aucl-client`

`egui` 0.36.1 with `eframe`, on `winit` 0.30 and `wgpu` 30.

Chosen over the alternatives for concrete reasons:

- **`egui`** — MIT/Apache-2.0 (compatible with GPL-3.0-or-later without
  complication), immediate mode suits a UI that redraws from game state anyway,
  runs on `winit` so the overlay and main window share one windowing stack, and
  `egui_extras` covers the tables the lobby browser needs.
- **`iced`** 0.14 — also good, Elm-style, arguably nicer for the settings form;
  rejected because the overlay needs raw window handle access that egui's
  `eframe` exposes more directly.
- **`slint`** 1.17 — excellent tooling, but its GPL-3.0 option would bind the
  licence choice permanently and its commercial option is irrelevant here.
- **`dioxus`** 0.7 — would keep the React-like model, but its desktop backend is
  a webview, which defeats the purpose of leaving Electron.

Four views, matching today's three windows plus the overlay:

| View | Replaces |
| --- | --- |
| `main` | `App.tsx` + `Voice.tsx`'s UI half + `Avatar.tsx` |
| `settings` | `Settings.tsx` (1,197 lines) |
| `lobbies` | `LobbyBrowser/` |
| `overlay` | `Overlay.tsx`, in its own transparent window |

Avatars are composited in `aucl-ui`: the recoloured base sprite from `image`,
then hat-back, skin, hat-front, visor and pet as textures. The hat collection is
still fetched at runtime, but the per-hat geometry — currently CSS strings like
`"32%"` — is parsed once into `f32` fractions at load.

Localisation moves from `i18next` to `fluent` 0.17. The 37 existing locale
directories are converted to `.ftl` by a one-off `xtask`; the translation
*content* is untouched.

## 3.4 Server

Small enough to port in a fortnight and the natural first phase.

```
server/
├── Cargo.toml
└── src/
    ├── main.rs         # axum + socketioxide, TLS, graceful shutdown
    ├── lobby.rs        # the in-memory registry, ported from index.ts
    ├── peer_config.rs  # peerConfig.yml, ICE server list, relay credentials
    └── web.rs          # /, /health, /lobbies
```

`socketioxide` 0.18.6 (updated 2026-08-07, actively maintained) mounts as a
`tower` service inside `axum` 0.8. The Pug status page becomes an `askama`
template. `serde_yaml_ng` reads `peerConfig.yml` unchanged.

Two behaviours from the current server must survive verbatim, because they were
bug fixes:

- `leaveroom` announces `left` so peers can distinguish a departure from a
  broken connection, and clearing `code` on `leave` so `disconnect` does not run
  the cleanup a second time;
- the input coercion (`asText`, `asCount`) that stops an unauthenticated client
  from taking the process down with a malformed `lobby` payload. In Rust this
  becomes `serde` with `#[serde(default)]` and explicit bounds, which is
  stronger than the current hand-written coercion.

The server keeps its own repository and its own release cadence. It is
deliberately the first phase because it is low-risk, independently shippable, and
proves the toolchain, CI and release story before any of it matters.

## 3.5 What is deliberately *not* ported

- **`electron-devtools-installer`, the Pug view engine, `morgan`** — replaced by
  `tracing` and `askama`, no user-visible surface.
- **The OBS browser overlay** (`obs.aucl.greluc.me`) stays a web page; it
  consumes the same `ObsVoiceState` payload and is unaffected.
- **Mobile clients.** The project already broke compatibility with
  socket.io 2 clients when it moved to socket.io 4; the `mobileHost` /
  `<code>_mobile` code paths are kept as-is in the port so that any future mobile
  client speaking the 4.x protocol still works, but they are not a constraint on
  the design.
