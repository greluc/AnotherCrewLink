# Where the vendored components came from, and under what terms

These modules are copied into the tree rather than installed. They used to be pulled from
unpinned branch HEADs, which meant an install could produce a different binary from one
day to the next; vendoring them fixes that. It also makes their licensing this project's
problem to state, which is what this file is for.

| Component | Upstream | Declared licence | Licence text present |
| --- | --- | --- | --- |
| `native/electron-overlay-window` | [SnosMe/electron-overlay-window](https://github.com/SnosMe/electron-overlay-window) | MIT | Yes, `LICENSE` |
| `native/memoryjs` | [Rob--/memoryjs](https://github.com/Rob--/memoryjs) | MIT | Yes, `LICENSE.md` |
| `native/node-keyboard-watcher` | [OhMyGuus/node-keyboard-watcher](https://github.com/OhMyGuus/node-keyboard-watcher) | **None** | No |
| `vendor/structron` | [LordVonAdel/structron](https://github.com/LordVonAdel/structron) | ISC, in `package.json` only | No, and upstream has none either |

## The one that is a problem

`node-keyboard-watcher` carries no licence at all: no `license` field in its
`package.json`, no licence file, and none upstream either, where the last commit is from
July 2023. Without a grant the default is that no permission is given, which sits badly in
a GPL-3.0-or-later project that redistributes it, and it disqualifies the project from the
free code-signing programmes for open source, which require every component to be open
source.

It is 313 lines: a polled `GetAsyncKeyState` loop on Windows, the same loop over
`XQueryKeymap` on X11, and a keycode table between them. `src/main/hook.ts` is its only
caller, for push-to-talk and push-to-mute.

Three ways out, none of them written yet:

1. **Ask upstream for a licence.** One line in a `package.json` settles it. The author
   wrote BetterCrewLink, which this is a fork of, so the request is not a cold one --
   but the repository has not moved in two years.
2. **Swap in a licensed module.** `uiohook-napi` and `node-global-key-listener` are both
   MIT. Push-to-talk is the feature players notice most when it misbehaves, so this is a
   change that has to be tested by playing rather than by a test suite.
3. **Reimplement it.** Small enough to be realistic, and it must be written from the
   documented Win32 and X11 APIs rather than from the unlicensed source, or the problem
   comes along with the copy.

## structron

`structron` declares ISC in its `package.json` and nowhere else, upstream included. The
declaration in the manifest is how an npm package states its terms, so this is a real
grant rather than an absence, but there is no licence text to reproduce. Nothing is
invented here to fill the gap: a licence file naming a copyright holder and year that
nobody has stated would be worse than the honest absence.
