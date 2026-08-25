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
│   ├── acl-types/             # AmongUsState, Player, settings, map data, mods
│   ├── acl-game/              # process memory, pattern scan, offsets, mods
│   ├── acl-audio/             # capture, APM, codec, jitter buffer, DSP graph, mix
│   ├── acl-net/               # socket.io client, WebRTC peers, signalling
│   ├── acl-platform/          # keyboard poll, overlay window, autostart, paths
│   ├── acl-i18n/              # the i18next locale tree, flattened once at start-up
│   ├── acl-ipc/               # helper ↔ core: postcard message types, framing
│   ├── acl-app/               # orchestration: state machine wiring the above
│   ├── acl-ui/                # egui views: main, settings, lobby browser, overlay
│   ├── acl-helper/            # elevated binary: game reader, key poll,
│   │                           #   overlay window
│   └── acl-core/              # unelevated binary: tokio, audio, net, GUI
├── server/                     # separate crate (may stay its own repository)
│   └── src/                    # socketioxide + axum
├── xtask/                      # build/release automation as Rust, not shell
├── static/                     # unchanged: images, sounds, locales
└── tests/
    ├── golden/                 # vectors captured from the Electron build
    └── interop/                # 1.x ↔ 2.x connection tests
```

`acl-i18n` was added in `P1+` and is not in the original tree. The loader has to read
files and parse JSON, and `acl-types` is the crate that must stay free of both so the
gates can test it in isolation. Putting the strings in the GUI crate would have been
worse still: `P6` is the last phase, and the loader is wanted long before it.

`acl-types`, `acl-game`, `acl-audio` and `acl-net` must all build and test
with no GUI dependency. That is what makes the go/no-go gates possible.

Two binaries, not one — see §3.2. `acl-ipc` is the only crate both of them
depend on, and it exists so that the boundary is a written type, defined in
`P1+`, rather than something `P3` and `P4` are retrofitted into later.

## 3.2 Processes, threads and data flow

Electron's process split (main = privileged, renderer = UI) is replaced by a
smaller split, not by a thread split. Two processes:

- **`acl-helper`**: process memory reading, the keyboard poll, and the overlay
  window. **Unelevated by default** — see below.
- **`acl-core`**, never elevated: tokio, signalling, WebRTC, audio, and the GUI.

> **The split is not about administrator rights, and calling the helper "the
> elevated binary" invites exactly the wrong reading.** Measured on 2026-08-24
> from an unelevated shell: opening a same-user child process and reading another
> module's memory out of it succeeds with an ordinary token, and still succeeds
> with `PROCESS_VM_WRITE | PROCESS_VM_OPERATION` requested. Windows grants both
> over a process at the same integrity level. `acl-game`'s
> `reads_another_process_without_any_elevation` is that measurement, kept as a
> test: if it ever needs elevation to pass, the premise here has changed.
>
> Elevation is needed in one configuration only — the *game* running at a higher
> integrity level, where a medium-integrity process cannot open it at all and no
> rights request helps. That is the case the README is about, and it also breaks
> the keyboard hook, because UIPI stops a low-level hook seeing input while an
> elevated window has focus.
>
> What the split actually buys is written in the paragraph below and does not
> depend on privilege at all: it is about what shares an address space with the
> handle on the game.
>
> **The injection path was removed on 2026-08-24, and that does not change any of
> this.** Writing a same-user process never needed elevation either — the same
> measurement covered it. What removing the writes changed is how much this
> project holds when it does open the game: `PROCESS_VM_READ |
> PROCESS_QUERY_LIMITED_INFORMATION` rather than `PROCESS_ALL_ACCESS`, in the 1.x
> client as well as here. The narrower set also *fails less often*:
> `PROCESS_QUERY_LIMITED_INFORMATION` exists precisely to be grantable where the
> wider query right is not, so "run it as administrator" is advice fewer people
> will need.

A thread boundary is not a privilege boundary. `catch_unwind` around a peer does
not contain a memory-safety bug in the APM's C++ or in a pre-1.0 RTP parser, and
it does nothing whatever about a process holding debug-level access to the game.
Splitting puts everything that parses bytes an arbitrary internet peer can push
at us in a process with no elevation, and leaves the elevated half with no
listening socket, no HTTP client and no image decoder. The overlay's renderer is
the one graphics dependency that stays on the elevated side, which is a reason
to prefer the cheapest rung of §3.3's chain for it.

Splitting also preserves a boundary that exists today: the Electron overlay is
its own `BrowserWindow` and Chromium's GPU work is out-of-process, so an overlay
fault or a driver crash does not currently take voice down with it.

The helper is started on demand and elevated per launch, through UAC, with no
Windows service anywhere in the design. On Windows `acl-core` spawns it the
first time a game process appears and spawns it *unelevated*; a same-user game
needs no privilege beyond that, which is the majority case and the one that works
today without anybody being asked anything. If `OpenProcess` comes back denied
because the game is running at a higher integrity level — the configuration the
README is about — the helper exits with that status and `acl-core` respawns it
through `ShellExecuteW`'s `runas` verb. That is one UAC prompt, once per session,
paid only by the users who need it. The service was the alternative, and it buys
the removal of that prompt at the price of a permanently installed `LocalSystem`
component holding debug-level access to arbitrary processes, reachable over an
IPC endpoint every account on the machine can open, present whether or not anyone
is playing. That is a larger and always-present privilege than the friction it
removes. The prompt is accepted, and it is also the one moment where the
operating system, rather than this project's documentation, tells the user that
the program is about to read another program's memory. Declining it is a
supported state and not a crash: the unelevated helper stays up and keeps polling
keys, and reports that it cannot read the game, so the client has no proximity
data and an overlay that cannot attach. §3.3 makes that a named UI state with a
way to ask again, rather than a blank screen. On Linux nothing elevates:
`process_vm_readv` against a same-uid process needs only the documented
`setcap cap_sys_ptrace+ep` on the common `ptrace_scope=1` default, so there the
split is a fault boundary and not a privilege boundary, and it is kept for the
first reason rather than the second.

The overlay is in the *elevated* half, which is counter-intuitive and is not a
free choice. UIPI blocks window manipulation and out-of-context `SetWinEventHook`
across integrity levels, so an unelevated overlay stops following an elevated
game — the configuration the README tells users to run. One consequence has to be
designed in from the first commit: the overlay receives **pre-rasterised sprites**
over the IPC and never fetches or decodes an image, so no image decoder enters
the elevated process.

```
┌─ acl-helper — elevated when the game is ─────────────────────┐
│  game thread (5 Hz, blocking)                                 │
│    acl-game: read process memory → AmongUsState              │
│  key thread (60 ms poll): GetAsyncKeyState / XQueryKeymap     │
│  overlay window (winit: transparent, click-through, topmost)  │
└──────────────┬────────────────────────────────▲───────────────┘
 AmongUsState  │  length-prefixed postcard over │  overlay geometry
 + key edges   │  a named pipe (Windows) or a   │  + pre-rasterised
 (~200 B, 5 Hz)│  Unix socket (Linux)           │  sprites
