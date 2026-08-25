//! Two peers, built the way the client will build them, connecting to each other.
//!
//! `experiments/webrtc-probe` proves the crate works. This proves *this crate's* answers
//! drive it: the configuration comes from [`acl_net::ice::RtcConfig`] through
//! [`acl_net::rtc::to_configuration`], the candidates go through
//! [`acl_net::peer::CandidateQueue`], and the handler is gated on
//! [`acl_net::peer::Generation`] because the `webrtc` crate gives no way to detach one.
//!
//! It is not gate G3 and it is not a substitute for it. Both ends are the same crate on
//! loopback; G3 was what would have proved a 1.0.2 Chromium client on the other side, and
//! it was struck on 2026-08-25. What this catches is a decision layer that compiles,
//! passes its unit tests, and then does not actually work when something calls it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
// The crate's builder and connection futures are 18 KB each. Boxing them inside a test
// would move an allocation nobody measures; where it matters is the client's own hot
// paths, and this is not one.
#![allow(clippy::large_futures)]

use std::sync::Arc;
use std::time::Duration;

use acl_net::ice::RtcConfig;
use acl_net::peer::{Attempt, CandidateQueue, Generation, Phase, Progress, is_current};
use acl_net::rtc::to_configuration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceCandidateInit,
    RTCPeerConnectionIceEvent, RTCPeerConnectionState,
};

/// Long enough for a loopback connection, short enough that a hang is a failure rather
/// than a hung suite.
const PATIENCE: Duration = Duration::from_secs(20);

enum Event {
    Candidate(RTCIceCandidateInit),
    State(RTCPeerConnectionState),
    Channel(Arc<dyn DataChannel>),
}

/// Tags every event with the generation it was built for, and drops the stale ones.
///
/// This is the pattern §4.6 item 2 says replaces `peer.ts`'s "null all five handlers
/// before closing": the crate takes one `Arc<dyn PeerConnectionEventHandler>` and offers
/// no way to detach it, so the handler has to know whether it still speaks for anything.
struct Handler {
    generation: Generation,
    current: Generation,
    events: UnboundedSender<Event>,
}

impl Handler {
    fn forward(&self, event: Event) {
        if !is_current(self.generation, self.current) {
            return;
        }
        let _ = self.events.send(event);
    }
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        if let Ok(init) = event.candidate.to_json() {
            self.forward(Event::Candidate(init));
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        self.forward(Event::State(state));
    }

    async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
        self.forward(Event::Channel(channel));
    }
}

async fn build(generation: Generation) -> (Arc<dyn PeerConnection>, UnboundedReceiver<Event>) {
    let (events, inbox) = unbounded_channel();
    // No ICE servers: a STUN server says nothing on loopback and would make the test
    // depend on the internet. The configuration still goes through this crate's type, so
    // the bundle policy and the transport policy are the ones the client would use.
    let configuration = to_configuration(&RtcConfig::new(&[], false));
    let connection = PeerConnectionBuilder::new()
        .with_configuration(configuration)
        .with_handler(Arc::new(Handler {
            generation,
            current: generation,
            events,
        }))
        .with_udp_addrs(vec!["127.0.0.1:0"])
        .build()
        .await
        .expect("a peer connection on loopback");
    (Arc::new(connection), inbox)
}

