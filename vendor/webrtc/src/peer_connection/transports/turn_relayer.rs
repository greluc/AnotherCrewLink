//! TURN relayer for async peer connections.

use crate::runtime::Runtime;
use log::{debug, error, trace, warn};
use rtc::ice::url::SchemeType;
use rtc::peer_connection::configuration::{RTCIceServer, RTCIceTransportPolicy};
use rtc::peer_connection::state::RTCIceGatheringState;
use rtc::peer_connection::transport::{
    CandidateConfig, CandidateRelayConfig, RTCIceCandidate, RTCIceCandidateInit,
};
use rtc::sansio::Protocol;
use rtc::shared::error::{Error, Result};
use rtc::shared::{FourTuple, TaggedBytesMut, TransportContext, TransportProtocol};
use rtc::stun::message::{METHOD_BINDING, Message as StunMessage, is_stun_message};
use rtc::turn::client::{
    Client as TurnClient, ClientConfig as TurnClientConfig, Event as TurnEvent,
};
use rtc::turn::proto::chandata::ChannelData;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

const MAX_PENDING_PACKETS_PER_PEER: usize = 64;

/// How many times a TURN connection over TCP may be made again after landing on a
/// four-tuple that is already a client's.
///
/// **Added by AnotherCrewLink.** A `FourTuple` is an address pair and carries no protocol,
/// so a TCP connection whose ephemeral source port happens to equal a bound UDP socket's
/// port produces the same key as the UDP client on that socket — the two cannot be told
/// apart, and one of them would be lost. Making the connection again draws a different
/// port; the source range is thousands wide, so four attempts is far more than enough and
/// is a bound rather than an expectation.
const MAX_TURN_TCP_ATTEMPTS: u8 = 4;

#[derive(Debug)]
pub(crate) enum RTCTurnRelayEventIn {
    SocketWriteFailure(FourTuple),
    /// A TCP connection to a TURN server is up and its stream is registered.
    ///
    /// **Added by AnotherCrewLink.** Carries the four-tuple the driver registered it
    /// under, which is what every later write and read is keyed on.
    TurnTcpConnected {
        /// The generation the connect was asked for in.
        generation: u64,
        /// Which attempt this was, so a four-tuple collision can be given one more go.
        attempt: u8,
        /// The URL it was for.
        url: String,
        /// What the driver registered the stream as.
        four_tuple: FourTuple,
        /// The credentials for the allocation.
        username: String,
        /// The credential.
        password: String,
    },
    /// A TCP connection to a TURN server could not be made.
    ///
    /// **Added by AnotherCrewLink.** Gathering has to stop waiting for it, or the
    /// end-of-candidates signal never arrives and the connection sits in `Gathering`.
    TurnTcpFailed {
        /// The generation the connect was asked for in.
        generation: u64,
        /// The URL it was for, for the log line.
        url: String,
    },
}

#[derive(Debug)]
pub(crate) enum RTCTurnRelayEventOut {
    LocalIceCandidate(RTCIceCandidateInit),
    TurnGatheringComplete,
    /// Open a TCP connection to a TURN server, and say when it lands.
    ///
    /// **Added by AnotherCrewLink.** This relayer is sans-IO -- it owns no sockets, which
    /// is the crate's whole design -- so a TURN server reached over TCP needs the driver
    /// to make the connection and register the stream. The allocation cannot be started
    /// until it has, because the client's local address must be the one the socket
    /// actually got.
    ConnectTurnTcp {
        /// Which round of transports this belongs to. A connect spawned before the
        /// transports were rebound must be discarded rather than registered into the new
        /// ones.
        generation: u64,
        /// How many times this connection has been made already, counting from zero.
        attempt: u8,
        /// The URL, carried back so the candidate can name it.
        url: String,
        /// Where the server is.
        peer_addr: SocketAddr,
        /// The credentials for the allocation.
        username: String,
        /// The credential.
        password: String,
    },
    /// Close a TCP connection to a TURN server; this relayer is done with it.
    ///
    /// **Added by AnotherCrewLink.** The relayer does not own the socket, so a client it
    /// drops leaves a stream registered in the TCP transport, still armed for reads,
    /// forever.
    CloseTurnTcp(FourTuple),
}

#[derive(Debug)]
struct PendingPermission {
    relay_addr: SocketAddr,
    peer_addr: SocketAddr,
}

struct ManagedTurnClient {
    client: TurnClient,
    url: String,
    allocate_tid: rtc::stun::message::TransactionId,
    local_addr: SocketAddr,
    relay_addr: Option<SocketAddr>,
    gather_finished: bool,
    /// Whether this client reaches its server over TCP.
    ///
    /// **Added by AnotherCrewLink.** The client itself does not know: its
    /// `transport_protocol` is left at UDP on purpose, because `rtc-turn` uses that field
    /// for the STUN `REQUESTED-TRANSPORT` attribute as well -- and per RFC 5766 that names
    /// the protocol of the *relayed* leg, which for ordinary media must be UDP. Asking a
    /// server for a TCP allocation is RFC 6062, a different feature, which coturn refuses
    /// by default. So the transport is applied here instead, on the way out.
    over_tcp: bool,
}

