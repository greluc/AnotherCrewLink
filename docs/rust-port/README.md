# Porting AnotherCrewLink to Rust

An assessment of whether the client, the server and the native parts of this
project can be rewritten in Rust with a native GUI, and — since they can — how.

Written against version 1.0.2 of the client and 1.0.0 of the server, on
2026-08-23.

> **Windows only, from 2026-08-25.** The client's Linux support was removed and the
> minimum Windows raised to 11, because nobody on this project can test either. These
> documents were written for three targets and still describe Linux in many places.
> Where a passage states a requirement it now carries a dated note; where it records
> an analysis or a measurement it is left as it was, because it is a true account of
> why something was chosen. The single shipping target is
> `x86_64-pc-windows-msvc`.
>
> Two things this does **not** mean. Linux CI runners are still used for work that
> has no target — formatting, licences, advisories, CodeQL, fuzzing — which is a
> runner choice and not a supported platform. And the AppImage users who exist are
> not migrated anywhere: every release from 1.0.1 to 1.0.5 published one, and those
> clients will sit on a `latest-linux.yml` feed that stops moving.

## The short version

**Feasible. No hard blockers. Recommended in the staged order below, at roughly
twice the effort first estimated here — but what is actually being built today
is the hardening track and the Rust server, and nothing beyond them is
committed.**

> **Update, 2026-08-24, after `P0+` shipped.** The Rust server is written, tested
> and merged, and the Node implementation has been **deleted** from the server
> repository — that repository is now the Rust server, with the crate at its root.
> The Node server is still the process running in production; what changed is that
> nobody will fix it there any more, so deploying the Rust one is now the thing that
> closes the gap rather than a thing to schedule later.
>
> One consequence for what follows. `H3`'s server half is already done: the envelope
> rules and first-claimer host are enforced in the Rust server from its first commit,
> so there is no Node change to make and no dual-stack window to manage.
>
> The OBS overlay and the mobile relay **stay in the Electron client** as they are.
> They are not carried into the Rust client, and the Rust server refuses the shape
> they use — a signal addressed to a room name — so both stop working the day it is
> deployed. That is the cost recorded in §4.2, paid once, and it takes no client
> release to pay: voice is unaffected either way.
>
> The abbreviation for the client and the server is `acl`. Crate names below follow.
> The deployment hostname is unrelated and unchanged.

**Scope, as of 2026-08-24.** `H1`–`H3` and `P0+` are funded. When the Rust
server ships there is an explicit decision point on whether the rest of the port
proceeds, taken on what building it actually cost rather than on this document.
Everything below is the plan for that port and is written as such; read it as a
route that has been surveyed, not a journey under way. The hardening track is
unaffected either way — it runs on the shipped Electron client and protects the
fleet that will never see 2.x.

Every component has a working Rust answer. The difficulty is concentrated almost
entirely in one place: Chromium currently supplies the whole real-time voice
stack below the Web Audio graph — echo cancellation, noise suppression, automatic
gain control, Opus with FEC and DTX, RTP/RTCP, the NetEQ adaptive jitter buffer,
packet loss concealment, device handling and resampling. None of that is in this
repository, and all of it would have to be assembled from Rust crates that are
younger and less exercised than the code they replace.

> **Read as of its date.** The injection path described below was removed on
> 2026-08-24, taking the `i686-pc-windows-msvc` target with it. The assessment is
> left as written; §4.4 item 6 of
> [04-implementation-plan.md](04-implementation-plan.md) records the decision.

Everything else — the memory reader, the shellcode injection, the keyboard hook,
the overlay, the server, the GUI, the build pipeline — is ordinary work, and the
4,390 lines of hand-written C and C++ it replaces end up *safer* in Rust. The
client as a whole does not: the port brings libopus and an echo canceller's C++
in with it, so it is written as two processes — an elevated helper holding only
the memory reader, the injection path, the key hook and the overlay, and an
unelevated process holding tokio, signalling, WebRTC, audio and the GUI.

So: build the server first, and decide there. It is the cheapest phase that
produces a real Rust artefact under real load, and four weeks of it says more
about the true cost of the sixty-odd that follow than any further estimating
will.
If the port continues, the audio engine is next on the critical path, built
standalone and measured against golden vectors captured from the current
Electron build; if it reaches parity, the rest is schedule, and if it does not,
the project stops having spent one phase rather than a year. Both stopping
points leave something worth having: a Rust server, and a hardened 1.x client.

