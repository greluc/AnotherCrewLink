# 5. Regression strategy

The requirement is that the port introduces no regressions. For a rewrite of a
working application that means one thing: **the old implementation is the
specification, and parity has to be measured, not asserted.**

Everything below exists to turn "it sounds fine to me" into a number that CI can
check.

## 5.1 The principle

For each layer, capture the current implementation's behaviour as data, commit
that data, and make the Rust implementation reproduce it.

| Layer | Reference captured from | Committed as | Tolerance |
| --- | --- | --- | --- |
| Game reader | Instrumented Electron build, real sessions | Memory-region recordings + expected `AmongUsState` | exact (floats 1e-6) |
| DSP nodes | `OfflineAudioContext` in the Electron build | 32-bit float WAV + SHA-256 | −80 dBFS RMS error |
| Proximity rules | Instrumented `calculateVoiceAudio` | `(inputs) → (gain, pan)` tuples | exact |
| Receive path | Electron client under emulated impairment | Latency + MOS numbers | ≤30 ms, ≤0.2 MOS |
| Signalling | Node `socket.io-client` against both servers | Recorded event traces | exact |
| Settings | Real 1.x `config.json` files | Fixture files | exact after migration |
| Offsets bundle | The 81 real upstream files, plus a hand-built bad corpus | Fixture bundles | every real file accepted, every bad one rejected |
| Resource use | Current Electron build, three renderer configurations | Recorded numbers per configuration | no worse than the recorded figure |

## 5.2 Test layers

### Unit

Pure functions get exhaustive table tests. The port creates a lot of these,
because it moves logic out of places where it could not be tested:

- `voice_params` — the whole proximity ruleset, ~150 cases. Currently untestable
  (180 lines inside a React component).
- `poseCollide` and the collider tables — `ColliderMap.test.ts` ports directly.
- `reconnectPolicy` — its tests port directly.
- `validateClientPeerConfig` — its tests port directly.
- Offset store retry/backoff — `offsetStore.test.ts` ports directly.
- VDF parsing — `vdf.test.ts` ports directly.
- Keycode mapping — a table test that would have caught the 1.0.0 bug where the
  left-arrow key was mapped to Home and CapsLock was missing entirely.

Five existing test files port across essentially unchanged. That is a meaningful
head start and they should be ported in phase 1, not later.

### Golden-vector (DSP)

The mechanism that makes audio parity objective.

```
Electron build ──► OfflineAudioContext ──► node under test ──► f32 WAV ──► tests/golden/
                                                                              │
Rust build ─────► same node, same input ──► f32 WAV ──────────► compare ◄──────┘
```

Inputs per node: a unit impulse, white noise with a fixed seed, a 20 Hz–20 kHz
sine sweep, and five seconds of speech. Configurations: exactly those the app
uses (§3.3), plus the boundaries — zero distance, `maxDistance`, distance beyond
`maxDistance`, gain 0 and 1.

Comparison is RMS error of the difference signal, not sample equality: FFT
convolution and floating-point ordering legitimately differ in the last bits.
−80 dBFS is roughly 60 dB below anything audible.

The impulse response used by the reverb (`static/sounds/reverb.ogx`) is decoded
by Chromium today. Nothing decodes it at runtime in the port: the file is 55,589
bytes, ships inside the application and never changes, so an `xtask` decodes it
once at build time and the raw PCM is embedded. Its decoded samples are still
captured as a golden vector and compared against Chromium's decode of the same
file, so a decoder difference shows up as a build failure rather than as a
mysterious reverb difference. The xtask prints the decoded frame count, which is
how the embedded size becomes a recorded number instead of an estimate.

### Replay (game reader)

`ReplayProcess` implements `ProcessMemory` over a recording, so the whole reader
runs in CI on Linux with no game and no Windows.

Recordings must cover, per map: lobby, task phase, meeting, a player in a vent, a
player on cameras, comms sabotaged, doors closed, a player dead, a player
disconnected, and a lobby with a duplicate player id (the case 1.0.0 added a
warning for). Recording once per map across the five maps is a day of work and
it is the difference between a reader that is tested and one that is not.

Recordings also pin down the awkward cases the code documents: a
`gameOptionsPtr` that resolves to zero on Among Us 17.4.0 x86 (which is why the
map falls back to ShipStatus), and a null `objectCachePtr`.

### Fuzzing

