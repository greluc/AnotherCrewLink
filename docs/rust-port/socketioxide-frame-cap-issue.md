# Upstream issue draft: `max_payload` is not applied to the WebSocket transport

Not filed. Filing it needs a GitHub account that is not the agent's, so this is the
report ready to paste. Target repository: `Totodore/socketioxide`. Everything below was
checked against the vendored sources of the exact versions this server builds against,
not from memory.

**Versions checked:** `socketioxide` 0.18.6, `engineioxide` 0.17.6,
`engineioxide-core` 0.2.2, `tungstenite` 0.30.0.

---

## Title

`max_payload` is silently ignored on the WebSocket transport

## Body

`SocketIoBuilder::max_payload` reads as a limit on payload size, and on the polling
transport it is one. On the WebSocket transport it is never consulted, so a server that
offers only WebSocket — the recommended configuration, and what both of our shipping
clients ask for — advertises a limit in the handshake and then enforces nothing
resembling it.

Where the value is used, in `engineioxide` 0.17.6:

- `src/transport/polling.rs:118,133` — outbound payload encoding.
- `src/transport/polling.rs:182` — inbound body decoding. This is the enforcement.
- `src/transport/mod.rs:21` — copied into the handshake OPEN packet the client is sent.

`src/transport/ws.rs` never mentions it. The only limit configured on that path is at
`src/transport/ws.rs:120`:

```rust
let ws_config = WebSocketConfig::default().read_buffer_size(engine.config.ws_read_buffer_size);
```

`WebSocketConfig::default()` (tungstenite 0.30.0, `src/protocol/mod.rs:95-105`) leaves
`max_message_size: Some(64 << 20)` and `max_frame_size: Some(16 << 20)` in place. So an
application that sets `max_payload(64 * 1024)` gets 64 KiB enforced on a transport it may
not even offer, and 64 MiB on the one it does — a factor of a thousand between the
configured value and the effective one, in the direction that matters.

### Why this is worth a fix rather than a documentation note

The gap cannot be closed by the application or by its operator:

- An application-level check on the decoded event runs after tungstenite has already
  allocated and assembled the message, so it rejects the payload without preventing the
  allocation it was meant to prevent.
- It cannot be pushed to a reverse proxy. `client_max_body_size` and its equivalents stop
  applying at the Upgrade, and neither nginx nor Caddy has a directive that bounds a
  frame after the connection is upgraded — both relay frames unexamined.

That leaves a process memory limit as the only real backstop, which turns a hostile
message into a restart rather than into a refusal.

### Suggested fix

Apply the configured value at `ws.rs:120`, which is a two-line change:

```rust
let ws_config = WebSocketConfig::default()
    .read_buffer_size(engine.config.ws_read_buffer_size)
    .max_message_size(Some(engine.config.max_payload as usize))
    .max_frame_size(Some(engine.config.max_payload as usize));
```

This makes `max_payload` mean the same thing on both transports and makes the handshake
advertisement true. It is a behaviour change for any application that set a small
`max_payload` while relying on large WebSocket messages getting through, so it may belong
behind a separate builder method — `ws_max_message_size`, defaulting to `max_payload` —
if you would rather not change existing behaviour in a patch release.

Happy to open the PR if you would like it in either shape.

---

## What this project does in the meantime

Recorded in `04-implementation-plan.md` §4.2 as an accepted risk, and in the server's
`deploy/README.md` §5 for operators. Three things bound it: the per-event size check in
`src/socket.rs`, the bounded per-member channel, and `MemoryMax=512M` in the systemd
unit, which is what turns the worst case into a restart instead of a host running out of
memory.
