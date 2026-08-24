# 4. Implementation plan

## 4.1 Shape of the plan

Eight phases. Each ends in something shippable or a decision. Three of them are
**gates**: work stops there until an explicit, measurable criterion is met.

The order is chosen so that the riskiest work happens as early as possible and
the most visible work happens last. That is deliberate and it is the opposite of
what feels natural — the temptation is to start with the GUI, because it is the
part you can show people. Starting with the GUI is how this project fails: you
arrive at the audio engine nine months in, discover the jitter buffer is not good
enough, and have a beautiful shell around nothing.

```
P0  Server                        ──► ships independently          2 wk
P1  Foundations & toolchain                                        1 wk
P2  Game reader                   ──► G1: parity with Electron     4 wk
P3  Audio engine (offline)        ──► G2: golden-vector parity     8 wk
P4  Transport & signalling        ──► G3: interop with 1.x         5 wk
P5  Platform layer                                                 3 wk
P6  GUI                                                           10 wk
P7  Packaging, update, rollout    ──► ships as 2.0                 4 wk
                                                            total ≈ 37 wk
```

Roughly nine months of full-time work for one developer; call it a year with
review, testing on real hardware and the inevitable. Phases P2–P5 can overlap
between two developers; P3 is on the critical path throughout.

## 4.2 Phase 0 — Server (2 weeks)

Ships on its own. Proves the toolchain, CI and release story on the smallest
piece of the system.

1. `server-rs` crate: `axum` 0.8 + `socketioxide` 0.18 + `tokio` 1.53.
2. Port the eleven socket events and the lobby registry from `src/index.ts`,
   keeping the two bug fixes named in §3.4.
3. Port `/`, `/health`, `/lobbies`; `askama` replaces Pug.
4. Port `peerConfig.yml` parsing and relay advertisement.
5. Multi-stage `Dockerfile` on `rust:1.98-alpine` → `alpine:3.22`, non-root, no
   shell in the final image.
6. Port `test/lobby.test.ts` to a Rust integration test that drives a real
   `socket.io-client` from Node against the Rust server, so the wire format is
   verified against the reference implementation and not against itself.

**Done when:** the existing 1.0.2 Electron client connects to the Rust server,
joins a lobby, exchanges signalling, and the lobby browser populates — with no
client change whatsoever.

## 4.3 Phase 1 — Foundations (1 week)

1. Workspace, `rust-toolchain.toml` pinned to **1.98.0**, `edition = "2024"`.
2. Workspace lints: `unsafe_op_in_unsafe_fn = "deny"`, `clippy::pedantic` at
   warn, `missing_docs` on public crates.
3. `cargo-deny` (`advisories`, `bans`, `licenses`, `sources`) and `cargo-vet`
   wired into CI as blocking.
4. `aucl-types`: port `src/common` wholesale, including the collider tables.
   Port `ColliderMap.test.ts` — it already exists and gives a free parity check
   on day one.
5. CI skeleton: `fmt`, `clippy -D warnings`, `test`, `deny`, on Windows x64,
   Windows i686 and Linux x64.

## 4.4 Phase 2 — Game reader (4 weeks) → **Gate G1**

1. `ProcessMemory` trait and the Windows implementation.
2. Pattern scanner, pointer-chain resolver, .NET dictionary/array walkers.
3. `offsetStore` port: fetch, cache, the two-host retry with backoff and the
   request timeout added in 1.0.1.
4. Mod detection, VDF parsing, avatar recolouring.
5. The Linux implementation.
6. Injection module, 32-bit Windows, feature-gated.

**Recording harness.** Before writing the reader, add a debug command to the
*existing Electron build* that dumps, once per frame, the raw bytes of every
region `GameReader` touches plus the `AmongUsState` it produced. Record a session
per map (Skeld, Mira, Polus, Airship, Fungle) covering lobby, tasks, meeting,
vents, cameras, sabotage, and deaths. Those recordings become `ReplayProcess`
fixtures.

