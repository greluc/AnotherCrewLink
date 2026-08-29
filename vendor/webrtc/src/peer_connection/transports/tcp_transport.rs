use crate::peer_connection::driver::PeerConnectionDriverEvent;
use crate::peer_connection::transports::{TcpReadResult, is_retryable_socket_recv_error};
use crate::runtime::{
    AsyncTcpListener, AsyncTcpStream, JoinHandle, Receiver, Runtime, Sender, channel,
};
use bytes::BytesMut;
use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use log::{error, trace};
use rtc::ice::candidate::Candidate;
use rtc::peer_connection::transport::{
    CandidateConfig, CandidateHostConfig, RTCIceCandidate, RTCIceCandidateInit,
};
use rtc::shared::FourTuple;
use rtc::shared::error::Error;
use rtc::shared::error::Result;
use rtc::shared::tcp_framing::{TcpFrameDecoder, frame_packet};
use rtc::shared::{TaggedBytesMut, TransportContext, TransportProtocol};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

const TCP_READ_BUF_LEN: usize = 4096;

/// How many messages may wait for one stream before the queue starts shedding media.
///
/// **Added by AnotherCrewLink**, with the rest of the writer below. Sixty-four packets is
/// a little over a second of audio at a 20 ms frame, which is far longer than a real-time
/// stream can usefully buffer.
///
/// It is a *soft* bound: see [`SEND_QUEUE_MAX_BYTES`] for why a hard one would fail a
/// healthy stream.
const SEND_QUEUE_SOFT: usize = 64;

/// How many bytes may wait for one stream before it is declared broken.
///
/// **Added by AnotherCrewLink.** The soft bound above cannot be the hard one, because
/// nothing yields between the pushes: `write` is synchronous, `handle_write`'s TCP branch
/// awaits nothing, and the driver's drain loops have no other await point — so the writer
/// task does not get to run until the whole pass is over. One SCTP congestion window is
/// hundreds of packets, and a data channel sending in bulk produces exactly that in one
/// pass, all of it undroppable. Failing the stream there would tear down a socket that is
/// draining perfectly well.
///
/// So a queue that cannot shed is allowed past the soft bound, and it is the byte count
/// that decides. Four mebibytes unsent is not a burst by any reading; it is a socket that
/// stopped moving a long time ago.
const SEND_QUEUE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// The two bytes RFC 4571 puts in front of a packet on a stream.
const RFC4571_HEADER: usize = 2;

/// The four bytes RFC 8656 §12.4 puts in front of relayed data.
const CHANNEL_DATA_HEADER: usize = 4;

/// The fixed part of a STUN message: type, length, cookie and transaction id.
const STUN_HEADER: usize = 20;

/// The type and length in front of every STUN attribute.
const STUN_ATTRIBUTE_HEADER: usize = 4;

/// `Send`, as an indication: RFC 5766 §10.1, the message that relays a payload to a peer
/// before a channel has been bound for it.
const SEND_INDICATION: [u8; 2] = [0x00, 0x16];

/// `DATA`, the attribute a Send indication carries its payload in.
const ATTR_DATA: u16 = 0x0013;

/// How the messages on one TCP stream are delimited.
///
/// Two kinds of stream arrive here and they are framed differently, which is why this
/// cannot be a property of the transport.
///
/// **Added by AnotherCrewLink.** Upstream has only the first: ICE-TCP. See
/// `docs/rust-port/12-turn-over-tcp.md` in that repository for why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Framing {
    /// RFC 4571: a sixteen-bit length in front of every packet. What ICE-TCP uses, and
    /// what every stream here used before TURN arrived.
    Rfc4571,
    /// RFC 8656 §3.1: STUN and `ChannelData` messages back to back, each delimited by its
    /// own header. A TURN server speaks this and knows nothing of RFC 4571 — a length
    /// prefix in front of an Allocate is not a longer Allocate, it is not STUN at all.
    Turn,
}

/// The payload a Send indication is relaying, if this is one.
///
/// **Added by AnotherCrewLink.** `rtc-turn` sends media inside a Send indication until a
/// channel has been bound for the peer — `Relay::send` takes that path for every
/// `BindingState` but `Ready` — and a `ChannelBind` takes a round trip to the relay. So
/// the window in which a relay is most likely to be slow is precisely the window in which
/// none of the traffic is `ChannelData`, and treating every STUN message on the stream as
/// control would make all of it undroppable at once.
///
/// Reading through to the payload is what keeps that from being a choice between dropping
/// a DTLS handshake record and dropping nothing: the audio inside a Send indication is as
/// droppable as the audio inside a `ChannelData`, and the handshake inside one is as
/// undroppable.
fn relayed_payload(message: &[u8]) -> Option<&[u8]> {
    if message.get(..2) != Some(&SEND_INDICATION[..]) {
        return None;
    }
    let mut at = STUN_HEADER;
    while let (Some(kind), Some(length)) = (
        message.get(at..at + 2),
        message.get(at + 2..at + STUN_ATTRIBUTE_HEADER),
    ) {
        let kind = u16::from_be_bytes([*kind.first()?, *kind.get(1)?]);
        let length = usize::from(u16::from_be_bytes([*length.first()?, *length.get(1)?]));
        let value = at + STUN_ATTRIBUTE_HEADER;
        if kind == ATTR_DATA {
            return message.get(value..value.checked_add(length)?);
        }
        // RFC 5389 §15: a value is padded to a multiple of four and the length does not
        // count the padding, so stepping by the length alone walks into the middle of the
        // next attribute.
        at = value.checked_add(length.div_ceil(4).checked_mul(4)?)?;
    }
    None
}

