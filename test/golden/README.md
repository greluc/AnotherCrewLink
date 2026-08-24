# Golden vectors

Chromium's own Web Audio output, rendered by `npm run golden` inside Electron, for the
Rust DSP graph to be measured against. Gate G2's first criterion is that every node
matches its vector to within −80 dBFS RMS error.

They are generated rather than hand-written, and generated **in the shipping runtime**
rather than from a reading of the specification. That is the whole point: parity then
stops being a matter of opinion, and a disagreement is a bug in the port rather than two
defensible readings of a paragraph.

## What is here

29 vectors, one per node, configuration and input, plus `manifest.json` with a SHA-256 of
each. The naming is `node__input__config.wav`.

Every input is deterministic — the noise carries its own xorshift rather than calling
`Math.random` — so re-running the generator produces byte-identical files. If it does
not, something is wrong with the generator, not with the run.

They are 32-bit float WAV. 16-bit would quantise the reference to about −96 dBFS, close
enough to the gate's −80 dBFS tolerance that the container would be part of the
measurement. Raw rather than compressed, because a golden vector you can open in an audio
editor when a test fails is worth the megabytes.

## Regenerating

```
npm run golden
```

The output directory is cleared first: a vector that stops being generated must stop being
committed, or the next person measures against something nothing produces.
