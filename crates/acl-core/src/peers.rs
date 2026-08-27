//! One `webrtc` connection per member of the lobby.
//!
//! The other half of §4.6 item 3. [`crate::session`] owns the socket and the membership
//! and says who is in the lobby; this owns the connections and says how to reach them.
//! Everything either of them decides was decided in `acl-net` and tested there — the
//! candidate queue, the generation gate, the signal route, the repair policy — and what is
//! here is the part that has to hold a `PeerConnection` to be worth anything.
//!
//! # The generation is shared, and that is the whole point of it
//!
//! §4.6 item 2: the `webrtc` crate takes one `Arc<dyn PeerConnectionEventHandler>` and
//! offers no way to detach it, so the protection `peer.ts` got from nulling five handlers
//! before `close()` has to be a value the handler reads.
//!
//! `acl-net`'s loopback test carries that value by copy, which is correct there because it
//! never replaces a connection. Here it must be an `AtomicU64` the set can raise: a
//! connection being rebuilt keeps running for a while, and its handler goes on being
//! called. Copied, the old handler would compare its own generation against itself, decide
//! it was current, and feed the candidates of a dead connection into a live one.
//!
//! # The track is not optional, and finding that out was the point of building this
//!
//! The first version of this module said audio was somebody else's boundary: open the
//! connection, report its state, attach a track later. It could not connect. An offer with
//! no media has no `m=` line, so it carries no ICE credentials at all, and the far end
//! answers `set_remote_description called with no ice-ufrag`.
//!
//! So every connection carries one Opus audio track from the moment it is built. That is
//! not audio integration -- nothing is written into it here -- it is the shape of the
//! negotiation, which is this layer's own business. Opus at 48 kHz stereo because that is
//! what `acl-audio` encodes and what a 1.x peer offers; a track added later would be a
//! renegotiation on every connection, for nothing.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use acl_net::ice::RtcConfig;
use acl_net::peer::{CandidateQueue, Generation, is_current};
use acl_net::rtc::to_configuration;
use acl_net::signal_route::{PeerState, SignalRoute, route_signal};
use rtc::media_stream::MediaStreamTrack;
use rtc::peer_connection::configuration::media_engine::MIME_TYPE_OPUS;
use rtc::rtp_transceiver::rtp_sender::{
    RTCRtpCodec, RTCRtpCodingParameters, RTCRtpEncodingParameters, RtpCodecKind,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use webrtc::media_stream::track_local::static_sample::TrackLocalStaticSample;
use webrtc::peer_connection::MediaEngine;
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceCandidateInit,
    RTCPeerConnectionIceEvent, RTCPeerConnectionState, RTCSessionDescription,
};

use crate::signalling::Payload;

/// Something to send to a peer through the signalling server.
#[derive(Clone, Debug, PartialEq)]
pub struct Outbound {
    /// Whose socket it goes to.
    pub to: String,
    /// What to send.
    pub payload: Payload,
}

/// One Opus packet that arrived from a peer.
///
/// The payload as it came off the wire, undecoded. Decoding belongs to `acl-audio` and
/// happens where the jitter buffer is; this crate's job is to say who it came from and in
/// what order, which is what the sequence number and timestamp are for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incoming {
    /// Whose socket it came from.
    pub peer: String,
    /// The RTP sequence number, which is what a jitter buffer orders by.
    pub sequence: u16,
    /// The RTP timestamp, in the 48 kHz clock Opus uses here.
    pub timestamp: u32,
    /// The Opus packet.
    pub payload: Vec<u8>,
}

/// Something that happened to a connection.
#[derive(Clone, Debug, PartialEq)]
pub enum PeerEvent {
    /// Its state changed.
    StateChanged {
        /// Whose.
        peer: String,
        /// The new state.
        state: RTCPeerConnectionState,
    },
}

