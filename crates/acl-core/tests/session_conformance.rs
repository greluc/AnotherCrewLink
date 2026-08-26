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
//! # Both sessions are driven together, and that is not tidiness
//!
//! A `Session` answers the server's heartbeat from inside `Session::next`, so a session
//! nobody is awaiting is a session that stops answering. The first version of this file
//! waited on one at a time and was disconnected for it on a CI runner: the first session
//! was dropped for a missed heartbeat while the test waited on the second, and the signal
//! it then sent arrived from a socket the server had already removed from the lobby —
//! refused, correctly, as coming from a stranger. Locally it passed every time, because
//! locally nothing waits long enough.
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

use acl_core::session::{Arrival, Event, Session};
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

/// The same, for the deployed-server check. A different code so that two runs against one
/// server cannot land in each other's lobby.
const PROBE: &str = "PROBE1";

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

/// Two sessions, driven together, with what each has said kept until somebody asks.
///
/// See the module documentation for why both rather than one at a time.
struct Pair {
    first: Session,
    second: Session,
    seen: [Vec<Event>; 2],
}

impl Pair {
    fn new(first: Session, second: Session) -> Self {
        Self {
            first,
            second,
            seen: [Vec::new(), Vec::new()],
        }
    }

    /// Waits for an event on one of them while driving both.
    ///
    /// `which` is 0 for the first and 1 for the second. On a timeout it reports what did
    /// arrive, because "the server should have answered" is a much weaker report than "it
    /// answered with these three things and not that one".
    async fn until<F>(&mut self, which: usize, mut wanted: F) -> Result<Event, Vec<String>>
    where
        F: FnMut(&Event) -> bool,
    {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            if let Some(at) = self.seen[which].iter().position(&mut wanted) {
                return Ok(self.seen[which].remove(at));
            }
            if tokio::time::Instant::now() >= deadline {
                let mut seen: Vec<String> = self.seen[which]
                    .iter()
                    .map(|event| format!("{event:?}"))
                    .collect();
                seen.push("<timed out>".to_owned());
                return Err(seen);
            }
            self.pump().await;
        }
    }

    /// Drives whichever session speaks first, and keeps what it said.
    async fn pump(&mut self) {
        tokio::select! {
            events = self.first.next() => self.seen[0].extend(events.unwrap_or_default()),
            events = self.second.next() => self.seen[1].extend(events.unwrap_or_default()),
        }
    }

    /// Runs both for a moment, so that anything in flight lands.
    async fn settle(&mut self, how_long: Duration) {
        let _ = tokio::time::timeout(how_long, async {
            loop {
                self.pump().await;
            }
        })
        .await;
    }
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
    let mut pair = Pair::new(
        Session::connect(&url).await.expect("the first connects"),
        Session::connect(&url).await.expect("the second connects"),
    );

    // The socket id arrives with the Socket.IO CONNECT, before anything is joined. It is
    // also what the other end addresses a signal to, so both are needed before either can
    // be used.
    pair.until(0, |event| matches!(event, Event::Connected(_)))
        .await
        .expect("the first session is told its id");
    pair.until(1, |event| matches!(event, Event::Connected(_)))
        .await
        .expect("the second session is told its id");
    let first_id = pair.first.socket_id().expect("an id").to_owned();
    let second_id = pair.second.socket_id().expect("an id").to_owned();
    assert_ne!(first_id, second_id);

    pair.first
        .join(LOBBY, 1, 901, true)
        .await
        .expect("the first joins");
    pair.settle(Duration::from_millis(500)).await;
    pair.second
        .join(LOBBY, 2, 902, false)
        .await
        .expect("the second joins");

    // The first is told about the second by `join`; the second learns about the first from
    // the `setClients` it gets on arrival. Two different events, one conclusion -- and
    // opposite sides of the offer, which is the part no unit test can show is right,
    // because it depends on which event a real server actually sends to whom.
    let seen_by_first = pair
        .until(0, |event| {
            matches!(event, Event::PeerJoined { socket_id, arrival, .. }
                if *socket_id == second_id && *arrival == Arrival::Newcomer)
        })
        .await;
    assert!(
        seen_by_first.is_ok(),
        "the first was never told about the second; saw {:?}",
        seen_by_first.unwrap_err()
    );

    let seen_by_second = pair
        .until(1, |event| {
            matches!(event, Event::PeerJoined { socket_id, arrival, .. }
                if *socket_id == first_id && *arrival == Arrival::Incumbent)
        })
        .await;
    assert!(
        seen_by_second.is_ok(),
        "the second was never told about the first, or was told to offer to them; saw {:?}",
        seen_by_second.unwrap_err()
    );

    assert!(pair.first.membership().knows(&second_id));
    assert!(pair.second.membership().knows(&first_id));

    // And a signal across, which is the only thing the lobby exists to carry.
    pair.first
        .signal(
            &second_id,
            json!({"type": "offer", "sdp": "v=0 conformance"}),
        )
        .await
        .expect("the signal goes out");
    let arrived = pair
        .until(1, |event| matches!(event, Event::Signal { .. }))
        .await;
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
    pair.second.leave().await.expect("the second leaves");
    assert!(pair.second.membership().is_empty());

    let noticed = pair
        .until(
            0,
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

/// The same handshake against a deployed server, over TLS.
///
/// Ignored, and pointed at a URL rather than hard-coded: it talks to somebody's production.
/// It is here because the local case cannot show one thing that matters —
/// `crates/acl-net/src/transport.rs` says in as many words that certificate verification
/// uses webpki's bundled roots rather than the operating system's store, and a plain
/// `http://127.0.0.1` never touches that code.
///
/// Deliberately gentle, the same way the server repository's own live probe is: two
/// sockets, a lobby code no Among Us game can produce, nothing published to anybody's
/// lobby browser, and gone in a couple of seconds.
///
/// ```text
/// ACL_LIVE_URL=https://aucl.greluc.me cargo test -p acl-core -- --ignored against_a_deployed
/// ```
#[tokio::test]
#[ignore = "talks to a deployed server; set ACL_LIVE_URL"]
async fn against_a_deployed_server() {
    let Ok(url) = std::env::var("ACL_LIVE_URL") else {
        panic!("set ACL_LIVE_URL to the server to check");
    };

    let mut pair = Pair::new(
        Session::connect(&url).await.expect("the first connects"),
        Session::connect(&url).await.expect("the second connects"),
    );
    pair.until(0, |event| matches!(event, Event::Connected(_)))
        .await
        .expect("an id for the first");
    pair.until(1, |event| matches!(event, Event::Connected(_)))
        .await
        .expect("an id for the second");
    let second_id = pair.second.socket_id().expect("an id").to_owned();

    // The relay offer, which is the one thing a deployed server has and a freshly started
    // one does not: a TURN credential minted for this session.
    let config = pair
        .until(0, |event| matches!(event, Event::PeerConfig(_)))
        .await;
    match config {
        Ok(Event::PeerConfig(config)) => eprintln!("peer config: {config:?}"),
        Ok(other) => panic!("expected a peer config, got {other:?}"),
        Err(seen) => panic!("no peer config arrived; saw {seen:?}"),
    }

    pair.first
        .join(PROBE, 1, 901, true)
        .await
        .expect("the first joins");
    pair.settle(Duration::from_millis(500)).await;
    pair.second
        .join(PROBE, 2, 902, false)
        .await
        .expect("the second joins");

    let seen = pair
        .until(
            0,
            |event| matches!(event, Event::PeerJoined { socket_id, .. } if *socket_id == second_id),
        )
        .await;
    assert!(
        seen.is_ok(),
        "the deployed server never reported the second client; saw {:?}",
        seen.unwrap_err()
    );

    pair.first.leave().await.expect("the first leaves");
    pair.second.leave().await.expect("the second leaves");
}