> **Gate G1 — parity of the reader.**
> For every recorded frame, the Rust reader's `AmongUsState` must equal the
> Electron reader's, field for field, with float positions within 1e-6.
> Non-negotiable: this is a lossless, purely mechanical transformation, so
> anything less than exact means a bug, not a tolerance.

## 4.5 Phase 3 — Audio engine (8 weeks) → **Gate G2**

The phase that decides the project. No UI, no network — a library plus a
command-line harness that reads WAV in and writes WAV out.

### 3a. DSP graph (3 wk)

Implement `Panner`, `Biquad`, `Convolver`, `Gain`, `Analyser` against the Web
Audio specification formulas. One golden-vector test per node.

**Generating the golden vectors.** A page loaded in the *current* Electron build
runs each node with `OfflineAudioContext` over a fixed set of inputs — impulse,
white noise with a fixed seed, a sine sweep, and 5 seconds of real speech — at
every configuration the app actually uses, and writes the output as 32-bit float
WAV plus a SHA-256. These files are committed under `tests/golden/`. They are the
contract: Chromium's own output is the reference, so "parity" is not a matter of
opinion.

### 3b. `voice_params` (1 wk)

Port `calculateVoiceAudio()` as a pure function. Table-driven tests covering
every branch: each game state, each of the eleven lobby settings, walls, doors,
vents, cameras, light radius, comms sabotage, dead/alive, impostor, radio, and
the interactions between them. Roughly 150 cases; they are cheap because the
function is pure.

Cross-check against the Electron build by instrumenting it to log
`(state, settings, me, other) → (gain, panPos)` for a recorded session, then
replaying those tuples through the Rust function. Every tuple must match.

### 3c. Capture and codec (2 wk)

`cpal` device enumeration and streams, `rubato` resampling, APM wiring
(`webrtc-audio-processing`, `bundled`, pinned exactly), the VAD port, `opus`
encode with FEC and DTX.

### 3d. Jitter buffer and playback (2 wk)

`neteq` integration, Opus decode, PLC, the mixer, output device selection
(replacing `setSinkId`).

**Network emulation harness.** A test that feeds the receive path a recorded RTP
stream through a configurable impairment model — loss 0/1/2/5/10 %, jitter
0/20/50/100 ms, reorder 0/1/5 %, and one 500 ms freeze — and measures output
continuity, added latency and PESQ/POLQA-style score against the clean source.
Run the same impairments through the Electron client for reference numbers.

> **Gate G2 — audio parity.**
> 1. Every DSP node matches its golden vector to within −80 dBFS RMS error.
> 2. `voice_params` matches the Electron implementation on every recorded tuple.
> 3. Under each impairment profile, the Rust receive path's added mouth-to-ear
>    latency is within 30 ms of Chromium's and its objective quality score is no
>    more than 0.2 MOS below it.
> 4. The render callback performs zero allocations under the CI allocator.
>
> **If (3) fails and cannot be fixed within two weeks, stop the port.** That is
> the honest exit: without a jitter buffer at least as good as NetEQ, a proximity
> voice chat is worse than the thing it replaces, and no amount of GUI work
> changes that. Phases 0–2 remain valuable on their own (a Rust server, and a
> Rust game reader that can be exposed to the Electron client through a small
> N-API shim if desired).

## 4.6 Phase 4 — Transport and signalling (5 weeks) → **Gate G3**

1. Socket.IO client, typed events, reconnect policy ported from
   `reconnectPolicy.ts` (its tests come across unchanged).
2. `Peer` over the `webrtc` crate: trickle ICE with candidate queueing, data
   channel, connect timeout, TURN with `relay`-only support.
3. The peer mesh: join, leave, offer glare, orphan cleanup, rebuild-on-failure.
4. `validateClientPeerConfig` port — its tests come across unchanged.

**The four 1.0.0 connection bugs become named regression tests**, because a port
will otherwise reintroduce them:

