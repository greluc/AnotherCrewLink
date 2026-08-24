# Recordings for gate G1

Empty on purpose. These files come from real play, and nothing else can produce them.

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
