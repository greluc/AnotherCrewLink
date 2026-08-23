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
by Chromium today and by `symphonia` in the port. Its decoded samples are
themselves captured as a golden vector, so a decoder difference shows up as a
decoder failure rather than as a mysterious reverb difference.

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

### Network emulation (receive path)

The harness from §4.5(3d). It runs against both implementations so the numbers
are comparable, and it runs in CI on every change to `aucl-audio`, because the
failure mode it guards against — audio that degrades under loss — is invisible
in a clean local test and is exactly what users report.

Profiles: `{loss 0,1,2,5,10 %} × {jitter 0,20,50,100 ms} × {reorder 0,1,5 %}`,
plus a 500 ms freeze and a 10 % → 0 % recovery ramp.

### Interop

The one test that cannot be automated cheaply and must be run by hand before
each release milestone:

1. 1.0.2 Electron client + Rust client, same lobby, direct connection.
2. Same, forced through coturn (`forceRelayOnly`).
3. Same, across a symmetric NAT.
4. Three clients, mixed versions, one leaves and rejoins.
5. Windows x64 ↔ Linux; Windows i686 ↔ Windows x64.

A written checklist, recorded results, in the release notes.

### End-to-end

A scripted session against a real Among Us instance on Windows, per release:
launch, join a lobby, play a round, meeting, death, vent, camera, sabotage,
leave. Compared against the same script on the Electron build.

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
| Interop checklist | manual | before each milestone |

Actions stay pinned to commit SHAs, as they already are.

## 5.5 Cross-platform coverage

The current CI matrix is Windows x64 and Linux x64. The port adds
Windows i686, because the shellcode path is 32-bit-only and is currently built
but never exercised in CI.

Reader and injection tests that need a real process run on Windows runners
against a stub target process built for the purpose — not against Among Us,
which cannot be installed on CI. The stub reproduces the memory layout the
recordings captured, which is enough to test `ProcessMemory`, the scanner and
`VirtualAllocEx`/`WriteProcessMemory` without the game.