pub(crate) struct RTCTurnRelayer {
    local_addrs: Vec<SocketAddr>,
    ice_servers: Vec<RTCIceServer>,
    ice_gather_policy: RTCIceTransportPolicy,
    state: RTCIceGatheringState,
    /// Host runtime, used to resolve TURN server hostnames.
    runtime: Arc<dyn Runtime>,
    clients: HashMap<FourTuple, ManagedTurnClient>,
    relay_addrs: HashMap<SocketAddr, FourTuple>,
    pending_permissions: HashMap<rtc::stun::message::TransactionId, PendingPermission>,
    pending_permission_pairs: HashMap<(SocketAddr, SocketAddr), rtc::stun::message::TransactionId>,
    pending_packets: HashMap<(SocketAddr, SocketAddr), VecDeque<TaggedBytesMut>>,
    wouts: VecDeque<TaggedBytesMut>,
    routs: VecDeque<TaggedBytesMut>,
    events: VecDeque<RTCTurnRelayEventOut>,
    /// How many TCP connections are still being made.
    ///
    /// **Added by AnotherCrewLink.** Gathering is complete when every client has finished
    /// *and* nothing is still on its way to becoming one. Without this the relayer emits
    /// end-of-candidates inside `gather()` itself, before any connect can land, and the
    /// relay candidate arrives after the connection has given up waiting for it.
    pending_connects: usize,
    /// Which round of transports the current connects belong to.
    ///
    /// **Added by AnotherCrewLink.** `bind_transports` replaces the relayer wholesale; a
    /// connect spawned before that must not register a stream into the transport that
    /// replaced it.
    generation: u64,
}

impl RTCTurnRelayer {
    /// `generation` is where this relayer's counter starts, and it must not restart at
    /// zero when one relayer replaces another.
    ///
    /// **Added by AnotherCrewLink.** `bind_transports` builds a replacement rather than
    /// reconfiguring, so a connect spawned for the relayer being replaced arrives at the
    /// new one — and if both started at zero it would be indistinguishable from a fresh
    /// one, register a stream into the transport that replaced its own, and allocate a
    /// second time on a relay the new `gather()` is already connecting to. The counter is
    /// carried across instead; see [`Self::generation`].
    pub(crate) fn new(
        local_addrs: Vec<SocketAddr>,
        ice_servers: Vec<RTCIceServer>,
        ice_gather_policy: RTCIceTransportPolicy,
        runtime: Arc<dyn Runtime>,
        generation: u64,
    ) -> Self {
        Self {
            local_addrs,
            ice_servers,
            ice_gather_policy,
            state: RTCIceGatheringState::New,
            runtime,
            clients: HashMap::new(),
            relay_addrs: HashMap::new(),
            pending_permissions: HashMap::new(),
            pending_permission_pairs: HashMap::new(),
            pending_packets: HashMap::new(),
            wouts: VecDeque::new(),
            routs: VecDeque::new(),
            events: VecDeque::new(),
            pending_connects: 0,
            generation,
        }
    }

