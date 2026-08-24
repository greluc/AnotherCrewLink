# 6. Security analysis

Two questions: what does the port fix, and what does it put at risk that is
currently safe. And one item that is neither, because it is already wrong and
the port would inherit it unchanged — the offsets supply chain, §6.5.

## 6.1 What the port improves

### The renderer's Node access disappears

The largest single item. Today:

```ts
webPreferences: {
    nodeIntegration: true,
    contextIsolation: false,
}
```

Every one of the three windows runs its page with full Node access — filesystem,
child processes, native modules. `hardenWindow()` blocks navigation, refuses
`window.open` to anything but the system browser, and prevents webview
attachment, which is a competent mitigation. But it is a deny-list guarding a
boundary that should not exist, and the renderer loads remote content: hat
images and hat geometry are fetched from `HAT_COLLECTION_URL` at runtime, and
avatar images are rendered from URLs in that collection.

A native GUI has no HTML, no script execution and no navigation. The class of
bug is removed rather than mitigated.

Note what `nodeIntegration: true` also does, because §6.2 depends on it: Electron
documents that it disables renderer sandboxing, and this app sets it on every
window. The renderer running WebRTC, Web Audio, Opus decode and hat-image decode
is therefore already unsandboxed. Moving those parsers into a native process is
not a sandbox regression, because there is no sandbox to lose. The regression is
elsewhere, and §6.2 says where.

### Chromium leaves the attack surface

Electron 43 bundles Chromium. Every Chromium security release is a release this
project has to ship, and a user who does not update is exposed through a browser
engine they did not know they were running. A Rust client has no browser engine.

### The C/C++ native modules become checked code

4,390 lines of hand-written C and C++ perform pointer arithmetic on another
process's address space, parse structures out of that memory, and poll global
key state every 60 ms. They are vendored into this repository and were patched
by hand for const-correctness to build under Electron 43. In Rust the same
operations are `unsafe` blocks of a few lines each, surrounded by checked code,
with the buffer arithmetic done by the compiler.

The remaining genuinely `unsafe` surface after the port is small and
enumerable: the process-memory syscalls, the key-state poll, the overlay's window
style manipulation, and the FFI into libopus and the APM. Each gets a safety
comment and `unsafe_op_in_unsafe_fn = "deny"` forces them to be explicit. On
Linux the reader can hold none of it, because `nix::sys::uio::process_vm_readv`
is a safe function whose lengths derive from the slices passed in.

There is no low-level keyboard hook among those lines, and the port must not
introduce one. `native/node-keyboard-watcher/src/lib/keyhandler.cpp` is a 60 ms
`GetAsyncKeyState` poll, aliased to `XQueryKeymap` on Linux; the port keeps the
poll. `SetWindowsHookEx(WH_KEYBOARD_LL)` would be new code described as a port,
and its callback runs on the installing thread's message pump — a desktop-wide
latency dependency in front of every keystroke on the machine, silently unhooked
if it ever exceeds `LowLevelHooksTimeout`, for no gain over a poll that
intercepts nothing.

The net C and C++ also runs the wrong way. The port deletes 4,390 lines and adds
libopus, the C and assembly of whichever crypto backend TLS resolves to, and —
unless `sonora` clears the `i686-pc-windows-msvc` build that gates it, which is
unproven — the bundled WebRTC APM C++ tree, an order of magnitude larger by
itself than everything being deleted. Net C and C++ in one address space goes
**up**, not down, and unlike today none of it sits in a separate process. The
defensible claim is the narrow one: the 4,390 lines this project wrote and
maintains stop being hand-checked. The C that remains is C nobody here writes,
and §6.2 is about what that costs.

### Supply chain

| | Today | After |
| --- | ---: | --- |
| Client packages | 699 | more than 350 crates |
| Install-time script execution | 6 packages allowed to run scripts | `build.rs` only, auditable |
| Native builds at install | 3 modules compiled on the user's machine | none — shipped compiled |
| Verification tooling | `npm audit` | `cargo-deny` + `cargo-vet` + `cargo-audit` |

