# 4. Implementation plan

## 4.1 Shape of the plan

Ten phases, plus a hardening track on the Electron client and the Node server
that runs before and alongside the first of them. Each phase ends in something
shippable or a decision. Five points are **gates**: work stops there until an
explicit, measurable criterion is met.

The order is chosen so that the riskiest work happens as early as possible and
the most visible work happens last. That is deliberate and it is the opposite of
what feels natural — the temptation is to start with the GUI, because it is the
part you can show people. Starting with the GUI is how this project fails: you
arrive at the audio engine nine months in, discover the jitter buffer is not good
enough, and have a beautiful shell around nothing.

```
H1  1.x emergency hardening       ──► ships as 1.0.3               2.0 wk
H2  1.x offsets trust chain       ──► G0: offsets trust chain      3.0 wk
H3  1.x/Node envelope & OBS       ──► ships as 1.0.5 + server      2.5 wk
                                                hardening subtotal 7.5 wk

P0+ Server                        ──► ships independently          4.0 wk
════ committed above ════► decision point: does the port proceed? ═══════
P1+ Foundations & toolchain                                        5.0 wk
P2+ Game reader                   ──► G1: parity with Electron     6.0 wk
P3+ Audio engine (offline)        ──► G2: golden-vector parity    10.0 wk
P4+ Transport & signalling        (G3 struck 2026-08-25)         10.5 wk
P5+ Platform layer                                                 6.0 wk
P6+ GUI                                                           11.5 wk
P7+ Packaging, update, rollout    ──► 2.0 build, opt-in            9.5 wk
P8  Bridge & sunset               ──► G4, then the 2.0 release     4.0 wk
                                                    phase subtotal 66.5 wk
                                                            total ≈ 74 wk

P9  Post-1.x cleanup                                               3.0 wk
                                              (outside the 2.0 budget)
```

**Only the hardening track and P0+ are committed.** P0+ ships a Rust server that
serves the existing Electron fleet, and it ends in an explicit decision point:
whether the rest of the port proceeds at all. Everything from P1+ onward is
planned in full and priced in full, and none of it is authorised by this
document. The later phases stay written because a plan that stops at its first
phase cannot say what that phase is buying, and because the ordering — riskiest
first, most visible last — is only defensible when the whole sequence is visible.
Read P1+ through P8 as what it would cost if the answer at the decision point is
yes, not as a schedule anyone is currently working to.

The hardening track does not depend on that answer. H1–H3 ship on the Electron
client and the Node server, they fix defects that are in the field today, and
they are worth doing whether the port continues, stops after P0+, or never
starts.

Phases keep their identifiers; the `+` marks one whose scope grew, and each of
those says below, in a line, what it grew by. Treat 74 as the honest midpoint of
a range whose low end is around 65 — it is the union of independently priced
corrections, several of which overlap, and nobody should want to find out where
the high end is.

Roughly seventeen months of full-time work for one developer; call it two years
with review, testing on real hardware and the inevitable. Phases P2+–P5+ can
still overlap between two developers, but two developers do not halve the total:
P3+ is no longer the sole critical path, P4+ now rivals it, and P7+ can start on
neither until both have landed.

> **Windows only, decided 2026-08-25.** The client's Linux support was removed, and
> the minimum Windows raised to 11, for one reason: nobody on this project can test
> either. Everything below that names Linux, X11, Wayland, an AppImage, a Unix socket
> or a `setcap` step describes work that is no longer in scope, and the individual
> passages carry their own notes. Two consequences are not local to a passage:
>
> **P7 and P8 lose their riskiest Linux item outright.** `electron-updater`'s AppImage
> path unlinks the running AppImage and runs the replacement with `execFileSync` and
> `APPIMAGE_EXIT_AFTER_INSTALL=true`; a Rust AppImage that started its GUI instead of
> exiting would have hung the old client on every Linux machine at once. That failure
> mode is gone rather than mitigated.
>
> **The effort table in §4.11 is not re-estimated here.** Its convention is that
> deleted work comes off the total, and work really is deleted — the Linux reader in
> P2, the Unix-socket half of P5's IPC, `x11.c`, Wayland detection, P6's software
> default, P7's hand-built AppImage and `setcap` tarball, and P8's AppImage handshake
> and its G4 leg. Putting a number on that is a judgement for whoever owns the
> schedule, and inventing one here would be the guessing this plan is written against.
>
> **There are AppImage users.** Every release from 1.0.1 to 1.0.5 published one, with
> a `latest-linux.yml` feed beside it. Those clients keep polling a feed that stops
> moving, and stay on 1.0.5 with no message. Saying so is CHANGELOG's job, and it
> does; it is recorded here because a plan that drops a platform without naming who
> was on it is how that gets missed.

**Where the extra thirty-seven weeks went.** Three items account for most of it,
and none of them is security. P4+ grows by 5.5 because `webrtc` 0.20 is a rewrite
on a sans-IO core, which kills the premise that the 237 lines of `peer.ts` map
onto it one-to-one. P7+ grows by 5.5 because `cargo-dist` builds neither of the
two artefact types this project must keep producing — there is no NSIS backend
and no AppImage backend — so both are hand-built and both must keep a CLI
contract the installed 1.x fleet already depends on. P1+ grows by 4.0 because the
Socket.IO client moves there out of P4, where it was quietly leaving three weeks
for the entire WebRTC half. The rest is spread thin across an owned lobby
registry, a mirrored offsets bundle, a second process, a GPU fallback chain and
two spikes.

**The hardening track.** H1–H3 ship on the Electron client and the Node server.
They are not 1.x maintenance running beside the real work: H2 is a hard
prerequisite for P2+, H1's cross-version single-instance lock has to be in the
field before any 2.x beta build exists, and all three protect the fleet that will
never see 2.x. H1 (2.0 wk) ships as 1.0.3 and is client-local or repository
configuration throughout, so its items can ship in any order and need no server
release. H2 (3.0 wk) ships as 1.0.4 and builds the offsets trust chain in
TypeScript first, because the bundle format must be proven before a Rust consumer
is written against it; it ends at gate G0. H3 (2.5 wk) ships as 1.0.5 plus a
server release and is the only part with a wire component, so it runs consumer
first — the OBS overlay page serves every client generation at once and lives in
neither repository. The per-step content is in
[09-technology-migration.md](09-technology-migration.md) §3.2, which carries the
same decisions: H2 is a mirror rather than a signing ceremony, and H3 ships the
envelope rules enforced with no logging period.

**H2 is a mirror, not a signature.** The offsets bundle is not signed. H2 mirrors
the upstream offsets tree into a repository this project controls, syncs it by a
scheduled pull request so that a human sees the diff before it reaches anyone,
pins upstream **by commit** rather than tracking a branch, embeds a known-good
bundle in the client as a floor, and runs the structural validator on every load
including from the cache. There is no key ceremony, no signing xtask, no second
designated signer and no revocation mechanism.

The reason is availability, and it is worth stating rather than assuming. An
Among Us update is a burst, not a trickle: upstream turned four cycles in a
single evening on 2026-06-06. Anything that puts a human holding an offline key
between that burst and the users is what keeps clients out of the game, and a
client that cannot read the game is indistinguishable from a client that is
broken. A pull-request merge keeps a human in the loop at a cost that can be paid
from a phone; a key ceremony cannot.

**What that leaves open, said plainly.** With no signature, whoever can push to
the mirror can change what every client reads. The mirror's branch protection and
the account that owns it are therefore part of this project's trusted set, and
should be administered like one — protected branch, no force-push, review
required on the sync PR, and the smallest possible set of people holding push.
What the change does close is the larger of the two problems: the client no
longer follows the unpinned branch HEAD of a repository nobody here controls.
A compromise now requires compromising us.

**H2 is 3.0 weeks, not 4.0.** The 1.0 removed is not scope trimmed to make a
number look better: it is the key ceremony and its backup media, the signing
xtask, the second signer's provisioning, the revocation path, and — the largest
single item — roughly a hundred lines of minisign format parsing in
`offsetStore.ts` that had to be byte-compatible with `minisign-verify` and came
with cross-implementation test vectors. What remains is where the value always
was: the mirror and its sync workflow, the bundle format, the embedded floor, the
structural validator, the full-prologue write-side check and the "reset offsets
to embedded" user action.

**H2 delivered 2026-08-24.** All six items are in. The mirror carries a
sync-by-PR workflow that proposes and never pushes, and branch protection on
`main` — required pull request, no force-push, no deletions, linear history,
administrators bound by all of it. `lookup.json` carries `bundle_version`,
`min_client_version` and `upstream_commit`. The client validates structurally on
**every** load including from the cache, falls back to a bundle compiled into the
build and says which one it is using, checks both detour sites before writing to
either, and has a "reset offsets to embedded" button.

Two things came out different from what this section assumed, and both are
recorded where the code is:

- **The bundle's function RVAs are placeholders.** `GameReader` overwrites
  `connectFunc`, `fixedUpdateFunc`, `showModStampFunc` and `modLateUpdateFunc`
  with pattern-scan results before use; the real values in the corpus are 255 to
  4095, far too small to be functions. What a bundle actually steers is the
  *signature*, so the tight bounds are on `patternOffset` and `addressOffset`, and
  the module-range check is on the resolved address where it is produced.
- **Required review is one approval short of what this section asked for.** The
  mirror has exactly one collaborator, so `required_approving_review_count: 1`
  with administrators bound would have made it impossible for the maintainer to
  merge an offsets update at all — the precise availability failure H2 exists to
  avoid. It is set to 0: a pull request is still mandatory for everyone including
  administrators, the diff is visible, and a human clicks merge. Raise it to 1 the
  day a second maintainer exists.

**Gate G0: four of five criteria met, and the fifth is waiting on Among Us rather
than on us.** Criteria 1 to 4 are covered by 134 tests — the malicious-bundle
corpus with a distinct error each, an edited on-disk cache rejected at load, all
44 real offsets files plus the live `lookup.json` accepted unchanged, and the
floor holding with the mirror unreachable while reporting which bundle is in use.
Criterion 5, the timed drill, needs a real Among Us update to drill against; the
procedure is in the mirror's README and the measurement is the interval between
the upstream release and the merged, purged bundle.

> **Gate G0 — the offsets trust chain is live.**
> A committed corpus of bad bundles — malformed, replayed lower `bundle_version`,
> truncated, RVAs out of module range — is rejected with a distinct error each,
> leaving the previously held bundle in force. Editing the cached bundle on disk
> between runs is rejected **as far as the validator reaches**, proving validation
> happens at load and not only at download. The validator accepts all 81 real
> upstream files unchanged — a validator that rejects real data is a
> self-inflicted outage, and that half matters as much as the first. The floor
> holds: with the mirror unreachable — DNS failure, a 404 on the pinned commit,
> an empty cache — the client starts, reads the embedded bundle, and says which
> bundle it is using rather than falling back silently. And a timed
> drill against the next real Among Us update gets a synced, pinned bundle to
> clients in under six hours.
>
> The on-disk criterion has a named limit: without a signature, an edit that
> stays structurally valid is not detectable, and the gate does not pretend
> otherwise. It catches corruption and the obvious classes of tampering, not a
> patient local attacker — who, having write access to the cache, has easier
> targets on that machine anyway.
>
> P2+'s offsets work does not start before G0.

> **Superseded in part, 2026-08-24.** Two of H3's three steps no longer exist.
>
> The **server** step is done: the Node implementation has been deleted from the
> server repository, which is now the Rust server, and that server has enforced the
> envelope rules and first-claimer host since its first commit. There is no Node
> change left to write and no dual-stack window to manage — the two servers cannot
> disagree about what they refuse, because there is only one.
>
> The **OBS overlay** step is not being taken either, but by a different decision:
> the overlay and the mobile relay stay in the Electron client exactly as they are,
> and are simply not built into the Rust client. Neither is migrated. The Rust server
> refuses the shape both use, so both stop working when it is deployed — which is the
> cost §4.2 already records, and it needs no client release to incur.
>
> That empties the ordering table below. It existed to stop a server from enforcing
> ahead of a client that had a working path; there is no new client path to wait for,
> and the paths that exist are ones we are deliberately ending. What is left of H3 is
> deploying the Rust server. The paragraphs below are kept as the record of what was
> decided and why.

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

**H3 enforces from the first release. There is no logging period.** The signal
envelope rules — `to` must be a co-member of the sender's lobby, `to != from`, a
size cap — and first-claimer host go on when the server release ships. No
rejection counter watched for seven days, no 0.1% threshold, no `SIGNAL_STRICT`
flag held back to be flipped later. The counter was never going to reach a floor:
1.x updates through electron-updater with no forcing function, so waiting for the
number to fall is waiting for an event with no cause.

This breaks the OBS overlay feed and the mobile relay for every client older than
1.0.5, at once, by decision. Anyone who never updates loses both permanently and
keeps voice, which is the part they installed the app for. That is the price and
it is being paid deliberately: the alternative is leaving every player's live
coordinates readable by anyone who knows a six-character lobby code, for as long
as the slowest updater takes, which is forever.

Two orderings that were preferences are now hard prerequisites, because with
enforcement on from day one there is no dual-stack window to absorb a mistake:

| Order | Step | Why it cannot move |
|---|---|---|
| 1 | The **OBS overlay page** learns `obs_state`, is switched to `transports: ['websocket']`, and is deployed and verified alone | It lives in neither repository and serves every client version simultaneously. It is also the only browser client, so the polling removal hits it before it hits anyone else |
| 2 | **1.0.5** is in the field | It is the client release that speaks the new events. A server enforcing ahead of it silences clients that had a working path |
| 3 | The **server** release, with the rules already on | Last, and only after 1 and 2 are verified |

H3 is 2.5 weeks, not 3.0: the seven-day watch, the threshold and the flag
plumbing are all gone, and the client and overlay work — which is most of the
phase — is unchanged. The rejection counter itself stays on `/health`, because
it costs nothing and it is how the break is measured after the fact; what went
is the idea that anything waits on it.

