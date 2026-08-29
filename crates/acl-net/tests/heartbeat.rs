#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! The heartbeat deadline, against a server that stops answering.
//!
//! Separate from `conformance.rs`, which needs the real server binary and skips without
//! it. This one carries its own server -- twenty lines of `tokio-tungstenite` that
//! completes the Engine.IO handshake and then says nothing at all -- because the thing
//! being checked is what happens in the *absence* of traffic, and a real server would
//! keep pinging.
//!
//! # What it is for
//!
//! `Connection::next` used to run the client's timers behind `sleep(TICK)` inside its
//! `select!`. `acl-client`'s worker wraps every call in a fifty-millisecond timeout, so
//! that sleep was cancelled at fifty milliseconds and started again from zero on the next
//! call, for ever: `on_tick` was never reached once in the life of a connection.
//! Acknowledgements never expired, and a socket that had stopped answering was
//! indistinguishable from a quiet one -- the heartbeat existed to notice a dead connection
//! and was itself dead.
//!
//! The test therefore polls the way the client polls, with a timeout far shorter than
//! `TICK`. Against the old code it never finishes.

use std::time::Duration;

use acl_net::client::{Action, CloseReason};
use acl_net::transport::Connection;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// What the server promises. Small, so the test is over in a second rather than in the
/// forty-five a real deployment would take -- the deadline is the sum of the two.
const PING_INTERVAL_MS: u64 = 200;
const PING_TIMEOUT_MS: u64 = 200;

/// How long the client's worker gives one call to `next`.
///
/// Not a number invented here: it is what `acl-client`'s `run` loop passes, and the whole
/// failure was that it is much shorter than `TICK`.
const CALLER_TIMEOUT: Duration = Duration::from_millis(50);

/// Long enough for a deadline of 400 ms plus a tick of a second, short enough that a hang
/// fails the test rather than the suite.
const PATIENCE: Duration = Duration::from_secs(10);

/// A websocket server that finishes the handshake and then goes silent.
///
/// It answers nothing afterwards -- not the Socket.IO CONNECT, not a ping -- because a
/// server that has stopped answering is precisely the case the deadline exists for. It
/// holds the socket open, so nothing else tells the client anything is wrong.
async fn silent_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let port = listener.local_addr().expect("an address").port();
    tokio::spawn(async move {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
            return;
        };
        let open = format!(
            r#"0{{"sid":"silent","upgrades":[],"pingInterval":{PING_INTERVAL_MS},"pingTimeout":{PING_TIMEOUT_MS},"maxPayload":65536}}"#
        );
        if socket.send(Message::Text(open.into())).await.is_err() {
            return;
        }
        // Read and discard whatever the client sends -- the CONNECT, and any pong it
        // decides to make -- without ever replying. Dropping the read half instead would
        // close the socket, which is a different failure and one the client already
        // notices.
        while let Some(Ok(_)) = socket.next().await {}
    });
    port
}

#[tokio::test]
async fn a_server_that_stops_answering_is_noticed() {
    let port = silent_server().await;
    let mut connection = Connection::connect(&format!("http://127.0.0.1:{port}"))
        .await
        .expect("the handshake should succeed");

    let ended = tokio::time::timeout(PATIENCE, async {
        loop {
            // Exactly how `acl-client`'s worker calls it. A `next` that does not finish
            // inside the caller's timeout is cancelled and called again, and the timers
            // have to survive that -- which is the whole point.
            let polled = tokio::time::timeout(CALLER_TIMEOUT, connection.next()).await;
            match polled {
                // The session is over: `next` reports it honestly rather than hanging.
                Ok(None) => return None,
                Ok(Some(actions)) => {
                    for action in actions {
                        if let Action::Closed { reason } = action {
                            return Some(reason);
                        }
                    }
                }
                // The ordinary case: nothing happened in fifty milliseconds.
                Err(_) => {}
            }
        }
    })
    .await;

    let reason = ended.expect(
        "the deadline never fired: `next` is being cancelled before its timers run, which is \
         the bug this test exists for",
    );
    assert_eq!(
        reason,
        Some(CloseReason::HeartbeatMissed),
        "a silent server should end the session for a missed heartbeat, not for anything else"
    );
}

/// The deadline is not reset by being polled.
///
/// The sharper half of the same fault. A sleep restarted on every call is not merely late,
/// it never arrives at all, so this asserts the connection ends within a bound derived
/// from what the server promised rather than merely ending eventually.
#[tokio::test]
async fn the_deadline_is_measured_from_the_last_word_not_from_the_last_poll() {
    let port = silent_server().await;
    let started = std::time::Instant::now();
    let mut connection = Connection::connect(&format!("http://127.0.0.1:{port}"))
        .await
        .expect("the handshake should succeed");

    let mut closed = false;
    while !closed && started.elapsed() < PATIENCE {
        match tokio::time::timeout(CALLER_TIMEOUT, connection.next()).await {
            Ok(None) => closed = true,
            Ok(Some(actions)) => {
                closed = actions
                    .iter()
                    .any(|action| matches!(action, Action::Closed { .. }));
            }
            Err(_) => {}
        }
    }

    assert!(closed, "the session never ended");
    // The deadline is `pingInterval + pingTimeout`, and the timers run once a second, so
    // the whole thing is over inside a couple of seconds. Generous, because CI machines
    // are slow and this is a bound rather than a measurement -- what it rules out is the
    // old behaviour, which was unbounded.
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the session took {:?} to end, which is long enough to suspect the timers are \
         being restarted rather than resumed",
        started.elapsed()
    );
}