The crate figure is a floor, not an estimate. The networking domain alone
resolves to 228 crates with the substitutions in
[07-dependencies-toolchain.md](07-dependencies-toolchain.md) applied, before
audio, GUI or platform bindings are added. Measure the real number against a
committed `Cargo.lock` at the end of phase 1 and requote it here; a first
comparison against 699 npm packages that a reader can falsify with one
`cargo tree` costs more credibility than it buys.

The honest summary of that table is narrower than "fewer dependencies". It is
that crates.io versions are immutable, which removes the unpublish/republish
class of attack npm has repeatedly suffered, and that install-time script
execution — today six packages allowed to run arbitrary code on the user's
machine during `npm install` — becomes `build.rs`, which is code a reviewer can
read in the dependency tree and which `cargo-deny`'s `[bans.build]` clause can
constrain. Both of those are real wins and neither depends on a count.

`cargo-vet` allows recording who audited which dependency version, which npm has
no equivalent for — but the ability to record an audit is not an audit. Across
the Mozilla, Google and Bytecode Alliance shared sets there are currently zero
audits for `cpal`, `opus`, `neteq`, `sonora`, `rubato`, `ringbuf`, `webrtc`,
`tokio-tungstenite`, `windows-sys`, `x11rb`, `directories`, `eframe`, `egui`,
`winit`, `rfd` or the update crate — that is, for every crate on the paths this
document is about. Importing the shared sets produces a large exemptions block
and no assurance. The last row of the table above is tooling available, not
coverage achieved, and it should be read that way. Two things are worth a human
audit rather than an exemption: the update
crate, and `zerocopy` 0.8.27 → 0.8.56, which is the code that parses
attacker-influenced game memory and whose existing audit chains stop short of the
pinned version.

### Update integrity

`electron-builder.yml` configures no code signing on either platform, so
`electron-updater`'s Windows publisher-name check has nothing to verify against.
Integrity currently rests on HTTPS to GitHub Releases plus the SHA-512 checksum
in `latest.yml` / `latest-linux.yml` — which is served from the same host as the
artefact, so it detects corruption but not a compromised release.

The failure is not passive. `quitAndInstall()` spawns the downloaded installer
with the current process token, `isAdminRightsRequired` is false for a per-user
install so there is no UAC prompt, and `verifySignature()` finds no publisher
name to check and returns null. A user who launched elevated to match the game —
which the README instructs — executes an unsigned downloaded binary as
administrator, silently.

The port must sign artefacts and verify a detached signature against a key
embedded in the binary before applying an update, on both platforms. Given that
the client may run elevated (§6.3), this is a requirement in phase 7, not a
nice-to-have.

## 6.2 What the port puts at risk

Being honest about the other direction.

### The parsers lose Google's fuzzing

This is the headline, and it is the one loss the port cannot buy back. libopus,
WebRTC's RTP and RTCP depacketizers, NetEQ, BoringSSL and the image codecs run on
ClusterFuzz and OSS-Fuzz continuously, at a scale measured in CPU-years, and
every fix that comes out of that arrives here today through one Electron bump.
`neteq` 0.9.1, `webrtc` 0.20.3 and `sonora` 0.2.0 are not fuzzed at anything
approaching that scale. `cargo-fuzz` in CI is the right thing to do and it is not
the same order of magnitude — it is core-hours a week against a corpus we seed
ourselves, against code that has had months rather than a decade of adversarial
attention.

No crate choice fixes this, because no Rust WebRTC stack has Chromium's fuzzing
budget. It can only be reduced: fuzz every path we can reach, keep a panic in one
peer from reaching the others, and put the parsers where a bug in them is worth
less. That last one is the next section.

### The whole pipeline in one elevated process

The shape this port must not take is Electron's process split replaced by a
thread split. Stated plainly, that result is one process —
elevated whenever the game is, unsandboxed, holding debug-level access to another
process's address space — containing the RTP and SRTP parser, the Opus decoder,
the jitter buffer, an image decoder fed by remotely fetched hats, the TLS stack
and a process-memory writer. Today those are at least spread across Electron's
main, renderer, GPU and utility processes. None of them is sandboxed (§6.1), but
a fault in one does not automatically hold the others, and the overlay is its own
`BrowserWindow`, so an overlay or GPU driver fault does not end the call.
`catch_unwind` around a peer contains a Rust panic; it does not contain a
memory-safety bug in libopus, and it does nothing at all about the elevation.

