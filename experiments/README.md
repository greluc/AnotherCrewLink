# Experiments

Two questions from `docs/rust-port/04-implementation-plan.md` §4.3 item 9. Both take
hours to answer now and are, in the plan's words, brutal to discover in month nine — each
one decides how a later phase is planned rather than how it is written.

They stay in the workspace rather than being deleted once answered, because both check a
property that a dependency update can take away again.

## 1. `overlay-probe` — a transparent, click-through, always-on-top window

**Answered 2026-08-24: available on both Windows targets. Linux is checked in CI.**

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

Linux has no equivalent read-back here: on X11 the same property is an empty input region
set through the shape extension, and reading it back needs an X connection of the probe's
own. The Linux leg answers what the experiment exists for — does it start, is the surface
transparent, does the renderer survive — and the passthrough claim there rests on winit's
implementation.

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

Two of G2's preconditions remain open and neither is a build question. The A/B
echo-return-loss-enhancement measurement against `webrtc-audio-processing` needs real
speaker-and-mic captures on Linux, where both crates build. And sonora's bus factor is
one — 209 of its 221 commits are from a single author — which no test run changes.

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
