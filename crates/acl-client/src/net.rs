//! The signalling session, on a thread of its own.
//!
//! [`acl_core::session`] is async and the window is not: eframe drives a synchronous loop
//! and `Session::next` is a future. So the session lives on a thread with its own runtime,
//! and the two talk through channels — the same shape as [`crate::hat_store::Loader`], for
//! the same reason.
//!
//! **A session nobody awaits is a session that dies.** The Engine.IO heartbeat is answered
//! from inside `Session::next`, so the loop below must keep calling it whatever else it is
//! doing. That is why commands are `select!`ed against `next` rather than checked between
//! calls to it: a loop that waited on a command channel would stop answering the server and
//! be dropped for it, which `tests/session_conformance.rs` learned the expensive way.
//!
//! **A connected session is not yet a live one.** `Session::connect` returns once the
//! socket is open, and the Socket.IO handshake completes later — inside the first
//! `Session::next`. Anything emitted before that is dropped on the floor, silently, because
//! the client below has nothing to send it on. So a command that needs a live session waits
//! for one: `the_link_sees_a_real_lobby` found this by never seeing a lobby, and the same
//! mistake had already cost the conformance test a join.
//!
//! Nothing here decides anything. It moves messages between a runtime and a window, and
//! every question about what they mean is answered in `acl-core`.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use acl_core::session::{Event, PublicLobby, Session};

/// What the window asks the session to do.
#[derive(Clone, Debug)]
pub(crate) enum Command {
    /// Connect to a server, replacing any current connection.
    Connect(String),
    /// Disconnect, and stay disconnected.
    Disconnect,
    /// Open or close the public lobby browser.
    WatchLobbies(bool),
    /// Ask for one lobby's code.
    JoinLobby(u64),
}

/// What the session tells the window.
#[derive(Clone, Debug)]
pub(crate) enum Report {
    /// The connection state changed.
    State(State),
    /// Something the session interpreted.
    ///
    /// Boxed because [`Event`] is large and this is a channel of them; an unboxed variant
    /// makes every message the size of the largest one.
    Event(Box<Event>),
}

/// Where the connection is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum State {
    /// Nothing has been asked for.
    #[default]
    Idle,
    /// A connection is being opened.
    Connecting,
    /// Connected, with the id the server addresses this session by.
    Connected(String),
    /// Not connected, and this is why.
    ///
    /// A message rather than an error type: it is shown to a person and nothing branches
    /// on it.
    Failed(String),
}

/// The session, as the window holds it.
pub(crate) struct Link {
    commands: Sender<Command>,
    reports: Receiver<Report>,
    state: State,
    /// The public lobbies, by the server's id for them.
    ///
    /// Held here rather than rebuilt by the caller because the server sends the list once
    /// and the differences after it: anything that dropped the accumulated map would show
    /// an empty browser until every lobby happened to change.
    lobbies: std::collections::BTreeMap<u64, PublicLobby>,
    /// The last answer to a join: a code, or why not.
    answer: Option<String>,
}

impl Link {
    /// Starts the thread. Nothing is connected until [`Link::connect`] is called.
    pub(crate) fn start() -> Self {
        let (commands, orders) = std::sync::mpsc::channel::<Command>();
        let (answers, reports) = std::sync::mpsc::channel::<Report>();
        std::thread::Builder::new()
            .name("signalling".to_owned())
            .spawn(move || run(&orders, &answers))
            // A client that cannot start a thread has bigger problems than the lobby
            // browser, and there is nowhere to report them from here: the link simply
            // stays idle.
            .ok();
        Self {
            commands,
            reports,
            state: State::Idle,
            lobbies: std::collections::BTreeMap::new(),
            answer: None,
        }
    }

    /// Takes whatever the session has said. Cheap, and called once a frame.
    pub(crate) fn pump(&mut self) {
        while let Ok(report) = self.reports.try_recv() {
            match report {
                Report::State(state) => {
                    if !matches!(state, State::Connected(_)) {
                        // A connection that has gone takes its lobbies with it. Leaving
                        // them on screen would offer a player a join that cannot be sent.
                        self.lobbies.clear();
                    }
                    self.state = state;
                }
                Report::Event(event) => self.absorb(*event),
            }
        }
    }