The answer is two processes, and it is cheap enough to be a requirement rather
than a roadmap item; [03-target-architecture.md](03-target-architecture.md) §3.2
now specifies them. `aucl-helper` runs elevated and holds memory reading,
injection, the key-state poll and the overlay window. `aucl-core` never elevates
and holds tokio, signalling, WebRTC, audio and the GUI. Between them,
length-prefixed `postcard` over a named pipe or a Unix socket.

The overlay is in the elevated half deliberately: UIPI blocks window manipulation
across integrity levels, so an unelevated overlay stops following an elevated
game, which is exactly the configuration the README tells people to run. The
consequence has to be designed in from the first commit — the overlay receives
pre-rasterised sprites over the IPC and never fetches or decodes an image, so no
image decoder enters the elevated process. What is left running elevated is then
a process with no listening socket, no HTTP client, no image decoder and no GPU
context, and every fuzzing target named in this document runs unelevated.

The cost is a launch question with no free answer — a per-launch UAC prompt or an
installed Windows service — and an IPC lifecycle to get right on three targets.
The scheduling is in [09-technology-migration.md](09-technology-migration.md)
§2.5.

### Young dependencies in the most sensitive path

`neteq` 0.9.1 and `sonora` 0.2.0 have fewer than 60k downloads between them and
are both pre-1.0 — `aec3` 0.3.2 was considered and dropped, being AEC only and
strictly dominated by `sonora`
([07-dependencies-toolchain.md](07-dependencies-toolchain.md) §7.7). They
process attacker-influenced input: RTP
payloads arriving from another player over a peer connection. A parsing bug in a
jitter buffer is a memory-safety-adjacent bug in Rust — a panic, not a code
execution — but a panic in the audio thread is still a denial of service against
the user.

Mitigations:

- **Fuzz the receive path.** `cargo-fuzz` over the RTP → jitter buffer → decode
  chain, in CI, with a corpus seeded from real captures. This is a hard
  requirement and it is something the current implementation gets for free from
  Chromium's own fuzzing.
- **Catch panics at the thread boundary.** A panic in the receive path for one
  peer must drop that peer, not the process.
- **Pin exactly** (`=0.9.1`, not `^0.9`) and review each bump.

### `unsafe` in the memory reader is now ours

Today the unsafe code is C++ in a vendored module; after the port it is `unsafe`
Rust in this repository. That is a net improvement in reviewability, but it means
the project owns it. `ReadProcessMemory` into a `&mut [u8]` is safe if the length
is right and unsound if it is not, and the compiler cannot check the length
against a remote address space. Every read goes through one checked helper that
takes a `&mut [u8]` and passes `buf.len()`; no call site computes a length.

### Injection is still injection

`VirtualAllocEx` with `PAGE_EXECUTE_READWRITE` followed by `WriteProcessMemory`
of hand-assembled shellcode and two `JMP` patches into a running process is the
same operation whichever language issues the syscall. Rust does not make it
safer. What the port should do:

- keep it feature-gated and off by default in any build that does not need it;
- keep it 32-bit-Windows-only, as today;
- document precisely what the two stubs do, in the code, next to the bytes —
  the current arrays have partial comments and are otherwise opaque;
- verify the target bytes before patching, so an unexpected game build fails
  closed instead of writing a `JMP` into the middle of an instruction.

That last point is a real improvement available for free: the current code
computes relative jumps from pattern-scan results and writes them without
checking that the five bytes it is about to overwrite are the five bytes it
expects.

### A larger `build.rs` surface

`opus` builds libopus, and `webrtc-audio-processing` with `bundled` compiles a
C++ tree — and demands meson, ninja and clang or gcc on every CI runner and every
contributor's machine on top of MSVC. Those are build-time code execution on the
build machine and on CI. They are pinned, vendored upstream and widely used, but
they are the reason the "no C++ anywhere" version of this port is not currently
realistic.

