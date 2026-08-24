# The embedded floor

Generated, not hand-maintained. Regenerate with:

```bash
node scripts/embed-offsets-rs.mjs
```

These three files are compiled into the binary and are what the reader falls back to when
the mirror cannot be reached — DNS failure, a 404, an empty cache. The alternative is a
client that will not start because a repository is down, which is a worse failure than a
client running slightly old offsets and saying so.

They carry the current game build only. A player on an older Among Us with an unreachable
mirror gets an honest error rather than offsets for a different game.