| Test | The bug it guards |
| --- | --- |
| `signal_from_unknown_socket_is_ignored_not_crashed` | the server sends `{data, from}`; `client` was destructured and always undefined |
| `trickle_candidate_without_type_is_forwarded` | only signals with a `type` were forwarded, so trickled ICE was dropped |
| `offer_glare_does_not_destroy_replacement` | the old connection's `close` tore down the new one for the same peer |
| `connection_stuck_in_new_times_out` | ICE never starts, so the connection never fails on its own |

> **Gate G3 — interop.**
> A 1.0.2 Electron client and a Rust client in the same lobby, against the same
> server, must hear each other in both directions: direct, and with
> `forceRelayOnly` through coturn. Tested on Windows and Linux, and across a NAT.
> This is what makes a staged rollout possible; without it, 2.0 must ship to
> everyone at once, which is not acceptable for a voice app.

## 4.7 Phase 5 — Platform layer (3 weeks)

Keyboard hook, overlay window, single-instance lock, autostart, paths, logging.
Port `native/electron-overlay-window/src/lib/windows.c` and `x11.c` logic
directly rather than re-deriving it — that code already knows about the window
managers and edge cases this needs.

## 4.8 Phase 6 — GUI (10 weeks)

In this order, so that the app is usable as early as possible:

1. Shell, custom title bar, window state persistence (2 wk)
2. Main view: player list, avatars, talking indicators, mute/deafen (3 wk)
3. Settings (3 wk) — the largest single screen
4. Lobby browser (1 wk)
5. Overlay view (1 wk)

Localisation runs alongside: the `xtask` that converts 37 locale directories from
i18next JSON to Fluent `.ftl` is written once, and translation content is never
retyped.

**Deliberately accepted:** the Rust UI will not be pixel-identical to the React
one. Layout, spacing and control affordances will differ. What must not differ is
what every control *does* — the settings schema is ported unchanged, including
defaults, so that an existing `config.json` keeps working.

## 4.9 Phase 7 — Packaging, update and rollout (4 weeks)

1. `cargo-dist` for Windows x64, Windows i686 and Linux x64; NSIS installer and
   AppImage to match today's artefacts.
2. Code signing on Windows; reproducible builds where the toolchain allows.
3. Auto-update: `self_update` against GitHub Releases, with **signature
   verification** — a hard requirement, and an improvement on today, where
   `electron-updater` verifies only the publisher certificate on Windows and
   nothing on Linux.
4. Settings migration: read the existing `electron-store` `config.json` on first
   run and write it forward. Test with real files from 1.x installs.
5. CI: the four existing workflows ported, actions still pinned to commit SHAs,
   `cargo-audit`/`cargo-deny` replacing `npm audit`, CodeQL still covering the
   repository.

**Rollout.** Because G3 guarantees interop, 2.0 can go out as an opt-in beta
alongside 1.x, then as the default once the beta is quiet for a full release
cycle. The Electron client stays buildable and receives security updates until
2.0 has been the default for one cycle.

## 4.10 Milestones and decision points

| | Milestone | Externally visible? |
| --- | --- | --- |
| M1 | Rust server serves 1.x clients | Yes — ships |
| M2 | **G1** reader parity on recorded sessions | No |
| M3 | **G2** audio parity and impairment results | No — **go/no-go** |
| M4 | **G3** Rust ↔ Electron in one lobby | No |
| M5 | Rust client usable end-to-end, no GUI polish | Internal alpha |
| M6 | Feature parity | Public beta |
| M7 | 2.0 default | Yes — ships |

Only M3 can end the project. M1 is valuable whatever happens after it. M2's
output (a Rust game reader) is reusable from the Electron client if the port
stops.

## 4.11 Effort summary

| Phase | Weeks | Parallelisable |
| --- | ---: | --- |
| P0 Server | 2 | independent |
| P1 Foundations | 1 | no |
| P2 Game reader | 4 | with P3 |
| P3 Audio engine | 8 | critical path |
| P4 Transport | 5 | with P5 |
| P5 Platform | 3 | with P4 |
| P6 GUI | 10 | after P5 |
| P7 Packaging | 4 | partly with P6 |
| **Total, one developer** | **37** | |
| **Two developers** | **~26** | P3 remains the critical path |
