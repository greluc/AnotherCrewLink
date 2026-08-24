# Porting AnotherCrewLink to Rust

An assessment of whether the client, the server and the native parts of this
project can be rewritten in Rust with a native GUI, and — since they can — how.

Written against version 1.0.2 of the client and 1.0.0 of the server, on
2026-08-23.

## The short version

**Feasible. No hard blockers. Recommended in the staged order below, with a
go/no-go decision after the audio engine and before any GUI work — at roughly
twice the effort first estimated here.**

Every component has a working Rust answer. The difficulty is concentrated almost
entirely in one place: Chromium currently supplies the whole real-time voice
stack below the Web Audio graph — echo cancellation, noise suppression, automatic
gain control, Opus with FEC and DTX, RTP/RTCP, the NetEQ adaptive jitter buffer,
packet loss concealment, device handling and resampling. None of that is in this
repository, and all of it would have to be assembled from Rust crates that are
younger and less exercised than the code they replace.

Everything else — the memory reader, the shellcode injection, the keyboard hook,
the overlay, the server, the GUI, the build pipeline — is ordinary work, and the
4,390 lines of hand-written C and C++ it replaces end up *safer* in Rust. The
client as a whole does not: the port brings libopus and an echo canceller's C++
in with it, so it is written as two processes — an elevated helper holding only
the memory reader, the injection path, the key hook and the overlay, and an
unelevated process holding tokio, signalling, WebRTC, audio and the GUI.

So: build the audio engine first, standalone, measured against golden vectors
captured from the current Electron build. If it reaches parity, the rest is
schedule. If it does not, the project stops having spent one phase rather than a
year — and the phases before it (a Rust server, a Rust game reader) are worth
having on their own.

Two things changed after the first draft. The bill is roughly twice the original
estimate — ~77 developer-weeks rather than 37, from work priced too low rather
than scope invented since. And establishing what the port would inherit turned up
problems in the *shipped* 1.0.2 client: an unpinned third-party offsets feed that
drives both the pointer arithmetic and the addresses the injection stub patches,
an update path that can run an unsigned installer with administrator rights, and
a signalling envelope that lets any lobby member read every player's position.
Those are worth fixing whatever happens to the port, and they ship as 1.0.3
through 1.0.5 before and alongside the first port phase.

## Effort

Roughly **77 developer-weeks** to 2.0 for one developer. Treat that as the
midpoint of a range whose low end is around 68: it is the union of independently
priced corrections and several of them overlap. Two developers do not halve it,
and the audio engine is no longer alone on the critical path — the transport
phase now rivals it.

```
H1  1.x emergency hardening      2.0   ships as 1.0.3
H2  1.x offsets trust chain      4.0   ships as 1.0.4, ends at G0
H3  1.x/Node envelope + OBS      3.0   ships as 1.0.5 plus a server release
P0+ Server                       4.0
P1+ Foundations                  5.0
P2+ Game reader                  6.0   G1
P3+ Audio engine                10.0   G2
P4+ Transport                   10.5   G3
P5+ Platform                     6.0
P6+ GUI                         11.5
P7+ Packaging and signing       11.0
P8  Bridge and sunset            4.0   G4
                                ----
                                77.0
P9  Post-1.x cleanup             3.0   outside the 2.0 budget
```

The three phases that moved most are not the security work: transport, because
the sans-IO rewrite of the `webrtc` crate killed the premise that 237 lines of
`peer.ts` map one-to-one onto it; packaging, because `cargo-dist` cannot build
either of the two artefact types this project must keep producing; and
foundations, because the Socket.IO client moves there from the transport phase,
where it was crowding out the entire WebRTC half.

## Verdict per component

| | Risk | |
| --- | --- | --- |
| Memory reader, injection, key hook, avatars, settings | **Low** | close to mechanical |
| Server | **Low** | mechanical, apart from an owned lobby registry and the signal envelope rules |
| Overlay, GUI | **Medium** | ordinary work; prototype a transparent click-through window before the GUI phase |
| WebRTC transport, packaging and update integrity | **Medium** | ordinary work, and where the estimate moved most |
| **The offsets supply chain** | **High** | the largest single risk in the project, and it is live in the shipped client |
| **AEC/NS/AGC, Opus + jitter buffer, Web Audio parity** | **High** | this is the project |

