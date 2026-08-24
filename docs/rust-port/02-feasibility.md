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
they are today, with two exceptions.

The first is inherited rather than created. The offsets the memory reader depends
on are fetched from an unpinned branch of a third-party repository, and those
numbers drive both the pointer arithmetic and the addresses the injection stub
patches. That is the largest single risk in this project, it is live in 1.0.2
today, and it has to be fixed in the Electron client before the Rust reader
consumes the same format.

The second is created by the port. Electron spreads this work across several
processes; a single Rust binary would put an elevated, unsandboxed process around
the RTP parser, the Opus decoder, the image decoder and a process-memory writer
at once. The answer is two processes rather than one, and it is a decision to
make before the first line of the client, not after — see row 17.

So the recommendation is: **yes, port — but in the order given in
[04-implementation-plan.md](04-implementation-plan.md), with a hard go/no-go gate
after the audio engine is proven and before any GUI work starts.** The audio
engine is the part that decides the project. Building it first, standalone and
measurable, means a no-go costs one phase rather than a year. The offsets work
starts earlier still, on the Electron client, because its format has to be proven
before the Rust reader is written against it.

## 2.2 Component-by-component

Risk is the chance of a regression users would notice, or of the component not
reaching parity at all. Effort is for one experienced developer.

| # | Component | Rust answer | Risk | Effort |
| --- | --- | --- | --- | --- |
| 1 | Signalling server | `socketioxide` + `axum` + `tower-http` | **Low** | 4 wk |
| 2 | Process memory reading | `windows-sys` / `nix` (`process_vm_readv`) | **Low** | 2–3 wk |
| 3 | Pattern scanning, pointer walking, struct parsing | hand-written, `zerocopy` | **Low** | 2 wk |
| 4 | Shellcode injection | `windows-sys`, bytes carry over verbatim | **Low** | 1 wk |
| 5 | Global keyboard hook | `windows-sys` (`GetAsyncKeyState` poll) / X11 | **Low** | 1 wk |
| 6 | Avatar recolouring | `image` | **Low** | 3 d |
| 7 | Settings, logging, offsets, VDF | `serde`, `tracing`, `ureq` | **Low** | 1 wk |
| 8 | Overlay window | `winit` + raw window handle | **Medium** | 2–3 wk |
| 9 | GUI | `egui` / `eframe` | **Medium** | 8–12 wk |
| 10 | Localisation (37 locales) | hand-written loader over the existing JSON | **Low** | 1 wk |
| 11 | WebRTC transport (ICE/DTLS/SRTP/TURN) | `webrtc` crate | **Medium** | 10.5 wk |
| 12 | Audio capture and playback | `cpal` | **Low–Medium** | 2 wk |
| 13 | **AEC / NS / AGC** | `sonora` (`webrtc-audio-processing` as a Linux baseline) | **High** | 3–5 wk |
| 14 | **Opus + jitter buffer + PLC** | `opus` + `neteq` | **High** | 4–6 wk |
| 15 | **Web Audio graph parity** | hand-written DSP + `fft-convolver` | **High** | 5–8 wk |
| 16 | Packaging, signing, auto-update | `cargo-dist` archives + `minisign` | **Medium** | 11 wk |
| 17 | Two-process split and its IPC | length-prefixed `postcard` over a named pipe / Unix socket | **Medium** | 2 wk |
| 18 | **Offsets trust chain** | mirror, signed bundle, embedded floor, structural validator | **High** | 6.5 wk |

Rows 13–15 are the audio problem. Rows 1–7 are close to mechanical. Row 18 is the
largest single risk in the document and the only one that is already live in the
shipped client.

The effort column is per component and the entries overlap — rows 12–15, for
instance, are all scheduled inside one phase. Do not add the column up; the
schedule is in [04-implementation-plan.md](04-implementation-plan.md) as extended
by [09-technology-migration.md](09-technology-migration.md), and it comes to
roughly 77 developer-weeks to 2.0. Treat 77 as the midpoint of a range whose low
end is 68. That figure covers work these rows do not: nine developer-weeks of
hardening on the Electron client and the Node server before and alongside phase
0, and four weeks of bridge and sunset after packaging. A further three weeks of
post-1.x cleanup sit outside the 2.0 budget.