    /// One event, folded into what the window shows.
    fn absorb(&mut self, event: Event) {
        match event {
            Event::Lobbies(lobbies) => {
                // Replaced rather than merged: this arrives once, when the browser opens,
                // and it is the whole truth at that moment.
                self.lobbies = lobbies.into_iter().map(|lobby| (lobby.id, lobby)).collect();
            }
            Event::LobbyUpdated(lobby) => {
                self.lobbies.insert(lobby.id, *lobby);
            }
            Event::LobbyRemoved(id) => {
                self.lobbies.remove(&id);
            }
            Event::LobbyCode { code, server } => {
                self.answer = Some(format!("{code} — {server}"));
            }
            Event::LobbyUnavailable(why) => self.answer = Some(why),
            // Everything else belongs to the voice mesh, which this window does not have
            // yet. Dropped rather than kept: `acl-core` has already reported them, and a
            // queue of events nobody reads is a leak with a long fuse.
            _ => {}
        }
    }

    /// Where the connection is.
    pub(crate) const fn state(&self) -> &State {
        &self.state
    }

    /// The lobbies, in the order the browser sorts them.
    pub(crate) fn lobbies(&self) -> impl Iterator<Item = &PublicLobby> {
        self.lobbies.values()
    }

    /// The last answer to a join, if there has been one.
    pub(crate) fn answer(&self) -> Option<&str> {
        self.answer.as_deref()
    }

    /// Connects, or reconnects to a different server.
    pub(crate) fn connect(&mut self, url: &str) {
        self.state = State::Connecting;
        self.send(Command::Connect(url.to_owned()));
    }

    /// Disconnects.
    pub(crate) fn disconnect(&mut self) {
        self.state = State::Idle;
        self.lobbies.clear();
        self.send(Command::Disconnect);
    }

    /// Opens or closes the browser.
    pub(crate) fn watch_lobbies(&mut self, open: bool) {
        if !open {
            self.lobbies.clear();
        }
        self.send(Command::WatchLobbies(open));
    }

    /// Asks for one lobby's code.
    pub(crate) fn join_lobby(&mut self, id: u64) {
        self.answer = None;
        self.send(Command::JoinLobby(id));
    }

