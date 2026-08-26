# Experiments

Two questions from `docs/rust-port/04-implementation-plan.md` §4.3 item 9, one from §4.6
item 1, and one from §4.8 item 6. All of them take hours to answer now and are, in the
plan's words, brutal to discover in month nine — each one decides how a later phase is
planned rather than how it is written.

`gui-spike` is the odd one out: it answers §4.8 item 1's framework question, and it is a
benchmark rather than a probe.

They stay in the workspace rather than being deleted once answered, because each checks a
property that a dependency update can take away again.

## 1. `overlay-probe` — a transparent, click-through, always-on-top window

**Answered 2026-08-24: available on both Windows targets.**

> **Superseded 2026-08-25.** This said "Linux is checked in CI". Linux support was
> removed from the client, and the probe's Linux arm and the `overlay-linux` job went
> with it. The Windows answer is unchanged and is the only one that describes something
> shipped.

The overlay is its own Electron `BrowserWindow` today, so whatever replaces it has to do
three things at once: be transparent, let clicks through to the game, and stay above it.
eframe's transparency has renderer-specific failures, and the known workarounds are
mutually exclusive with a single-process design — which is why this had to be answered
before the GUI phase is planned around it.

```
cargo run -p overlay-probe
cargo run -p overlay-probe --target i686-pc-windows-msvc
```

It does not report "looks right". On Windows it asks the OS what it actually did:

| Target | Result |
| --- | --- |
| `x86_64-pc-windows-msvc` | `layered=true transparent=true topmost=true`, exstyle `0x000c0138` |
| `i686-pc-windows-msvc` | identical |

`WS_EX_TRANSPARENT` is the one that matters. Without it the window can be invisible and
still swallow every click meant for the game.

**A note on how this nearly gave the wrong answer.** The first version read
`GetForegroundWindow()`, which reported `layered=false transparent=false topmost=false` —
a clean negative that would have sent the GUI phase looking for workarounds. A
click-through window with no taskbar button never *becomes* the foreground window, so it
was reading the console's styles. It now finds the window by title. Any probe that asks
the OS a question has to be sure it is asking about the right object.

There was a Linux arm here until 2026-08-25 with no equivalent read-back: on X11 the same
property is an empty input region set through the shape extension, and reading it back
needs an X connection of the probe's own. It could report that the window started and was
transparent and no more, so the passthrough claim there rested on winit's implementation
rather than on a measurement. It went with the client's Linux support.

## 2. `apm-probe` — does the echo canceller exist on 32-bit Windows?

**Answered 2026-08-24: yes. Builds, and its own test suite passes. The question then
stopped mattering the same day**, when the injection path was removed and the
`i686-pc-windows-msvc` target went with it. The result is kept because a fact does not
stop being one when it stops being needed — and because if a 32-bit target is ever wanted
again, this is the answer rather than the question.

Gate G2 precondition (a). `webrtc-audio-processing`, which the plan originally named as
the conservative choice, does not compile for either Windows target — so `sonora` is the
default and this is the question that decides whether the audio phase has an APM at all.

```
cargo build -p apm-probe --target i686-pc-windows-msvc
```

