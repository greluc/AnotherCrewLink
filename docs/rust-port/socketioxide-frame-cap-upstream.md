# The WebSocket payload cap upstream: already reported, already fixed, not merged

This file used to be a drafted issue. It is not one any more, because searching
before filing found the work already done. Kept as the record of what was checked,
so nobody repeats the search.

**Checked 2026-08-24.** `socketioxide` 0.18.6 and `engineioxide` 0.17.6 are the newest
published versions — no pre-release carries a fix. The unreleased `main` branch does not
either: `crates/engineioxide/src/transport/ws.rs:120` still builds a
`WebSocketConfig::default()` and the file never mentions `max_payload`.

## The open pull request

[Totodore/socketioxide#762](https://github.com/Totodore/socketioxide/pull/762) —
*fix(engineioxide): enforce max_payload on the websocket transport*, opened 2026-07-17
by `schulzfel`, +117/-2, CI green.

It is the same diagnosis and the same three lines this project arrived at
independently, plus two integration tests. Its author says they carry it downstream as
a vendored patch for the same reason we would: to enable the WebSocket transport under
a payload budget.

**It is blocked on review, not on doubt.** The maintainer requested changes on
2026-07-18 and has not disputed the bug. Two things were asked for:

1. Separate options for frame size and message size, rather than reusing `max_payload`
   for both — the two do not have the same implications.
2. Propagation to the socketioxide-level builder, alongside `ws_read_buffer_size`.

Nothing has moved since 2026-07-18.

## What this project should do about it

Not file an issue. #762 covers it, and a duplicate costs the maintainer time without
adding information — our repro would only restate what the PR's own tests already
demonstrate.

The two open options are worth a decision rather than a drift:

- **Wait.** The risk stays as recorded in `04-implementation-plan.md` §4.2 and the
  server's `deploy/README.md` §5, bounded by the per-event size check, the bounded
  per-member channel, and `MemoryMax=512M` in the systemd unit.
- **Finish it.** The requested changes are small and well specified: two builder
  methods on `EngineIoConfig` (`ws_max_frame_size`, `ws_max_message_size`), each
  defaulting to `max_payload`, and the matching pair on `SocketIoBuilder`. Offering
  that to #762's author, or opening a follow-up that credits it, is the shortest path
  from "accepted risk" to "fixed upstream" — and it is the only one that also removes
  the risk for everyone else using this crate.

Either way, when the fix lands, `server-rs` sets the new option and the risk section in
both documents comes out rather than being softened.
