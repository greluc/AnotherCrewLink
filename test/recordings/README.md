# Recordings for gate G1

These files come from real play, and nothing else can produce them.

## What is here

| File | Covers |
| --- | --- |
| `skeld-menu-x86.ndjson.gz` | The menu, and no game at all: the reader must report nothing rather than guess |
| `skeld-freeplay-x64.ndjson.gz` | Freeplay on 64-bit: movement, tasks, vents |
| `skeld-lobby-x86-nine-players.ndjson.gz` | A real online lobby of nine on 32-bit: the player list, hats, colours, host |

## What is still missing

No recording covers a live match. Every frame is a menu, freeplay or a lobby, so the two
game states a real round produces -- `TASKS` and `DISCUSSION` -- are absent, and with them
`comsSabotaged`, `closedDoors`, a `currentCamera` that ever changes, and `lightRadius`
under a lights sabotage.

Freeplay cannot fill that gap -- sabotage and cameras are not reachable there -- so it
needs somebody to record an online round. That is the whole of issue #10.

**Do not guess at this list.** It was written by hand once and was wrong in both
directions: it said `isDead` had never been compared, when freeplay produces it true in
4126 player-frames, and it did not notice that `currentCamera` is the constant `7` in
every frame of every recording -- compared, and never once exercised. Run the measurement
instead:

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

Play, close the app, and copy `userData/recordings/polus-tasks.ndjson` here. Then:

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