**Rows 13 and 14 are High partly because of a target.** `i686-pc-windows-msvc`
exists only for the injection path, and it is what forecloses LiveKit's
`libwebrtc` binding — the one dependency that would supply AEC3, NS, AGC, Opus
with FEC and DTX, RTP/RTCP and NetEQ together, and whose build script maps only
x86_64 and aarch64. It is also what puts NASM in the build once TLS enters the
tree, and what turns MSVC's 4-byte alignment of larger types into a real
unsoundness hazard for the struct parsing in row 3. Splitting injection into a
small 32-bit helper process, leaving the rest of the client 64-bit only, is an
explicit open decision rather than a settled one; taking it would move rows 13
and 14 from High to Low.

**Row 17 is not in the current architecture and is the one structural change this
table asks for.** `aucl-helper` runs elevated and holds memory reading,
injection, the keyboard hook and the overlay window. `aucl-core` never runs
elevated and holds tokio, signalling, WebRTC, audio and the GUI. The overlay
belongs in the elevated half because UIPI blocks window manipulation across
integrity levels, so an unelevated overlay stops following an elevated game —
which is the configuration the README instructs people into. It receives
pre-rasterised sprites and never decodes an image, so no image decoder enters the
elevated process.

**Row 9 has a precondition that must not be deferred into the phase it
protects,** and row 8 shares it: a transparent, click-through, always-on-top
window, prototyped on Windows x64, Windows i686 and Linux in phase 1, before GUI
work begins. Transparency under eframe on Windows is the weakest-evidenced part
of the GUI plan — egui #4451 has been open since 2024-05-03 and the known
workarounds are renderer-specific, and #4091 crashes on precisely the overlay's
combination of fullscreen, transparent and always-on-top. Since eframe picks one
renderer per process for every viewport, that is a constraint on the framework
decision and not a detail inside it. Row 9 also carries a requirement the table
cannot show: a GPU fallback chain. Linux defaults to software, as the Electron
client already does; Windows goes wgpu/DX12, then WARP, then a CPU rasteriser,
with no glow rung.

## 2.3 The four things that decide it

### (a) Acoustic echo cancellation and noise suppression — solvable, but pick carefully

Two settings in the UI (`echoCancellation`, `noiseSuppression`) currently map
straight onto Chromium's `getUserMedia` constraints. Users play with speakers
more often than headsets; without AEC the port would ship an obvious, loud
regression on day one.

Two credible options, and the choice is settled by which of them builds:

| | `sonora` 0.2.0 | `webrtc-audio-processing` 2.1.0 |
| --- | --- | --- |
| What it is | Pure-Rust port of Google's C++ APM (AEC3, NS, AGC, HPF), from WebRTC M145 | Rust bindings over that same C++ APM |
| Builds for `x86_64-pc-windows-msvc` | Yes | **No** |
| Builds for `i686-pc-windows-msvc` | **Unproven** | **No** |
| Builds for Linux | Yes | Yes |
| Algorithm quality | Validated against the C++ reference test suite (2,400+ tests), within 1.07–1.24× the C++ runtime | The reference implementation |
| Build cost | None — no C++ toolchain, no meson, no submodule | A C++ toolchain, or `bundled`, which needs meson, ninja and clang/gcc on every machine |
| Maturity | 15k downloads, two releases, one author | 104k downloads, updated 2026-05-13 |
| Licence | BSD-3-Clause | `license-file = "COPYING"`, so crates.io reports it as non-standard and cargo-deny needs a clarification entry |

`webrtc-audio-processing` 2.1.0 does not compile for either Windows target. PR
#102, "Support MSVC targets", has been open and unmerged since 2026-08-08; issue
#34, "Windows build", has been open since 2023-09-27; and the crate's CI matrix
runs on `ubuntu-latest` only, so nothing upstream would catch a regression even
after #102 lands. PR #102 is scoped to x86_64 alone — i686 has no evidence of
ever having been attempted. Windows is the overwhelming majority of this app's
users, so "the conservative choice, because it is literally the same algorithm"
is not a choice that is available on the platform that has the users.

