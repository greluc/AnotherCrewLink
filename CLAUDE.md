# CLAUDE.md — AnotherCrewLink

Free, open proximity voice chat for Among Us. An Electron app that reads the
game's state out of process memory and mixes every other player's voice by how
far away they are, whether a wall is between you, whether you are dead, and
whether the lights are out.

A fork of BetterCrewLink, which forked CrewLink. GPL-3.0-or-later.

## Commands

```bash
npm ci                # install; compiles three native modules
npm run dev           # electron-vite dev server
npm run compile       # build main + preload + renderer
npm test              # vitest, node environment, src/**/*.test.ts
npm run typecheck     # tsc --noEmit
npm run lint          # biome check (lint + format)
npm run lint:fix      # biome check --write
npm run dist:64       # Windows x64 installer
npm run dist:linux    # Linux AppImage
npm run audit         # npm audit --audit-level=high
```

Building needs Node 22+ and a C++ toolchain. On Windows: Visual Studio with the
"Desktop development with C++" workload and Python 3. On Linux:
`build-essential`, `libxcb1-dev`, `libx11-dev`, `libasound2-dev`, `libxtst-dev`,
`libxrandr-dev`, `libxt-dev`.

## Layout

| Path | What lives there |
| --- | --- |
| `src/main` | Electron main: windows, memory reading, key hooks, IPC, auto-update |
| `src/renderer` | The UI, the voice pipeline, the in-game overlay |
| `src/common` | Types and constants shared by both processes |
| `native/` | Vendored native modules, built from C/C++ at install time |
| `vendor/structron` | Vendored binary struct parser |
| `static/locales` | 37 translations |
| `docs/rust-port/` | Assessment and plan for a possible Rust rewrite |

The server is a separate repository: `greluc/AnotherCrewLink-server`.

## The files that matter

- **`src/renderer/Voice.tsx`** (1,733 lines) — socket signalling, the peer mesh,
  the whole Web Audio graph, and `calculateVoiceAudio()`, which decides gain and
  pan per peer on every game frame. Almost every voice bug is in here.
- **`src/main/GameReader.ts`** (1,223 lines) — pattern-scans `GameAssembly.dll`,
  walks pointer chains, and on 32-bit Windows injects two hand-assembled x86
  shellcode stubs. The most fragile file in the project: it depends on the game's
  build.
- **`src/renderer/peer.ts`** — a minimal `RTCPeerConnection` wrapper that
  replaced simple-peer. Small, and four separate audio bugs lived in it.
- **`src/main/offsetStore.ts`** — fetches memory offsets over HTTP, validates
  them on every load, and falls back to `embeddedOffsets.ts` when the mirror is
  unreachable. Regenerate the embedded floor with `node scripts/embed-offsets.mjs`.

## Conventions

- **Biome**, not ESLint or Prettier. Tabs, 120 columns, single quotes,
  semicolons, ES5 trailing commas. `npm run lint` is the authority.
- `noExplicitAny` is an error. `useHookAtTopLevel` is an error — conditional
  hooks were a real bug here.
- British spelling in prose (README, CHANGELOG, comments).
- Comments explain **why**, not what. The codebase's convention is to record the
  reason a line exists when it is not obvious — a game-build quirk, an ordering
  that matters, a specification constant. Match that density; do not narrate.
- Commit subjects are written as a plain statement of the effect, not as a
  conventional-commits prefix. See `git log`.

## Things that will bite you

- **The renderer runs with `nodeIntegration: true` and
  `contextIsolation: false`.** `hardenWindow()` in `src/main/index.ts` blocks
  navigation and window opening because of it. Do not weaken those guards, and do
  not load remote content into a renderer.
- **Vite builds the renderer as a browser target**, which stubs out Node
  built-ins. `electron.vite.config.ts` has a plugin that resolves them to a
  runtime `require` instead. If `path.join` is suddenly undefined, that is why.
- **`AmongUsState.map` must never be undefined.** A collider lookup for an
  undefined map silently reports that no wall is ever in the way, which disabled
  walls-block-audio for everyone without an error. `GameReader.lastState` is
  seeded with `MapType.UNKNOWN` for this reason, and the map falls back to
  ShipStatus when the game options pointer resolves to zero (it does, on Among Us
  17.4.0 x86).
- **A `ConvolverNode` with a null buffer outputs silence**, it does not pass
  audio through. The reverb impulse response is decoded asynchronously; the
  effect is skipped until it lands.
- **Connect an effect before disconnecting the direct path.** The other order
  leaves the player with no output at all if the second step throws.
- **Tests are node-environment only.** Anything touching Electron or the DOM is
  not unit-tested; it is covered by running the app. Six files have tests:
  `ColliderMap`, `reconnectPolicy`, `validateClientPeerConfig`, `offsetStore`,
  `offsetsValidator`, `vdf`. The last two read `test/fixtures/offsets`, a vendored
  copy of the real offsets tree — gate G0 requires the validator to accept every
  real file unchanged, so that half is tested against the whole corpus rather than
  a sample.
- **Native modules are vendored on purpose.** They used to be installed from
  unpinned branch HEADs. Do not replace a `file:native/...` dependency with a
  git or registry one.

## Wire protocol

Socket.IO 4 (Engine.IO v4). **This client cannot talk to the official
BetterCrewLink server**, which runs Socket.IO 2. Eleven events, one namespace:
`join`, `leave`, `id`, `setHost`, `signal`, `VAD`, `lobby`, `remove_lobby`,
`join_lobby`, `lobbybrowser`, `disconnect`.

Changing an event name or payload shape breaks every player who has not updated.
Add alongside; do not repurpose.

## Releases

`CHANGELOG.md` is written for players, not for developers: each entry says what
was wrong, what the user saw, and what changed. Read the 1.0.0–1.0.2 entries
before writing a new one — the register is deliberate.

CI is four GitHub Actions workflows with every action pinned to a commit SHA.
The Windows and Linux legs use `fail-fast: false` so one broken platform does not
hide the other.