┌──────────────▼────────────────────────────────┴───────────────┐
│ acl-core — never elevated                                    │
│                                                               │
│  ┌─ tokio runtime (multi-thread) ────────────────────────┐    │
│  │  acl-net: socket.io client ─ signalling ─► WebRTC    │    │
│  │  acl-app: state machine, lobby settings, reconnect   │    │
│  └────┬────────────────────────────────────┬─────────────┘    │
│       │ mix parameters (lock-free SPSC)    │ encoded frames   │
│       ▼                                    ▼                  │
│  ┌─ audio render thread (cpal callback, real-time) ──────┐    │
│  │  per peer: jitter buffer → decode → pan → filter →    │    │
│  │            reverb → gain ──┐                          │    │
│  │                            └──► sum ──► output device │    │
│  └───────────┬───────────────────────────────────────────┘    │
│              │ far-end reference                              │
│  ┌───────────▼───────────────────────────────────────────┐    │
│  │  audio capture thread (cpal callback, real-time)      │    │
│  │  input ──► ring buffer ──┐                            │    │
│  └──────────────────────────┼────────────────────────────┘    │
│  ┌──────────────────────────▼────────────────────────────┐    │
│  │  capture worker thread (not a callback)               │    │
│  │  APM (AEC/NS/AGC) → VAD → gain → encode → RTP         │    │
│  └───────────────────────────────────────────────────────┘    │
│                                                               │
│  ┌─ UI thread (winit event loop, repaint on demand) ─────┐    │
│  │  eframe: main window, settings, lobby browser         │    │
│  └───────────────────────────────────────────────────────┘    │
└───────────────────────────────────────────────────────────────┘
```

Helper → core is a ~200-byte struct at 5 Hz plus key edges, so the boundary costs
nothing that matters. Core owns the audio ring buffers outright.

Five rules keep this honest:

1. **The audio callback never allocates, never locks, never logs.** Parameters
   reach it through a lock-free ring buffer written by the app threads.
   Violations are caught in CI by a debug allocator that panics if the render
   thread allocates.

   **The APM is why the capture side has a worker thread at all.** An earlier
   version of the diagram above ran it inside the cpal capture callback, which
   this rule forbids and which `sonora` cannot honour: measured, its capture path
   allocates about 75 times per 20 ms frame, inside its own adaptive filters and
   not in anything this crate wrote. The render path — being handed the buffer on
   its way to the speakers — allocates nothing, which is what lets it stay on the
   render callback where the far-end reference has to be taken.

   So the capture callback does one thing: copy the microphone's samples into a
   ring buffer. Everything after that runs on a thread that is allowed to
   allocate. The cost is one buffer of added latency on the send path, which is
   the cheapest thing in the budget; the alternative is an echo canceller that
   takes a lock inside the operating system's audio callback.

   `crates/acl-audio/tests/allocations.rs` records both numbers, and the test
   fails if the capture figure ever reaches zero — if a future `sonora` becomes
   allocation-free, this paragraph is wrong and should be deleted rather than
   quietly left standing.
2. **`AmongUsState` is produced in exactly one place** — the helper's game thread
   — and rebroadcast inside `acl-core` with `tokio::sync::watch`. Consumers get
   the latest, never a queue.
3. **The UI is a pure function of state.** No UI thread ever writes voice state.
4. **Real-time-safe APIs are selected by name, not assumed.** The methods the
   callback may call are the ones that write into a caller-owned buffer:
   `opus`'s `decode_float(&mut [f32])` and `encode(&[i16], &mut [u8])`, `rubato`'s
   `process_into_buffer`, `realfft`'s `process_with_scratch` against preallocated
   scratch. Their allocating siblings — `decode_vec`, `encode_vec`, `process` —
   are banned from the audio crates by a clippy `disallowed-methods` lint, so
   rule 1 is enforced at compile time and not only by the CI allocator.
5. **Nothing logs from an audio callback.** A `tracing::warn!` formats,
   allocates and takes the subscriber's lock. Callback diagnostics leave over the
   same SPSC queue as everything else and are logged on the consumer side.

## 3.3 Crate-by-crate

### `acl-types`

Direct ports of `src/common`. `AmongUsState`, `Player`, `GameState`, `MapType`,
`CameraLocation`, the collider tables in `ColliderMap.ts`, `AmongusMap.ts`,
`playerColors.ts`, `Mods.ts`, and the settings schema.

The collider maps are static geometry — port them as `const` arrays and keep
`ColliderMap.test.ts` as the parity reference; it already exists and passes.

### `acl-game`

```rust
pub trait ProcessMemory {
    fn read(&self, addr: u64, buf: &mut [u8]) -> Result<()>;
    fn write(&self, addr: u64, buf: &[u8]) -> Result<()>;
    fn alloc(&self, size: usize, prot: Protection) -> Result<u64>;
}
```

Two implementations: `WindowsProcess` (`OpenProcess` +
`ReadProcessMemory`/`WriteProcessMemory`/`VirtualAllocEx` over `windows-sys`)
and `LinuxProcess` (`process_vm_readv` through `nix`'s safe wrapper, whose
lengths derive from the slices handed in, so the Linux reader contains no
`unsafe` at all, plus `/proc/<pid>/maps`). Plus `ReplayProcess`, which serves a
recorded memory snapshot — that is what makes the game reader testable in CI
with no Among Us installed — and `FuzzProcess`, which answers from `Arbitrary`
bytes so the state parsing is fuzzable without a game. See
[05-regression-strategy.md](05-regression-strategy.md) §5.2.

`OpenProcess` requests `PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION`
and nothing more. `PROCESS_VM_WRITE | PROCESS_VM_OPERATION` and
`PROCESS_CREATE_THREAD` are never asked for: the injection path they served was
removed on 2026-08-24. `PROCESS_ALL_ACCESS` is not carried over — and is gone
from `native/memoryjs` as well, so the 1.x client already opens the game with
exactly the two rights named above. Finding the process is ~25 lines of
`CreateToolhelp32Snapshot` on Windows and a `/proc` scan on Linux, done once
with the handle kept, rather than a crate and a rescan on every poll.

Above the trait: pattern scanning, pointer-chain resolution, .NET dictionary and
array walking, offset fetching and caching, and mod detection. An injection
module was to sit alongside them, transliterating the x86 shellcode byte arrays
from `GameReader.ts`; §4.4 item 6 records why it was dropped instead, and the
`i686` target and NASM went with it.

The reader lives in `acl-helper`; none of this crate is linked into `acl-core`.

The parsing above the trait is fuzzed through `FuzzProcess`, which is worth
almost nothing to build once `ProcessMemory` is a trait and is the only way to
reach the two hazards a modded or corrupted game process presents today: a
self-referential pointer chain that loops forever, and an attacker-influenced
array length used to size a `Vec`. Both need an explicit cap. For that to find
anything the parsing layer must stay pure — `&dyn ProcessMemory` in, `Result`
out, no `unwrap`, no `as` truncation.

Whether the 32-bit requirement can be confined to a second, small helper process
is an **open decision**, not a settled one — a different question from the
elevation split in §3.2, which is settled, and one that would add a *third*
process rather than change the two. That single target is what forecloses
LiveKit's `libwebrtc` binding, what puts NASM in the build once TLS enters the
tree, and what creates the alignment hazard for exactly the struct parsing this
crate is full of: MSVC on `i686` may align >4-byte types to only 4 bytes, so
`zerocopy`'s reference APIs (`ref_from_bytes`, `try_ref_from_bytes`) must never
be used on a struct containing `u64`/`i64`/`f64` — `read_from_bytes`, which
copies, costs tens of bytes at 30 Hz.
>
> **Superseded 2026-08-24.** Confining injection to its own 32-bit process would
> have moved the two highest injection-related risk rows from High to Low.
> Removing the injection path removes those rows instead, and with them the
> `i686` target, the NASM requirement, the alignment hazard above and the
> foreclosure of `libwebrtc`. The paragraph is kept because the alignment rule it
> states is the reason a 32-bit target would still be expensive if anyone
> proposes one again.

### `acl-audio`

The crate that decides the project. Structured so each stage is independently
testable against golden vectors:

```
capture:  cpal::Stream → Resampler(rubato; non-48 kHz devices only)
                       → Apm(sonora) → Vad → Gain → OpusEncoder
                       → EncodedFrame
                            ▲
                            │ far-end reference
                            │