/// Whether losing this message costs a frame of audio or breaks the connection.
///
/// **Added by AnotherCrewLink.** The bytes are already framed, so the payload does not
/// start at zero: RFC 4571 puts a sixteen-bit length in front of it and a `ChannelData`
/// header is four bytes. Both are stepped over before the demultiplexing byte is read.
///
/// What a TURN stream carries is decided by what is inside it: a `ChannelData`'s payload,
/// or a Send indication's — see [`relayed_payload`]. Everything else there is control.
fn droppable(framing: Framing, framed: &[u8]) -> bool {
    let payload = match framing {
        Framing::Rfc4571 => framed.get(RFC4571_HEADER..),
        Framing::Turn => match framed.first() {
            // A channel number is 0x4000–0x7FFF, so the top two bits of a `ChannelData`
            // message are 01 and those of a STUN message are 00.
            Some(first) if first >> 6 == 0b01 => framed.get(CHANNEL_DATA_HEADER..),
            // A Send indication is relaying something, so what it is relaying decides.
            // Every other STUN message here is an Allocate, a Refresh, a
            // `CreatePermission` or a `ChannelBind` — control, rare, and the allocation
            // dies without it.
            _ => relayed_payload(framed),
        },
    };
    // RFC 7983 §7: 128–191 is RTP and RTCP. Everything else that travels on these streams
    // is STUN, or DTLS — which is what carries the data channel — or a TURN message, and
    // losing one of those does not cost a frame of audio, it breaks what carries the audio.
    matches!(payload.and_then(<[u8]>::first), Some(128..=191))
}

/// One message, framed the way its stream expects.
///
/// **Added by AnotherCrewLink.** A TURN message is already framed for a stream when it
/// leaves the client: `rtc-turn`'s `ChannelData::encode` pads to four bytes and a STUN
/// message carries its own length and is aligned by construction. So there it is a copy.
/// RFC 4571's length prefix belongs to ICE-TCP and to nothing else, and putting one in
/// front of an Allocate does not make a longer Allocate -- it makes something that is not
/// STUN at all.
fn frame_for(framing: Framing, message: &[u8]) -> Vec<u8> {
    match framing {
        Framing::Turn => message.to_vec(),
        Framing::Rfc4571 => frame_packet(message),
    }
}

/// One message waiting for its stream.
struct Queued {
    /// Framed and ready to go out as it is.
    bytes: Vec<u8>,
    /// What [`droppable`] said about it, decided once on the way in.
    droppable: bool,
}

/// What a full queue did with a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shed {
    /// There was room.
    Queued,
    /// There was not, and an older packet of media made room.
    Displaced,
    /// There was not, nothing older could be dropped, and this could.
    Dropped,
    /// There was not, and nothing here can be dropped at all.
    ///
    /// `first` is true only for the message that discovered it, so a socket that stops
    /// draining writes one line to the log and not one per packet.
    Stuck { first: bool },
}

/// The bytes waiting for one stream, and the rule for what goes when they stop moving.
///
/// **Added by AnotherCrewLink.** Sending used to be awaited inside the driver's own loop,
/// so a socket that stopped draining stopped the whole peer connection — no reads, no
/// timers, and therefore no consent freshness either. What replaces it must decide what to
/// lose, because a real-time stream that queues is a real-time stream that is already
/// broken.
#[derive(Default)]
struct SendQueue {
    waiting: VecDeque<Queued>,
    /// How much is waiting, so the ceiling does not have to walk the queue to find out.
    bytes: usize,
    /// Whether the ceiling has already been reported. Cleared the moment anything fits
    /// again, so a stream that recovers and stalls again says so twice.
    stuck: bool,
}

impl SendQueue {
    fn push(&mut self, bytes: Vec<u8>, droppable: bool) -> Shed {
        if self.waiting.len() < SEND_QUEUE_SOFT {
            self.enqueue(bytes, droppable);
            return Shed::Queued;
        }
        // The oldest media, not the newest: what is at the front has been waiting longest
        // and is the least worth playing by the time it would arrive.
        if let Some(oldest) = self.waiting.iter().position(|queued| queued.droppable) {
            if let Some(gone) = self.waiting.remove(oldest) {
                self.bytes = self.bytes.saturating_sub(gone.bytes.len());
            }
            self.enqueue(bytes, droppable);
            return Shed::Displaced;
        }
        if droppable {
            return Shed::Dropped;
        }
        // Nothing here can go and neither can this, so the soft bound is not the answer:
        // see `SEND_QUEUE_MAX_BYTES`. A congestion window's worth of data channel arrives
        // in one pass with nothing yielding in between, and that is a busy stream and not
        // a dead one.
        if self.bytes.saturating_add(bytes.len()) > SEND_QUEUE_MAX_BYTES {
            let first = !self.stuck;
            self.stuck = true;
            return Shed::Stuck { first };
        }
        self.enqueue(bytes, droppable);
        Shed::Queued
    }

