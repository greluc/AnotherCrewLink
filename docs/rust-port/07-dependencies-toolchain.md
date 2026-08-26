# 7. Dependencies and toolchain

Versions were re-checked against crates.io on **2026-08-24**. One did not exist:
`serde_json` 1.0.229. The two serde crates have not shared a version number
since 2019, and collapsing them into one table cell produced a version that
cannot resolve; the row is split and corrected in §7.2. Every other version
held. Entries are the latest stable release except where a note says why an
older or an exactly pinned one is carried instead.

## 7.1 Toolchain

| | Version | Note |
| --- | --- | --- |
| Rust | **1.98.0** (2026-08-20) | pinned in `rust-toolchain.toml` |
| Edition | 2024 | |
| MSRV policy | = pinned stable | no back-compat burden; this is an application |
| Target | `x86_64-pc-windows-msvc` | the only one. i686 went 2026-08-24 with the injection path; Linux went 2026-08-25 with the client's Linux support |

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.98.0"
components = ["rustfmt", "clippy", "rust-src"]
targets = ["x86_64-pc-windows-msvc"]
```

The toolchain is bumped deliberately, one commit at a time, not floated on
`stable` — a floating channel means CI can break on a day nobody touched the
code.

> **Resolved 2026-08-24, and the resolution is the good news this section was asking
> for.** The injection path was removed, `i686-pc-windows-msvc` went with it, and every
> cost below was paid off at once: `libwebrtc` is reachable and has since been measured
> building, linking and running on x64; NASM is not required; the alignment hazard is
> gone with the target. The open decision at the end of the section — a small 32-bit
> helper talking to a 64-bit client — does not need taking. What follows is the
> reasoning as it stood, kept because the alignment rule is still worth knowing if the
> target ever returns.

`i686-pc-windows-msvc` exists for one reason: the injection path, which is
feature-gated and 32-bit-Windows-only. It is also the most expensive line in
this document, and the cost is paid by everything else. It forecloses LiveKit's
`libwebrtc` binding — the one dependency that would supply AEC3, NS, AGC, Opus
with FEC and DTX, RTP/RTCP and NetEQ together — whose build script maps only
`x86_64` and `aarch64`. It puts NASM on every build machine the moment a crate
defaults to `aws-lc-rs`, which ships prebuilt NASM objects for Windows x86-64
only. And it carries MSVC's 4-byte alignment quirk, which is a live unsoundness
hazard for exactly the struct parsing `acl-game` is made of: never use
zerocopy's reference APIs (`ref_from_bytes`, `try_ref_from_bytes`) on a struct
containing a 64-bit field on this target. `read_from_bytes` copies, and tens of
bytes at 30 Hz costs nothing. Make it a clippy `disallowed-method` lint rather
than a convention.

Splitting injection into a small 32-bit helper process talking to a 64-bit
client is an open decision, not a settled one. It would move rows 13 and 14 of
the risk table from High to Low, make the i686 leg of every gate optional, and
bring the integrated-pipeline option back into scope. It should be decided
explicitly rather than inherited from a target list.

## 7.2 Client

### Audio — the crates that decide the project

| Crate | Version | Role | Note |
| --- | --- | --- | --- |
| `cpal` | 0.18.2 | Device enumeration, input/output streams | 18.5 M downloads; the standard choice. 0.18 is a ground-up rework whose PipeWire host now takes priority over PulseAudio on Linux, and four open issues sit on the WASAPI device-change path — hot-plug is a G2 item, not a follow-up |
| `opus` | **=0.3.1** | Opus encode/decode | binds libopus, the same library Chromium uses. 0.4.0's entire content is a `-sys` backend swap; it waits until `opusic-sys` is shown to link on `i686-pc-windows-msvc` |
| `sonora` | **=0.2.0** | AEC3, noise suppression, AGC, HPF | the default APM. Pure Rust: no C++ toolchain, no meson, no ninja, no git submodule. Conditional on a green i686 build at G2 |
| ~~`webrtc-audio-processing`~~ | ~~**=2.1.0**~~ | Struck 2026-08-25: a Linux-only baseline with no Linux to run it on, and already out on merit — see `experiments/README.md`, 2026-08-24 | **Was Linux only.** It does not build on either Windows target: PR #102 "Support MSVC targets" has been open and unmerged since 2026-08-08, issue #34 "Windows build" since 2023-09-27, and its CI runs on `ubuntu-latest` alone |
| `neteq` | **=0.9.1** | Adaptive jitter buffer, PLC | `default-features = false` is mandatory, with an `AudioDecoder` implementation over the `opus` crate so libopus stays the only codec in the binary. Young (38 k downloads); measured at gate G2 |
| `rubato` | **=5.0.0** | Sample-rate conversion | four breaking majors in four months, each locked to a matching `audioadapter`. `process_into_buffer` only, and scoped to devices that are not already at 48 kHz |
| `realfft` | 3.5.0 | FFT for the analyser | `process_with_scratch` with preallocated scratch; `process` allocates |
| `fft-convolver` | 0.4.0 | ConvolverNode | the one DSP node that is an algorithm rather than a formula |
| `ringbuf` | 0.5.1 | Lock-free SPSC into the audio callback | |
| `hound` | 3.5.1 | WAV I/O for the golden-vector tests | dev-dependency; never a runtime dependency, never reads a file the app did not produce |

The APM sits behind a trait, and which implementation is behind it changed once
the Windows build was actually attempted. `webrtc-audio-processing` was the
conservative choice for as long as nobody checked; on the platform that has the
overwhelming majority of the users it does not compile at all, and an APM that
does not exist on Windows ships an audible regression on day one. ~~It stays in
the tree as a Linux-only baseline to measure against~~ — struck 2026-08-25. The A/B
echo-return-loss-enhancement measurement it was kept for could only be run on the one
platform where both build, and there is no such platform in scope any more. It had
already stopped being the comparison that matters on 2026-08-24, when `libwebrtc` came
back into reach.

`sonora`'s risks are real but they are not the ones a young crate usually
carries. It is ported from WebRTC M145 and its README states it is validated
against the C++ reference suite of 2,400-plus tests, within 1.07–1.24x of the
C++ on the benchmarks — running the reference implementation's own conformance
suite is a stronger correctness argument than a download count. What it has
instead is two releases, one author with 209 of 221 commits, an AI-assisted port
acknowledged in its own history, and an **unverified** i686 build: its SIMD
paths are SSE2/AVX2/NEON and its README validates on Ubuntu x86-64 only. That
last one is a gate criterion at G2 — a green `cargo build --target
i686-pc-windows-msvc` of whichever APM wins — and it is an hour of work to
answer.

`cubeb` 0.38.0 is the documented fallback for `cpal`, behind the same device
trait. The trigger is written down here so it is not a judgement call in the
middle of an incident: adopt it if the WASAPI device-change issues are still
open at G2 and the hot-plug test cannot be made to pass, or if a released `cpal`
faults from safe code on the enumeration path. It is not free — Mozilla's shared
audit chain for cubeb stops at 0.34.1, so the delta to 0.38.0 would be ours to
audit.

The reverb impulse response is decoded in the `xtask` and embedded as PCM with
`include_bytes!`. Nothing decodes a media container at runtime: the file ships
inside the application, is 55,589 bytes and never changes, and pulling a
general-purpose demuxer into the shipped binary to read a compile-time constant
is a parser we would be adding for no reason. Have the xtask print the decoded
frame count so the embedded size is recorded rather than estimated.

The DSP nodes themselves — panner, biquad, gain, analyser — are written in this
repository rather than taken from a crate. `fundsp` 0.23 and `biquad` 0.6 were
considered and rejected: parity with the Web Audio specification is the
requirement, and matching a third-party crate's coefficient conventions to the
spec's is more work than implementing four documented formulas. The convolver is
the exception, and it is why the list is four rather than five. It is not a
formula but an algorithm with a performance requirement — uniformly partitioned
FFT convolution with correct overlap-add accumulation, correct latency
alignment, and no allocation or denormal stall in the callback — and its failure
modes are all quiet ones: a reverb tail slightly late, slightly smeared or
slightly quiet produces no crash, no test failure and no bug report anyone can
articulate. `fft-convolver` does the general part and the Web Audio
normalisation scalar is the one line the spec hands you. Its real-time-safety
claim is the crate's own and is **not independently verified**; check denormal
flush-to-zero before the first golden-vector run.

### Network

| Crate | Version | Role | Note |
| --- | --- | --- | --- |
| `webrtc` | **=0.20.3** | ICE, DTLS, SRTP, data channels, TURN client | pin exactly: since 0.20.0 the crate is a thin async wrapper over a new sans-IO `rtc` core, and the maintainer states a minor bump may carry breaking changes |
| `tokio-tungstenite` | 0.30.0 | WebSocket transport under the Socket.IO client | the primary signalling transport, not a fallback — see §7.5 |
| `tokio` | 1.53.1 | Runtime | |
| `ureq` | 3.4.0 | Offset bundle, hat collection, update check | synchronous, driven from `tokio::task::spawn_blocking`; `platform-verifier` on and the bundled webpki roots off |

Three GETs do not need an async HTTP client. `reqwest` 0.13.4 resolves 94 crates
against `ureq`'s 27, and measured across the whole networking domain the
difference is 272 crates with `aws-lc-sys`, `ring`, `native-tls`, `rustls` and
`schannel` all present against 228 with only `ring` and `rustls`. reqwest 0.13
changed its crypto foundation quietly — its `rustls` default is aws-lc-rs — so
the plan as written linked two C and assembly crypto libraries where webrtc-rs
already links one, and imposed NASM on the mandatory 32-bit target for it.
Running ureq from `spawn_blocking` is arguably the better shape anyway: three
update GETs then cannot stall the runtime the voice path shares. Do not drop to
`hyper` directly — redirects, proxies, connection pooling and TLS plumbing are
the write-our-own trap.

`str0m` 0.23.1 is the alternative to `webrtc`, and the reason it is not the
default is TURN. Its README puts TURN explicitly out of scope and its feature
table marks it absent. This app forces relay —
`validateClientPeerConfig` carries a server-pushed `forceRelayOnly` alongside
`turn:` and `turns:` URLs with username and credential — so choosing str0m means
writing or sourcing an RFC 8656 client: Allocate, Refresh, CreatePermission,
ChannelBind, long-term credentials with realm, nonce and stale-nonce handling.
That is a multi-week job in exactly the category that should not be
hand-written, and `webrtc-rs` ships `rtc-turn`. The reason the plan has given
until now — that str0m's peer-to-peer path has had less testing — is not the
reason; it is weaker and it is going stale.

Record what is given up rather than pretending the chosen option dominates.
str0m is sans-IO end to end, so its deterministic feed-time-in, drain-output
testing model is available where the `webrtc` wrapper's is not; it ships a
GCC-style bandwidth estimator and a pacer where `rtc-interceptor` ships TWCC
feedback and no estimator on top of it; and it advertises DTLS 1.2 and 1.3 where
webrtc-rs's version support is unconfirmed. Whether either crate interoperates
with Chromium is settled by no CI that exists today — only the P4+ spike answers
it, against a real 1.0.2 client, and the answer belongs in this document with
its date and its evidence whichever way it goes. One housekeeping consequence of
the pin: `rtc-dtls` pulls `rkyv` for DTLS session resumption this app will never
call, so rkyv's 2026 advisories will appear in every `cargo audit` run and need
a dated `deny.toml` note plus a confirmation that those two functions stay
unreachable.

### Platform

| Crate | Version | Role | Note |
| --- | --- | --- | --- |
| `windows-sys` | 0.61.2 | Win32: process memory, key polling, window styles | replaces `windows` 0.62.2 once `sysinfo` is gone — 2 dependencies against 15, a 3.5x faster clean build, the same five `unsafe` blocks at the call site, and binaries within 4 KB of each other |
| ~~`x11rb`~~ | ~~0.14.0~~ | Struck 2026-08-25 with the client's Linux support | was: X11 key polling and XFixes input regions |
| ~~`nix`~~ | ~~0.31.3~~ | Struck 2026-08-25 with the Linux reader; it was the workspace's only use of the crate | was: `process_vm_readv`, a safe `fn` whose lengths derive from the slices passed in |
| `directories` | 6.0.0 | Config, cache and log paths | archived on GitHub and moved to Codeberg, frozen since release; pulls MPL-2.0 `option-ext` and an unbounded `windows-sys >=0.59` |

Process enumeration is written here rather than taken from a crate: about 25
lines of `CreateToolhelp32Snapshot` and `Process32FirstW`/`NextW` on Windows,
one `unsafe` block around a frozen API, and a direct transliteration of code already
in `native/`. (A `read_dir("/proc")` scan stood beside it until 2026-08-25.) `sysinfo` bought
3 ms per call over that, and cost 25 crates, 26 seconds of clean build,
`winapi` 0.3.9 and 107 MB of MinGW import libraries for targets this project
never builds. It is also what forced `windows` 0.62.2 into the tree, and
shipping two Win32 binding crates is worse than either alone. Neither figure
should be paid on a poll loop in any case: find the PID once, keep the handle,
re-scan only on read failure, rather than re-enumerating on every check as the
current client does.

Budget for one disruption on the Windows side: windows-rs master is at 0.100.0
with a wholesale restructure — feature names move from `Win32_Foundation`-style
paths to lowercase header names, edition 2024, MSRV 1.95. The next release
rewrites every `use` and every feature name in the Win32 layer.

The Windows key hook stays what it is today: a 60 ms `GetAsyncKeyState` poll. (It was
aliased to `XQueryKeymap` on Linux until 2026-08-25.) The plan has until now named
`SetWindowsHookEx(WH_KEYBOARD_LL)` as the Windows key hook and described it as a
port. There is no such hook anywhere in the current code — `native/` and `src/`
contain no `SetWindowsHookEx`, no `WH_KEYBOARD_LL` and no
`LowLevelKeyboardProc` — so that would have been new behaviour wearing a port's
clothes. A `WH_KEYBOARD_LL` callback runs on the installing thread's
message pump, and until it returns every keystroke on the desktop waits behind
it; exceed `LowLevelHooksTimeout` and Windows silently unhooks it. That is a
desktop-wide latency dependency in exchange for nothing, over a poll that
already works and intercepts no key from the game. (A free improvement on the Linux
side stood here — one x11rb connection held open instead of
`XOpenDisplay`/`XCloseDisplay` per key check — and went with Linux on 2026-08-25.)

`rdev` was rejected: last released 2023-06-26.

### GUI

| Crate | Version | Role | Note |
| --- | --- | --- | --- |
| `eframe` | 0.36.1 | Application shell | `default-features = false` with the features named explicitly — wgpu became the default renderer only in eframe 0.34.0, and inheriting a renderer is not a decision |
| `egui` | 0.36.1 | Widgets | |
| `egui_extras` | 0.36.1 | Tables for the lobby browser | its defaults are already empty; never enable `all_loaders` |
| `winit` | 0.30.13 | Windowing (via eframe) | |
| `wgpu` | 30.0.1 | Rendering (via eframe) | not in wgpu's own CI for i686 |
| `raw-window-handle` | 0.6.2 | Overlay: reaching the native handle | already transitive; 27 months old because it is finished |
| `image` | 0.25.10 | Avatar recolouring, hat compositing | `default-features = false, features = ["png"]` — the default pulls a full AV1 encoder plus exr, tiff, webp, gif, jpeg and rayon. Set `image::Limits` explicitly |
| `rfd` | 0.17.2 | File dialogs | the XDG-portal handling that was the original reason is moot since 2026-08-25; it stays as the ordinary choice for a native dialog |

egui and `eframe` are MIT OR Apache-2.0, both compatible with GPL-3.0-or-later;
`winit` is Apache-2.0 only, which is compatible in the same direction.

Localisation is a loader in this repository, not a crate. Measured across all 37
locale directories: 128 keys each, no key difference against `en`, and zero
occurrences of any interpolation syntax, plural suffix or selector in all 4,631
strings. Every feature that distinguishes Fluent from a flat map is unused, and
the runtime i18next configuration uses only `resources`, `fallbackLng` and
`escapeValue`. So the JSON stays byte-for-byte, `serde_json` reads it, the
nested keys flatten once at startup into a map, and the whole surface is
`fn t(&self, key: &str) -> &str` — under 100 lines and no new dependency.
Keeping the format also keeps one Crowdin project, one format for volunteer
translators across 37 locales, and one tree that both the 1.x and 2.x clients
read unchanged during the beta. Two things the loader must carry that a bare
`HashMap<String, String>` would lose: the per-locale base text direction, and a
comment that `format!` covers the first string that ever needs formatting, so
nobody reopens the question on the strength of one interpolation.

Rendering needs a fallback chain, and it is required rather than desirable. The
evidence is first-party: the Electron client already disables hardware
acceleration on demand through a setting it shipped because the field made it
necessary. (It also disabled it unconditionally on Linux; that arm and the Linux rung
it justified both went on 2026-08-25.) Windows tries wgpu on DX12, then
wgpu on WARP; the CPU rung below it went on 2026-08-26, because WARP is that
rung. There is no
glow rung: glow needs GL 3.3 or ES 3.0, and a Windows machine without a vendor
driver offers software GL 1.1, so the rung does not save the RDP and bare-VM
cases it would exist for. Migrate the existing `hardware_acceleration` value
forward rather than inventing a new key, and keep automatic demotion
non-persistent by default — a key written by a process in the act of crashing
pins users to the slow rung for reasons that may have nothing to do with the
GPU.

A transparent, click-through window must be prototyped in P1+, before the GUI phase
begins — which it was, on Windows x64, Windows i686 and Linux, when all three were
targets. eframe's transparency issue
has been open since 2024-05-03 with activity this month, transparent windows
render solid black for many users, and the known workarounds are
renderer-specific — which matters because eframe picks one renderer per process
for all viewports. The immediate-viewport crash issue names precisely the
overlay's configuration: fullscreen plus transparent plus always-on-top. This is
hours of work now against a discovery in month nine of an 11.5-week phase.

### Support

| Crate | Version | Role | Note |
| --- | --- | --- | --- |
| `serde` | 1.0.229 | Settings, signalling payloads, offsets | |
| `serde_json` | 1.0.151 | The same, plus the locale files | separate row: the two crates have not shared a version number since 2019 |
| `thiserror` | 2.0.20 | Library errors | |
| `anyhow` | 1.0.104 | Binary errors | clear of RUSTSEC-2026-0190 by exactly one release — an argument for caret plus lockfile rather than an exact pin |
| `tracing` | 0.1.44 | Structured logging | never from the audio callback: a log line formats, allocates and takes the subscriber's lock |
| `tracing-subscriber` | 0.3.23 | Log file and filtering | |
| `zerocopy` | 0.8.56 | Parsing structures out of raw memory | kept for `KnownLayout`'s trailing-slice support, which `bytemuck` has no equivalent for; reference APIs banned on i686, see §7.1 |
| `parking_lot` | 0.12.5 | Locks outside the audio path | |
| `self_update` | 1.0 line, unreleased | Auto-update against GitHub Releases | 0.44.0 is not shippable — see below |

`self_update` 0.44.0 declares `quick-xml ^0.38` as a non-optional dependency, so
it compiles into every build including a GitHub-only one, and quick-xml 0.38.x
carries RUSTSEC-2026-0194 and RUSTSEC-2026-0195, both CVSS 7.5, both fixed only
at 0.41.0 — which `^0.38` cannot reach. There is no feature or lockfile escape,
and the pinned version fails this document's own advisory gate. The fix exists
only in the unreleased 1.0 line, which §7.6(1) bars from a shipped build, so
today no version of this crate satisfies both rules at once.

Two exits, and the choice is made at P7+ rather than now. Track the 1.0 line and
pin it exactly once it stabilises, with `default-features = false` and the ureq,
rustls, github, signatures, checksums and archive features named, `no_confirm`
set and output suppressed — the defaults block on an interactive stdin prompt a
windowed binary cannot answer. Note what that feature list does not buy: the
crate's `signatures` feature verifies a signed archive, and this project ships an
NSIS `.exe` and an AppImage, so the manifest signature below is ours to check
either way and exit 1 does not save the work. Or write the updater: a GET of the
releases API, a semver compare, download of artefact plus detached signature,
verification against an embedded key using `minisign`, then `self-replace`. That
is not writing crypto; the verification stays inside a purpose-built crate, and
the private key stays offline rather than in a release-workflow secret. Either
way the update path verifies against a key we hold, because without that the port
is a lateral move from `electron-updater`: the same trust root, minus the
differential download, plus a live advisory.

What that key carries changed with the signing decision. Windows artefacts are
**not** Authenticode-signed — no certificate is being bought, from SignPath,
Certum or anyone else — so users keep seeing the unknown-publisher warning on
first run, and publisher verification of the kind `NsisUpdater` performs is not
part of the 2.x story at all. A minisign signature over the update manifest,
verified in-process against a public key embedded in the binary before anything
is written or executed, is therefore the whole of update integrity rather than
the second of two controls. Say plainly what it does and does not cover: it
proves the artefact is the one this project published, and it does nothing about
SmartScreen, which scores reputation rather than provenance and will keep warning
about a first-seen unsigned binary no matter how it was verified. It is also why
the key stays offline. A release is a planned event with no availability
pressure, which is exactly the property the offsets bundle does not have and
exactly why that one ships unsigned instead (§5.6, `G0`); the two decisions look
opposite and rest on the same argument.

Immutable releases take one requirement off the verification path and add one to
the release path. A published manifest can no longer be edited, so the updater
never has to reason about a manifest that changed under a version it has already
seen, and the freeze rules that would otherwise defend against that are not
written — that is real work not done. In exchange there is no in-place
correction: a wrong manifest is superseded by a new tagged release, deleting a
release does not free its tag, and a staged rollout is a sequence of tagged
releases each with its own build, manifest and signature rather than a percentage
edited into a published file. `stagingPercentage` is not available to this
updater and no code should be written expecting it.

The two-process split adds one crate the tables above do not price yet:
`postcard`, for the length-prefixed frames between `acl-helper` and
`acl-core`. Version at adoption. The helper is the elevated half and holds
memory reading, injection, the keyboard poll and the overlay window; `acl-core`
is never elevated and holds tokio, signalling, WebRTC, audio and the GUI. The
overlay is in the helper because UIPI blocks window manipulation and
out-of-context `SetWinEventHook` across integrity levels, which is exactly the
configuration the README instructs users into — and the consequence for this
document is that the overlay receives pre-rasterised sprites over the IPC and
never decodes an image, so `image` never enters the elevated process.

## 7.3 Server

| Crate | Version | Replaces | Note |
| --- | --- | --- | --- |
| `axum` | 0.8.9 | `express` | |
| `socketioxide` | 0.18.6 | `socket.io` | `features = ["tracing", "extensions", "state"]` — none are default, and without `tracing` the internal error paths, including the rejected-malformed-payload log, emit nothing at all |
| `tower` | 0.5.3 | middleware | protocol-agnostic only: limit, timeout, retry, buffer, load-shed |
| `tower-http` | 0.7.0 | the middleware `tower` does not have | CORS, body limits, request-body timeout, HTTP tracing, panic catching. `tower` ships no HTTP-aware middleware whatsoever, so "tower replaces middleware" was only half true |
| `tracing` + `tracing-subscriber` | 0.1.44 / 0.3.23 | `morgan` + custom logger | |
| `tokio` | 1.53.1 | Node runtime | |

`socketioxide` is actively maintained (updated 2026-08-07) and is the strongest
single argument that the server port is low-risk. There is also no alternative
crate to fall back to, and it has a bus factor of one, so what it does and does
not do for us belongs in this document rather than in a commit message.

The server is **websocket-only**, and that is a decision rather than a
configuration detail. Engine.IO polling is not mounted: the socket.io route
accepts the WebSocket upgrade and nothing else, and a client that opens with
`transport=polling` is refused at the handshake. Both first-party clients already
pass `transports: ['websocket']` (§7.5), so nothing this project ships notices.
What does notice is any socket.io client that starts on polling and upgrades
afterwards, which is the default in the browser and in every mobile socket.io
binding. The mobile-client undertaking in 03 §3.5 is dropped for that reason
rather than softened: a future 4.x mobile client would be refused before it sent
a single event, and no amount of care on our side changes that while the polling
transport is absent.

The OBS overlay page is where this lands, because it is a browser client on
another origin — the settings dialog builds a URL at `obs.aucl.greluc.me`
carrying the server address — and it lives in neither repository, so its
transport configuration cannot be read from here and has to be checked against
the deployed page. It must connect with `transports: ['websocket']`, and that
change has to be deployed and verified **before** the server release, alongside
the event rework H3 already requires of the same page. One deployment, two
changes, the same ordering for both, and a page that ships only the event half
still breaks at the handshake.

Dropping polling also moves CORS out of the load-bearing path, which is the
opposite of what this section said while polling was on the table. A WebSocket
upgrade is not subject to CORS — there is no preflight, and no `CorsLayer`
decides whether the overlay page connects; its transport setting does.
`tower-http`'s `CorsLayer` therefore stays on the plain HTTP routes a browser
genuinely fetches, `/health` and `/lobbies`, and the socket.io route is left
without an origin allow-list deliberately: `Origin` is a header, trivially set by
anything that is not a browser, so an allow-list there rejects nothing that
matters and breaks the overlay in the field the first time the page changes host.
What constrains a connected client is the signal envelope rules, not where it
says it came from.

And `socketioxide`'s WebSocket path has no inbound frame cap. `max_payload` is
applied on the polling transport, which this server does not mount, and
advertised in the handshake OPEN packet, which a hostile client ignores — so
after the transport decision it constrains nothing at all and survives only as a
number in an OPEN packet. On the one transport there is, tungstenite's defaults
stand at 64 MiB per message. This cannot be fixed at a reverse proxy:
`client_max_body_size` stops applying at the Upgrade, and neither nginx nor
Caddy has a post-upgrade frame directive — both relay frames. The routes are an
upstream PR exposing `WebSocketConfig::max_message_size`, or a fork. Record it
as an accepted risk with an upstream issue filed against it, not as a
configuration line we have not written yet — and record that going websocket-only
made this the whole story rather than half of it.

`peerConfig.yml` becomes TOML or JSON. It holds an ICE server list — a handful
of url, username and credential fields — and it is not worth a YAML parser whose
own parser dependency is an archived c2rust transliteration of C (§7.5).
`serde_json` is already in the tree.

Configuration comes from the environment: systemd `EnvironmentFile=` or docker
`--env-file` plus `std::env::var`. TLS terminates at a reverse proxy and axum
binds to localhost, which takes `aws-lc-rs` out of the server binary entirely
and brings `limit_req`, `limit_conn`, certificate rotation and ACME with it. It
does not solve the frame cap above; nothing at that layer does.

## 7.4 Tooling and CI

| Tool | Version | Role |
| --- | --- | --- |
| `cargo-deny` | 0.20.2 | Advisories, licence policy, banned crates, source allow-list, build-script policy |
| `cargo-vet` | latest | Recorded dependency audits — read §7.6(7) before relying on it |
| `cargo-audit` | latest | RustSec advisories, also on a schedule |
| `cargo-fuzz` | latest | RTP → jitter buffer → decode, and the game reader through a `FuzzProcess` implementation of the existing `ProcessMemory` trait. Nightly-only; see §7.8 |
| `cargo-dist` | 0.32.0 | Installers for all three targets |
| `cargo-nextest` | latest | Test runner in CI — its process-per-test isolation is also what delivers the per-peer panic isolation the security section asks for |
| `cargo-about` | 0.9.2 | Third-party attribution notices. `cargo-deny` enforces licence policy but produces no notice file, and GPL distribution wants one |
| `cargo-auditable` | 0.7.5 | Embeds the dependency list in the shipped binary, so `cargo audit bin` works on an artefact months after the build. One line; cargo-dist already integrates it |

GitHub Actions keep the current convention: **every action pinned to a commit
SHA** and workflow-name-scoped concurrency groups. The matrix over three targets with
`fail-fast: false` went on 2026-08-25: one target needs no matrix, and has no sibling
whose failure it could hide. CodeQL stays.

Renovate or Dependabot is configured for `Cargo.toml` and for the workflow SHAs,
grouped so that a routine bump is one review rather than twenty.

Two things are absent from that table on purpose. There is no code-signing step:
Windows artefacts ship unsigned (§7.2), so there is no `signtool` invocation, no
certificate in a workflow secret, and no CA-specific release job to maintain or
to break when a certificate is renewed. And the minisign signature over the
update manifest is not produced in CI either — it is an offline step against a
key that never enters a GitHub secret, which a planned release can afford and an
offsets update cannot. Releases are immutable, so a workflow that publishes a
wrong manifest is corrected by tagging another release and never by re-uploading
an asset; the release job should not offer an overwrite path that no longer
exists on the other end.

## 7.5 The three stale dependencies, and what to do about them

There are three, not one, and the worst of them is on the server.

**`serde_yaml_ng` 0.10.0** (published 2024-05-26) is the worst because of what
it depends on rather than its own age: `unsafe-libyaml ^0.2.11`, whose
repository was archived by its owner in March 2024 and whose README describes it
as libyaml translated from C to unsafe Rust with the assistance of c2rust. That
is wall-to-wall `unsafe` with none of the reviewability this port is being
justified by, parsing an operator-supplied file, in the component that ships
first. RUSTSEC-2023-0075 against it is patched, but the fix came from a
maintainer who has since archived the repository, so there will be no next fix.
Move `peerConfig.yml` to TOML or JSON and the question disappears. If operator
compatibility makes YAML non-negotiable, `serde-saphyr` 1.1.0 forbids `unsafe`,
runs fuzzing and Miri in CI, and has no libyaml lineage — but say out loud that
it reached 1.0 four weeks ago. The reason to move is that the incumbent's parser
is an archived machine translation of C, not that the replacement has a better
pedigree.

**`dotenvy` 0.15.7** (2023-03-22) is the oldest artefact in this document at 3.4
years. The project is *not* abandoned — the repository was pushed 2026-08-18 and
has a live v0.16 branch — so this is a release-age call, not an abandonment
call. It is dropped anyway, because §7.6(5) asks why not the standard library
and this one has no answer: `EnvironmentFile=` or `--env-file` plus
`std::env::var` covers production with zero dependencies.

Neither of those is caught by `cargo-deny`, because nobody has filed an
unmaintained advisory against either. §7.6(3) checks a list, not reality, and
the policy should say so rather than let a green gate stand in for a judgement.

**`rust_socketio` 0.6.0** was last released in April 2024, and the age is the
least of it. It pulls `backoff` 0.4.0 under RUSTSEC-2025-0012 and, through it,
`instant` under RUSTSEC-2024-0384 — both unmaintained, both with an empty
patched-versions list, meaning no fixed version exists to move to. It also pulls
a second `reqwest`, a `tokio-tungstenite` at `^0.21` against the 0.30.0 above —
a duplicate major under cargo's 0.x rules — and `native-tls` plus `schannel` as
a third TLS stack. Two unfixable advisories and a pair of allow-list entries, on
the first commit that adds it, for a crate that parses unauthenticated server
input. It is not a starting point to be replaced later; CI is red from the day
it lands.

So the Socket.IO client is written from the start, against `tokio-tungstenite`,
and it moves out of P4 into **P1+** where it is built and conformance-tested
against the Node server P0+ has just proven. The scope is smaller than it looks
because the app is already WebSocket-only: both the voice client and the lobby
browser pass `transports: ['websocket']` today, so connecting directly with
`transport=websocket` deletes HTTP long-polling, the probe-and-upgrade
handshake, and base64 binary framing from the specification surface entirely.
What remains is five Engine.IO v4 packet types, five Socket.IO v5 types, the
packet grammar with the default namespace omitted, and one ack-id counter for
`join_lobby` — roughly 440 lines.

That deletion is now permanent on both ends rather than a client convention the
server tolerates: the Rust server does not mount the polling transport at all
(§7.3). No polling path is kept in the client against a future mobile binding
either, because that undertaking is dropped. Third-party 1.x servers are the Node
server, which accepts a direct `transport=websocket` connection exactly as ours
does, so websocket-only costs nothing there.

Five things go into the conformance suite by name, because they are how
hand-written v4 clients fail, and all five present as "it worked until it
didn't":

1. **Heartbeat direction.** In Engine.IO v4 the server sends `ping` and the
   client replies `pong`, reversed from v3. Get it backwards and everything
   appears to work until the server's `pingTimeout` fires.
2. `pingInterval`, `pingTimeout` and `maxPayload` are read from the OPEN packet,
   not hard-coded, and `pingTimeout` is the liveness deadline that feeds the
   reconnect policy.
3. The CONNECT ack carries a Socket.IO `sid` distinct from the Engine.IO `sid`.
4. Ack ids are released even when the server never acks `join_lobby`.
5. A `CONNECT_ERROR` is distinguishable from a transport close, so an auth
   rejection does not drive the reconnect policy.

Reconnection itself is not the transport's problem: the existing reconnect
policy is 34 lines of pure functions with no transport coupling and its tests
port across unchanged. The transport's only obligation is to report "closed"
honestly.

Two of this client's obligations are permanent, and that is worth writing down
here because the 1.x wire protocol is switched off when 2.0 ships and it would be
easy to schedule their removal alongside it. The `join_lobby` ack and the socket
lobby-browser events are what a 2.x client falls back to when the server offers
no `GET /lobbies/{id}/code` and no `/lobbies/stream` — which is every third-party
server, run by an operator who upgrades on their own schedule or not at all.
Switching our server off the 1.x format reaches our server and nothing else, so
`P9` deletes the 1.x path from the server and keeps both fallbacks in the client.
They are not transitional code with an expiry date; they are the client's answer
to an old server nobody here controls, and they are the only reason the answer to
that question is yes.

Two line items no line-count estimate of this job includes, and they are not
small: Chromium
supplies system proxy resolution and the Windows certificate store for free, and
`tokio-tungstenite` supplies neither. Users behind a TLS-inspecting corporate or
school proxy are the same population already forced onto TURN, and the symptom
is not degraded audio but "it will not connect at all". Budget
`rustls-platform-verifier` and a proxy resolver on all three targets — `ureq`
above is configured for the same reason.

The interface is a trait either way, so if the conformance suite cannot be made
to pass inside the budget, the fallback is more time on the hand-written client
and a narrower first cut — **not** `rust_socketio`, not even for one release.
Two unmaintained advisories with no fixed version, on the crate that parses
unauthenticated server input, is not a trade this project takes: CI is red from
the commit that adds it, and §7.7 records it as not usable even briefly.

## 7.6 Dependency policy

1. Latest stable at the time of each phase; no pre-releases in a shipped build.
   Release age is evidence, not a verdict. `realfft` 3.5.0 and
   `raw-window-handle` 0.6.2 are old because they are finished: a small,
   complete primitive with no open surface does not need commits to be healthy.
   The rule is about neglect, and the exception is stated here so a reviewer
   does not re-raise those two every quarter. The distinction is whether an
   unfixed problem exists, not when the last release was cut — which is exactly
   why §7.5's three entries are argued individually rather than by date.
2. Exact pins (`=x.y.z`) for the crates whose upstreams have shown they will
   break inside a minor or a patch: `sonora`, `webrtc-audio-processing`,
   `neteq`, `rubato`, `opus` and `webrtc`. Caret elsewhere with a committed
   `Cargo.lock` — `anyhow` clearing an advisory by one release is the argument
   for keeping caret where the upstream is disciplined.
3. `cargo-deny` blocks: unmaintained advisories, GPL-incompatible licences, and
   any source other than crates.io.

   Duplicate major versions of the same crate are a **warning** with a dated,
   reviewed allow-list (`bans.skip` / `skip-tree`), not a block. As a block it
   is unsatisfiable against this dependency set and would be switched off in
   week one, which would take the advisory and licence gates with it: measured
   on the best available networking configuration there are still 13
   duplicate-major pairs, because the RustCrypto ecosystem is mid-migration and
   `rtc-dtls` itself declares `rand ^0.10.1` and `rand_core ^0.6.4` side by
   side. Every entry carries the date it was added and the reason, and expires
   into a review rather than sitting forever.

   Two honest limits on this rule. It checks a list, not reality: `dotenvy` and
   `serde_yaml_ng` pass it cleanly and §7.5 rejects both. And several crates
   here need explicit `[licenses.clarify]` entries before the first CI run —
   `aws-lc-sys`, `ring`, `webrtc-audio-processing`'s `license-file`, and the
   MPL-2.0 `option-ext` that arrives under `directories`.
4. No git dependencies and no path dependencies on anything outside the
   workspace — the pre-1.0.0 client depended on unpinned branch HEADs of three
   native modules, and that is precisely what the vendoring in `native/` was
   done to end. The Rust version must not reintroduce it. Note what this rule
   does *not* cover as written: it constrains Cargo.toml sources, not
   build-script network access, so a crate that downloads a prebuilt binary at
   build time passes it. That is a gap to close in rule 6, not a technicality to
   argue about later. The offsets fetch violates the spirit of this rule today
   and must not be waived for itself.
5. Every new dependency is justified in the pull request that adds it: what it
   replaces, why not standard library, and its maintenance status.
6. Build scripts and proc macros are the actual code-execution vector in a Cargo
   build, and the supply-chain claim is unenforced without a clause about them.
   `[bans.build]` with `allow-build-scripts`, `executables = "deny"` (it reads
   file headers, so it works on Windows), `interpreted = "deny"`,
   `include-dependencies = true`, and per-crate SHA-256 `bypass` entries.
   Budget the bypass list as day-one work: `opus` and the APM baseline fail it
   immediately. Be honest about the ceiling — cargo-deny's own documentation
   says it cannot catch a build script that fetches from a remote server in
   vanilla Rust. This raises awareness; it is not a sandbox.
7. `cargo-vet` is imported from Mozilla's, Google's and the Bytecode Alliance's
   shared sets, and the policy must state what that does and does not buy.
   Across all three sets there are **zero** audits for `cpal`, `opus`,
   `webrtc-audio-processing`, `neteq`, `sonora`, `rubato`, `ringbuf`, `webrtc`,
   `tokio-tungstenite`, `windows-sys`, `directories`, `eframe`,
   `egui`, `winit`, `rfd` or `self-replace` — that is, for very nearly every
   crate whose failure would matter here. Making it blocking from P1 produces a
   large exemptions block and no assurance, so the policy states that here
   rather than letting the supply-chain table imply a coverage that does not
   exist. Two deltas
   are worth a real human audit rather than an exemption, and only two:
   `zerocopy` 0.8.27 → 0.8.56, because it parses attacker-influenced memory and
   both shared audit chains stop short of the pinned version, and whichever
   crate ends up verifying update signatures. The second is not a matter of
   thoroughness. With Windows artefacts unsigned, that verification is the only
   control over what an update installs, so an exemption there exempts the whole
   update path — and it is the one exemption this policy does not permit.
8. `--locked` on every cargo invocation in CI. The lockfile is committed; a
   lockfile that CI silently regenerates is not a lockfile, and none of the pins
   above mean anything without it.

## 7.7 Considered and dropped

One line each, so nobody re-adds them in month six on the strength of a good
crates.io page.

| Crate | Version considered | Why not |
| --- | --- | --- |
| `symphonia` | 0.6.1 | Out of the shipped runtime: a general-purpose demuxer and decoder framework to read one 55 KB constant that never changes. It stays available to the `xtask` that decodes the impulse response at build time — what is dropped is the parser in the binary, not the decode |
| `aec3` | 0.3.2 | AEC only — no NS, AGC or HPF. Strictly dominated by `sonora` |
| `fundsp` | 0.23 | A graph-and-combinator library with its own audio-rate execution model; adopting it for a handful of nodes means adopting the model and then fighting it for Web Audio parity |
| `biquad` | 0.6 | Takes a linear quality factor, but Web Audio's lowpass and highpass coefficient blocks use the decibel alpha term — so the spec's "Q −15" is legal and this crate will either reject it or silently produce something else |
| `libwebrtc` (LiveKit) | 0.3.45 | The integrated option genuinely exists and is declined for two reasons worth stating: no 32-bit x86 path in its build script, and a build-time download of a prebuilt binary |
| `rust_socketio` | 0.6.0 | `backoff` (RUSTSEC-2025-0012) and `instant` (RUSTSEC-2024-0384), both unmaintained with no fixed version, plus a second reqwest and a third TLS stack. Not usable even briefly — §7.5 |
| `reqwest` | 0.13.4 | 94 crates against `ureq`'s 27, and its 0.13 rustls default links `aws-lc-rs` beside webrtc-rs's `ring`, which puts NASM on the i686 build |
| `str0m` | 0.23.1 | No TURN client, and this app forces relay — §7.2 |
| `sysinfo` | 0.39.6 | 25 crates, `winapi` 0.3.9 and 107 MB of MinGW import libraries to save 3 ms per call over 25 lines of Toolhelp32; and it is what pins the heavier Win32 binding crate in the tree |
| `windows` | 0.62.2 | Replaced by `windows-sys` 0.61.2 once `sysinfo` goes. Both need exactly five `unsafe` blocks at the call site; the ergonomics it buys are ~150 lines of helpers the project wants anyway |
| `global-hotkey` | 0.8.0 | `RegisterHotKey` swallows the key, so a PTT key the game also uses never reaches the game; registration fails outright if Discord or OBS holds the combination; up to 50 ms of open microphone after release; X11 only. Not the key-up reason previously given here — `HotKeyState::Released` has existed since 0.4.0 |
| `rdev` | 0.5.3 | Last released 2023-06-26, and §7.6(4) forbids git dependencies, so fixes living only on HEAD are unreachable |
| `fluent` | 0.17.0 | 128 constant strings across 37 locales with no interpolation, no plurals and no selectors; and FTL identifiers cannot contain dots, which is what Crowdin and every call site key on |
| `arboard` | 3.6.1 | Already in the tree through `egui-winit`'s default `clipboard` feature, with `smithay-clipboard` on Wayland. A direct line adds no capability and is version-drift surface |
| `askama` | 0.16.0 | A proc-macro template engine and a build input for one status page; `/health` and `/lobbies` go through serde_json and need no templating |
| `serde_yaml_ng` | 0.10.0 | Its parser is `unsafe-libyaml`, an archived c2rust transliteration of C — §7.5 |
| `dotenvy` | 0.15.7 | The environment covers it: `EnvironmentFile=` or `--env-file` plus `std::env::var` — §7.5 |

## 7.8 What the toolchain does not yet cover

`cargo-fuzz` needs nightly, and libFuzzer with sanitizers is in practice a Linux
story. Neither fits §7.1 as written: one pinned stable channel and, as it then was, a
three-target matrix.

> **Still true after 2026-08-25, and worth saying because it looks as though it should
> not be.** Dropping Linux as a *target* does not drop it as a *runner*. Fuzzing
> `x86_64-unknown-linux-gnu` remains the plan: what is fuzzed is an RTP parser and a
> `FuzzProcess` over `Arbitrary` bytes, neither of which is platform code. The
> qualification in the last sentence below is the one that matters, and it is
> unchanged. The resolution is cheap but it has to be decided now rather
than discovered in the phase that needs it — a second pinned toolchain entry
used by the fuzz job alone, pinned by date so it does not float, and an explicit
statement that fuzzing runs on `x86_64-unknown-linux-gnu` only. That is an
acceptable answer for the RTP and jitter-buffer targets, which are
platform-independent parsers. It is a weaker answer for the game reader, whose
`FuzzProcess` implementation exercises struct parsing whose alignment behaviour
differs on exactly the target that cannot be fuzzed; the mitigations for that
live in §7.1, not in the fuzzer.

Two smaller items belong in the same place, because both are toolchain
configuration rather than dependency choices. `.cargo/config.toml` should carry
`-C control-flow-guard=yes` for the Windows targets and `-C
link-arg=/CETCOMPAT` for x86-64: Rust on MSVC gets ASLR and DEP by default but
neither of these, Chromium ships with CFG on, and they are free. And NASM
belongs in the i686 CI image if any crate in that build resolves to `aws-lc-rs`
— the client avoids it by using `ring` throughout, but the P4+ spike builds on
i686 and it is nearly free to discover while the rig is standing.

One thing the toolchain cannot cover, stated here so it is not mistaken for
something `cargo audit` handles: RustSec does not systematically track CVEs in
the C vendored inside `-sys` crates. `opus` has no advisory page; `ring` and
`aws-lc-sys` do. So `cargo audit` will not report a libopus or APM security
release. Today one Electron bump patches libopus, libvpx, BoringSSL, libpng and
the whole WebRTC stack at once, against a public feed with CVE numbers. After
the port that becomes a named human responsibility with a named upstream watch
list — an item that has to be created deliberately, because no tool in §7.4
produces it.