playback: RtpPacket → NetEq(neteq) ──pull──► AudioDecoder → opus decode
                            │       ◄─── 10 ms PCM ───────────────────┘
                            ▼
                       Panner → Biquad → Convolver(fft-convolver) → Gain
                            │
                            ▼
                       Mixer → render buffer → cpal::Stream → output device
                            │
                            └──────────────────────► far-end reference (above)
```

**NetEQ is not a stage in a pipe.** It is a pull-based jitter buffer: accelerate,
preemptive expand and expand each decide, per 10 ms of output, how much decoded
audio they need and ask for it. The arrow between buffer and decoder therefore
points the other way, and `neteq` is taken with `default-features = false` plus
an implementation of its `AudioDecoder` trait over the `opus` crate. That keeps
libopus the only codec in the binary: the crate's defaults otherwise pull a
second Opus implementation, a second `cpal`, a web framework and a CLI parser.
Whether it can signal loss to the decoder in a way that permits out-of-order
in-band FEC recovery is **unproven** — its documented surface says nothing about
it — which is why that is a G2 criterion and not a detail to discover in `P4`.

**The far-end reference is a real path, and it is missing from every naive
version of this diagram.** Echo cancellation needs the render signal aligned with
the capture signal. Here the render signal is a mixed multi-peer output produced
*after* the DSP graph, so the reference the APM receives has to be the buffer
handed to the output device — not any single peer's decoded audio, and not the
mix before panning, filtering and reverb, because what the microphone picks up
contains all of it. Same block size, known delay, written down. Getting this
wrong does not fail a test: the canceller runs, reports nothing, and silently
does nothing, which is the most common way an echo canceller is broken.

**Capture and render run on independent clocks.** Over a long session two device
clocks diverge and the jitter buffer slowly fills or starves — in a bug report
that is indistinguishable from the NetEQ problems G2 exists to catch. `rubato`
5.0.0's `Slip` resampler is the tool for it: a clutch that occasionally slips a
frame under a short crossfade to match two almost-equal rates. Resampling
otherwise exists only for devices that are not 48 kHz, since Opus, the APM and
the mixer all run at 48 kHz. Pin `=5.0.0` — four breaking majors in four months,
and 5.0.0's headline fix was an index-out-of-bounds panic in the async resamplers
that shipped through the whole 4.x line. A panic on the audio thread is a denial
of service.

**The APM.** `sonora` 0.2.0 is the default: pure Rust, BSD-3-Clause, ported from
WebRTC M145, validated against the C++ reference test suite, and it removes the
meson/ninja/clang build entirely.

> **Settled 2026-08-24.** The `i686` precondition this paragraph used to carry went
> with the target itself, which reopened the choice on wider grounds. `libwebrtc`'s
> AEC3 was measured against `sonora` on the same echo path: 11.3 dB against
> 11.6 dB, which is no difference. The decision fell to AEC3 arriving as a
> prebuilt 86 MB library nobody here compiled, in a project that spent the same
> week removing prebuilt binaries from `native/`. `sonora` stays, and it is wired
> in `acl-audio::apm` behind the trait this section always specified — with a
> test that measures the cancellation with and without the far-end reference,
> 12.3 dB against 0.1 dB, because a canceller with the reference wired wrongly
> reports success and removes nothing.
`webrtc-audio-processing` `=2.1.0` stays only as a Linux-only baseline to A/B
echo-return-loss-enhancement against: it does not build on either Windows target
(PR #102 "Support MSVC targets" open and unmerged since 2026-08-08, issue #34
"Windows build" open since 2023-09-27, CI on `ubuntu-latest` only), and Windows
is where the users are. Either way the APM sits behind the trait, so the gate can
change the answer without changing the graph.

The DSP nodes live in `acl-audio::graph` and are deliberately *not* a general
Web Audio implementation. They are the exact subset the app uses, each written
against the formula in the specification, each with a golden-vector test:

| Node | Configuration actually used |
| --- | --- |
| `Panner` | `equalpower`, `linear`, `refDistance` 0.1, `rolloffFactor` 1, `maxDistance` from lobby settings |
| `Biquad` | lowpass 2000 Hz Q 20; lowpass 2300 Hz Q −15; highpass 1000 Hz Q 10 |
| `Convolver` | one impulse response, `static/sounds/reverb.ogx`, normalised per spec, run through `fft-convolver` |
| `Gain` | scalar, `set_value_at_time` semantics |
| `Analyser` | 1024-point FFT, Blackman window, `smoothingTimeConstant` 0.2, byte output |

Four of those five are formulas. The convolver is not: uniformly partitioned FFT
convolution needs correct overlap-add accumulation, correct latency alignment,
and neither an allocation nor a denormal stall in the callback, and its failure
modes are quiet — a reverb tail slightly late, slightly smeared or slightly quiet
produces no crash, no failing test and no bug report anyone can articulate. So
that one node is a crate: `fft-convolver` 0.4.0 does the general part and the Web
Audio normalisation scalar is the single line the specification hands us. Its
real-time-safety claim is the crate's own and was **not independently checked**,
and whether it flushes denormals to zero is unverified; both belong in gate work
rather than in an assumption. The impulse response itself never changes, so
`xtask` decodes `reverb.ogx` once and `include_bytes!` embeds the PCM: no media
framework in the shipped runtime, byte-identical across platforms, and a
convolver testable with no I/O.

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

### `acl-net`

Two halves, plus the three plain HTTP GETs that have nowhere better to live.

**Signalling.** A typed Socket.IO client, written against `tokio-tungstenite`
0.30.0 from the first commit rather than adopted and then replaced.
`rust_socketio` 0.6.0 speaks the right revisions — Socket.IO 5 / Engine.IO 4,
which is what the 4.x server uses — but it pulls `backoff` and, through it,
`instant`, both under unmaintained RustSec advisories with no fixed version, plus
a second `reqwest`, a second `tungstenite` and a third TLS stack. It fails the
workspace's own dependency gate on contact, so it is not used even briefly.

The protocol surface here is eleven events over one namespace with no binary
payloads and no acknowledgements except `join_lobby`, and it is smaller still
because both existing clients already pass `transports: ['websocket']`.
Connecting directly with `transport=websocket` deletes HTTP long-polling, the
probe/upgrade handshake and base64 binary framing from the specification
entirely; what remains is five Engine.IO packet types, five Socket.IO packet
types, one grammar and one ack-id counter — roughly 440 lines. Five things are
named conformance tests, because they are how hand-written v4 clients fail: in
v4 the **server** sends `ping` and the client answers `pong`, reversed from v3;
`pingInterval`, `pingTimeout` and `maxPayload` are read from the OPEN packet
rather than hard-coded; the Socket.IO `sid` is not the Engine.IO `sid`; ack ids
are released even when the server never acks `join_lobby`; and a
`CONNECT_ERROR` must be distinguishable from a transport close, or an auth
rejection drives the reconnect policy. Two things Chromium supplied for free and
`tokio-tungstenite` does not are line items on all three targets: system proxy
resolution and the platform certificate store, via `rustls-platform-verifier`.
The wire format is unchanged, so the existing server and existing 1.x clients
keep working. This lands in `P1+`, not `P4` — in `P4` it was crowding out the
WebRTC half of a phase that has none to spare.

**HTTP.** The offset store, the hat collection and the update check are three
GETs. They go through `ureq` 3.4.0 with the `platform-verifier` feature, driven
from `tokio::task::spawn_blocking`. `ureq` is synchronous, which here is a
feature: three update GETs cannot stall the runtime the voice path shares.

**Peers.** A `Peer` type mirroring `peer.ts`: initiator/answerer, one audio track
each way, one data channel (`anothercrewlink`) for lobby settings and impostor
radio, trickle ICE with candidates queued until the remote description lands,
and the 20-second connect timeout that exists because a connection stuck in
`new` never fails on its own. All four connection bugs fixed in 1.0.0 are
carried over as named tests.

The stack is `webrtc` `=0.20.3`, pinned exactly. It is chosen for TURN: the
client forces relay-only and validates server-pushed `turn:`/`turns:` URLs, and
`str0m` states plainly that a TURN client is out of its scope — an RFC 8656
client is a multi-week job in the wrong category for hand-writing. The pin is not
decoration; 0.20.0 is a rewrite over a sans-IO core and the maintainer says a
minor bump may break. One consequence lands on this crate's design: `peer.ts`
nulls all five event handlers before `pc.close()`, and that teardown is precisely
how the 1.0.0 fixes avoid acting on events from a connection being replaced.
`webrtc` 0.20 takes a single `Arc<dyn PeerConnectionEventHandler>` with no
per-event detach, so the same guarantee becomes a generation counter or an atomic
detached flag inside the handler — and the `&self` handler forces interior
mutability through the peer layer, which collides with §3.2 rule 1 and has to be
kept off the audio path. Neither crate can demonstrate Chromium interop in CI,
and Chromium interop is the whole constraint; the `P4+` spike against a real
1.0.2 client is what answers it.

### `acl-platform`

| Concern | Windows | Linux |
| --- | --- | --- |
| Key state | `GetAsyncKeyState` poll on an owned thread, as today | `XQueryKeymap` poll, as today |
| Overlay attach | `SetWinEventHook` + `SetWindowLongPtrW` for `WS_EX_LAYERED\|WS_EX_TRANSPARENT\|WS_EX_TOPMOST` | `XFixes` input region + `_NET_WM_STATE_ABOVE` |
| Paths | `directories` | `directories` |
| Single instance | named mutex | abstract socket |
| Helper launch | spawned unelevated on demand; `ShellExecuteW` `runas` only after `OpenProcess` is denied, one UAC prompt per session (§3.2) | ordinary `Command::spawn`; no elevation, `setcap cap_sys_ptrace+ep` documented instead |

The Windows key path stays a 60 ms `GetAsyncKeyState` poll. The Electron client
no longer polls: `native/node-keyboard-watcher` carried no licence and was
replaced by `native/uiohook-napi`, which does install
`SetWindowsHookEx(WH_KEYBOARD_LL)`. The port has no such licensing pressure and
it is worse: the callback runs on the installing thread's message pump, every
keystroke on the desktop is blocked until it returns, and exceeding
`LowLevelHooksTimeout` (300 ms by default) gets it silently unhooked. That is a
desktop-wide latency dependency for no gain over a poll that already works and
intercepts nothing. On Linux the one free improvement is to stop calling
`XOpenDisplay`/`XCloseDisplay` per key check and hold one x11rb connection open
from startup.

The overlay window lives in `acl-helper` (§3.2) and receives pre-rasterised
sprites; the ported UIPI access check becomes a first-class UI state, so a user
whose game is elevated and whose helper is not gets an accurate message instead
of a blank screen. A declined UAC prompt (§3.2) is the same class of state and
gets the same treatment: the helper is running but cannot read an elevated game,
so the main view says that and offers to ask again, because the alternative is a
client that looks broken for a reason the user chose. Exclusive-fullscreen
detection comes with it: with Fullscreen Optimizations off a layered window will
not appear at all, and the alternative — hooking the swapchain — is not something
this project ships.

`winit`'s `set_cursor_hittest(false)` handles click-through on all three targets,
X11 included: winit's X11 backend sets an input shape through the X server
(`shape_rectangles(SO::SET, SK::INPUT, …)`), so it is not window-manager
dependent. What *is* window-manager dependent is the other half of an overlay —
`_NET_WM_STATE_ABOVE`, override-redirect, staying above a fullscreen game — and
that is the half `x11rb` is kept for, and the half
`native/electron-overlay-window/src/lib/x11.c` actually implements. *(The audit
reports that `x11.c` includes only `<xcb/xcb.h>` and contains no XFixes or Shape
code at all, so the Linux input region may be new code rather than a port; that
reading was not independently re-verified and should be checked before the
estimate is trusted.)*

A transparent, click-through, always-on-top window is prototyped on Windows x64,
Windows i686 and Linux in `P1+`, before the GUI phase starts. eframe's own
transparency issues are open and their known workarounds are renderer-specific,
so this is hours of work that either confirms the design or changes it while
changing it is still cheap.

### `acl-ui` and `acl-core`

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
| `overlay` | `Overlay.tsx`, in its own transparent window, drawn in `acl-helper` |

Avatars are composited in `acl-ui`: the recoloured base sprite from `image`,
then hat-back, skin, hat-front, visor and pet as textures. The hat collection is
still fetched at runtime, but the per-hat geometry — currently CSS strings like
`"32%"` — is parsed once into `f32` fractions at load.

Localisation stays exactly where it is: 37 i18next JSON directories, read at
startup by a loader of under 100 lines over the `serde_json` already in the tree,
flattening the nested keys once into a map behind `fn t(&self, key: &str) -> &str`
with an English fallback chain. Measured, the corpus has 128 keys per locale,
zero key difference against `en`, and **zero** interpolation placeholders,
plurals or selectors across all 4,736 strings — every feature that would
distinguish Fluent from a flat map is unused. Converting would also not leave
translation content untouched: Fluent identifiers cannot contain dots and the
keys are dotted throughout, and those keys are what Crowdin and every call site
key on. Two things the loader must carry that a bare `HashMap<String, String>`
loses: per-locale base text direction, and a note that `format!` covers the first
string that ever needs formatting, so nobody reopens the question by reflex.

The renderer is a chain, not a choice. Linux defaults to software, which is what
the Electron client already does unconditionally; Windows tries wgpu on DX12,
then wgpu with `force_fallback_adapter` (WARP), then a CPU rasteriser. There is
no glow rung: glow needs GL 3.3 / ES 3.0, and a Windows machine without a vendor
driver offers software GL 1.1, so the rung would fail in exactly the RDP and
bare-VM cases it would exist for. The existing `hardware_acceleration` setting
migrates forward rather than being replaced by a new key, automatic demotion is
non-persistent by default, and `--renderer=auto|gpu|software` is documented next
to the elevation note. Chromium gives every user SwiftShader for free today; a
wgpu-only client would give the same users a window that never opens.

Idle repaint is a policy, not a default. eframe is reactive and parks on
`ControlFlow::Wait`, so a still window costs nearly nothing — but a UI driven
from game state calls `request_repaint` somewhere, and an unconditional call in
`update()` turns that into continuous repaint at display refresh forever. The
main view uses `ctx.request_repaint_after(Duration::from_millis(200))` driven off
the game-state watch channel, and a much longer interval when minimised or
occluded.

## 3.4 Server

Four weeks and the natural first phase. The translation itself is a fortnight;
the owned registry, the lobby endpoints and the envelope rules are what fill out
the rest.

```
server/
├── Cargo.toml
└── src/
    ├── main.rs         # axum + socketioxide, graceful shutdown
    ├── lobby.rs        # the in-memory registry, ported from index.ts
    ├── peer_config.rs  # peerConfig.toml, ICE server list, relay credentials
    └── web.rs          # /, /health, /lobbies
