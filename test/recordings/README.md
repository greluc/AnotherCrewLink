# Recordings for gate G1

These files come from real play, and nothing else can produce them.

## What is here

| File | Covers |
| --- | --- |
| `skeld-menu-x86.ndjson.gz` | The menu, and no game at all: the reader must report nothing rather than guess |
| `skeld-freeplay-x64.ndjson.gz` | Freeplay on 64-bit: movement, tasks, vents |
| `skeld-lobby-x86-nine-players.ndjson.gz` | A real online lobby of nine on 32-bit: the player list, hats, colours, host |
| `maps-and-lobbies-x86.ndjson.gz` | 32-bit: freeplay with vents, an impostor and a death; then online lobbies cycling all five maps and three player limits |

## What is still missing

No recording covers a live match. Every frame is a menu, freeplay or a lobby, so the two
game states a real round produces -- `TASKS` and `DISCUSSION` -- are absent, and with them
`comsSabotaged`, `closedDoors` and a `currentCamera` that ever changes.

**Freeplay cannot fill that gap, and the reason is stronger than "sabotage is awkward
there".** All three of those fields are read inside `if (state === GameState.TASKS)` in
`GameReader.ts`, and freeplay never leaves the InnerNet client's joined state, so the
reader does not read them at all. A 2324-frame freeplay session is uniformly `LOBBY`.
Sabotaging comms in freeplay changes nothing in a recording, because nothing looks.

It needs an online round of four. That is the whole of issue #10.

### What freeplay and a lobby *can* reach, measured on 2026-08-26

Worth writing down, because two of these were only found by trying:

- **32-bit in a game.** The pointer width changes the player-array, door-list and
  dictionary strides. Until this recording the only in-game session was 64-bit.
- **Every map, from an online lobby's settings.** Not from freeplay and not from the
  menu: the reader takes the map from the game options, and freeplay does not write its
  map there. A whole freeplay session on Polus arrives labelled `THE_SKELD`, which is not
  a reader bug -- the same field really does say Skeld -- but it does mean the only way to
  move `map` is to host a lobby and change the setting.
- **`maxPlayers`, likewise.** Same object, same route. It cannot be changed inside a
  lobby; three lobbies started and abandoned at 15, 10 and 8 is what produced three
  values.

## Do not guess at this list

It was written by hand once and was wrong in both directions: it said `isDead` had never
been compared, when freeplay produces it true in 4126 player-frames, and it did not notice
that `currentCamera` is the constant `7` in every frame of every recording -- compared, and
never once exercised. Run the measurement instead:

```bash
cargo test -p acl-game --test corpus_coverage -- --nocapture
```

It prints what the corpus reaches and what it does not, before a session so you know what
to go after, and after one so you know whether you got it. A parity run cannot tell you:
a branch neither reader reaches compares equal on both sides.

## Making one

Set `ACL_RECORD` to a session name before starting the Electron client:

```bash
set ACL_RECORD=polus-tasks && npm run dev
```

Play, **close the app rather than killing it** -- the recording is flushed on `will-quit`
and a killed process leaves the tail of it in a buffer -- then copy
`userData/recordings/polus-tasks.ndjson` here, gzip it, and:

```bash
cargo test -p acl-game --test parity
```

## What to cover

The plan asks for one session per map — Skeld, Mira, Polus, Airship, Fungle — and each
should cover lobby, tasks, meeting, vents, cameras, sabotage and deaths.

That list is not thoroughness for its own sake. Each item is a branch of the reader that
is only reachable in that situation: a recording where nobody dies never exercises the
dead flag, one that never enters a vent never exercises `inVent`, and a parity run over
frames that only show a lobby proves the reader agrees about a lobby.

## What the gate compares

Every field of `AmongUsState`, exactly. The one allowance is float positions, within
1e-6, and that is for JSON's decimal round trip rather than for the reader.

Without recordings the test **skips loudly** rather than passing. A green run that
compared nothing would report that the gate is met, which is the worst outcome available.

**It earns its keep.** Adding `maps-and-lobbies-x86` took the corpus from 4653 frames to
12574 and broke the gate on 23 of them, in four places the Rust reader had got wrong and
nothing else had noticed: the menu hold that keeps reporting a menu until the game rebuilds
its player table, the 9999 sentinel for a player the reader could not make sense of, a
player dropped for having a null object pointer, and two fields that are `undefined` in the
Electron reader and cannot be a `u32` or a `bool` here.