    fn enqueue(&mut self, bytes: Vec<u8>, droppable: bool) {
        self.bytes = self.bytes.saturating_add(bytes.len());
        self.waiting.push_back(Queued { bytes, droppable });
        self.stuck = false;
    }

    /// Everything waiting, as one buffer.
    ///
    /// Concatenated rather than written one at a time: every framing here is
    /// self-delimiting, so the receiver cannot tell the difference, and one write of a
    /// batch is one segment instead of a dozen.
    fn drain(&mut self) -> Vec<u8> {
        let mut batch = Vec::with_capacity(self.bytes);
        for queued in self.waiting.drain(..) {
            batch.extend_from_slice(&queued.bytes);
        }
        self.bytes = 0;
        self.stuck = false;
        batch
    }
}

/// A lock that a panic elsewhere cannot take away.
///
/// There is no invariant in a [`SendQueue`] that a panic can leave half-applied, so
/// poisoning would only turn somebody else's bug into a dead stream.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The writing half of one stream: what is queued for it, and the task that empties it.
struct Outbound {
    queue: Arc<Mutex<SendQueue>>,
    /// Rung after a push. Capacity one, because a ring already pending is a wake-up that
    /// has not happened yet and a second would tell the writer nothing new.
    doorbell: Sender<()>,
    /// How this stream's messages are delimited, which decides both how they are framed on
    /// the way out and what may be dropped.
    framing: Framing,
    /// The task doing the writing, so that dropping this ends it rather than leaving it
    /// parked in a write to a socket nobody is reading.
    writer: Box<dyn JoinHandle>,
    /// Held, never sent on. Dropping it is what tells the armed read to give up its
    /// reference to the socket; see [`RTCTcpTransport::arm_read`].
    removed: Sender<()>,
    /// The other end, cloned for each new read. `async_channel`'s receiver is a
    /// multi-consumer handle, so a clone stays live after the previous read consumed its
    /// own.
    reader: Receiver<()>,
}

impl Drop for Outbound {
    fn drop(&mut self) {
        // **This is not optional.** `JoinHandle`'s documented drop behaviour is to
        // *detach*, so a writer whose queue and doorbell have gone keeps running and keeps
        // its `Arc<dyn AsyncTcpStream>` alive. `bind_transports` replaces the whole
        // transport on every ICE restart, and without this each restart would leave a task
        // parked in a write, holding a socket open on a port the rebind may want back.
        self.writer.abort();
    }
}

pub(crate) type TcpAcceptResult = (
    SocketAddr,
    io::Result<(Arc<dyn AsyncTcpStream>, SocketAddr)>,
);

pub(crate) struct RTCTcpTransport {
    listeners: HashMap<SocketAddr, Arc<dyn AsyncTcpListener>>,
    streams: HashMap<FourTuple, Arc<dyn AsyncTcpStream>>,
    decoders: HashMap<FourTuple, TcpFrameDecoder>,
    /// The TURN streams, with their own reader. Keyed the same way as `streams`; a key is
    /// in exactly one of this and `decoders`.
    turn_decoders: HashMap<FourTuple, acl_turn_framing::Frames>,
    /// The writing half of each stream. Keyed the same way as `streams`, and present for
    /// every key in it.
    writers: HashMap<FourTuple, Outbound>,
    /// For spawning a writer per stream.
    runtime: Arc<dyn Runtime>,
    /// How a writer reports that its socket is gone. The driver owns the reaction; a task
    /// off to the side must not be the thing that tears a transport down.
    events: Sender<PeerConnectionDriverEvent>,
    pub(crate) accept_futures: FuturesUnordered<BoxFuture<'static, TcpAcceptResult>>,
    pub(crate) read_futures: FuturesUnordered<BoxFuture<'static, TcpReadResult>>,
}

