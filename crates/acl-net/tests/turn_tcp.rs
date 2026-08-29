//! Two peers connecting through a relay reached over TCP, with no UDP path between them.
//!
//! This is the test the TURN-over-TCP fork exists for. `webrtc =0.20.3` threw away every
//! `?transport=tcp` relay URL before allocating anything, so a player whose network blocks
//! outbound UDP gathered no relay candidate and could reach nobody — which is the one
//! player a relay is deployed for. `vendor/webrtc` carries the patch that removes the
//! skip; `docs/rust-port/12-turn-over-tcp.md` says why it is a fork and not one of the
//! four alternatives.
//!
//! The server below is a real TURN server, not a recording: long-term credentials, a UDP
//! socket per allocation, permissions, channel bindings, Send indications and `ChannelData`
//! in both directions. It speaks only TCP, and it hands out relayed addresses on loopback.
//! Both peer connections are given nothing but its `turn:` URL under
//! [`IceTransportPolicy::Relay`], so **every byte between them goes out over a TCP socket,
//! through the server, and back**. If any part of the fork is wrong — the framing, the
//! `REQUESTED-TRANSPORT` attribute, the re-tagging on the way out, the gathering
//! accounting — there is no second path for the connection to fall back to, and the test
//! fails rather than passing for the wrong reason.
//!
//! It is not gate G3. G3 wanted a 1.0.2 Chromium client on the other side and was struck
//! on 2026-08-25; both ends here are this same crate.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
// The crate's builder and connection futures are 18 KB each. Boxing them inside a test
// would move an allocation nobody measures.
#![allow(clippy::large_futures)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use acl_net::ice::{IceServer, RtcConfig};
use acl_net::rtc::to_configuration;
use rtc::stun::attributes::{ATTR_NONCE, ATTR_REALM};
use rtc::stun::error_code::{CODE_UNAUTHORIZED, ErrorCodeAttribute};
use rtc::stun::fingerprint::FINGERPRINT;
use rtc::stun::integrity::MessageIntegrity;
use rtc::stun::message::{
    CLASS_ERROR_RESPONSE, CLASS_INDICATION, CLASS_REQUEST, CLASS_SUCCESS_RESPONSE, Getter,
    METHOD_ALLOCATE, METHOD_BINDING, METHOD_CHANNEL_BIND, METHOD_CREATE_PERMISSION, METHOD_DATA,
    METHOD_REFRESH, METHOD_SEND, Message, MessageType, Setter, TransactionId, is_stun_message,
};
use rtc::stun::textattrs::{Nonce, Realm, Username};
use rtc::stun::xoraddr::XorMappedAddress;
use rtc::turn::proto::chandata::ChannelData;
use rtc::turn::proto::channum::ChannelNumber;
use rtc::turn::proto::data::Data;
use rtc::turn::proto::lifetime::Lifetime;
use rtc::turn::proto::peeraddr::PeerAddress;
use rtc::turn::proto::relayaddr::RelayedAddress;
use rtc::turn::proto::reqtrans::RequestedTransport;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceCandidateInit,
    RTCPeerConnectionIceEvent, RTCPeerConnectionState,
};

/// Long enough for an allocation, a permission, a channel bind and DTLS over loopback;
/// short enough that a hang fails the suite instead of hanging it.
const PATIENCE: Duration = Duration::from_secs(30);

/// What the test server asks for and the connections are told to send.
const USERNAME: &str = "anothercrewlink";
const PASSWORD: &str = "a-relay-password";
const REALM: &str = "test.invalid";

// ---------------------------------------------------------------------------------------
// The server
// ---------------------------------------------------------------------------------------

/// What one accepted TCP connection has allocated, if anything.
struct Allocation {
    /// The socket the relayed address belongs to. Peers send to it; what arrives is
    /// forwarded to the client over its TCP connection.
    socket: Arc<UdpSocket>,
    /// Channel number to peer address, from `ChannelBind`.
    channels: HashMap<u16, SocketAddr>,
}

