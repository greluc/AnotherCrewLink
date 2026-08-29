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
- **Nagle off.** Every stream this transport opens or accepts carries real-time media or
  the signalling for it, in packets far below the MSS. Nagle holds such a write until the
  previous segment is acknowledged — up to a frame of added latency on every packet, and
  jitter of the same size. Both the connect and the accept path, because an accepted stream
  is the passive half of ICE-TCP and carries exactly the same traffic; setting it on only
  one would have added the latency in one direction.
- **The four-tuple carries no protocol.** `FourTuple` is an address pair, and that is what
  the relayer keys its clients by, what a read off a stream is tagged with, and what a write
  failure names. A TCP connection whose ephemeral source port happens to equal a bound UDP
  socket's port therefore produces *the same key* as the UDP client on that socket — the two
  become indistinguishable, and the TCP allocation is the one that loses. UDP and TCP
  ephemeral ports come from independent namespaces over the same range, so this is roughly
  a one-in-sixteen-thousand event per connection, and it is silent: the guard that catches
  it returned `Ok(())`. It is caught explicitly now, the socket is given back, and the
  connection is made again — a new draw of the source port, up to four times. On the
  machine this feature exists for, UDP is blocked and that allocation is the only way
  through, so losing it one time in sixteen thousand is not a rounding error.
- **One connection per relay, however often it is advertised.** The UDP path is
  deduplicated by the client map; a TCP client does not exist yet at the point the decision
  is made, so the gather pass keeps its own record. Two entries naming the same relay used
  to mean two allocations, and a relay's port range is finite.
- **The family the machine actually has.** The TCP branch used to take the head of the
  resolved list. A relay with both an A and a AAAA record would then be connected to over
  IPv6 on a machine with no IPv6 route, and the connect fails for a reason that has nothing
  to do with the relay. It now prefers an address whose family matches a socket this machine
  bound.

## What is *not* in it

**`turns:`** — TURN over TLS, on 443, which is what gets through a network that inspects
as well as blocks. The relayer still skips a secure URL, and `ice::transport_can_use`
still answers `false` for one, deliberately: an operator writing `turns:` is saying the
relay traffic must be inside TLS, and connecting to it in plaintext would be worse than
not connecting. It is a separate piece of work — a TLS stream type in the runtime, a
certificate and a listener on the server — and it is a separate decision.

**RFC 6062** — TCP allocations, where the *relayed* leg is TCP. Not wanted: media is UDP.

## What it costs the relay, and why that is not new

A relay allocation is a port, and a relay has a finite range of them. Every client now
holds **two** allocations per peer connection where it held one -- the UDP relay and the
TCP one -- because ICE allocates on every URL it is given and both are now reachable. In a
fourteen-player lobby that is 182 allocations across the lobby rather than 91.

That is not a new load on the server, it is the load the server was already carrying.
`Voice.tsx:976` in the 1.x client calls the same `withTcpRelays` and hands both URLs to a
Chromium `RTCPeerConnection`, which allocates on both. The 2.x client was the outlier: it
took the same list and allocated on half of it, because the transport threw the other half
away. This change makes it match the client it replaces rather than exceed it.

Worth watching anyway, because relay rule one exists for a reason -- audit 1 found every
player holding *three* reservations where one would do, and one player exhausting a
server's supply was a real report. If halving the relay's capacity ever matters, the
answer is not to stop offering TCP: it is to decide once per session whether this machine
can reach a relay over UDP at all, and offer the TCP form only when it cannot. That is a
probe and a cached verdict, and it belongs in `acl-net`, not in the fork.

## The writer, and what a full queue throws away

**Added 2026-08-29, after the first version of this document said it was not worth doing.**

`RTCTcpTransport::write` used to be awaited inside the driver's select loop. That was
upstream's design and it was already true of ICE-TCP; what the fork changed is that it put
that await on the media path. A relay that stops draining its socket would fill the send
buffer and then stop that peer connection completely — no reads, no timers, so ICE consent
freshness fails and the connection dies of a stall rather than of the network.

