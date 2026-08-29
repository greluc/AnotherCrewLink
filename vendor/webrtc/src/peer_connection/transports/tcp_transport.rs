use crate::peer_connection::driver::PeerConnectionDriverEvent;
use crate::peer_connection::transports::{TcpReadResult, is_retryable_socket_recv_error};
use crate::runtime::{AsyncTcpListener, AsyncTcpStream, Runtime, Sender};
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
use rtc::shared::error::Result;
use rtc::shared::tcp_framing::{TcpFrameDecoder, frame_packet};
use rtc::shared::{TaggedBytesMut, TransportContext, TransportProtocol};
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

const TCP_READ_BUF_LEN: usize = 4096;

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
    pub(crate) accept_futures: FuturesUnordered<BoxFuture<'static, TcpAcceptResult>>,
    pub(crate) read_futures: FuturesUnordered<BoxFuture<'static, TcpReadResult>>,
}

impl RTCTcpTransport {
    pub(crate) fn new(tcp_listeners: HashMap<SocketAddr, Arc<dyn AsyncTcpListener>>) -> Self {
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

    fn find_stream(&self, four_tuple: &FourTuple) -> Option<Arc<dyn AsyncTcpStream>> {
        if let Some(stream) = self.streams.get(four_tuple) {
            return Some(stream.clone());
        }
        // The fallback matches on the peer address alone, which is a different stream to
        // the one asked for. It must not cross the two framings: an ICE-TCP write landing
        // on a TURN stream would send a length-prefixed packet to a TURN server, and a
        // TURN write landing on an ICE-TCP stream would send an Allocate to a peer. Both
        // would be accepted by the socket and understood by nobody.
        self.streams
            .iter()
            .filter(|(key, _)| !self.turn_decoders.contains_key(*key))
            .find(|(_, stream)| {
                stream
                    .peer_addr()
                    .is_ok_and(|peer| peer == four_tuple.peer_addr)
            })
            .map(|(_, stream)| stream.clone())
    }

    pub(crate) fn remove_stream(&mut self, four_tuple: &FourTuple) {
        self.streams.remove(four_tuple);
        self.decoders.remove(four_tuple);
        self.turn_decoders.remove(four_tuple);
    }

    pub(crate) fn write<'a>(
        &self,
        msg: &'a TaggedBytesMut,
    ) -> Pin<Box<dyn Future<Output = Result<usize>> + Send + 'a>> {
        let four_tuple = FourTuple::from(&msg.transport);
        let Some(stream) = self.find_stream(&four_tuple) else {
            trace!("No TCP stream found for {:?}", four_tuple);
            return Box::pin(async { Ok(0) });
        };

        // A TURN message is already framed for a stream when it leaves the client:
        // `rtc-turn`'s `ChannelData::encode` pads to four bytes, and a STUN message
        // carries its own length and is aligned by construction. So the write is a copy.
        // RFC 4571's length prefix belongs to ICE-TCP and to nothing else.
        let framed = if self.turn_decoders.contains_key(&four_tuple) {
            msg.message.to_vec()
        } else {
            frame_packet(&msg.message)
        };
        let len = msg.message.len();
        Box::pin(async move {
            stream.write_all(&framed).await?;
            Ok(len)
        })
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

    fn arm_read(&mut self, four_tuple: FourTuple, stream: Arc<dyn AsyncTcpStream>) {
        self.read_futures.push(
            async move {
                let mut buf = vec![0u8; TCP_READ_BUF_LEN];
                match stream.read(&mut buf).await {
                    Ok(n) => TcpReadResult::Packet { four_tuple, n, buf },
                    Err(err) => TcpReadResult::Error {
                        four_tuple,
                        err,
                        buf,
                    },
                }
            }
            .boxed(),
        );
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
        self.arm_read(four_tuple, stream);
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
                    } else if let Some(stream) = self.streams.get(&four_tuple).cloned() {
                        self.arm_read(four_tuple, stream);
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
                    if let Some(stream) = self.streams.get(&four_tuple).cloned() {
                        self.arm_read(four_tuple, stream);
                    }
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
