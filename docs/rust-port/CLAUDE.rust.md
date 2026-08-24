# CLAUDE.md — AnotherCrewLink (Rust)

> This is the `CLAUDE.md` the Rust workspace starts with. Copy it to the
> repository root as `CLAUDE.md` at the start of phase 1, replacing the
> TypeScript one, and keep it current as crates land.

Proximity voice chat for Among Us. Reads the game's state out of process memory
and mixes every other player's voice by distance, walls, role and game state.

## Build and test

```bash
cargo build --workspace                  # debug
cargo build --workspace --release
cargo nextest run --workspace            # all tests
cargo nextest run -p aucl-audio          # one crate
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                         # advisories, licences, bans, sources
cargo run -p aucl-client                 # the app
cargo xtask golden --verify              # DSP golden vectors
cargo xtask netemu                       # receive path under emulated loss
```

The toolchain is pinned in `rust-toolchain.toml`. Do not float it on `stable`;
bump it in its own commit.

Targets: `x86_64-pc-windows-msvc`, `i686-pc-windows-msvc` (the injection path is
32-bit only), `x86_64-unknown-linux-gnu`.

## Layout

| Crate | Responsibility |
| --- | --- |
| `aucl-types` | `AmongUsState`, `Player`, map colliders, settings schema. No I/O. |
| `aucl-game` | Process memory, pattern scanning, offsets, shellcode injection |
| `aucl-audio` | Capture, APM, Opus, jitter buffer, the DSP graph, the mixer |
| `aucl-net` | Socket.IO signalling, WebRTC peer mesh |
| `aucl-platform` | Keyboard hook, overlay window, paths, single instance |
| `aucl-app` | The state machine wiring the above together |
| `aucl-ui` | egui views: main, settings, lobby browser, overlay |
| `aucl-client` | The binary |
| `server/` | `axum` + `socketioxide` signalling relay |
| `xtask/` | Build and release automation, in Rust rather than shell |

`aucl-types`, `aucl-game`, `aucl-audio` and `aucl-net` must build and test with
**no GUI dependency**. Do not add one.

## Rules that are not negotiable

### The audio render callback

**Never allocate, never lock, never log, never `.await` in the `cpal`
callback.** Parameters reach it through a lock-free ring buffer. CI runs a
debug allocator that panics if the render thread allocates; if that job fails,
the fix is in your change, not in the job.

### Parity is measured, not asserted

The Electron implementation is the specification. Before changing anything in
`aucl-audio::graph` or `voice_params`, read
[docs/rust-port/05-regression-strategy.md](docs/rust-port/05-regression-strategy.md).
Golden vectors under `tests/golden/` were captured from Chromium's own output
and are the contract. If a change moves a golden vector, that is a regression
until proven otherwise — do not regenerate the vector to make the test pass.

### `AmongUsState` has exactly one producer

`aucl-game` produces it; everything else reads it through `tokio::sync::watch`.
Nothing outside `aucl-game` constructs or mutates it.

### `unsafe`

`unsafe_op_in_unsafe_fn` is denied workspace-wide. Every `unsafe` block carries a
`// SAFETY:` comment saying what invariant makes it sound. All remote memory
reads go through the one checked helper in `aucl-game::mem`; no call site
computes a buffer length itself.

### Panic isolation

A panic in the receive path for one peer drops that peer. It must not reach the
process. The boundary is in `aucl-net::peer`.

## The bugs that must not come back

The 1.0.x releases fixed a specific set of problems, and a port reintroduces
exactly this kind of bug because the fix reads as noise. Each has a named test;
the name says what it guards. Before touching the area, read the test.

- `map_falls_back_to_ship_status_when_options_pointer_is_zero` — the game
  options pointer resolves to zero on Among Us 17.4.0 x86, which silently
  disabled walls-block-audio, comms sabotage and hearing through cameras.
- `convolver_is_skipped_until_impulse_response_is_decoded` — a convolver with no
  buffer outputs silence rather than passing audio through.
- `effect_is_connected_before_direct_path_is_dropped` — the other order leaves a
  player with no output if the second step fails.
- `signal_from_unknown_socket_is_ignored_not_crashed`,
  `trickle_candidate_without_type_is_forwarded`,
  `offer_glare_does_not_destroy_replacement`,
  `connection_stuck_in_new_times_out` — the four causes of "one player cannot
  hear one other player, but everyone else is fine".
- `push_to_talk_release_is_unconditional` — a role change between key-down and
  key-up used to leave the microphone open until restart.
- `player_volume_map_is_pruned_not_emptied` — the old code deleted every entry
  past 50, so anyone who had met 50 players lost all their volumes, repeatedly.

The full list is in
[docs/rust-port/05-regression-strategy.md](docs/rust-port/05-regression-strategy.md) §5.3.

## Wire compatibility

The Socket.IO protocol between client and server is **frozen**. A 1.x Electron
client, a 2.x Rust client and any mobile client speaking Socket.IO 4 share
lobbies. Changing an event name or payload shape breaks players who have not
updated. If an event has to change, it is added alongside the old one.

Eleven events, one namespace: `join`, `leave`, `id`, `setHost`, `signal`, `VAD`,
`lobby`, `remove_lobby`, `join_lobby`, `lobbybrowser`, `disconnect`.

## Settings

The schema is ported from the TypeScript version unchanged, defaults included,
so an existing `config.json` from a 1.x install keeps working. Migration is
tested against real files under `tests/fixtures/settings/`. Adding a field is
fine; renaming or repurposing one needs a migration step.

## Dependencies

Latest stable, no pre-releases in a shipped build. `webrtc-audio-processing` and
`neteq` are pinned exactly (`=x.y.z`) because they sit in the audio path and the
former does not follow semver strictly. No git dependencies, no path
dependencies outside the workspace — the pre-1.0 client depended on unpinned
branch HEADs of three native modules and that is what the vendoring ended.

Every new dependency is justified in its pull request: what it replaces, why not
the standard library, and its maintenance status.

## Style

- British spelling in prose, as in the existing changelog and documentation.
- Comments explain *why*, and are worth writing when the reason is not visible
  from the code — a magic constant from a specification, a workaround for a game
  build, an ordering that matters. Do not narrate what the line does.
- Errors: `thiserror` in libraries, `anyhow` in binaries.
- Logging: `tracing`. No `println!` outside `xtask`.
- Public items in `aucl-*` crates are documented; `missing_docs` is warned.

## Where the design lives

[docs/rust-port/](docs/rust-port/) — inventory, feasibility, architecture, plan,
regression strategy, security, dependencies. Read `03-target-architecture.md`
before adding a crate or moving a responsibility between them.