/// A TURN server that speaks TCP and nothing else.
///
/// Deliberately not general: it implements exactly the flow `rtc-turn`'s client performs,
/// answers every request it is meant to answer, and does not pretend to enforce anything a
/// real deployment must. What it does do faithfully is the *framing* — messages arrive back
/// to back on a stream with no length prefix in front of them, which is the whole reason
/// [`acl_turn_framing`] exists, and which is what a length-prefixed write would break.
struct TurnServer {
    addr: SocketAddr,
    /// Every `REQUESTED-TRANSPORT` value an Allocate carried, so the test can assert the
    /// client asked for a *UDP* relayed leg while talking over TCP.
    requested_transports: Arc<Mutex<Vec<u8>>>,
}

impl TurnServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requested_transports = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requested_transports);

        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                // Nagle off on the server side too. A relay that batches is a relay that
                // adds a frame of latency to everything it carries.
                let _ = stream.set_nodelay(true);
                let seen = Arc::clone(&seen);
                tokio::spawn(async move {
                    serve(stream, seen).await;
                });
            }
        });

        Self {
            addr,
            requested_transports,
        }
    }

    /// The URL a client is configured with. Names its transport explicitly, so
    /// `with_tcp_relays` leaves it alone and the connection has exactly one relay to try.
    fn url(&self) -> String {
        format!("turn:{}?transport=tcp", self.addr)
    }
}

/// One client connection, from the first byte to the last.
async fn serve(stream: TcpStream, requested_transports: Arc<Mutex<Vec<u8>>>) {
    let peer = stream.peer_addr().unwrap();
    let (mut reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));
    let allocation: Arc<Mutex<Option<Allocation>>> = Arc::new(Mutex::new(None));

    // The same splitter the transport uses, on the other end of the same stream. If the
    // client ever writes an RFC 4571 length prefix, this is what notices.
    let mut frames = acl_turn_framing::Frames::new();
    let mut buf = vec![0_u8; 4096];

    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        frames.feed(&buf[..n]);
        loop {
            match frames.next_message() {
                Ok(Some(message)) => {
                    handle_from_client(&message, peer, &writer, &allocation, &requested_transports)
                        .await;
                }
                Ok(None) => break,
                Err(why) => panic!("the client wrote something that is not TURN framing: {why}"),
            }
        }
    }
}

