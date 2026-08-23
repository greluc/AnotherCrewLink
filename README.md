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

The build is [electron-vite](https://electron-vite.org/). `npm run lint` runs ESLint
with Prettier; `npx tsc --noEmit` typechecks without emitting.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Translations live in `static/locales`; adding
a language means adding a folder there and registering it in
`src/renderer/language/languages.ts`.

## Licence

GPL-3.0-or-later, inherited from CrewLink. See [LICENSE](LICENSE).