/// Offer, answer and the queued candidates, in the order the signalling server imposes.
///
/// Returns how many candidates the offerer had to hold, which is what proves the queue
/// was exercised rather than carried.
async fn negotiate(
    offerer: &Arc<dyn PeerConnection>,
    answerer: &Arc<dyn PeerConnection>,
    from_offerer: &mut UnboundedReceiver<Event>,
    to_answerer: &mut CandidateQueue<RTCIceCandidateInit>,
    to_offerer: &mut CandidateQueue<RTCIceCandidateInit>,
    offerer_attempt: &mut Attempt,
) -> usize {
    let offer = offerer.create_offer(None).await.expect("an offer");
    offerer
        .set_local_description(offer.clone())
        .await
        .expect("the local description");

    // Wait for the offerer to gather something before the answer is applied.
    //
    // Not padding. In the client an answer travels to the signalling server and back, and
    // candidates accumulate for the whole of that round trip -- which is why the queue
    // exists. In this process the answer is available immediately, so without this the
    // remote description would be set before the first candidate existed and the queue
    // would never hold anything. Measured rather than assumed: the first version of this
    // test asserted the queue was exercised and failed, which is how the difference was
    // noticed.
    let held_before_the_answer = tokio::time::timeout(PATIENCE, async {
        loop {
            let held = drain(from_offerer, to_answerer, offerer_attempt);
            if held > 0 {
                return held;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the offerer gathered at least one candidate");

    answerer
        .set_remote_description(offer)
        .await
        .expect("the remote description");
    for candidate in to_answerer.flush() {
        answerer
            .add_ice_candidate(candidate)
            .await
            .expect("a queued candidate the answerer can now take");
    }

    let answer = answerer.create_answer(None).await.expect("an answer");
    answerer
        .set_local_description(answer.clone())
        .await
        .expect("the local description");
    offerer
        .set_remote_description(answer)
        .await
        .expect("the remote description");
    for candidate in to_offerer.flush() {
        offerer
            .add_ice_candidate(candidate)
            .await
            .expect("a queued candidate the offerer can now take");
    }

    held_before_the_answer
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_decision_layer_drives_a_real_connection() {
    let generation = Generation::first();
    let (offerer, mut from_offerer) = build(generation).await;
    let (answerer, mut from_answerer) = build(generation).await;

    let sender = offerer
        .create_data_channel("anothercrewlink", None)
        .await
        .expect("a data channel");

    // The queues are the point. Candidates gathered before the remote description is set
    // are refused by the crate, and the client's own peers therefore hold them.
    let mut to_answerer: CandidateQueue<RTCIceCandidateInit> = CandidateQueue::new();
    let mut to_offerer: CandidateQueue<RTCIceCandidateInit> = CandidateQueue::new();
    let mut offerer_attempt = Attempt::new();
    let mut answerer_attempt = Attempt::new();

    let held_before_the_answer = negotiate(
        &offerer,
        &answerer,
        &mut from_offerer,
        &mut to_answerer,
        &mut to_offerer,
        &mut offerer_attempt,
    )
    .await;

    let mut received = None;
    let settled = tokio::time::timeout(PATIENCE, async {
        loop {
            drain_into(
                &mut from_offerer,
                &answerer,
                &mut to_answerer,
                &mut offerer_attempt,
                &mut None,
            )
            .await;
            drain_into(
                &mut from_answerer,
                &offerer,
                &mut to_offerer,
                &mut answerer_attempt,
                &mut received,
            )
            .await;
            let both_up = matches!(offerer_attempt.phase(), Phase::Connected)
                && matches!(answerer_attempt.phase(), Phase::Connected);
            if both_up && received.is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;

    assert!(
        settled.is_ok(),
        "nothing settled in {PATIENCE:?}: offerer {:?}, answerer {:?}, channel {}",
        offerer_attempt.phase(),
        answerer_attempt.phase(),
        received.is_some()
    );

    // The queue was not decoration. If the crate ever starts accepting candidates before
    // the remote description, this is the assertion that notices — and the queue could
    // then be deleted rather than carried for a reason that expired.
    assert!(
        held_before_the_answer > 0,
        "no candidate was gathered before the answer, so the queue was never exercised"
    );

    assert_eq!(offerer_attempt.poll(Duration::ZERO), Progress::Connected);
    assert_eq!(answerer_attempt.poll(Duration::ZERO), Progress::Connected);

    let channel = received.expect("the answerer was offered the channel");
    sender
        .send_text("through the decision layer")
        .await
        .expect("a message");

    let delivered = tokio::time::timeout(PATIENCE, async {
        while let Some(event) = channel.poll().await {
            if let DataChannelEvent::OnMessage(message) = event {
                return String::from_utf8(message.data.to_vec()).ok();
            }
        }
        None
    })
    .await
    .expect("the message arrived in time");

    assert_eq!(delivered.as_deref(), Some("through the decision layer"));

    offerer.close().await.expect("a clean close");
    answerer.close().await.expect("a clean close");
}

/// Moves whatever is waiting into the queue, returning how many candidates were held.
fn drain(
    from: &mut UnboundedReceiver<Event>,
    queue: &mut CandidateQueue<RTCIceCandidateInit>,
    attempt: &mut Attempt,
) -> usize {
    let mut held = 0;
    while let Ok(event) = from.try_recv() {
        match event {
            Event::Candidate(candidate) => {
                if queue.offer(candidate).is_none() {
                    held += 1;
                }
            }
            Event::State(state) => note(attempt, state),
            Event::Channel(_) => {}
        }
    }
    held
}

/// The same, but applying anything the queue lets through.
async fn drain_into(
    from: &mut UnboundedReceiver<Event>,
    into: &Arc<dyn PeerConnection>,
    queue: &mut CandidateQueue<RTCIceCandidateInit>,
    attempt: &mut Attempt,
    channel: &mut Option<Arc<dyn DataChannel>>,
) {
    while let Ok(event) = from.try_recv() {
        match event {
            Event::Candidate(candidate) => {
                if let Some(ready) = queue.offer(candidate) {
                    // A refusal here is not fatal: a candidate for a path that is already
                    // decided is ordinary.
                    let _ = into.add_ice_candidate(ready).await;
                }
            }
            Event::State(state) => note(attempt, state),
            Event::Channel(opened) => *channel = Some(opened),
        }
    }
}

fn note(attempt: &mut Attempt, state: RTCPeerConnectionState) {
    match state {
        RTCPeerConnectionState::Connecting => attempt.started(),
        RTCPeerConnectionState::Connected => attempt.connected(),
        _ => {}
    }
}
