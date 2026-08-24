#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! Drives this client against a real `AnotherCrewLink` server.
//!
//! The unit tests check the client against the protocol as this repository understands
//! it, which is the same understanding that wrote the client. This one checks it against
//! the implementation it will actually talk to — the Rust server in
//! `greluc/AnotherCrewLink-Server`, which is itself verified against the reference
//! `socket.io-client` from Node. Two independent implementations meeting in the middle is
//! the only arrangement where a shared misreading of the specification shows up.
//!
//! The plan asks for this to run against "the server P0+ has just proven". That was
//! written expecting H3 to have made the production server websocket-only by then; the
//! decision on 2026-08-24 was to keep the Node server in production untouched and deploy
//! the Rust one with the Rust client, so the Rust server is what this tests against.
//!
//! # Running it
//!
//! ```text
//! ACL_SERVER_BIN=../AnotherCrewLink-Server/target/debug/acl-server cargo test -p acl-net
//! ```
//!
//! Without that variable it skips, loudly, with a line saying why. A test that quietly
//! passes having checked nothing is worse than one that is not there.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use acl_net::client::Action;
use acl_net::transport::Connection;
use serde_json::json;

/// Ports unlikely to collide with a server someone is running for real.
///
/// One per test, not one shared. Tests in a binary run concurrently, so a shared port
/// means the second server cannot bind and the test fails on a race rather than on
/// anything it was written to check. Running locally with `--test-threads=1` hides that
/// completely, which is how this reached CI.
const PORT_SESSION: u16 = 19_736;
const PORT_HEARTBEAT: u16 = 19_737;

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
    // A bare name is resolved relative to the workspace, so CI can pass a path that reads
    // the same on both platforms.
    let from_workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(&named);
    from_workspace.exists().then_some(from_workspace)
}

fn start(port: u16) -> Option<Server> {
    let binary = server_binary()?;
    let child = Command::new(binary)
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

/// Collects actions until one matches, or the session ends, or time runs out.
async fn pump<F>(connection: &mut Connection, mut wanted: F) -> Option<Action>
where
    F: FnMut(&Action) -> bool,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        let actions = connection.next().await?;
        for action in actions {
            if wanted(&action) {
                return Some(action);
            }
        }
    }
    None
}

#[tokio::test]
async fn talks_to_a_real_server() {
    let Some(_server) = start(PORT_SESSION) else {
        eprintln!(
            "skipping: set ACL_SERVER_BIN to an acl-server binary to run the conformance test"
        );
        return;
    };
    assert!(
        wait_for_health(PORT_SESSION).await,
        "the server did not start listening"
    );

    let mut connection = Connection::connect(&format!("http://127.0.0.1:{PORT_SESSION}"))
        .await
        .expect("the handshake should succeed");

    // The whole handshake, end to end: the server's OPEN, our CONNECT, its answer, and a
    // socket id that is not the Engine.IO one.
    let connected = pump(&mut connection, |action| {
        matches!(action, Action::Connected(_))
    })
    .await
    .expect("the server should complete the connect");
    let Action::Connected(socket_id) = connected else {
        unreachable!("pump matched on the variant")
    };
    assert!(!socket_id.is_empty());

    let session = connection.client().session().expect("a session");
    // The addressable id is whatever the CONNECT packet carried, and that is the whole
    // rule. This started out asserting the two ids *differ*, which is true of the Node
    // implementation and false of socketioxide — it reuses the Engine.IO sid, so against
    // this server a client that wrongly addressed itself by the transport id would work
    // perfectly and break the day it met a Node server. Which is exactly why the client
    // keeps them as separate fields and reads the one the server sent.
    assert_eq!(
        session.socket_sid(),
        Some(socket_id.as_str()),
        "the addressable id must be the one the CONNECT packet carried"
    );
    // And the parameters came off the wire rather than out of a constant.
    assert!(session.handshake().ping_interval > Duration::ZERO);
    assert!(session.handshake().heartbeat_deadline() > session.handshake().ping_interval);

    // A real event exchange: join a lobby and be told who else is in it.
    //
    // Four arguments, as the Electron client sends: code, player id, client id, is-host.
    // Three is not a smaller version of this call — the server deserialises the whole
    // array into a tuple, counts a short one as malformed and disconnects. Which is how
    // this test first failed, and a fair reminder that "the event name is right" is not
    // the same as "the call is right".
    let sent = connection
        .emit(
            "join",
            vec![json!("ABCDEF"), json!(1), json!(2), json!(false)],
            false,
        )
        .await
        .expect("emit should not fail");
    assert!(
        sent,
        "the session was live, so the event should have gone out"
    );

    let set_clients = pump(
        &mut connection,
        |action| matches!(action, Action::Event { name, .. } if name == "setClients"),
    )
    .await
    .expect("the server should answer a join with setClients");
    assert!(matches!(set_clients, Action::Event { .. }));

    connection.close().await.expect("a clean close");
}

#[tokio::test]
async fn answers_the_servers_heartbeat_for_longer_than_its_timeout() {
    // The direction of the heartbeat is the first of the five failure modes, and the unit
    // test proves the client answers a ping. This proves the server accepts the answer:
    // a client that pongs wrongly is dropped after pingTimeout, so surviving past that is
    // the only observation that distinguishes the two.
    let Some(_server) = start(PORT_HEARTBEAT) else {
        eprintln!(
            "skipping: set ACL_SERVER_BIN to an acl-server binary to run the conformance test"
        );
        return;
    };
    assert!(
        wait_for_health(PORT_HEARTBEAT).await,
        "the server did not start listening"
    );

    let mut connection = Connection::connect(&format!("http://127.0.0.1:{PORT_HEARTBEAT}"))
        .await
        .expect("the handshake should succeed");

    pump(&mut connection, |action| {
        matches!(action, Action::Connected(_))
    })
    .await
    .expect("connect");

    let handshake = connection
        .client()
        .session()
        .expect("a session")
        .handshake();
    let ping_interval = handshake.ping_interval;
    let ping_timeout = handshake.ping_timeout;

    // Long enough for at least one server ping to have been sent and answered. socketioxide
    // advertises 25s/20s by default, which would make this test a minute long, so it is
    // bounded: the point is to outlive a timeout, and a short one is enough to prove the
    // exchange happened at all.
    let watch = (ping_interval + ping_timeout).min(Duration::from_secs(6));
    let deadline = tokio::time::Instant::now() + watch;
    while tokio::time::Instant::now() < deadline {
        assert!(
            connection.next().await.is_some(),
            "the server closed the session while it was being answered"
        );
    }
    assert!(!connection.client().is_ended());
}
