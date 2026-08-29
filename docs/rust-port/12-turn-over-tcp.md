# 12 · TURN over TCP

**2026-08-29.** Audit finding C1, and the only one of the sixty-five that could not be
fixed inside this repository. This is what was done instead, and why.

## The problem, in one sentence

A player whose network blocks outbound UDP could not reach anybody, because
`webrtc =0.20.3` throws away every relay URL that is not plain UDP before it allocates:

```rust
// webrtc-0.20.3/src/peer_connection/transports/turn_relayer.rs:255-263
if url.is_secure() { warn!("Skipping unsupported secure TURN url {}", url); continue; }
if url.proto.to_string() != "udp" { warn!("Skipping unsupported non-UDP TURN url {}", url); continue; }
```

That is the whole failure. The relay was advertised, the credentials were right, the
server was up — and the client gathered no relay candidate at all. It compounded three
ways: `peers.rs` builds every connection with `with_udp_addrs` and never
`with_tcp_addrs`, so there were no ICE-TCP host candidates either; and until 2026-08-29
no `log` implementation was installed anywhere in the workspace, so both `warn!` calls
went to the no-op logger and the player got no diagnostic.

Schools, corporate guest networks, hotels and some mobile carriers block outbound UDP.
Those are exactly the players a relay is deployed for. The 1.x client uses Chromium,
which allocates over TURN/TCP perfectly well. **The same player is audible on 1.x and
silent on 2.x**, and this project has such users.

## What was measured before forking

Five options. Four were rejected, and one of them was rejected only after it had been
designed in full, which is worth recording.

**1. Wait for upstream.** `0.21.0-beta.2` has the identical skip. There is no issue
tracking it and no branch. Rejected: it makes a shipping regression wait on somebody
else's roadmap.

**2. A local UDP-to-TCP TURN shim.** A small proxy on `127.0.0.1` speaking UDP to the
transport and TCP to the real relay, advertised as `turn:127.0.0.1:PORT`. Designed
completely, then **proved unreachable**: the only path that would reach it is the
relay-only rebuild at `peers.rs:534`, and `reconnect.rs:135` refuses to start that
rebuild when `signals.relay_candidates == Some(0)` — which is precisely what a
UDP-blocked player has. The guard at `ice.rs` had been checked; its sibling in
`reconnect.rs` had not. Recorded because the design survived a full review and died on a
line nobody had followed.

**3. Replace the transport.** `str0m`, or `libwebrtc` through bindings. Both are months
of work and neither is a decision to take for one attribute of one URL.

**4. Ship without it, loudly.** What `ebeca28f` did as far as it goes: install a `log`
bridge so the transport's warning is visible, and make `ice::usable_relay` answer what
the transport can allocate through rather than what the server advertised, so a client
never *believes* in a fallback it cannot reach. Honest, and it fixes nothing for the
player who cannot hear anybody.

**5. Fork the transport.** Chosen.

## Why the fork is small

Because the transport is sans-IO. The relayer owns no sockets: it produces messages
tagged with a `TransportContext`, and the driver decides what to do with each. The
driver already tests TCP first:

```rust
// webrtc-0.20.3/src/peer_connection/driver.rs:851
if msg.transport.transport_protocol == TransportProtocol::TCP {
    self.tcp_transport.write(&msg).await
}
```

So a TURN message can be sent over TCP by setting one field on it, and `poll_write` is
the single choke point every outgoing TURN message passes through. That is the mechanism,
and it is four lines.

The rest is what has to be true around it.

### The conflation that makes it not-quite-four-lines

`rtc-turn`'s `TurnClientConfig::transport_protocol` is used for **two different things**:

- the transport its bytes go out on, and
- the STUN `REQUESTED-TRANSPORT` attribute of the Allocate.

Those are different protocols. RFC 5766 §6.1: `REQUESTED-TRANSPORT` names the protocol of
the **relayed** leg — between the relay and the far peer — and for ordinary media it must
be UDP. Asking a server for a *TCP* allocation is RFC 6062, a different feature, which
coturn refuses by default.

