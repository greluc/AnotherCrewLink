# 9. Technology migration plan

Where this project should change technology rather than crate, and how each change
reaches the field without breaking a lobby. Written on 2026-08-24.

## Verification status

Five technology areas, each analysed and then attacked by a second pass arguing from
the position of whoever has to support the result at three in the morning. Where the
attack landed, the correction is what appears below. §7 lists what neither pass could
settle.

Checked directly, not delegated:

- **The `mobilePlayerInfo` chain in §2.3 is real and complete.** `Voice.tsx` handles that
  branch above the sender check; `Voice.tsx:649-652` then broadcasts
  `{ gameState, lobbySettings }` — every player's coordinates, impostor flag and vent
  state — to the room `<code>_mobile`; the server relays `signal` to any `to` without
  checking lobby membership, and its `join` handler accepts an arbitrary string as a
  room name. The four links close into a working attack on 1.0.2.
- **The OBS secret** is `Math.random().toString(36).substr(2, 9).toUpperCase()` at
  `Settings.tsx:1147`, and the same payload class is emitted to a room named by it.
- **`elevate.exe` does ship** in a real installed 1.0.2, 107,520 bytes. §6 question 7
  can be answered without further investigation.
- **Hardware acceleration is already disabled** unconditionally on Linux and on demand
  on Windows, at `src/main/index.ts:37-39`. The GPU fallback argument rests on this
  project's own field experience, not on someone else's issue tracker.
- **`http:` server URLs are accepted** at `ServerURLInput.tsx:17`, so signalling can run
  in cleartext today.

---

**Extends `04-implementation-plan.md`. Does not replace it.** Phase identifiers `P0`–`P7` and gates `G1`–`G3` keep their existing meaning; this document adds a pre-port hardening track (`H1`–`H3`), two new phases (`P8`, `P9`), two new gates (`G0`, `G4`), amendments to `G2` and `G3`, and scope deltas against existing phases written as `P0+` … `P7+`.

Every recommendation below survived an adversarial review by someone who would have to support the result. Where that review found a recommendation unsound or incomplete, the review's correction is what is written here — the original proposal is named only where knowing what was rejected matters.

---

## 1. Decisions

Four verdicts: **switch** (do it, scheduled below), **switch later** (right answer, wrong time, scheduled in `P9` or gated), **keep** (the plan is right, or the proposed change was killed), **measure first** (a decision that cannot honestly be made from a document).

### 1.1 Signalling and the server

