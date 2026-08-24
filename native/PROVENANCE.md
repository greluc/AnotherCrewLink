# Where the vendored components came from, and under what terms

These modules are copied into the tree rather than installed. They used to be pulled from
unpinned branch HEADs, which meant an install could produce a different binary from one
day to the next; vendoring them fixes that. It also makes their licensing this project's
problem to state, which is what this file is for.

| Component | Upstream | Licence | Licence text present |
| --- | --- | --- | --- |
| `native/electron-overlay-window` | [SnosMe/electron-overlay-window](https://github.com/SnosMe/electron-overlay-window) | MIT | Yes, `LICENSE` |
| `native/memoryjs` | [Rob--/memoryjs](https://github.com/Rob--/memoryjs) | MIT | Yes, `LICENSE.md` |
| `native/uiohook-napi` | [SnosMe/uiohook-napi](https://github.com/SnosMe/uiohook-napi) 1.5.5 | MIT wrapper around [libuiohook](https://github.com/kwhat/libuiohook) under LGPL-3.0-or-later | Yes for the wrapper, `LICENSE`; libuiohook carries its notice in each source file |
| `vendor/structron` | [LordVonAdel/structron](https://github.com/LordVonAdel/structron) | ISC, in `package.json` only | No, and upstream has none either |

LGPL-3.0-or-later combines with this project's GPL-3.0-or-later. Both halves are OSI
approved, which the free code-signing programmes for open source require of every
component.

## The prebuilt binaries were removed

`uiohook-napi` ships seven compiled `.node` files for platforms it supports. They are not
vendored: `node-gyp-build` prefers a prebuild over compiling, so keeping them would mean
shipping a binary nobody here built, from a tarball, inside an installer we are asking
people to trust. Without them it compiles `libuiohook` from the C sources in this tree,
and `electron-builder install-app-deps` then rebuilds it against Electron's ABI.

If an `npm ci` starts failing on a machine without a C++ toolchain, this is why. The
toolchain requirement is in `CLAUDE.md`, and it already applied to the other three.

## The local patch

`src/lib/addon.c` drops mouse motion, drags and the wheel on the hook thread, before
anything is allocated or handed to JavaScript. `WH_MOUSE_LL` reports every movement, and a
probe against a real session measured about 126 of them a second arriving in the main
process -- the same process that reads the game's memory -- for events this client never
reads. It binds keys and the two extra mouse buttons.

A consequence worth having on purpose: the process never receives a cursor position at
all, which is a better thing to be able to say about something holding a global input
hook.

## What was measured, and how

Against a running session, with the game in the foreground:

| Check | Result |
| --- | --- |
| Module loads under Electron, hook starts and stops | yes |
| Keycodes match `src/main/keyBindings.ts` | `V`=47, `CtrlRight`=3613, `AltRight`=3640 |
| A synthetic `F13` arrives | yes, keycode 91 |
| Real keystrokes arrive | yes, the player's own movement keys |
| Extra mouse buttons arrive | yes, `mousedown` and `mouseup` for 4 and 5 |
| Mouse motion arrives | no, 0 events where an unpatched build reported 1011 in 8 seconds |

The mouse buttons were driven with `mouse_event` XBUTTON1 and XBUTTON2, which Among Us
binds to nothing. The commit that made this change says they were unverified; they were
verified immediately afterwards and this table is the record.

**Re-apply the patch when upgrading.** It is fifteen lines in `dispatch_proc`, marked `LOCAL
PATCH`. If mouse motion starts arriving again, that is what was lost. Note that a rebuild
does not always recompile: `node-gyp-build` will reuse `build/Release/*.node` if it is
there, so `rm -rf native/uiohook-napi/build` first and verify by counting `mousemove`
events, not by reading the source.

## What was here before

`node-keyboard-watcher` polled `GetAsyncKeyState` for the shortcut keys, and was removed
in favour of `uiohook-napi` because it carried no licence at all -- no field in its
`package.json`, no licence file, and none upstream, where the last commit is from July
2023. Without a grant the default is that no permission is given, which is an odd thing
for a GPL-3.0-or-later project to redistribute.

`src/main/keyBindings.ts` is what the swap cost: the shortcuts players have saved are
names, not codes, so nobody's settings needed migrating, but the two libraries number keys
differently and the extra mouse buttons moved to a different event. That file is the only
place that knows.

## structron

`structron` declares ISC in its `package.json` and nowhere else, upstream included. The
declaration in the manifest is how an npm package states its terms, so this is a real
grant rather than an absence, but there is no licence text to reproduce. Nothing is
invented here to fill the gap: a licence file naming a copyright holder and year that
nobody has stated would be worse than the honest absence.
