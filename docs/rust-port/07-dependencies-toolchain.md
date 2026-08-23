# 7. Dependencies and toolchain

All versions verified against crates.io on **2026-08-23**. Every entry is the
latest stable release; nothing pre-1.0-of-its-own-line is chosen where a stable
alternative exists.

## 7.1 Toolchain

| | Version | Note |
| --- | --- | --- |
| Rust | **1.98.0** (2026-08-20) | pinned in `rust-toolchain.toml` |
| Edition | 2024 | |
| MSRV policy | = pinned stable | no back-compat burden; this is an application |
| Targets | `x86_64-pc-windows-msvc`, `i686-pc-windows-msvc`, `x86_64-unknown-linux-gnu` | i686 is required for the injection path |

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.98.0"
components = ["rustfmt", "clippy", "rust-src"]
targets = [
  "x86_64-pc-windows-msvc",
  "i686-pc-windows-msvc",
  "x86_64-unknown-linux-gnu",
]
```

The toolchain is bumped deliberately, one commit at a time, not floated on
`stable` — a floating channel means CI can break on a day nobody touched the
code.

## 7.2 Client

### Audio — the crates that decide the project

| Crate | Version | Role | Note |
| --- | --- | --- | --- |
| `cpal` | 0.18.2 | Device enumeration, input/output streams | 18.5 M downloads; the standard choice |
| `opus` | 0.4.0 | Opus encode/decode | binds libopus, same library Chromium uses |
| `webrtc-audio-processing` | **=2.1.0** | AEC3, noise suppression, AGC | pin exactly: the crate documents that it does not follow semver strictly |
| `neteq` | **=0.9.1** | Adaptive jitter buffer, PLC | young (38 k downloads); measured at gate G2 |
| `rubato` | 5.0.0 | Sample-rate conversion | |
| `realfft` | 3.5.0 | FFT for convolution and the analyser | |
| `symphonia` | 0.6.1 | Decodes the reverb impulse response | |
| `ringbuf` | 0.5.1 | Lock-free SPSC into the audio callback | |
| `hound` | 3.5.1 | WAV I/O for the golden-vector tests | dev-dependency |

Alternatives kept on file: `sonora` 0.2.0 (pure-Rust APM — revisit once it has a
track record; would remove the largest C++ build dependency) and `aec3` 0.3.2.

The DSP nodes themselves — panner, biquad, convolver, gain, analyser — are
written in this repository rather than taken from a crate. `fundsp` 0.23 and
`biquad` 0.6 were considered and rejected: parity with the Web Audio
specification is the requirement, and matching a third-party crate's coefficient
conventions to the spec's is more work than implementing five documented
formulas.

### Network

| Crate | Version | Role |
| --- | --- | --- |
| `webrtc` | 0.20.3 | ICE, DTLS, SRTP, data channels, TURN client |
| `rust_socketio` | 0.6.0 | Socket.IO client — **see §7.5** |
| `tokio` | 1.53.1 | Runtime |
| `reqwest` | 0.13.4 | Offset store, hat collection, update check |
| `tokio-tungstenite` | 0.30.0 | Fallback path if `rust_socketio` is replaced |

`str0m` 0.23.1 is the alternative to `webrtc`; see §2.3(d) for why it is not the
default.

### Platform

| Crate | Version | Role |
| --- | --- | --- |
| `windows` | 0.62.2 | Win32: process memory, hooks, window styles |
| `x11rb` | 0.14.0 | X11: key polling, XFixes input regions |
| `sysinfo` | 0.39.6 | Process enumeration |
| `directories` | 6.0.0 | Config, cache and log paths |
| `global-hotkey` | 0.8.0 | Considered for shortcuts; the low-level hook is kept because push-to-talk needs key-up as well as key-down |

`rdev` was rejected: last released June 2023.

### GUI

| Crate | Version | Role |
| --- | --- | --- |
| `eframe` | 0.36.1 | Application shell |
| `egui` | 0.36.1 | Widgets |
| `egui_extras` | 0.36.1 | Tables for the lobby browser |
| `winit` | 0.30.13 | Windowing (via eframe) |
| `wgpu` | 30.0.1 | Rendering (via eframe) |
| `raw-window-handle` | 0.6.2 | Overlay: reaching the native handle |
| `image` | 0.25.10 | Avatar recolouring, hat compositing |
| `fluent` | 0.17.0 | Localisation, 37 locales |
| `rfd` | 0.17.2 | File dialogs |
| `arboard` | 3.6.1 | Clipboard (OBS overlay URL) |

egui and `eframe` are MIT OR Apache-2.0, both compatible with GPL-3.0-or-later.

### Support

| Crate | Version | Role |
| --- | --- | --- |
| `serde` / `serde_json` | 1.0.229 | Settings, signalling payloads, offsets |
| `thiserror` | 2.0.20 | Library errors |
| `anyhow` | 1.0.104 | Binary errors |
| `tracing` | 0.1.44 | Structured logging |
| `tracing-subscriber` | 0.3.23 | Log file and filtering |
| `zerocopy` | 0.8.56 | Parsing structures out of raw memory |
| `parking_lot` | 0.12.5 | Locks outside the audio path |
| `self_update` | 0.44.0 | Auto-update against GitHub Releases |

## 7.3 Server

| Crate | Version | Replaces |
| --- | --- | --- |
| `axum` | 0.8.9 | `express` |
| `socketioxide` | 0.18.6 | `socket.io` |
| `tower` | 0.5.3 | middleware |
| `askama` | 0.16.0 | `pug` |
| `serde_yaml_ng` | 0.10.0 | `yaml` |
| `dotenvy` | 0.15.7 | `dotenv` |
| `tracing` + `tracing-subscriber` | 0.1.44 / 0.3.23 | `morgan` + custom logger |
| `tokio` | 1.53.1 | Node runtime |

`socketioxide` is actively maintained (updated 2026-08-07) and is the strongest
single argument that the server port is low-risk.

## 7.4 Tooling and CI

| Tool | Version | Role |
| --- | --- | --- |
| `cargo-deny` | 0.20.2 | Advisories, licence policy, banned crates, source allow-list |
| `cargo-vet` | latest | Recorded dependency audits |
| `cargo-audit` | latest | RustSec advisories, also on a schedule |
| `cargo-fuzz` | latest | RTP → jitter buffer → decode |
| `cargo-dist` | 0.32.0 | Installers for all three targets |
| `cargo-nextest` | latest | Test runner in CI |

GitHub Actions keep the current convention: **every action pinned to a commit
SHA**, matrix over Windows x64, Windows i686 and Linux x64, `fail-fast: false`,
and workflow-name-scoped concurrency groups. CodeQL stays.

Renovate or Dependabot is configured for `Cargo.toml` and for the workflow SHAs,
grouped so that a routine bump is one review rather than twenty.

## 7.5 The one stale dependency, and what to do about it

`rust_socketio` 0.6.0 was last released in **April 2024**. It implements
Socket.IO protocol rev 5 over Engine.IO rev 4, which is exactly what the 4.x
server speaks, so it works — but a two-year-old release in a project whose
stated goal is "nothing unmaintained" is a contradiction that should be resolved
rather than lived with.

The client's usage is small: one namespace, eleven events, JSON payloads only,
one acknowledgement callback (`join_lobby`). Engine.IO v4 over WebSocket is a
frame-type prefix, a handshake and a ping/pong. Implementing it directly on
`tokio-tungstenite` is roughly 400 lines and fully testable against the real
Node server.

**Plan:** start with `rust_socketio` in phase 4 to get moving; treat replacing it
as a scheduled task inside phase 4, not as future work. The interface is a trait
either way, so the swap does not reach the rest of the client.

## 7.6 Dependency policy

1. Latest stable at the time of each phase; no pre-releases in a shipped build.
2. Exact pins (`=x.y.z`) for `webrtc-audio-processing` and `neteq`; caret
   elsewhere with a committed `Cargo.lock`.
3. `cargo-deny` blocks: unmaintained advisories, GPL-incompatible licences,
   duplicate major versions of the same crate, and any source other than
   crates.io.
4. No git dependencies and no path dependencies on anything outside the
   workspace — the pre-1.0.0 client depended on unpinned branch HEADs of three
   native modules, and that is precisely what the vendoring in `native/` was
   done to end. The Rust version must not reintroduce it.
5. Every new dependency is justified in the pull request that adds it: what it
   replaces, why not standard library, and its maintenance status.