    /// What the next relayer to replace this one should start counting from.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn state(&self) -> RTCIceGatheringState {
        self.state
    }

    pub(crate) fn update_configuration(
        &mut self,
        ice_servers: Vec<RTCIceServer>,
        ice_gather_policy: RTCIceTransportPolicy,
    ) {
        // A pure credential rotation — same servers, same policy, different username or
        // password — must NOT tear the allocations down. `local_addrs` is fixed at
        // construction (the sockets are shared with host candidates), so a teardown here
        // means the following `gather()` re-`Allocate`s on exactly the same 5-tuple. The
        // server still holds the previous allocation for it and answers **437 (Allocation
        // Mismatch)** per RFC 5766 §6.2, and the restart gathers no relay candidate at all
        // (issue #835).
        //
        // An allocation is a 5-tuple resource, independent of the credential that created
        // it, so the correct move is to re-sign the allocation we already have: update the
        // client's credential and Refresh. Allocation, permissions and channel bindings all
        // survive, so the data path is never interrupted.
        if self.is_credential_only_change(&ice_servers, ice_gather_policy) {
            self.rotate_credentials(&ice_servers);
            self.ice_servers = ice_servers;
            return;
        }

        // Which sockets have to be handed back. Collected before the teardown because
        // `remove_client` reports them through `events`, and `events` is cleared below.
        // (Added by AnotherCrewLink along with the rest of TURN over TCP.)
        let over_tcp: Vec<FourTuple> = self
            .clients
            .iter()
            .filter(|(_, managed)| managed.over_tcp)
            .map(|(four_tuple, _)| *four_tuple)
            .collect();

        let keys: Vec<FourTuple> = self.clients.keys().copied().collect();
        for key in keys {
            self.remove_client(key);
        }
        self.relay_addrs.clear();
        self.pending_permissions.clear();
        self.pending_permission_pairs.clear();
        self.pending_packets.clear();
        self.wouts.clear();
        self.routs.clear();
        self.events.clear();

        // A connect spawned for the old configuration must not become a client of the new
        // one: it carries the old server's credentials, and the count it was added to is
        // gone. Raising the generation is what makes it arrive stale.
        self.generation = self.generation.wrapping_add(1);
        self.pending_connects = 0;
        for four_tuple in over_tcp {
            self.events
                .push_back(RTCTurnRelayEventOut::CloseTurnTcp(four_tuple));
        }

        self.ice_servers = ice_servers;
        self.ice_gather_policy = ice_gather_policy;
        self.state = RTCIceGatheringState::New;
    }

    /// True when `ice_servers` names exactly the same TURN URLs as the running
    /// configuration under the same policy, differing only in credentials.
    ///
    /// URLs are compared after parsing, so formatting differences do not matter. Any change
    /// to the server set, their order, or the transport policy falls through to the full
    /// teardown path — those genuinely need re-gathering, and only the credential-rotation
    /// case can reuse an allocation.
    fn is_credential_only_change(
        &self,
        ice_servers: &[RTCIceServer],
        ice_gather_policy: RTCIceTransportPolicy,
    ) -> bool {
        if ice_gather_policy != self.ice_gather_policy || self.clients.is_empty() {
            return false;
        }

        // Nothing to re-sign unless every live client's allocation is established; a client
        // still mid-Allocate has no allocation to keep.
        if self
            .clients
            .values()
            .any(|managed| managed.relay_addr.is_none())
        {
            return false;
        }

        let urls_of = |servers: &[RTCIceServer]| -> Option<Vec<String>> {
            let mut out = Vec::new();
            for server in servers {
                for url in server.urls().ok()? {
                    out.push(url.to_string());
                }
            }
            Some(out)
        };

        match (urls_of(ice_servers), urls_of(&self.ice_servers)) {
            (Some(new_urls), Some(old_urls)) => !new_urls.is_empty() && new_urls == old_urls,
            _ => false,
        }
    }

    /// Re-signs every live allocation with the rotated credential and refreshes it.
    ///
    /// Best-effort per client: a client whose URL is no longer present, or whose refresh
    /// fails to encode, is left alone rather than torn down — its allocation keeps working
    /// on the old credential until it expires, which is strictly better than forcing the
    /// 437 this path exists to avoid.
    fn rotate_credentials(&mut self, ice_servers: &[RTCIceServer]) {
        // url string -> (username, credential), matching how `gather` derives them.
        let mut credentials: HashMap<String, (String, String)> = HashMap::new();
        for ice_server in ice_servers {
            let Ok(urls) = ice_server.urls() else {
                continue;
            };
            for url in urls {
                credentials.insert(
                    url.to_string(),
                    (url.username.clone(), url.password.clone()),
                );
            }
        }

        for managed in self.clients.values_mut() {
            let Some((username, password)) = credentials.get(&managed.url) else {
                continue;
            };

            managed
                .client
                .update_credentials(username.clone(), password.clone());

            if let Err(err) = managed.client.refresh_allocations() {
                warn!(
                    "TURN credential rotation: refresh failed for {} via {}: {}",
                    managed.local_addr, managed.url, err
                );
            } else {
                debug!(
                    "TURN credentials rotated for {} via {}, allocation kept",
                    managed.local_addr, managed.url
                );
            }
        }
    }

    pub(crate) fn is_turn_message(&self, msg: &TaggedBytesMut) -> bool {
        self.matching_client_key(msg).is_some()
    }

    pub(crate) fn contains_local_addr(&self, local_addr: SocketAddr) -> bool {
        self.relay_addrs.contains_key(&local_addr)
    }

    pub(crate) async fn gather(&mut self) -> Result<()> {
        if self.state == RTCIceGatheringState::Gathering {
            return Ok(());
        }

        if self.state == RTCIceGatheringState::Complete {
            self.emit_existing_candidates()?;
            self.events
                .push_back(RTCTurnRelayEventOut::TurnGatheringComplete);
            return Ok(());
        }

        self.state = RTCIceGatheringState::Gathering;

        // Which relays a TCP connection has already been asked for, so that a server
        // advertised twice is one allocation and not two. The UDP path is deduplicated by
        // `clients.contains_key` below, but a TCP client does not exist yet at the point
        // the decision is made, so it needs its own record. (Added by AnotherCrewLink with
        // the rest of TURN over TCP; a relay's port range is finite and holding two
        // allocations where one would do is how a lobby exhausts it.)
        let mut connecting: Vec<SocketAddr> = Vec::new();

        // Clone the handle up front so the per-server borrows below stay disjoint.
        let runtime = Arc::clone(&self.runtime);

        for ice_server in &self.ice_servers {
            let urls = ice_server.urls()?;

            for url in urls {
                if !matches!(url.scheme, SchemeType::Turn | SchemeType::Turns) {
                    continue;
                }

                if url.is_secure() {
                    warn!("Skipping unsupported secure TURN url {}", url);
                    continue;
                }

                // The non-UDP skip that used to stand here is gone. It is why a player
                // on a network blocking outbound UDP gathered no relay candidate and could
                // reach nobody -- the case a relay exists for. `turns:` is still skipped
                // above: a TLS URL that connects in plaintext would be worse than one that
                // does not connect.
                let over_tcp = url.proto.to_string() != "udp";

                let turn_server_addr = format!("{}:{}", url.host, url.port);
                let resolved_addrs = match runtime.resolve_host(&turn_server_addr).await {
                    Ok(addrs) => addrs,
                    Err(err) => {
                        error!(
                            "Failed to resolve TURN server {}: {}",
                            turn_server_addr, err
                        );
                        continue;
                    }
                };

                if over_tcp {
                    // One connection per server, not one per local address: a TCP socket
                    // picks its own source and the four-tuple comes back from the driver
                    // once it has. Asking per interface would open one allocation per
                    // interface, and a relay's port range is finite.
                    //
                    // The family is chosen to match a socket this machine actually bound,
                    // rather than taken from the head of the list: a relay with both an A
                    // and a AAAA record would otherwise be connected to over IPv6 on a
                    // machine with no IPv6 route, and the connect fails for a reason that
                    // has nothing to do with the relay.
                    let reachable = resolved_addrs.iter().copied().find(|addr| {
                        self.local_addrs
                            .iter()
                            .any(|local| local.is_ipv4() == addr.is_ipv4())
                    });
                    let Some(peer_addr) = reachable.or_else(|| resolved_addrs.first().copied())
                    else {
                        continue;
                    };
                    if connecting.contains(&peer_addr) {
                        continue;
                    }
                    connecting.push(peer_addr);
                    self.pending_connects += 1;
                    debug!("TURN over TCP: connecting to {} for {}", peer_addr, url);
                    self.events.push_back(RTCTurnRelayEventOut::ConnectTurnTcp {
                        generation: self.generation,
                        attempt: 0,
                        url: url.to_string(),
                        peer_addr,
                        username: url.username.clone(),
                        password: url.password.clone(),
                    });
                    continue;
                }

                for local_addr in &self.local_addrs {
                    let Some(peer_addr) = resolved_addrs
                        .iter()
                        .copied()
                        .find(|addr| addr.is_ipv4() == local_addr.is_ipv4())
                    else {
                        continue;
                    };

                    let four_tuple = FourTuple {
                        local_addr: *local_addr,
                        peer_addr,
                    };
                    if self.clients.contains_key(&four_tuple) {
                        continue;
                    }

                    let mut client = TurnClient::new(TurnClientConfig {
                        stun_serv_addr: peer_addr.to_string(),
                        turn_serv_addr: peer_addr.to_string(),
                        local_addr: *local_addr,
                        transport_protocol: TransportProtocol::UDP,
                        username: url.username.clone(),
                        password: url.password.clone(),
                        realm: String::new(),
                        software: String::new(),
                        rto_in_ms: 0,
                    })?;

                    let allocate_tid = client.allocate()?;
                    debug!(
                        "TURN allocation started from {} to {} via {}",
                        local_addr, peer_addr, url
                    );

                    self.clients.insert(
                        four_tuple,
                        ManagedTurnClient {
                            client,
                            url: url.to_string(),
                            allocate_tid,
                            local_addr: *local_addr,
                            relay_addr: None,
                            gather_finished: false,
                            over_tcp: false,
                        },
                    );
                }
            }
        }

        // `pending_connects` as well as `clients`, added by AnotherCrewLink: a relayer
        // with no clients *yet* because every relay it was given is reached over TCP is not
        // a relayer that has finished.
        if self.clients.is_empty() && self.pending_connects == 0 {
            self.state = RTCIceGatheringState::Complete;
            self.events
                .push_back(RTCTurnRelayEventOut::TurnGatheringComplete);
        }

        Ok(())
    }

    fn emit_existing_candidates(&mut self) -> Result<()> {
        for managed_client in self.clients.values() {
            if let Some(relay_addr) = managed_client.relay_addr {
                self.events
                    .push_back(RTCTurnRelayEventOut::LocalIceCandidate(
                        Self::build_local_candidate(
                            relay_addr,
                            managed_client.local_addr,
                            &managed_client.url,
                        )?,
                    ));
            }
        }

        Ok(())
    }

    fn build_local_candidate(
        relay_addr: SocketAddr,
        local_addr: SocketAddr,
        url: &str,
    ) -> Result<RTCIceCandidateInit> {
        let candidate = CandidateRelayConfig {
            base_config: CandidateConfig {
                network: "udp".to_owned(),
                address: relay_addr.ip().to_string(),
                port: relay_addr.port(),
                component: 1,
                ..Default::default()
            },
            rel_addr: local_addr.ip().to_string(),
            rel_port: local_addr.port(),
            url: Some(url.to_owned()),
        }
        .new_candidate_relay()?;

        let mut candidate_init = RTCIceCandidate::from(&candidate).to_json()?;
        candidate_init.url = Some(url.to_owned());
        Ok(candidate_init)
    }

    fn maybe_emit_gathering_complete(&mut self) {
        if self.state == RTCIceGatheringState::Gathering
            && self.pending_connects == 0
            && self.clients.values().all(|client| client.gather_finished)
        {
            self.state = RTCIceGatheringState::Complete;
            self.events
                .push_back(RTCTurnRelayEventOut::TurnGatheringComplete);
        }
    }

    fn matching_client_key(&self, msg: &TaggedBytesMut) -> Option<FourTuple> {
        let exact = FourTuple::from(&msg.transport);
        if self.clients.contains_key(&exact) {
            return Some(exact);
        }

        let same_local: Vec<FourTuple> = self
            .clients
            .keys()
            .copied()
            .filter(|four_tuple| four_tuple.local_addr == msg.transport.local_addr)
            .collect();
        if same_local.is_empty() {
            return None;
        }

        if ChannelData::is_channel_data(&msg.message) {
            return Self::match_same_local_client(&same_local, msg.transport.peer_addr);
        }

        if !is_stun_message(&msg.message) {
            return None;
        }

        let mut stun_message = StunMessage::new();
        stun_message.raw = msg.message.to_vec();
        if stun_message.decode().is_err() {
            return None;
        }

        if stun_message.typ.method == METHOD_BINDING {
            return None;
        }

        Self::match_same_local_client(&same_local, msg.transport.peer_addr)
    }

    fn match_same_local_client(
        candidates: &[FourTuple],
        peer_addr: SocketAddr,
    ) -> Option<FourTuple> {
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }

        if let Some(exact) = candidates
            .iter()
            .copied()
            .find(|four_tuple| four_tuple.peer_addr == peer_addr)
        {
            return Some(exact);
        }

        let mut matching_port = candidates
            .iter()
            .copied()
            .filter(|four_tuple| four_tuple.peer_addr.port() == peer_addr.port());
        let first = matching_port.next()?;
        if matching_port.next().is_none() {
            Some(first)
        } else {
            None
        }
    }

    /// Starts an allocation on a TURN server already connected to over TCP.
    ///
    /// **Added by AnotherCrewLink.** Everything about it matches the UDP path in
    /// [`gather`](Self::gather) except where the addresses come from: the local one is the
    /// address the connected socket actually got, not one of `local_addrs`, which are the
    /// UDP sockets shared with the host candidates.
    fn allocate_over_tcp(
        &mut self,
        four_tuple: FourTuple,
        url: String,
        username: String,
        password: String,
    ) -> Result<()> {
        let mut client = TurnClient::new(TurnClientConfig {
            stun_serv_addr: four_tuple.peer_addr.to_string(),
            turn_serv_addr: four_tuple.peer_addr.to_string(),
            local_addr: four_tuple.local_addr,
            // UDP, on a TCP connection, on purpose: this field is also the STUN
            // `REQUESTED-TRANSPORT` attribute, which names the relayed leg. See `over_tcp`.
            transport_protocol: TransportProtocol::UDP,
            username,
            password,
            realm: String::new(),
            software: String::new(),
            rto_in_ms: 0,
        })?;

        let allocate_tid = client.allocate()?;
        debug!(
            "TURN allocation started over TCP from {} to {} via {}",
            four_tuple.local_addr, four_tuple.peer_addr, url
        );

        self.clients.insert(
            four_tuple,
            ManagedTurnClient {
                client,
                url,
                allocate_tid,
                local_addr: four_tuple.local_addr,
                relay_addr: None,
                gather_finished: false,
                over_tcp: true,
            },
        );
        Ok(())
    }

    fn remove_client(&mut self, four_tuple: FourTuple) {
        if let Some(mut managed_client) = self.clients.remove(&four_tuple) {
            if let Some(relay_addr) = managed_client.relay_addr.take() {
                self.relay_addrs.remove(&relay_addr);
                self.pending_packets
                    .retain(|(addr, _), _| *addr != relay_addr);
                self.pending_permissions
                    .retain(|_, pending| pending.relay_addr != relay_addr);
                self.pending_permission_pairs
                    .retain(|(addr, _), _| *addr != relay_addr);
            }
            let _ = managed_client.client.close();
            if managed_client.over_tcp {
                self.events
                    .push_back(RTCTurnRelayEventOut::CloseTurnTcp(four_tuple));
            }
        }
    }

    fn buffer_packet(
        &mut self,
        relay_addr: SocketAddr,
        peer_addr: SocketAddr,
        packet: TaggedBytesMut,
    ) {
        let queue = self
            .pending_packets
            .entry((relay_addr, peer_addr))
            .or_default();
        if queue.len() >= MAX_PENDING_PACKETS_PER_PEER {
            let _ = queue.pop_front();
        }
        queue.push_back(packet);
    }

    fn flush_pending_packets(&mut self, relay_addr: SocketAddr, peer_addr: SocketAddr) {
        let Some(four_tuple) = self.relay_addrs.get(&relay_addr).copied() else {
            return;
        };
        let Some(mut packets) = self.pending_packets.remove(&(relay_addr, peer_addr)) else {
            return;
        };
        let Some(managed_client) = self.clients.get_mut(&four_tuple) else {
            return;
        };

        while let Some(packet) = packets.pop_front() {
            match managed_client
                .client
                .relay(relay_addr)
                .and_then(|mut relay| relay.send_to(&packet.message, peer_addr))
            {
                Ok(()) => {}
                Err(Error::ErrNoPermission) => {
                    self.pending_packets
                        .entry((relay_addr, peer_addr))
                        .or_default()
                        .push_front(packet);
                    break;
                }
                Err(err) => {
                    error!(
                        "Failed to flush buffered relay packet to {} via {}: {}",
                        peer_addr, relay_addr, err
                    );
                }
            }
        }
    }
}