One decision is still open, and it would move two rows of the risk table from
High to Low: the `i686-pc-windows-msvc` target exists only for the injection
path, and it is what forecloses LiveKit's libwebrtc binding, puts NASM in the
build, and creates the alignment hazard MSVC brings to the struct parsing in
`aucl-game`. Splitting injection into a small 32-bit helper process removes all
three. It has not been decided either way.

## Documents

| | |
| --- | --- |
| [01-inventory.md](01-inventory.md) | What exists today: sizes, components, dependencies, what Chromium provides for free |
| [02-feasibility.md](02-feasibility.md) | Component-by-component feasibility, the four things that decide it, and the case against |
| [03-target-architecture.md](03-target-architecture.md) | Workspace layout, threading model, crate responsibilities, GUI framework choice |
| [04-implementation-plan.md](04-implementation-plan.md) | Ten phases and a hardening track, five gates, milestones, effort — the phase map, the gates and the effort are extended by 09 |
| [05-regression-strategy.md](05-regression-strategy.md) | How parity is measured rather than asserted; the bugs that must not come back |
| [06-security.md](06-security.md) | What improves, what gets riskier, and the checklist — read with 08 §5 and 09 §2.1 |
| [07-dependencies-toolchain.md](07-dependencies-toolchain.md) | Every crate and tool at its latest stable version, with the dependency policy |
| [08-dependency-review.md](08-dependency-review.md) | A second opinion on every crate in 07, checked against crates.io and the advisory database on 2026-08-24 |
| [09-technology-migration.md](09-technology-migration.md) | Where to change technology rather than crate, and the phased migration for each change |
| [CLAUDE.rust.md](CLAUDE.rust.md) | The `CLAUDE.md` the Rust workspace starts with; copy to the repository root at phase 1 |

## Read 08 and 09 before acting on 07

Two later reviews correct this set of documents rather than extending it. The crate
list in 07 was checked against crates.io: one version does not exist, and the echo
canceller 02 recommends as the conservative choice does not build on either Windows
target. The migration review found three security problems in the **shipped 1.0.2
client** while establishing what the port would inherit; those are hardening work for
1.0.3 through 1.0.5, not port work. 06 does not mention the offsets supply chain at
all, which is the item both reviews rank first — 09 §2.1 is the replacement for the
paragraph it is missing. The effort, the phase map and the gate list on this page
already reflect both. Both reviews are summarised at the top of their own documents.

## The five gates

| Gate | After | Criterion | If it fails |
| --- | --- | --- | --- |
| **G0** | 1.x offsets trust chain (H2) | A signed bundle verifies on every load including from cache; a malicious-bundle corpus is rejected with a distinct error each; the validator accepts all 81 real upstream files unchanged; a revocation drill recovers a client without a client release; a signed bundle is published within 6 hours of a real Among Us update | P2+ does not start its offsets work |
| **G1** | Game reader | `AmongUsState` matches the Electron reader exactly on every recorded frame | Bug; fix and retry |
| **G2** | Audio engine | DSP within −80 dBFS of golden vectors; added latency within 30 ms and quality within 0.2 MOS of Chromium under emulated loss and jitter; the receive path recovers Opus in-band FEC from a Chromium sender at 5% loss; and a green build of the chosen APM on `i686-pc-windows-msvc` | **Stop the port** |
| **G3** | Transport | A 1.0.2 Electron client and a Rust client hear each other in the same lobby, direct and via TURN; the same call repeated under each impairment profile; and a three-client mixed-generation lobby with one client leaving and rejoining | No staged rollout; reconsider scope |
| **G4** | Bridge (P8) | Real 1.0.2 installs on Windows x64, Windows ia32 and Linux each update from a staging feed to the bridge, silently, with the correct architecture selected | No fleet migration; 2.0 stays a parallel install |

G2 is the one that can end the project, and it is still reached well before the
half-way mark — month eight of eighteen with the hardening track counted, rather
than at the end. That is the entire point of the ordering. Every amendment to G2
and G3 makes them harder to pass; none makes them easier.