Setting `transport_protocol: TCP` would therefore send over TCP *and* ask for the wrong
kind of allocation. So the client is built with `UDP` and left that way, and the transport
is applied on the way out, in `poll_write`, where only the first meaning applies. The
integration test asserts the server saw `REQUESTED-TRANSPORT = 17` (UDP) on every
Allocate, because getting this wrong is the failure mode that looks like everything else
working.

### The framing

A TURN server on a stream speaks RFC 8656 §3.1: STUN and `ChannelData` messages back to
back, each delimited by its own header, and nothing in front of them. The TCP transport
frames everything with RFC 4571 — a sixteen-bit length prefix — because until now every
stream it carried was ICE-TCP.

A length prefix in front of an Allocate is not a longer Allocate. It is not STUN at all:
the first two bits stop meaning what they meant, and every byte after it is garbage. So
framing became a property of the *stream* rather than of the transport, and a TURN stream
gets its own reader.

That reader is `crates/acl-turn-framing`, a workspace member with **no dependencies at
all**. It is the one place in this tree that parses bytes arriving from a network a
hostile party may sit on, so its trusted computing base is worth counting, and it is
`std` and nothing else. Being a workspace member rather than a module inside the fork is
what keeps it inside every gate this project's CI runs — clippy, the test suite,
`cargo vet` as first-party code.

Losing the boundary on that stream is terminal, and the reader says so. There is no marker
in this framing to resynchronise on — nothing that cannot also occur inside a payload — so
a stream whose boundary is lost is a stream to close, not one to keep reading.

### What else had to be true

- **Gathering has to wait for the connect.** The relayer is synchronous and the driver
  makes the connection, so `gather()` returns before any client exists. Without a
  `pending_connects` count it emits end-of-candidates immediately, and the relay candidate
  arrives after the connection has stopped waiting for it. The count guards both places
  completion is decided.
- **A connect can outlive its configuration.** `update_configuration` tears every client
  down; a connect spawned before it must not become a client of the new one, carrying the
  old server's credentials. A generation counter makes such an arrival stale, and the
  socket is handed back rather than registered.
- **One connection per server, not per interface.** A TCP socket picks its own source
  address, so there is nothing to iterate; asking per interface would hold one allocation
  per interface, and a relay's port range is finite.
- **The peer-address fallback must not cross the two framings.** `find_stream` falls back
  to matching on peer address alone. An ICE-TCP write landing on a TURN stream would send
  a length-prefixed packet to a TURN server, and a TURN write landing on an ICE-TCP stream
  would send an Allocate to a peer. Both are accepted by the socket and understood by
  nobody. The fallback now skips TURN streams.
- **Nagle off.** Every stream this transport opens carries real-time media or the
  signalling for it, in packets far below the MSS. Nagle holds such a write until the
  previous segment is acknowledged — up to a frame of added latency on every packet, and
  jitter of the same size. Two lines, and it helps ICE-TCP as much as it helps TURN.

## What is *not* in it

**`turns:`** — TURN over TLS, on 443, which is what gets through a network that inspects
as well as blocks. The relayer still skips a secure URL, and `ice::transport_can_use`
still answers `false` for one, deliberately: an operator writing `turns:` is saying the
relay traffic must be inside TLS, and connecting to it in plaintext would be worse than
not connecting. It is a separate piece of work — a TLS stream type in the runtime, a
certificate and a listener on the server — and it is a separate decision.

**RFC 6062** — TCP allocations, where the *relayed* leg is TCP. Not wanted: media is UDP.

## One limitation, stated rather than hidden

`RTCTcpTransport::write` is awaited inside the driver's select loop. That is upstream's
design and it was already true of ICE-TCP; what is new is that it is now on the media path.
If a relay stops draining its socket, the TCP send buffer fills and that peer connection
makes no progress at all until it drains — no reads, no timers, so ICE consent freshness
fails and the connection dies.

Measured rather than feared: at Opus rates the relay has to read nothing for roughly six
seconds to fill a default send buffer, by which point the connection is failing for its own
reasons. Each peer has its own driver and its own connection to the relay, so one stuck
relay stalls one peer and not the lobby.