It is gone. `write` is now a synchronous function that hands the framed bytes to a queue
and returns; one task per stream empties that queue. **The property is enforced by the
signature and not by a test**: the function returns `Result<usize>` and not a future, so
there is nothing for the driver to await and no way to reintroduce the stall without
changing the type.

One task per stream, not one per packet. Two writers on one socket would interleave two
half-messages and lose the framing boundary for good, which on a TURN stream is
unrecoverable — there is no marker to resynchronise on.

### What a real-time queue must decide

A queue that only grows is a worse answer than a stall: it turns a wedged socket into a
memory leak, and audio that arrives a second late is not audio. So the queue sheds at 64
messages — a little over a second at a 20 ms frame.

That is a *soft* bound, and the distinction is load-bearing. Nothing yields between the
pushes: `write` is synchronous, `handle_write`'s TCP branch awaits nothing, and the
driver's drain loops have no other await point, so the writer task does not get to run
until a whole pass is over. One SCTP congestion window is hundreds of packets, and a data
channel sending in bulk produces exactly that in one pass, all of it undroppable. A hard
bound of 64 would tear down a socket that is draining perfectly well — measured: with one,
a 1 MiB transfer through a relay reports "could not shed" and takes three minutes instead
of seven seconds. What decides that a socket is actually dead is a ceiling of four
mebibytes, which is not a burst by any reading.

*What* it sheds is the whole design. Both framings carry more than media:

- an ICE-TCP stream carries STUN connectivity checks, DTLS records — which is what the
  data channel runs inside — and RTP/RTCP;
- a TURN stream carries STUN control (Allocate, Refresh, `CreatePermission`, `ChannelBind`)
  and `ChannelData`, and what is *inside* a `ChannelData` is again STUN, DTLS or RTP.

Dropping a packet of RTP costs a frame of audio nobody will notice. Dropping a DTLS record
breaks the connection that carries the audio, permanently. So the queue classifies by RFC
7983 §7 — 128–191 is RTP and RTCP, everything else is not — and only RTP is ever dropped.
The byte is read after the framing header, which is two bytes for RFC 4571 and four for
`ChannelData`; reading offset zero would classify a DTLS record by the high half of its
length, and a 128-byte record would look exactly like RTP. There is a test for precisely
that mistake.

There is a fourth thing on a TURN stream, and missing it was the review's most useful
finding. `rtc-turn` relays media inside a **Send indication** — a STUN message — until a
channel has been bound for that peer, and a `ChannelBind` takes a round trip to the relay.
Worse, a server that refuses `ChannelBind` leaves the client on that path *forever*: every
media packet is then a STUN message, and treating all STUN on the stream as control would
make an entire media stream undroppable. So a Send indication is opened: the `DATA`
attribute is found by walking the attributes — they are padded to four bytes and the
length does not count the padding — and RFC 7983 is applied to what is inside it. The
audio in a Send indication is as droppable as the audio in a `ChannelData`, and the
handshake record travelling beside it is as undroppable.

The oldest media goes first, not the newest: what is at the front has waited longest and is
least worth playing by the time it would arrive.

And past the ceiling, the write fails. What that means then differs by stream, and the
existing wiring already gets it right for each. On a TURN stream the relayer's drain loop
turns the error into a `SocketWriteFailure`, which removes the client and closes the
socket — and a TURN allocation can be made again. On an ICE-TCP stream nothing consumes
it, and that is also correct: `RTCTcpTransport::connect` is reachable only from a remote
passive candidate, so a stream removed there is never rebuilt, and tearing one down would
turn a stall into a dead path. Left alone, the queue drains when the remote resumes, and
if it does not, ICE's own consent freshness fails the pair — the checks travel on that
same stream.

An earlier draft of this reported the failure for both. A reviewer showed that it made
ICE-TCP strictly worse, which is why it does not. The line is logged once per stall rather
than once per packet, so a wedged socket does not flood the log either.