Two targets, not one. The RTP path — depacketiser, jitter buffer, decode — is
already scoped in §6.2. The game reader joins it: `ProcessMemory` is already a
trait, so a `FuzzProcess` backed by `Arbitrary` bytes answering from a sparse map
sits alongside `ReplayProcess` and makes `AmongUsState::read_from` fuzzable for
almost nothing. Two hazards are reachable today from a modded or corrupted game
process and neither appears in the replay recordings, because the recordings are
of a game behaving: a self-referential pointer chain, which loops forever, and an
attacker-influenced array length used to size a `Vec`, which is an out-of-memory
kill. Cap chain depth and element counts, and let the fuzzer prove the caps hold.

The cost is a constraint on how `aucl-game` is written rather than on the test:
the parsing layer has to be pure — `&dyn ProcessMemory` in, `Result` out, no
`unwrap`, no `as` truncation — or the fuzzer finds panics that are artefacts of
the harness. That is a decision taken before the crate exists, not after.

`cargo-fuzz` needs a nightly toolchain and runs on Linux only, which the single
pinned stable channel in §7.1 does not provide. It runs as a scheduled job on its
own toolchain, outside the blocking matrix.

### Network emulation (receive path)

The harness from §4.5(3d). It runs against both implementations so the numbers
are comparable, and it runs in CI on every change to `aucl-audio`, because the
failure mode it guards against — audio that degrades under loss — is invisible
in a clean local test and is exactly what users report.

Profiles: `{loss 0,1,2,5,10 %} × {jitter 0,20,50,100 ms} × {reorder 0,1,5 %}`,
plus a 500 ms freeze and a 10 % → 0 % recovery ramp.

The same rig carries the loss profiles into a live 1.x↔2.x call rather than only
into one implementation's receive path, at 1, 2, 5 and 10 % loss. That is `G3`'s
added leg, and it is not redundant with the job above: the emulation job measures
one receive path, while Opus in-band FEC is a property of the pair. A Chromium
sender emits FEC only once receiver reports tell it there is loss, so a Rust peer
that reports nothing suppresses FEC in **both** directions — clean on a LAN,
broken at 3 % loss, and indistinguishable from a 1.x bug to the person reporting
it. A clean-network interop test cannot see it at all.

### Audio device hot-plug (manual)

Named, because the alternative is that it is nobody's job. Before `G2` signs off,
on Windows and on PipeWire, with a call live: unplug and replug a USB microphone,
switch the Windows default device, and connect and then disconnect a Bluetooth
headset. Each leg ends with audio working again and with the app having said
which device it fell back to, rather than falling back silently — the silence is
what made the original bug invisible.

This is the app's most-reported symptom class: "my microphone stopped working".
It is also the one place the port can measurably improve on the Electron build,
because cpal 0.18 added `ErrorKind::DeviceChanged`, WASAPI rerouting for default
streams and `DeviceNotAvailable` on ALSA disconnection. That backend is ten weeks
old and has four open issues sitting on exactly this path, so the port is an early
user of the fix as much as a beneficiary of it. The test exists to find out which.

### Interop

The one test that cannot be automated cheaply and must be run by hand before
each release milestone:

1. 1.0.2 Electron client + Rust client, same lobby, direct connection.
2. Same, forced through coturn (`forceRelayOnly`).
3. Same, across a symmetric NAT.
4. Three clients, mixed generations — 1.0.2, hardened 1.x and Rust — in one
   lobby, one leaves and rejoins. This row is also a `G3` criterion.
5. Windows x64 ↔ Linux; Windows i686 ↔ Windows x64.

A written checklist, recorded results, in the release notes.

### End-to-end

A scripted session against a real Among Us instance on Windows, per release:
launch, join a lobby, play a round, meeting, death, vent, camera, sabotage,
leave. Compared against the same script on the Electron build.

### Performance baseline

Idle resource use is one of the port's headline claims and §2.4 marks its own
figures as estimated. An estimate cannot be regression-tested, so the baseline is
measured on the **current Electron build first**, before there is a Rust client to
compare it to. Until that exists the order-of-magnitude claim should not be quoted
publicly.

It is a matrix of three configurations rather than one, because
`src/main/index.ts:37-39` already disables hardware acceleration unconditionally
on Linux and on demand on Windows — three configurations are what the project
actually ships:

| Configuration | Measured |
| --- | --- |
| Windows, GPU | Idle CPU, RSS, GPU memory, cold start to a usable window |
| Windows, software-rendered | Same |
| Linux, software-rendered | Same |

Two further numbers are taken in the configuration users actually run — GUI
visible, overlay up, game running: **audio callback overruns and dropped frames
per minute**. `G2` measures audio offline with no GUI and no game, which is the
one configuration nobody runs, so a regression that only appears when the overlay
is compositing would pass every other test in this document.

## 5.3 The bugs that must not come back

