# 6. Security analysis

Two questions: what does the port fix, and what does it put at risk that is
currently safe.

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

### Chromium leaves the attack surface

Electron 43 bundles Chromium. Every Chromium security release is a release this
project has to ship, and a user who does not update is exposed through a browser
engine they did not know they were running. A Rust client has no browser engine.

### The C/C++ native modules become checked code

4,390 lines of hand-written C and C++ perform pointer arithmetic on another
process's address space, parse structures out of that memory, and install a
low-level keyboard hook. They are vendored into this repository and were patched
by hand for const-correctness to build under Electron 43. In Rust the same
operations are `unsafe` blocks of a few lines each, surrounded by checked code,
with the buffer arithmetic done by the compiler.

The remaining genuinely `unsafe` surface after the port is small and
enumerable: the process-memory syscalls, the hook callback, the overlay's window
style manipulation, and the FFI into libopus and the APM. Each gets a safety
comment and `unsafe_op_in_unsafe_fn = "deny"` forces them to be explicit.

### Supply chain

| | Today | After |
| --- | ---: | --- |
| Client packages | 699 | ~250–350 crates |
| Install-time script execution | 6 packages allowed to run scripts | `build.rs` only, auditable |
| Native builds at install | 3 modules compiled on the user's machine | none — shipped compiled |
| Verification tooling | `npm audit` | `cargo-deny` + `cargo-vet` + `cargo-audit` |

crates.io versions are immutable, which removes the unpublish/republish class of
attack. `cargo-vet` allows recording who audited which dependency version, which
npm has no equivalent for.

### Update integrity

`electron-builder.yml` configures no code signing on either platform, so
`electron-updater`'s Windows publisher-name check has nothing to verify against.
Integrity currently rests on HTTPS to GitHub Releases plus the SHA-512 checksum
in `latest.yml` / `latest-linux.yml` — which is served from the same host as the
artefact, so it detects corruption but not a compromised release.

The port must sign artefacts and verify a detached signature against a key
embedded in the binary before applying an update, on both platforms. Given that
the client may run elevated (§6.3), this is a requirement in phase 7, not a
nice-to-have.

## 6.2 What the port puts at risk

Being honest about the other direction.

### Young dependencies in the most sensitive path

`neteq` 0.9.1, `sonora` 0.2.0 and `aec3` 0.3.2 have fewer than 80k downloads
between them and are all pre-1.0. They process attacker-influenced input: RTP
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

`webrtc-audio-processing` with `bundled` compiles a C++ tree at build time, and
`opus` builds libopus. Those are build-time code execution on the build machine
and on CI. They are pinned, vendored upstream and widely used, but they are the
reason the "no C++ anywhere" version of this port is not currently realistic. If
`sonora` matures, dropping `webrtc-audio-processing` removes the larger of the
two.

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
- **The client reads another process's memory**, which requires the same
  privilege level as the game and, if the game runs elevated, means the client
  runs elevated. An elevated client with an auto-updater is a meaningful target,
  which is why update signature verification in §6.1 is a requirement.

## 6.4 Security checklist for the port

- [ ] `cargo-deny` and `cargo-vet` blocking in CI from phase 1
- [ ] `unsafe_op_in_unsafe_fn = "deny"`, safety comment on every `unsafe` block
- [ ] One checked helper for all remote reads; no call site computes a length
- [ ] Injection feature-gated, 32-bit only, verifies target bytes before patching
- [ ] `cargo-fuzz` over RTP → jitter buffer → decode, in CI, corpus committed
- [ ] Panic isolation per peer; a bad peer cannot take down the process
- [ ] Update artefacts signed and verified on both platforms
- [ ] Audio-processing crates pinned exactly, bumps reviewed individually
- [ ] No network access from the audio or game threads
- [ ] Hat collection fetch: size-limited, timeout-bounded, images decoded with
      `image`'s limits set, never executed
- [ ] CodeQL retained; add `cargo-audit` on a schedule as well as on push