/// Why an operation on a peer failed.
#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    /// The `webrtc` crate refused something.
    #[error("the peer connection failed: {0}")]
    Rtc(#[from] webrtc::error::Error),
    /// A signal arrived that could not be made sense of.
    #[error("a signal that is not one: {0}")]
    Malformed(String),
}

/// What the handler reports back, tagged with the connection it came from.
enum Inbound {
    Candidate {
        peer: String,
        generation: Generation,
        init: RTCIceCandidateInit,
    },
    Audio(Incoming),
    State {
        peer: String,
        generation: Generation,
        state: RTCPeerConnectionState,
    },
}

/// Forwards a connection's events, and stops forwarding once it has been replaced.
struct Handler {
    peer: String,
    generation: Generation,
    /// The generation the set considers current for this peer.
    ///
    /// Shared and atomic rather than copied. See the module documentation: a copy makes
    /// every handler eternally current, which is the failure this whole mechanism exists
    /// to prevent.
    current: Arc<AtomicU64>,
    events: UnboundedSender<Inbound>,
}

impl Handler {
    fn forward(&self, make: impl FnOnce() -> Inbound) {
        if !is_current(
            self.generation,
            Generation::from_raw(self.current.load(Ordering::Acquire)),
        ) {
            return;
        }
        let _ = self.events.send(make());
    }
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        let Ok(init) = event.candidate.to_json() else {
            return;
        };
        self.forward(|| Inbound::Candidate {
            peer: self.peer.clone(),
            generation: self.generation,
            init,
        });
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        self.forward(|| Inbound::State {
            peer: self.peer.clone(),
            generation: self.generation,
            state,
        });
    }

    /// The far end's audio.
    ///
    /// A task per track, and it ends when the track does: `poll` returns `None`, or the
    /// generation moves on and `forward` starts discarding. A task that outlived its
    /// connection would be a task feeding audio from a peer that has gone into a mixer
    /// that still has a slot for them.
    ///
    /// The payload is passed on undecoded. Decoding belongs where the jitter buffer is, and
    /// this crate does not have one -- what it can say is who it came from and in what
    /// order, which is exactly what a jitter buffer needs and all it needs.
    async fn on_track(&self, track: Arc<dyn webrtc::media_stream::track_remote::TrackRemote>) {
        use webrtc::media_stream::track_remote::TrackRemoteEvent;

        let peer = self.peer.clone();
        let generation = self.generation;
        let current = Arc::clone(&self.current);
        let events = self.events.clone();
        tokio::spawn(async move {
            while let Some(event) = track.poll().await {
                if !is_current(
                    generation,
                    Generation::from_raw(current.load(Ordering::Acquire)),
                ) {
                    return;
                }
                let TrackRemoteEvent::OnRtpPacket(packet) = event else {
                    continue;
                };
                // An empty payload is a packet the far end sent to keep the stream alive --
                // DTX does exactly that -- and it is not audio to hand anybody.
                if packet.payload.is_empty() {
                    continue;
                }
                let sent = events.send(Inbound::Audio(Incoming {
                    peer: peer.clone(),
                    sequence: packet.header.sequence_number,
                    timestamp: packet.header.timestamp,
                    payload: packet.payload.to_vec(),
                }));
                if sent.is_err() {
                    return;
                }
            }
        });
    }
}