The APM decision moves most of that surface. `sonora` 0.2.0 is the default APM:
pure Rust, no C++ toolchain, no meson, no submodule, which removes the single
largest `build.rs` in the tree. `webrtc-audio-processing` is demoted to a
Linux-only test baseline, because it does not build on either Windows target —
PR #102 "Support MSVC targets" has been open and unmerged since 2026-08-08, issue
#34 "Windows build" since 2023-09-27, and its CI runs on `ubuntu-latest` only, so
nothing upstream would catch a regression even after #102 lands. That leaves
libopus as the one C tree the shipped client compiles. The conditional is real
and belongs at `G2`: `sonora`'s `i686-pc-windows-msvc` build is unproven, its
README validates only on Ubuntu x86_64, and if it does not go green the bundled
C++ tree comes back with it.

The build-time surface itself is enforceable, and today nothing enforces it.
`[bans.build]` in `deny.toml` — `allow-build-scripts`, `executables = "deny"`,
`interpreted = "deny"`, per-crate SHA-256 bypasses — is what turns the "auditable
`build.rs`" claim in §6.1 into a gate. Budget the bypass list as day-one work,
because `opus` fails it immediately, and be honest about the ceiling: cargo-deny
cannot catch a build script that fetches from a remote server in ordinary Rust.
It raises awareness; it is not a sandbox.

### The C keeps its memory-unsafety and loses its allocator

Chromium runs its C and C++ on PartitionAlloc: guard pages, freelist pointer
encoding, per-bucket type isolation, address space reserved rather than returned
on decommit. A Rust binary runs libopus, any bundled APM tree and the crypto
backend on the plain system allocator, and Rust's memory safety does not cross
FFI. The precise code that keeps its memory-unsafety in the port is the code that
loses its allocator hardening.

Two acceptable answers. Link a hardening allocator globally so the C inherits it
— mimalloc in secure mode is the cheap version — and measure the audio callback
afterwards, because a global allocator swap is not free on a real-time path even
when the callback is not supposed to allocate at all. Or accept it and say so.
What is not acceptable is silence, which lets §6.1's compiler-checked framing be
read as covering the C the compiler does not check.

### `cargo audit` will not tell you libopus shipped a fix

RustSec does not systematically track CVEs in C vendored inside `-sys` crates.
The asymmetry is easy to check: `openssl-src`, `aws-lc-sys`, `audiopus_sys` and
`ring` have advisory pages; `opus`, `libz-sys` and `curl-sys` have none. So the
tooling row in §6.1's table is narrower than it looks — `cargo audit` covers the
Rust, and the Rust is not where the memory-unsafety is.

Today one `electron` bump patches libopus, libvpx, BoringSSL, libpng and the
whole WebRTC stack at once, with CVE numbers and a public feed. After the port
that becomes a named person watching a named list. The list is short: Opus
security advisories and release
notes; the upstream WebRTC release notes for whichever APM ships, including
`sonora`'s own tracking of the M-line it was ported from; BoringSSL or aws-lc for
the TLS backend; and libpng through `image` and `png`. Put it in the repository
with the name of whoever owns it. It is the one part of the dependency-security
story that no tool in §6.4 performs.

None of the above is the largest risk in this project. That is §6.5, and it is
already live.

## 6.3 Threat model, unchanged by the port

Worth restating because the port does not alter it:

- **The server is unauthenticated.** Anyone can join any lobby whose code they
  know, spoof a `clientId` (the server logs the attempt and does not act on it),
  and read the public lobby list. That is inherited from CrewLink's design and is
  out of scope here.
- **Signalling payloads are relayed opaquely.** The server does not inspect SDP.
  A malicious peer can send arbitrary SDP to another peer; the WebRTC stack is
  the parser and therefore the boundary. This is another argument for fuzzing.
- **TURN credentials are shipped to every client** that connects, by design.
  `config/peerConfig.example.yml` in the server repository contains working
  public credentials; those should be treated as public and rotated if they ever
  gate anything that matters.
