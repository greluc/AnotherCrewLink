//! P4+ experiment 3: does the pinned `webrtc` crate actually connect?
//!
//! §4.6 item 1 budgets three weeks to prove `webrtc` `=0.20.3` against a real 1.0.2
//! Chromium client. Gate G3, which that spike fed, was struck on 2026-08-25 — but the
//! crate still has to work, and most of what the spike was for does not need a Chromium
//! peer at all. Three of its four questions are answerable from one process:
//!
//! 1. **What does the dependency cost, and does it collide?** Answered from the lockfile
//!    rather than here; see `experiments/README.md`.
//! 2. **Does a connection establish at all** — offer, answer, and candidates trickled in
//!    both directions after the descriptions are set, which is the shape the signalling
//!    server imposes and the shape `peer.ts` uses today.
//! 3. **Does a data channel open and carry a message**, which is what the mesh needs
//!    before any of the audio work matters.
//!
//! The fourth — does Chromium agree — is the one that needed the rig, and it is the one
//! nothing here can answer.
//!
//! # What this deliberately does not prove
//!
//! Both ends are this same crate, on loopback, with no relay and no NAT. A connection
//! that works here can still fail against Chromium, through coturn, or across a NAT. The
//! probe answers "is the crate usable", not "is the client interoperable"; with G3 struck
//! there is now nothing in the plan that answers the second, and that is a known gap
//! rather than an oversight.
//!
//! # Why the candidates are trickled rather than waited for
//!
//! Gathering everything before sending the offer would connect too, and would prove less.
//! `peer.ts` trickles, the server relays candidates as they come, and §4.6's
//! `trickle_candidate_without_type_is_forwarded` exists because that path was broken once
//! already. So the probe uses the path the client uses.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder,
    RTCIceCandidateInit, RTCPeerConnectionIceEvent, RTCPeerConnectionState,
};

/// Nothing here should take this long. A probe that hangs reports nothing at all, which
/// is worse than a probe that fails.
const PATIENCE: Duration = Duration::from_secs(20);

/// What one end reports back to the test body.
enum Event {
    /// A candidate this end gathered, ready to be handed to the other.
    Candidate(RTCIceCandidateInit),
    /// The connection state changed.
    State(RTCPeerConnectionState),
    /// The far end opened a data channel and this end was told about it.
    Channel(Arc<dyn DataChannel>),
}

/// Forwards the three events the probe cares about and ignores the rest.
///
/// The whole handler is one `Arc<dyn PeerConnectionEventHandler>` with no way to detach a
/// single event, which is the shape §4.6 item 2 warns about: `peer.ts` nulls all five of
/// its handlers before closing, and that teardown is how the 1.0.0 fixes avoid acting on
/// events from a connection being replaced. A probe does not need the generation counter
/// that replaces it, but it is worth seeing that the API really does force one.
struct Handler {
    name: &'static str,
    events: UnboundedSender<Event>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        match event.candidate.to_json() {
            Ok(init) => {
                println!("  {} gathered {}", self.name, init.candidate);
                let _ = self.events.send(Event::Candidate(init));
            }
            Err(error) => println!("  {} could not serialise a candidate: {error}", self.name),
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        println!("  {} is {state}", self.name);
        let _ = self.events.send(Event::State(state));
    }

    async fn on_data_channel(&self, data_channel: Arc<dyn DataChannel>) {
        println!("  {} was offered a data channel", self.name);
        let _ = self.events.send(Event::Channel(data_channel));
    }
}

/// Builds one end, bound to loopback so the probe needs no network and no permission.
async fn peer(
    name: &'static str,
) -> Result<(Arc<dyn PeerConnection>, UnboundedReceiver<Event>), Box<dyn std::error::Error>> {
    let (events, inbox) = unbounded_channel();
    // No ICE servers. A STUN server would add a reflexive candidate that says nothing on
    // loopback, and reaching for one would make the probe depend on the internet.
    let configuration = RTCConfigurationBuilder::default().build();
    let connection = PeerConnectionBuilder::new()
        .with_configuration(configuration)
        .with_handler(Arc::new(Handler { name, events }))
        .with_udp_addrs(vec!["127.0.0.1:0"])
        .build()
        .await?;
    // `build` returns `impl PeerConnection`, not a trait object. Both ends want the same
    // type so they can be passed to one function, so it is erased here rather than
    // threaded through as a generic.
    Ok((Arc::new(connection), inbox))
}

/// Drains whatever either end has gathered so far into the other end.
///
/// Candidates arriving before the remote description is set are refused by the crate, so
/// this runs after both descriptions are in place — the same ordering the client's
/// candidate queue exists to provide.
async fn trickle(
    from: &mut UnboundedReceiver<Event>,
    into: &Arc<dyn PeerConnection>,
    connected: &mut bool,
    channel: &mut Option<Arc<dyn DataChannel>>,
) {
    while let Ok(event) = from.try_recv() {
        match event {
            Event::Candidate(candidate) => {
                if let Err(error) = into.add_ice_candidate(candidate).await {
                    println!("  a candidate was refused: {error}");
                }
            }
            Event::State(RTCPeerConnectionState::Connected) => *connected = true,
            Event::State(_) => {}
            Event::Channel(opened) => *channel = Some(opened),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("webrtc =0.20.3, two peers on loopback\n");

    let (offerer, mut from_offerer) = peer("offerer").await?;
    let (answerer, mut from_answerer) = peer("answerer").await?;

    // The offerer opens the channel, as the client's initiator does.
    let sender = offerer.create_data_channel("probe", None).await?;

    let offer = offerer.create_offer(None).await?;
    offerer.set_local_description(offer.clone()).await?;
    answerer.set_remote_description(offer).await?;

    let answer = answerer.create_answer(None).await?;
    answerer.set_local_description(answer.clone()).await?;
    offerer.set_remote_description(answer).await?;

    println!("\ndescriptions exchanged; trickling\n");

    let mut offerer_connected = false;
    let mut answerer_connected = false;
    let mut received_channel = None;

    let settled = tokio::time::timeout(PATIENCE, async {
        loop {
            trickle(
                &mut from_offerer,
                &answerer,
                &mut offerer_connected,
                &mut None,
            )
            .await;
            trickle(
                &mut from_answerer,
                &offerer,
                &mut answerer_connected,
                &mut received_channel,
            )
            .await;
            if offerer_connected && answerer_connected && received_channel.is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;

    if settled.is_err() {
        println!("\nFAILED: nothing settled within {PATIENCE:?}");
        println!(
            "  offerer connected: {offerer_connected}, answerer connected: {answerer_connected}, channel: {}",
            received_channel.is_some()
        );
        std::process::exit(1);
    }

    println!("\nboth ends connected; sending\n");

    let receiver = received_channel.ok_or("the answerer was never offered a channel")?;
    sender.send_text("phase four is reachable").await?;

    let delivered = tokio::time::timeout(PATIENCE, async {
        while let Some(event) = receiver.poll().await {
            if let DataChannelEvent::OnMessage(message) = event {
                return String::from_utf8(message.data.to_vec()).ok();
            }
        }
        None
    })
    .await;

    match delivered {
        Ok(Some(text)) => {
            println!("  the answerer received: {text:?}");
            println!("\nPASSED: offer, answer, trickle both ways, data channel, one message");
        }
        Ok(None) => {
            println!("\nFAILED: the channel closed without delivering anything");
            std::process::exit(1);
        }
        Err(_) => {
            println!("\nFAILED: the message did not arrive within {PATIENCE:?}");
            std::process::exit(1);
        }
    }

    offerer.close().await?;
    answerer.close().await?;
    Ok(())
}
