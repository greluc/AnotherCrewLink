# Voice decision tuples

Gate G2's second criterion: the Rust `voice_params` must match the Electron client on
every recorded tuple.

`src/renderer/voiceRecorder.ts` writes them while the client runs with `ACL_RECORD` set —
the same variable that records the memory frames gate G1 needs, so one session produces
both and the two cannot be captured out of step.

Each line is one distinct `(inputs, outputs)` pair. Distinct is the point: a lobby of ten
standing still produces the same tuple ninety times a second, and a corpus of those
measures one case very thoroughly. What is wanted is every case the session reached.

The outputs are read back off the Web Audio nodes rather than captured inside the
decision. `calculateVoiceAudio` returns a gain and writes everything else onto live nodes,
so the answer is spread across a graph; reading it back records what actually arrived
there rather than what the code meant to put there.

## Adding some

```
set ACL_RECORD=meeting-and-vents
```

Then copy `userData/recordings/*.voice.ndjson` here. Gzip them if they are large; the test
reads either.

Without any, the test skips loudly. A gate that quietly passes having compared nothing is
worse than one that fails.