```

`socketioxide` 0.18.6 (updated 2026-08-07, actively maintained) mounts as a
`tower` service inside `axum` 0.8. `tower` alone is not the middleware story:
it ships protocol-agnostic layers — limit, timeout, retry, buffer, load-shed —
and no HTTP-aware middleware at all, so `tower-http` 0.7.0 supplies the body cap,
the request-body timeout that hyper's header timeout does not cover, panic
catching, and CORS.

**CORS is not on the socket.io route, and that is a consequence of dropping
polling rather than an omission.** The OBS overlay page at `obs.aucl.greluc.me`
is a browser client on a different origin from whatever `serverURL` the user has
set, and while polling was on the table its handshake was an XHR that needed a
`CorsLayer` — `socketioxide` ships no CORS handling of its own. With polling
gone, the page connects by WebSocket upgrade, which is not a CORS request and
needs no server-side permission; what it needs instead is
`transports: ['websocket']` in the page itself, deployed and verified before the
server release (§3.5, [06-security.md](06-security.md) §6.3). Do not substitute
an `Origin` allow-list: `Origin` is a header any non-browser client sets freely,
so it rejects nothing that matters while being the one thing that can take the
overlay off the air. `CorsLayer` stays on the plain HTTP routes a browser really
does fetch with XHR — `/health` and `/lobbies`.

**The server is websocket-only.** The Engine.IO polling transport is not
enabled. Both existing clients already pass `transports: ['websocket']`, so
polling is served to nobody legitimate, and it carried
[GHSA-r635-g3xr-vw7x](https://github.com/advisories/GHSA-r635-g3xr-vw7x) (HIGH),
which leaves with it. It is also half the Engine.IO specification, the half the
hand-written client in §3.3 does not implement. What that costs is a client
population, and §3.5 names it.

TLS terminates at a reverse proxy and axum binds to loopback. That keeps
`aws-lc-rs`, ACME and certificate rotation out of the server binary, and nginx's
`limit_req`/`limit_conn` are built in. It does **not** close the frame-size hole,
and dropping polling makes that hole the only case rather than one of two:
engineioxide applies `max_payload` on the polling transport alone, so with
polling off it now governs nothing inbound at all, and the WebSocket path takes
tungstenite's defaults — 64 MiB per message, 16 MiB per frame — with no
configuration knob and no proxy directive that survives the Upgrade. The inbound
cap is therefore entirely the handlers' own, alongside the per-socket token
bucket below, and there is no layer left that could be mistaken for covering it.
The residue is an **accepted risk with an upstream issue filed**, not a config
line, and the honest form of it belongs in the plan rather than a claim that a
proxy handles it.

The same Upgrade governs rate limiting. `hyper::upgrade::on` takes over the
connection, so every Socket.IO event for the rest of the session passes through
zero tower layers: a `GovernorLayer` protects the handshake and nothing else.
Per-event limiting is a per-socket token bucket held in the socket's
`Extensions`, inside the handlers, and it has to be written there from the start
because there is no layer to retrofit it into later.

The Pug status page becomes a `format!` and a written-out escape of the five
characters that matter: `/health` and `/lobbies` go through `serde_json` and need
no templating at all, and a proc-macro template engine for one page does not
survive the "why not the standard library" question. `peerConfig` moves from YAML
to TOML — it is a handful of url/username/credential fields, and `serde_yaml_ng`
reaches `unsafe-libyaml`, an archived c2rust transliteration of C that will not
be fixed again. Configuration comes from the environment, through systemd's
`EnvironmentFile=` or docker's `--env-file`, not from a `.env` reader.

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

> **Superseded 2026-08-25.** The Electron client no longer has either feature. The
> `mobileHost` setting, the `<code>_mobile` broadcast, `obsOverlay`, `obsSecret` and the
> `ObsVoiceState` payload were removed from it, along with the settings that turned them
> on. Nothing this project ships emits either feed any more, and there is no client left
> to break.
>
> What that changes here: the OBS overlay page is no longer a scheduling constraint on
> any server release — there is no sender for it to stay compatible with — and the
> mobile relay is not something the envelope rules break, because it is already gone.
> The paragraphs around this note are kept as the record of what was decided and why.

- **`electron-devtools-installer`, the Pug view engine, `morgan`** — replaced by
  `tracing` and a formatted string, no user-visible surface.
- **The OBS browser overlay** (`obs.aucl.greluc.me`) stays a web page and
  consumes the same `ObsVoiceState` payload. It is not, however, *unaffected*,
  and in two ways. It is a browser client on another origin, and it has to be
  switched to `transports: ['websocket']`, because the polling handshake it
  gets for free from the Node stack today is not offered by either server after
  H3 (§3.4). And
  it is one deployment serving every client version at once, in neither
  repository, which makes it a scheduling constraint rather than a bystander:
  the page has to learn the post-envelope event and be deployed and verified
  *before* the server release that enforces the envelope rules
  ([06-security.md](06-security.md) §6.3). Not ported is not the same as not on
  the critical path.
- **Mobile clients, and the promise that a future one would work.** The server
  is websocket-only (§3.4). Mobile `socket.io-client` defaults to
  `["polling","websocket"]` and opens with a polling handshake, so a mobile
  client written against the 4.x protocol is refused at the handshake rather
  than degraded — and refused before any application event, so there is no
  message the server could send it to explain why. The undertaking that used to
  stand here, that the `mobileHost` / `<code>_mobile` paths would be kept working
  for such a client, is withdrawn rather than qualified: it was a promise made to
  a client that does not exist, and keeping it meant carrying the polling
  transport and its advisory for that client's benefit alone. What replaces it is
  nothing. Anyone who wants a mobile client afterwards is making
  a protocol decision, not collecting on a promise already kept: either a
  websocket-first client written against this server's events — `transports:
  ['websocket']` is one line in `socket.io-client` and the wire format is
  otherwise unchanged — or polling deliberately re-enabled on the server and
  re-argued against the advisory it brings back. Both are decisions with a named
  cost, which is the state this paragraph should have been in from the start.
