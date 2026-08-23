# 2. Feasibility

## 2.1 Verdict

**A full port to Rust with a native Rust GUI is technically possible. There is no
component without a working Rust answer, and no hard blocker.**

That is not the same as it being a good idea to start tomorrow as one rewrite.
The difficulty is distributed very unevenly, and almost all of it sits in one
place:

> Chromium currently supplies, for free, the entire real-time voice stack below
> the Web Audio graph: acoustic echo cancellation, noise suppression, automatic
> gain control, Opus with in-band FEC and DTX, RTP/RTCP with NACK, the NetEQ
> adaptive jitter buffer with time-stretching, packet loss concealment, device
> handling and resampling. **None of that exists in this repository. All of it
> would have to be assembled in Rust from crates that are considerably younger
> and less exercised than the code they replace.**

Everything else — the memory reader, the shellcode injection, the keyboard hook,
the overlay window, the server, the GUI, the build and release pipeline — is
ordinary work with mature Rust answers. Those parts are lower risk in Rust than
they are today.

So the recommendation is: **yes, port — but in the order given in
[04-implementation-plan.md](04-implementation-plan.md), with a hard go/no-go gate
after the audio engine is proven and before any GUI work starts.** The audio
engine is the part that decides the project. Building it first, standalone and
measurable, means a no-go costs one phase rather than a year.

## 2.2 Component-by-component

Risk is the chance of a regression users would notice, or of the component not
reaching parity at all. Effort is for one experienced developer.

| # | Component | Rust answer | Risk | Effort |
| --- | --- | --- | --- | --- |
| 1 | Signalling server | `socketioxide` + `axum` | **Low** | 1–2 wk |
| 2 | Process memory reading | `windows` crate / `process_vm_readv` | **Low** | 2–3 wk |
| 3 | Pattern scanning, pointer walking, struct parsing | hand-written, `zerocopy` | **Low** | 2 wk |
| 4 | Shellcode injection | `windows` crate, bytes carry over verbatim | **Low** | 1 wk |
| 5 | Global keyboard hook | `windows` crate / X11 | **Low** | 1 wk |
| 6 | Avatar recolouring | `image` | **Low** | 3 d |
| 7 | Settings, logging, offsets, VDF | `serde`, `tracing`, `reqwest` | **Low** | 1 wk |
| 8 | Overlay window | `winit` + raw window handle | **Medium** | 2–3 wk |
| 9 | GUI | `egui` / `eframe` | **Medium** | 8–12 wk |
| 10 | Localisation (37 locales) | `fluent` | **Low** | 1 wk |
| 11 | WebRTC transport (ICE/DTLS/SRTP/TURN) | `webrtc` crate | **Medium** | 3–4 wk |
| 12 | Audio capture and playback | `cpal` | **Low–Medium** | 2 wk |
| 13 | **AEC / NS / AGC** | `webrtc-audio-processing` or `sonora` | **High** | 3–5 wk |
| 14 | **Opus + jitter buffer + PLC** | `opus` + `neteq` | **High** | 4–6 wk |
| 15 | **Web Audio graph parity** | hand-written DSP | **High** | 5–8 wk |
| 16 | Packaging, signing, auto-update | `cargo-dist` / `self_update` | **Medium** | 2–3 wk |

Rows 13–15 are the project. Rows 1–7 are close to mechanical.

The effort column is per component and the entries overlap — rows 12–15, for
instance, are all scheduled inside one eight-week phase. Do not add the column
up; the schedule is in [04-implementation-plan.md](04-implementation-plan.md),
and it comes to 37 developer-weeks.

## 2.3 The four things that decide it

### (a) Acoustic echo cancellation and noise suppression — solvable, but pick carefully

Two settings in the UI (`echoCancellation`, `noiseSuppression`) currently map
straight onto Chromium's `getUserMedia` constraints. Users play with speakers
more often than headsets; without AEC the port would ship an obvious, loud
regression on day one.

Two credible options:

