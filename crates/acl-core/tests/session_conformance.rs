#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! Two Rust clients meeting in a lobby on a real server.
//!
//! `acl-net`'s own conformance test drives one connection against the server and checks the
//! protocol. This drives two `Session`s and checks the thing a single connection cannot
//! show: that the driver turns the server's events into a correct picture of who is in the
//! lobby, and that a signal sent by one arrives at the other.
//!
//! Nothing here is a second opinion about the protocol. It is a first opinion about the
//! translation, which is all `acl_core::session` is.
//!
//! # Running it
//!
//! ```text
//! ACL_SERVER_BIN=../ACL-Server/target/debug/acl-server cargo test -p acl-core
//! ```
//!
//! Without that variable it skips, loudly. The same rule as `acl-net`'s: a test that
//! quietly passes having checked nothing is worse than one that is not there.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use acl_core::session::{Event, Session};
use serde_json::json;

/// A port unlikely to collide with a server somebody is running for real, and its own —
/// tests in a binary run concurrently, and a shared port fails on a race rather than on
/// anything it was written to check.
const PORT: u16 = 19_740;

/// How long to wait for something the server should send.
///
/// Generous rather than tight, for the reason `acl-net`'s harness gives: a deadline that
/// is merely usually enough produces a test that usually passes.
const PATIENCE: Duration = Duration::from_secs(30);

/// A code no Among Us game can produce: six characters is the right shape, and digits are
/// not in the alphabet the game draws from.
const LOBBY: &str = "SESS01";

/// Kills the server however the test ends, including on a panic.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn server_binary() -> Option<PathBuf> {
    let named = std::env::var("ACL_SERVER_BIN").ok()?;
    let path = PathBuf::from(&named);
    if path.exists() {
        return Some(path);
    }
    let from_workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(&named);
    from_workspace.exists().then_some(from_workspace)
}

fn start(port: u16) -> Option<Server> {
    let child = Command::new(server_binary()?)
        .env("PORT", port.to_string())
        .env("BIND", "127.0.0.1")
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    Some(Server(child))
}

async fn wait_for_health(port: u16) -> bool {
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// Collects events until one matches, reporting what arrived instead if none does.
async fn until<F>(session: &mut Session, mut wanted: F) -> Result<Event, Vec<String>>
where
    F: FnMut(&Event) -> bool,
{
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while tokio::time::Instant::now() < deadline {
        let Some(events) = session.next().await else {
            seen.push("<session ended>".to_owned());
            return Err(seen);
        };
        for event in events {
            if wanted(&event) {
                return Ok(event);
            }
            seen.push(format!("{event:?}"));
        }
    }
    seen.push("<timed out>".to_owned());
    Err(seen)
}

#[tokio::test]
async fn two_sessions_see_each_other_and_a_signal_crosses() {
    let Some(server) = start(PORT) else {
        eprintln!(
            "SKIPPED: set ACL_SERVER_BIN to a built acl-server binary to run this against a \
             real server"
        );
        return;
    };
    assert!(wait_for_health(PORT).await, "the server never listened");

    let url = format!("http://127.0.0.1:{PORT}");
    let mut first = Session::connect(&url).await.expect("the first connects");
    let mut second = Session::connect(&url).await.expect("the second connects");

    // The socket id arrives with the Socket.IO CONNECT, before anything is joined. It is
    // also what the other end will address a signal to, so both are needed before either
    // can be used.
    until(&mut first, |event| matches!(event, Event::Connected(_)))
        .await
        .expect("the first session is told its id");
    until(&mut second, |event| matches!(event, Event::Connected(_)))
        .await
        .expect("the second session is told its id");
    let first_id = first.socket_id().expect("an id").to_owned();
    let second_id = second.socket_id().expect("an id").to_owned();
    assert_ne!(first_id, second_id);

    first
        .join(LOBBY, 1, 901, true)
        .await
        .expect("the first joins");
    // Drained before the second joins, so the `setClients` for an empty lobby does not sit
    // in the way of the `join` that follows it.
    let _ = tokio::time::timeout(Duration::from_millis(500), first.next()).await;

    second
        .join(LOBBY, 2, 902, false)
        .await
        .expect("the second joins");

    // The first is told about the second by `join`; the second learns about the first from
    // the `setClients` it gets on arrival. Two different events, one conclusion, which is
    // the whole reason the driver produces `PeerJoined` for both.
    let seen_by_first = until(
        &mut first,
        |event| matches!(event, Event::PeerJoined { socket_id, .. } if *socket_id == second_id),
    )
    .await;
    assert!(
        seen_by_first.is_ok(),
        "the first was never told about the second; saw {:?}",
        seen_by_first.unwrap_err()
    );

    let seen_by_second = until(
        &mut second,
        |event| matches!(event, Event::PeerJoined { socket_id, .. } if *socket_id == first_id),
    )
    .await;
    assert!(
        seen_by_second.is_ok(),
        "the second was never told about the first; saw {:?}",
        seen_by_second.unwrap_err()
    );

    assert!(first.membership().knows(&second_id));
    assert!(second.membership().knows(&first_id));

    // And a signal across, which is the only thing the lobby exists to carry.
    first
        .signal(
            &second_id,
            json!({"type": "offer", "sdp": "v=0 conformance"}),
        )
        .await
        .expect("the signal goes out");
    let arrived = until(&mut second, |event| matches!(event, Event::Signal { .. })).await;
    match arrived {
        Ok(Event::Signal { from, data }) => {
            assert_eq!(from, first_id);
            assert_eq!(data["sdp"], "v=0 conformance");
        }
        Ok(other) => panic!("expected a signal, got {other:?}"),
        Err(seen) => panic!("no signal arrived; saw {seen:?}"),
    }

    // Leaving is this end's decision and the server confirms nothing, so the membership is
    // this end's to clear. A driver that waited for a `left` about itself would hold
    // connections to a lobby it is no longer in.
    second.leave().await.expect("the second leaves");
    assert!(second.membership().is_empty());

    let noticed = until(
        &mut first,
        |event| matches!(event, Event::PeerLeft { socket_id } if *socket_id == second_id),
    )
    .await;
    assert!(
        noticed.is_ok(),
        "the first was never told the second left; saw {:?}",
        noticed.unwrap_err()
    );

    drop(server);
}