**Recommendation: `sonora` 0.2.0 as the default APM, conditional on a green
`cargo build --target i686-pc-windows-msvc`.** That build is a precondition of
the recommendation, not a follow-up action, and it belongs at gate G2 alongside
an A/B echo-return-loss-enhancement measurement of the two on Linux, where both
build, against real speaker-and-mic captures. `sonora`'s 32-bit status is
genuinely unproven: its README validates on Ubuntu x86_64 and its SIMD paths are
SSE2/AVX2/NEON. If it will not build for i686 and nothing else will either, the
APM decision reopens at G2, while the port is still stoppable.

`webrtc-audio-processing` stays in the workspace as a Linux-only test baseline,
pinned `=2.1.0` — the crate's own warning is that it does not follow semver
strictly — because a reference implementation that builds on one platform is
still the right thing to measure against. Keep the trait boundary in either case;
it is what makes the baseline usable and what makes a later swap contained.

The risks that come with `sonora` are real, but they are not the ones the
"young and unproven" framing names. Running the reference implementation's own
conformance suite is a stronger correctness argument than a download count. What
should be watched instead is a bus factor of one (209 of 221 commits by a single
author), two releases in total, an AI-assisted port acknowledged in the project's
own history, and the unproven 32-bit build. Those are gate items, and the first
three are the reason the trait boundary is not optional.

### (b) Jitter buffer and packet loss concealment — the least visible, most dangerous row

This is the part nobody thinks about until it is missing. A fixed-size buffer
produces exactly the failure mode this project spent 1.0.0 fixing by other
means: one player who sounds broken to one other player, intermittently, in a
way that is very hard to reproduce.

`neteq` 0.9.1 (updated 2026-08-16) is a NetEQ-inspired adaptive jitter buffer for
audio and is the right starting point. It is a young crate: 38k downloads, still
0.x. Take it with `default-features = false` — its default feature set pulls a
web framework, a CLI parser, a second `cpal` three majors behind the one this
plan pins, and a second Opus implementation — and implement its `AudioDecoder`
trait over the `opus` crate, so libopus remains the only codec in the binary.
Treat the crate itself as a component to be **measured, not trusted** — see the
network emulation harness in
[05-regression-strategy.md](05-regression-strategy.md). If it does not hold up,
the fallbacks are a well-tuned fixed jitter buffer with Opus in-band FEC and PLC,
which is what most peer-to-peer voice apps ship, or vendoring and porting the
reference NetEQ, which is well-specified but a multi-week job on its own.

Opus itself is not a risk: the `opus` crate binds libopus, the same library
Chromium uses. Pin `=0.3.1` for now. 0.4.0 was published on 2026-08-23 and its
entire content is a supply-chain change — the `-sys` backend moved from
`audiopus_sys` to `opusic-sys` — so move to it only once `opusic-sys` is shown to
link on `i686-pc-windows-msvc`, which its CI does not cover. That is a one-hour
experiment, not a research question.

In-band FEC is not a flag. libwebrtc emits Opus FEC only once RTCP receiver
reports tell it there is loss, so a Rust client that sets the flag and sends no
RR achieves nothing — and because the Chromium peer never learns it is losing
packets either, it stops emitting FEC too, in both directions, for that one pair.
The receive half has to be implemented as well: `decode(..., fec: true)` on
packet *N+1* to reconstruct *N*, driven by the jitter buffer's loss signal.
Whether `neteq` 0.9.1 can signal loss to the decoder in a way that permits
out-of-order recovery is **unverified** — its documented surface says nothing
about it — which is why it is criterion 5 of gate G2 rather than a phase-4
detail. If the answer is no, the cost of vendoring the reference NetEQ is decided
at the gate that can still stop the port.

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
  The app uses exactly three settings: lowpass 2000/Q 20 (vents), lowpass 2300/
  Q −15 (cameras), highpass 1000/Q 10 (impostor radio).
