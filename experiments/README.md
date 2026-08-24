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