### The failure that now arrives late

A write error used to be the return value of the call the driver made. It is now reported
by the writer task through a new driver event, and the driver does what it did before:
drops the stream and hands `SocketWriteFailure` to the relayer, which removes the client and
lets gathering finish. Without that report the Allocate would merely time out — the relayer
does recover, eight seconds later, by way of `TurnEvent::TransactionTimeout` — so the event
is what keeps the old timing rather than what prevents a hang.

### The read has to let go of the socket too

`AsyncTcpStream` has no `close`: a socket closes when its last `Arc` drops, and an armed
read future parked in `read()` holds one. `read_futures` is a `FuturesUnordered` with no
way to remove an entry, so a stream removed from every map stayed open until the far end
sent something or hung up — for a TURN relay, until the allocation lapses, minutes later.
That is upstream's, not the fork's, but the collision retry below makes it fire four times
where it used to fire once.

The read is now raced against a channel whose sending half lives beside the writer, so
dropping the stream's entry ends the read as well. Both re-arm sites go through one place
that takes the stream and the signal together, because taking them from different maps is
how a read gets armed for a stream that has already gone.

### Dropping a writer has to stop it

`JoinHandle`'s documented behaviour on drop is to **detach**, not to cancel. A writer whose
queue and doorbell have gone would therefore keep running and keep its `Arc` on the socket
alive. `bind_transports` replaces the entire transport on every ICE restart, so each restart
would leave a task parked in a write, holding a socket open on a port the rebind may want
back. `Outbound` has a `Drop` that aborts, which is the only thing standing between this
design and that leak.

## Where it lives

- `vendor/webrtc/` — the fork. Six files differ from upstream.
- `patches/webrtc-0.20.3-turn-tcp.patch` — the diff.
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

- **The fork's own test suite, both trees.** 131 passed and 2 failed on the fork — 123 of
  those are upstream's and 8 are the send queue's — against 123 passed and 2 failed on the
  unmodified copy. The same two failures on each: they need a routable LAN address this
  machine does not have. No upstream test changed behaviour. CI re-runs the unit half and
  not the integration half, because gating on those two would gate on the runner's
  networking.
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
- **`one_advertised_relay_is_allocated_over_udp_and_over_tcp`.** The same server, answering
  on both transports on one port number, given to a client as the single bare `turn:` URL a
  deployment advertises. `with_tcp_relays` adds the TCP form beside it and the client
  allocates over each — so the fork did not trade one transport for the other. It also
  asserts that both Allocates asked for a *UDP* relayed leg, which is the distinction the
  whole fork turns on.
- **The send queue's policy, in the fork's own unit tests**: that RTP is droppable and DTLS
  and STUN are not, on both framings; that a Send indication is judged by what it is
  relaying and a truncated one is never droppable; that the classifying byte is read after
  the framing header and not at offset zero; that a full queue loses the oldest media
  first and never displaces control traffic; that a burst of twelve hundred undroppable
  messages queues rather than failing the stream, and that the byte ceiling still catches a
  dead socket and reports it once; that a batch keeps its order; and that a TURN message
  goes out with nothing in front of it. CI runs these — they open no sockets.
- **`a_bulk_transfer_through_a_tcp_relay_is_not_mistaken_for_a_stall`.** The end-to-end
  half: 384 KiB over a data channel through the TCP relay, arriving whole with nothing shed
  and no writer giving up.
- The whole workspace suite (1,138 tests), `cargo fmt --check`, `cargo clippy --workspace
  --all-targets --features acl-updater/ceremony -- -D warnings`, `cargo deny check bans
  licenses`, and `cargo vet`.

## If upstream ever fixes this

Delete `vendor/webrtc`, the `[patch.crates-io]` entry, the patch file and the `fork` CI
job, and raise the version. Keep `crates/acl-turn-framing` and `crates/acl-net/tests/turn_tcp.rs`:
the first is a dependency-free parser worth having either way, and the second is the test
that would catch a regression in whatever replaces this.
