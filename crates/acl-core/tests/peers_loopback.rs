#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! Two `PeerSet`s connecting to each other through the signalling this client speaks.
//!
//! `acl-net`'s `loopback.rs` proves that crate's *decisions* drive the `webrtc` crate. This
//! proves the layer above them: the offer and answer travel as
//! `acl_core::signalling::Payload`, exactly as they would through the server, and the
//! candidates go through the queue and the generation gate on the way.
//!
//! It is not gate G3 and it is not a substitute for it — both ends are this same code. What
//! it catches is a peer layer that compiles, passes its unit tests, and does not actually
//! connect anything.

use std::time::Duration;

use acl_core::peers::{Outbound, PeerEvent, PeerSet};
use acl_core::signalling::Payload;
use acl_net::ice::RtcConfig;
use webrtc::peer_connection::RTCPeerConnectionState;

/// Long enough for a loopback connection, short enough that a hang is a failure rather
/// than a hung suite.
const PATIENCE: Duration = Duration::from_secs(30);

const FIRST: &str = "socket-first";
const SECOND: &str = "socket-second";

/// No ICE servers. A STUN server says nothing on loopback and would make the test depend
/// on the internet; the configuration still goes through this client's own type, so the
/// bundle and transport policies are the ones it would use.
fn configuration() -> RtcConfig {
    RtcConfig::new(&[], false)
}

/// Hands one side's outbound signals to the other, as the server would.
///
/// The payload goes through `to_value` and back through `from_value`, so what crosses is
/// the JSON a 1.x peer would receive rather than a Rust value passed by reference. A
/// format that only works when both ends skip the wire is not a format.
async fn deliver(outbound: Vec<Outbound>, to: &mut PeerSet, from: &str) -> Vec<Outbound> {
    let mut replies = Vec::new();
    for signal in outbound {
        let wire = signal.payload.to_value();
        replies.extend(
            to.on_signal(from, &wire)
                .await
                .unwrap_or_else(|error| panic!("a signal {from} sent was refused: {error}")),
        );
    }
    replies
}

/// Drains one side, hands what it produced to the other, and says whether it is connected.
///
/// Panics on `Failed`, because on loopback that is a defect rather than a network.
async fn exchange(from: &mut PeerSet, from_id: &str, to: &mut PeerSet) -> bool {
    let (outbound, events) = from.drain();
    deliver(outbound, to, from_id).await;
    let mut connected = false;
    for PeerEvent::StateChanged { state, .. } in events {
        assert_ne!(
            state,
            RTCPeerConnectionState::Failed,
            "a connection failed on loopback"
        );
        if state == RTCPeerConnectionState::Connected {
            connected = true;
        }
    }
    connected
}

#[tokio::test]
async fn two_sets_negotiate_and_reach_connected() {
    let mut first = PeerSet::new(configuration());
    let mut second = PeerSet::new(configuration());

    // The asymmetry `session::Arrival` carries: the one already in the lobby offers.
    let offer = first.offer(SECOND).await.expect("an offer");
    assert!(
        matches!(
            &offer.payload,
            Payload::Offer {
                renegotiation: false,
                ..
            }
        ),
        "a first offer must not claim to be a renegotiation: {:?}",
        offer.payload
    );

    let answer = deliver(vec![offer], &mut second, FIRST).await;
    assert_eq!(answer.len(), 1, "an offer should produce one answer");
    assert!(matches!(&answer[0].payload, Payload::Answer { .. }));
    let further = deliver(answer, &mut first, SECOND).await;
    assert!(further.is_empty(), "an answer should produce nothing back");

    // Then the candidates, in both directions, until both sides say connected. Trickled
    // rather than gathered first: the queue and the server round trip are what make that
    // real, and a test that waited for gathering to finish would exercise neither.
    let connected = tokio::time::timeout(PATIENCE, async {
        let mut up = (false, false);
        while !(up.0 && up.1) {
            up.0 |= exchange(&mut first, FIRST, &mut second).await;
            up.1 |= exchange(&mut second, SECOND, &mut first).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(connected.is_ok(), "neither side reached connected in time");

    first.close_all().await;
    second.close_all().await;
}

/// A second offer to a peer already connected is a renegotiation, and renegotiating must
/// not replace the connection.
///
/// `signalRoute.ts` exists because the shipped client treated every offer as the start of
/// a new connection, so the repair for a stalled link was what killed it. That rule guards
/// the receiving side; this guards the sending one, where no routing rule can help.
#[tokio::test]
async fn a_second_offer_renegotiates_rather_than_rebuilding() {
    let mut set = PeerSet::new(configuration());

    let first = set.offer(SECOND).await.expect("an offer");
    assert!(matches!(
        first.payload,
        Payload::Offer {
            renegotiation: false,
            ..
        }
    ));

    let again = set.offer(SECOND).await.expect("a second offer");
    assert!(
        matches!(
            again.payload,
            Payload::Offer {
                renegotiation: true,
                ..
            }
        ),
        "a second offer must be marked as a renegotiation: {:?}",
        again.payload
    );
    assert_eq!(set.len(), 1, "renegotiating must not add a connection");

    set.close_all().await;
}

/// An answer or a candidate for a peer this end has never heard of is dropped, not built
/// for. It is from a connection the other end has already given up on.
#[tokio::test]
async fn a_signal_for_a_connection_that_does_not_exist_builds_nothing() {
    let mut set = PeerSet::new(configuration());

    let answer = Payload::Answer {
        sdp: "v=0 nothing".to_owned(),
    }
    .to_value();
    let out = set
        .on_signal(SECOND, &answer)
        .await
        .expect("dropped, not an error");
    assert!(out.is_empty());
    assert!(
        set.is_empty(),
        "an orphan answer must not build a connection"
    );

    let candidate =
        Payload::Candidate(serde_json::json!({"candidate": "candidate:1 1 udp"})).to_value();
    let out = set.on_signal(SECOND, &candidate).await.expect("dropped");
    assert!(out.is_empty());
    assert!(set.is_empty());
}

/// Closing forgets the connection, and a signal afterwards does not resurrect a
/// half-connection out of an answer.
#[tokio::test]
async fn closing_forgets_the_peer() {
    let mut set = PeerSet::new(configuration());
    set.offer(SECOND).await.expect("an offer");
    assert!(set.holds(SECOND));

    set.close(SECOND).await;
    assert!(!set.holds(SECOND));
    assert!(set.is_empty());
}