The stronger check is sonora's own suite, run against a checkout of
[`dignifiedquire/sonora`](https://github.com/dignifiedquire/sonora) at v0.2.0:

```
cargo test --target i686-pc-windows-msvc --workspace \
    --exclude sonora-bench --exclude sonora-ffi --exclude sonora-sys
```

| Profile | Result on `i686-pc-windows-msvc` |
| --- | --- |
| debug | 515 passed, 0 failed |
| release | 713 passed, 0 failed |

The release number is the one that counts: it is where the SIMD paths the plan was
worried about are actually taken.

One of G2's preconditions remains open and it is not a build question: sonora's bus
factor is one — 209 of its 221 commits are from a single author — which no test run
changes.

> **Superseded 2026-08-25.** A second precondition stood here: an A/B
> echo-return-loss-enhancement measurement against `webrtc-audio-processing`, which
> needed real speaker-and-mic captures on Linux because that is the only place both
> crates build. Dropping Linux would have made it impossible — except that the section
> below had already made it pointless on 2026-08-24, when `webrtc-audio-processing` was
> ruled out on MSVC and the comparison became sonora against `libwebrtc`. A measurement
> whose whole purpose was to rank a candidate that is out is not one this project owes.

### The comparison is no longer sonora against `webrtc-audio-processing`

**Measured 2026-08-24.** Striking gate G2's criterion 6 removed the
`i686-pc-windows-msvc` target, and that target was the only thing foreclosing
`libwebrtc` — LiveKit's binding to the real Chromium stack, which would supply AEC3, NS
and AGC together with Opus, RTP/RTCP and NetEQ as one dependency. Whether it builds where
the users are had never been asked, because until the injection path was removed it could
not matter. It does:

| Candidate | `x86_64-pc-windows-msvc` |
| --- | --- |
| `sonora` 0.2.0 | builds, links, runs |
| `libwebrtc` 0.3.45 | builds, links, runs — 48 s |
| `webrtc-audio-processing` 2.1.0 | still no. PR #102 "Support MSVC targets" open since 2026-08-08, issue #34 "Windows build" open since 2023-09-27, latest release still 2.1.0 |

So `webrtc-audio-processing` is out on the same grounds as before, and the real choice is
**sonora against libwebrtc**.

**It is not a like-for-like comparison, and the difference is not about audio quality.**
`libwebrtc` does not build the Chromium stack: `webrtc-sys-build` downloads a prebuilt
release from LiveKit and links it. That is an 86 MB `webrtc.lib` and 493 MB in the build
directory, and it is a binary this project did not compile. Two weeks of work in this
repository has gone the other way — the prebuilt `.node` files were deliberately left out
of `native/uiohook-napi` so libuiohook is compiled from the C sources in the tree, and the
code-signing application rests on every artifact being built from source in a verifiable
way. Taking `libwebrtc` would put a downloaded binary blob at the centre of the audio path
and would have to be argued for on those terms, not on ERLE.

### The A/B, and the decision

**Measured 2026-08-24.** Both cancellers, the same speech-like far end, the same echo path
— 60 ms of delay, three reflections, twelve seconds, measured over the last two after the
adaptive filter has converged:

| Canceller | ERLE |
| --- | --- |
| `sonora` 0.2.0 | **11.6 dB** |
| AEC3, through `libwebrtc` 0.3.45 | **11.3 dB** |

`cargo run -p apm-probe --release` produces the first. The second needs its own binary, for
a reason below, and is not in this repository.

**Read that difference as "no difference".** Three tenths of a decibel is not a result, and
the fact that two independent cancellers land on the same number is a warning about the
measurement rather than a finding about them: this echo path is probably easy enough that
both reach whatever ceiling the harness imposes. A real A/B with room recordings could
still separate them.

**It does not matter, because the decision does not turn on ERLE.** What the measurement
establishes is the negative that was actually in doubt — sonora is not *worse* — and with
that gone, the remaining differences all point one way:

- `libwebrtc` links a prebuilt 86 MB `webrtc.lib` downloaded from LiveKit, 493 MB in the
  build directory, compiled by nobody here. This project spent the same week removing
  prebuilt binaries from `native/uiohook-napi` so libuiohook is built from the C in the
  tree, and the code-signing application rests on that.
- It **does not link at all in release on Windows** without `-C target-feature=+crt-static`.
  The conflict is inside `webrtc-sys` itself: `desktop_frame.obj` is `/MT` and the
  cxx-generated `desktop_capturer.rs.o` is `/MD`, so `link.exe` gives LNK2038 and LNK1169.
  Debug links. Release does not. Forcing the static CRT on the whole binary to work around
  a defect in a dependency is a decision with its own consequences.
- It brings NetEQ, RTP/RTCP and Opus with it, which sounds like an argument for it until
  you notice that phase 4 has not chosen a transport yet and this would choose it.

**Decision: sonora.** Revisit if a room-recording A/B shows a real gap, or if the prebuilt
blob stops being a concern.

**A warning about measuring this.** The first attempt reported a hard failure —
`fatal error C1083: Cannot open include file: 'absl/types/optional.h'` — and the header
was there all along. The build ran under a path 298 characters deep, and `cl.exe` does not
opt in to long paths whatever `LongPathsEnabled` says. Anyone repeating this must build
from a short directory, or they will record "libwebrtc does not build on Windows" and be
wrong.

## 3. `webrtc-probe` — does the pinned WebRTC crate connect, and what does it cost?

**Answered 2026-08-25: yes, and 141 crates.**

§4.6 item 1 budgets three weeks to prove `webrtc` `=0.20.3` against a real 1.0.2 Chromium
client. Gate G3, which that spike fed, was struck on 2026-08-25 — but the crate still has
to work, and three of the spike's four questions never needed a Chromium peer.

```
cargo run -p webrtc-probe
```

Two peer connections in one process on loopback: offer, answer, candidates trickled in
both directions *after* the descriptions are set, a data channel, and one message through
it. It exits non-zero if any step does not settle within twenty seconds.

| Question | Answer |
| --- | --- |
| Does it build and run? | Yes, `x86_64-pc-windows-msvc` |
| Does a connection establish? | Yes — both ends reach `connected` |
| Trickle both ways, data channel, a message? | Yes |
| Crypto backend | `ring` 0.17.14 |
| New crates in the tree | 141 |
| `cargo deny` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo audit` | clean |
| `cargo vet` | 141 new exemptions; see `supply-chain/README.md` |

**The crypto answer is the one worth having early.** The plan called the backend "nearly
free to discover while the rig is standing and expensive to discover in P7+". It resolves
to `ring` 0.17.14 — the exact version the tree already carries for `rustls`, so there is
no second TLS backend and no version split. What `webrtc` *does* add beside it is a full
RustCrypto software stack — `aes-gcm`, `ccm`, `chacha20poly1305`, `p256`, `p384`,
`x25519-dalek` — because DTLS and SRTP need primitives `ring` does not expose. Two crypto
implementations in one binary is a fact about the shipped artefact, not a bug, and it is
better known now than at packaging time.

**`rtc-turn` is in the tree.** §4.6 gives TURN as the reason `webrtc` wins over `str0m`,
which "explicitly does not implement TURN". The dependency list confirms the premise
rather than leaving it as a claim.

**What it does not prove, and nothing else now does either.** Both ends are this same
crate, on loopback, with no relay and no NAT. G3 was the thing that would have proved
interoperability with a 1.0.2 Chromium client, through coturn, across a NAT — and it was
struck. So "the crate is usable" is established and "the client is interoperable" is not,
by anything, until the field says so.


## 4. `gpu-probe` — which renderer rungs does a Windows machine actually offer?

**Answered 2026-08-26: two, not three. It removed a rung from the plan.**

§4.8 item 6 asked for "wgpu/DX12, then WARP through `force_fallback_adapter`, then a CPU
rasteriser". Two of those three are assertions about what Windows provides, and the client
has to choose between them at start-up on a machine nobody here has seen. So this
enumerates the DX12 adapters and prints what each one is.

```
cargo run -p gpu-probe --release
```

```text
RESULT adapters=3
  DiscreteGpu name="NVIDIA GeForce RTX 5090" driver="32.0.16.1656" backend=Dx12
  IntegratedGpu name="AMD Radeon(TM) Graphics" driver="32.0.21045.5002" backend=Dx12
  Cpu name="Microsoft Basic Render Driver" driver="10.0.26100.8972" backend=Dx12
RUNG Hardware available=true
RUNG SoftwareAdapter available=true
```

**WARP *is* the CPU rasteriser.** The third adapter is Windows's own Direct3D 12
implementation running on the processor, and its driver version is the operating system's
build number rather than a vendor's — which is what says it ships with Windows rather than
with a card. The plan's second and third rungs named one adapter twice.

Nothing was lost by dropping the third. There is no CPU rasteriser for egui outside a wgpu
adapter — no crate provides one — so it named something that could not have been built.
What it was there to guarantee still holds and holds better: the last rung is part of the
operating system rather than of a driver.

It stays in the workspace for the reason all of these do. If a future wgpu drops the DX12
`Cpu` adapter, or a machine turns out not to enumerate it, this is what says so — and the
answer is load-bearing, because "no GPU is not a failure to launch" rests on that one row.

The probe reports through `acl_ui::renderer::choose`, the shipped rule, rather than through
a copy of it: the `RUNG` lines are what the client would decide given the adapters above.