Two things changed after the first draft. The bill is roughly twice the original
estimate — ~74 developer-weeks rather than 37, from work priced too low rather
than scope invented since. And establishing what the port would inherit turned up
problems in the *shipped* 1.0.2 client: an unpinned third-party offsets feed that
drives both the pointer arithmetic and the addresses the injection stub patches,
an update path that can run an unsigned installer with administrator rights, and
a signalling envelope that lets any lobby member read every player's position.
Those are worth fixing whatever happens to the port, and they ship as 1.0.3
through 1.0.5 before and alongside the first port phase.

## Effort

**What the full port costs, not what has been agreed to spend.** The figure
below is the price of everything, kept here because it is the honest one and
because a plan that hides its total is not a plan. It is not a budget: only the
first four rows are funded.

Roughly **74 developer-weeks** to 2.0 for one developer — 77 as this plan first
priced it, less the 3.0 weeks the decisions of 2026-08-24 took out (no
Authenticode, no offsets signing ceremony, no staged envelope rollout). Treat it
as the midpoint of a range whose low end is around 65: it is the union of
independently priced corrections and several of them overlap. Two developers do
not halve it, and the audio engine is no longer alone on the critical path — the
transport phase now rivals it.

```
H1  1.x emergency hardening      2.0   ships as 1.0.3               funded
H2  1.x offsets trust chain      3.0   ships as 1.0.4, ends at G0   funded
H3  1.x/Node envelope + OBS      2.5   1.0.5 + a server release     funded
P0+ Server                       4.0   decision point at its end    funded
--------------------------------------------------------------------------
P1+ Foundations                  5.0
P2+ Game reader                  6.0   G1
P3+ Audio engine                10.0   G2
P4+ Transport                   10.5
P5+ Platform                     6.0
P6+ GUI                         11.5
P7+ Packaging and signing        9.5
P8  Bridge and sunset            4.0   G4 — and a 2.0 release prerequisite
                                ----
                                74.0   of which 11.5 is committed
P9  Post-1.x cleanup             3.0   outside the 2.0 budget
```

> **Where this actually is, 2026-08-26 (evening).** The table is effort, not progress, and
> reading it as progress has been wrong for some days.
>
> `P1+` through `P3+` are built and both their gates are met — G2 in full, and **G1 over
> the 12 574 frames that were recorded**, which is the honest form: five situations were
> never in the corpus and still are not. `TASKS`, `DISCUSSION`, comms sabotage, doors and
> cameras need four people in a real round, and freeplay provably cannot reach them —
> the Electron reader takes all five *inside* `if (state === GameState.TASKS)`.
> `test/recordings/README.md` records the measurement.
>
> `P4+` is **built**, and that is the sentence this note has been unable to write. The
> client joins the lobby the reader reports, offers to newcomers, routes signals through
> `signal_route`, carries Opus over the mesh, and opens a microphone and a speaker at both
> ends — capture, encode, send, order, decode, place by `voice_params`, mix, play.
>
> The echo canceller and the resampler are in it too: `Apm::render` is fed from the output
> callback before every `Apm::capture`, and a device that does not offer 48 kHz is opened at
> its own rate and resampled rather than refused. Measured on a real machine: one buffer
> underrun at start-up, none in the twenty seconds after.
>
> What has never been done is the thing no test here can do: two people, two machines,
> hearing each other.
>
> `P5+` is **built**. The overlay window draws — that was the last piece, and it took a
> sprite protocol to get there.
>
> `P6+` is **built**: all six items. Shell and window state, main view, settings (screen
> and file), lobby browser end to end, overlay view with its seven positions and the
> meeting table, and the GPU fallback chain — which lost a rung to measurement.
>
> `P7+` is built bar its ceremonies: the settings migration, the signed update path, the
> hand-built NSIS installer, the release workflow. It cannot *complete* without a release
> key, which is a ceremony performed offline, and without shipping an ordinary 1.0.x
> release through the new installer — §4.9's own instruction.
>
> `P8`'s mechanism is built: `latest.yml`, the bridge installer that renames rather than
> deletes, the switch-off message. What is left of it is not code — three staged releases,
> a fleet that has to move, and G4 rehearsed on real 1.0.2 installs.
>
> `P9` is three-quarters **already true** and one-quarter blocked, which is not how it
> reads. The data channel was never built, so there is none to drop; SCTP cannot be
> feature-disabled — it is a hard dependency of `rtc` — but is never negotiated, and a test
> asserts the offer contains no `m=application`; and there are no SCTP fuzz targets to
> delete. What is left is moving the lobby settings and the impostor radio claim to the
> socket, and that is blocked for a mechanical reason: 1.x reads both off the *data
> channel*, and the rollout puts both generations in one lobby for weeks. It is not a
> cleanup being deferred; it is a change that would break people still in the lobby.
>
> The weeks are unchanged and are not a record of what anything cost.
> [04-implementation-plan.md](04-implementation-plan.md) §4.11 carries the same statement
> beside the same arithmetic.

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