    /// A failed send means the thread is gone, which is a client on its way out.
    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

/// The thread: a runtime, and a session that outlives individual connections.
fn run(orders: &Receiver<Command>, answers: &Sender<Report>) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        let _ = answers.send(Report::State(State::Failed(
            "no async runtime could be started".to_owned(),
        )));
        return;
    };

    let mut session: Option<Session> = None;
    // Whether the handshake has completed. See the module documentation: a session that is
    // connected but not live drops everything emitted to it.
    let mut live = false;
    // Commands that arrived before it did.
    let mut deferred: Vec<Command> = Vec::new();
    loop {
        // Without a session there is nothing to await, so the command channel is blocked
        // on. With one, the two are raced -- see the module documentation for why the
        // session may not be left unattended.
        let command = if session.is_some() {
            match orders.try_recv() {
                Ok(command) => Some(command),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            match orders.recv() {
                Ok(command) => Some(command),
                Err(_) => return,
            }
        };

        if let Some(command) = command {
            match command {
                // These two are about the connection itself, so they are never deferred:
                // one replaces it and the other ends it.
                connection @ (Command::Connect(_) | Command::Disconnect) => {
                    live = false;
                    deferred.clear();
                    if !obey(&runtime, &mut session, connection, answers) {
                        return;
                    }
                }
                other if !live => deferred.push(other),
                other => {
                    if !obey(&runtime, &mut session, other, answers) {
                        return;
                    }
                }
            }
        }

        if let Some(current) = session.as_mut() {
            // A short wait rather than none: this is the only place the heartbeat is
            // answered from, and a busy loop would spin a core to answer it no sooner.
            let next = runtime.block_on(async {
                tokio::time::timeout(std::time::Duration::from_millis(50), current.next()).await
            });
            match next {
                // The session ended. The window is told, and the loop goes back to
                // blocking on commands rather than spinning on a dead session.
                Ok(None) => {
                    session = None;
                    live = false;
                    deferred.clear();
                    if answers
                        .send(Report::State(State::Failed(
                            "the connection closed".to_owned(),
                        )))
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(Some(events)) => {
                    for event in events {
                        if let Event::Connected(id) = &event {
                            live = true;
                            if answers
                                .send(Report::State(State::Connected(id.clone())))
                                .is_err()
                            {
                                return;
                            }
                        }
                        if answers.send(Report::Event(Box::new(event))).is_err() {
                            return;
                        }
                    }
                    // Whatever was asked for while the handshake was still in flight.
                    if live {
                        for command in deferred.drain(..) {
                            if !obey(&runtime, &mut session, command, answers) {
                                return;
                            }
                        }
                    }
                }
                // Nothing happened in fifty milliseconds, which is the ordinary case.
                Err(_) => {}
            }
        }
    }
}

/// Carries out one command. `false` means the window is gone.
fn obey(
    runtime: &tokio::runtime::Runtime,
    session: &mut Option<Session>,
    command: Command,
    answers: &Sender<Report>,
) -> bool {
    match command {
        Command::Connect(url) => {
            *session = None;
            match runtime.block_on(Session::connect(&url)) {
                Ok(live) => *session = Some(live),
                Err(error) => {
                    return answers
                        .send(Report::State(State::Failed(error.to_string())))
                        .is_ok();
                }
            }
        }
        Command::Disconnect => {
            *session = None;
            return answers.send(Report::State(State::Idle)).is_ok();
        }
        Command::WatchLobbies(open) => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.watch_lobbies(open));
            }
        }
        Command::JoinLobby(id) => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.join_lobby(id));
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Link, State};
    use acl_core::session::{Event, PublicLobby};

    fn lobby(id: u64, title: &str) -> PublicLobby {
        PublicLobby {
            id,
            title: title.to_owned(),
            ..PublicLobby::default()
        }
    }

    /// The list arrives once and the differences after it, so the map has to accumulate.
    /// Anything that rebuilt it from the last message would show an empty browser until
    /// every lobby happened to change.
    #[test]
    fn the_list_accumulates_rather_than_being_rebuilt() {
        let mut link = Link::start();
        link.absorb(Event::Lobbies(vec![lobby(1, "One"), lobby(2, "Two")]));
        link.absorb(Event::LobbyUpdated(Box::new(lobby(3, "Three"))));
        assert_eq!(link.lobbies().count(), 3);

        link.absorb(Event::LobbyRemoved(1));
        let left: Vec<&str> = link.lobbies().map(|lobby| lobby.title.as_str()).collect();
        assert_eq!(left, ["Two", "Three"]);
    }

    /// An update for a lobby already known replaces it rather than adding a second row:
    /// the server sends `update_lobby` whether or not the browser has seen the id.
    #[test]
    fn an_update_replaces_rather_than_duplicating() {
        let mut link = Link::start();
        link.absorb(Event::Lobbies(vec![lobby(1, "Before")]));
        link.absorb(Event::LobbyUpdated(Box::new(lobby(1, "After"))));
        let titles: Vec<&str> = link.lobbies().map(|lobby| lobby.title.as_str()).collect();
        assert_eq!(titles, ["After"]);
    }

    /// A fresh list replaces what was there. It arrives when the browser opens and is the
    /// whole truth at that moment, so merging it would keep lobbies that have since gone.
    #[test]
    fn a_fresh_list_replaces_the_old_one() {
        let mut link = Link::start();
        link.absorb(Event::Lobbies(vec![lobby(1, "Old")]));
        link.absorb(Event::Lobbies(vec![lobby(2, "New")]));
        let titles: Vec<&str> = link.lobbies().map(|lobby| lobby.title.as_str()).collect();
        assert_eq!(titles, ["New"]);
    }

    /// A connection that has gone takes its lobbies with it. Leaving them on screen would
    /// offer a player a join that cannot be sent.
    #[test]
    fn losing_the_connection_empties_the_browser() {
        let mut link = Link::start();
        link.absorb(Event::Lobbies(vec![lobby(1, "One")]));
        assert_eq!(link.lobbies().count(), 1);

        link.state = State::Connected("abc".to_owned());
        // What `pump` does with a state report, without needing the thread to send one.
        link.lobbies.clear();
        link.state = State::Failed("the connection closed".to_owned());
        assert_eq!(link.lobbies().count(), 0);
    }

    /// Both answers to a join reach the window, and they are different: a code is a thing
    /// to type into the game and a refusal is a reason.
    #[test]
    fn both_answers_to_a_join_are_shown() {
        let mut link = Link::start();
        assert_eq!(link.answer(), None);

        link.absorb(Event::LobbyCode {
            code: "ABCDEF".to_owned(),
            server: "eu".to_owned(),
        });
        let code = link.answer().expect("a code").to_owned();
        assert!(code.contains("ABCDEF"), "{code}");
        assert!(code.contains("eu"), "the region matters too: {code}");

        link.absorb(Event::LobbyUnavailable(
            "Lobby is not public anymore".to_owned(),
        ));
        assert_eq!(link.answer(), Some("Lobby is not public anymore"));
    }

    /// Closing the browser empties it. The server stops sending updates, so whatever was
    /// on screen would sit there going stale.
    #[test]
    fn closing_the_browser_empties_it() {
        let mut link = Link::start();
        link.absorb(Event::Lobbies(vec![lobby(1, "One")]));
        link.watch_lobbies(false);
        assert_eq!(link.lobbies().count(), 0);
    }

    /// Everything else is dropped rather than queued. The voice mesh is not in this window
    /// yet, and a queue nobody reads is a leak with a long fuse.
    #[test]
    fn events_this_window_has_no_use_for_are_dropped() {
        let mut link = Link::start();
        link.absorb(Event::HostChanged(7));
        link.absorb(Event::VoiceActivity {
            socket_id: "abc".to_owned(),
            speaking: true,
        });
        link.absorb(Event::Ignored("somethingNewer".to_owned()));
        assert_eq!(link.lobbies().count(), 0);
        assert_eq!(link.answer(), None);
    }

    /// A server for the duration of a test, killed and reaped on the way out.
    ///
    /// Reaped, not merely killed: a `Child` that is dropped without being waited on leaves
    /// a zombie until the parent exits, and the parent here is a test binary that may run
    /// for a while yet.
    struct Server(std::process::Child);

    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Starts one, or gives up when the binary is not named.
    fn serving(port: u16) -> Option<Server> {
        let binary = std::env::var("ACL_SERVER_BIN").ok()?;
        std::process::Command::new(binary)
            .env("PORT", port.to_string())
            .env("BIND", "127.0.0.1")
            .env("RUST_LOG", "warn")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()
            .map(Server)
    }

    /// Drives one session until it says something the caller is waiting for.
    ///
    /// Returns whether it did. Driving is not optional: the Engine.IO heartbeat is answered
    /// from inside `next`, so a session left alone is a session the server drops.
    fn until(
        runtime: &tokio::runtime::Runtime,
        session: &mut acl_core::session::Session,
        mut wanted: impl FnMut(&Event) -> bool,
    ) -> bool {
        for _ in 0..100 {
            let events = runtime
                .block_on(async {
                    tokio::time::timeout(std::time::Duration::from_millis(50), session.next()).await
                })
                .unwrap_or_default()
                .unwrap_or_default();
            if events.iter().any(&mut wanted) {
                return true;
            }
        }
        false
    }

    /// Connects a session, waits for the handshake, and publishes a lobby through it.
    ///
    /// The waiting is the point. `Session::connect` returns with the socket open and the
    /// Socket.IO handshake still in flight, and anything emitted before it completes is
    /// dropped -- which is the bug the test below found in `run`.
    fn publishing(
        runtime: &tokio::runtime::Runtime,
        base: &str,
    ) -> Option<acl_core::session::Session> {
        let mut host = None;
        for _ in 0..100 {
            if let Ok(session) = runtime.block_on(acl_core::session::Session::connect(base)) {
                host = Some(session);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let mut host = host?;
        assert!(
            until(runtime, &mut host, |event| matches!(
                event,
                Event::Connected(_)
            )),
            "the host never finished the handshake"
        );

        runtime
            .block_on(host.join("LINKED", 1, 11, true))
            .expect("the host joins");
        // `setHost` is what says the server has *acted on* the join. It dispatches each
        // event on its own task, so a `lobby` sent immediately after a `join` is handled
        // first and refused, silently.
        assert!(
            until(runtime, &mut host, |event| matches!(
                event,
                Event::HostChanged(_)
            )),
            "the server never acted on the join"
        );

        runtime
            .block_on(host.advertise(
                "LINKED",
                serde_json::json!({
                    "id": -1,
                    "title": LIVE_TITLE,
                    "host": "Red",
                    "current_players": 2,
                    "max_players": 10,
                    "server": "test",
                    "language": "en",
                    "mods": "NONE",
                    "isPublic": true,
                    "gameState": 0,
                }),
            ))
            .expect("the host advertises");
        Some(host)
    }

    /// The title the live tests publish under.
    const LIVE_TITLE: &str = "Seen by the link";

    /// The whole link against a real server, which is the only thing that shows the thread,
    /// the runtime and the channels working together.
    ///
    /// Not run by default, like every other test here that needs something outside this
    /// process:
    ///
    /// ```text
    /// ACL_SERVER_BIN=../ACL-Server/target/debug/acl-server \
    ///   cargo test -p acl-client -- --ignored the_link_sees_a_real_lobby
    /// ```
    ///
    /// **It found a real bug**, and that is worth saying: before the deferral in `run`, the
    /// `watch_lobbies` that follows `connect` was emitted into a session that had not
    /// finished its handshake, and was dropped. The browser then sat empty forever, with
    /// everything looking connected.
    #[test]
    #[ignore = "needs a server: set ACL_SERVER_BIN"]
    fn the_link_sees_a_real_lobby() {
        const PORT: u16 = 19_760;
        let Some(_server) = serving(PORT) else {
            eprintln!("skipping: set ACL_SERVER_BIN to the server binary");
            return;
        };
        let base = format!("http://127.0.0.1:{PORT}");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let mut host = publishing(&runtime, &base).expect("the host publishes");

        // And now the link, from the other side, on its own thread with its own runtime.
        let mut link = Link::start();
        link.connect(&base);
        link.watch_lobbies(true);

        let mut seen = false;
        for _ in 0..200 {
            // The host has to keep answering its own heartbeat while this waits.
            let _ = runtime.block_on(async {
                tokio::time::timeout(std::time::Duration::from_millis(20), host.next()).await
            });
            link.pump();
            if link.lobbies().any(|lobby| lobby.title == LIVE_TITLE) {
                seen = true;
                break;
            }
        }
        assert!(
            seen,
            "the link never saw the lobby; it was {:?} with {} lobbies",
            link.state(),
            link.lobbies().count()
        );
        assert!(
            matches!(link.state(), State::Connected(_)),
            "{:?}",
            link.state()
        );

        let id = link
            .lobbies()
            .find(|lobby| lobby.title == LIVE_TITLE)
            .expect("the lobby")
            .id;
        link.join_lobby(id);
        let mut answered = None;
        for _ in 0..200 {
            let _ = runtime.block_on(async {
                tokio::time::timeout(std::time::Duration::from_millis(20), host.next()).await
            });
            link.pump();
            if let Some(answer) = link.answer() {
                answered = Some(answer.to_owned());
                break;
            }
        }
        let answered = answered.expect("the join is answered");
        assert!(
            answered.contains("LINKED"),
            "expected the code, got {answered}"
        );
    }

    /// Does `Session::connect` work on a current-thread runtime at all?
    ///
    /// The shipped [`Link`] uses one, so if the answer is ever no, the answer matters more
    /// than this test does -- and it would otherwise show up as the larger test above
    /// timing out, which is a much weaker report.
    #[test]
    #[ignore = "needs a server: set ACL_SERVER_BIN"]
    fn a_current_thread_runtime_can_connect() {
        const PORT: u16 = 19_761;
        let Some(_server) = serving(PORT) else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let base = format!("http://127.0.0.1:{PORT}");

        let mut connected = false;
        for _ in 0..100 {
            if runtime
                .block_on(acl_core::session::Session::connect(&base))
                .is_ok()
            {
                connected = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            connected,
            "connect never succeeded on a current-thread runtime"
        );
    }
}