| | `webrtc-audio-processing` 2.1.0 | `sonora` 0.2.0 |
| --- | --- | --- |
| What it is | Rust bindings over Google's C++ APM (AEC3, NS, AGC, HPF) | Pure-Rust port of the same |
| Maturity | 104k downloads, updated 2026-05-13 | 15k downloads, updated 2026-07-29 |
| Algorithm quality | The reference implementation | Port of the reference implementation |
| Build cost | Needs a C++ toolchain, or `bundled` feature | None |
| Supply chain | Vendored C++ tree | Pure Rust |
| Risk | Known-good audio, heavier build | Lighter, but young and unproven |

**Recommendation: `webrtc-audio-processing` with the `bundled` feature for the
first release**, because the whole point of this row is "sound as good as
Chromium did", and it is literally the same algorithm. Re-evaluate `sonora` once
it has a track record; the interface between the two is narrow enough that
swapping later is a contained change. Note the crate's own warning that it does
not follow semver strictly — pin it exactly.

### (b) Jitter buffer and packet loss concealment — the least visible, most dangerous row

This is the part nobody thinks about until it is missing. A fixed-size buffer
produces exactly the failure mode this project spent 1.0.0 fixing by other
means: one player who sounds broken to one other player, intermittently, in a
way that is very hard to reproduce.

`neteq` 0.9.1 (updated 2026-08-16) is a NetEQ-inspired adaptive jitter buffer for
audio and is the right starting point. It is a young crate: 38k downloads, still
0.x. Treat it as a component to be **measured, not trusted** — see the network
emulation harness in [05-regression-strategy.md](05-regression-strategy.md).
If it does not hold up, the fallback is to vendor and port the reference NetEQ,
which is well-specified but a multi-week job on its own.

Opus itself is not a risk: the `opus` crate (0.4.0, updated 2026-08-23) binds
libopus, the same library Chromium uses. Enable in-band FEC and DTX to match
what the browser negotiated.

### (c) Web Audio graph parity — high risk, but fully specified

`PannerNode`, `BiquadFilterNode`, `ConvolverNode`, `GainNode` and `AnalyserNode`
all have to be reimplemented. The good news is that the Web Audio API
specification defines each of them by exact formula, not by prose:

- **PannerNode**, `panningModel: 'equalpower'`, `distanceModel: 'linear'` — the
  azimuth/elevation computation, the equal-power gain pair and the linear
  distance rolloff are all closed-form. `refDistance`, `maxDistance` and
  `rolloffFactor` are used as the app already sets them (0.1, `maxDistance`, 1).
- **BiquadFilterNode**, `lowpass` and `highpass` — coefficients come from the
  Audio EQ Cookbook formulas quoted in the spec, driven by `frequency` and `Q`.
  The app uses exactly four settings: lowpass 2000/Q 20 (vents), lowpass 2300/
  Q −15 (cameras), highpass 1000/Q 10 (impostor radio).
- **ConvolverNode** — linear convolution with a normalisation factor the spec
  gives explicitly. Partitioned FFT convolution via `realfft` 3.5.0.
- **GainNode** and `setValueAtTime` — trivial, but the k-rate/a-rate distinction
  and the ramp semantics have to be respected or the pan will step audibly.
- **AnalyserNode** — the VAD depends on `getByteFrequencyData`: Blackman window,
  FFT, smoothing over time with `smoothingTimeConstant`, dB conversion clamped
  between `minDecibels` and `maxDecibels`, quantised to 0–255. All spec'd.

Because every one of these is defined by formula, **bit-comparable parity is
achievable and testable**, which is what makes this row high-risk but not
open-ended. The test strategy is golden vectors generated from the current
Electron build; see [05-regression-strategy.md](05-regression-strategy.md).

One genuine simplification: the app never uses HRTF panning, never automates
parameters beyond `setValueAtTime`, and never reconfigures the graph topology
except to switch two effects in and out. The subset needed is small.

### (d) WebRTC transport — mature enough, with one caveat