**The server is websocket-only, and the mobile promise is dropped.** Engine.IO
polling is removed rather than deprecated, and `03-target-architecture.md` §3.5's
undertaking that a future 4.x mobile client keeps working is deleted with it, not
softened. Mobile `socket.io-client` defaults to `["polling","websocket"]`, so a
websocket-only server refuses its handshake; the two could not both hold and this
is which one gives way. Both shipping clients already pass
`transports: ['websocket']`, so nothing in the field regresses, and dropping
polling also drops the advisory it carried
([GHSA-r635-g3xr-vw7x](https://github.com/advisories/GHSA-r635-g3xr-vw7x), HIGH)
along with base64 binary framing and the probe-and-upgrade handshake. The one
client that does *not* already pass it is the OBS overlay page, which is why the
transport change rides with step 1 above rather than with the server.

## 4.2 Phase 0 — Server (4 weeks)

Ships on its own. Proves the toolchain, CI and release story on the smallest
piece of the system.

**Why 4 and not 2:** the lobby registry is owned rather than borrowed from
socketioxide rooms, the two HTTP endpoints the client will want are new code
rather than a port, and the signal envelope rules are enforced from the first
commit instead of arriving later as hardening.

1. `server-rs` crate: `axum` 0.8 + `socketioxide` 0.18 + `tower-http` 0.7 +
   `tokio` 1.53. None of socketioxide's features are on by default; name
   `tracing`, `extensions` and `state` explicitly, or its rejected-payload paths
   log nothing at all. `tower` alone ships no HTTP-aware middleware — the body
   cap, the request-body timeout, the panic catch and CORS are all in tower-http.
2. Port the eleven socket events from `src/index.ts`, keeping the two bug fixes
   named in §3.4. Own the lobby registry rather than leaning on socketioxide
   rooms: rooms hold only socketioxide sockets and cannot answer "is socket X in
   the lobby socket Y is in", which is the predicate the envelope rules need.
   Three acceptance criteria go in writing before the first line of `lobby.rs`,
   because a faithful-port instinct will otherwise undo them — a bounded
   per-member channel with a logged overflow policy and a counter on `/health`,
   serialisation once per encoding rather than once per member, and no lock held
   across an await.
3. Port `/`, `/health` and `/lobbies`. No template engine: `/health` and
   `/lobbies` are `serde_json`, and the one status page is a string literal.
4. Port the peer-config parsing and relay advertisement. Move the file itself to
   TOML or JSON — the YAML crate the plan would otherwise use depends on
   `unsafe-libyaml`, an archived machine translation of C, and the file is a
   handful of url/username/credential fields.
5. `GET /lobbies/{id}/code` with `Cache-Control: no-store` — a lobby code is the
   credential that gates entry to a game — and `GET /lobbies/stream` as SSE with
   a 15–30 s heartbeat comment and `Last-Event-ID`, or every reverse-proxied
   deployment cuts the stream at nginx's 60 s default and the list goes silently
   stale.
6. Websocket transport only, and therefore **no CORS layer on the socket.io
   route**. That requirement existed for the polling handshake, and polling is
   gone: a browser's WebSocket upgrade is not a CORS request and needs no
   server-side permission, so the OBS overlay page connects from its own origin
   without one. Do not replace it with an `Origin` allow-list either — it
   restricts only browsers, and the only browser that matters is ours, so it buys
   nothing against a non-browser client that sets any `Origin` it likes while
   being the one thing that can take the overlay off the air. CORS stays only on
   the HTTP endpoints a browser actually fetches with XHR (`/lobbies`,
   `/lobbies/stream`).
7. Multi-stage `Dockerfile` on `rust:1.98-alpine` → `alpine:3.22`, non-root, no
   shell in the final image. TLS terminates at a reverse proxy and axum binds to
   localhost, which keeps a crypto stack out of the server binary entirely.
   Configuration comes from the environment — systemd `EnvironmentFile=` or
   docker `--env-file` plus `std::env::var` — with no dotfile loader crate in
   between.
8. Port `test/lobby.test.ts` to a Rust integration test that drives a real
   `socket.io-client` from Node against the Rust server, so the wire format is
   verified against the reference implementation and not against itself. That
   client passes `transports: ['websocket']`, as both shipping clients do; one
   test case deliberately omits it and asserts the handshake is refused, so the
   thing §3.5 gave up is a fact this suite states rather than an assumption.

The envelope rules are written into this server from birth and **on**, because by
the time P0+ ships H3 has already switched them on in the Node server this one
replaces. That is what makes the replacement a like-for-like swap: the two
servers agree about which messages they refuse, so a rollback from Rust to Node
is a deployment change and not a behaviour change. It also means P0+ inherits
H3's ordering — the Rust server must not reach production before 1.0.5 and the
migrated overlay page are in the field.

**Accepted risk, recorded here rather than closed.** socketioxide never applies
`max_payload` to the WebSocket transport — it governs the outbound `emit()` and
the handshake advertisement, both of which a hostile client ignores — so the one
transport this server offers is bounded only by tungstenite's defaults, 64 MiB
per message and 16 MiB per frame. That is a bound, not an absence of one: the
exposure is a client forcing a 64 MiB allocation where the configured cap says
64 KB, a factor of a thousand, and not unbounded memory. Our per-event size check
runs after the frame is decoded, so it refuses the payload but does not prevent
the allocation. This cannot be fixed at a reverse proxy, because neither nginx
nor Caddy has a post-upgrade frame directive; the routes are an upstream change
exposing `WebSocketConfig::max_message_size`, or a fork. That upstream change is
already open as [socketioxide#762](https://github.com/Totodore/socketioxide/pull/762),
so note the exposure here and do not file a duplicate; do not write it down as a
configuration line either.

**Done when:** the existing 1.0.2 Electron client connects to the Rust server,
joins a lobby, exchanges signalling, and the lobby browser populates — with no
client change whatsoever.

**Delivered 2026-08-24**, in `AnotherCrewLink-Server`. It shipped as `server-rs/`
alongside the Node server and, once it was proven, replaced it: the Node
implementation is deleted and the crate now sits at the repository root. The
abbreviation throughout is `acl` — binary, systemd unit, service account, log
target.
The acceptance criterion was met with two unmodified installed 1.0.3 clients rather
than one: both reached `ONSTREAM`, `/health` reported `connectionCount: 2` and
`lobbiesCount: 1`. Items 1–8 are all in, and against the estimate above the phase
took days rather than four weeks — that ratio is the number the decision point asks
for, and it is worth reading with the caveat that this was the smallest piece by
design and that the four-week figure was drawn for a human working week.

What the code does that this section did not anticipate:

- The lobby registry is owned as planned, and the three acceptance criteria in
  item 2 are enforced: a 128-slot per-member channel whose overflow increments
  `droppedFullBuffer`, one `serde_json::value::RawValue` rendered per event and
  shared by every recipient, and no lock held across an await. `/health` reports
  four counters — `droppedFullBuffer`, `refusedSignals`, `refusedOversize`,
  `refusedMalformed` — where the Node server reported none.
- The host of a lobby is the first socket to claim it, held until that socket
  leaves. §3.4 named the two bug fixes; this is a third, found while porting: the
  Node server let a later socket assert the role and take authority over lobby
  settings from whoever already held it.
- The wire test drives the reference `socket.io-client` across 22 checks, one of
  which is item 8's deliberate omission of `transports: ['websocket']`.
- Operational artefacts beyond item 7: a shell-less `HEALTHCHECK` built from a
  second static binary (`docker/healthcheck.rs`) because the final image has no
  curl to call `/health` with, a 19.8 MB image that stops in 581 ms, a systemd
  unit under `deploy/`, `deny.toml` with a dated allow-list for duplicate majors,
  and `.github/workflows/rust.yml` with a path filter mirrored into `build.yml`
  so neither server's workflow runs for the other's changes.

**The upstream issue this phase asks for should not be filed.** Searching before
writing it found the fix already open as
[socketioxide#762](https://github.com/Totodore/socketioxide/pull/762) — the same
diagnosis, the same three lines, with tests, blocked since 2026-07-18 on a
maintainer asking for separate frame-size and message-size options rather than
reusing `max_payload`. A duplicate issue would cost the maintainer time and add
nothing. `socketioxide-frame-cap-upstream.md` records what was checked and puts
the real choice — wait, or finish #762's requested changes — where it can be
decided instead of drifting. Everything else in P0+ is done and verified.

> **Decision point — does the rest of the port proceed?**
> P0+ is the last committed phase. It is deliberately the smallest useful piece
> of the system, and it is chosen so that the answer here is informed by evidence
> instead of by this document's estimates: after it, the toolchain, CI, the
> release story and the cost of writing and operating a Rust component of this
> project are all measured rather than projected. Compare the four weeks it
> actually took against the four weeks written here, and scale accordingly —
> that ratio is the single most useful number the project will have.
>
> It is also the last cheap place to stop. P1+ spends five weeks on foundations
> that are worth nothing on their own, and from there the next thing that reaches
> a user is the 2.0 build, fifty-eight and a half weeks later. A decision
> deferred past this point is not deferred, it is made.
>
> A "no" leaves a Rust server serving the Electron fleet, the hardening track
> completed, and nothing abandoned mid-flight. A "yes" commits the remaining
> ~62.5 weeks, subject to G2 still being able to end it on technical grounds.

## 4.3 Phase 1 — Foundations (5 weeks)

**Why 5 and not 1:** the Socket.IO client is built here rather than in P4, where
it was crowding out the WebRTC half of that phase, and two cheap experiments that
de-risk the two most expensive phases are run here rather than discovered later.

1. Workspace, `rust-toolchain.toml` pinned to **1.98.0**, `edition = "2024"`.
2. Workspace lints: `unsafe_op_in_unsafe_fn = "deny"`, `clippy::pedantic` at
   warn, `missing_docs` on public crates. In `.cargo/config.toml`,
   `-C control-flow-guard=yes` on both Windows targets and `-C link-arg=/CETCOMPAT`
   on x86_64 — both are off by default on stable, both are free, and Chromium
   already ships with the first.
3. `cargo-deny` (`advisories`, `bans`, `licenses`, `sources`) and `cargo-vet`
   wired into CI as blocking, with two honest qualifications written into the
   policy rather than discovered by a red build. The rule against duplicate major
   versions is unsatisfiable against this dependency set — the RustCrypto
   ecosystem is mid-migration and `rtc-dtls` alone declares two majors of `rand`
   side by side — so it becomes a warning plus a dated, reviewed allow-list; a
   gate that fails on day one gets switched off in week one. And cargo-vet begins
   with a large exemptions block, because Mozilla's, Google's and the Bytecode
   Alliance's shared audit sets contain **no** audits at all for the crates that
   matter here: `cpal`, `opus`, `neteq`, `sonora`, `rubato`, `ringbuf`, `webrtc`,
   `tokio-tungstenite`, `windows-sys`, `x11rb`, `eframe`, `egui`, `winit` and the
   update crate among them. Say that, rather than letting the supply-chain table
   imply coverage that does not exist. Add `cargo-about` for the third-party
   attribution file GPL distribution wants, and `cargo-auditable` so
   `cargo audit bin` works on a shipped artefact months later.
4. `acl-types`: port `src/common` wholesale, including the collider tables.
   Port `ColliderMap.test.ts` — it already exists and gives a free parity check
   on day one.
5. CI skeleton: `fmt`, `clippy -D warnings`, `test`, `deny`, on Windows x64,
   Windows i686 and Linux x64.
6. **The Socket.IO client**, hand-written against Engine.IO v4 / Socket.IO v5 on
   `tokio-tungstenite` 0.30.0, websocket transport only, default namespace, no
   binary attachments. `rust_socketio` is not used at all, not even for one
   release: it pulls `backoff` (RUSTSEC-2025-0012) and `instant`
   (RUSTSEC-2024-0384), both unmaintained with no fixed version, so CI would be
   red from the first commit that added it. Both existing clients already pass
   `transports: ['websocket']`, which deletes polling, the upgrade handshake and
   base64 binary framing from the surface — and after H3 the server offers
   nothing else, so this is the contract on both sides rather than a client-side
   simplification that a server change could invalidate. Conformance-test it
   against the Node server P0+ has just proven, and name the five failure modes
   as explicit tests, because they are how hand-written v4 clients fail: the
   server sends `ping` and the client replies `pong`, reversed from v3;
   `pingInterval`, `pingTimeout` and `maxPayload` come from the OPEN packet
   rather than being hard-coded; the Socket.IO `sid` in the CONNECT ack is not
   the Engine.IO `sid`; ack ids leak if the server never acks `join_lobby`; and a
   `CONNECT_ERROR` must be distinguishable from a transport close, so an auth
   rejection does not drive the reconnect policy. `reconnectPolicy.ts` comes across with its tests
   unchanged — it is 34 lines of pure functions with no transport coupling, and
   the transport's only obligation is to report "closed" honestly. Budget
   `rustls-platform-verifier` and a system proxy resolver as named line items on
   all three targets: Chromium supplied WPAD/PAC resolution and the Windows
   certificate store for free, tokio-tungstenite supplies neither, and the
   symptom for a user behind a TLS-inspecting corporate proxy is "won't connect
   at all".
7. The localisation loader: under 100 lines over the 37 existing i18next JSON
   directories, flattening the dotted keys once at startup into a map behind
   `fn t(&self, key: &str) -> &str`, with the English fallback chain and each
   locale's base text direction. See §4.8 for why there is no conversion.
8. The IPC transport trait for the two-process split (§4.7), so P3+ and P4+ build
   against the boundary instead of being retrofitted into it later.
9. **Two experiments, both hours, both brutal to discover in month nine.** A
   transparent click-through window on Windows x64, Windows i686 and Linux —
   eframe's transparency has renderer-specific failures and the known workarounds
   are mutually exclusive with a single-process design, so this must be answered
   before the GUI phase is planned around it, not during it. And a
   `cargo build --target i686-pc-windows-msvc` of the chosen echo canceller,
   which is a G2 criterion and the one thing that decides whether the audio phase
   has an APM at all.

**P1+ delivered 2026-08-24.** All nine items are in, in the client repository:
`Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, `deny.toml`,
`supply-chain/`, `about.toml`, `crates/acl-types`, `crates/acl-net`,
`crates/acl-i18n`, `crates/acl-ipc`, `experiments/` and
`.github/workflows/rust.yml`. 58 tests, clippy silent under `-D warnings` with
`pedantic` on, and `cargo deny` green on all four checks.

**Both experiments came back positive, and both were worth running.**

- A transparent, click-through, always-on-top window exists on `x86_64` and
  `i686` Windows with identical extended styles — `WS_EX_LAYERED`,
  `WS_EX_TRANSPARENT`, `WS_EX_TOPMOST`. The GUI phase can be planned around a
  single-process overlay after all. The Linux leg runs in CI under llvmpipe.
- `sonora` builds for `i686-pc-windows-msvc` and its own suite passes there: 515
  tests in debug, 713 in release. That closes gate G2's precondition (a), which
  this document called genuinely unproven, and it is the release number that
  counts because that is where the SIMD paths are taken. G2's other two
  preconditions are untouched — the A/B echo-return-loss measurement needs real
  captures, and sonora's bus factor of one is not a thing a test run changes.

**Six corrections this phase made to the document.**

1. `eframe ^0.61.2` is not a version that exists. The current release is 0.36.1,
   and its `App` trait has split `update` into `logic` and `ui` since whatever
   this figure was taken from.
2. The overlay probe's first result was a false negative — all three style bits
   clear — because it read `GetForegroundWindow()`. A click-through window with
   no taskbar button never becomes the foreground window, so it was reporting the
   console's styles. A probe that asks the OS a question must be sure it is asking
   about the right object.
3. §4.8's "not one interpolation placeholder across 4,631 strings" is now off by
   one, by H2's hand: `reset_offsets_done` carries `{{version}}`. The loader
   carries the smallest possible substitution and nothing more.
4. The bundle's function RVAs are placeholders — already recorded under H2, and it
   is what decided where `acl-types`'s bounds went.
5. `postcard`'s default feature pulls `heapless` 0.7 and through it
   `atomic-polyfill`, unmaintained under RUSTSEC-2023-0089. Neither side of the
   IPC boundary is `no_std`, so the feature is off. The dependency gate caught
   that on its first real dependency, which is the argument for having it.
6. `acl-i18n` is a crate the architecture tree did not have. See
   [03-target-architecture.md](03-target-architecture.md) for why it is not part
   of `acl-types`.

**What cargo-vet actually buys, measured rather than claimed.** With Mozilla's,
Google's and the Bytecode Alliance's shared sets imported: 45 of 346 crates
covered, 301 exemptions. `sonora`, `eframe`, `egui`, `winit`, `x11rb`,
`windows-sys`, `kurbo`, `serde_json`, `tokio-tungstenite` and `webpki-roots` have
no audit in any shared set; `postcard` does. The count moved twice during this
phase — 283, then 316 when the transport landed, then 346 when the overlay probe
named eframe's platform features, then 360 when the offsets store brought in
`ureq` and `rustls-platform-verifier` (45 covered, 315 exemptions) — and the gate
failed each time until the new crates were written down. The last of those also
failed `cargo-deny`, on `webpki-root-certs`: Mozilla's CA data ships under
CDLA-Permissive-2.0, which is a licence for data rather than code and is allowed
in `deny.toml` with that reasoning written beside it. §8 asked that this be said plainly instead of letting a
supply-chain table imply coverage that does not exist, and `supply-chain/README.md`
says it.

**Item 6's conformance test found two bugs, both mine.** It asserted the two
session ids differ — true of the Node implementation, false of socketioxide,
which reuses one value. A client that wrongly addressed itself by the transport id
would therefore pass against our server and break against a Node one. And it
called `join` with three arguments where the server deserialises four into a
tuple, counts a short one as malformed and disconnects. Two implementations
meeting in the middle is the only arrangement where either would have shown up.

**Still open from item 6, and named rather than absorbed:** certificate
verification uses webpki's bundled roots rather than the platform store, and there
is no proxy resolution. Chromium supplied both for free. They are budgeted here as
`P4` line items and the symptom of not having them — a user behind a
TLS-inspecting corporate proxy cannot connect at all — is written into
`transport.rs` where someone will meet it.

## 4.4 Phase 2 — Game reader (6 weeks) → **Gate G1**

**Why 6 and not 4:** the reader consumes a mirrored, commit-pinned offsets bundle
and validates it structurally on every load, instead of trusting whatever an
unpinned branch of a third-party repository returned. Dropping the signature (H2)
takes a crate call out of this phase, not a phase item — the validator, the
malicious-bundle corpus and the full-prologue write-side check are where the two
extra weeks live, and all three survive unchanged. P2+ stays at 6.0.

1. `ProcessMemory` trait and the Windows implementation. `OpenProcess` requests
   `PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION` and nothing more.
   `PROCESS_VM_WRITE | PROCESS_VM_OPERATION` were to go behind `--features
   injection`; item 6 was dropped instead, so there is no feature and no write
   right at all, and `PROCESS_CREATE_THREAD` is never requested. This was written
   as the cheapest security improvement in the port, and it turned out to be
   cheaper still: `native/memoryjs` opened the game with `PROCESS_ALL_ACCESS`
   until 2026-08-24 and now asks for the same two rights this line does, so the
   improvement landed in 1.x rather than waiting for 2.0. **Delivered.**
   Process enumeration is roughly 25 lines of Toolhelp32 — a direct transliteration
   of code already in `native/`, and since 2026-08-25 there is no `/proc` scan
   beside it — rather than a crate that costs 25 dependencies and drags `winapi` 0.3.9 in with it,
   and dropping that crate is also what lets the project's own Win32 move from
   `windows` 0.62.2 to `windows-sys` 0.61.2. Find the PID once and keep the
   handle; re-scan only on read failure.
2. Pattern scanner, pointer-chain resolver, .NET dictionary/array walkers. On
   `i686-pc-windows-msvc`, native code may align 8-byte types to 4, so
   zerocopy's reference APIs are banned on any struct containing a 64-bit field —
   a `clippy::disallowed-method` entry, and `read_from_bytes`, which copies, at
   the call sites. Tens of bytes at 30 Hz; the copy costs nothing.
3. Offsets bundle consumer: re-run the structural validator on **every** load,
   including from cache, and check the bundle against the floor embedded at build
   time; keep the cache, the two-host retry with backoff and the request timeout
   added in 1.0.1. The bundle carries no signature, so the validator and the
   embedded floor are the whole of the check and neither may be skipped for a
   cached file — a cache hit is exactly the path that would otherwise never be
   examined again. The three GETs this client makes — offsets, hats,
   update check — go through `ureq` 3.4.0 with the `platform-verifier` feature,
   driven from `spawn_blocking`, which is a feature and not a compromise: an
   update check then cannot stall the runtime the voice path shares. On an
   unknown game build, fall back read-only with a
   log line rather than silently to `lookup.versions.default`, and raise a banner
   only on self-test failure — many Among Us builds ship without moving offsets,
   so a "not supported" banner would cry wolf. The format is the one G0 proved,
   so this is a consumer, not a design.
4. Mod detection, VDF parsing, avatar recolouring.
5. ~~The Linux implementation.~~ **Struck 2026-08-25** with the client's Linux
   support; `acl-game::linux` was written and is deleted. The two warnings it
   carried outlived it in a useful form: the trait method is `read_exact` because
   the C zero-filled a short read silently, and that rule now applies to every
   reader. Original text follows.

   `nix::sys::uio::process_vm_readv` is a safe `fn`
   whose lengths derive from the slices passed in, so the Linux reader contains
   no `unsafe` at all. Two things not to port: the C code's response to a short
   read is to zero-fill the buffer silently, so the trait method is `read_exact`
   and a short read is an error; and Yama `ptrace_scope=1` is the Ubuntu and
   Debian default and blocks reading a non-descendant process, which no crate
   choice fixes and which the packaging phase has to document.
6. ~~Injection module, 32-bit Windows, feature-gated.~~ **Dropped 2026-08-24, and
   removed from the Electron client with it.** The full-replayed-prologue check
   and the "already patched by us" third state were both real requirements — the
   instruction at +4 straddles the five-byte boundary, and the initialisation path
   can re-run against a live patched process — but they were requirements of a
   feature that drew a version stamp in the menu corner and nothing else. The
   note below §4.4's item list prices it. Nothing replaced this item; the phase is
   six items.
7. A `FuzzProcess` implementation of `ProcessMemory`, backed by arbitrary bytes
   answering from a sparse map, so `AmongUsState::read_from` is fuzzable for
   almost nothing on top of the trait that already exists. Two hazards are
   reachable today from a modded or corrupted game process — a self-referential
   pointer chain, and an attacker-influenced array length used to size a `Vec` —
   so cap chain depth and element counts. For this to find anything the parsing
   layer must stay pure: `&dyn ProcessMemory` in, `Result` out, no `unwrap`, no
   `as` truncation.

> **Measured 2026-08-24, and it changes the shape of this decision.** The
> injection path exists for two features, and one of them is already switched off.
>
> The `FixedUpdate` detour is there so the lobby browser can make the game join a
> lobby without the player typing the code. That path is dead:
> `GameReader.joinGame` has its parameters marked unused and its body entirely
> commented out, and the only `JOIN_LOBBY` sender in the renderer
> (`LobbyBrowser.tsx:244`) is commented out too. The shellcode is written and its
> flag is never set.
>
> What still runs is `fixPingMessage`, which enables an icon and replaces the
> game's ping string with `AnotherCrewLink v1.0.3 / aucl.greluc.me / Ping: {0}ms`.
>
> So today the client allocates an executable page in another process, overwrites
> the first five bytes of two functions with jumps into hand-assembled x86, and
> replaces a string pointer — to put a version number and a URL in the corner of
> the game's menu. Nothing about voice depends on any of it, and none of it happens
> on 64-bit builds at all.
>
> That does not decide anything by itself, but it prices it honestly. The `i686`
> target, the NASM requirement, the alignment hazard item 2 lints around, the
> `PROCESS_VM_WRITE` right, and the foreclosure of LiveKit's `libwebrtc` binding
> are all bought by a branding stamp.

> **Decided 2026-08-24: removed, not split.** The measurement above priced it, and
> the answer was that a branding stamp does not buy an `i686` target. The
> injection module, the `injection` feature, the `PROCESS_VM_WRITE` right and the
> 32-bit target are all gone, from this port and from the 1.x client — where
> `native/memoryjs` now opens the game with `PROCESS_VM_READ |
> PROCESS_QUERY_LIMITED_INFORMATION` instead of `PROCESS_ALL_ACCESS`.
>
> Verified against a real process with the rebuilt module: reading still works and
> a write to the same address leaves the bytes unchanged. `memoryjs` never noticed
> the refusal, because `writeBuffer` ignores `WriteProcessMemory`'s return value —
> which is why the write declarations came out of the type definitions too.
>
> What this buys, beyond the risk table: no NASM on the build machine, no MSVC
> 4-byte alignment hazard in struct parsing, and LiveKit's `libwebrtc` binding is
> live again for `P3+` — AEC3, Opus with FEC, RTP/RTCP and NetEQ as one dependency
> instead of five separate crates with a bus factor of one between them.
>
> What it does **not** change is elevation, which never depended on writing. A
> same-user game needs none; a game running at a higher integrity level cannot be
> opened at all, whatever rights are asked for, and takes push-to-talk with it
> through UIPI. The `runas` fallback in `03-target-architecture.md` §3.2 stays, and
> is now the only thing standing behind that one configuration.
>
> The paragraph below is kept as the record of what the decision was between.

**An explicit open decision: split the injection path into its own 32-bit
process.** The `i686-pc-windows-msvc` target exists for nothing but item 6, and
it is the largest available lever on this project's risk profile. That one target
is what forecloses LiveKit's `libwebrtc` binding — the integrated option that
would supply AEC3, Opus with FEC, RTP/RTCP and NetEQ as a single dependency, and
whose build script has no 32-bit x86 path at all. It is what puts a NASM
requirement on the build machine as soon as a crypto stack that ships prebuilt
objects for x86-64 only enters the tree. And it is where MSVC's 4-byte alignment
of larger types creates the unsoundness hazard item 2 has to lint around, in
exactly the struct-parsing code this crate is made of. A small 32-bit helper
talking to a 64-bit client moves rows 13 and 14 of the risk table from High to
Low and makes the integrated audio option live again. It is not scheduled here
because it is not decided; it should be decided rather than defaulted into. It is
cheaper than it was when it was first written down: the elevated helper is
settled (§4.7) and P1+ already builds the IPC transport it would talk over, so a
32-bit injection helper is another endpoint on an existing boundary rather than a
new mechanism to invent.

**Recording harness.** Before writing the reader, add a debug command to the
*existing Electron build* that dumps, once per frame, the raw bytes of every
region `GameReader` touches plus the `AmongUsState` it produced. Record a session
per map (Skeld, Mira, Polus, Airship, Fungle) covering lobby, tasks, meeting,
vents, cameras, sabotage, and deaths. Those recordings become `ReplayProcess`
fixtures.

> **Deferred 2026-08-25, with an owner and a trigger.** The recording session happens
> when the maintainer can get a play group together again. That is a commitment with a
> precondition rather than an open question, and it is the difference between a gate
> waiting on somebody and a gate waiting on nobody — but the gate itself is unchanged, so
> P2 is not complete and nothing downstream may assume the reader has been proved.
>
> Nothing here is blocked on it. The parity harness is built and idle; when the corpus
> arrives the test stops skipping, and either it passes or it names the field it differs
> on.
>
> **Status, corrected 2026-08-25: the corpus is not empty and the gate runs.**
> This paragraph said "the corpus is empty" and was left behind when the first
> recordings landed on 2026-08-24. `test/recordings/` holds three sessions — a
> menu, 64-bit freeplay, and a real nine-player lobby on 32-bit — and
> `cargo test -p acl-game --test parity` reports **4653 frames, no differences**.
> Getting there took eleven fixes to the Rust reader that nothing but real frames
> could have found.
>
> What is missing is narrower than "a corpus": **a live online round**. Freeplay
> holds the raw game state at 1 or 3 throughout, so every frame in it derives to
> `LOBBY` — the meeting-hud branch is unreachable, and so is everything guarded by
> `state === TASKS`. Cameras, doors and comms sabotage are never read on either
> side, and the gate passes over them because both readers skip them identically,
> which says nothing about whether either is right. `isDead` and `lightRadius`
> under a lights sabotage are in the same position.
>
> So G1 is met for the states the corpus reaches and open for the rest. It is
> tracked as [issue #10](https://github.com/greluc/AnotherCrewLink/issues/10) and
> needs somebody to play an online game with `ACL_RECORD` set;
> `test/recordings/README.md` has the procedure. A fixture written by hand would
> only prove the two implementations share an author's assumptions, which is the
> one thing this gate is not for.

> **Extended 2026-08-26.** A fourth session took the corpus from 4653 frames to **12574**
> and the gate broke on 23 of them, which is what a corpus is for: a branch neither reader
> reaches compares equal on both sides, so the only way to find a divergence is to reach
> it. Four fixes, all in the Rust reader — the menu hold that keeps reporting a menu until
> the game has rebuilt its player table, the 9999 sentinel for a player the reader cannot
> make sense of, a player dropped for having a null object pointer, and two fields that
> are `undefined` on the Electron side and therefore cannot be a `u32` and a `bool` here.
>
> Two things it reached that were not expected to be reachable without a round, and both
> only because they were tried:
>
> * **32-bit in a game.** Pointer width changes the player-array, door-list and dictionary
>   strides, and every in-game recording before this was 64-bit.
> * **Every map.** From an online lobby's *settings*, which is the only route: the reader
>   takes the map from the game options, and freeplay does not write its map there. A whole
>   freeplay session on Polus arrives labelled `THE_SKELD` — the reader is not wrong, the
>   field really does say Skeld. `maxPlayers` is the same object and the same route, and
>   three abandoned lobbies at 15, 10 and 8 is what gave it three values.
>
> **And one thing that is now known to be impossible rather than merely awkward.** The
> paragraph above says freeplay cannot reach the `TASKS` branch. It is stronger than that:
> comms, doors and cameras are all read *inside* `if (state === GameState.TASKS)`, so
> sabotaging in freeplay changes nothing in a recording, because nothing looks. Issue #10
> cannot be closed by a more determined solo session; it needs four players.

> **Gate G1 — parity of the reader.**
> For every recorded frame, the Rust reader's `AmongUsState` must equal the
> Electron reader's, field for field, with float positions within 1e-6.
> Non-negotiable: this is a lossless, purely mechanical transformation, so
> anything less than exact means a bug, not a tolerance.
>
> Unchanged by the hardening track, with one addition: G1 must pass byte-for-byte
> using the embedded bundle, which proves the bundle format lost nothing that
> `lookup.json` carried.

## 4.5 Phase 3 — Audio engine (10 weeks) → **Gate G2**

> **Status, 2026-08-25.** Every item is built, `crates/acl-audio` carries 244 tests, and
> every gate criterion that has not been struck is met.
>
> Closing it changed two things it was supposed only to measure: the jitter buffer's depth
> had to become adaptive, and the FEC controller turned out to have been doing nothing at
> all. Both were found by comparing against Chromium rather than against the plan.
>
> | Item | State |
> | --- | --- |
> | 3a DSP graph | done — every node within −80 dBFS of Chromium's own output |
> | 3b `voice_params` | done — 1035 recorded tuples, no difference |
> | 3c Capture and codec | done — `stream::choose` decides what to ask a device for, with tests; the `cpal` layer over it is translation only |
> | 3d Jitter buffer and playback | done — the buffer adapts its depth, which measuring against Chromium forced; `NetEq` bridge, mixer, output selection |
> | 3e FEC feedback loop | done both directions, and the loop was found to have been achieving nothing until `Signal::Voice` was set; less the `ReceiverReportInterceptor` call that would pick P4's transport crate by accident |
>
> | Gate G2 | State |
> | --- | --- |
> | 1. DSP against golden vectors | **met** |
> | 2. `voice_params` parity | **met** |
> | 3. Latency and quality against Chromium | **met** — every profile within the 30 ms budget, and less invented audio than Chromium under loss |
> | 4. Zero allocations on the render callback | **met**, and it moved the APM off the capture callback to stay met |
> | 5. FEC recovery with a Chromium sender | **met** — all four legs, against Chromium's own encoder and its own receiver |
> | 6. `i686` build | struck; the target no longer exists |
>
> **Criteria 3 and 5 were parked behind P4 and did not belong there.** Both were read
> as needing a Chromium peer across a network. Neither does:
>
> - Chromium's *encoder* is reachable from a page through WebCodecs, so criterion 5's
>   receiving half is measurable now. It put redundancy in 862 of 1001 packets, and at
>   5% loss the receive path recovers 39 frames where a control with the redundancy
>   removed recovers none.
> - Chromium's *receive path* is reachable through a loopback peer connection, and an
>   encoded transform is a place to drop frames before they reach it. That is NetEQ, its
>   delay manager and its concealment, under the same profiles as ours.
>
> | | ours | Chromium |
> | --- | --- | --- |
> | latency, every profile | 40 ms | 10–30 ms |
> | worst difference | +30.0 ms | budget 30 ms |
> | invented audio, 10% loss | 3.7% | 8.8% |
> | invented audio, clean | 2.0% | 0.0% |
>
> **Measuring it changed the design.** A fixed depth cannot meet the criterion: 40 ms is
> within budget on a clean network and invents 17% of frames under 50 ms of jitter, and
> 60 ms survives the jitter and is 50 ms adrift on a clean one. The buffer now moves — it
> deepens when a packet arrives after its slot has played, which is the only observation
> that means "too shallow", and regains depth by inserting one concealment frame so the
> stream falls further behind the network. An earlier version deepened on every gap and
> grew to 185 ms under 10% loss, buying nothing: a packet that never arrives is loss, and
> no depth recovers it.
>
> **Criterion 5, leg by leg.** Its observable is `fecPacketsSent` climbing in both
> directions, which is a counter meaning "this encoder emitted redundancy".
> `opus_packet_has_lbrr` answers the same question about the same bytes, and answers it
> about *these* packets rather than about a total:
>
> | | verified | how |
> | --- | --- | --- |
> | Chromium emits redundancy | yes | 862 of 1001 packets, inspected |
> | our receiver recovers it | yes | 39 frames at 5% loss; 0 with the redundancy stripped |
> | we emit redundancy | yes | 171 of 200 packets, told mid-call |
> | a Chromium receiver recovers ours | yes | it conceals 1.43% of our stream against 4.42% of the same audio without redundancy |
>
> The fourth had been parked behind P4 on the grounds that Chromium has to *receive* our
> stream and nothing exists to carry it. Something does: an encoded transform can **replace**
> a frame's payload, not only drop it. So Chromium packetises our Opus, sends it to itself,
> and its own receive path — NetEQ, libopus, its FEC recovery — decodes it.
>
> The loss goes in on the **receiving** side, after depacketisation. Injected before
> packetisation the sequence numbers close up and nothing downstream learns a frame is
> missing, which is why `scripts/receive-reference` reports `fecPacketsSent` as zero and why
> a WebRTC field trial for a simulated lossy network changed nothing on a loopback — 504
> packets sent, 504 received.
>
> `fecPacketsSent` itself is still zero and is not the thing to chase. It is a counter
> meaning "this encoder emitted redundancy", and that question is answered directly, about
> the actual bytes, by `opus_packet_has_lbrr` — for both encoders.

The phase that decides the project. No UI, no network — a library plus a
command-line harness that reads WAV in and writes WAV out.

**Why 10 and not 8:** in-band FEC is not a flag. Making it work costs a feedback
loop in both directions, and it is new work the flag-only design did not carry.

### 3a. DSP graph (3 wk)

Implement `Panner`, `Biquad`, `Convolver`, `Gain`, `Analyser` against the Web
Audio specification formulas. One golden-vector test per node.

Four of the five are formulas. The convolver is not: it is uniformly partitioned
FFT convolution with correct overlap-add accumulation, correct latency alignment
and no allocation in the callback, and its failure modes are quiet — a reverb
tail slightly late or slightly smeared produces no crash, no test failure and no
bug report anyone can articulate. Use `fft-convolver` 0.4.0 for the general part
and apply the Web Audio normalisation scalar, which genuinely is the one line the
spec hands you. The impulse response is decoded once in the `xtask` and embedded
as raw PCM: it is a 55 KB compile-time constant that never changes, and pulling a
general-purpose media demuxer into the shipped runtime to read it is the wrong
trade. Have the xtask print the decoded frame count so the embedded size is a
recorded number rather than an estimate.

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

`cpal` device enumeration and streams, `rubato` resampling pinned `=5.0.0` and
used through `process_into_buffer` only, APM wiring, the VAD port, `opus` encode
with FEC and DTX, pinned `=0.4.0`.

> **Corrected 2026-08-24: `=0.3.1` is not usable.** That pin binds libopus through
> `audiopus_sys`, which carries RUSTSEC-2026-0150 — implicitly unmaintained, last commit
> five years old, and pinning a CMake version that CMake 4.0 refuses, so the build breaks
> for anyone with a current one. `cargo-deny` fails on it, which is the gate doing its job.
> `opus` 0.4.0, released 2026-08-23, binds through `opusic-sys` instead and builds clean.
> The pin stands as a pin; only the number moved.

The APM is `sonora` 0.2.0, behind the trait boundary the architecture already
specifies for it. The condition attached to this — a green `i686` build, run by
P1+ and confirmed by G2 — lapsed on 2026-08-24 with the target itself. The trait
boundary is what matters now, because LiveKit's `libwebrtc` binding is reachable
again and P3 should weigh it against `sonora` before committing.
> **Superseded 2026-08-25.** The Linux-only test baseline below cannot be run any
> more, and does not need to be: the 2026-08-24 measurement in
> `experiments/README.md` ruled `webrtc-audio-processing` out on MSVC entirely and
> made the real comparison sonora against `libwebrtc`. The A/B measurement described
> here would have ranked a candidate that is already out.

`webrtc-audio-processing` `=2.1.0` stays in the tree as a **Linux-only test
baseline**, not as the shipping canceller: it does not build on either Windows
target, PR #102 "Support MSVC targets" has been open and unmerged since
2026-08-08, issue #34 "Windows build" has been open since 2023-09-27, and its CI
runs on `ubuntu-latest` only, so nothing upstream would catch a regression even
after #102 lands. Windows is the overwhelming majority of this app's users, and
an A/B echo-return-loss-enhancement measurement of the two on Linux — where both
build — is what justifies the choice rather than asserting it. sonora's own risks
are different from the ones a "young crate" framing suggests, and they are gate
items: bus factor 1, two releases, and an i686 build that is genuinely unproven.

Keep the device layer behind a trait from day one, exactly as the APM is, and
name `cubeb` 0.38.0 as the documented fallback with a written trigger condition —
cpal 0.18 is a ten-week-old rework whose WASAPI device-change path, the one this
app already has a bug class around, has four open issues on it.

> **Where 3c stands, 2026-08-24.** Resampling, the codec, the VAD, the APM and
> device enumeration are built and tested. `acl-audio::ring` is the buffer between
> the capture callback and the worker that §3.2 now requires, with the wrap, the
> overwrite-oldest and the all-or-nothing frame read under test and measured at
> zero allocations.
>
> `acl-audio::stream` is the rest of it, split so that the part with decisions in it
> can be tested and the part that touches a sound card cannot hide any. `choose`
> takes what a device says it supports and picks a rate, a channel count and a
> buffer size, with twelve tests: 48 kHz over everything else because Opus, the
> canceller and the mixer all run there; the fewest channels that carry the audio,
> because a device offering eight will open with eight; and a buffer of one frame
> at whatever rate was chosen — 960 at 48 kHz, 882 at 44.1 — clamped into what the
> device accepts.
>
> The `cpal` layer over it translates types and nothing else. It cannot be tested
> here: CI has no sound card, and §5.2 already puts device behaviour in the manual
> pass, with a call live, because that is the only place it can be seen. Keeping it
> that thin is the point — every decision it might have made wrongly has already
> been made above it, in a function with tests.
>
> The ring deliberately does **not** split across threads. Making it a real
> single-producer single-consumer queue is either hand-written `unsafe` in the
> middle of the audio path's trusted computing base or a dependency, and that is a
> choice to make when the streams are wired to a running pipeline rather than
> ahead of it.

### 3d. Jitter buffer and playback (2 wk)

`neteq` integration with `default-features = false` — mandatory, or the audio
crate pulls a web framework, a CLI parser, a second `cpal` three majors behind
the pinned one and a second Opus implementation — plus an implementation of
`neteq::codec::AudioDecoder` over the `opus` crate, so libopus stays the only
codec in the binary. Then Opus decode, PLC, the mixer, and output device
selection (replacing `setSinkId`). NetEQ is pull-based: accelerate, preemptive
expand and expand all drive decode on demand, so the `AudioDecoder`
implementation is what makes the pipeline work at all, not an optimisation.

> **The mixer is `acl-audio::mixer`, and it produces two buffers rather than one.**
> The output the device is handed, and the mono downmix the echo canceller needs as
> its far-end reference. §3.3 spends a paragraph on why that reference must be this
> buffer and no other, and putting the downmix anywhere else leaves a caller free to
> assemble the wrong one — which does not fail, it silently stops cancelling.
>
> It clamps, because Chromium's destination node clamps and every other number in
> this crate is matched against Chromium; differing at the last addition after
> matching five DSP nodes to −80 dBFS would be strange. Thirteen peers, zero
> allocations, and the clamping is reported rather than hidden.
>
> Output device *selection* is `acl-audio::device`: enumeration, defaults, and
> `reacquire`, which finds a device again by id and falls back to its name — the
> failure the Electron client has a bug class around, where a driver update changes
> the id and `setSinkId` silently sends one player's voice nowhere.

Measure it against a well-tuned fixed jitter buffer with Opus in-band FEC and PLC
under the same emulation. That is what most peer-to-peer voice apps ship, and
without it the gate has no baseline to judge NetEQ against — and no fallback
short of porting the reference implementation, which is a multi-week job.

> **Built and measured 2026-08-24, and the measurement is mostly about the
> measurement.** `acl-audio::neteq_bridge` is the `AudioDecoder` implementation
> this item asks for: `neteq` now decodes through the same libopus as everything
> else, which is what keeps `ropus` out of a binary that already links the
> reference implementation. `tests/jitter_comparison.rs` runs both buffers through
> the same twelve impairment profiles.
>
> **`neteq` 0.9.1 cannot be evaluated offline.** Its delay manager ignores the
> `arrival_time` on the packet it is handed and calls `Instant::now()` itself
> (`delay_manager.rs:245`), measuring the gap between consecutive `insert_packet`
> calls. There is no clock to inject and no seam to add one. So the network it
> believes it is on is the timing of whatever process is feeding it.
>
> Two versions of the harness drove it from simulated time before this was found.
> Both produced a tidy table: its estimator saturated at `base_maximum_delay_ms`,
> it stretched every frame chasing a two-second target, and every frame came back
> classified `Expand`. One of those runs was very nearly written up as *NetEQ under
> packet loss*.
>
> Running the comparison in real time is the only option left, and a test is not a
> good clock: Windows' timer granularity is about 15 ms against a 20 ms packet
> interval, so the harness contributes jitter of the same order as the thing being
> simulated. The `clean` profile is the control that proves it — `neteq` reports
> about 11% of frames as concealment on a network with **no impairment at all**.
>
> | | fixed | `neteq` |
> | --- | --- | --- |
> | clean | 3.1% gaps, 60 ms | 11.2% gaps, 207 ms |
> | 10% loss | 5.8% gaps, 60 ms | 18.9% gaps, 110 ms |
> | 500 ms freeze | 11.0% gaps, 60 ms | 19.0% gaps, 149 ms |
>
> **Read only the left column.** The right one is this harness's scheduling as much
> as it is `neteq`, and publishing it as a verdict would be the same mistake the FEC
> counter made: a number that looks like evidence and measures the instrument.
>
> What the item asked for — a baseline to judge NetEQ against — exists now for the
> fixed buffer, under the same real-time conditions, and those numbers are
> comparable to each other. Judging `neteq` itself needs either a version whose
> delay manager takes a clock or a harness that is not a test, and that is P4's
> problem, at the point there is a real network to put it on. **The fixed buffer
> ships until then**, which is what 3e's FEC recovery already assumed.

### 3e. The Opus FEC feedback loop, both directions (2 wk)

libwebrtc emits Opus FEC only once RTCP receiver reports tell it there is loss.
A Rust client that sets the flag but sends no RR achieves nothing, and because
the Chromium peer then never learns it is losing packets either, it stops
emitting FEC too. On a clean LAN 1.x↔2.x is perfect; at 3% loss, 1.x↔1.x sounds
normal and 1.x↔2.x sounds broken in **both** directions, intermittently, for one
pair — and it presents as a 1.x bug.

Route the loss fraction out of `rtc`'s `ReceiverReportInterceptor` rather than
rebuilding RR generation, drive `OPUS_SET_PACKET_LOSS_PERC` with hysteresis, and
clamp the reported loss so a lying peer cannot drive the encoder anywhere
harmful. Then implement the receive half: `decode(input, output, fec: true)` on
packet *N+1* to reconstruct *N*, driven by the jitter buffer's loss signal.
Whether `neteq` 0.9.1 can signal loss to the decoder in a way that permits
out-of-order FEC recovery is not established anywhere in its documented surface;
that is why it is a G2 criterion.

> **Answered 2026-08-24: it cannot.** `neteq` 0.9.1's `AudioDecoder` trait is
> `sample_rate`, `channels` and `decode(&[u8])` — there is no way to say "this payload is
> the next packet, decode the redundant copy of the previous one out of it". Its source
> does not mention forward error correction at all; it fills a gap from its own expansion
> in `expand.rs` rather than by asking the decoder.
>
> So recovery has to be arranged by whatever owns the packet sequence, and the fixed
> buffer this item already required as a baseline is where it lives:
> `acl-audio::jitter`. It holds packet *N+1* before giving up on *N*, which is what makes
> the recovery possible at all, and it reports per frame whether the audio came from a
> packet, from the redundancy, from concealment or from nothing — so the impairment
> harness can say *how* a stream survived rather than only that it did.
>
> That does not rule `neteq` out. It rules out `neteq` alone meeting criterion 5, which
> means the comparison the item asks for is now between a buffer that can recover and one
> that cannot, and the measurement has to say what that is worth.
>
> **Sending half done, 2026-08-24.** `acl-audio::fec` turns a receiver report into
> `OPUS_SET_PACKET_LOSS_PERC`: it rises quickly, falls slowly, holds a dead band so a
> settled call stops re-planning libopus's bit allocation every interval, and clamps at 25%
> so a peer that lies degrades its own audio and nobody else's. `idle()` decays when reports
> stop, or a peer that left would be paid for until the call ended.
>
> **The `ReceiverReportInterceptor` wiring is deliberately not written, and this is the
> decision rather than a gap.** `rtc-rtcp` has a stable 0.20.3 matching the `webrtc =0.20.3`
> that §7 proposes, so it *could* be taken today. It should not be: §4.6 item 1 schedules a
> three-week spike to choose between `webrtc` and `str0m`, and taking a dependency on one
> of them to save a single call would make that choice by accident, in the wrong phase, for
> the wrong reason. The seam is `observe_fraction_lost(u8)`, and its argument is RFC 3550
> §6.4.1's `fraction lost` — a definition no crate choice changes. Whichever arm the spike
> picks, the wiring is one line at the point the interceptor delivers a report.
>
> Criterion 5's Chromium sender is phase 4 for the same reason: there is no transport to
> put a Chromium peer on the other end of.
>
> **Building the join found that the receiving half had been measuring nothing.**
> `decode_lost` succeeds whether or not the packet it is handed carries a redundant copy:
> given none it produces concealment and returns the same frame size. The buffer counted
> those successes as recoveries, so it reported *identical* numbers for a sender that had
> been told about loss and one that never had -- 46 frames either way -- which is the exact
> failure this item exists to prevent, wearing the label of the fix. `codec::has_redundancy`
> asks `opus_packet_has_lbrr` instead. With the loop closed: 37 recovered, 22 gaps. With it
> open: 0 recovered, 59 gaps.
>
> The corrected classification appeared to expose a threshold -- below about 5% reported
> loss, no redundancy at all, zero rather than a little -- and it was written down here as
> a property of libopus.
>
> **It was not. Corrected 2026-08-24.** LBRR lives in libopus's SILK layer; libopus decides
> for itself whether a signal is speech or music, and music is coded by CELT, which carries
> no LBRR. The encoder had been left to guess. `Encoder::new` now says `Signal::Voice`,
> which is simply true of this application, and 1% recovers 6 frames where it recovered 0,
> 2% recovers 20, and 5% recovers 39 instead of 28.
>
> **The same mistake also broke the controller outright, which is worse.** Told about 5%
> before its first frame, the encoder protected 175 of 200 packets. Told the same thing
> after two hundred frames -- which is what a receiver report does, and the only thing the
> controller ever does -- it protected **none**: an encoder settled into a mode without LBRR
> did not go back for it. So the loop §3e exists to close was reporting success and
> achieving nothing, wearing the costume of the fix for exactly that fault. With
> `Signal::Voice` it protects 171 of 200.
>
> It was found by asking of our own packets the question that had been asked of Chromium's:
> not "did the call succeed" but `opus_packet_has_lbrr` -- is the redundancy actually in
> there. No bitrate ladder — below roughly 16–20 kbps
libopus carries no meaningful LBRR, so a ladder's bottom rung would disable this
loop exactly when it is needed.

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
> 5. Under a 5% loss profile with a Chromium sender, the Rust receive path
>    recovers Opus in-band FEC — `decode(..., fec: true)` on the following
>    packet, driven by the jitter buffer's loss signal — and `getStats()` on the
>    Electron peer shows `fecPacketsSent` climbing in both directions.
> 6. ~~A green `cargo build --target i686-pc-windows-msvc` of whichever APM is
>    shipping, and its test suite passing there.~~ **Struck 2026-08-24.** The
>    `i686` target existed only for the injection path, which no longer exists.
>    The criterion is not weakened, it is unreachable: there is no such build to
>    be green. This also removes the constraint that ruled `libwebrtc` out, so
>    the APM choice in §4.5 is open again on wider grounds than when it was
>    made.
>
>    **Measured 2026-08-24:** `libwebrtc` 0.3.45 builds, links and runs on
>    `x86_64-pc-windows-msvc`, so it really is a live option and not a
>    theoretical one. But `webrtc-sys-build` downloads a prebuilt 86 MB
>    `webrtc.lib` from LiveKit rather than compiling it — 493 MB in the build
>    directory — and this project has spent the same week going the other way,
>    stripping prebuilt binaries out of `native/uiohook-napi` so everything is
>    compiled from source. That is the axis the choice now turns on, not echo
>    return loss. See `experiments/README.md`.
>
>    **Decided 2026-08-24: sonora.** The A/B was run — same far end, same echo
>    path, both cancellers — and it came out 11.6 dB against 11.3 dB, which is to
>    say no difference at all. That settles the only thing that was in doubt,
>    which was whether sonora is worse. It is not, so the decision falls to the
>    prebuilt blob, and to the fact that `webrtc-sys` does not link in release on
>    Windows without forcing the static CRT on the whole binary.
>
> Criterion 3's 30 ms latency budget and the NACK target-delay decision are made
> **jointly**, not independently: a buffer deep enough to make a retransmission
> useful over a 60–100 ms RTT can consume the entire budget.
>
> **If (3) fails and cannot be fixed within two weeks, stop the port.** That is
> the honest exit: without a jitter buffer at least as good as NetEQ, a proximity
> voice chat is worse than the thing it replaces, and no amount of GUI work
> changes that. Phases 0–2 remain valuable on their own (a Rust server, and a
> Rust game reader that can be exposed to the Electron client through a small
> N-API shim if desired).
>
> Criterion 5 is part of the same stop decision. If `neteq` cannot support
> out-of-order FEC recovery, the cost of vendoring the reference NetEQ is decided
> here, at the gate, not five weeks later in P4 — which is precisely why the
> criterion sits here rather than with the other interop work.

## 4.6 Phase 4 — Transport and signalling (10.5 weeks)

> **Status, 2026-08-25, amended 2026-08-26. The decisions are built and tested; one of
> the two missing pieces of wiring is now there.**
>
> The same split `acl-net` already used for the Socket.IO client: everything that decides
> anything is a pure function with tests, and the layer that touches a transport is
> translation. That layer does not exist yet for `webrtc`.
>
> | Item | State |
> | --- | --- |
> | 1 The crate spike | three of four questions answered by `experiments/webrtc-probe` — it connects, `ring` 0.17.14 is shared with the existing tree, 141 new crates and all four supply-chain gates pass. The Chromium arm is unanswered and, with G3 struck, unanswerable |
> | 2 `Peer` | done — candidate queue, generation counter for the un-detachable handler, connect timeout, and `rtc::to_configuration` over the crate. `tests/loopback.rs` drives two real connections through all of it |
> | 3 The mesh | **done, 2026-08-26.** The relay rules, `RepairPolicy` and `mesh::Membership` were already here; the driver this row asked for arrived as `acl-core::session` for the socket and the membership, and `acl-core::peers` for the connections. Two `PeerSet`s negotiate through the client's own signal format and reach `Connected` in `tests/peers_loopback.rs`, and two `Session`s meet in a lobby on a real server binary in CI |
> | 4 `validateClientPeerConfig` | done, with its tests |
> | The four named regression tests | all four exist and pass |
>
> Three things were found by doing it rather than by planning it. `reconnect.rs` claimed
> to be a straight port and was a port of half the file, with a doc comment carrying the
> pre-1.0.4 meaning of `should_give_up` — the behaviour relay rule four forbids.
> `build()` returns `impl PeerConnection` with one un-detachable handler, which item 2
> predicted and the probe confirmed. And on loopback the answer arrives before the first
> candidate is gathered, so a naive integration test exercises the candidate queue not at
> all — the queue's whole reason for existing is the signalling round trip, and a test
> without one proves nothing about it.

> **The driver, 2026-08-26.** `acl-core::session` is split in two, and the split is the
> point rather than tidiness: `Lobby` holds the membership and the interpreting and touches
> no socket, `Session` holds the connection. The first version was one type, and it cost
> something immediately — the only way to test that a signal from outside the lobby is
> refused was through a real server, and **the server refuses those itself**. That test
> passed by timing out, having never reached the client's rule. It is a unit test now, and
> it fails if the rule is removed.
>
> Two conformance cases run against a real server binary in CI, which already provides one:
> two sessions join a lobby, each is told about the other by a *different* event —
> `join` for the one already there, `setClients` for the one arriving — and a signal
> crosses between them. That both paths produce the same `PeerJoined` is the whole reason
> the driver exists.

> **The connections, 2026-08-26.** `acl-core::peers` holds one per member. Three things
> the building of it contradicted, each found by running it rather than by reading:
>
> * **The generation has to be shared.** `acl-net`'s loopback test carries it by copy,
>   which is right there because it never replaces a connection. A copy makes every handler
>   compare its own generation against itself and conclude it is current, so a replaced
>   connection goes on feeding candidates into the live one. It is an `AtomicU64` the set
>   raises before the old connection is dropped.
> * **A renegotiation must not rebuild.** `offer` to a peer that already has a connection
>   makes a fresh offer on it. Item 2's `signalRoute` guards the receiving side against
>   exactly this — the shipped client treated every offer as a new connection, so the
>   repair for a stalled link killed it — and nothing guarded the sending side until now.
> * **Audio is not a boundary this layer can defer.** The first version opened a connection
>   with no media and reported its state, leaving the track to `acl-audio`. It cannot
>   connect: an offer with no `m=` line carries no ICE credentials, and the far end answers
>   `set_remote_description called with no ice-ufrag`. Every connection now carries an Opus
>   track from the moment it is built, and the default codecs are registered with the media
>   engine — without which the same mistake surfaces several steps later as
>   `ErrRTPTransceiverCodecUnsupported`. Nothing is written into the track here; what this
>   settles is the shape of the negotiation, which is this layer's own.



**Why 10.5 and not 5:** since 0.20.0 the `webrtc` crate is a runtime-agnostic
rewrite on a sans-IO core rather than a Pion port, so this is a port and not a
mapping — and the Socket.IO client that used to be item 1 has moved to P1+.

1. **The crate spike, weeks 1–3**, on all three targets including i686, because
   whatever crypto backend the tree resolves to is nearly free to discover while
   the rig is standing and expensive to discover in P7+. Spend it proving
   `webrtc` `=0.20.3` against a real 1.0.2 Chromium client — direct, relay-only,
   trickle in both directions, SDP captured. The two arms are not symmetric and
   must not be run as though they were: `str0m` explicitly does not implement
   TURN, and this client requires relay-only through coturn, so that arm cannot meet
   the requirement without first importing or writing an RFC 8656 client, and its result
   would measure a hand-written I/O loop as much as the crate. Timebox it to a
   written feasibility read answering only "what TURN client and what event loop
   would a 14-peer mesh need". TURN is the reason `webrtc` wins here; its
   staleness relative to str0m is not, and neither crate can demonstrate Chromium
   interop in CI, which is why this is a spike and not a table.

   > **Partly answered, 2026-08-25**, by `experiments/webrtc-probe` — and the part that
   > is answered is the part that was expensive to leave until P7+.
   >
   > The crate connects: two peers, offer and answer, candidates trickled both ways after
   > the descriptions are set, a data channel, and a message through it. The crypto
   > backend resolves to **`ring` 0.17.14**, which is the version the tree already carries
   > for `rustls` — no second TLS backend, no version split. It costs **141 new crates**,
   > and all four supply-chain gates accept them: `cargo deny`, `cargo audit`,
   > `cargo about`, and `cargo vet` once the exemptions were written down.
   > `rtc-turn` is in the dependency list, so the premise this item rests on — that TURN
   > is why `webrtc` wins over str0m — holds as a fact rather than a claim.
   >
   > What is *not* answered is the Chromium arm, and with G3 struck nothing else answers
   > it either. The three weeks this item budgets should shrink to what is left: the
   > i686 leg went with the 32-bit target on 2026-08-24, the crypto question is closed,
   > and the str0m arm's written read is now moot — a spike that cannot be adjudicated by
   > a gate is a spike with no decision attached to it.
2. `Peer` over the `webrtc` crate, pinned `=0.20.3` because the maintainer states
   a minor bump may carry breaking changes: trickle ICE with candidate queueing,
   data channel, connect timeout, TURN with `relay`-only support. One shape
   change to design for rather than discover: `peer.ts` nulls all five event
   handlers before `pc.close()`, and that teardown is exactly how the 1.0.0 fixes
   avoid acting on events from a connection being replaced. `webrtc` 0.20 takes a
   single `Arc<dyn PeerConnectionEventHandler>` with no per-event detach, so the
   pattern becomes a generation counter or an atomic detached flag inside the
   handler. That is where offer glare and stuck-in-`new` come back.
3. The peer mesh: join, leave, offer glare, orphan cleanup, rebuild-on-failure.

   **Relay discipline, learned the expensive way in 1.0.4** (§5.3). A mesh's demand
   for relay reservations is quadratic in the lobby, and a relay grants a finite
   number of them: the production one was granting twelve, shared across every
   player, and the Electron client was asking for three per connection. One player
   exhausted the server. Four rules follow, and none of them is obvious from the
   `webrtc` crate's API:

   - **One allocation per connection.** A server that advertises the same relay
     twice must not produce two.
   - **A refusal is temporary.** RFC 5766's 486 means the relay is reachable and
     full; the reservations come back when somebody leaves. Retry, and never
     report it as a network problem at this end.
   - **Never force relay-only without a relay candidate in hand.** It leaves the
     connection with no candidates at all, so a peer that sometimes connected
     directly stops connecting ever. This is the escalation that makes things
     worse, and it is the one a counter-based rule reaches for.
   - **Do not give up.** Six attempts and then silence for the rest of the round
     was the old behaviour, and the obstacle is frequently not permanent.

   Rebuild-on-failure is also not the first response to trouble. A connection that
   goes `disconnected` should get an ICE restart after a few seconds -- it keeps
   the connection, its tracks and its DTLS session -- and only a `failed` should
   cost a full rebuild. Measure that the restart really renegotiates on whatever
   stack this lands on; a repair that quietly does nothing is indistinguishable
   from the fault.
4. `validateClientPeerConfig` port — its tests come across unchanged.

**The four 1.0.0 connection bugs become named regression tests**, because a port
will otherwise reintroduce them:

| Test | The bug it guards |
| --- | --- |
| `signal_from_unknown_socket_is_ignored_not_crashed` | the server sends `{data, from}`; `client` was destructured and always undefined |
| `trickle_candidate_without_type_is_forwarded` | only signals with a `type` were forwarded, so trickled ICE was dropped |
| `offer_glare_does_not_destroy_replacement` | the old connection's `close` tore down the new one for the same peer |
| `connection_stuck_in_new_times_out` | ICE never starts, so the connection never fails on its own |

> **Gate G3 — struck 2026-08-25.**
> It required a 1.0.2 Electron client and a Rust client in the same lobby against
> the same server, hearing each other both ways — direct and with
> `forceRelayOnly` through coturn, on Windows and Linux, across a NAT — then the
> same call repeated under each of P3+'s impairment profiles within 0.2 MOS of a
> 1.x↔1.x call, plus a three-client mixed-generation row with one client leaving
> and rejoining.
>
> **What striking it costs, written down rather than absorbed.** `README.md`'s
> gate table already priced failing this one: *no staged rollout; reconsider
> scope*. That is now the standing position rather than a contingency. Interop
> between the two generations is unproven, so the parallel install in §4.10 no
> longer rests on a guarantee, and §4.12's assumption that one lobby holds both
> generations for weeks is an assumption rather than a measurement.
>
> Nothing about the work gets smaller. P4 still has to build a mesh that talks to
> 1.x, the relay rules still hold, and the four named regression tests below still
> stand. What is gone is the rig that would have caught a mistake before the fleet
> did — so the first evidence of an interop fault now arrives as a player who
> cannot be heard.

## 4.7 Phase 5 — Platform layer (6 weeks)

> **Status, 2026-08-26. Four of the platform calls exist; the two processes do not.**
>
> This phase's shape was "the decision logic stands and is tested; the platform calls are
> missing entirely", and four of them were one call each.
>
> | Built | Where |
> | --- | --- |
> | Single-instance lock | `acl-core::single_instance` — measured against a running 1.x rather than guessed; see the note further down |
> | Push-to-talk poll | `acl-core::keys` — `GetAsyncKeyState`'s high bit, turning a level into edges |
> | Exclusive-fullscreen detection | `acl-core::fullscreen` — the bit `overlay::availability` always took and nobody produced |
> | The pipe between the two halves | `acl-ipc::pipe` — the helper is the server, because a pipe server can impersonate its client |
> | The elevated process | `acl-helper` — reads the game, sends frames, and exits when the core does |
> | Starting it, elevated or not | `acl-core::launch` — unelevated first, UAC second, and a declined prompt is an ordinary state |
> | The UIPI access check | `acl-core::game_window` — ported from `windows.c`, quirk included: a hung window refuses the probe with the same error an integrity mismatch gives |
> | Following the game's window | `acl-core::game_window::Follow` — polled, not hooked, for the reason below |
> | The driver that owns all of it | `acl-core::link` — §4.6 says this belongs here, and it is what keeps `HelperState` true |
> | The overlay window | `acl-helper::overlay` — a layered window, composed from sprites, with no toolkit and no GPU under it |
>
> **The overlay window, 2026-08-26.** Built, and not with a GUI framework. §6's checklist
> says what this process may be — "no listening socket, no HTTP client, no image decoder
> and **no GPU context**" — and a toolkit-drawn overlay has the last of those. The rest of
> these documents already describe the alternative without naming it as one: §3.3 calls it
> a *layered window* throughout, and this section says it "receives pre-rasterised sprites
> ... and never fetches or decodes an image". `UpdateLayeredWindow` from a premultiplied
> bitmap needs no toolkit, no renderer and no GPU, and pre-rasterised is exactly what it
> wants. `experiments/overlay-probe` used eframe, and that is not a contradiction — it
> answered whether such a window is available at all, in P1, with the framework candidate
> to hand. Its answer transfers; its implementation does not.
>
> **And "sprites" is a size limit, not a turn of phrase.** The first version of the IPC
> carried a whole frame. `acl_ipc::MAX_FRAME` is 64 KiB and an overlay covering a 2560×1440
> screen is 14.7 MB of premultiplied BGRA, so a picture cannot cross that pipe and never
> could. The protocol is `ClearOverlay`, `DrawSprite` and `PresentOverlay`, and the
> composition happens on the far side — a blend, which needs no decoder.
>
> Its tests ask the operating system rather than looking: every extended style is asserted
> with the failure its absence causes, because a window that is merely invisible looks
> exactly like one that is transparent, and one that swallows clicks looks exactly like one
> that does not until somebody tries to play through it.
>
> **What is left is the content**, which is §4.8 item 5 rather than this phase: what the
> overlay shows, and the rasterising that turns it into sprites.
>
> **This section says "port `windows.c` directly rather than re-deriving it", and one part
> of it is deliberately not ported.** That file follows the game with
> `SetWinEventHook` on `EVENT_OBJECT_LOCATIONCHANGE`, `EVENT_OBJECT_DESTROY` and
> `EVENT_SYSTEM_FOREGROUND`, which is right for its consumer: JavaScript, which a poll
> would cross into sixty times a second. This consumer is a render loop that is already
> awake, so the hook buys nothing and costs a message loop on a dedicated thread — an
> out-of-context hook only delivers to a thread that pumps — plus that thread's affinity.
>
> And the hook is not reliable on its own. `windows.c` re-checks `GetForegroundWindow()`
> after every focus event, with a comment saying the hook fires for windows that did not
> actually get focus: the workaround for the hook is the poll. `Follow` therefore polls,
> which is the same reasoning this section already applied to the keyboard, for the same
> reason — a direct call cannot be silently unhooked.
>
> **And one item struck rather than deferred.** This section lists autostart. The Electron
> client has none — no `setLoginItemSettings`, no run key, nothing in `ISettings` — so there
> is nothing to port and no shipped behaviour to match. It is a new feature, and it should
> be decided as one rather than arrive as a line in a platform checklist.
>
> One thing the built items have in common is worth recording. Every one of them was
> decided by a measurement on a real machine, and four of those measurements contradicted
> something believed beforehand: the single-instance name was not the mutex this document
> named; the display state under a running game is `QUNS_BUSY` rather than the
> D3D-exclusive value the overlay logic keys on; `WaitNamedPipeW` does not wait for a pipe
> that does not exist yet, which is the only case it was called for; and a duplicated pipe
> handle deadlocks, because it refers to the same synchronous file object. Three of the
> four looked correct and passed a first message before failing. The platform layer is
> where a port stops being a translation.

**Why 6 and not 3:** the client becomes two processes, and the overlay moves into
the elevated one.

Keyboard hook, overlay window, single-instance lock, autostart, paths, logging.
Port `native/electron-overlay-window/src/lib/windows.c` logic directly rather than
re-deriving it — that code already knows about the edge cases this needs.

> **Corrected 2026-08-25.** This also named `x11.c`, which has been deleted along
> with the client's Linux support. The same change removes the Unix-socket half of
> the IPC below, Wayland detection, and the exclusive-fullscreen decision's backend
> argument — `acl-core::overlay` is now one bit wide.

**Two processes, not one.** `acl-helper` runs elevated and holds memory reading,
injection, the keyboard hook and the overlay window. `acl-core` is never
elevated and holds tokio, signalling, WebRTC, audio and the GUI. Length-prefixed
`postcard` over a named pipe. A thread
boundary is not a privilege boundary, and the alternative — a single elevated,
unsandboxed address space holding the RTP parser, the Opus decoder, an image
decoder for remotely fetched hats, the TLS stack and a process-memory writer — is
also a straight availability regression against today, where the overlay is its
own `BrowserWindow` and a driver fault there does not drop the call.

**The helper is started on demand, with a UAC prompt each launch.** There is no
Windows service. `acl-core` starts unelevated, and the first thing that needs
the game — the memory reader — launches `acl-helper` through a
`runas` elevation, once per session. The prompt is visible friction and it is
accepted: a service would remove it by installing a permanently resident
process running as SYSTEM that holds a process-memory reader and a code
injector, which is a worse thing to own than a dialog. Three consequences to
build for rather than discover.

"The user clicked No" is an ordinary state, not a startup failure. `acl-core`
runs without a helper and says so accurately: voice works, the game reader does
not, and neither does the overlay. Push-to-talk must not be on that list — the
key poll is `GetAsyncKeyState` and needs no elevation of its own, so it is the
one helper-side item that falls back into `acl-core` when there is no helper
rather than disappearing with it. Losing the ability to speak because of a dialog
is not a degradation anybody would accept.

The prompt fires at a moment the user can connect to something they did — opening
the app, or joining a lobby — never from a background timer several minutes after
launch, which reads as malware and gets answered No for that reason alone.

And because the split is settled, the `--single-process` fallback feature is not
built and CI does not carry it.

The overlay is in the **helper**, which is the counter-intuitive half. UIPI
blocks window manipulation and out-of-context `SetWinEventHook` across integrity
levels, so an unelevated overlay stops following an elevated game — the exact
configuration the README instructs users into. The consequence is a design
constraint from the first commit: the overlay receives **pre-rasterised sprites**
over the IPC and never fetches or decodes an image, so no image decoder enters
the elevated process. Port the UIPI access check too; it is the difference
between "the overlay is broken" and an accurate message about elevation.

**The keyboard hook stays a poll**, though the reason has changed. The Electron
client used to poll `GetAsyncKeyState` every 60 ms through
`native/node-keyboard-watcher`; that module carried no licence and was replaced by
`native/uiohook-napi`, which installs `SetWindowsHookEx(WH_KEYBOARD_LL)` and
`WH_MOUSE_LL`. So `SetWindowsHookEx` is now in the tree, and a port that used one
would be porting something real.

It should not. A desktop-wide hook is a latency dependency in front of every
keystroke on the machine and is silently unhooked if a callback exceeds
`LowLevelHooksTimeout`; the Electron client accepted that to escape an unlicensed
dependency, which is not a constraint the port has. `GetAsyncKeyState` is a direct
call and needs no crate. See §6.1 for what libuiohook does to make its hook
tolerable, and for the mouse-motion patch the client carries.

Also here: exclusive-fullscreen detection, because with Fullscreen Optimizations
off a layered window will not appear at all and the alternative is a swapchain
hook this project must not ship; ~~Wayland detection gated on the **live winit
backend** rather than `XDG_SESSION_TYPE`~~ — struck 2026-08-25, and worth keeping
legible because it was the subtlest decision in this phase: the variable describes
the session and not the backend the process actually got, so reading it would have
greyed out the overlay for XWayland users who work today; and the single-instance
lock, so a 1.x and a 2.x install on
one machine cannot run two keyboard hooks, two overlays and two memory readers
against the same game.

> **Corrected 2026-08-25.** This said "the same `Local\AnotherCrewLink` name H1
> puts into the field". H1 puts no such name into the field. The shipped client
> calls `app.requestSingleInstanceLock()` and nothing else — `src/main/index.ts`
> line 270 is the only lock in the tree, and Electron keys its `ProcessSingleton`
> on the userData directory rather than on a name a second implementation could
> claim.
>
> So the requirement stands and the mechanism named for it does not. A 2.x client
> taking a mutex called `Local\AnotherCrewLink` would exclude other copies of
> itself and nothing else, which is the failure this bullet exists to prevent
> while looking exactly like the fix. Whatever P5 uses has to be something a
> running 1.x actually holds, and that is either a name added to 1.x in a patch
> release first, or the userData path Electron already keys on. Decided in P5, not
> here — but not by assuming.
>
> **How to find out, since it is a measurement and not a judgement.** Chromium's
> `ProcessSingleton` on Windows is a hidden message window plus a mutex, both named
> from the user-data directory rather than from the product — which is why no name
> in this tree matches it. Start 1.x, enumerate its window classes and named kernel
> objects, and the answer is whichever one a second launch collides with.
>
> One negative result already, taken on 2026-08-25 with the client not running:
> `%APPDATA%\AnotherCrewLink` carries no `SingletonLock`. That is the POSIX half of
> the same mechanism, so its absence confirms the Windows path is the in-memory one
> rather than a file a second implementation could take.

> **Answered 2026-08-26, by the measurement this asked for.** The client was started and
> its windows and named objects enumerated. It holds a message-only window of class
> `Chrome_MessageWindow` whose **window text is the user-data directory**:
>
> ```text
> MSG  class=[Chrome_MessageWindow]  text=[C:\Users\lucas\AppData\Roaming\AnotherCrewLink]
> mutex Local\AnotherCrewLink                                        does not exist
> ```
>
> That is Chromium's `ProcessSingleton`, and it is the only thing a running 1.x holds that
> a different implementation can find. Three properties of the lookup were measured rather
> than assumed, each of them a way to write this and have it silently never match: the text
> carries no trailing separator, the comparison is case-insensitive, and the same process
> holds a second window of that class with empty text — so the class alone is not the lock,
> class and text together are.
>
> Built as `acl-core::single_instance`. It refuses to start when that window exists, and
> takes two names of its own: one derived from the user-data directory, so two 2.x
> installations that keep their files apart may both run; and one fixed, so that the coarse
> case has a name 1.x could also spell. The specific one is claimed first — the other order
> refuses just as correctly and then tells the user a 1.x is running when it is a 2.x.
>
> **One direction remains open, and it is the harder one.** A 1.x *started while a 2.x is
> already running* still starts: 1.x looks for that window and nothing else. Two ways to
> close it, neither free.
>
> 1. **Add a name to 1.x in a patch release.** Electron has no named-mutex API, so in pure
>    Node this is a pid file in the user-data directory, checked with `process.kill(pid, 0)`.
>    Cheap, and it only ever protects installations that took the patch — which is the
>    smaller half of the fleet on the day 2.0 ships and never becomes all of it.
> 2. **Register the same window from 2.x** and answer Chromium's `WM_COPYDATA` handshake.
>    This needs no 1.x change and therefore covers every install in the field, including
>    the ones that will never update. It is also reverse-engineered behaviour that does not
>    fail safe: a newcomer that times out on the handshake concludes the lock is stale and
>    takes it anyway.
>
> Option 2 is testable on this machine — register the window from a probe, launch 1.x, see
> whether it exits — and that experiment is what should decide it, not this paragraph.

## 4.8 Phase 6 — GUI (11.5 weeks)

> **Item 1's spike is built and measured, 2026-08-26.** `experiments/gui-spike`, held to
> the bar this section sets rather than to three text controls: a lobby-browser table of
> sixty-four rows with sortable columns, and twelve avatars composited from four layers
> each, animating, every frame. The table's default order goes through
> `acl_ui::lobby_list::sort` — the shipped rule, on the shipped type — so the spike
> exercises the model rather than a copy of it.
>
> ```text
> RESULT frames=590 rows=64 avatars=12
>        work_median_ms=0.25 work_p95_ms=0.28 work_worst_ms=0.52
>        interval_median_ms=16.67 interval_p95_ms=17.87 interval_worst_ms=44.42
> ```
>
> **A quarter of a millisecond to build a frame**, against a 16.7 ms budget: about 1.5% of
> it, with the table and the avatars both at more than the sizes the real screens need.
> On this evidence the framework question is not close, and the decision point this
> section puts at the end of the main-view milestone has nothing to overturn it with
> unless something later is far more expensive than these two.
>
> **Two numbers rather than one, and the first version had only the wrong one.** It
> reported the frame-to-frame interval — 16.66 ms — which is the display's refresh rate
> and not a cost. Read on its own it says egui takes 16 ms a frame; what it actually
> measures is vsync. The interval is still reported, because it is what says whether
> anything was dropped, but the work figure is the one with headroom in it.
>
> Two caveats, recorded rather than buried. This is the `glow` rung, chosen so the number
> is comparable with `overlay-probe`'s; the wgpu rung of the fallback chain below is a
> separate measurement. And the worst interval is a hitch of 44 ms against a worst *work*
> of 0.52 ms, so whatever caused it was not the drawing — on a desktop with other things
> running, that is the expected shape and not a finding.

**Why 11.5 and not 10:** net of dropping the localisation conversion (−1.0), the
phase gains a framework spike, the GPU fallback chain and the performance
baseline the footprint claims are currently asserted without.

In this order, so that the app is usable as early as possible:

1. Framework spike (0.5 wk) and shell, custom title bar, window state
   persistence (2 wk)
2. Main view: player list, avatars, talking indicators, mute/deafen (3 wk)
3. Settings (3 wk) — the largest single screen
4. Lobby browser (1 wk)
5. Overlay view (1 wk)
6. GPU fallback chain and the performance baseline (1 wk)

The spike must produce more than three text controls — a lobby-browser table with
sortable columns and one composited animating avatar — and its decision point is
the end of the main-view milestone, roughly week five, not the end of the phase
where it can no longer change anything. The transparent click-through window was
answered in P1+, before this phase was planned around it.

**No GPU is not a failure to launch.** Chromium currently gives every user
SwiftShader for free, and this project has already found the problem in the
field: hardware acceleration is disabled on demand through a shipped setting.
Windows goes wgpu/DX12, then WARP through `force_fallback_adapter`, then a CPU
rasteriser.

> **Corrected 2026-08-25.** This said acceleration was "disabled unconditionally on
> Linux today", and that Linux therefore defaults to software. Both halves went with
> the client's Linux support, on the same day and in the same change: the Electron
> client's unconditional arm and `acl-ui::renderer`'s `Platform` enum. The chain
> below is unchanged for the platform that ships. **No glow rung** — glow needs GL
3.3 or ES 3.0, and a Windows machine without a vendor driver offers software GL
1.1, so the rung does not save the RDP and bare-VM cases it would exist for.
Migrate the existing `hardware_acceleration` answer forward rather than inventing
a key, and make automatic demotion non-persistent by default: a key written by a
process in the act of crashing pins users to the slow rung for reasons unrelated
to the GPU.

**Localisation is not a conversion.** The 37 locale directories stay i18next
JSON, read by the loader written in P1+. Measured across all 4,631 strings — it was
4,736 until 1.0.5 removed the mobile-host and OBS keys from all 37 locales — there
is not one interpolation placeholder, not one plural key and not one selector, so
every feature that would distinguish a localisation framework from a flat map is
unused — and Fluent identifiers cannot contain dots, so "translation content is
untouched" would be true of the values and false of the keys, which are what
Crowdin and every call site key on. Keeping the JSON also means 1.x and 2.x
consume the identical tree during the beta, one Crowdin project, translators
working in one format. `format!` covers the first string that ever needs
formatting; nobody should reopen this.

No separate clipboard crate either: `egui-winit`'s default `clipboard` feature
already provides one, and a direct line would only add version-drift surface. (The
original reason was its better Wayland coverage. The conclusion outlived it.)

**Deliberately accepted:** the Rust UI will not be pixel-identical to the React
one. Layout, spacing and control affordances will differ. What must not differ is
what every control *does* — the settings schema is ported unchanged, including
defaults, so that an existing `config.json` keeps working.

## 4.9 Phase 7 — Packaging, update and rollout (9.5 weeks)

**Why 9.5 and not 4:** `cargo-dist` cannot build either artefact type this
project must keep producing, and the update manifest is signed and verified here
rather than delegated to a crate that cannot verify the artefacts we ship.

**Why 9.5 and not 11:** there is no Authenticode code signing. That removes the
CA application and whatever it would have cost in correspondence and rejections,
the signing step wired into two hand-built installer pipelines, the timestamping
and certificate-rotation runbook, and the `publisherName` question in all its
forms. A developer-week and a half, and it was the least predictable time in the
phase, because its schedule belonged to a certificate authority rather than to
us.

1. `cargo-dist` for Windows x64 — for archives and the release job only. Its
   installer set is shell, PowerShell, npm, Homebrew and MSI: there is no NSIS
   backend, and MSI would strand every installed 1.x client, because
   `electron-updater`'s `findFile` picks by extension and changing artefact type is
   the same act as abandoning the installed base. So the NSIS script is hand-built
   and keeps its exact CLI contract.

   > **Corrected 2026-08-25.** This named three targets and a hand-built AppImage,
   > and told the reader to put literal `x64` and `ia32` tokens in the installer
   > names "or 32-bit users silently receive the 64-bit installer". Two things are
   > now true instead. Linux is out. And 1.x never published a token in a name:
   > every release from 1.0.1 to 1.0.5 shipped exactly one
   > `AnotherCrewLink-Setup-<version>.exe`, which is electron-builder's single
   > multi-architecture NSIS installer. With ia32 dropped for the Windows 11 floor
   > that installer becomes x64-only under the same filename, so `findFile` keeps
   > picking it and nothing about the update path changes. Turn on `github-attestations` and `cargo-auditable`, and
   write down the exit: the output is checked-in GitHub Actions YAML, which is
   what makes a one-maintainer build tool an acceptable dependency.
2. **No Authenticode code signing.** Windows artefacts ship unsigned and users go
   on seeing the unknown-publisher warning on every install, exactly as they do
   with 1.0.2 today. Nothing regresses and nothing improves. The
   `publisherName` question falls away with it: no name is configured, so
   `NsisUpdater.verifySignature` skips rather than fails, which is the behaviour
   the installed fleet already runs on — and the failure mode it was written to
   avoid, bricking every install by later switching CA, cannot occur if there is
   never a first name. Reproducible builds where the toolchain allows, with
   `github-attestations` from item 1 doing the work that matters here: they
   answer "prove this binary came from this commit" better than chasing
   bit-for-bit output across three targets, and better than a certificate proves
   who built it.

   What is not covered, said plainly rather than left to be inferred: an
   unsigned artefact means SmartScreen reputation is never accumulated, so the
   warning stays for every user on every release forever; enterprise
   allow-listing by publisher is unavailable; and any integrity guarantee this
   project offers lives entirely in item 3, which protects the update path and
   nothing else. A first download from the website is covered by TLS and by the
   attestation of anyone who checks it, which is nobody.
3. Auto-update, in a separate `acl-updater` binary. `self_update` 0.44.0 is not
   shippable: its non-optional `quick-xml ^0.38` carries RUSTSEC-2026-0194 and
   RUSTSEC-2026-0195, both CVSS 7.5, both fixed only at `>= 0.41.0`, which the
   caret cannot reach — the project's own advisory gate fails against its own
   pinned version, and its signature feature verifies nothing about an NSIS
   `.exe` or an AppImage anyway. Either track its 1.0 line and pin exactly once
   stable, or write the updater around `minisign` verification and `self-replace`
   — that is not writing crypto, the verification stays in a purpose-built crate.
   Two embedded public keys, the operational key held offline and never in a
   release-workflow secret; rollback protection that the user can bypass, because
   the 2.0→1.x downgrade path is documented; no freeze rule, which is a
   fleet-wide time bomb dependent on the user's clock. Never install an update
   while elevated. (There was a second, AppImage-shaped update code path here that
   this project would have owned permanently. It went on 2026-08-25 with Linux, and
   one update path is the improvement.)

   **This is signed where the offsets bundle is not, and the difference is
   availability, not importance.** A release is a planned event: it happens when
   the maintainer decides it happens, and a key that has to be fetched from
   somewhere safe costs nothing on a schedule of our own choosing. An offsets
   bundle is an unplanned burst that starts when Among Us updates and ends when
   players can hear each other again, and a human holding a key in that window is
   the outage. Same project, same threat model, opposite answer, for a reason
   that is about the clock and not about the crypto. The signature covers the
   manifest and, through it, the artefact this project published; it says nothing
   about whether Windows trusts it.
4. Settings migration: read the existing `electron-store` `config.json` on first
   run and write it forward. Test with real files from 1.x installs. The importer
   reads once and **never writes back** — during the beta a user runs both
   clients, and neither may silently rewrite the other's settings.
5. CI: the four existing workflows ported, actions still pinned to commit SHAs,
   `cargo-audit`/`cargo-deny` replacing `npm audit`, `cargo-about` producing the
   attribution file GPL distribution wants, CodeQL still covering the repository.
   One thing CI cannot do for us, and it is a real loss against today: RustSec
   does not systematically track CVEs in the C vendored inside `-sys` crates, so
   `cargo audit` will never report a libopus or APM security release. One
   `electron` bump currently patches libopus, libvpx, BoringSSL and libpng at
   once, with CVE numbers and a public feed. After the port that becomes a named
   human with a named upstream watch list, and it needs an owner here.
6. ~~The Linux tarball with a documented `setcap cap_sys_ptrace+ep` step.~~ Struck
   2026-08-25 with the client's Linux support. It was here because on the common
   `ptrace_scope=1` default the client cannot read the game at all, and an AppImage
   that silently fails is worse than a documented step.

Prove the new NSIS script by shipping an **ordinary 1.0.x release** with it, so
its CLI contract is tested against real 1.x updaters before it carries anything
important.

**Rollout.** The 2.0 build goes out first as a
parallel install — different appId, different directory, config read forward,
opt-in by download only — and sits there for a full release cycle while 1.x keeps
receiving 1.x updates. That parallel install is not the 2.0 release; it is the
opt-in build that precedes it. Moving the installed fleet is P8 (§4.12), and it
does not begin until the elevation gate and the minisign-verified update path are
in the field. Immutable releases already are: they were enabled and verified
through the API on 2026-08-24. The Electron client stays buildable and receives
security updates until the fleet has moved.

**P7+ does not end in the 2.0 release.** The 1.x wire protocol is switched off
when 2.0 ships (§4.12), so the fleet has to be on the bridge before that release
rather than after it. What P7+ produces is a downloadable 2.0 build and a
migration path; what turns it into *the* release is P8 completing. Anyone reading
this phase as the finish line will schedule the switch-off a full bridge rollout
too early.

## 4.10 Milestones and decision points

| | Milestone | Externally visible? |
| --- | --- | --- |
| H1 | 1.0.3 hardening in the field | Yes — ships |
| H2 | **G0** offsets trust chain live | Yes — ships as 1.0.4 |
| H3 | Envelope rules enforced, new OBS feed live | Yes — ships as 1.0.5 |
| M1 | Rust server serves 1.x clients | Yes — ships, then **decision point** |
| M2 | **G1** reader parity on recorded sessions | No |
| M3 | **G2** audio parity and impairment results | No — **go/no-go** |
| M4 | Rust ↔ Electron in one lobby — **G3 struck 2026-08-25**, so this is no longer gated | No |
| M5 | Rust client usable end-to-end, no GUI polish | Internal alpha |
| M6 | Feature parity | Public beta |
| M7 | 2.0 build available as an opt-in parallel install | Yes — ships |
| M8 | **G4** bridge rehearsal on real 1.0.2 installs | No |
| M9 | 1.x fleet moved; 2.0 released; 1.x wire format off | Yes — ships |

M3 can end the project on technical grounds, and the decision point after M1 can
end it on any grounds; those are the two exits. G0 gates the work that follows
it. G4 no longer gates only P8 — because the 1.x wire format is switched off at
M9, G4 is a prerequisite of the 2.0 release itself. M1 is valuable whatever
happens after it. M2's output (a Rust game reader) is reusable from the Electron
client if the port stops.

**Decisions taken before phase 1.** Fifteen questions were put to the maintainer;
the full set, with the reasoning, is in
[09-technology-migration.md](09-technology-migration.md) §6. Ten are settled and
five remain open; the eight settled ones that change work in this document are
recorded below. They are decisions, not preferences, and the phases above are
written as though they hold.

| Affects | Decision | Consequence carried in |
| --- | --- | --- |
| everything | **Only P0+ is committed.** The 74-week plan stands as a plan; the funded work is the hardening track and the Rust server, and the rest is decided again after it ships. | §4.1, §4.2 |
| P0+, H3 | **The mobile-client promise is dropped.** The server is websocket-only; `03-target-architecture.md` §3.5's undertaking to a future 4.x mobile client is deleted, not softened. | §4.1, §4.2 item 6 |
| P1+, P5+ | **The elevated helper is started on demand, with a per-launch UAC prompt.** No Windows service, no `--single-process` fallback, and the two-process split is available and scheduled. | §4.7 |
| H1, P7+, P8 | **GitHub immutable releases are on**, enabled and verified through the API on 2026-08-24. `latest.yml` is frozen once published, so `stagingPercentage` does not exist as a mechanism. | §4.9, §4.12 item 3 |
| H2, P2+ | **The offsets bundle is not signed.** Mirror, scheduled sync PR, upstream pinned by commit, embedded floor, validator on every load. | §4.1, §4.4 item 3 |
| H3, P0+ | **The signal envelope rules are enforced immediately.** No logging period, no threshold, no flag. Clients older than 1.0.5 lose the OBS feed and the mobile relay at once. | §4.1, §4.2 |
| P7+, P8 | **No Authenticode code signing.** Windows artefacts stay unsigned; update integrity is a minisign signature over the manifest and nothing more. | §4.9 items 2–3 |
| P8, P9 | **The 1.x wire protocol is switched off when 2.0 ships.** No dated sunset, no open-ended dual stack; the bridge must have migrated the fleet first. | §4.12 |

One of the five that remain open bears on this document and is not resolved by
any of the above: whether `/socket.io/` is accepted as the permanent server wire
protocol with no second path. It no longer blocks P0+ — a second path was never going to be built
before one existed — but it should be answered before P9 assumes it. The
decisions above lean towards yes: the 2.x client keeps its Socket.IO fallbacks
permanently for third-party servers (§4.12), so a second path deletes nothing
from the client even after our own 1.x support ends.

## 4.11 Effort summary

| Phase | Weeks | Status | Parallelisable |
| --- | ---: | --- | --- |
| H1 1.x emergency hardening | 2.0 | committed | before P0+ |
| H2 1.x offsets trust chain → G0 | 3.0 | committed | before P2+ |
| H3 1.x/Node envelope and OBS | 2.5 | committed | alongside P0+–P1+ |
| P0+ Server | 4.0 | committed | independent |
| **Committed subtotal** | **11.5** | | ends at the decision point |
| P1+ Foundations | 5.0 | built | no |
| P2+ Game reader | 6.0 | built, G1 met | with P3+ |
| P3+ Audio engine | 10.0 | built, G2 met | critical path |
| P4+ Transport | 10.5 | decisions built, transport not wired | critical path |
| P5+ Platform | 6.0 | partly built | with P4+ |
| P6+ GUI | 11.5 | planned | after P5+ |
| P7+ Packaging | 9.5 | planned | partly with P6+ |
| P8 Bridge and sunset → G4 | 4.0 | planned | before the 2.0 release, not after |
| **Total to 2.0, one developer** | **74** | | midpoint of a range whose low end is 65 |
| **Two developers** | not half | | P3+ and P4+ are both on the critical path, and P7+ waits on both |
| P9 Post-1.x cleanup | 3.0 | planned | outside the 2.0 budget |

> **Status column corrected 2026-08-26.** Every phase above read "planned" long after
> it stopped being true — P1+ through P3+ are written, and the two gates they carry are
> met. The weeks are untouched: they are what the phases were estimated at, not a record
> of what they cost, and nothing here re-prices them.
>
> "Partly built" for P5+ is the honest word rather than a hedge. What exists is the
> single-instance lock, the push-to-talk poll, the exclusive-fullscreen check and the
> named pipe the two processes speak over. What does not is the overlay window, the
> elevation path, autostart, and the `acl-helper` binary that would hold the first two.
> §4.7 carries the detail.

**What moved, against the 77 written before these decisions.** H2 −1.0 (no key
ceremony, no signing xtask, no minisign parser in TypeScript, no revocation
path), H3 −0.5 (no seven-day watch, no threshold and no flag to flip), P7+ −1.5
(no Authenticode, no CA). Nothing was re-estimated and no scope was cut to reach
the number: three decisions deleted work that was really in the plan, and the
total follows them down. Everything else is unchanged, including the phases
nobody has committed to. This table, `README.md`'s effort block and
[09-technology-migration.md](09-technology-migration.md) §3.1 carry the same
arithmetic; 09 §3.1 is where it is derived, and any figure that still reads 77
is from before 2026-08-24.

## 4.12 Phase 8 — Bridge and sunset (4 weeks) → **Gate G4**

P7+ produces a 2.0 build anyone can download. This phase moves the people who
will never download anything, and it is the moment a large number of machines
execute a downloaded installer. It does not start before the elevation gate and
the minisign-verified update path are in the field; immutable releases already
are.

**This phase finishes before the 2.0 release, not after it.** The 1.x wire
protocol is switched off when 2.0 ships — no dated sunset, no open-ended dual
stack — so the bridge has to have migrated the fleet *first*. Run it the other
way round and every 1.x user is cut off on release day, which is the same outcome
as never having written the bridge at all, reached at greater expense. Two things
follow. G4 is a prerequisite of the 2.0 release itself rather than a gate on this
phase's own output. And the release is not a date somebody picks: it is whenever
per-version join counts say the fleet has moved, which is why §4.11 gives this
phase weeks of effort and no calendar.

**Accepted risk, recorded rather than closed.** The installer this phase hands to
a large number of machines is unsigned (§4.9 item 2), and the minisign work in
that phase cannot help here: `electron-updater` on the installed 1.x fleet does
not know about it. What actually protects the bridge download is the SHA-512 in
`latest.yml` fetched over TLS from GitHub, plus the fact that immutable releases
make that manifest unmodifiable once published. That is a real control and it is
the whole of the control. `NsisUpdater.verifySignature` will skip rather than
check, because no `publisherName` is configured — which is exactly what 1.0.2
does today, so this is not a regression, but it is the largest single population
of installer executions this project will ever cause and it deserves to be
written down and not discovered.

The mechanism is fully specified and can be read out of the installed
`electron-updater`: `latest.yml` supplies version, path and SHA-512; `findFile`
picks by extension and then prefers a filename containing `x64` or `ia32`;
`NsisUpdater` spawns the installer with `--updated /S /D=<installDirectory>`.

> **Superseded 2026-08-25.** What stood here was the AppImage half, and it was the
> most dangerous item in this phase: the updater unlinks the running AppImage, moves
> the replacement into place and runs it with `execFileSync` and
> `APPIMAGE_EXIT_AFTER_INSTALL=true`. `execFileSync` is synchronous, so a Rust
> AppImage that started its GUI instead of exiting would hang the old client
> forever, on every Linux machine, at once — and the mitigation was one variable
> check in `main()` that nothing would have failed loudly for. Dropping Linux
> deletes the failure mode rather than mitigating it.
>
> It also strands the AppImage users who exist. See the note in §4.1.

1. The bridge is built by the Rust pipeline and published into the 1.x feed as
   **1.1.0**, as one NSIS installer under the name 1.x has always used. The choice
   this item used to pose — tokenised per-architecture installers or one combined
   dual-arch one — was settled by the Windows 11 floor: x64 alone, and the name does
   not change, so `findFile` behaves exactly as it does today. The AppImage arm went
   with Linux on 2026-08-25.
2. No `.blockmap` asset for the bridge, or the updater attempts a differential
   download against a file that is not there.
3. Staged rollout as sequential tagged releases — 1.1.0, 1.1.1, 1.1.2, a week
   apart, cohort baked in at build time. `stagingPercentage` is not available and
   this is settled rather than weighed: it lives in `latest.yml`, `latest.yml` is
   a published release asset, and immutable releases have been on since
   2026-08-24. A published manifest cannot be edited, so a percentage cannot be
   raised. Each step is therefore its own build, its own manifest and its own
   minisign signature over that manifest. Three release ceremonies is why this
   phase is four weeks and not two; dropping Authenticode makes each ceremony
   shorter but does not remove one, and what it saves this phase spends on step 6
   below.
4. The first bridge installer **renames rather than deletes** the Electron
   install and its config, and 2.x ships a documented way back. Only after the
   bridge has sat at full rollout for a cycle does it begin deleting.
5. The **migration is complete before the switch-off**, and "complete" is a
   number agreed in advance and read off the per-version join counts below, not a
   feeling about how long it has been. Until that number is met, 1.1.x keeps
   going out and the 2.0 release waits. This is the step with no upper bound on
   its duration, and pretending otherwise is how the switch-off arrives early.
6. Only then, **the switch-off**: the server drops the 1.x event set and the
   legacy room-addressed feed, and answers a 1.x handshake with a message the
   1.x client displays rather than closing the socket on it. A client that has
   not updated must be told why it stopped working, in the app, in its own
   language — the 37 locale directories are already there. This is small work and
   it is the difference between a sunset and an outage.

**Rollback** is re-marking the 1.0.2 release as *Latest*: un-updated clients
revert within one check interval without touching a frozen asset. It is
all-or-nothing and cannot express a percentage. Deleting the bridge release does
not free the tag, so a retry is 1.1.1.

**Measuring it needs no new telemetry.** The server sees every client that joins
a lobby, so per-version join counts show the fleet moving. The rollout is working
if the 1.0.2 share falls roughly in proportion to the cohort and the 2.x share
rises to match, **with no drop in total joins**. A drop in total joins is the
signal to stop.

> **Gate G4 — bridge rehearsal.**
> On **real 1.0.2 installs**, not dev builds: Windows x64 updates from a staging
> feed to the bridge. Silent install; correct install directory; a working uninstall
> entry; migrated config.
>
> **Narrowed 2026-08-25.** This had three legs: Windows x64, Windows ia32 and Linux,
> with "the correct architecture selected" and "on Linux the old process exits within
> two seconds rather than hanging". ia32 went with the Windows 11 floor and Linux with
> its support, so one leg is left — and the architecture-selection criterion has
> nothing left to select between. The staged rollout puts both generations in one lobby, by design,
> for weeks; G3 was what would have proved that works before the fleet met it, and
> it was struck on 2026-08-25.
>
> **G4 is a prerequisite of the 2.0 release, not of this phase's output.** The
> 1.x wire format goes off at that release, so an unrehearsed bridge is the
> difference between moving the fleet and stranding it. If G4 cannot be passed
> there is no fleet migration, therefore no switch-off, therefore no 2.0
> release: the 2.0 build stays an opt-in parallel install and the server keeps
> speaking 1.x indefinitely. That is a worse outcome than it sounds — it is the
> open-ended dual stack this decision exists to avoid — but it is survivable,
> and shipping the switch-off on an unrehearsed bridge is not.

**Sunset.** There is no dated sunset. The 1.x wire format is switched off when
2.0 ships, and 2.0 ships when the fleet has moved — a sequence, not a calendar,
because a date promises something the rollout cannot guarantee and a date that
slips teaches everyone that the next one will slip too. What is still owed to
users is warning: announce it in-app through the existing update-notification
path at least two release cycles ahead, and when it arrives have the server
return a message the 1.x client displays rather than failing silently (item 6).

**What this decision cannot reach.** Third-party operators run their own servers
on their own schedule, and [the README](../../README.md) invites them to. Our
switch-off says nothing about theirs. A 2.x client therefore still has to cope
with a server that speaks the old dialect — which is the whole reason the 2.x
client keeps its Socket.IO `join_lobby` ack and its socket lobby-browser
fallbacks permanently. That is 09 §6 question 15 answered yes, and it is answered
yes for third-party servers and for no other reason: against our own server those
fallbacks are dead code from the day of the switch-off, and they should be
commented as such so that a later reader does not mistake them for evidence that
our own 1.x support is still alive. The per-step detail for this phase and for
the hardening track is in
[09-technology-migration.md](09-technology-migration.md) §3.

## 4.13 Phase 9 — Post-1.x cleanup (3 weeks, outside the 2.0 budget)

Everything here is blocked not by effort but by the existence of 1.x clients in
the same lobby, and it is not part of the 74 weeks. The switch-off (§4.12 item 6)
is what unblocks it, and it unblocks all of it at once rather than gradually:
after that release there is no 1.x client in any lobby of ours, so the threshold
this phase used to wait on is the same number P8 item 5 already had to meet.

Move lobby settings and the impostor radio claim to the socket, drop the data
channel and disable SCTP, and delete the SCTP fuzz targets. A second wire
protocol is not one of the things the switch-off unlocks, and it is worth being
explicit about why, because half its stated precondition is now met and that
invites the wrong conclusion: the OBS page has been migrated since H3, but the
2.x client keeps its Socket.IO fallbacks permanently for third-party servers
(§4.12), so the Socket.IO parser never leaves the client and a second path still
deletes nothing. Revisit it only if that changes. Revisit the Opus bitrate ladder
only if the impairment harness has actually shown self-inflicted congestion in a
12–15 player mesh — until then it fights the FEC loop on the same input with the
opposite sign, and it is the one behaviour with no Electron reference to measure
against.
