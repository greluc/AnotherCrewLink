<p align="center">
  <img src="static/images/logos/sizes/256-BCL-Logo-shadow.png" alt="AnotherCrewLink" width="128">
</p>

<h1 align="center">AnotherCrewLink</h1>

<p align="center">Free, open proximity voice chat for Among Us.</p>

---

AnotherCrewLink reads the game's state out of memory and mixes every other player's
voice by how far away they are, whether a wall is between you, whether you are dead,
and whether the lights are out. It runs alongside the game; nothing is injected into
your account and no game files are modified.

It is a fork of [BetterCrewLink](https://github.com/OhMyGuus/BetterCrewLink) by
OhMyGuus, which in turn forked [CrewLink](https://github.com/ottomated/CrewLink) by
ottomated. See [CREDITS.md](CREDITS.md).

## Windows says the app is not safe

It is not signed, so Windows has nothing to check it against and says so. Two different
mechanisms do this and only one of them can be waved past.

**SmartScreen** shows "Windows protected your PC". Click **More info**, then **Run
anyway** — the link is deliberately unobtrusive. If the dialog reappears, right-click the
installer, choose **Properties**, tick **Unblock** at the bottom and apply; that removes
the mark-of-the-web the browser attached. Edge and Chrome may also refuse the download
itself before any of this, which needs **Keep** in the downloads list.

**Smart App Control** is a different feature, on by default only on Windows 11 machines
that were installed clean. It blocks unsigned programs outright and offers no way past for
a single app. The only options are to turn it off — under **Windows Security → App &
browser control → Smart App Control** — or to wait for a signed build. Understand what that
costs before doing it: **Smart App Control cannot be switched back on afterwards without
reinstalling Windows.** That is how Microsoft designed it, and it applies to the whole
machine, not to this app.

If there is no "Run anyway" and no way to allow the app, it is Smart App Control rather
than SmartScreen.

### Checking what you downloaded

A signature would prove where the installer came from. Until there is one, the checksum at
least proves you have the same bytes the build produced. Compare it with the value on the
[release page](https://github.com/greluc/AnotherCrewLink/releases):

```powershell
Get-FileHash -Algorithm SHA256 .\AnotherCrewLink-Setup-1.0.4.exe
```

The value to compare it against is on the release itself. It is not repeated here: a
checksum written in two places is a checksum that will disagree with itself one release
from now, and the wrong half is the one people would trust.

## Server

**AnotherCrewLink does not work with the official BetterCrewLink server.** This
client speaks socket.io 4 (Engine.IO v4); `bettercrewl.ink` runs socket.io 2, and the
two protocols are not interoperable in either direction.

The default server is `https://aucl.greluc.me`. You can point the client at any
instance of [the server](https://github.com/greluc/AnotherCrewLink-server) under
Settings → Server.

## Requirements

- Windows 10 or later, or Linux
- Among Us
- A microphone

Reading another process's memory needs the app to run at the same privilege level as
the game. If the game runs as administrator, so must AnotherCrewLink.

## Building from source

Building needs Node.js 22 or later and a C++ toolchain, because three native modules
are compiled from the sources vendored under `native/`:

- **Windows**: Visual Studio with the "Desktop development with C++" workload, and Python 3
- **Linux**: `build-essential`, `libxcb1-dev`, `libx11-dev`

```bash
npm ci
npm run dev
```

To produce installers:

```bash
npm run dist:64      # Windows x64
npm run dist:linux   # Linux AppImage
```

## Project layout

| Path | What lives there |
| --- | --- |
| `src/main` | Electron main process: window handling, game memory reading, IPC |
| `src/renderer` | The UI, the voice pipeline and the in-game overlay |
| `src/common` | Types and constants shared by both processes |
| `native/` | Vendored native modules (memory reading, keyboard hook, overlay window) |
| `vendor/` | Vendored JavaScript dependencies |
| `static/locales` | Translations |
| `docs/rust-port` | Assessment and plan for a possible rewrite in Rust |

The build is [electron-vite](https://electron-vite.org/). `npm run lint` runs
[Biome](https://biomejs.dev/) for both linting and formatting, `npm run typecheck`
runs TypeScript without emitting, and `npm test` runs the Vitest suite.

## A possible Rust rewrite

[docs/rust-port](docs/rust-port/) assesses whether the client, the server and the
native parts could be rewritten in Rust with a native GUI. Short answer: yes,
with no hard blockers, but the real-time voice stack that Chromium currently
supplies for free is the part that decides it — so the plan builds and measures
that first, before anything else.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Translations live in `static/locales`; adding
a language means adding a folder there and registering it in
`src/renderer/language/languages.ts`.

## Licence

GPL-3.0-or-later, inherited from CrewLink. See [LICENSE](LICENSE).