The two-process split is settled: an elevated helper started on demand with a
per-launch UAC prompt, no Windows service. One decision below it is still open,
and it would move two rows of the risk table from High to Low. The
`i686-pc-windows-msvc` target exists only for the injection path, and it is what
forecloses LiveKit's libwebrtc binding, puts NASM in the build, and creates the
alignment hazard MSVC brings to the struct parsing in `acl-game`. Confining
injection to a 32-bit process — the helper itself, or a third smaller one —
removes all three. It has not been decided either way.

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

09 §6 also carries the maintainer's answers to the questions it raised, recorded
against 2026-08-24. Ten are settled and are written into that document as
decisions with their consequences followed through; five remain open and are
marked as such. Where this page and 09 disagree about a number, 09 is the
arithmetic and this page is the summary of it.

## The five gates

| Gate | After | Criterion | If it fails |
| --- | --- | --- | --- |
| **G0** | 1.x offsets trust chain (H2) | A malicious-bundle corpus is rejected with a distinct error each; on-disk tampering with the cached bundle is rejected as far as the validator can catch it, proving validation at load and not only at download; the validator accepts all 81 real upstream files unchanged; the embedded floor holds with the mirror unreachable, and the client says which bundle it is using; a bundle is published within 6 hours of a real Among Us update | P2+ does not start its offsets work |
| **G1** | Game reader | `AmongUsState` matches the Electron reader exactly on every recorded frame | Bug; fix and retry |
| **G2** | Audio engine | DSP within −80 dBFS of golden vectors; added latency within 30 ms and quality within 0.2 MOS of Chromium under emulated loss and jitter; the receive path recovers Opus in-band FEC from a Chromium sender at 5% loss (the `i686-pc-windows-msvc` build criterion was struck on 2026-08-24 with the target) | **Stop the port** |
| ~~**G3**~~ | Transport | **Struck 2026-08-25.** It asked that a 1.0.2 Electron client and a Rust client hear each other in the same lobby, direct and via TURN; the same call under each impairment profile; and a three-client mixed-generation lobby with one client leaving and rejoining | Was *no staged rollout; reconsider scope*. That is now the standing position rather than a contingency |
| **G4** | Bridge (P8) — and a prerequisite of the 2.0 release itself | Real 1.0.2 installs on Windows x64 update from a staging feed to the bridge, silently. Narrowed from three legs on 2026-08-25: ia32 went with the Windows 11 floor and Linux with its support | **2.0 does not ship.** The 1.x wire format is switched off when it does, so releasing over an unmigrated fleet cuts every 1.x user off on the day |

G0 lost two criteria on 2026-08-24, both belonging to a signature the offsets
bundle no longer carries: the signed-verification criterion and the revocation
drill. What replaced them is a mirror we control, pinned by commit, with a
validator that runs on every load — cheaper, faster in an emergency, and honest
about the fact that whoever can push to the mirror can change what clients read.
09 §2.1 states that residual risk rather than filing it away. It gained one in
exchange: the embedded floor is now the only thing standing between an
unreachable mirror and a client that cannot read the game, so the gate proves it
loads. 05 §5.6 carries the harness for all five.

G4 changed shape rather than criteria. It used to be able to fail into "2.0
stays a parallel install"; with the 1.x wire format switched off at the 2.0
release, that fallback no longer exists, and a G4 that has not passed means the
release waits.

G2 is the one that can end the project, and it is still reached well before the
half-way mark — month eight of eighteen with the hardening track counted, rather
than at the end. That is the entire point of the ordering. Every amendment to G2
and G3 made them harder to pass and none made them easier — and then G3 was
struck outright on 2026-08-25, which is the one change that does make the path
easier. Interop with 1.x is no longer proved before the fleet meets it; see
`04-implementation-plan.md` §4.6 for what that costs. G2 is also the second
decision point, not the first: the first is the end of P0+, and it is a
commercial decision rather than a technical one.