impl RTCTcpTransport {
    pub(crate) fn new(
        tcp_listeners: HashMap<SocketAddr, Arc<dyn AsyncTcpListener>>,
        runtime: Arc<dyn Runtime>,
        events: Sender<PeerConnectionDriverEvent>,
    ) -> Self {
        let accept_futures = FuturesUnordered::new();
        for (local_addr, listener) in &tcp_listeners {
            let local_addr = *local_addr;
            let listener = listener.clone();
            accept_futures.push(
                async move {
                    match listener.accept().await {
                        Ok((stream, peer_addr)) => (local_addr, Ok((stream, peer_addr))),
                        Err(err) => (local_addr, Err(err)),
                    }
                }
                .boxed(),
            );
        }

        Self {
            listeners: tcp_listeners,
            streams: HashMap::new(),
            decoders: HashMap::new(),
            turn_decoders: HashMap::new(),
            writers: HashMap::new(),
            runtime,
            events,
            accept_futures,
            read_futures: FuturesUnordered::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.listeners.is_empty()
    }

    pub(crate) fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    pub(crate) fn has_stream_for(&self, four_tuple: &FourTuple) -> bool {
        self.streams.contains_key(four_tuple)
    }

    fn find_outbound(&self, four_tuple: &FourTuple) -> Option<&Outbound> {
        if let Some(outbound) = self.writers.get(four_tuple) {
            return Some(outbound);
        }
        // The fallback matches on the peer address alone, which is a different stream to
        // the one asked for. It must not cross the two framings: an ICE-TCP write landing
        // on a TURN stream would send a length-prefixed packet to a TURN server, and a
        // TURN write landing on an ICE-TCP stream would send an Allocate to a peer. Both
        // would be accepted by the socket and understood by nobody.
        let key = *self
            .streams
            .iter()
            .filter(|(key, _)| !self.turn_decoders.contains_key(*key))
            .find(|(_, stream)| {
                stream
                    .peer_addr()
                    .is_ok_and(|peer| peer == four_tuple.peer_addr)
            })
            .map(|(key, _)| key)?;
        self.writers.get(&key)
    }

    pub(crate) fn remove_stream(&mut self, four_tuple: &FourTuple) {
        self.streams.remove(four_tuple);
        self.decoders.remove(four_tuple);
        self.turn_decoders.remove(four_tuple);
        // Dropping the writer aborts it; see `impl Drop for Outbound`. Left to itself a
        // writer parked in a write to a socket nobody is reading never returns, and it
        // holds the socket open while it waits. (Added by AnotherCrewLink.)
        self.writers.remove(four_tuple);
    }

    /// Hands a message to the stream's writer. **Does not wait for it to be sent.**
    ///
    /// **Changed by AnotherCrewLink**, and this is the point of the change. This used to
    /// return a future that the driver awaited inside its own select loop, so a socket
    /// that stopped draining stopped that peer connection entirely: no reads, no timers,
    /// no consent freshness. It queues now, and the writer task does the waiting.
    ///
    /// A full queue sheds rather than blocks — see [`SendQueue::push`]. The error case
    /// left is a queue that cannot shed at all, which means every message waiting is
    /// control traffic; that is a broken stream and it is reported as one.
    pub(crate) fn write(&self, msg: &TaggedBytesMut) -> Result<usize> {
        let four_tuple = FourTuple::from(&msg.transport);
        let Some(outbound) = self.find_outbound(&four_tuple) else {
            trace!("No TCP stream found for {:?}", four_tuple);
            return Ok(0);
        };

        let framed = frame_for(outbound.framing, &msg.message);
        let len = msg.message.len();
        let shed = {
            let droppable = droppable(outbound.framing, &framed);
            lock(&outbound.queue).push(framed, droppable)
        };
        match shed {
            Shed::Queued => {}
            Shed::Displaced => {
                trace!(
                    "Send queue for {:?} full; dropped an older packet",
                    four_tuple
                );
            }
            Shed::Dropped => {
                trace!("Send queue for {:?} full; dropped this packet", four_tuple);
                return Ok(0);
            }
            Shed::Stuck { first } => {
                if first {
                    error!(
                        "Send queue for {:?} is full of traffic that cannot be dropped",
                        four_tuple
                    );
                }
                // The error goes back to the caller and no further, deliberately. On a TURN
                // stream the relayer's drain loop turns it into a `SocketWriteFailure`,
                // which removes the client and closes the socket — and a TURN allocation
                // can be made again. On an ICE-TCP stream nothing consumes it, and that is
                // the right answer too: `RTCTcpTransport::connect` is reachable only from a
                // remote passive candidate, so a stream removed here is never rebuilt, and
                // tearing one down would turn a stall into a dead path. Left alone, the
                // queue drains when the remote resumes; if it does not, ICE's own consent
                // freshness fails the pair, because the checks travel on this same stream.
                return Err(Error::Other(format!(
                    "TCP stream {four_tuple:?} is not draining and cannot shed"
                )));
            }
        }
        // A ring already pending is a wake-up that has not happened yet, so a second one
        // would tell the writer nothing it is not about to find out.
        let _ = outbound.doorbell.try_send(());
        Ok(len)
    }

    /// The task that empties one stream's queue.
    ///
    /// **Added by AnotherCrewLink.** It is the only thing that ever writes to this stream,
    /// which is what keeps the framing intact: a batch is written whole, in the order it
    /// was queued, and nothing else can interleave with it. Two tasks writing one stream
    /// would interleave two half-messages and lose the boundary for good.
    ///
    /// Under tokio this lands on the same runtime the driver is running on — `spawn` is
    /// `tokio::spawn`, which attaches to the current context, and every caller of this is
    /// inside the driver loop. A driver pinned to a reactor thread therefore keeps its
    /// writer on that thread too, and the wake-up costs no hop.
    fn spawn_writer(
        &self,
        four_tuple: FourTuple,
        stream: Arc<dyn AsyncTcpStream>,
        queue: Arc<Mutex<SendQueue>>,
        mut doorbell: Receiver<()>,
    ) -> Box<dyn JoinHandle> {
        let events = self.events.clone();
        self.runtime.spawn(Box::pin(async move {
            while doorbell.recv().await.is_some() {
                loop {
                    let batch = lock(&queue).drain();
                    if batch.is_empty() {
                        break;
                    }
                    if let Err(err) = stream.write_all(&batch).await {
                        error!("TCP write to {four_tuple:?} failed: {err}");
                        let _ = events
                            .send(PeerConnectionDriverEvent::TcpStreamWriteFailed(four_tuple))
                            .await;
                        return;
                    }
                }
            }
        }))
    }

    fn arm_accept(&mut self, local_addr: SocketAddr) {
        if let Some(listener) = self.listeners.get(&local_addr).cloned() {
            self.accept_futures.push(
                async move {
                    match listener.accept().await {
                        Ok((stream, peer_addr)) => (local_addr, Ok((stream, peer_addr))),
                        Err(err) => (local_addr, Err(err)),
                    }
                }
                .boxed(),
            );
        }
    }

    /// Arms the next read on a stream, racing it against the stream being removed.
    ///
    /// **The race is added by AnotherCrewLink**, and it is what actually closes a socket.
    /// `AsyncTcpStream` has no `close`: a socket is closed when its last `Arc` drops, and
    /// a read future parked in `read()` holds one. `read_futures` is a `FuturesUnordered`
    /// with no way to remove an entry, so before this a stream that had been removed from
    /// every map stayed open until the far end sent something or hung up — which for a
    /// TURN relay means until the allocation lapses, minutes later. Every collision retry
    /// and every reconfiguration left one behind.
    ///
    /// The signal is the far end of a channel `Outbound` holds, so dropping the writing
    /// half is what ends the read. That makes `remove_stream` and dropping the whole
    /// transport both sufficient.
    fn arm_read(
        &mut self,
        four_tuple: FourTuple,
        stream: Arc<dyn AsyncTcpStream>,
        mut removed: Receiver<()>,
    ) {
        self.read_futures.push(
            async move {
                let mut buf = vec![0u8; TCP_READ_BUF_LEN];
                // The race is resolved in its own scope so the borrow of `buf` ends before
                // `buf` is moved into the result.
                let outcome = {
                    let read = stream.read(&mut buf).fuse();
                    futures::pin_mut!(read);
                    futures::select! {
                        result = read => Some(result),
                        // The channel is never sent on; what ends the wait is its sender
                        // being dropped, so `recv` resolves to `None`.
                        _ = removed.recv().fuse() => None,
                    }
                };
                match outcome {
                    Some(Ok(n)) => TcpReadResult::Packet { four_tuple, n, buf },
                    Some(Err(err)) => TcpReadResult::Error {
                        four_tuple,
                        err,
                        buf,
                    },
                    // Reported as a zero-length read because that is already the
                    // end-of-stream path, and by the time this can happen the stream is
                    // gone from every map anyway.
                    None => TcpReadResult::Packet {
                        four_tuple,
                        n: 0,
                        buf,
                    },
                }
            }
            .boxed(),
        );
    }

    /// Arms the next read on a stream that is still registered, and does nothing for one
    /// that is not.
    ///
    /// **Added by AnotherCrewLink** so the two re-arm sites cannot take the stream from one
    /// map and the removal signal from another: a stream with no writer has been removed,
    /// and arming a read for it would put a reference to the socket back into
    /// `read_futures` with nothing left to end it.
    fn rearm(&mut self, four_tuple: FourTuple) {
        let Some(stream) = self.streams.get(&four_tuple).cloned() else {
            return;
        };
        let Some(reader) = self.writers.get(&four_tuple).map(|out| out.reader.clone()) else {
            return;
        };
        self.arm_read(four_tuple, stream, reader);
    }

    pub(crate) fn register_stream(
        &mut self,
        four_tuple: FourTuple,
        stream: Arc<dyn AsyncTcpStream>,
    ) {
        self.register_stream_with(four_tuple, stream, Framing::Rfc4571);
    }

    /// Registers a stream that carries something other than ICE-TCP.
    ///
    /// **Added by AnotherCrewLink**, so that a TURN allocation can be made over TCP. The
    /// framing is a property of what is on the stream, not of the transport, and getting
    /// it wrong is silent: RFC 4571's two-byte prefix in front of an Allocate does not
    /// produce an error, it produces bytes whose first two bits mean something else.
    pub(crate) fn register_stream_with(
        &mut self,
        four_tuple: FourTuple,
        stream: Arc<dyn AsyncTcpStream>,
        framing: Framing,
    ) {
        self.streams.insert(four_tuple, stream.clone());
        match framing {
            Framing::Rfc4571 => {
                self.decoders.insert(four_tuple, TcpFrameDecoder::new());
            }
            Framing::Turn => {
                self.turn_decoders
                    .insert(four_tuple, acl_turn_framing::Frames::new());
            }
        }
        let queue = Arc::new(Mutex::new(SendQueue::default()));
        let (doorbell, ring) = channel(1);
        let (removed, reader) = channel(1);
        let writer = self.spawn_writer(four_tuple, stream.clone(), Arc::clone(&queue), ring);
        self.writers.insert(
            four_tuple,
            Outbound {
                queue,
                doorbell,
                framing,
                writer,
                removed,
                reader: reader.clone(),
            },
        );
        self.arm_read(four_tuple, stream, reader);
    }

    pub(crate) fn on_accept(
        &mut self,
        local_addr: SocketAddr,
        res: io::Result<(Arc<dyn AsyncTcpStream>, SocketAddr)>,
    ) -> Option<FourTuple> {
        let accepted = match res {
            Ok((stream, peer_addr)) => Some((stream, peer_addr)),
            Err(err) => {
                error!("TCP accept error: {}", err);
                None
            }
        };

        self.arm_accept(local_addr);

        let (stream, peer_addr) = accepted?;

        let stream_local_addr = stream.local_addr().unwrap_or(local_addr);
        let four_tuple = FourTuple {
            local_addr: stream_local_addr,
            peer_addr,
        };
        trace!(
            "Accepted TCP stream on {} from {}",
            stream_local_addr, peer_addr
        );
        self.register_stream(four_tuple, stream);
        Some(four_tuple)
    }

    pub(crate) fn on_read(&mut self, res: TcpReadResult) -> Vec<TaggedBytesMut> {
        let mut out = Vec::new();
        match res {
            TcpReadResult::Packet { four_tuple, n, buf } => {
                if n == 0 {
                    trace!("TCP connection EOF for {:?}", four_tuple);
                    self.remove_stream(&four_tuple);
                } else {
                    let mut lost_the_boundary = false;
                    if let Some(decoder) = self.decoders.get_mut(&four_tuple) {
                        decoder.extend_from_slice(&buf[..n]);
                        while let Some(packet) = decoder.next_packet() {
                            out.push(Self::tagged(four_tuple, &packet));
                        }
                    } else if let Some(frames) = self.turn_decoders.get_mut(&four_tuple) {
                        // The TURN reader, added by AnotherCrewLink. It can fail, and
                        // failing is terminal: there is no marker in this framing to
                        // resynchronise on -- nothing that cannot also occur inside a
                        // payload -- so a stream whose boundary is lost is a stream to
                        // close. Reading on would hand the TURN client bytes that parse
                        // into the wrong thing.
                        frames.feed(&buf[..n]);
                        loop {
                            match frames.next_message() {
                                Ok(Some(message)) => out.push(Self::tagged(four_tuple, &message)),
                                Ok(None) => break,
                                Err(why) => {
                                    error!("TURN stream {:?} is unreadable: {}", four_tuple, why);
                                    lost_the_boundary = true;
                                    break;
                                }
                            }
                        }
                    }
                    if lost_the_boundary {
                        self.remove_stream(&four_tuple);
                    } else {
                        self.rearm(four_tuple);
                    }
                }
            }
            TcpReadResult::Error {
                four_tuple,
                err,
                buf: _,
            } => {
                if is_retryable_socket_recv_error(&err) {
                    trace!("Transient TCP read error on {:?}: {}", four_tuple, err);
                    self.rearm(four_tuple);
                } else {
                    error!("TCP read error on {:?}: {}", four_tuple, err);
                    self.remove_stream(&four_tuple);
                }
            }
        }
        out
    }

    /// One message, tagged with the stream it came off.
    fn tagged(four_tuple: FourTuple, message: &[u8]) -> TaggedBytesMut {
        TaggedBytesMut {
            now: Instant::now(),
            transport: TransportContext {
                local_addr: four_tuple.local_addr,
                peer_addr: four_tuple.peer_addr,
                ecn: None,
                transport_protocol: TransportProtocol::TCP,
            },
            message: BytesMut::from(message),
        }
    }

    pub(crate) fn gather_candidates(&self) -> Vec<RTCIceCandidateInit> {
        let mut candidates = Vec::new();
        for local_addr in self.listeners.keys() {
            // Gather passive TCP candidate
            let passive_config = CandidateHostConfig {
                base_config: CandidateConfig {
                    network: "tcp".to_owned(),
                    address: local_addr.ip().to_string(),
                    port: local_addr.port(),
                    component: 1,
                    ..Default::default()
                },
                tcp_type: rtc::ice::tcp_type::TcpType::Passive,
            };
            if let Ok(candidate) = passive_config.new_candidate_host()
                && let Ok(candidate_init) = RTCIceCandidate::from(&candidate).to_json()
            {
                candidates.push(candidate_init);
            }

            // Gather active TCP candidate
            let active_config = CandidateHostConfig {
                base_config: CandidateConfig {
                    network: "tcp".to_owned(),
                    address: local_addr.ip().to_string(),
                    port: 9, // Discard port placeholder for active candidates
                    component: 1,
                    ..Default::default()
                },
                tcp_type: rtc::ice::tcp_type::TcpType::Active,
            };
            if let Ok(candidate) = active_config.new_candidate_host()
                && let Ok(candidate_init) = RTCIceCandidate::from(&candidate).to_json()
            {
                candidates.push(candidate_init);
            }
        }
        candidates
    }

    pub(crate) fn connect(
        candidate: &Candidate,
        runtime: Arc<dyn Runtime>,
        tx: Sender<PeerConnectionDriverEvent>,
    ) {
        if candidate.network_type().is_tcp()
            && candidate.tcp_type() == rtc::ice::tcp_type::TcpType::Passive
            && let Ok(ip) = candidate.address().parse::<IpAddr>()
        {
            let remote_addr = SocketAddr::new(ip, candidate.port());
            let runtime_clone = runtime.clone();
            runtime.spawn(Box::pin(async move {
                trace!("Initiating TCP connect to {:?}", remote_addr);
                match runtime_clone.connect_tcp(remote_addr).await {
                    Ok(stream) => {
                        let local_addr = stream
                            .local_addr()
                            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap());
                        let peer_addr = stream.peer_addr().unwrap_or(remote_addr);
                        let four_tuple = FourTuple {
                            local_addr,
                            peer_addr,
                        };
                        let _ = tx
                            .send(PeerConnectionDriverEvent::IncomingTcpStream(
                                four_tuple, stream,
                            ))
                            .await;
                    }
                    Err(err) => {
                        error!("Failed to connect TCP to {:?}: {}", remote_addr, err);
                    }
                }
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A framed RTP packet: RFC 4571's length, then a payload beginning with 0x80.
    fn rtp_over_ice_tcp() -> Vec<u8> {
        frame_packet(&[0x80, 0x6f, 0, 1, 0, 0, 0, 0])
    }

    /// A framed DTLS record, which is what carries the data channel.
    fn dtls_over_ice_tcp() -> Vec<u8> {
        frame_packet(&[23, 0xfe, 0xfd, 0, 0])
    }

    /// A `ChannelData` message carrying RTP: channel 0x4000, then the payload.
    fn rtp_over_turn() -> Vec<u8> {
        let mut out = vec![0x40, 0x00, 0x00, 0x08];
        out.extend_from_slice(&[0x80, 0x6f, 0, 1, 0, 0, 0, 0]);
        out
    }

    /// An Allocate request: a STUN message, whose first two bits are zero.
    fn allocate_over_turn() -> Vec<u8> {
        let mut out = vec![0x00, 0x03, 0x00, 0x00];
        out.extend_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
        out.extend_from_slice(&[0; 12]);
        out
    }

    #[test]
    fn media_may_be_dropped_and_nothing_else_may() {
        // The whole shedding policy rests on this one function, and getting it backwards
        // would drop the DTLS handshake to make room for audio.
        assert!(droppable(Framing::Rfc4571, &rtp_over_ice_tcp()));
        assert!(!droppable(Framing::Rfc4571, &dtls_over_ice_tcp()));
        assert!(droppable(Framing::Turn, &rtp_over_turn()));
        assert!(!droppable(Framing::Turn, &allocate_over_turn()));
    }

    #[test]
    fn the_payload_is_not_at_the_start_of_the_message() {
        // RFC 4571's length prefix comes first, so reading byte zero would classify by the
        // high half of a length. A packet of 128 bytes would then look like RTP whatever
        // it actually carried -- including a DTLS record.
        let mut dtls = frame_packet(&[23, 0xfe, 0xfd]);
        assert_eq!(dtls[0], 0);
        dtls[0] = 128;
        assert!(
            !droppable(Framing::Rfc4571, &dtls),
            "the length prefix must not be read as the payload type"
        );
    }

    #[test]
    fn a_truncated_message_is_never_droppable() {
        assert!(!droppable(Framing::Rfc4571, &[]));
        assert!(!droppable(Framing::Rfc4571, &[0, 1]));
        assert!(!droppable(Framing::Turn, &[]));
        assert!(!droppable(Framing::Turn, &[0x40, 0x00, 0x00, 0x04]));
    }

    /// A Send indication relaying `payload`, with the attributes `rtc-turn` puts in one.
    fn send_indication(payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0x00, 0x16, 0x00, 0x00];
        out.extend_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
        out.extend_from_slice(&[0; 12]);
        // XOR-PEER-ADDRESS first, so the walk has to step over an attribute to find DATA.
        out.extend_from_slice(&[0x00, 0x12, 0x00, 0x08]);
        out.extend_from_slice(&[0x00, 0x01, 0x2b, 0x3c, 0x4d, 0x5e, 0x6f, 0x70]);
        out.extend_from_slice(&ATTR_DATA.to_be_bytes());
        #[expect(clippy::cast_possible_truncation, reason = "test payloads are tiny")]
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        out.extend_from_slice(payload);
        out.resize(out.len().div_ceil(4) * 4, 0);
        out
    }

    #[test]
    fn what_a_send_indication_relays_decides_whether_it_may_go() {
        // `rtc-turn` relays media inside a Send indication until a channel is bound, so
        // this is every packet for one round trip to the relay -- and it is the window in
        // which a relay is most likely to be slow. Calling all of it control would make a
        // stalled stream unsheddable exactly when it matters, and calling all of it media
        // would drop the DTLS handshake travelling beside it.
        assert!(droppable(
            Framing::Turn,
            &send_indication(&[0x80, 0x6f, 0, 1, 0, 0, 0, 0])
        ));
        assert!(!droppable(
            Framing::Turn,
            &send_indication(&[23, 0xfe, 0xfd, 0, 0])
        ));
        assert!(!droppable(Framing::Turn, &send_indication(&[0x00, 0x01])));
    }

    #[test]
    fn a_send_indication_with_no_payload_is_not_droppable() {
        // A malformed or truncated one must not be read as media just because the walk
        // fell off the end.
        let mut truncated = send_indication(&[0x80, 0x6f, 0, 1]);
        truncated.truncate(STUN_HEADER + 6);
        assert!(!droppable(Framing::Turn, &truncated));
        assert!(relayed_payload(&allocate_over_turn()).is_none());
        assert!(relayed_payload(&[]).is_none());
    }

    #[test]
    fn a_burst_of_control_traffic_is_not_a_stall() {
        // The driver writes a whole congestion window in one pass and never yields, so the
        // writer cannot drain in between. A hard bound of `SEND_QUEUE_SOFT` would fail a
        // socket that is draining perfectly well; only the byte ceiling decides.
        let mut queue = SendQueue::default();
        for _ in 0..(SEND_QUEUE_SOFT * 20) {
            assert_eq!(queue.push(vec![0; 1200], false), Shed::Queued);
        }
        assert_eq!(queue.waiting.len(), SEND_QUEUE_SOFT * 20);
    }

    #[test]
    fn a_queue_past_the_ceiling_is_a_dead_socket() {
        let mut queue = SendQueue::default();
        let chunk = 64 * 1024;
        while queue.bytes + chunk <= SEND_QUEUE_MAX_BYTES {
            assert_eq!(queue.push(vec![0; chunk], false), Shed::Queued);
        }
        assert_eq!(
            queue.push(vec![0; chunk], false),
            Shed::Stuck { first: true }
        );
        // And only the first one says so, or a wedged socket writes a line per packet for
        // as long as it stays wedged.
        assert_eq!(
            queue.push(vec![0; chunk], false),
            Shed::Stuck { first: false }
        );
        // Draining makes room again, so a slow reader recovers rather than being condemned
        // by one bad moment -- and the next stall is reported afresh.
        let batch = queue.drain();
        assert!(batch.len() <= SEND_QUEUE_MAX_BYTES);
        assert_eq!(queue.bytes, 0);
        assert_eq!(queue.push(vec![0; chunk], false), Shed::Queued);
    }

    #[test]
    fn a_full_queue_loses_the_oldest_media_first() {
        let mut queue = SendQueue::default();
        for _ in 0..SEND_QUEUE_SOFT {
            assert_eq!(queue.push(vec![1], true), Shed::Queued);
        }
        assert_eq!(queue.push(vec![2], true), Shed::Displaced);
        assert_eq!(queue.waiting.len(), SEND_QUEUE_SOFT);
        // The one that went is the one that had waited longest, so what is left is the
        // freshest audio rather than the stalest.
        let batch = queue.drain();
        assert_eq!(batch.len(), SEND_QUEUE_SOFT);
        assert_eq!(batch.last(), Some(&2));
    }

    #[test]
    fn control_traffic_is_never_displaced_by_media() {
        let mut queue = SendQueue::default();
        for _ in 0..SEND_QUEUE_SOFT {
            let _ = queue.push(vec![1], false);
        }
        // Nothing waiting can go, so the new packet of media goes instead of a handshake.
        assert_eq!(queue.push(vec![2], true), Shed::Dropped);
        assert_eq!(queue.waiting.len(), SEND_QUEUE_SOFT);
    }

    #[test]
    fn a_turn_message_goes_out_exactly_as_it_arrived() {
        // The length prefix is the whole hazard: two bytes in front of an Allocate and the
        // server reads something that is not STUN, with no error and no way back.
        let allocate = allocate_over_turn();
        assert_eq!(frame_for(Framing::Turn, &allocate), allocate);

        let framed = frame_for(Framing::Rfc4571, &allocate);
        assert_eq!(framed.len(), allocate.len() + RFC4571_HEADER);
        assert_eq!(
            &framed[RFC4571_HEADER..],
            &allocate[..],
            "the prefix goes in front, it does not replace anything"
        );
    }

    #[test]
    fn a_batch_keeps_the_order_it_was_queued_in() {
        // Every framing here is self-delimiting, so a batch may be one write -- but only
        // if it is the same bytes in the same order.
        let mut queue = SendQueue::default();
        let _ = queue.push(vec![1, 2], true);
        let _ = queue.push(vec![3], false);
        let _ = queue.push(vec![4, 5, 6], true);
        assert_eq!(queue.drain(), vec![1, 2, 3, 4, 5, 6]);
        assert!(queue.drain().is_empty());
    }
}