/// The microphone track every connection carries.
///
/// One per connection rather than one shared: a track belongs to the sender that carries
/// it, and the SSRC is per connection.
///
/// The SSRC is derived from the peer's socket id rather than chosen at random. Random would
/// need a generator this crate does not otherwise have, and a stable one per peer means a
/// rebuild reuses the same synchronisation source — which is what the far end expects of a
/// connection that has been repaired rather than replaced.
/// The synchronisation source for a peer's track.
///
/// Its own function because two things need it and they must not disagree: the track is
/// built with it and every sample written to that track has to name it.
fn microphone_ssrc(peer: &str) -> u32 {
    let mut ssrc: u32 = 0x811c_9dc5;
    for byte in peer.as_bytes() {
        ssrc = (ssrc ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
    }
    ssrc
}

fn microphone_track(
    peer: &str,
    generation: Generation,
) -> Result<Arc<TrackLocalStaticSample>, PeerError> {
    let ssrc = microphone_ssrc(peer);
    let track = MediaStreamTrack::new(
        format!("acl-{peer}"),
        format!("acl-audio-{}", generation.raw()),
        "AnotherCrewLink microphone".to_owned(),
        RtpCodecKind::Audio,
        vec![RTCRtpEncodingParameters {
            rtp_coding_parameters: RTCRtpCodingParameters {
                ssrc: Some(ssrc),
                ..Default::default()
            },
            codec: RTCRtpCodec {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                clock_rate: 48_000,
                channels: 2,
                sdp_fmtp_line: String::new(),
                rtcp_feedback: vec![],
            },
            ..Default::default()
        }],
    );
    Ok(Arc::new(TrackLocalStaticSample::new(track)?))
}

/// The payload type Opus is registered at.
///
/// 111, from the `rtc` crate's own default media engine -- `register_default_codecs`
/// registers Opus there and nowhere else, so this is read out of the crate rather than
/// taken from the convention it happens to match.
const OPUS_PAYLOAD_TYPE: u8 = 111;

/// One connection and what is known about it.
struct Peer {
    connection: Arc<dyn PeerConnection>,
    /// The synchronisation source the track was built with, so a sample can name it.
    ssrc: u32,
    /// The outgoing track, kept rather than handed to `add_track` and forgotten.
    ///
    /// Until this was kept there was nowhere to write audio to: the track existed because
    /// an offer with no media has no ICE credentials, and it carried silence because
    /// nothing could reach it.
    microphone: Arc<TrackLocalStaticSample>,
    generation: Generation,
    current: Arc<AtomicU64>,
    queue: CandidateQueue<RTCIceCandidateInit>,
    /// Whether a remote description has been applied.
    ///
    /// `peer.ts`'s `negotiated`, and [`acl_net::signal_route`] reads it: an offer arriving
    /// for a connection that has one is a renegotiation to apply, and for one that does
    /// not it is a fresh start.
    negotiated: bool,
}

/// Every connection this client holds.
pub struct PeerSet {
    config: RtcConfig,
    peers: HashMap<String, Peer>,
    events: UnboundedSender<Inbound>,
    inbox: UnboundedReceiver<Inbound>,
}

impl PeerSet {
    /// An empty set, configured from what the server advertised.
    #[must_use]
    pub fn new(config: RtcConfig) -> Self {
        let (events, inbox) = unbounded_channel();
        Self {
            config,
            peers: HashMap::new(),
            events,
            inbox,
        }
    }

    /// Replaces the configuration for connections built after this.
    ///
    /// Existing ones keep theirs. A relay credential expires and the server issues a new
    /// one on the next connect; rebuilding every live connection to adopt it would drop
    /// the call to fix something that is not broken.
    pub fn reconfigure(&mut self, config: RtcConfig) {
        self.config = config;
    }

    /// How many connections are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Whether there is a connection for this peer.
    #[must_use]
    pub fn holds(&self, peer: &str) -> bool {
        self.peers.contains_key(peer)
    }

    /// Opens a connection to a peer and offers.
    ///
    /// For [`crate::session::Arrival::Newcomer`] only. Offering to somebody who was
    /// already in the lobby races the offer they are making, which is the glare the
    /// arrival distinction exists to prevent.
    ///
    /// # Errors
    ///
    /// [`PeerError`] if the connection cannot be built or the offer cannot be made.
    pub async fn offer(&mut self, peer: &str) -> Result<Outbound, PeerError> {
        // A connection that already exists is renegotiated, not replaced, and the two are
        // not interchangeable. `signalRoute.ts` exists because the shipped client treated
        // every offer as the start of a new connection: the repair for a stalled link was
        // what killed it. Rebuilding here would put that back on the sending side, where
        // no routing rule can catch it.
        let renegotiation = self.holds(peer);
        if !renegotiation {
            Box::pin(self.build(peer)).await?;
        }
        let connection = self.connection(peer)?;
        let offer = connection.create_offer(None).await?;
        connection.set_local_description(offer.clone()).await?;
        Ok(Outbound {
            to: peer.to_owned(),
            payload: Payload::Offer {
                sdp: offer.sdp,
                renegotiation,
            },
        })
    }

    /// Handles one signal from a peer.
    ///
    /// # Errors
    ///
    /// [`PeerError`] if the signal is not one, or the crate refuses it.
    pub async fn on_signal(
        &mut self,
        from: &str,
        data: &serde_json::Value,
    ) -> Result<Vec<Outbound>, PeerError> {
        let payload = Payload::from_value(data)
            .ok_or_else(|| PeerError::Malformed(format!("from {from}: {data}")))?;

        let state = PeerState {
            exists: self.holds(from),
            has_session: self.peers.get(from).is_some_and(|peer| peer.negotiated),
        };
        match route_signal(payload.route(), state) {
            // Nothing to apply it to, and nothing worth building for it: an answer or a
            // candidate for a connection this end does not have is from a connection the
            // other end has already given up on.
            SignalRoute::Drop => return Ok(Vec::new()),
            SignalRoute::Create => Box::pin(self.build(from)).await?,
            SignalRoute::Existing => {}
        }

        match payload {
            Payload::Offer { sdp, .. } => self.apply_offer(from, sdp).await,
            Payload::Answer { sdp } => {
                self.apply_remote(from, RTCSessionDescription::answer(sdp)?)
                    .await?;
                Ok(Vec::new())
            }
            Payload::Candidate(candidate) => {
                self.apply_candidate(from, candidate).await?;
                Ok(Vec::new())
            }
            // The other end wants a fresh offer. It gets one, marked as a renegotiation so
            // that it is applied to the connection rather than replacing it -- the
            // distinction that made a repair destroy what it was repairing.
            Payload::Renegotiate => Ok(vec![Box::pin(self.offer(from)).await?]),
        }
    }

    /// Applies an offer and answers it.
    async fn apply_offer(&mut self, from: &str, sdp: String) -> Result<Vec<Outbound>, PeerError> {
        self.apply_remote(from, RTCSessionDescription::offer(sdp)?)
            .await?;
        let connection = self.connection(from)?;
        let answer = connection.create_answer(None).await?;
        connection.set_local_description(answer.clone()).await?;
        Ok(vec![Outbound {
            to: from.to_owned(),
            payload: Payload::Answer { sdp: answer.sdp },
        }])
    }

    /// Sets a remote description and releases everything the queue was holding.
    async fn apply_remote(
        &mut self,
        from: &str,
        description: RTCSessionDescription,
    ) -> Result<(), PeerError> {
        let connection = self.connection(from)?;
        connection.set_remote_description(description).await?;
        let (connection, held) = {
            let peer = self
                .peers
                .get_mut(from)
                .ok_or_else(|| PeerError::Malformed(format!("no connection for {from}")))?;
            peer.negotiated = true;
            (Arc::clone(&peer.connection), peer.queue.flush())
        };
        for candidate in held {
            // A candidate that no longer applies is not fatal, and `peer.ts` says so in
            // the same place: a stale one is discarded rather than allowed to fail the
            // connection it arrived for.
            let _ = connection.add_ice_candidate(candidate).await;
        }
        Ok(())
    }

    /// Applies a candidate, or holds it until there is something to apply it to.
    async fn apply_candidate(
        &mut self,
        from: &str,
        candidate: serde_json::Value,
    ) -> Result<(), PeerError> {
        let init: RTCIceCandidateInit = serde_json::from_value(candidate)
            .map_err(|error| PeerError::Malformed(format!("a candidate from {from}: {error}")))?;
        let peer = self
            .peers
            .get_mut(from)
            .ok_or_else(|| PeerError::Malformed(format!("no connection for {from}")))?;
        let Some(now) = peer.queue.offer(init) else {
            return Ok(());
        };
        let connection = Arc::clone(&peer.connection);
        let _ = connection.add_ice_candidate(now).await;
        Ok(())
    }

    /// Builds a connection, replacing any that is there.
    ///
    /// Raising the generation before the old connection is dropped is what stops its
    /// still-running handler from speaking for the new one.
    async fn build(&mut self, peer: &str) -> Result<(), PeerError> {
        let generation = self
            .peers
            .get(peer)
            .map_or_else(Generation::first, |existing| existing.generation.next());
        let current = self.peers.get(peer).map_or_else(
            || Arc::new(AtomicU64::new(0)),
            |existing| Arc::clone(&existing.current),
        );
        current.store(generation.raw(), Ordering::Release);

        // `0.0.0.0:0` rather than a named interface: the client does not know which one a
        // player's traffic will leave by, and binding one is how a machine with a VPN or a
        // second adapter gathers candidates for the wrong path. The type parameter exists
        // because the builder is generic over address kinds; it is not a choice here.
        // Boxed, and not because clippy asked. The crate's builder future is eighteen
        // kilobytes; held across an await inside this state machine it is eighteen
        // kilobytes of every future that contains it, all the way up to the client's own
        // loop. `acl-net`'s loopback test allows the lint with a note saying the client's
        // hot paths are where it matters -- this is one of them.
        // The default media engine knows no codecs, and a track whose codec it has not
        // registered is refused with `ErrRTPTransceiverCodecUnsupported` -- at
        // `create_offer`, several steps after the mistake. Registering the defaults is what
        // puts Opus in it.
        let mut media = MediaEngine::default();
        media.register_default_codecs()?;

        // Built before the connection so it can be kept: `add_track` takes it by handle and
        // there is no way back out afterwards, and audio has to be written *into* it.
        let ssrc = microphone_ssrc(peer);
        let microphone = microphone_track(peer, generation)?;

        let connection = Box::pin(
            PeerConnectionBuilder::new()
                .with_udp_addrs(vec!["0.0.0.0:0"])
                .with_media_engine(media)
                .with_configuration(to_configuration(&self.config))
                .with_handler(Arc::new(Handler {
                    peer: peer.to_owned(),
                    generation,
                    current: Arc::clone(&current),
                    events: self.events.clone(),
                }))
                .build(),
        )
        .await?;

        // Before anything is offered or answered. A connection with no media negotiates no
        // ICE, which is the failure recorded at the top of this file.
        connection.add_track(microphone.clone()).await?;

        if let Some(previous) = self.peers.insert(
            peer.to_owned(),
            Peer {
                connection: Arc::new(connection),
                ssrc,
                microphone,
                generation,
                current,
                queue: CandidateQueue::new(),
                negotiated: false,
            },
        ) {
            // After the replacement is in place, so that nothing observes a gap. Its
            // handler is already stale by generation, so whatever it emits on the way down
            // is discarded.
            let _ = previous.connection.close().await;
        }
        Ok(())
    }

    /// Tears down a connection, if there is one.
    pub async fn close(&mut self, peer: &str) {
        if let Some(gone) = self.peers.remove(peer) {
            // Raised so that anything the connection emits while closing is stale, which
            // is the same protection `build` relies on.
            gone.current
                .store(gone.generation.next().raw(), Ordering::Release);
            let _ = gone.connection.close().await;
        }
    }

    /// Tears down everything.
    pub async fn close_all(&mut self) {
        let peers: Vec<String> = self.peers.keys().cloned().collect();
        for peer in peers {
            self.close(&peer).await;
        }
    }

    /// Everything the connections have reported since the last call.
    ///
    /// Never blocks. The candidates come back as [`Outbound`] to be sent through the
    /// signalling server; the state changes are for whoever is showing them; the audio is
    /// for whoever has a jitter buffer, which is not this crate.
    ///
    /// Audio is returned in arrival order and not sorted. Ordering is what a jitter buffer
    /// is *for*, and sorting here would be a second, worse one that also throws away the
    /// arrival order the real one uses to measure delay.
    pub fn drain(&mut self) -> (Vec<Outbound>, Vec<PeerEvent>, Vec<Incoming>) {
        let mut outbound = Vec::new();
        let mut events = Vec::new();
        let mut audio = Vec::new();
        while let Ok(message) = self.inbox.try_recv() {
            match message {
                Inbound::Candidate {
                    peer,
                    generation,
                    init,
                } => {
                    // Checked again here and not only in the handler. Between the handler
                    // deciding it was current and this reading the queue, the connection
                    // may have been replaced -- the message is in flight, not in the past.
                    if !self.is_live(&peer, generation) {
                        continue;
                    }
                    if let Ok(candidate) = serde_json::to_value(&init) {
                        outbound.push(Outbound {
                            to: peer,
                            payload: Payload::Candidate(candidate),
                        });
                    }
                }
                Inbound::State {
                    peer,
                    generation,
                    state,
                } => {
                    if !self.is_live(&peer, generation) {
                        continue;
                    }
                    events.push(PeerEvent::StateChanged { peer, state });
                }
                Inbound::Audio(incoming) => {
                    // No generation check: the task that produced it stops as soon as its
                    // generation is stale, and a packet already in flight from a connection
                    // that has just been replaced is still audio that peer said. Dropping
                    // it would put a gap in the stream at exactly the moment a repaired
                    // connection is trying not to have one.
                    audio.push(incoming);
                }
            }
        }
        (outbound, events, audio)
    }

    /// Everybody this set holds a connection to.
    ///
    /// For the sender, which has one packet and needs to give it to each of them: who can
    /// actually *hear* it is the receiver's decision, applied where the audio is played.
    #[must_use]
    pub fn peers(&self) -> Vec<String> {
        self.peers.keys().cloned().collect()
    }

    /// Sends one Opus packet to a peer.
    ///
    /// The packet is `acl_audio::codec::Encoder`'s output, whole. This adds the RTP around
    /// it and nothing else -- no encoding, no packing, no opinion about silence.
    ///
    /// `Ok(false)` when there is no connection to that peer, which is ordinary: a player
    /// who has left is a player the mixer has not stopped calling about yet.
    ///
    /// # Errors
    ///
    /// [`PeerError`] if the track refuses the sample.
    pub async fn send_audio(
        &self,
        peer: &str,
        packet: &[u8],
        duration: std::time::Duration,
    ) -> Result<bool, PeerError> {
        let Some((track, ssrc)) = self
            .peers
            .get(peer)
            .map(|peer| (Arc::clone(&peer.microphone), peer.ssrc))
        else {
            return Ok(false);
        };
        let sample = rtc::media::Sample {
            data: bytes::Bytes::copy_from_slice(packet),
            duration,
            ..Default::default()
        };
        track
            .write_sample(ssrc, OPUS_PAYLOAD_TYPE, &sample, &[])
            .await?;
        Ok(true)
    }

    /// Whether a message still speaks for the connection this set holds.
    fn is_live(&self, peer: &str, generation: Generation) -> bool {
        self.peers
            .get(peer)
            .is_some_and(|held| held.generation == generation)
    }

    /// The connection for a peer, cloned so the borrow ends.
    fn connection(&self, peer: &str) -> Result<Arc<dyn PeerConnection>, PeerError> {
        self.peers
            .get(peer)
            .map(|held| Arc::clone(&held.connection))
            .ok_or_else(|| PeerError::Malformed(format!("no connection for {peer}")))
    }
}