1.0.0–1.0.2 fixed a specific set of problems. A rewrite reintroduces exactly this
kind of bug, because the fix lives in a line of code that the porter reads as
noise. Each becomes a named test, and the test name says what it guards.

| Test name | Guards |
| --- | --- |
| `map_falls_back_to_ship_status_when_options_pointer_is_zero` | walls-block-audio silently doing nothing on 17.4.0 x86 |
| `camera_lookup_tolerates_unknown_map_and_camera_id` | an out-of-range camera throwing out of the audio pass and muting everyone |
| `convolver_is_skipped_until_impulse_response_is_decoded` | a null convolver buffer outputting silence rather than passing audio |
| `effect_is_connected_before_direct_path_is_dropped` | a failed effect connection leaving a player with no output |
| `talking_state_is_cleared_for_disconnected_players` | a speaking ring stuck lit on a player who left |
| `settings_apply_however_the_panel_is_closed` | the title-bar button discarding lobby settings |
| `player_volume_map_is_pruned_not_emptied` | losing every per-player volume past 50 entries |
| `microphone_id_falls_back_to_label` | a device id changing across driver updates |
| `push_to_talk_release_is_unconditional` | the impostor radio leaving the microphone open forever |
| `left_event_is_distinct_from_connection_failure` | not being able to tell a departure from a broken link |
| `leave_does_not_run_lobby_cleanup_twice` | double-counting departures |
| `arrow_and_capslock_keycodes_are_correct` | left arrow mapped to Home; CapsLock missing |
| plus the four connection tests in §4.6 | the paired audio dropouts |

## 5.4 CI

| Job | Runs on | Blocking |
| --- | --- | --- |
| `fmt` | Linux | yes |
| `clippy -D warnings` | Linux, Windows | yes |
| `test` (unit + golden + replay) | Linux, Windows x64, Windows i686 | yes |
| `network-emulation` | Linux | yes on `aucl-audio` changes |
| `cargo-deny` (advisories, bans, licenses, sources) | Linux | yes |
| `cargo-vet` | Linux | yes |
| `no-alloc-in-audio-callback` | Linux | yes |
| CodeQL | Linux | yes |
| `cargo-dist` build | all three targets | yes on tags |
| `fuzz` (RTP path, game reader) | Linux, nightly toolchain | scheduled, not on pull requests |
| Interop checklist | manual | before each milestone |
| Audio device hot-plug checklist | manual | before `G2` sign-off |
| Bridge rehearsal on real 1.0.2 installs | manual | before `G4` |

Actions stay pinned to commit SHAs, as they already are.

Two rows in that table are weaker than they read, and the document should say so
rather than let a green tick imply otherwise.

`cargo-deny`'s ban on duplicate major versions is unsatisfiable against this
dependency set. The RustCrypto ecosystem is mid-migration, and on the best
available networking configuration there are still thirteen duplicate-major pairs
before the GUI stack is counted. It therefore runs as a **warning against a dated,
reviewed allow-list**, not as a hard failure. The cost of pretending otherwise is
higher than the cost of the warning: a policy that fails on day one gets switched
off in week one, and it is the central supply-chain claim of the port.

`cargo-vet` runs with a large exemptions block, because across Mozilla's, Google's
and the Bytecode Alliance's shared audit sets there are **zero** audits for the
crates that carry the risk here — cpal, opus, the APM, neteq, webrtc,
tokio-tungstenite, the Windows bindings, eframe and the update crate among them. A
green `cargo-vet` means the exemptions file is current, not that anything was
audited. Two items are worth a real human audit and should be named as such rather
than counted as coverage: the update crate, and the `zerocopy` 0.8.27 → 0.8.56
delta, because that is the code parsing attacker-influenced game memory.

## 5.5 Cross-platform coverage

The current CI matrix is Windows x64 and Linux x64. The port adds
Windows i686, because the shellcode path is 32-bit-only and is currently built
but never exercised in CI.

Reader and injection tests that need a real process run on Windows runners
against a stub target process built for the purpose — not against Among Us,
which cannot be installed on CI. The stub reproduces the memory layout the
recordings captured, which is enough to test `ProcessMemory`, the scanner and
`VirtualAllocEx`/`WriteProcessMemory` without the game.

The i686 leg also carries a gate item rather than only coverage. `G2` requires a
green `cargo build --target i686-pc-windows-msvc` of whichever audio processing
module ships. The default is `sonora` 0.2.0, whose i686 status is genuinely
unproven — its own validation is Ubuntu x86_64 and its SIMD paths are SSE2, AVX2
and NEON. The fallback is not the previous default: `webrtc-audio-processing`
2.1.0 does not build on **either** Windows target, so it is kept only as a
Linux-only baseline for the A/B echo-return-loss measurement. If neither builds,
Windows ships without an echo canceller, which is an audible regression on day one
for the overwhelming majority of this app's users. That is why it is a gate and
not a CI convenience.