The fix is a per-stream writer task with a bounded queue and drop-on-full, because queueing
real-time audio is the wrong answer anyway. It is about eighty lines, it changes ICE-TCP's
behaviour as well as TURN's, and it would roughly double a patch whose whole value is that
a reviewer can read it. It is not done. If a relay is ever observed stalling a connection
this way, that is what to write.

## Where it lives

- `vendor/webrtc/` — the fork. Six files differ from upstream.
- `patches/webrtc-0.20.3-turn-tcp.patch` — the diff, 762 lines.
- `crates/acl-turn-framing/` — the stream splitter, no dependencies, nine tests.
- `crates/acl-net/tests/turn_tcp.rs` — the proof.
- `Cargo.toml` — `[patch.crates-io] webrtc = { path = "vendor/webrtc", version = "=0.20.3" }`.

The `version` is not decoration: `deny.toml` keeps `allow-wildcard-paths = false`, and an
unversioned path entry is exactly the wildcard that setting exists to catch.

## How it is kept honest

A vendored dependency is a place where an unreviewed change can be made to somebody else's
code and never noticed again, because nothing rebuilds it from a known source. The `fork`
job in `.github/workflows/rust.yml` therefore re-derives the entire directory on every run:
it downloads `webrtc-0.20.3.crate` from crates.io, checks it against the sha256 that
`Cargo.lock` recorded **before** the patch entry replaced the registry source, applies the
patch, and diffs the result against `vendor/webrtc`. Any difference fails. What is in the
tree is then provably upstream plus exactly that diff.

`Cargo.lock` and `target/` are excluded from the comparison, because the fork is its own
workspace root — so that it can be built and tested on its own — and both are outputs
rather than sources. Everything else, `.vscode/` included, is compared; that directory is
force-added past this repository's own `.gitignore` so the tree really does match the
tarball.

The job also builds the fork under all three runtime features separately. `--all-features`
is not a valid combination upstream: the runtime features are mutually exclusive and
`default_runtime` has no branch for two at once.

## What was verified

- **The fork's own test suite, both trees.** 123 passed and 2 failed on the fork; 123
  passed and 2 failed on the unmodified copy, the same two — they need a routable LAN
  address this machine does not have. Identical results, so the patch changes no upstream
  behaviour. CI does not re-run this, because gating on those two would gate on the
  runner's networking.
- **`crates/acl-net/tests/turn_tcp.rs`.** Two peer connections, a real TURN server on
  loopback that speaks only TCP — long-term credentials, a UDP socket per allocation,
  permissions, channel bindings, Send indications and `ChannelData` both ways — and
  `IceTransportPolicy::Relay` with one `turn:…?transport=tcp` URL and nothing else. Every
  byte between the two peers goes out over a TCP socket, through the server, and back.
  It asserts a relay candidate is gathered on both sides, that every candidate is
  `typ relay`, that every Allocate asked for a UDP relayed leg, that the connection
  reaches `Connected`, and that a payload arrives.
- **Both halves of the patch are load-bearing, proved by breaking them.** Restoring the
  non-UDP skip: `both sides must gather a relay candidate over TCP; offerer got 0 and
  answerer got 0` — the exact upstream symptom. Restoring the RFC 4571 prefix on TURN
  writes: `the client wrote something that is not TURN framing: a STUN length of 3 is not
  a multiple of four`, from the splitter, at the server.
- The whole workspace suite, `cargo fmt --check`, `cargo clippy --workspace --all-targets
  --features acl-updater/ceremony -- -D warnings`, `cargo deny check bans licenses`, and
  `cargo vet`.

## If upstream ever fixes this

Delete `vendor/webrtc`, the `[patch.crates-io]` entry, the patch file and the `fork` CI
job, and raise the version. Keep `crates/acl-turn-framing` and `crates/acl-net/tests/turn_tcp.rs`:
the first is a dependency-free parser worth having either way, and the second is the test
that would catch a regression in whatever replaces this.