// One function per protocol message would be six names for six two-line bodies, and the
// shape of a TURN server is exactly this list.
#[expect(clippy::too_many_lines, reason = "a dispatch table for one protocol")]
async fn handle_from_client(
    message: &[u8],
    peer: SocketAddr,
    writer: &Arc<Mutex<OwnedWriteHalf>>,
    allocation: &Arc<Mutex<Option<Allocation>>>,
    requested_transports: &Arc<Mutex<Vec<u8>>>,
) {
    if !is_stun_message(message) {
        // ChannelData: the payload goes out of the allocation's socket to the peer the
        // channel is bound to.
        let mut data = ChannelData {
            raw: message.to_vec(),
            ..Default::default()
        };
        if data.decode().is_err() {
            return;
        }
        let guard = allocation.lock().await;
        if let Some(alloc) = guard.as_ref()
            && let Some(target) = alloc.channels.get(&data.number.0).copied()
        {
            let _ = alloc.socket.send_to(&data.data, target).await;
        }
        return;
    }

    let mut request = Message::new();
    request.raw = message.to_vec();
    if request.decode().is_err() {
        return;
    }

    if request.typ.class == CLASS_INDICATION {
        if request.typ.method == METHOD_SEND {
            let mut peer_address = PeerAddress::default();
            let mut data = Data::default();
            if peer_address.get_from(&request).is_ok() && data.get_from(&request).is_ok() {
                let target = SocketAddr::new(peer_address.ip, peer_address.port);
                let guard = allocation.lock().await;
                if let Some(alloc) = guard.as_ref() {
                    let _ = alloc.socket.send_to(&data.0, target).await;
                }
            }
        }
        return;
    }

    if request.typ.class != CLASS_REQUEST {
        return;
    }

    let integrity = MessageIntegrity::new_long_term_integrity(
        USERNAME.to_owned(),
        REALM.to_owned(),
        PASSWORD.to_owned(),
    );

    // The long-term credential dance: an unauthenticated request is answered with a realm
    // and a nonce, and the client repeats it signed. `rtc-turn`'s client depends on being
    // challenged — it learns the realm from the 401 and has nowhere else to get it.
    if Username::get_from_as(&request, rtc::stun::attributes::ATTR_USERNAME).is_err() {
        let raw = {
            let mut response = Message::new();
            response
                .build(&[
                    Box::new(request.transaction_id),
                    Box::new(MessageType::new(request.typ.method, CLASS_ERROR_RESPONSE)),
                    Box::new(ErrorCodeAttribute {
                        code: CODE_UNAUTHORIZED,
                        reason: b"Unauthorized".to_vec(),
                    }),
                    Box::new(Realm::new(ATTR_REALM, REALM.to_owned())),
                    Box::new(Nonce::new(ATTR_NONCE, "a-nonce".to_owned())),
                    Box::new(FINGERPRINT),
                ])
                .unwrap();
            response.raw
        };
        send(writer, &raw).await;
        return;
    }

    let method = request.typ.method;
    if method == METHOD_ALLOCATE {
        {
            // What the client asked to relay over. RFC 5766 §6.1: this names the protocol
            // of the *relayed* leg and must be UDP for ordinary media, whatever transport
            // carried the request here. The fork's whole difficulty is that `rtc-turn`
            // uses one field for both.
            let mut transport = RequestedTransport::default();
            if transport.get_from(&request).is_ok() {
                requested_transports.lock().await.push(transport.protocol.0);
            }

            let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
            let relayed = socket.local_addr().unwrap();
            *allocation.lock().await = Some(Allocation {
                socket: Arc::clone(&socket),
                channels: HashMap::new(),
            });

            // Everything arriving on the relayed address goes back to this client, as
            // ChannelData when a channel is bound and as a Data indication before that.
            let to_client = Arc::clone(writer);
            let alloc = Arc::clone(allocation);
            tokio::spawn(async move {
                let mut buf = vec![0_u8; 2048];
                loop {
                    let Ok((n, from)) = socket.recv_from(&mut buf).await else {
                        return;
                    };
                    let channel = {
                        let guard = alloc.lock().await;
                        guard.as_ref().and_then(|alloc| {
                            alloc
                                .channels
                                .iter()
                                .find(|(_, addr)| **addr == from)
                                .map(|(number, _)| *number)
                        })
                    };
                    let encoded = if let Some(number) = channel {
                        {
                            let mut data = ChannelData {
                                data: buf[..n].to_vec(),
                                number: ChannelNumber(number),
                                raw: Vec::new(),
                            };
                            data.encode();
                            data.raw
                        }
                    } else {
                        {
                            let mut indication = Message::new();
                            indication
                                .build(&[
                                    Box::new(TransactionId::new()),
                                    Box::new(MessageType::new(METHOD_DATA, CLASS_INDICATION)),
                                    Box::new(PeerAddress {
                                        ip: from.ip(),
                                        port: from.port(),
                                    }),
                                    Box::new(Data(buf[..n].to_vec())),
                                    Box::new(FINGERPRINT),
                                ])
                                .unwrap();
                            std::mem::take(&mut indication.raw)
                        }
                    };
                    send(&to_client, &encoded).await;
                }
            });

            let raw = encode_success(
                &request,
                &integrity,
                vec![
                    Box::new(RelayedAddress {
                        ip: relayed.ip(),
                        port: relayed.port(),
                    }),
                    Box::new(XorMappedAddress {
                        ip: peer.ip(),
                        port: peer.port(),
                    }),
                    // RFC 5766 §6.2: never below the default, and never zero on a success.
                    Box::new(Lifetime(Duration::from_secs(600))),
                ],
            );
            send(writer, &raw).await;
        }
    } else if method == METHOD_CREATE_PERMISSION || method == METHOD_REFRESH {
        {
            // Permissions are not enforced here: this server relays for whoever asks, and
            // what the test is about is the transport underneath, not the access control
            // above it.
            let raw = encode_success(&request, &integrity, vec![]);
            send(writer, &raw).await;
        }
    } else if method == METHOD_CHANNEL_BIND {
        {
            let mut number = ChannelNumber::default();
            let mut peer_address = PeerAddress::default();
            if number.get_from(&request).is_ok() && peer_address.get_from(&request).is_ok() {
                let target = SocketAddr::new(peer_address.ip, peer_address.port);
                if let Some(alloc) = allocation.lock().await.as_mut() {
                    alloc.channels.insert(number.0, target);
                }
            }
            let raw = encode_success(&request, &integrity, vec![]);
            send(writer, &raw).await;
        }
    } else if method == METHOD_BINDING {
        {
            let raw = {
                let mut response = Message::new();
                response
                    .build(&[
                        Box::new(request.transaction_id),
                        Box::new(MessageType::new(METHOD_BINDING, CLASS_SUCCESS_RESPONSE)),
                        Box::new(XorMappedAddress {
                            ip: peer.ip(),
                            port: peer.port(),
                        }),
                        Box::new(FINGERPRINT),
                    ])
                    .unwrap();
                response.raw
            };
            send(writer, &raw).await;
        }
    }
}