The whole leg exists for one reason: the 32-bit target is required by the
injection path and nothing else. If the open decision to split injection into a
small 32-bit helper process is taken, this matrix leg narrows to that helper and
its stub target, and the APM build above stops being existential.

## 5.6 Gate harnesses

Parity is measured here, so every gate criterion is a measurement, and a
measurement nobody has built is a criterion that gets waived at the moment it
would have been inconvenient. What follows is the harness each gate needs and
where in §5.2 it already lives. `G1` is unchanged. `G2` remains the
stop-the-port gate, and the amendments below make it harder to pass, not easier.

**`G0` — the offsets trust chain.** Five harnesses, four cheap and one that
cannot be run on demand.

1. A committed malicious-bundle corpus: unsigned, signed with the wrong key, a
   replayed lower `bundle_version`, truncated, and RVAs outside the module range.
   Each is rejected with a **distinct** error, and the previously-held bundle is
   still in force afterwards. Distinct errors are the point — one generic "bundle
   rejected" hides which control actually fired, and the next incident is the
   wrong time to find out.
2. An on-disk tamper test: edit the cached bundle between runs and confirm it is
   rejected. This proves verification happens at every load and not only at
   download, which is what closes local tampering with `offsets.json` in
   `userData` — a class no network-only fix reaches.
3. The validator accepts all 81 real upstream files unchanged. A validator that
   rejects real data is a self-inflicted outage, and this half is worth as much
   as the first.
4. A revocation drill: publish a bad bundle, supersede it, and confirm an
   affected client returns to a good state with no client release.
5. A timed drill against the next real Among Us update: a signed bundle published
   within six hours, recorded. This one waits for the game to ship an update, so
   it cannot be scheduled, and it is the criterion that decides whether inserting
   a second human into the offsets chain is affordable at all. An Among Us update
   arrives as a burst rather than as a single event, which is what the six hours
   have to survive.

`P2+`'s offsets work does not start before `G0`, and `G1` must still pass
byte-for-byte using the embedded bundle, which is what proves the bundle format
lost no data on the way in.

**`G2` criteria 5 and 6 — Opus in-band FEC recovery, and the i686 APM build.** Under a
5 % loss profile with a **Chromium sender**, the Rust receive path reconstructs
packet N from packet N+1 with `decode(..., fec: true)` driven by the jitter
buffer's loss signal, and `getStats()` on the Electron peer shows
`fecPacketsSent` climbing in both directions. The harness is the §5.2 network
emulation rig with a real 1.0.2 client standing in as the sender; the offline
receive-path measurement cannot produce this number, because the sender has to be
Chromium deciding for itself to emit FEC. `neteq` 0.9.1's documented surface says
nothing about out-of-order FEC recovery, so this criterion is where that is
settled — at the gate, where the cost of vendoring the reference NetEQ is still a
decision, rather than five weeks later at `G3`, where it is an emergency. The
green i686 build of the chosen APM from §5.5 is criterion 6, and the device
hot-plug checklist in §5.2 signs off alongside it.

**`G3` — impairment legs and a mixed-generation row.** Two additions, both run by
hand against a real 1.0.2 client. (a) The 1.x↔2.x call repeated under each `P3`
impairment profile — 1, 2, 5 and 10 % loss — scoring within 0.2 MOS of a 1.x↔1.x
call under the identical profile. (b) The three-client mixed-generation row from
§5.2's interop checklist, with one client leaving and rejoining. Neither is new
equipment; both are the existing rigs run against a pairing the clean-network
version of this gate never exercises.

**`G4` — bridge rehearsal.** On **real 1.0.2 installs**, not dev builds: Windows
x64, Windows ia32 and Linux each update from a staging feed to the bridge
release. Silent install, correct install directory, working uninstall entry,
migrated config, and on Linux the old process exiting within two seconds rather
than hanging. The architecture-selection leg is the one that is easy to skip and
expensive to get wrong: electron-updater's `findFile` prefers a filename
containing the literal `x64` or `ia32` and otherwise takes the first `.exe` in the
feed, so a misnamed artefact hands every 32-bit user a 64-bit installer and
nothing anywhere reports an error. The cost is three real installs and a staging
feed — roughly a day of setup — and it cannot be substituted with a dev build,
because the thing under test is what the *shipped* 1.0.2 updater does.