The `webrtc` crate (0.20.3 stable; 0.21.0-beta.2 published 2026-08-22) is a port
of Pion and exposes essentially the same `RTCPeerConnection` surface `peer.ts`
already wraps: offer/answer, trickle ICE, data channels, TURN with
`iceTransportPolicy: 'relay'`. The existing wrapper is 237 lines and maps onto it
almost one-to-one, which also means the four connection bugs fixed in 1.0.0
(dropped trickle candidates, the join/signal race, offer glare tearing down the
replacement, and connections stuck in `new`) have to be re-fixed deliberately in
the port rather than rediscovered. They are listed as explicit test cases.

`str0m` 0.23.1 is the alternative: sans-IO, actively maintained, and its
deterministic design would make connection-state testing much nicer. Its own
documentation says the peer-to-peer path has had less testing than the SFU path,
and this app is a pure P2P mesh, so `webrtc` is the safer default. Revisit if
`webrtc` proves awkward.

The caveat: the port must stay wire-compatible with itself across versions.
During rollout, a 1.x Electron client and a 2.x Rust client will be in the same
lobby. Interop between Chromium's WebRTC and `webrtc-rs` is standards-based and
should work, but it is an assumption to be tested early and explicitly, not
assumed — see gate G3.

## 2.4 The rows that get *better* in Rust

Worth stating plainly, because the risk table above is one-sided:

- **The renderer's Node access disappears.** Today the renderer runs with
  `nodeIntegration: true` and `contextIsolation: false`; any successful
  navigation or injection in that window has full filesystem and process access.
  The window is hardened against navigation, but the mitigation is a deny-list,
  not a boundary. A native GUI has no such surface at all.
- **The bundled Chromium leaves the attack surface** along with its CVE
  treadmill. Today every Chromium security release is a release this project has
  to ship, and a user who does not update is exposed through a browser engine
  they did not know they were running.
- **699 npm packages become roughly 250–350 crates.** Not a dramatic count
  reduction, but `cargo-vet`/`cargo-deny` and crates.io's immutability make the
  supply chain materially easier to reason about than npm's.
- **The three C/C++ native modules become safe Rust.** 4,390 lines of
  hand-written C and C++ doing pointer arithmetic on another process's memory,
  currently vendored and patched by hand, become code the compiler checks.
- **Distribution shrinks substantially.** An Electron app ships a browser
  engine; a Rust binary does not. Expect the installed footprint and idle memory
  to fall by roughly an order of magnitude, and startup to go from seconds to
  milliseconds. (Estimated, not measured — worth measuring on the current build
  before quoting numbers publicly.)
- **The 32-bit build stops being awkward.** Rust cross-compiles to
  `i686-pc-windows-msvc` cleanly, which matters because the shellcode path is
  32-bit-only.

## 2.5 What would make this a bad idea

Honest counter-arguments, for the record:

1. **It is a large project.** Roughly 37 developer-weeks before parity, and
   parity is the bar — this is a working application with users, not a
   greenfield. Call it a year with review, testing on real hardware, and the
   inevitable.
2. **The audio crates are young.** `neteq`, `sonora` and `aec3` together have
   fewer than 80k downloads. Depending on three 0.x crates for the core of a
   real-time voice application is a genuine risk, mitigated but not removed by
   the measurement harness.
3. **Immediate-mode GUI is a different paradigm.** `Settings.tsx` alone is 1,197
   lines of declarative React. egui is pleasant but the settings screen, the
   avatar compositing and the lobby table are all real work, and the result will
   not look identical.
4. **Two codebases during the transition.** Until 2.0 ships, bugs have to be
   fixed in both, or the Electron version has to be frozen — and freezing it
   means no Chromium security updates for its users.
5. **Nothing is currently broken.** After 1.0.0–1.0.2 the Electron client is on
   current dependencies with zero known vulnerabilities. The port buys
   architecture, not a fix for a present emergency.

If those outweigh section 2.4 for this project, the correct decision is to stop
after phase 0 (the server), which is worth doing on its own merits regardless.