/// A signed success response to `request`, carrying `extra`.
///
/// Synchronous on purpose. `dyn Setter` is not `Send`, so a function holding one across an
/// await makes the whole connection task unspawnable; encoding to bytes first keeps every
/// setter inside one statement with no await in it.
fn encode_success(
    request: &Message,
    integrity: &MessageIntegrity,
    extra: Vec<Box<dyn Setter>>,
) -> Vec<u8> {
    let mut setters: Vec<Box<dyn Setter>> = vec![
        Box::new(request.transaction_id),
        Box::new(MessageType::new(request.typ.method, CLASS_SUCCESS_RESPONSE)),
    ];
    setters.extend(extra);
    setters.push(Box::new(integrity.clone()));
    setters.push(Box::new(FINGERPRINT));

    let mut response = Message::new();
    response.build(&setters).unwrap();
    response.raw
}

/// One message onto the stream, with nothing in front of it.
///
/// RFC 8656 §3.1: a TURN message on a stream is delimited by its own header and by
/// nothing else. Adding a length prefix here — which is what RFC 4571 does, and what the
/// transport does for ICE-TCP — would make every byte after it unreadable.
async fn send(writer: &Arc<Mutex<OwnedWriteHalf>>, bytes: &[u8]) {
    let mut guard = writer.lock().await;
    let _ = guard.write_all(bytes).await;
    let _ = guard.flush().await;
}

// ---------------------------------------------------------------------------------------
// The peers
// ---------------------------------------------------------------------------------------

enum Event {
    Candidate(RTCIceCandidateInit),
    State(RTCPeerConnectionState),
    Channel(Arc<dyn DataChannel>),
}

struct Handler {
    events: UnboundedSender<Event>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        if let Ok(init) = event.candidate.to_json() {
            let _ = self.events.send(Event::Candidate(init));
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        let _ = self.events.send(Event::State(state));
    }

    async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
        let _ = self.events.send(Event::Channel(channel));
    }
}

/// A connection whose only way out is the relay, over TCP.
async fn build(url: &str) -> (Arc<dyn PeerConnection>, UnboundedReceiver<Event>) {
    let (events, inbox) = unbounded_channel();
    let server = IceServer {
        urls: vec![url.to_owned()],
        username: Some(USERNAME.to_owned()),
        credential: Some(PASSWORD.to_owned()),
    };
    // `true` is `forceRelayOnly`, and it is honoured because the URL names a relay this
    // transport can now reach. Before the fork `usable_relay` answered no for a TCP URL
    // and this would have quietly come back as `All` — which would let the test pass over
    // a host candidate and prove nothing.
    let configuration = RtcConfig::new(&[server], true);
    assert_eq!(
        configuration.ice_transport_policy,
        acl_net::ice::IceTransportPolicy::Relay,
        "a TCP relay has to count as a relay, or this test proves nothing"
    );

    let connection = PeerConnectionBuilder::new()
        .with_configuration(to_configuration(&configuration))
        .with_handler(Arc::new(Handler { events }))
        .with_udp_addrs(vec!["127.0.0.1:0"])
        .build()
        .await
        .expect("a peer connection");
    (Arc::new(connection), inbox)
}