- **A hostile or compromised signalling server can redirect all media.**
  `validateClientPeerConfig` checks the *shape* of the ICE configuration the
  server pushes — urls present, credential a string, url parses — not its trust.
  Whoever controls the signalling server can set `forceRelayOnly` together with a
  `turn:` URL they own and route every lobby's audio through a relay of their
  choosing. DTLS-SRTP keys off the fingerprints in the relayed SDP, so this is
  metadata and availability rather than eavesdropping — but it is the one trust
  decision the client inherits and structurally cannot check, and it is unchanged
  by the port. Certificate pinning does not address it, because a
  configured-but-hostile server is already inside the relay path by design. What
  is worth tightening is narrower: `isUri` falls through to `new URL(value)`, so
  `data:` and `file:` schemes pass validation today.
- **The client reads another process's memory**, which requires the same
  privilege level as the game and, if the game runs elevated, means the client
  runs elevated. An elevated client with an auto-updater is a meaningful target,
  which is why update signature verification in §6.1 is a requirement.

### What the port inherits from 1.0.2

Three defects in the shipped client are not port risks. They are live today, and
a faithful port carries them across. They are recorded here because §6.2 is about
new risk and these are the other kind.

- **`mobilePlayerInfo` is handled above the `from` guard** in `Voice.tsx`, so any
  lobby member can turn on another client's mobile broadcast and then receive its
  full `AmongUsState` at 5 Hz — per-player `x`, `y`, `isImpostor`, `isDead` and
  `inVent`. That is a working positional wallhack plus impostor disclosure,
  reachable over the signalling server without touching WebRTC.
- **Lobby settings fall back to `serverHostId`** whenever the game's own `hostId`
  reads 0, and `setHost` is unauthenticated, so a lobby member can claim host and
  push `maxDistance: 1000`. `hostId` reads 0 whenever the pointer chain fails,
  which is exactly the state between an Among Us patch and an offsets update — so
  the window is neither narrow nor 32-bit-only.
- **The OBS secret is `Math.random().toString(36).substr(2, 9).toUpperCase()`.**
  Not a CSPRNG, about 46 bits, and it can return fewer than nine characters,
  which silently disables the overlay for that user.

The fixes ship on the 1.x line rather than waiting for the port, and are
scheduled in [09-technology-migration.md](09-technology-migration.md) §3.2.

## 6.4 Security checklist for the port

- [ ] `cargo-deny` and `cargo-vet` blocking in CI from phase 1
- [ ] `unsafe_op_in_unsafe_fn = "deny"`, safety comment on every `unsafe` block
- [ ] One checked helper for all remote reads; no call site computes a length
- [ ] `OpenProcess` with least privilege — `PROCESS_VM_READ |
      PROCESS_QUERY_LIMITED_INFORMATION` for the reader, `PROCESS_VM_WRITE |
      PROCESS_VM_OPERATION` only under `--features injection`,
      `PROCESS_CREATE_THREAD` never, and never `PROCESS_ALL_ACCESS` as the
      current C++ does
- [ ] Injection feature-gated, 32-bit only, verifies target bytes before patching
- [ ] `cargo-fuzz` over RTP → jitter buffer → decode, in CI, corpus committed
- [ ] `cargo-fuzz` over the game reader too, through a `FuzzProcess`
      implementation of the `ProcessMemory` trait; chain depth and array lengths
      capped, parsing layer kept pure so it is fuzzable at all
- [ ] Panic isolation per peer; a bad peer cannot take down the process
- [ ] Two processes: elevation confined to `aucl-helper`, which holds no
      listening socket, no HTTP client, no image decoder and no GPU context
- [ ] `.cargo/config.toml`: `-C control-flow-guard=yes` on both Windows targets,
      `-C link-arg=/CETCOMPAT` on x86_64 — both are off by default and Chromium
      ships with CFG on
- [ ] `[bans.build]` in `deny.toml`: `allow-build-scripts`,
      `executables = "deny"`, `interpreted = "deny"`, per-crate SHA-256 bypasses
      reviewed rather than accumulated