impl Protocol<TaggedBytesMut, TaggedBytesMut, RTCTurnRelayEventIn> for RTCTurnRelayer {
    type Rout = TaggedBytesMut;
    type Wout = TaggedBytesMut;
    type Eout = RTCTurnRelayEventOut;
    type Error = Error;
    type Time = Instant;

    fn handle_read(&mut self, msg: TaggedBytesMut) -> Result<()> {
        if let Some(client_key) = self.matching_client_key(&msg)
            && let Some(managed_client) = self.clients.get_mut(&client_key)
        {
            managed_client.client.handle_read(msg)?;
        }
        Ok(())
    }

    fn poll_read(&mut self) -> Option<Self::Rout> {
        self.routs.pop_front()
    }

    fn handle_write(&mut self, msg: TaggedBytesMut) -> Result<()> {
        let relay_addr = msg.transport.local_addr;
        let peer_addr = msg.transport.peer_addr;

        let Some(four_tuple) = self.relay_addrs.get(&relay_addr).copied() else {
            return Err(Error::Other(format!(
                "unknown relay local address {} for outbound packet",
                relay_addr
            )));
        };
        let Some(managed_client) = self.clients.get_mut(&four_tuple) else {
            return Err(Error::Other(format!(
                "missing TURN client for relay local address {}",
                relay_addr
            )));
        };

        match managed_client
            .client
            .relay(relay_addr)
            .and_then(|mut relay| relay.send_to(&msg.message, peer_addr))
        {
            Ok(()) => Ok(()),
            Err(Error::ErrNoPermission) => {
                if !self
                    .pending_permission_pairs
                    .contains_key(&(relay_addr, peer_addr))
                    && let Some(tid) = managed_client
                        .client
                        .relay(relay_addr)?
                        .create_permission(peer_addr)?
                {
                    self.pending_permissions.insert(
                        tid,
                        PendingPermission {
                            relay_addr,
                            peer_addr,
                        },
                    );
                    self.pending_permission_pairs
                        .insert((relay_addr, peer_addr), tid);
                }

                self.buffer_packet(relay_addr, peer_addr, msg);
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn poll_write(&mut self) -> Option<Self::Wout> {
        for managed_client in self.clients.values_mut() {
            while let Some(mut msg) = managed_client.client.poll_write() {
                // **This is the whole of TURN over TCP, added by AnotherCrewLink.**
                //
                // The client is built with `transport_protocol: UDP` and stays that way,
                // because `rtc-turn` uses that one field for two different things: the
                // transport its bytes leave on, and the STUN `REQUESTED-TRANSPORT`
                // attribute -- which per RFC 5766 §6.1 names the protocol of the *relayed*
                // leg and must be UDP for ordinary media. Setting it to TCP would send over
                // TCP and also ask for an RFC 6062 TCP allocation, which is a different
                // feature and which coturn refuses by default.
                //
                // Re-tagging here separates them. `PeerConnectionDriver::handle_write`
                // tests `transport_protocol == TCP` before anything else and hands the
                // message to the TCP transport; the reply comes back keyed by a
                // `FourTuple`, which drops the protocol, so it reaches this same client.
                if managed_client.over_tcp {
                    msg.transport.transport_protocol = TransportProtocol::TCP;
                }
                self.wouts.push_back(msg);
            }
        }
        self.wouts.pop_front()
    }

    fn handle_event(&mut self, evt: RTCTurnRelayEventIn) -> Result<()> {
        match evt {
            RTCTurnRelayEventIn::SocketWriteFailure(four_tuple) => {
                self.remove_client(four_tuple);
                self.maybe_emit_gathering_complete();
            }
            RTCTurnRelayEventIn::TurnTcpConnected {
                generation,
                attempt,
                url,
                four_tuple,
                username,
                password,
            } => {
                if generation != self.generation {
                    // The configuration changed while this was being connected. Its place in
                    // the count went with the generation it belonged to, so only the socket
                    // is left to give back.
                    debug!(
                        "Discarding a TURN TCP connection to {} from a past configuration",
                        url
                    );
                    self.events
                        .push_back(RTCTurnRelayEventOut::CloseTurnTcp(four_tuple));
                    return Ok(());
                }
                if self.clients.contains_key(&four_tuple) {
                    // The socket drew a source port that one of the UDP sockets is already
                    // bound to, so this connection's key is a client's key. `FourTuple` is
                    // an address pair with no protocol in it, so nothing downstream could
                    // tell the two apart: every read off this stream would be handed to the
                    // UDP client, and this allocation would never be made. Give the socket
                    // back and draw again.
                    self.events
                        .push_back(RTCTurnRelayEventOut::CloseTurnTcp(four_tuple));
                    if attempt < MAX_TURN_TCP_ATTEMPTS {
                        debug!(
                            "TURN over TCP: {:?} collides with a UDP client; connecting again",
                            four_tuple
                        );
                        self.events.push_back(RTCTurnRelayEventOut::ConnectTurnTcp {
                            generation,
                            attempt: attempt + 1,
                            url,
                            peer_addr: four_tuple.peer_addr,
                            username,
                            password,
                        });
                        // Still outstanding, so the count is left alone and gathering keeps
                        // waiting for it.
                        return Ok(());
                    }
                    warn!(
                        "TURN over TCP: gave up on {} after {} four-tuple collisions",
                        url, MAX_TURN_TCP_ATTEMPTS
                    );
                    self.pending_connects = self.pending_connects.saturating_sub(1);
                    self.maybe_emit_gathering_complete();
                    return Ok(());
                }
                self.pending_connects = self.pending_connects.saturating_sub(1);
                if let Err(err) = self.allocate_over_tcp(four_tuple, url, username, password) {
                    error!("Failed to start a TURN allocation over TCP: {}", err);
                    self.events
                        .push_back(RTCTurnRelayEventOut::CloseTurnTcp(four_tuple));
                }
                self.maybe_emit_gathering_complete();
            }
            RTCTurnRelayEventIn::TurnTcpFailed { generation, url } => {
                if generation != self.generation {
                    return Ok(());
                }
                self.pending_connects = self.pending_connects.saturating_sub(1);
                warn!("TURN over TCP: could not connect to {}", url);
                // Without this, a relay that cannot be reached leaves gathering unfinished
                // and the connection waits for candidates that will never come.
                self.maybe_emit_gathering_complete();
            }
        }
        Ok(())
    }

    fn poll_event(&mut self) -> Option<Self::Eout> {
        let keys: Vec<FourTuple> = self.clients.keys().copied().collect();
        for four_tuple in keys {
            let mut gathered_complete = false;
            let mut local_candidate = None;
            let mut pending_flush: Vec<(SocketAddr, SocketAddr)> = vec![];
            let mut pending_drop: Vec<(SocketAddr, SocketAddr)> = vec![];
            let mut read_msgs: Vec<TaggedBytesMut> = vec![];

            if let Some(managed_client) = self.clients.get_mut(&four_tuple) {
                while let Some(event) = managed_client.client.poll_event() {
                    match event {
                        TurnEvent::AllocateResponse(tid, relay_addr) => {
                            if tid == managed_client.allocate_tid {
                                managed_client.relay_addr = Some(relay_addr);
                                managed_client.gather_finished = true;
                                self.relay_addrs.insert(relay_addr, four_tuple);
                                local_candidate = Some(Self::build_local_candidate(
                                    relay_addr,
                                    managed_client.local_addr,
                                    &managed_client.url,
                                ));
                                gathered_complete = true;
                            }
                        }
                        TurnEvent::AllocateError(tid, err) => {
                            if tid == managed_client.allocate_tid {
                                error!(
                                    "TURN allocation failed from {} to {}: {}",
                                    four_tuple.local_addr, four_tuple.peer_addr, err
                                );
                                managed_client.gather_finished = true;
                                gathered_complete = true;
                            }
                        }
                        TurnEvent::CreatePermissionResponse(tid, peer_addr) => {
                            if let Some(pending) = self.pending_permissions.remove(&tid) {
                                self.pending_permission_pairs
                                    .remove(&(pending.relay_addr, pending.peer_addr));
                                pending_flush.push((pending.relay_addr, peer_addr));
                            }
                        }
                        TurnEvent::CreatePermissionError(tid, err) => {
                            error!("TURN permission request failed: {}", err);
                            if let Some(pending) = self.pending_permissions.remove(&tid) {
                                self.pending_permission_pairs
                                    .remove(&(pending.relay_addr, pending.peer_addr));
                                pending_drop.push((pending.relay_addr, pending.peer_addr));
                            }
                        }
                        TurnEvent::DataIndicationOrChannelData(_, peer_addr, data) => {
                            if let Some(relay_addr) = managed_client.relay_addr {
                                read_msgs.push(TaggedBytesMut {
                                    now: Instant::now(),
                                    transport: TransportContext {
                                        local_addr: relay_addr,
                                        peer_addr,
                                        ecn: None,
                                        transport_protocol: TransportProtocol::UDP,
                                    },
                                    message: data,
                                });
                            }
                        }
                        TurnEvent::TransactionTimeout(tid) => {
                            error!("TURN transaction timed out: {:?}", tid);
                            if let Some(pending) = self.pending_permissions.remove(&tid) {
                                self.pending_permission_pairs
                                    .remove(&(pending.relay_addr, pending.peer_addr));
                                pending_drop.push((pending.relay_addr, pending.peer_addr));
                            } else if tid == managed_client.allocate_tid {
                                managed_client.gather_finished = true;
                                gathered_complete = true;
                            }
                        }
                        TurnEvent::BindingResponse(_, _) | TurnEvent::BindingError(_, _) => {}
                    }
                }
            }

            for (relay_addr, peer_addr) in pending_flush {
                self.flush_pending_packets(relay_addr, peer_addr);
            }
            for (relay_addr, peer_addr) in pending_drop {
                self.pending_packets.remove(&(relay_addr, peer_addr));
            }
            for msg in read_msgs {
                self.routs.push_back(msg);
            }
            if let Some(candidate_result) = local_candidate {
                match candidate_result {
                    Ok(candidate) => {
                        trace!("LocalRelayCandidate {:?}", candidate);
                        self.events
                            .push_back(RTCTurnRelayEventOut::LocalIceCandidate(candidate));
                    }
                    Err(err) => {
                        error!("failed to build relay candidate after allocation: {}", err);
                    }
                }
            }
            if gathered_complete {
                self.maybe_emit_gathering_complete();
            }
        }

        self.events.pop_front()
    }

    fn handle_timeout(&mut self, now: Self::Time) -> Result<()> {
        for managed_client in self.clients.values_mut() {
            managed_client.client.handle_timeout(now)?;
        }
        Ok(())
    }

    fn poll_timeout(&mut self) -> Option<Self::Time> {
        let mut eto = None;
        for managed_client in self.clients.values_mut() {
            if let Some(next) = managed_client.client.poll_timeout() {
                eto = Some(eto.map_or(next, |current| std::cmp::min(current, next)));
            }
        }
        eto
    }

    fn close(&mut self) -> Result<()> {
        let keys: Vec<FourTuple> = self.clients.keys().copied().collect();
        for key in keys {
            self.remove_client(key);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use rtc::peer_connection::configuration::RTCIceServer;
    use rtc::stun::attributes::{ATTR_NONCE, ATTR_REALM};
    use rtc::stun::error_code::CODE_UNAUTHORIZED;
    use rtc::stun::message::{CLASS_ERROR_RESPONSE, MessageType, TransactionId};
    use rtc::stun::textattrs::{Nonce, Realm};
    use std::net::{IpAddr, Ipv4Addr};

    fn build_turn_allocate_unauthorized(transaction_id: TransactionId) -> StunMessage {
        let mut msg = StunMessage::new();
        msg.build(&[
            Box::new(transaction_id),
            Box::new(MessageType::new(
                rtc::stun::message::METHOD_ALLOCATE,
                CLASS_ERROR_RESPONSE,
            )),
            Box::new(CODE_UNAUTHORIZED),
            Box::new(Realm::new(ATTR_REALM, "webrtc.rs".to_owned())),
            Box::new(Nonce::new(ATTR_NONCE, "nonce".to_owned())),
        ])
        .expect("failed to build TURN unauthorized response");
        msg
    }

    #[test]
    fn routes_turn_allocate_response_by_local_addr_and_port() {
        futures::executor::block_on(async {
            let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50000);
            let turn_peer_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3478);
            let mut relayer = RTCTurnRelayer::new(
                vec![local_addr],
                vec![RTCIceServer {
                    urls: vec![format!("turn:{}?transport=udp", turn_peer_addr)],
                    username: "user".to_owned(),
                    credential: "pass".to_owned(),
                }],
                RTCIceTransportPolicy::Relay,
                crate::runtime::default_runtime().expect("test requires a runtime feature"),
                0,
            );

            relayer.gather().await.expect("TURN gather should start");
            let initial_request = relayer.poll_write().expect("initial Allocate request");
            assert_eq!(initial_request.transport.peer_addr, turn_peer_addr);

            let mut initial_request_msg = StunMessage::new();
            initial_request_msg.raw = initial_request.message.to_vec();
            initial_request_msg
                .decode()
                .expect("decode initial Allocate request");

            let response = build_turn_allocate_unauthorized(initial_request_msg.transaction_id);
            let msg = TaggedBytesMut {
                now: Instant::now(),
                transport: TransportContext {
                    local_addr,
                    peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), 3478),
                    ecn: None,
                    transport_protocol: TransportProtocol::UDP,
                },
                message: BytesMut::from(&response.raw[..]),
            };

            assert!(
                relayer.is_turn_message(&msg),
                "TURN error response on the same local socket and TURN port should route to the relayer"
            );

            relayer
                .handle_read(msg)
                .expect("relayer should accept TURN unauthorized response");

            let retry_request = relayer
                .poll_write()
                .expect("authenticated Allocate retry after unauthorized response");
            assert_eq!(retry_request.transport.peer_addr.port(), 3478);
            assert!(
                retry_request.message.len() > initial_request.message.len(),
                "authenticated retry should be larger than the unauthenticated Allocate request"
            );
        });
    }
}
