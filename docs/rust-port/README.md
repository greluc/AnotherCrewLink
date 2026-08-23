# Porting AnotherCrewLink to Rust

An assessment of whether the client, the server and the native parts of this
project can be rewritten in Rust with a native GUI, and — since they can — how.

Written against version 1.0.2 of the client and 1.0.0 of the server, on
2026-08-23.

## The short version

**Feasible. No hard blockers. Recommended in the staged order below, with a
go/no-go decision after the audio engine and before any GUI work.**

Every component has a working Rust answer. The difficulty is concentrated almost
entirely in one place: Chromium currently supplies the whole real-time voice
stack below the Web Audio graph — echo cancellation, noise suppression, automatic
gain control, Opus with FEC and DTX, RTP/RTCP, the NetEQ adaptive jitter buffer,
packet loss concealment, device handling and resampling. None of that is in this
repository, and all of it would have to be assembled from Rust crates that are
younger and less exercised than the code they replace.

Everything else — the memory reader, the shellcode injection, the keyboard hook,
the overlay, the server, the GUI, the build pipeline — is ordinary work, and most
of it ends up *safer* in Rust than it is today.

So: build the audio engine first, standalone, measured against golden vectors
captured from the current Electron build. If it reaches parity, the rest is
schedule. If it does not, the project stops having spent one phase rather than a
year — and the phases before it (a Rust server, a Rust game reader) are worth
having on their own.

## Effort

Roughly **37 developer-weeks** to parity for one developer; about 26 for two,
with the audio engine on the critical path throughout.

## Verdict per component

| | Risk | |
| --- | --- | --- |
| Server, memory reader, injection, key hook, avatars, settings | **Low** | close to mechanical |
| Overlay, WebRTC transport, GUI, packaging | **Medium** | ordinary work |
| **AEC/NS/AGC, Opus + jitter buffer, Web Audio parity** | **High** | this is the project |

## Documents

| | |
| --- | --- |
| [01-inventory.md](01-inventory.md) | What exists today: sizes, components, dependencies, what Chromium provides for free |
| [02-feasibility.md](02-feasibility.md) | Component-by-component feasibility, the four things that decide it, and the case against |
| [03-target-architecture.md](03-target-architecture.md) | Workspace layout, threading model, crate responsibilities, GUI framework choice |
| [04-implementation-plan.md](04-implementation-plan.md) | Eight phases, three gates, milestones, effort |
| [05-regression-strategy.md](05-regression-strategy.md) | How parity is measured rather than asserted; the bugs that must not come back |
| [06-security.md](06-security.md) | What improves, what gets riskier, and the checklist |
| [07-dependencies-toolchain.md](07-dependencies-toolchain.md) | Every crate and tool at its latest stable version, with the dependency policy |
| [CLAUDE.rust.md](CLAUDE.rust.md) | The `CLAUDE.md` the Rust workspace starts with; copy to the repository root at phase 1 |

## The three gates

| Gate | After | Criterion | If it fails |
| --- | --- | --- | --- |
| **G1** | Game reader | `AmongUsState` matches the Electron reader exactly on every recorded frame | Bug; fix and retry |
| **G2** | Audio engine | DSP within −80 dBFS of golden vectors; added latency within 30 ms and quality within 0.2 MOS of Chromium under emulated loss and jitter | **Stop the port** |
| **G3** | Transport | A 1.0.2 Electron client and a Rust client hear each other in the same lobby, direct and via TURN | No staged rollout; reconsider scope |

G2 is the one that can end the project, and it is reached in month four rather
than month twelve. That is the entire point of the ordering.