- [ ] Licence clarifications written before the first CI run, or the gate fails
      on day one: `aws-lc-sys`, `ring`, `webrtc-audio-processing`'s
      `license-file`, and MPL-2.0 `option-ext` arriving through `directories`
- [ ] Update artefacts signed and verified on both platforms
- [ ] Offsets bundle verified on every load including from cache, structural
      validator after the signature, full prologue checked before any patch
- [ ] A named owner and a written watch list for the C vendored inside `-sys`
      crates, because `cargo audit` does not cover it
- [ ] Audio-processing crates pinned exactly, bumps reviewed individually
- [ ] No network access from the audio or game threads
- [ ] Hat collection fetch: size-limited, timeout-bounded, images decoded with
      `image`'s limits set, off-thread with per-image `catch_unwind`, never
      executed, and never in the elevated process
- [ ] CodeQL retained; add `cargo-audit` on a schedule as well as on push,
      and `cargo-auditable` so `cargo audit bin` works on a shipped artefact

## 6.5 The offsets supply chain

The largest single risk in the project, and it is live today rather than
introduced by the port.

`src/main/offsetStore.ts` fetches `lookup.json` from an unpinned branch HEAD of a
third-party repository with no pin, no hash, no signature and no validation,
falling back to an unauthenticated local cache in `userData`. Every number in the
result is then used unchecked. `player.bufferLength` sizes an allocation. The
pointer chains that produce the player list, `hostId` and the game state are
offsets into another process's address space that arrive over the network.
`fixedUpdateFunc` and `modLateUpdateFunc` become the RVAs at which the injection
path writes a five-byte `E9 rel32` into `GameAssembly.dll`, and `disableWriting`
is the flag that gates that write — so whoever controls the file also controls
its own safety check. The write half is 32-bit-Windows-only, as injection is
everywhere else in this document, so it reaches a shrinking minority; the read
half reaches everyone. The client doing either may be running elevated, and none
of it requires compromising any infrastructure of ours.

It also violates the port's own dependency rule against unpinned branch HEADs
([07-dependencies-toolchain.md](07-dependencies-toolchain.md) §7.6), which must
not be waived for the one fetch it was written for.

The answer is the `H2` hardening track, written in TypeScript first so the bundle
format is proven in the field before the Rust reader consumes it. Mirror the
upstream tree into a repository we control and sync it by scheduled PR, so a
human sees the diff — 81 blobs, 284,896 bytes, a review a few times a year. Build
one canonical bundle carrying `bundle_version`, `min_acceptable_bundle_version`
and a target minimum client version. Sign it with minisign from a key held
offline, never in CI. Embed the build-time bundle with `include_bytes!` as a
floor the client can always fall back to. Verify on **every** load, including
from the cache, which is what closes tampering with the cached file that no
network-only fix reaches. Then run a structural validator, because a signature
answers "did we intend this" and validation answers "can this do harm", the two
fail independently, and the common real failure is an upstream generator emitting
garbage that is correctly delivered.

That reduces the trusted set from seven parties to two: an offline key and the
review on the mirror PR. It closes the arbitrary-location code write, the
unbounded `readBuffer`, and local tampering with `offsets.json`.

The cost is availability, and it is not zero. Inserting a second human between an
Among Us update and a working client is the price, and an Among Us update is a
burst rather than an event — the upstream history shows four cycles in one
evening. A client that cannot verify offsets fails closed on the game read, which
other players experience as "he cannot hear us". `G0` measures exactly that: a
timed drill against the next real Among Us update must publish a signed bundle
within six hours, and the validator must accept all 81 real upstream files
unchanged, because a validator that rejects real data is a self-inflicted outage.
`P2+`'s offsets work does not start before `G0` passes. If the drill fails, the
retreat is auto-merge on the mirror with post-hoc review, keeping the signature;
dropping the signature is not on the table.

The bundle format, the gate criteria and the schedule are in
[09-technology-migration.md](09-technology-migration.md) §2.1 and §3.2.