| # | Question | Decision | Reason |
|---|---|---|---|
| S1 | Add a second raw-WSS `/v2` path alongside `/socket.io/`? | **keep** | It deletes nothing — the server keeps the Socket.IO parser forever for 1.x and the OBS page, so both stated benefits are never realised; a `serde` enum inside the existing event layer buys the typed contract with no second path ([README.md:26-28](../../README.md) invites third-party servers, so "once operators upgrade" is never) |
| S2 | Lobby registry: socketioxide rooms, or the server's own structure? | **switch** | socketioxide rooms hold only socketioxide sockets; owning the map also fixes three live bugs (`connectedCount` drift, 9-char rooms never left, and no "is X a co-member of Y" predicate) — worth doing even though S1 was rejected |
| S3 | Constrain the `signal` envelope (`to` is a co-member, `to != from`, size cap)? | **switch** | [index.ts:336-347](https://github.com/greluc/AnotherCrewLink-Server/blob/master/src/index.ts) never checks `code`; one inbound message fans out to fifteen with no limit, and any socket knowing a 6-char code can address the room |
| S4 | Rate-limit `signal` at 100 per 10 s? | **measure first** | The arithmetic fails: [peer.ts:69-72](../../src/renderer/peer.ts) emits one `signal` per ICE candidate, unbatched — 14 peers × 10–25 candidates is 2–3× the budget, and the overflow is dropped silently |
| S5 | `mobilePlayerInfo` accepted from any socket | **switch** | [Voice.tsx:1279-1291](../../src/renderer/Voice.tsx) returns before the `from` guard at 1292; the leaked payload is a live positional wallhack plus impostor identity, not (as first framed) a pointer leak |
| S6 | Should the server parse the SDP it relays? | **keep** | Adding a text parser on unauthenticated input to protect a parser that must run anyway; opacity is also what lets 1.x and 2.x SDP cross unchanged |
| S7 | JSON or a binary encoding on the wire? | **keep** | Zero binary payloads today; the dominant payload is an SDP text blob; a transcode leg is where mixed lobbies break |
| S8 | Public lobby list: socket events, or `GET /lobbies` + SSE? | **switch** | Read-mostly, no session, no peer identity — but the stream needs a 15–30 s heartbeat and `Last-Event-ID`, or every reverse-proxied deployment cuts it at nginx's 60 s default and the list goes silently stale |
| S9 | Lobby browser opens a second Socket.IO connection | **switch** | [LobbyBrowser.tsx:108](../../src/renderer/LobbyBrowser/LobbyBrowser.tsx) runs in its own `BrowserWindow` while [Voice.tsx:872](../../src/renderer/Voice.tsx) is live, so `connectionCount` is wrong by 2× — fix by IPC reuse in 1.x, independent of SSE |
| S10 | `join_lobby` ack → `GET /lobbies/{id}/code`? | **switch** | Not for protocol simplification (there is no `/v2`) but for two real bugs: the double `callbackFn` at index.ts:274-287 and the missing `typeof` guard that reaches `uncaughtException` |
| S11 | Constrain `setHost` on the server? | **switch** | index.ts:218 checks only that the sender is in the lobby it names; first-claimer-wins released on leave, **enforced from the `H3` release** — the log-then-enforce staging went with the envelope decision of 2026-08-24 (question 3) |
| S12 | Client-side lobby-settings authorisation | **switch** | The half that actually protects users, including on third-party servers: refuse settings while `gameState.hostId` is 0 rather than falling back to `serverHostId` |
| S13 | Move lobby settings off the data channel? | **keep** | The server cannot forge them today; moving them hands a third party control of every player's audio range, and the benefit is unavailable for the whole window in which 1.x exists |
| S14 | WebTransport / gRPC / MQTT / HTTP long-poll | **keep** (rejected) | Each is a simultaneous flag day against 1.x, third-party servers and the browser OBS page, with no negotiation path; none can be justified on latency because signalling is not on the audio path |
| S15 | `rust_socketio` 0.6.0 (2024-04-16) | **switch** | 28 months stale, would be the workspace's one knowingly-unmaintained crate on unauthenticated input ([crates.io](https://crates.io/api/v1/crates/rust_socketio)); the required subset is ~440 lines and byte-identical on the wire |
| S16 | Engine.IO polling transport on the server | **switch** | Both clients already pass `transports:['websocket']`; polling is served to nobody legitimate once S20 drops the mobile promise, and it carried [GHSA-r635-g3xr-vw7x](https://github.com/advisories/GHSA-r635-g3xr-vw7x) (HIGH), which goes with it |
| S17 | Inbound payload cap | **switch** | The real Node default is `maxHttpBufferSize` **1e6**, not 100 KB ([socket.io docs](https://socket.io/docs/v4/server-options/)); on the Rust side socketioxide's `max_payload` governs **`emit()`**, so it is an outbound guard and a separate inbound check is needed |
| S18 | Tighten `cors.origin` from `*` | **keep** (drop the change) | The WebSocket upgrade is not subject to browser CORS and every non-browser client sends no `Origin`; it is the one part that can break the OBS page for no gain |
| S19 | OBS secret: joinable room name, or bearer capability? | **switch** | [Settings.tsx:1147](../../src/renderer/settings/Settings.tsx) uses `Math.random().toString(36).substr(2,9)` — not a CSPRNG, ~46 bits, and can return fewer than 9 chars, which silently disables the overlay at Voice.tsx:659 |
| S20 | Keep the `mobileHost` / 4.x-protocol promise in §3.5? | **switch** (drop the promise) | Mobile `socket.io-client` defaults to `["polling","websocket"]`, so a websocket-only server rejects its handshake at the first request. §3.5 and S16 cannot both hold and S16 holds: the server ships websocket-only, and §3.5's undertaking that a future 4.x mobile client keeps working is **deleted, not softened** — decided 2026-08-24 |

### 1.2 Media transport

| # | Question | Decision | Reason |
|---|---|---|---|
| M1 | WebRTC, or quinn / iroh / libp2p / Noise? | **keep** | A 1.x Electron peer speaks Chromium WebRTC and cannot be changed; a mixed-transport mesh doubles the hostile-packet surface it was meant to shrink |
| M2 | `webrtc` 0.20.x or `str0m` 0.23.1? | **measure first** | 0.20.0 is a sans-IO rewrite whose own announcement says to expect a real port ([webrtc.rs](https://webrtc.rs/blog/2026/07/31/announcing-webrtc-v0.20.0.html)); neither crate has automated Chromium interop, and Chromium interop is the whole constraint |
| M3 | Opus FEC: a flag, or an RTCP feedback loop? | **switch** | libwebrtc emits FEC only once RTCP reports loss; a Rust client that sends no RR also stops the *Chromium* peer emitting FEC, so 1.x↔2.x degrades in **both** directions and looks like a 1.x bug |
| M4 | Opus FEC **recovery** on the receive side | **switch** (G2 criterion) | The send-side loop is half the problem; the receiver must call `decode(..., fec: true)` driven by the jitter buffer's loss signal, and `neteq` 0.9.1's documented surface says nothing about it — this may change the buffer choice, so it belongs at G2, not G3 |
| M5 | Build RR generation from scratch? | **keep** (use the crate) | `rtc` 0.6.0 ships a `ReceiverReportInterceptor` exposing loss, jitter and RTT; the job is to route the stat, not rebuild the reporter |
| M6 | Google Congestion Control | **keep** (record the omission) | A 24–32 kbps audio-only stream has nothing to congestion-control; neither crate ships an estimator, so it is 6+ weeks of build-it-yourself |
| M7 | Three-step Opus bitrate ladder | **switch later** (not in 2.0) | It fights M3 on the same input with the opposite sign — below ~16–20 kbps libopus carries no meaningful LBRR, so the ladder's bottom rung disables the FEC loop exactly when it is needed; and it is the one behaviour with no Electron reference to measure against |
| M8 | Data-channel-open as the connect signal | **switch** | Voice liveness depends on SCTP for no reason; use `connectionState === 'connected'`, zero interop cost |
| M9 | `setTimeout(..., 1000)` before the host's settings push | **switch** (in 1.x) | A peer whose channel is slow past that window silently runs default proximity rules; retry-until-acknowledged fixes it on the existing transport with no protocol change |
| M10 | Move lobby settings / radio to the socket, drop SCTP | **switch later** (`P9`) | The benefit is structurally unavailable while any 1.x client is in the lobby, and during the mixed window two clients can hold the radio simultaneously, one per transport |
| M11 | Full mesh, or a server-side SFU? | **keep** | The two claims are mutually exclusive: the transparent design saves **no** uplink and **no** encoder CPU (the client still opens one PeerConnection and one Opus encoder per socket id); the 14× design cannot be transparent for 1.x. Also makes the server a single point of failure for calls in progress |
| M12 | Transparent server relay for reliability only | **measure first** | The one genuinely free idea in that area — but measure it against the coturn fallback that already provides most of it, and do not commit a 6–8 week SFU estimate to the document |
| M13 | `neteq` 0.9.1 | **keep** | Neither transport crate ships a jitter buffer, so this is owned regardless; add the M4 check and tune target delay jointly with NACK |
| M14 | Negotiate `a=rtcp-fb:111 nack` | **measure first** | Buffer depth sufficient to make NACK useful over a 60–100 ms RTT can consume G2's entire 30 ms latency budget; decide the two together |

### 1.3 Distribution, signing and update integrity

| # | Question | Decision | Reason |
|---|---|---|---|
| D1 | minisign, TUF, Sigstore, or no auto-update? | **switch** to minisign over a signed manifest | Verifiable offline, zero-dependency verifier ([minisign-verify 0.2.5](https://crates.io/api/v1/crates/minisign-verify)), and already the shape Tauri v2 ships at desktop scale; Sigstore proves provenance, not authorisation, and [sigstore-rs still says it does not verify attestations](https://docs.rs/sigstore/latest/sigstore/) |
| D2 | Freeze protection (reject a manifest older than N days) | **keep** (drop it) | The same fleet-wide time bomb TUF was rejected for, plus a dependency on the user's system clock |
| D3 | Rollback protection (monotonic version) | **switch**, user-bypassable | Otherwise it blocks the documented 2.0→1.x downgrade path |
| D4 | Revocation: `min_acceptable_bundle_version` + "reset to embedded" | **keep** (drop the version floor) | Signed revocation of the offsets bundle falls away with the signature (§2.1). What survives is the manual half: a "reset offsets to embedded" user action, and reverting the mirror. That is slower and needs a human on each affected machine, which is the price of the decision and not a free consequence of it |
| D5 | Two embedded public keys, operational key offline | **switch**, first release only | There is no revocation for a key baked into a binary; retrofitting protects only people who install after the retrofit |
| D6 | `self_update` 0.44.0 | **keep** (drop it) | Its `signatures` feature is zipsign over tar/zip only — it verifies nothing about an NSIS `.exe` or an AppImage, while dragging in eleven non-optional deps |
| D7 | cargo-dist for installers | **switch** to archives-and-release only | Its installer set is `{shell, powershell, npm, homebrew, msi}` — [no NSIS backend, no AppImage backend](https://axodotdev.github.io/cargo-dist/book/installers/index.html); MSI needs WiX v3 on Windows and would strand every 1.x client |
| D8 | Installer naming for x64 / ia32 | **switch** | Today's artefact is one dual-arch installer (`--x64 --ia32`); electron-updater's `findFile` prefers a filename containing the literal `x64` or `ia32` and otherwise takes the first `.exe` — misname it and 32-bit users silently get the 64-bit installer |
| D9 | Windows signing: Azure, EV, SignPath, Certum? | **keep** (rejected — no Authenticode) | None of the four is worth its price here: Azure requires US/Canada individual validation and a paid subscription ([FAQ](https://learn.microsoft.com/en-us/azure/artifact-signing/faq)); EV no longer bypasses SmartScreen ([Microsoft, 2026-08-17](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)), which was the only reason to pay for it; and SignPath eligibility turns on whether the memory reader counts as circumventing security measures, which cannot be known without applying. Windows artefacts ship **unsigned** and users keep seeing the unknown-publisher warning — decided 2026-08-24 |
| D10 | `win.publisherName` value | **keep** (the question falls away with D9) | `publisherName` only matters once something is signed. With no certificate, `NsisUpdater.verifySignature` finds no name in `app-update.yml` and returns null, exactly as it does today — there is no CA switch to survive and nothing to brick |
| D11 | Separate `acl-updater` binary | **switch** | Verification credibility: a verifier inside a process that also parses SDP, decodes RTP through three pre-1.0 crates and reads another process's memory is not a trust boundary |
| D12 | Never install an update while elevated | **switch**, ship in 1.0.3 | Today `quitAndInstall()` runs an **unsigned** downloaded installer with the current token, silently, and `verifySignature` returns null because no signing is configured |
| D13 | Linux updater shape | **switch** (verify-then-replace in-AppImage) | There is no elevation and nothing to install on Linux; this is a second update code path the project chooses to own, and it must be written down |
| D14 | GitHub immutable releases | **switch** — **enabled 2026-08-24**, verified through the API | [GA since 2025-10-28](https://github.blog/changelog/2025-10-28-immutable-releases-are-now-generally-available/); closes post-publication asset substitution and is the only item here that protects the existing 1.0.x fleet. A published `latest.yml` can no longer be edited, which settles D16 |
| D15 | Build-provenance attestations | **switch**, as a third-party signal only | An attacker with repo write access produces a valid attestation |
| D16 | Staged rollout via `stagingPercentage` | **switch** to sequential tagged releases | Settled by D14 rather than chosen: `latest.yml` is a release asset and immutable releases froze it on 2026-08-24, so `stagingPercentage` is not a mechanism this project has. Each rollout step is its own tag, build and manifest; the surviving rollback is re-marking 1.0.2 as *Latest*, which `GitHubProvider` resolves via `/releases/latest` |
| D17 | NSIS per-machine install | **measure first** | A silent per-machine install from the 1.x client's unelevated token only works via an undesigned error-code fallback to `elevate.exe` in the *shipped 1.0.2's* resources — verify it exists in a real install first |
| D18 | MSIX | **keep** (rejected) | Cannot host `requireAdministrator` without the restricted `allowElevation` capability, and virtualises the filesystem an app whose job is to reach outside itself |
| D19 | winget | **switch** | Not a format but a channel, and its `InstallerSha256` lives in a repository we do not control — an independent integrity check plus an update path that does not involve the app modifying itself |
| D20 | Flatpak | **keep** (rejected) | Its PID namespace makes `process_vm_readv` against the game impossible; the only workaround is a documented sandbox escape |
| D21 | Linux tarball + documented `setcap cap_sys_ptrace+ep` | **switch** | On the common `ptrace_scope=1` default the client cannot read the game at all; an AppImage that silently fails is worse than a documented step |
| D22 | 1.x sunset date | **switch** — no date; the 1.x wire format is switched off **when 2.0 ships** | A date published years ahead is either too early to keep or too late to matter, and an open-ended dual stack is the same thing with the decision deferred. Binding the switch-off to the 2.0 release makes `P8`'s fleet migration a prerequisite of that release rather than a follow-up to it, which is the ordering the plan needs anyway — decided 2026-08-24 |

### 1.4 Third-party runtime data and local state

| # | Question | Decision | Reason |
|---|---|---|---|
| T1 | Offsets fetched from an unpinned branch HEAD of a 3-star repo | **switch** (mirror + reviewed sync + embed + validate) | This is the highest expected-loss item in the project: it needs no compromise of our infrastructure, and it yields arbitrary reads plus an arbitrary-location `E9 rel32` write into `GameAssembly.dll` on a client that may be elevated. It also violates the plan's own §7.6 policy. The bundle is **not signed** — see §2.1 for what that buys and what it leaves behind |
| T2 | Pin offsets to a commit hash instead | **keep** (rejected) | Forces a full client release inside the emergency window after an Among Us update. The *mirror* still pins its upstream input by commit; it is the client that must not be pinned |
| T3 | Structural validation of offsets on every load | **switch** | With no signature (T1) this is the only content check there is, so it carries alone the question a signature never answered anyway: not "did we intend this" but "can this do harm". The common real case — an upstream generator producing garbage — was never a signature problem |
| T4 | `broadcastVersion` round-trip as a self-test invariant | **keep** (drop it) | Tautological: `broadcastVersion` is read once through `lookup.json`'s own pattern, so there is no independent second path to cross-check |
| T5 | Verify prologue bytes before patching | **switch**, with an "already patched by us" state | The constants already exist in our source (`55 8B EC 56 8B 75 08` at GameReader.ts:657); but `initializeoffsets()` can re-run against a live already-patched process, and a naive check then kills the mod stamp until the user restarts the game |
| T6 | ~~Compare the 5-byte patch span or the full replayed prologue?~~ Moot: the injection path was removed 2026-08-24 | ~~**switch** to the full 7- and 6-byte originals~~ | The instruction at +4 straddles the patch boundary; checking only the overwritten bytes lets a build through whose shellcode return lands mid-instruction |
| T7 | Silent fallback to `lookup.versions.default` on an unknown build | **switch** to read-only + a log line | Banner only on self-test failure — many Among Us builds ship without moving offsets, so a "not supported" banner would cry wolf |
| T8 | Hat collection commit pin | **keep** | A jsDelivr `/gh/…@<40-hex>/` path is content-addressed and `immutable`; the maintainer got this one right |
| T9 | Self-generated SHA-256 hat manifest | **switch**, 2.x only | `generate:///` is **not** the chokepoint — [cosmetics.ts:102](../../src/renderer/cosmetics.ts) returns bare URLs for every non-multi-colour hat, decoded by Chromium directly; the manifest is ~340 KB over 2,817 blobs |
| T10 | Decode limits + per-image panic isolation | **switch** | The port moves PNG decode from Chromium's fuzzed, sandboxed renderer into a possibly-elevated main process; a 4 GB decompression bomb in a 40 KB file is an OOM kill of the client |
| T11 | Ship ~30 hats in-binary as a GC fallback | **keep** (drop it) | Contradicts the correctly-argued don't-vendor position; detection via the manifest is the valuable half |
| T12 | Settings store: JSON, TOML, redb, sled, SQLite? | **keep** JSON via serde | Neither documented settings loss was caused by the format — [5502c47](../../src/main/hook.ts) records that schema rejection and corrupt-config wiping were ruled out |
| T13 | `schema_version` + migrations + atomic write + split volume map | **switch** | `conf` 15 already writes via `atomically`; the naive `serde_json::to_writer(File::create(..))` translation silently regresses crash-safety |
| T14 | Audio device identity | **switch** to `cpal::DeviceId` **primary** | The claim that cpal has no opaque identifier is false for the pinned version: [cpal 0.18.2](https://docs.rs/cpal/0.18.2/cpal/) documents `DeviceId` as "a stable identifier … across program runs, device disconnections, and system reboots where possible". Name-primary reintroduces the bug for two identically-named endpoints |
| T15 | Retire the label-recovery fallback as dead code | **keep** it, as tier 3 | Windows prepends an enumeration counter (`2- Arctis 7`) on re-plug to a different port and the endpoint GUID changes with the instance — so neither key survives the exact event the original bug cites |
| T16 | Surface the device fallback instead of falling back silently | **switch** | The silent fallback is what made the original bug invisible; this is the item's real value |
| T17 | Certificate pinning / server signing key / TOFU | **keep** (rejected) | WebRTC keys SRTP from DTLS fingerprints relayed by the server, so a configured-but-hostile server is already a full media MITM; pinning defends a threat that is not the one implied |
| T18 | `http:` server URLs permitted | **switch** to https-only + resolved-loopback exemption | [ServerURLInput.tsx:12-23](../../src/renderer/settings/ServerURLInput.tsx) explicitly admits `http:`, which with `transports:['websocket']` yields cleartext `ws://` signalling |
| T19 | Relay-only as a floor the server cannot lower | **keep** — already correct | [Voice.tsx:1112-1115](../../src/renderer/Voice.tsx) already applies `forceRelay()` on top of the server's config; the work is a named regression test so the port does not lose it, not a feature |
| T20 | ICE config scheme allowlist and bounds | **switch**, enforce directly | `isUri` falls through to `new URL(value)`, so `data:` and `file:` pass; and the proposed "warn-only so we learn" step is fiction — there is no telemetry, the warnings land in a local log nobody sends |

### 1.5 Client runtime shape

| # | Question | Decision | Reason |
|---|---|---|---|
| R1 | egui or iced for the main UI? | **measure first** | egui has no bidi and no complex-script shaping ([#1016](https://github.com/emilk/egui/issues/1016), open since 2021) — and [languages.ts:50,102,130](../../src/renderer/language/languages.ts) renders the picker itself in native script, so an Arabic speaker cannot find their language to switch away. But it is **3** locales, not 7 (CJK is font loading), and iced is a single-maintainer project with a 15-month release gap versus Rerun-backed monthly egui |
| R2 | GPU fallback chain | **switch**, reshaped | [index.ts:38-40](../../src/main/index.ts) already disables hardware acceleration unconditionally on Linux and on demand on Windows — first-party field evidence stronger than any external issue tracker |
| R3 | A glow / GL rung in the chain | **keep** (drop it) | glow needs GL 3.3 / ES 3.0; Windows without a vendor driver offers software GL 1.1, so it does not save the RDP or bare-VM cases it exists for |
| R4 | Persist a new "last known good rung" key | **keep** (use `hardware_acceleration`) | A key written by a process in the act of crashing pins users to the slow rung for reasons unrelated to the GPU; migrate the answer the user already gave and make automatic demotion non-persistent by default |
| R5 | One process, or several? | **switch** to **two** | §6.3 (elevated) and §6.2 (pre-1.0 crates parsing attacker RTP) are both in the plan and never joined; a thread boundary is not a privilege boundary, and today's Electron overlay is its own `BrowserWindow` so the plan is also a straight availability regression |
| R6 | Where does the overlay process live? | **switch** — in the **elevated helper**, not the UI | [windows.c:428](../../native/electron-overlay-window/src/lib/windows.c) exists precisely to probe UIPI, and out-of-context `SetWinEventHook` against a higher-integrity process is blocked — an unelevated overlay stops following an elevated game, which is the configuration the README is about |
| R7 | winit alone for the overlay, or port `windows.c` / `x11.c`? | **keep** (port them) | winit supplies the transparent topmost surface — roughly 10% of the job |
| R8 | Port the UIPI access check | **switch** | It is the difference between "overlay is broken" and an accurate message about elevation |
| R9 | Exclusive-fullscreen detection | **switch** | With Fullscreen Optimizations off, a layered window will not appear; the alternative is a swapchain hook, which this project must not ship |
| R10 | Wayland overlay | **keep** (declare unsupported) | `set_window_level` is a documented no-op on Wayland ([winit #899](https://github.com/rust-windowing/winit/issues/899)); Mutter does not implement wlr-layer-shell; gamescope's single overlay slot is permanently held by mangoapp ([MangoHud #775](https://github.com/flightlessmango/MangoHud/issues/775)) |
| R11 | Detect Wayland via `XDG_SESSION_TYPE`? | **switch** to the live winit backend | `XDG_SESSION_TYPE` describes the session, not the backend the process got — gating on it greys out the overlay for XWayland users who work today |
| R12 | Convert 37 locales from i18next JSON to Fluent | **keep** (drop the conversion) | Measured: 37 locales × 128 keys, zero key diff, **zero placeholders, plurals or selectors** — every distinguishing Fluent feature is unused. And Fluent identifiers cannot contain dots, so "content untouched" is true of the values and false of the keys, which is what Crowdin and every call site key on |
| R13 | Locale key-parity CI ratchet vs new English-only keys | **measure first** | R10/R11 add three new strings; the ratchet as proposed breaks in 36 locales the day they land — pick the allow-list rule before either lands |
| R14 | xilem / makepad / freya / slint / no framework | **keep** (rejected) | xilem 9,946 total downloads; makepad-widgets no release since 2025-05-13; slint's software renderer is [documented as western-scripts-only](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/), so it fails the same 37-locale test on the fallback path |
| R15 | Idle CPU / RSS / startup / overlay frame cost | **measure first** | §2.4's order-of-magnitude claim is explicitly unmeasured; and the baseline is not one configuration but three, because of R2 |
| R16 | Blocking performance metric | **switch** | Add audio callback overruns and dropped frames per minute **with the GUI visible, the overlay up and the game running** — G2 measures audio offline with no GUI and no game, which is the only configuration users never run |

---

## 2. The switches worth making, in order of value

### 2.1 The offsets supply chain — 5.5 developer-weeks

**Wrong today, minus one item.** The host moved on 2026-08-24: [`src/main/offsetStore.ts`](../../src/main/offsetStore.ts) now fetches `https://raw.githubusercontent.com/greluc/AnotherCrewlink-Offsets/main/lookup.json`, with `cdn.jsdelivr.net` as a second mirror, so the .NET-binary supply chain described below is no longer in the path and the account that can change what every client reads is ours. Everything else in this item stands: it is still an unpinned branch HEAD, still fetched with no pin, hash, signature or validation, still falling back to an unauthenticated local cache. That repo's own hourly workflow downloads and executes a .NET binary from the `latest` release of a second repo whose newest release is dated 2022-12-10, on a runner holding `GH_TOKEN` and Steam credentials. Every number in the result is then used unvalidated: `player.bufferLength` is an unbounded allocation at GameReader.ts:246; `fixedUpdateFunc` and `modLateUpdateFunc` become attacker-chosen RVAs at GameReader.ts:749/752, where a 5-byte `E9 rel32` is written into `GameAssembly.dll`; `disableWriting` is the attacker's own kill switch for the safety gate at GameReader.ts:605. `04-implementation-plan.md` §4.4 item 3 ports this verbatim, which directly contradicts §7.6 item 4.

**Replacement.** Mirror the upstream tree into `greluc/AnotherCrewLink-Offsets` and sync it by scheduled pull request, so a human sees the diff before it reaches anyone (81 blobs / 284,896 bytes — a review a few times a year). Each sync PR pins the upstream tree it copied by commit SHA and records that SHA in the bundle, so what the client reads is never a third party's branch HEAD. Build one canonical bundle carrying `bundle_version` and a target minimum client version. Embed the build-time bundle with `include_bytes!` as the floor. Run a total structural validator on **every load**, including from the cache, and check the full replayed prologue before any patch.

**Not signed, and why.** The bundle carries no signature. An Among Us update is not an event, it is a burst — four upstream cycles in one evening on 2026-06-06 — and every client is out of the game until a bundle that matches the new build arrives. Inserting a human with an offline key between that burst and the users is precisely what would keep them out: the key ceremony, the second designated signer, the ~100 lines of minisign format parsing that must stay byte-compatible with `minisign-verify`, and the availability floor under all of it. Merging a reviewed pull request is a thing one person can do from a phone at midnight; a key ceremony is not. Signing was the more expensive half of this item and it bought the smaller share of the risk.

**What that leaves.** With no signature, whoever can push to the mirror can change what the client reads. The mirror's branch protection and the account that owns it are therefore part of the trusted set, and they are a GitHub credential rather than an offline key — a real residual risk, stated as one. What the change does close is the larger of the two problems: the client no longer follows the unpinned branch HEAD of a third-party repository whose own hourly workflow executes a .NET binary from a 2022 release on a runner holding `GH_TOKEN` and Steam credentials. Protect the mirror accordingly: required review, no force-push, no bypass for administrators.

**Security.** Reduces the trusted set from seven parties to two: the mirror repository and the human reviewing its sync PR. Closes the arbitrary-location code write and the unbounded `readBuffer`, and — because the validator runs at load and not at download — bounds local tampering with `offsets.json` in `userData` to whatever the validator can catch, which is structure and range, not authorship.

**Performance.** Faster in the common case: the first launch no longer blocks on two HTTPS round trips with a 10 s timeout and three retry rounds before the game can be read. Validating a 285 KB bundle is sub-millisecond; the binary grows ~30 KB gzipped.

**Abandon if.** The timed drill against the next real Among Us update cannot publish a bundle within 6 hours of upstream. That is the availability price of inserting a reviewer into the chain, and it is the price the decision above was taken to keep small. If the drill fails, the retreat is auto-merge on the mirror with post-hoc review — the mirror, the pin, the embedded floor and the validator all survive that retreat, which is the property that makes it an acceptable one.

### 2.2 Update integrity, end to end — 8.0 developer-weeks

**Wrong today.** `src/main/index.ts` calls `autoUpdater.checkForUpdates()` at startup and `quitAndInstall()` on download, which spawns the installer with the current process token. `isAdminRightsRequired` is false for a per-user install, so there is no UAC prompt; `verifySignature()` reads `publisherName` from `app-update.yml`, finds nothing because `electron-builder.yml` configures no signing, and returns null. If the user launched elevated to match the game — which [README.md:37](../../README.md) instructs — an unsigned downloaded binary executes as administrator, silently. The SHA-512 that gates it comes from `latest.yml` on the same host as the artefact. Separately, the app installs per-user into `%LOCALAPPDATA%\Programs` and is then told to run elevated — an elevated process loading from a user-writable path.

**Replacement.** Immutable releases — on since 2026-08-24; the elevation gate in 1.0.3; **no Authenticode**; minisign over a signed manifest with rollback protection but no freeze rule; two embedded public keys with the operational key held offline, verified in-process and never by shelling out; a separate `acl-updater` binary on Windows and verify-then-replace inside the AppImage on Linux; hand-built NSIS and AppImage with literal `x64`/`ia32` tokens in the filename.

The signature is on the manifest, not on the artefact's PE header, and that is a deliberate split rather than an oversight. A release is a planned event on our own schedule: nobody is locked out of a game while the manifest is signed, which is exactly the pressure that made signing wrong for the offsets bundle in §2.1 and leaves it right here.

**Say plainly what this does and does not cover.** The minisign signature proves that the installer a user is about to run is the one this project published, and it proves it offline, against a key baked into the binary, without trusting GitHub. It does nothing about SmartScreen. Windows artefacts are unsigned, every fresh download shows the unknown-publisher warning, and users will keep being told by their own operating system that this is untrusted software — for a client whose README already instructs people to run it as administrator, that is the least comfortable line in this document. It is accepted because Authenticode's remaining benefit is reputation, EV no longer short-circuits reputation, and no certificate this project can obtain changes what an attacker who has not compromised the signing key can do.

**Security.** Removes unprompted administrator-level code execution from the update path. Reduces what must be trusted to verify an update from an entire Electron process to a few hundred auditable lines. Makes compromising GitHub insufficient to push code to users.

**Performance.** None.

**Abandon if.** Nothing left to abandon: the CA question was the one external dependency in this item and it is closed. What remains is entirely within the project's own pipeline, which means the failure mode is schedule rather than a third party declining. The one thing that must not slip is the embedded key: it ships in the first release that verifies anything, because a key retrofitted later protects only people who install after the retrofit.

### 2.3 The `signal` envelope and the three features hung off it — 2.5 developer-weeks

**Wrong today.** `io.to(to).emit('signal', {data, from})` treats `to` as a **room name**, not a socket id, and never checks that the sender is in any lobby. Three consequences traced end to end: a socket that never joined anything can broadcast to any lobby whose 6-character code it knows; `mobilePlayerInfo` is handled *above* the `from` guard at Voice.tsx:1292, so any lobby member can flip `mobileRunning` on every other client and then receive their full `AmongUsState` at 5 Hz — live `x`, `y`, `isImpostor`, `isDead`, `inVent` per player, which is a working wallhack plus impostor disclosure; and `setHost` is unauthenticated, feeding `parsedHostId` whenever `gameState.hostId` is 0, after which a lobby member pushes `maxDistance: 1000` and hears the whole map.

The exposure window for the `setHost` cheat is not the niche 32-bit population first assumed — `hostId` is read by a plain pointer chain at GameReader.ts:225 on both bitnesses, and returns 0 whenever the chain fails, which is exactly what happens for the days between an Among Us patch and an offset update.

**Replacement.** Three envelope rules (`to` is a current co-member, `to != from`, 64 KB cap), first-claimer-wins host held until leave, and a client that refuses lobby settings while `gameState.hostId` is 0 rather than trusting `serverHostId`. **Enforced from the H3 server release.** No logging period, no `SIGNAL_STRICT` flag to flip later, no 0.1% threshold to wait for.

That is a deliberate break, taken because the alternative was worse. The counter was never going to reach a floor: 1.x updates through electron-updater with no forced upgrade, so a flag waiting on the fleet is a flag that never flips, and the vulnerable window stays open for as long as the last un-updated client survives — which is indefinitely. Shipping the rules on trades a knowable one-time break for an unbounded one.

**What breaks, and for whom.** Every client older than 1.0.5 loses the OBS overlay feed and the mobile relay, at the moment of the server release, at once. Voice, lobbies and the browser are untouched. Anyone who never updates loses those two features permanently and keeps talking to their friends. The rejection counter still goes on `/health`, but as an operational signal after the fact, not as a gate before it.

**Two orderings that are now hard prerequisites, not preferences.**

1. The **OBS overlay page must be deployed and verified before the server release.** It lives in neither repository and serves every client generation simultaneously, so it is the one component that cannot be rolled back per-client. If it does not already speak `obs_state` when the server starts refusing the legacy feed, every streamer's overlay goes blank at the same instant, including the ones on 1.0.5.
2. **1.0.5 must be in the field before or with the server change.** It is the client release that speaks the new events, and it is what makes the break "clients older than 1.0.5" rather than "all clients".

Neither is a scheduling nicety. Get them the wrong way round and the enforcement release is an outage rather than a fix.

**Security.** Closes an unauthenticated remote game-state disclosure, an OBS-feed injection path, an unmetered 15× fan-out amplifier, and a lobby-scoped audio cheat reachable without touching WebRTC — and closes them on a date, rather than on a condition that may never be met.

**Performance.** Bounds worst-case fan-out per inbound message.

**Abandon if.** Nothing. The condition the old escape hatch waited on has been removed on purpose. The client-side halves (S5, S12) still ship and still protect users on third-party servers running `S1` forever, and they remain the part that must not be cut — but they are no longer a consolation for a server rule that never arrives.

### 2.4 The Opus FEC feedback loop, both directions — 2.0 developer-weeks

**Wrong (about to be).** The plan treats in-band FEC as a flag. libwebrtc only emits Opus FEC once RTCP receiver reports tell it there is loss, and practically only when NACK is also negotiated. A Rust client that sets the flag but sends no RR achieves nothing — and because the Chromium peer then never learns it is losing packets either, it stops emitting FEC too. On a clean LAN 1.x↔2.x is perfect; at 3% loss, 1.x↔1.x sounds normal and 1.x↔2.x sounds broken in **both** directions, intermittently, for one pair. `G3` as written passes on a clean network.

**Replacement.** Read the `ReceiverReportInterceptor`'s loss fraction rather than rebuilding RR; drive `OPUS_SET_PACKET_LOSS_PERC` with hysteresis; **and** implement the receive side — `decode(input, output, fec: true)` on packet *N+1* to reconstruct *N*, driven by the jitter buffer's loss signal. Verify `neteq` 0.9.1 supports out-of-order FEC recovery as a named `G2` criterion, not a `G3` one.

**Security.** Neutral; clamp reported loss so a lying peer cannot drive the encoder anywhere harmful.

**Performance.** FEC costs 20–30% extra bitrate while active — negligible against 8–14 concurrent streams. The gain is that a 5% loss link stays intelligible.

**Abandon if.** `neteq` cannot signal loss to the decoder in a way that permits FEC recovery, and vendoring the reference NetEQ exceeds the two-week `G2` exit window. That is a `G2` stop-the-port decision, not a `P4` inconvenience — which is precisely why it moves to `G2`.

### 2.5 The two-process split, with the overlay in the elevated half — 2.0 developer-weeks

**Wrong (about to be).** §3.2 puts the elevated privilege, the RTP parsers from three pre-1.0 crates, remote hat fetch and decode, the auto-updater and a wgpu context in one address space, and calls it "Electron's process split replaced by a thread split". `catch_unwind` around a peer does not contain a memory-safety bug in a C++ APM, and it does nothing about the process holding debug-level access to the game. It also collapses a boundary that exists **today**: the overlay is its own `BrowserWindow` and Chromium's GPU work is out-of-process, so today an overlay fault or a driver crash does not kill voice.

**Replacement.** `acl-helper`, elevated: memory reading, keyboard hook, **and the overlay window** (injection was listed here until it was removed on 2026-08-24). `acl-core`, never elevated: tokio, signalling, WebRTC, audio, GUI. Length-prefixed `postcard` over a named pipe / Unix socket. The overlay is in the helper because UIPI blocks out-of-context `SetWinEventHook` and window manipulation across integrity levels — putting it in the unelevated process breaks it on exactly the machines the README is about. Consequence to design in from the first commit: the overlay receives **pre-rasterised sprites** over the IPC and never fetches or decodes an image itself, so no image decoder enters the elevated process.

**Security.** Elevation shrinks to a process with no listening socket, no HTTP client, no image decoder and no GPU context. The plan's own named fuzzing targets stop running elevated, and the auto-updater loses its debug-privilege neighbour.

**Performance.** Helper→core is a ~200-byte struct at 5 Hz. Core owns the audio ring, so §3.2's no-alloc/no-lock render rule is untouched.

**Launch.** `acl-helper` is started on demand by `acl-core`, with a **per-launch UAC prompt**. No Windows service is installed. The prompt is visible friction once per session, and it is accepted: a service would be an always-resident elevated process with an auto-start entry, installed on every machine to save one click on the machines that play, and it would have to be maintained, upgraded and uninstalled correctly by an installer this project also has to write. The split is therefore available, and §3.2, `06-security.md` and `02-feasibility.md` should read as settled rather than hedged.

**Abandon if.** Nothing. The launch question was the only thing that could have collapsed this item and it is answered. A `--single-process` build is not kept as an escape hatch — an untaken configuration that CI does not exercise stops compiling by month six, and there is now no reason to take it.

### 2.6 Hand-written Engine.IO/Socket.IO client — 3.5 developer-weeks

**Wrong.** `rust_socketio` 0.6.0, published 2024-04-16, would be the workspace's one knowingly-unmaintained dependency, sitting on unauthenticated network input, against a blocking cargo-deny/cargo-vet policy.

**Replacement.** ~440 lines against Engine.IO v4 / Socket.IO v5, websocket transport only, default namespace, no binary attachments — **plus** the network stack the original scoping forgot: Chromium supplies system proxy resolution (WPAD/PAC, `HTTP(S)_PROXY`, Windows Internet Options) and the Windows certificate store for free, and tokio-tungstenite supplies neither. Users behind a TLS-inspecting school or corporate proxy are the same population already forced onto TURN, and the symptom is "won't connect at all". Budget `rustls-platform-verifier` and a proxy resolver as named line items on Windows x64, Windows i686 and Linux.

**Security.** Removes an unmaintained crate from the path that parses unauthenticated server input and shrinks the parser to a fuzzable subset.

**Performance.** Neutral.

**Abandon if.** The conformance suite cannot be made to pass against the real Node socket.io 4.8.3 server within the budget. The rollback is **more time on the hand-written client and a narrower first cut** — not `rust_socketio`, not even for one release. It pulls `backoff` (RUSTSEC-2025-0012) and `instant` (RUSTSEC-2024-0384), both unmaintained with empty patched-versions lists, so there is no fixed version to move to and CI is red from the commit that adds it; `07-dependencies-toolchain.md` §7.5 and `08-dependency-review.md` §2.4 both record it as not usable even briefly. Keeping it in the lockfile as a standing cargo-deny exception would mean carrying two unfixable advisories on the crate that parses unauthenticated server input, against the policy this workspace is meant to demonstrate.

### 2.7 Owned lobby registry — 1.0 developer-week

**Wrong.** A faithful port of `index.ts` inherits `io.to(room)` at every fan-out site, plus three live bugs: `connectedCount` decremented unconditionally at index.ts:139 while index.ts:142 tests actual room size; `leaveroom` only calls `socket.leave` for 4- and 6-character codes, so 9-character OBS-secret rooms and `<code>_mobile` rooms are never left; and the registry cannot answer "is socket X in the lobby socket Y is in" — the exact predicate §2.3 needs.

**Replacement.** `Lobby { members: HashMap<ConnId, Member>, host, public }` with a per-member sink, plus three written `P0` acceptance criteria the original proposal omitted: **bounded** per-member channel with a logged overflow policy and a counter on `/health` (unbounded is a new DoS the Node server does not have; bounded-and-silent surfaces as peers that never connect); **serialise once per encoding, not once per member** (`io.to(room)` serialised once; naive iteration is 15× the CPU on the hot path); and no lock held across an await.

**Security.** Enabling — §2.3's rules are a two-line predicate against this structure and are not expressible against socketioxide rooms.

**Performance.** Neutral with the serialise-once rule; 15× worse without it.

**Abandon if.** Nothing. It is a shape decision made once, before the first line of `lobby.rs`. The risk is not failure but a faithful-port instinct quietly undoing it, which is why it is a written acceptance criterion.

### 2.8 GPU fallback chain — 1.0 developer-week

**Wrong.** wgpu treats device-removed as fatal by default, and the plan specifies no fallback at all. This window sits on top of a game saturating the same GPU on integrated hardware; driver auto-updates happen while people play. The strongest evidence is first-party: `index.ts:38-40` already disables hardware acceleration unconditionally on Linux and on demand on Windows via a shipped `hardware_acceleration` setting in the beta section — the project already found this in the field and already shipped the escape hatch.

**Replacement.** Linux defaults to software, matching today. Windows: wgpu/DX12 → wgpu with `force_fallback_adapter` (WARP) → CPU rasteriser; **no glow rung**. Migrate the existing `hardware_acceleration` value forward rather than inventing a key. Automatic demotion is non-persistent by default and offers to remember. `--renderer=auto|gpu|software`, documented next to the elevation note.

**Security.** Positive — the software rung removes a GPU driver from a process that may be elevated.

**Performance.** Higher CPU on the software rung; the alternative is no window.

**Abandon if.** With §2.5 in place, device-lost recovery is "let the UI half die and be restarted", which is cheaper and more robust than in-process recovery. If §2.5 is abandoned, in-process recovery under eframe means patching or vendoring it — at which point drop runtime recovery and keep only startup selection plus the flag.

### 2.9 Audio device identity — 1.0 developer-week

**Wrong.** Chromium's `deviceId` is a salted, per-origin, deliberately unstable handle, invalidated by driver updates, re-plugging and salt rotation. The port cannot even produce one, so a mechanical port of `ISettings` would carry two dead fields across.

**Replacement.** `cpal::DeviceId` primary, device name secondary, friendly-name fuzzy match as tier three — because Windows prepends an enumeration counter (`2- Arctis 7 Headset`) on re-plug to a different port and the endpoint GUID changes with the device instance, so neither machine key survives that event. And surface the fallback instead of performing it silently, which is what made the original bug invisible.

**Security.** Neutral; drops a salted cross-origin-correlatable value.

**Performance.** None.

**Abandon if.** Nothing to abandon — a wrong resolution is one dropdown click for the user. Reclassify the risk though: lowest technical blast radius, **highest** support-ticket volume of anything in this document.

### 2.10 Hat decode limits and integrity — 1.0 developer-week

**Wrong (about to be).** The port moves PNG decode from Chromium's continuously-fuzzed, sandboxed renderer into an unsandboxed, possibly elevated main process using `image` 0.25.10. The realistic outcome is a panic or a 4 GB allocation from a 40 KB file, not code execution — which is why limits and panic isolation, not a different decoder, are the answer.

**Replacement.** Per-file and per-session byte caps, request timeout, `image::Limits` with explicit `max_alloc`, off-thread decode with per-image `catch_unwind`. A self-generated SHA-256 manifest (~340 KB over 2,817 blobs), landing in **2.x only** — `generate:///` sees only recoloured hats, so full 1.x coverage would mean reworking how `Avatar.tsx` loads images.

**Security.** Bounds a decompression bomb from an OOM kill to a dropped hat; removes jsDelivr and the TLS path from the set that can influence decoded bytes.

**Performance.** One SHA-256 per image against a network fetch; off-thread decode removes decoding from the frame budget.

**Abandon if.** Nothing. The pin already makes this sixth-ranked; if budget is short, ship the limits and panic isolation and defer the manifest.

### 2.11 The negative switch: do not convert the locales — saves 1.0 developer-week

Measured, not assumed: 37 locale directories, 128 keys each, zero symmetric key difference against `en`, longest value 122 characters, and **zero occurrences** of any interpolation syntax across all 4,736 strings. Every feature that distinguishes Fluent from a flat map is unused. Fluent identifiers cannot contain dots and the corpus is dotted throughout, so §4.8's "translation content is untouched" is true of the values and false of the identifiers — and the identifiers are what Crowdin keys on and what every call site references. Keeping i18next JSON also means 1.x and 2.x consume the identical tree during the beta, one Crowdin project, translators working in one format. Two things the ~150-line loader must carry that the original proposal omitted: per-locale base text direction (a `HashMap<String, String>` loses the metadata that connects this to §2.1's RTL question), and a note that `format!` covers the first formatted string so nobody reopens the Fluent question.

---

## 3. The migration plan

### 3.1 Extended phase map

```
H1  1.x emergency hardening      ──► ships as 1.0.3              2.0 wk   funded
H2  1.x offsets trust chain      ──► G0                          3.0 wk   funded
H3  1.x/Node envelope + OBS      ──► ships as 1.0.5              2.5 wk   funded
                                                       H subtotal  7.5 wk

P0+ Server                        2.0 →  4.0   ◄── funded; decision point at its end
────────────────────────────────────────────── everything below is planned, not funded
P1+ Foundations                   1.0 →  5.0
P2+ Game reader     ──► G1        4.0 →  6.0   (G1 unchanged)
P3+ Audio engine    ──► G2        8.0 → 10.0   (G2 amended: +FEC recovery)
P4+ Transport       ──► G3        5.0 → 10.5   (G3 amended: +loss legs)
P5+ Platform                      3.0 →  6.0
P6+ GUI                          10.0 → 11.5
P7+ Packaging & signing           4.0 →  9.5
P8  Bridge & sunset ──► G4          —  →  4.0
                                                       P subtotal 66.5 wk
P9  Post-1.x cleanup                —  →  3.0   (outside the 2.0 budget)
```

**Only `H1`–`H3` and `P0+` are committed work.** The full port is planned in the order above and priced below, but the decision to continue past the Rust server is taken **after `P0+` ships**, on what building it actually cost, not now on this document. That is not a hedge added to the plan — it is the plan's own logic applied one phase earlier than `G2`: the server is the cheapest phase that produces a real Rust artefact under real load, and the hardening track is worth doing whether or not anything follows it, because it runs on the Electron client and the Node server and protects the fleet that will never see 2.x.

**Total to 2.0: ~74 developer-weeks**, against the existing plan's 37 and against the 77 this document priced before the decisions of 2026-08-24. Those decisions took 3.0 weeks out and none of it was scope: `H2` −1.0 with the offsets key ceremony, second signer and minisign-format parsing gone (§2.1); `H3` −0.5 with the log-then-enforce staging, the flag and the counter watch gone (§2.3); `P7+` −1.5 with Authenticode, the CA application and the `publisherName` work gone (§2.2). Two developers do not halve what is left: `P3` is no longer the sole critical path — `P4` now rivals it.

That number is the union of independently-priced corrections and is honest rather than comfortable. The three largest drivers are not security: `P4` +5.5 because the sans-IO `webrtc` 0.20 rewrite killed the "237 lines map one-to-one" premise behind the original estimate; `P7` +5.5 because cargo-dist cannot build either artefact type this project must keep producing; and `P1` +4.0 because the Socket.IO client moves out of `P4` where it was crowding out the entire WebRTC half. If the whole figure is unacceptable, the cut list is in §6, question 13 — but §2.1, §2.2 and §2.3 are the three that should not be cut.

### 3.2 The hardening track (`H1`–`H3`)

These ship on the Electron client and the Node server, **before and alongside `P0`**. They are not "1.x maintenance while the real work happens" — several are hard prerequisites for the port, and all of them protect the fleet that will never see 2.x. The track is unaffected by the scope decision above: it runs on the shipped Electron client and the shipped Node server, and it is worth doing whether or not anything follows `P0+`.

**`H1` → 1.0.3 (2.0 wk).** Everything here is client-local or repository configuration, so it can ship in any order and needs no server release.

| Change | Ships first | Wire | Rollback |
|---|---|---|---|
| Move the `mobilePlayerInfo` branch below the `from` guard (Voice.tsx:1292) | client only | none | patch release |
| `WireGameState` projection stripping `ptr`/`taskPtr`/`objectPtr` — **mobile path only**; the OBS path at Voice.tsx:664-696 already projects | client only | consumers must tolerate missing fields they never read | patch release |
| Elevation gate: skip `checkForUpdates` and the `update-app` IPC when elevated, notify instead | client only | none | revert the guard |
| Cross-version single-instance lock, named `Local\AnotherCrewLink` — **not `Global\`**, which needs `SeCreateGlobalPrivilege` a standard user does not hold, so it would silently fail in the common case | **must be in the field before any 2.x beta build exists** | none | revert |
| Client-side host hardening: refuse settings while `gameState.hostId` is 0; require the peer to be the `isHost` claimant reported at join | client only | none | revert |
| `actions/attest-build-provenance` in both workflows | repository setting | none | drop the step |
| Node: `typeof callbackFn === 'function'` guard + remove the double `callbackFn` fall-through | server only | none | revert |
| `settings.obsSecret.length === 9` → minimum-length check | client only | none | revert |
| Replace `setTimeout(..., 1000)` before the settings push with retry-until-acknowledged | client only | none | revert |

Two rows left this table on 2026-08-24. **Immutable releases** are already on — enabled and verified through the API that day — so they are a completed action rather than a scheduled one, and every consequence of the frozen `latest.yml` is written into `P8` below as fact. **`win.publisherName`** is not shipped at all: with no Authenticode certificate (§2.2) `verifySignature` has no name to check and returns null, as it does today. That was the one row marked "must be right first time" because a later CA switch would have bricked every install's updater permanently; there is now no CA switch to survive. It is the single cheerful consequence of not signing.

**`H2` → 1.0.4 (3.0 wk) → gate `G0`.** The offsets trust chain, in TypeScript first, because the format must be proven before `P2+` consumes it.

1. Mirror repository + scheduled sync-by-PR workflow, with the upstream commit SHA recorded in each PR and carried into the bundle. Branch protection on the mirror — required review, no force-push, no administrator bypass — is part of the deliverable and not a settings afterthought, because with no signature it is the control that replaces the key.
2. Bundle format with `bundle_version` and a target minimum client version.
3. `offsetStore.ts` reads the mirror rather than upstream, over TLS, and never a third party's branch HEAD.
4. Embedded current bundle as a JSON asset, serving as the floor when the network fails or the mirror is unreachable.
5. Read-side structural validator, running on **every** load including from the `userData` cache, and write-side full-prologue check with the "already patched by us" third state.
6. "Reset offsets to embedded" user action — the manual recovery path that replaces signed revocation, and the reason a bad mirror merge needs a human on each affected machine rather than a republish.

The signing xtask, the key ceremony, the second designated signer, the ~100 lines of minisign format parsing in `node:crypto`, its cross-implementation test vectors and the `ALLOW_UNSIGNED_OFFSETS` escape hatch are all **not in `H2`**. §2.1 records why, and what it leaves behind.

> **Gate `G0` — the offsets chain is pinned, validated and fast enough.**
> 1. A committed malicious-bundle corpus — truncated, structurally malformed, RVAs out of module range, `bufferLength` absurd, `disableWriting` flipped — is rejected with a distinct error each, leaving the previously-held bundle in force.
> 2. Editing the cached bundle on disk between runs is rejected **as far as the validator can catch it**, proving validation at load and not only at download. It catches structure and range; it cannot catch a plausible-but-wrong offset, and that limit is the gate's honest boundary rather than a test to be written around.
> 3. The validator accepts all 81 real upstream files unchanged. *A validator that rejects real data is a self-inflicted outage, and this half matters as much as the first.*
> 4. The floor holds: with the mirror unreachable — DNS failure, a 404 on the pinned commit, an empty cache — the client starts, reads the embedded bundle, and says which bundle it is using rather than falling back silently. The signed design did not need this criterion and this one does: pinning to a mirror we own replaces a third party's availability with our own, and the answer to that is that a failed fetch is never fatal.
> 5. A timed drill against the next real Among Us update publishes a bundle in under 6 hours, recorded.
>
> The revocation drill is gone with the signature: there is no signed floor to supersede a bad bundle from, so recovery is reverting the mirror plus "reset to embedded" on the affected machine, and that is a support procedure rather than a gate criterion.
>
> `P2+`'s offsets work must not start before `G0`; `G1` must still pass byte-for-byte using the embedded bundle, proving the bundle format lost no data.

**`H3` → 1.0.5 + Node server (2.5 wk).** The only part of the track with a wire component, and the one place where the ordering *is* the design. The rules ship enforced — there is no logging period and no flag (§2.3) — so the two consumers must be ready before the server refuses anything, and "ready" means deployed and verified, not merged.

| Step | Side | Ordering |
|---|---|---|
| 1 | **Overlay page** learns `obs_state` while still accepting the legacy `signal`-to-room feed. Deployed and verified **alone**. | **prerequisite of step 3** — it lives in neither repository, serves every client generation at once, and cannot be rolled back per client |
| 2 | **Client** 1.0.5 emits `obs_state`/`mobile_state` and generates CSPRNG secrets. The new secret is **strictly additive** — issued only on an explicit user "regenerate", never as a side effect of an upgrade, because the field failure here is a streamer's overlay going blank mid-broadcast. | **prerequisite of step 3** — in the field before or with it |
| 3 | **Server** adds `obs_state` and `mobile_state` handlers and a capability-scoped subscribe path, **enforces** the three envelope rules and first-claimer host from the same release, sets `transports: ['websocket']` and `maxHttpBufferSize` to 64 KB. | last |

Duration: two release cycles, driven by how long 1.0.5 takes to reach the field rather than by the work. **Version negotiation on the wire:** none — `obs_state` and `mobile_state` are new event names that old clients neither send nor listen for, and at step 3 the legacy `signal`-to-room path stops being honoured for them. **Rollback:** step 3 is a server deploy, revertible in minutes; steps 1 and 2 are additive and roll back by themselves. **What does not roll back:** the break itself is intended, so a client older than 1.0.5 that loses its overlay is not a regression to be reverted — it is the decision arriving.

The websocket-only transport lands here too, and with it the mobile promise in `03-target-architecture.md` §3.5 is deleted rather than deferred (S20). Any mobile `socket.io-client` defaults to polling first and is refused at the handshake; there is no configuration on the server that leaves that door open for a client that does not exist yet.

### 3.3 Scope deltas inside existing phases

**`P0+` Server, 2.0 → 4.0 wk.** Owned registry with the three written acceptance criteria (§2.7). `GET /lobbies/{id}/code` with `Cache-Control: no-store` — a lobby code is the credential that gates entry to a game, and this is now a cacheable GET behind `trust proxy`. `GET /lobbies/stream` as SSE **with** a 15–30 s heartbeat comment and `Last-Event-ID`. Explicit inbound frame-size limit (socketioxide's `max_payload` is an outbound `emit()` guard, not the inbound validation wanted). Born enforcing the `H3` envelope and host rules, because by the time `P0+` ships the Node server it replaces is already enforcing them (§2.3) — there is no mode to match and no flag to inherit. **No gate dependency.** This is also the phase the scope decision rests on: it is the only committed phase of the port, and what it costs against its 4.0 weeks is the evidence the decision to continue is taken on. The ban check stays on the socket, where the server knows the socket id, the lobby and the registered `clientId`; the HTTP endpoint knows an IP and nothing else, so it is strictly worse context for exactly the decision the code comments anticipate.

**`P1+` Foundations, 1.0 → 5.0 wk.** The hand-written Socket.IO client moves here from `P4`, where it was leaving 3 weeks for the entire WebRTC half. Built and conformance-tested against the Node server `P0` has just proven. Four details go into the conformance suite explicitly, because they are how hand-written v4 clients fail: server-initiated ping with client pong (reversed from v3); `pingTimeout` as a liveness deadline feeding the reconnect policy; `maxPayload` from the OPEN packet; and the `41` disconnect / `44` namespace-connect-error packets — get the last wrong and a server restart presents as a client that believes it is still connected. Plus `rustls-platform-verifier` and a system proxy resolver on all three targets, plus the i18n loader and its two CI ratchets. Also here: the IPC transport trait for §2.5, so `P3` and `P4` build against the boundary rather than being retrofitted into it.

**`P2+` Game reader, 4.0 → 6.0 wk.** The Rust bundle consumer, the validator and the prologue check against the format `G0` proved. The write-side half is explicitly **32-bit-Windows-only** (GameReader.ts:605 returns early on `is_64bit` or `is_linux`), so it protects a shrinking minority; the read-side validator protects everyone. `G1` unchanged.

**`P3+` Audio engine, 8.0 → 10.0 wk.** The RTCP feedback loop, both directions (§2.4). No bitrate ladder.

> **`G2` amended — add criterion 5.** Under a 5% loss profile with a Chromium sender, the Rust receive path recovers Opus in-band FEC — `decode(..., fec: true)` on the following packet, driven by the jitter buffer's loss signal — and `getStats()` on the Electron peer shows `fecPacketsSent` climbing in both directions. **This is part of the stop-the-port decision.** If `neteq` 0.9.1 cannot signal loss to the decoder in a way that permits out-of-order FEC recovery, the cost of vendoring the reference NetEQ is decided here, at the gate, not five weeks later at `G3`.
>
> Criterion 3's 30 ms latency budget and the NACK target-delay decision are made **jointly**, not independently: depth sufficient to make a retransmission useful over a 60–100 ms RTT can consume the entire budget.

**`P4+` Transport, 5.0 → 10.5 wk.** Weeks 1–3 are the crate spike, on **all three targets including i686** (aws-lc-rs supports `i686_pc_windows_msvc` but requires NASM in the build environment — nearly free to discover while the rig is standing, expensive to discover in `P7`). The two arms are **not symmetric** and must not be run as though they were: `str0m` is sans-IO with no internal threads and **ships no TURN client at all**, while `G3` explicitly requires relay-only through coturn — so the `str0m` arm cannot reach the test criterion without first importing or writing one, and its result would measure a hand-written I/O loop as much as the crate. Spend the spike proving `webrtc` 0.20 against a real 1.0.2 Chromium client (direct, relay-only, trickle both directions, SDP capture) and timebox `str0m` to a written feasibility read answering only "what TURN client and what event loop would a 14-peer mesh need". Treat the `runtime-mock` virtual clock as a first-class selection criterion — it is what turns the four 1.0.0 connection bugs from flaky timing tests into deterministic state-machine tests. Weeks 4–10.5 are the port, which is now a port and not a mapping: the `&self` event-handler trait forces interior mutability through the peer layer, which collides with §3.2 rule 1.

> **`G3` amended.** Add: (a) the 1.x↔2.x call repeated under each `P3` impairment profile (1/2/5/10% loss), scoring within 0.2 MOS of a 1.x↔1.x call under the identical profile — the clean-network version of this gate cannot see the FEC failure at all; (b) the three-client mixed-generation row from §5.2 item 4, with one client leaving and rejoining.

**`P5+` Platform, 3.0 → 6.0 wk.** The two-process split and the IPC lifecycle. The overlay port gains the UIPI access check surfaced as a first-class UI state, exclusive-fullscreen detection, and Wayland detection **gated on the live winit backend, not `XDG_SESSION_TYPE`**.

**`P6+` GUI, 10.0 → 11.5 wk.** Net of dropping the Fluent conversion (−1.0). Adds the 3-day framework spike, the GPU chain, and the performance baseline. The spike must produce more than three text controls: a lobby-browser table with sortable columns (iced ships no table widget; §3.3 leans on `egui_extras` for this) and one composited animating avatar (a handful of `Painter` calls in immediate mode; a custom `Widget` or `canvas` in iced). The decision point is the **end of the main-view milestone**, roughly week 5 — not the end of the phase, where it can no longer change anything.

**`P7+` Packaging and signing, 4.0 → 9.5 wk.** Everything in §2.2 except the bridge — which no longer includes Authenticode, a CA application or the `publisherName` array, and is 1.5 weeks lighter for it. What remains under "signing" is minisign over the update manifest and the `acl-updater` binary that verifies it. Also: `perMachine` conditional on `D17`, winget manifest and its CI PR job, the Linux tarball with documented `setcap`, and the settings work — `schema_version`, migration chain, `#[serde(default)]` on every field, **never** `deny_unknown_fields` (a 2.0.1 config opened by a 2.0.0 binary must not lose fields), `tempfile::NamedTempFile` + fsync + persist + one `.bak`, the volume map split into its own file, and `config.broken.<timestamp>.json` on parse failure. The 1.x importer reads `config.json` once and **never writes back** — a user running both clients during the beta must not have one silently rewrite the other's settings, which §4.9 item 4 does not currently say. Store `key_epoch` alongside the binary, not in per-user config, or a fresh Windows profile silently disables rollback protection; a missing epoch means "accept and record", not "accept anything".

Prove the new NSIS script by shipping an **ordinary 1.0.x release** with it, so its CLI contract (`--updated /S /D=`) is tested against real 1.x updaters before it carries anything important.

**`P8` Bridge and sunset, 4.0 wk → gate `G4`.** This is the moment a large number of machines execute a downloaded installer, and it must not happen before the minisign manifest chain, the elevation gate and immutable releases are all in the field. It is also, since 2026-08-24, **a prerequisite of the 2.0 release rather than a follow-up to it**: the 1.x wire protocol is switched off when 2.0 ships, so the fleet has to be on the bridge *before* that day, not after. `P8` finishing late does not delay a migration; it cuts people off.

The mechanism is fully specified and can be read directly out of the installed `electron-updater` 6.8.9. `latest.yml` gives `version`, `path`, `sha512`; `findFile` picks by extension and then prefers a filename containing `process.arch` — literally `x64` or `ia32`; `NsisUpdater.doInstall` spawns with `['--updated']` plus `/S` and `/D=<installDirectory>`; `AppImageUpdater` unlinks the running AppImage, `mv -f`s the replacement into place, and then runs it with **`execFileSync`** and `APPIMAGE_EXIT_AFTER_INSTALL=true`. That last detail decides the Linux rollout: `execFileSync` is synchronous, so a Rust AppImage that starts its GUI instead of exiting hangs the old client forever, on every Linux machine, at once. The Rust binary must check that variable in `main()` before anything else.

Ordered steps:

1. 2.0 ships as a **parallel install** — different appId, different directory, config read forward, opt-in by download only — and sits there for a full release cycle while 1.x keeps receiving 1.x updates.
2. Bridge built by the Rust pipeline, published into the 1.x feed as **1.1.0**. Windows: NSIS installer(s) named with the literal tokens `x64` and `ia32`, or one combined dual-arch installer — **decide explicitly**, because the default `findFile` behaviour on a mismatch is to hand every client the first `.exe` in `latest.yml`. Linux: an AppImage that *is* the Rust client and exits immediately on `APPIMAGE_EXIT_AFTER_INSTALL`.
3. **No `.blockmap` asset** for the bridge — `NsisUpdater` attempts a differential download against `CURRENT_APP_INSTALLER_FILE_NAME` first.
4. Staged rollout as **sequential tagged releases** 1.1.0 → 1.1.1 → 1.1.2, a week apart, with the cohort baked in at build time. `stagingPercentage` is not available and this is settled rather than argued: it lives in `latest.yml`, `latest.yml` is a release asset, and immutable releases have frozen release assets since 2026-08-24. Each step is therefore its own build, minisign signature and manifest — three release ceremonies, which is why this phase is 4 weeks and not 2.
5. The first bridge installer **renames rather than deletes** the Electron install and its config, and 2.x ships a documented way back. Only after the bridge has sat at full rollout for a cycle does it start deleting.

**Rollback:** re-mark the 1.0.2 release as *Latest*. With `allowPrerelease` false, `GitHubProvider.getLatestVersion` resolves the tag from `/releases/latest`, so un-updated clients revert within one check interval without touching a frozen asset. It is all-or-nothing and cannot express a percentage. Deleting the bridge release instead does **not** free the tag, so the retry must be 1.1.1.

**Measurement of parity:** no new telemetry is required — the server sees every client that joins a lobby, so per-version join counts show the fleet moving. The rollout is working if the 1.0.2 share falls roughly in proportion to the cohort and the 2.x share rises to match, **with no drop in total joins**. A drop in total joins is the signal to stop.

> **Gate `G4` — bridge rehearsal, and a release prerequisite.** On **real 1.0.2 installs**, not dev builds: Windows x64, Windows ia32 and Linux each update from a staging feed to the bridge. Silent install; the **correct architecture** selected; correct install directory; working uninstall entry; migrated config; and on Linux the old process exits within 2 seconds rather than hanging. Prerequisites in the field: the minisign manifest chain, the elevation gate, immutable releases, and `G3` passing — because during the staged rollout the same lobby contains both generations, by design, for weeks.
>
> **`G4` gates the 2.0 release itself.** It is no longer a gate whose failure mode is "no fleet migration; 2.0 stays a parallel install" — that fallback died with the sunset decision. Since 2.0 shipping is what switches the 1.x wire format off, a `G4` that has not passed and a fleet that has not moved mean 2.0 cannot ship without cutting off every remaining 1.x user on the day. If `G4` fails, 2.0 waits.

**Sunset.** No date is published, and none is needed: the 1.x wire format is switched off **when 2.0 ships**. The server remains the binding commitment — users who never update keep working only for as long as the server speaks 1.x — but that window is now defined by the rollout rather than by a calendar, which is why `P8` runs before the release and not after it. Announce it in-app through the existing update-notification path at least two release cycles ahead, and when it arrives have the server return a message the 1.x client displays rather than failing silently.

**What this decision cannot reach.** Third-party operators run their own servers on their own schedule, and `README.md` invites them to. Switching our server off says nothing about theirs, so a 2.x client still has to cope with a server that speaks only the old shape indefinitely. That, and nothing else, is why the 2.x client keeps its `join_lobby` ack and socket lobby-browser fallbacks permanently (§6, question 15). Against **our** server those fallbacks are dead code from the day 2.0 ships; they exist for other people's deployments.

**`P9` Post-1.x cleanup, 3.0 wk, outside the 2.0 budget.** Once the 1.x share is below the agreed threshold: move lobby settings and the radio claim to the socket, drop the data channel and disable SCTP, delete the SCTP fuzz targets, and revisit the bitrate ladder only if the impairment harness has actually shown self-inflicted congestion in a 12–15 player mesh. A second wire protocol is *not* on that list, and the reason has changed rather than been met: the OBS page migrated at `H3`, but the 2.x client keeps its Socket.IO fallbacks permanently for third-party servers (question 15), so the parser never leaves the client and a second path still deletes nothing.

### 3.4 What has no wire component

Explicitly, so nobody designs a negotiation for it: the offsets bundle, the hat manifest, the settings schema, device identity, the GUI framework, the renderer chain, the process split, the overlay, localisation, the Socket.IO client rewrite, and the ICE-config allowlist are all **local to one machine**. They need no version field, no capability probe and no dual-stack window. A 1.x client and a 2.x client differ in what they trust locally and are indistinguishable to each other on the wire. Only `H3` step 3, `P8`, and `P9`'s data-channel removal have wire consequences.

---

## 4. What must not change

**Killed by the challenge round, and now closed questions:**

- **A second `/v2` wire protocol.** It never deletes the Socket.IO parser, so the server ends with two protocols, two client transports, a probe/cache/fallback state machine, and both original parsers still facing unauthenticated input. The 7-day cached protocol choice also misses the actual reverse-proxy failure — an upgrade that succeeds and then silently drops frames — presenting as a client sitting in a lobby hearing nobody, for up to a week, with the only remedy a settings toggle the user must find.
- **An SFU, as a costed roadmap item.** The transparent design and the 14× design are different systems. Quote neither number. The transparent relay's *reliability* upside is real and is the only free idea; the bandwidth and CPU figures attached to it are **unverifiable as stated**, because the client still opens one PeerConnection and one Opus encoder per socket id.
- **Moving lobby settings and impostor radio to the socket during the dual-stack window.** The benefit is structurally unavailable while any 1.x client is in the lobby, and it creates a failure that does not exist today: two simultaneous radio holders, one per transport. The client-side host cross-check against `parsedHostId` — derived from the game's own process memory, which the server structurally cannot perform — must be kept regardless.
- **The adaptive bitrate ladder in 2.0.** It fights the FEC loop on the same input with the opposite sign, it is the one behaviour with no Electron reference to measure against, and per-peer semantics mean 14 control loops on one microphone.
- **Freeze protection on the update manifest.** The same fleet-wide availability bomb TUF was rejected for, plus a dependency on the user's clock.
- **`self_update`.** Its signature feature verifies nothing about an NSIS `.exe` or an AppImage.
- **cargo-dist MSI and axoupdater.** MSI strands every Windows 1.x client; axoupdater does no signature verification. (And the "axo.dev no longer resolves" claim was simply wrong — do not repeat it.)
- **`stagingPercentage` as the rollout mechanism.** Incompatible with immutable releases; the two were proposed together and are mutually exclusive.
- **Tightening `cors.origin`.** No security gain once polling is off; the one part of that bullet that can break the OBS page in the field.
- **Certificate pinning, a project-operated server signing key, or TOFU.** Defends a threat that is not the one implied by a user-configurable server URL.
- **The Fluent conversion.**
- **Slint, xilem, makepad, freya, dioxus, and raw Win32+GTK.**
- **Flatpak.** Not a trade-off — its PID namespace makes reading the game process impossible.
- **A Wayland overlay.** Architecturally impossible for an ordinary client.
- **Retiring the device label-recovery fallback.** It is the tier that recovers a setting when Windows renames the endpoint on re-plug.
- **The `broadcastVersion` round-trip self-test.** Tautological.
- **An in-binary hat fallback.** Contradicts the don't-vendor position it sits next to.

**Closed by the maintainer's decisions of 2026-08-24, and not to be reopened inside 2.0:**

- **The mobile-client promise in `03-target-architecture.md` §3.5.** Deleted, not softened. The server is websocket-only; any mobile `socket.io-client` negotiates polling first and is refused at the handshake. Do not add a polling path back to serve a client that does not exist.
- **A signature on the offsets bundle.** No minisign, no key ceremony, no second signer, no revocation. The reasoning is availability and is written out in §2.1, along with the residual risk it accepts. The mirror, the commit pin, the embedded floor and the validator are the replacement and are not optional.
- **Authenticode code signing, and with it the CA question.** No SignPath application, no Certum subscription, no `publisherName` array. Windows artefacts are unsigned and SmartScreen will say so. Minisign over the update manifest stays and is a different question with a different answer, because a release has no availability pressure.
- **A logging period, a 0.1% threshold or a `SIGNAL_STRICT` flag before enforcing the envelope rules.** The rules ship on. The counter that the flag was supposed to wait for had no forcing function behind it and would not have moved.
- **A Windows service for the elevated helper.** Started on demand, per-launch UAC prompt, no auto-start entry and nothing resident.
- **`stagingPercentage` as the rollout mechanism.** Immutable releases have been on since 2026-08-24, so this is now a fact about the repository rather than a design preference.
- **A dated 1.x sunset, and an open-ended dual stack.** The 1.x wire format goes off when 2.0 ships, which makes `P8` and `G4` prerequisites of that release.

**Kept because the plan is right:**

- WebRTC as the media transport, and the full mesh for 2.0.
- Socket.IO, JSON, and opaque SDP relay.
- `neteq`, with the `G2` amendment.
- winit as the windowing base, and porting `windows.c` / `x11.c` rather than re-deriving them.
- The four-view split, and the settings schema and `config.json` staying compatible.
- JSON via serde for settings.
- The hat commit pin.
- The AppImage and NSIS artefact types, and their exact CLI contracts — `findFile` picks by extension, so changing artefact type is the same act as abandoning the installed base.
- `G2`'s stop rule, verbatim. Two of the amendments above make it *harder* to pass; none makes it easier.
- The five existing test files ported in `P1`, and the twelve named regression tests in §5.3 plus the four connection tests in §4.6.
- `unsafe_op_in_unsafe_fn = "deny"`; `cargo-deny` and `cargo-vet` blocking from `P1`; §7.6's no-unpinned-branch-HEADs rule — which the offsets fetch violates today and which must not be waived for it.
- `Voice.tsx:1112-1115`. The relay-only floor already works. Do not "add" it; do not lose it in the port.

---

## 5. Interop matrix

**Client generations in the field.** `C1` = 1.0.2 and earlier Electron. `C2` = 1.0.3–1.0.5 hardened Electron. `C3` = 2.0 Rust client. `C-OBS` = the browser overlay page at `obs.aucl.greluc.me`, which serves every generation simultaneously and lives in neither repository. `C-MOB` = any mobile client speaking Socket.IO. §3.5's promise that a future 4.x mobile client would keep working was **deleted on 2026-08-24** (S20); the row is kept below so the consequence is visible, not because such a client is planned.

**Server generations.** `S1` = today's Node server. `S2/S3` = the `H3` Node server: `obs_state`/`mobile_state`, websocket-only transport, 64 KB payload cap, **and the envelope and host rules enforced from the same release**. These were two generations only while enforcement was staged behind a flag; that staging was removed on 2026-08-24, so there is exactly one Node generation after `H3` and it refuses from its first minute. `S4` = the Rust server from `P0+`, born enforcing. Third-party operators will run **any** of these, indefinitely — which is the constraint that decided S1 and shapes every row below.

| Client → | `S1` | `S2/S3` | `S4` |
|---|---|---|---|
| **`C1`** 1.0.2 | Works, as today. | **Degrades, immediately:** OBS overlay feed and mobile relay are **refused** — `C1` addresses non-member rooms and the envelope rule rejects it. Voice, lobbies and the browser are unaffected. There is no grace period; a `C1` that never updates loses both features permanently. | Same, until 2.0 ships — at which point `S4` stops speaking the 1.x wire format at all and `C1` cannot connect. Being on the far side of that day is exactly what `P8` and `G4` exist to prevent. |
| **`C2`** hardened 1.x | Works. Emits `obs_state`; `S1` has no handler, so the overlay silently gets nothing — **`C-OBS` must therefore accept both feeds throughout**, and be deployed before `C2`. | Works fully. Nothing `C2` sends is refused. | Works fully — and, being 1.x on the wire, `C2` is cut off by the 2.0 switch-off on the same day as `C1`. Hardening does not exempt it; only the bridge does. |
| **`C3`** 2.0 Rust | Works. **Degrades:** `GET /lobbies/{id}/code` and `/lobbies/stream` 404, so `C3` falls back to the `join_lobby` ack and the socket lobby-browser events. **`C3` must retain both socket fallbacks permanently** — but only for third-party `S1` deployments, which will never all upgrade. Against our own server they are dead code from the day 2.0 ships. | Works. HTTP endpoints available. | Works fully. |
| **`C-OBS`** | Legacy feed only. | New feed only — the legacy room is refused from the same release. **This is why the page is deployed and verified before the server, not alongside it.** | New feed only. |
| **`C-MOB`** | Works. | **Refused at handshake.** The server is websocket-only and `socket.io-client` defaults to `["polling","websocket"]`. No server setting reopens this; §3.5's promise is gone. | Same. |

**Client-to-client, same lobby.**

| Pair | Result |
|---|---|
| `C1` ↔ `C1` | Baseline. |
| `C1` ↔ `C3` | **Works — this is `G3`.** Media over WebRTC; lobby settings and impostor radio over the data channel, which `C3` keeps for 2.0. Degrades only under loss if §2.4 is not implemented, and then **in both directions**. |
| `C2` ↔ `C3` | As above. `C2` additionally refuses a settings push from a peer that is not the reported `isHost` claimant, so a `C3` bug in host reporting presents as "settings do not apply", not as a cheat. |
| `C1` ↔ `C2` | Works. `C2` is immune to the `mobilePlayerInfo` attack; `C1` in the same lobby is not, until it updates. |
| `C3` ↔ `C3` | Works. Relay-only asymmetry is not a failure: if one enforces relay and the other does not, ICE negotiates the relayed pair, one IP stays hidden and the other does not. That is the correct outcome. |
| Three clients, mixed, one rejoins | `G3` amended row (b). |

**Field conditions that refuse outright.**

| Condition | Behaviour | Fix |
|---|---|---|
| `C1`/`C2` and `C3` installed on the same machine | Today: **two keyboard hooks on the same key, two overlays on the same game window, two memory readers**, because the Electron `requestSingleInstanceLock` keyed to `me.greluc.anothercrewlink` is invisible to a Rust binary. | `H1`'s `Local\AnotherCrewLink` mutex, in the field before any 2.x beta build exists. Second instance refuses to start. |
| `C3` pointed at an `http://` server | Refused. | `--allow-insecure-server`, or a resolved-loopback address. Two-release deprecation for `C1`/`C2`, with the warning on the **main window and the connect-failure path** — not only in the settings dialog a self-hoster never reopens. |
| Bridge installer misnamed | 32-bit users silently receive the 64-bit installer, because `findFile` falls through to the first `.exe`. | `G4`, ia32 leg. |
| `C3` on Wayland | Overlay unavailable; everything else works. Under XWayland outside gamescope it is attempted. Inside gamescope it is not. | R11 detection. |
| `C3` where offsets cannot be validated and the embedded bundle predates the current game build | `C3` fails closed on the game read, so **it never joins the lobby at all** — 1.x players experience this as "he cannot hear us". | The embedded floor plus `G0`'s 6-hour drill. This is the honest interop cost of §2.1, it is not zero, and dropping the signature is what keeps the window measured in hours rather than in whoever holds the key being awake. |
| `C3` where the game is elevated and the helper is not | UIPI blocks the overlay. | The ported `has_uipi_access` check and an accurate message, not a blank screen. |

---

## 6. Open questions for the maintainer, before phase 1

Each is answerable yes or no. Ten were put to the maintainer and answered on 2026-08-24; the answers are recorded inline and are settled. Five remain open and are marked **OPEN**. Of the five, only number 1 blocks work that starts in `H1`–`P1`.

1. **Is `/socket.io/` accepted as the permanent server wire protocol, with no `/v2`, unless and until a dated 1.x sunset and a migrated OBS page both exist?**
   > **OPEN.** Note that question 4 has since removed the dated sunset from the world, so the standing condition now reads "unless and until 2.0 has shipped and the OBS page has migrated".
2. **Is the mobile-client promise in `03-target-architecture.md` §3.5 being kept?** If yes, `transports: ['websocket']` cannot ship and §3.5 stands; if no, §3.5 is amended in the same commit as the server change. It cannot be left unanswered.
   > **Answered 2026-08-24: no.** §3.5's undertaking is deleted rather than softened, the server ships websocket-only, and any mobile socket.io client is refused at the handshake.
3. **May the server enforce the signal envelope and host claims once the rejection counter is below 0.1% of signals for 7 consecutive days, accepting that some 1.x clients will never update and will lose their OBS overlay and mobile relay?**
   > **Answered 2026-08-24: yes, and sooner — enforce immediately.** No logging period, no threshold and no flag; `H3` ships the rules on, breaking the OBS feed and the mobile relay for every client older than 1.0.5 at once.
4. **Do we publish a dated sunset for the 1.x signalling wire format in the `P0` document now, rather than letting it happen by accident?**
   > **Answered 2026-08-24: no date — the 1.x wire format is switched off when 2.0 ships**, which makes `P8`'s fleet migration and `G4` prerequisites of the 2.0 release rather than follow-ups to it.
5. **Do we accept a per-launch UAC prompt for the elevated helper, rather than installing a Windows service?** A "no" to both means the two-process split collapses and §2.5's security benefit is not available.
   > **Answered 2026-08-24: yes, per-launch UAC prompt, no service.** The prompt is visible friction once per session and is accepted; the two-process split is therefore available and §2.5, `03-target-architecture.md` §3.2, `06-security.md` and `02-feasibility.md` stop hedging about it.
6. **Do we accept that Linux gets no separate updater process — verify-then-replace inside the AppImage — i.e. a second update code path we own permanently?**
   > **OPEN.**
7. **Do we make the Windows install per-machine, conditional on confirming that `elevate.exe` actually ships inside a real installed 1.0.2?** If it does not, the bridge is per-user and the `%LOCALAPPDATA%` privilege-escalation fix waits for a later release.
   > **OPEN** — but the condition is discharged: `elevate.exe` does ship, 107,520 bytes, checked against a real install (see *Verification status*). Only the per-machine decision itself is outstanding.
8. **Do we enable GitHub immutable releases now, accepting that `latest.yml` becomes frozen and the staged rollout must be sequential tagged releases rather than percentage edits?**
   > **Answered 2026-08-24: yes — enabled that day and verified through the API.** `stagingPercentage` is consequently not a mechanism this project has, and `P8`'s rollout is sequential tagged releases, each with its own build and manifest.
9. **Do we ship `win.publisherName` as an array of every CA subject we might ever use, in 1.0.3, before knowing which CA approves us?** A "no" here risks permanently bricking every 1.0.3 install's updater on a later CA switch.
   > **Answered 2026-08-24: the question falls away.** With no Authenticode certificate (question 10) there is no subject to declare, `verifySignature` finds no `publisherName` and returns null as it does today, and there is no CA switch to survive.
10. **Do we disclose the memory reader and the 32-bit injection stub to SignPath in writing before applying, and accept Certum at roughly €30–70/year out of pocket if SignPath declines?** *Their answer is unverifiable from here; the pattern-scanning of another process's address space is plausibly in scope of their exclusion clause on its own, independently of the injection feature.*
    > **Answered 2026-08-24: no — no Authenticode at all.** Windows artefacts ship unsigned and users keep seeing the unknown-publisher warning. Update integrity rests on minisign over the manifest, which proves the artefact is the one this project published and does nothing about SmartScreen.
11. **Do we accept removing `ar`, `fa` and `he` from the language picker if the framework spike shows egui is otherwise the better choice?** The alternative is trading a Rerun-backed, monthly-released framework for a single-maintainer one with a 15-month release gap, on the strength of 3 locales out of 37.
    > **OPEN** — and no longer urgent, since it belongs to `P6+`, which is not funded.
12. **Do we accept the `P4` re-baseline — the transport phase roughly doubling — as the price of the sans-IO `webrtc` 0.20 rewrite, rather than pinning the bug-fix-only 0.17.x line?**
    > **OPEN** — subsumed by the scope decision below; it is decided if and when the port continues past `P0+`.
13. **Do we accept ~77 developer-weeks to 2.0 instead of 37?** If no, the defensible cut is: keep `H1`, `H2`, `P0+`'s registry and envelope work, `P2+`, `P3+`, and `P7+`'s signing and minisign work; defer `H3`'s OBS capability rework (which also defers signal *enforcement*), the SSE lobby stream, winget, the two-process split, and the GPU chain's runtime recovery. That lands near 60 weeks. **§2.1, §2.2 and §2.3 are not on the cut list.**
    > **Answered 2026-08-24: not as a commitment.** `H1`–`H3` and `P0+` are funded; the rest is planned but not committed, and whether it proceeds is decided at an explicit decision point after the Rust server ships, on what building it actually cost. The figure itself stands (~74 weeks after the other decisions of that day), and the cut list above stands as the answer to a later "no".
14. **Do we take on being the second person in the availability chain during an Among Us update, with a second designated signer provisioned before the first signed bundle ships?**
    > **Answered 2026-08-24: the question falls away with the signature.** There is no key, no ceremony and no second signer; the human in the chain is a pull-request reviewer, which is the whole point of §2.1's availability argument.
15. **Do we accept that the 2.x client keeps its Socket.IO `join_lobby` ack and socket lobby-browser fallbacks permanently, because third-party servers will never all upgrade?**
    > **Answered 2026-08-24: yes — for that reason and no other.** Our own server stops speaking 1.x when 2.0 ships, so the fallbacks are dead code against it; they exist because third-party operators run their own servers on their own schedule.

---

## 7. Claims that remain unverified

Recorded so they are not read as settled:

- **`webrtc` versus `str0m`.** Neither can demonstrate Chromium interop in CI. Only the `P4+` spike answers it, and its result must be written into `07-dependencies-toolchain.md` with the date and the evidence, whichever way it goes.
- **egui versus iced for this UI.** Only the `P6+` spike answers it, and only if the spike includes the lobby table and a composited avatar.
- **`neteq` 0.9.1's Opus FEC recovery.** Its documented surface says nothing about it. `G2` criterion 5 answers it.
- **`cpal::DeviceId` stability across a re-plug to a different USB port on Windows.** Documented as stable "where possible"; the platform's own endpoint renaming says it is not possible for that event. Hence tier three.
- ~~**SignPath eligibility.**~~ Moot since 2026-08-24: there is no Authenticode signing, so nobody applies and nothing turns on the answer.
- **Whether `elevate.exe` ships in the resources of a real installed 1.0.2.** Must be checked against an actual install, not against the repository.
- **The idle CPU, RSS, startup and overlay frame-cost figures in §2.4.** Explicitly estimated. Do not quote the order-of-magnitude claim publicly until the `P6+` baseline exists, and take it as a matrix (Windows/GPU, Windows/software, Linux/software) because `index.ts:38-40` already makes those three the shipping reality.
- ~~**Whether the `signal` rejection counter will ever reach the 0.1% floor.**~~ Moot since 2026-08-24: there is no floor and no flag. The underlying observation stands and is now a *reason* rather than a risk — 1.x updates through electron-updater with no forced upgrade, so a rule that waits on the fleet waits forever, which is why the rules ship on.
- **How many 1.x clients lose the OBS overlay and the mobile relay at the `H3` server release, and for how long.** Knowable only afterwards, from per-version join counts. The decision was taken without it.
- **Time from an upstream offsets commit to a published bundle.** Only the `G0` drill against a real Among Us update answers it. Dropping the signature should shorten it materially; that expectation is not evidence.
- **Whether branch protection on the offsets mirror is a sufficient substitute for a signature.** It is a judgement, not a measurement: with no signature, whoever can push to the mirror can change what every client reads. §2.1 states the risk; nothing in this plan tests it.
- **The effort arithmetic in §3.1.** It is the union of independently-priced corrections, several of which overlap; treat 74 weeks as the honest midpoint of a range whose low end is around 65 and whose high end nobody should want to find out. Only `H1`–`H3` and `P0+` — 11.5 of it — are committed, and the rest is a plan rather than a promise.