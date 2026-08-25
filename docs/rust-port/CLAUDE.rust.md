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
cargo nextest run -p acl-audio          # one crate
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                         # advisories, licences, bans, sources
cargo run -p acl-core                   # the app (spawns acl-helper)
cargo xtask golden --verify              # DSP golden vectors
cargo xtask netemu                       # receive path under emulated loss
```

The toolchain is pinned in `rust-toolchain.toml`. Do not float it on `stable`;
bump it in its own commit.

Target: `x86_64-pc-windows-msvc`, and only that one. `i686-pc-windows-msvc` went on
2026-08-24 with the injection path it existed for; `x86_64-unknown-linux-gnu` went on
2026-08-25 with the client's Linux support, because nobody here can test it. Linux CI
*runners* are still used for work that has no target — formatting, licences,
advisories, CodeQL, fuzzing — and that is not the same thing as a Linux build.

## Layout

| Crate | Responsibility |
| --- | --- |
| `acl-types` | `AmongUsState`, `Player`, map colliders, settings schema. No I/O. |
| `acl-game` | Process memory, pattern scanning, offsets. Reads only: the injection path was removed on 2026-08-24 |
| `acl-audio` | Capture, APM, Opus, jitter buffer, the DSP graph, the mixer |
| `acl-net` | Socket.IO signalling, WebRTC peer mesh |
| `acl-platform` | Keyboard poll, overlay window, paths, single instance |
| `acl-ipc` | Helper ↔ core: `postcard` message types and framing |
| `acl-app` | The state machine wiring the above together |
| `acl-ui` | egui views: main, settings, lobby browser, overlay |
| `acl-helper` | Elevated binary: game reader, key poll, overlay window |
| `acl-core` | Unelevated binary: tokio, signalling, WebRTC, audio, GUI |
| `server/` | `axum` + `socketioxide` signalling relay |
| `xtask/` | Build and release automation, in Rust rather than shell |

`acl-types`, `acl-game`, `acl-audio` and `acl-net` must build and test with
**no GUI dependency**. Do not add one.

**Two binaries, not one.** `acl-helper` is the only elevated process and holds
memory reading, the key poll and the overlay window; `acl-core` never elevates and
holds tokio, signalling, WebRTC, audio and the GUI. They talk length-prefixed
`postcard` over a named pipe.
`acl-core` starts the helper **on demand, with a per-launch UAC prompt**. There
is no Windows service, nothing auto-starts and nothing elevated is resident
between sessions; the prompt is accepted friction, so do not "improve" it away
with a scheduled task, an installed service or a cached elevation token.
The overlay is in the helper because UIPI blocks window manipulation across
integrity levels; it receives **pre-rasterised sprites** and never decodes an
image, so no image decoder enters the elevated process. `acl-game` is never
linked into `acl-core`. See
[docs/rust-port/03-target-architecture.md](docs/rust-port/03-target-architecture.md) §3.2.

## Rules that are not negotiable

### The audio render callback

**Never allocate, never lock, never log, never `.await` in the `cpal`
callback.** Parameters reach it through a lock-free ring buffer. CI runs a
debug allocator that panics if the render thread allocates; if that job fails,
the fix is in your change, not in the job.

### Parity is measured, not asserted

The Electron implementation is the specification. Before changing anything in
`acl-audio::graph` or `voice_params`, read
[docs/rust-port/05-regression-strategy.md](docs/rust-port/05-regression-strategy.md).
Golden vectors under `tests/golden/` were captured from Chromium's own output
and are the contract. If a change moves a golden vector, that is a regression
until proven otherwise — do not regenerate the vector to make the test pass.

### `AmongUsState` has exactly one producer

`acl-game`, on the helper's game thread, produces it; it crosses the IPC once
and everything inside `acl-core` reads it through `tokio::sync::watch`. Nothing
outside `acl-game` constructs or mutates it.

### `unsafe`

`unsafe_op_in_unsafe_fn` is denied workspace-wide. Every `unsafe` block carries a
`// SAFETY:` comment saying what invariant makes it sound. All remote memory
reads go through the one checked helper in `acl-game::mem`; no call site
computes a buffer length itself.

### Panic isolation

A panic in the receive path for one peer drops that peer. It must not reach the
process. The boundary is in `acl-net::peer`.

### The offsets bundle is validated, never trusted

Offsets come from `greluc/AnotherCrewLink-Offsets`, a mirror we control, synced
from upstream by reviewed pull request. **Never fetch upstream directly, and
never follow a third party's branch HEAD** — that is what §7.6 of the dependency
policy forbids and it is how the 1.x client got its worst live problem.

The bundle carries **no signature**, by decision: an Among Us update is a burst,
and a human with an offline key between that burst and the users is what keeps
players out of the game. Everything therefore rests on the structural validator.
Run it on **every** load, including from the `userData` cache and including the
`include_bytes!` embedded floor, before a single number reaches `acl-game`.
Every offset is range-checked against the module before use; `bufferLength` is a
bound, not a hint; the full replayed prologue is compared before any patch, with
an explicit "already patched by us" state. A validator that rejects real data is
a self-inflicted outage, so changes to it run against the corpus of real
upstream files as well as the malicious one — both are in `tests/`.

See [docs/rust-port/09-technology-migration.md](docs/rust-port/09-technology-migration.md)
§2.1 for the reasoning and for the residual risk this accepts.

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

The Socket.IO protocol between client and server is **frozen** for as long as
1.x is in the field. A 1.x Electron client and a 2.x Rust client share lobbies;
changing an event name or payload shape breaks players who have not updated, so
if an event has to change, it is added alongside the old one.

Thirteen events, one namespace: `join`, `leave`, `id`, `setHost`, `signal`,
`VAD`, `lobby`, `remove_lobby`, `join_lobby`, `lobbybrowser`, `disconnect`,
plus `obs_state` and `mobile_state`, which replaced the legacy `signal`-to-room
overlay and mobile feeds in 1.0.5. The server enforces the envelope rules on
`signal` — `to` must be a current co-member, `to != from`, 64 KB cap — and a
client that addresses a room it is not in is refused, not warned.

**Transport is websocket-only.** Engine.IO polling is off on the server, and
there is no mobile client to keep it on for: the 4.x mobile promise was deleted
in 2026-08. Do not add a polling fallback.

Our server stops speaking the 1.x wire format when 2.0 ships. Third-party
operators do not, and never will, so the client keeps its `join_lobby` ack and
its socket lobby-browser events as permanent fallbacks for their deployments —
they are dead code against our own server and must not be deleted for that
reason.

## Settings

The schema is ported from the TypeScript version unchanged, defaults included,
so an existing `config.json` from a 1.x install keeps working. Migration is
tested against real files under `tests/fixtures/settings/`. Adding a field is
fine; renaming or repurposing one needs a migration step.

## Dependencies

Latest stable, no pre-releases in a shipped build. `sonora`, `neteq`, `rubato`,
`opus`, `webrtc` and the `webrtc-audio-processing` test baseline are pinned
exactly (`=x.y.z`), because each sits on the audio or media path and each
upstream has broken inside a minor or a patch. No git dependencies, no path
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
- Public items in `acl-*` crates are documented; `missing_docs` is warned.

## Where the design lives

[docs/rust-port/](docs/rust-port/) — inventory, feasibility, architecture, plan,
regression strategy, security, dependencies. Read `03-target-architecture.md`
before adding a crate or moving a responsibility between them.