/// Offer, answer, and every candidate each side gathers, in both directions.
async fn negotiate(
    offerer: &Arc<dyn PeerConnection>,
    answerer: &Arc<dyn PeerConnection>,
    from_offerer: &mut UnboundedReceiver<Event>,
    from_answerer: &mut UnboundedReceiver<Event>,
) -> (Vec<RTCIceCandidateInit>, Vec<RTCIceCandidateInit>) {
    let offer = offerer.create_offer(None).await.expect("an offer");
    offerer
        .set_local_description(offer.clone())
        .await
        .expect("the local description");
    answerer
        .set_remote_description(offer)
        .await
        .expect("the remote description");
    let answer = answerer.create_answer(None).await.expect("an answer");
    answerer
        .set_local_description(answer.clone())
        .await
        .expect("the local description");
    offerer
        .set_remote_description(answer)
        .await
        .expect("the remote description");

    // Both sides gather asynchronously — an allocation over TCP takes a connect, a
    // challenge and a signed retry — so candidates are pumped across for as long as the
    // connection needs them.
    let mut offerer_candidates = Vec::new();
    let mut answerer_candidates = Vec::new();
    for _ in 0..200 {
        while let Ok(event) = from_offerer.try_recv() {
            if let Event::Candidate(candidate) = event {
                offerer_candidates.push(candidate.clone());
                let _ = answerer.add_ice_candidate(candidate).await;
            }
        }
        while let Ok(event) = from_answerer.try_recv() {
            if let Event::Candidate(candidate) = event {
                answerer_candidates.push(candidate.clone());
                let _ = offerer.add_ice_candidate(candidate).await;
            }
        }
        if !offerer_candidates.is_empty() && !answerer_candidates.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    (offerer_candidates, answerer_candidates)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_relay_reached_over_tcp_carries_a_connection() {
    let server = TurnServer::start().await;
    let url = server.url();

    let (offerer, mut from_offerer) = build(&url).await;
    let (answerer, mut from_answerer) = build(&url).await;

    let channel = offerer
        .create_data_channel("voice", None)
        .await
        .expect("a data channel");

    let (offerer_candidates, answerer_candidates) =
        negotiate(&offerer, &answerer, &mut from_offerer, &mut from_answerer).await;

    // Relay candidates, and only relay candidates. Under `Relay` the transport gathers
    // nothing else, so an empty list means the allocation never happened — which is
    // exactly what upstream produced for this URL.
    assert!(
        !offerer_candidates.is_empty() && !answerer_candidates.is_empty(),
        "both sides must gather a relay candidate over TCP; \
         offerer got {} and answerer got {}",
        offerer_candidates.len(),
        answerer_candidates.len()
    );
    for candidate in offerer_candidates.iter().chain(&answerer_candidates) {
        assert!(
            candidate.candidate.contains("typ relay"),
            "relay-only gathered something that is not a relay: {}",
            candidate.candidate
        );
    }

    // The relayed leg was asked for as UDP. `REQUESTED-TRANSPORT` names the protocol
    // between the relay and the far peer, not the one carrying the request — asking for
    // TCP there is RFC 6062, a different feature, which coturn refuses by default. Getting
    // this wrong is the failure mode that looks like everything else working.
    let transports = server.requested_transports.lock().await.clone();
    assert!(
        !transports.is_empty(),
        "no Allocate ever reached the server"
    );
    assert!(
        transports.iter().all(|protocol| *protocol == 17),
        "every Allocate must ask for a UDP relayed leg (17); got {transports:?}"
    );

    // And it connects, over the relay, over TCP.
    let connected = tokio::time::timeout(PATIENCE, async {
        loop {
            while let Ok(Event::State(state)) = from_offerer.try_recv() {
                if state == RTCPeerConnectionState::Connected {
                    return true;
                }
                if state == RTCPeerConnectionState::Failed {
                    return false;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("the connection settled within the timeout");
    assert!(
        connected,
        "the peer connection failed rather than connecting"
    );

    // One frame's worth of payload across it, which is the only proof that the relay is
    // actually forwarding rather than merely allocating.
    let remote = tokio::time::timeout(PATIENCE, async {
        loop {
            while let Ok(event) = from_answerer.try_recv() {
                if let Event::Channel(channel) = event {
                    return channel;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("the answerer saw the data channel");

    let payload = "an opus frame's worth of bytes";
    channel
        .send_text(payload)
        .await
        .expect("a send over the relay");

    let delivered = tokio::time::timeout(PATIENCE, async {
        while let Some(event) = remote.poll().await {
            if let DataChannelEvent::OnMessage(message) = event {
                return String::from_utf8(message.data.to_vec()).ok();
            }
        }
        None
    })
    .await
    .expect("the payload arrived through the relay in time");
    assert_eq!(delivered.as_deref(), Some(payload));

    offerer.close().await.expect("a clean close");
    answerer.close().await.expect("a clean close");
}