- **ConvolverNode** — linear convolution with a normalisation factor the spec
  gives explicitly. This is the one node that is an algorithm rather than a
  formula: uniformly partitioned FFT convolution with correct overlap-add
  accumulation and latency alignment, and no allocation or denormal stall in the
  callback. Its failure modes are quiet — a reverb tail slightly late or slightly
  smeared produces no crash and no bug report anyone can articulate. Use
  `fft-convolver` 0.4.0 for the general part and apply the normalisation scalar,
  which genuinely is the one line the spec gives. The impulse response is
  decoded in `xtask` and the PCM embedded, so nothing decodes a media file at
  runtime.
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

### (d) WebRTC transport — the right crate, but it is a port and not a mapping

The `webrtc` crate covers what `peer.ts` needs: offer/answer, trickle ICE, data
channels, TURN with `iceTransportPolicy: 'relay'`. Pin it `=0.20.3` — the
maintainer states that a minor bump may carry breaking changes, and
0.21.0-beta.2 already adds crypto-backend features and tightens a `build()`
bound.

Two things about that crate govern the estimate. It is **not** a port of Pion.
Since 0.20.0, published 2026-07-31, it is a thin async wrapper over a new sans-IO
`rtc` core — a rewrite, described by its own announcement as one, with four patch
releases behind it. And its event surface is not the one `peer.ts` was written
against: `webrtc` 0.20 takes a single `Arc<dyn PeerConnectionEventHandler>` at
`PeerConnectionBuilder::with_handler()`, with **no per-event detach**. Lines
188–192 of `peer.ts` null all five event handlers before `pc.close()`, and that
teardown is exactly how the 1.0.0 fixes avoid acting on events from a connection
that is being replaced. In the port it has to become a generation counter or an
atomic detached flag inside the handler. That is where offer glare and
stuck-in-`new` come back, and "maps almost one-to-one" hides it. The four
connection bugs fixed in 1.0.0 — dropped trickle candidates, the join/signal
race, offer glare tearing down the replacement, and connections stuck in `new` —
have to be re-fixed deliberately, against a different event model, and they are
listed as explicit test cases. The `&self` handler signature also forces interior
mutability through the peer layer, which collides with the no-lock rule on the
audio path and has to be designed around rather than discovered.

`str0m` 0.23.1 is the alternative, and the reason to decline it is TURN. Its
README states that TURN is out of scope and its feature table marks it absent.
This app depends on TURN: `validateClientPeerConfig.ts` validates a server-pushed
`forceRelayOnly` flag together with `turn:`/`turns:` URLs carrying username and
credential, and the client applies relay-only on top of whatever the server
sends. Choosing `str0m` means writing or sourcing an RFC 8656 client — Allocate,
Refresh, CreatePermission, ChannelBind, long-term credentials with realm, nonce
and stale-nonce handling — which is a multi-week job in exactly the wrong
category for hand-writing. `webrtc-rs` ships `rtc-turn`. Record what is being
given up rather than pretending the chosen option dominates: `str0m`'s
deterministic sans-IO testing model is still the nicer one, and it ships a
GCC-style bandwidth estimator and pacer where `rtc` ships TWCC feedback with
nothing on top of it.

Neither crate can demonstrate automated Chromium interop, and Chromium interop is
the whole constraint, so the choice is **unsettled until it is measured**: the
first three weeks of the transport phase are a spike on all three targets,
proving `webrtc` 0.20 against a real 1.0.2 client, with `str0m` timeboxed to a
written feasibility read that answers only what TURN client and what event loop a
14-peer mesh would need.

The caveat that remains: the port must stay wire-compatible with itself across
versions. During rollout, a 1.x Electron client and a 2.x Rust client will be in
the same lobby. Interop between Chromium's WebRTC and `webrtc-rs` is
standards-based and should work, but it is an assumption to be tested early and
explicitly, not assumed — see gate G3, which now runs the 1.x-to-2.x call under
each impairment profile rather than only on a clean network, and adds a
three-client mixed-generation row.

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
- **The supply chain becomes one that can be reasoned about.** This is not a
  count reduction, and the count should not be quoted until it is measured: the
  networking domain alone resolves to 228 crates as the plan is now written —
  272 before `reqwest` gave way to `ureq` — so 699 npm packages become something
  well past 350 once audio, GUI and platform are added. Take the number from a real `Cargo.lock` at the end of
  phase 1. The win is a different one and it survives:
  crates.io artefacts are immutable, install-time script execution collapses to
  `build.rs` that can be listed and audited, and `cargo-deny` and `cargo-audit`
  give a policy gate and a scheduled feed that npm's equivalent does not match.
  Two limits on that win belong in the same breath: `cargo-vet` has no audits in
  any shared set for the crates that actually matter here, so it starts as an
  exemptions block rather than as coverage; and RustSec does not track CVEs in C
  vendored inside `-sys` crates, so `cargo audit` will not report a libopus or
  APM security release. Today one Electron bump patches all of that at once, with
  CVE numbers. After the port it is a named human with a watch list.
- **The three C/C++ native modules stop being hand-maintained.** 4,390 lines of
  vendored, hand-patched C and C++ doing pointer arithmetic on another process's
  memory become Rust. What that does *not* mean is less C and C++ in the address
  space. The port keeps libopus, which is an order of magnitude more C than it
  removes, adds whatever the TLS stack brings, and adds a bundled WebRTC C++ tree
  on any machine that builds the comparison baseline in §2.3(a). Net native code
  goes up, not down, and unlike today none of it sits behind a process boundary
  unless row 17 is built. The honest claim is that the C and C++ *this project
  maintains itself* goes to zero, which is worth having on its own.
- **Distribution shrinks substantially.** An Electron app ships a browser
  engine; a Rust binary does not. Expect the installed footprint and idle memory
  to fall by roughly an order of magnitude, and startup to go from seconds to
  milliseconds. (Estimated, not measured — worth measuring on the current build
  before quoting numbers publicly.)
- **The 32-bit build stops being awkward to produce.** Rust cross-compiles to
  `i686-pc-windows-msvc` as a Tier 1 target with host tools, which is better than
  the current arrangement for the shellcode path, which is 32-bit-only. The
  target itself does not stop being awkward — see §2.2 — and the cheapest way to
  collect the benefit without the constraint is to stop building the whole client
  for it.

## 2.5 What would make this a bad idea

Honest counter-arguments, for the record:

1. **It is a large project.** Roughly 77 developer-weeks to 2.0 — the midpoint of
   a range whose low end is 68 — and parity is the bar, because this is a working
   application with users, not a greenfield. That figure includes nine weeks of
   hardening on the Electron client and the Node server before and alongside
   phase 0, and four weeks of bridging the installed base afterwards; three
   further weeks of post-1.x cleanup sit outside it. A second developer does not
   halve it, because the audio engine is no longer the sole critical path — the
   transport phase now rivals it. Call it well over a year with review, testing
   on real hardware, and the inevitable.
2. **The audio crates are young.** `neteq` and `sonora` are both 0.x, and
   `sonora` now carries more load than any other crate in the project: it is the
   default acoustic echo canceller on the platform that has the users, it has two
   releases and one author, and its 32-bit build is unproven. Depending on two
   0.x crates for the core of a real-time voice application is a genuine risk,
   mitigated but not removed by the measurement harness and the trait boundary.
3. **Immediate-mode GUI is a different paradigm.** `Settings.tsx` alone is 1,197
   lines of declarative React. egui is pleasant but the settings screen, the
   avatar compositing and the lobby table are all real work, and the result will
   not look identical.
4. **Two codebases during the transition.** Until 2.0 ships, bugs have to be
   fixed in both, or the Electron version has to be frozen — and freezing it
   means no Chromium security updates for its users.
5. **The port is not the fix for what is currently broken.** After 1.0.0–1.0.2
   the Electron client is on current dependencies with zero known
   vulnerabilities in them. The things that are wrong — the offsets trust chain,
   the update path, the signal envelope — are wrong in the design, not in a
   dependency, and every one of them is fixed in the Electron client and the
   Node server by the hardening track, before the Rust client exists. The port
   buys architecture. It does not buy any of those fixes, and waiting for it to
   deliver them would be the expensive way to get them.

If those outweigh section 2.4 for this project, the correct decision is to stop
after phase 0 (the server), which is worth doing on its own merits regardless.
The hardening track is not part of that decision: it ships on the Electron client
and the Node server, it protects the fleet that will never see 2.x, and it is
worth doing whether or not a single line of the Rust client is ever written.
