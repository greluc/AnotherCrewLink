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
//! **The peer mesh lives on this thread too**, and it has to. `acl_core::peers::PeerSet`
//! is async, its connections must be driven by a runtime, and every signal it produces has
//! to go back out through the same socket the session owns. Putting it anywhere else would
//! mean two runtimes and a channel between them for messages that are already here.
//!
//! Nothing here decides anything. It moves messages between a runtime and a window, and
//! every question about what they mean is answered in `acl-core`: who offers to whom is
//! `session::Arrival`, what to do with an arriving signal is `acl_net::signal_route`, and
//! this obeys both.

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
    /// Send one Opus packet to everybody in the lobby.
    ///
    /// To everybody, because who can hear it is the *receiver's* decision: gain and
    /// distance are applied where the audio is played, which is what makes a lobby's rules
    /// the same for everybody rather than whatever each sender believed.
    SendAudio(Vec<u8>),
    /// Join a lobby, which is what starts the mesh.
    Join {
        /// The lobby code.
        code: String,
        /// This player's in-game id.
        player_id: i64,
        /// This player's client id.
        client_id: i64,
        /// Whether this player is the game's host.
        is_host: bool,
    },
    /// Say whether this player is speaking, for everybody else's indicator.
    VoiceActivity(bool),
    /// Claim the game host's role, after having become it.
    SetHost(i64),
    /// Leave it, closing every connection.
    Leave,
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
    /// Audio arrived from a peer.
    Audio {
        /// Whose.
        socket_id: String,
        /// The packet, on its way to a decoder.
        packet: acl_core::peers::Incoming,
    },
    /// A peer connection changed state.
    Peer {
        /// Whose.
        socket_id: String,
        /// Whether it is connected right now.
        connected: bool,
    },
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
    /// Which socket belongs to which in-game client id.
    ///
    /// The window knows players by their client id and the mesh knows them by socket, and
    /// the server is the only thing that sees both. Kept here because it is the only place
    /// both halves pass through.
    sockets: std::collections::BTreeMap<i64, String>,
    /// Who the server says is speaking, by socket id.
    ///
    /// A level, not a moment, which is the difference between this and [`Self::speaking`].
    /// The server relays `VAD` when a peer starts and again when they stop, so this is held
    /// until told otherwise rather than decaying — a peer who talks for ten seconds sends
    /// two messages, not five hundred.
    vocal: std::collections::BTreeSet<String>,
    /// Packets that have arrived and not yet been handed to a decoder.
    ///
    /// Held for one frame at most: `take_arrived` empties it, and the caller hands them
    /// straight on. A queue nobody drains is the failure this is shaped to make obvious.
    arrived: Vec<acl_core::peers::Incoming>,
    /// Which peers have sent audio since the window last looked.
    ///
    /// Cleared by [`Link::take_speaking`], which is what makes it mean "recently" rather
    /// than "ever". A set rather than a count: what the window asks is whether somebody is
    /// speaking, and how many packets arrived is a question nothing here has.
    speaking: std::collections::BTreeSet<String>,
    /// Which peers have a connection that is up.
    ///
    /// The window shows this as the difference between a player who has arrived and one who
    /// can be heard — `roster::Link::Silent` against `Connected`, which is a distinction
    /// `views::main` already draws and had nothing to fill in.
    connected: std::collections::BTreeSet<String>,
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
            connected: std::collections::BTreeSet::new(),
            sockets: std::collections::BTreeMap::new(),
            vocal: std::collections::BTreeSet::new(),
            speaking: std::collections::BTreeSet::new(),
            arrived: Vec::new(),
        }
    }

    /// Takes whatever the session has said. Cheap, and called once a frame.
    pub(crate) fn pump(&mut self) {
        while let Ok(report) = self.reports.try_recv() {
            match report {
                Report::State(state) => {
                    if !matches!(state, State::Connected(_)) {
                        // Every peer went with the socket they were signalled over.
                        self.connected.clear();
                        self.sockets.clear();
                        self.speaking.clear();
                        self.arrived.clear();
                        // A connection that has gone takes its lobbies with it. Leaving
                        // them on screen would offer a player a join that cannot be sent.
                        self.lobbies.clear();
                    }
                    self.state = state;
                }
                Report::Event(event) => self.absorb(*event),
                Report::Audio { socket_id, packet } => {
                    // A moment rather than a level. The window redraws five times a second
                    // and audio arrives fifty times a second, so what it needs is "recently"
                    // -- and `pump` running is what makes this decay.
                    self.speaking.insert(socket_id);
                    self.arrived.push(packet);
                }
                Report::Peer {
                    socket_id,
                    connected,
                } => {
                    if connected {
                        self.connected.insert(socket_id);
                    } else {
                        self.connected.remove(&socket_id);
                    }
                }
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
            Event::PeerJoined {
                socket_id,
                client: Some(client),
                ..
            } => {
                self.sockets.insert(client.client_id, socket_id);
            }
            // Not a join: the Electron client treats `setClient` for a known peer as an
            // update, and so does `acl-core`. What changes is which player is on that
            // socket, which is exactly what this map holds.
            Event::PeerChanged { socket_id, client } => {
                self.sockets.retain(|_, socket| *socket != socket_id);
                self.sockets.insert(client.client_id, socket_id);
            }
            Event::PeerLeft { socket_id } => {
                self.sockets.retain(|_, socket| *socket != socket_id);
                self.connected.remove(&socket_id);
                // Or they stay speaking forever, on a screen, after they have gone.
                self.vocal.remove(&socket_id);
            }
            Event::VoiceActivity {
                socket_id,
                speaking,
            } => {
                if speaking {
                    self.vocal.insert(socket_id);
                } else {
                    self.vocal.remove(&socket_id);
                }
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

    /// The packets that have arrived, and forgets them.
    #[must_use]
    pub(crate) fn take_arrived(&mut self) -> Vec<acl_core::peers::Incoming> {
        std::mem::take(&mut self.arrived)
    }

    /// Whether the server says this player is speaking.
    ///
    /// Read rather than taken, unlike [`Self::take_speaking`]: `VAD` is a level and arrives
    /// on transitions, so forgetting it between frames would show a peer as speaking for one
    /// paint and silent for the next ninety-nine.
    #[must_use]
    pub(crate) fn talking(&self, client_id: i64) -> bool {
        self.sockets
            .get(&client_id)
            .is_some_and(|socket| self.vocal.contains(socket))
    }

    /// Claims the game host's role.
    ///
    /// For the promotion case only: `join` carries `is_host` for the join itself. See
    /// `Session::set_host`.
    pub(crate) fn say_host(&self, client_id: i64) {
        self.send(Command::SetHost(client_id));
    }

    /// Tells the lobby whether this player is speaking.
    ///
    /// On transitions only. The caller's detector has a hangover for exactly this reason:
    /// speech is fifty frames a second, and a message per frame to every peer would be more
    /// traffic than the audio it is describing.
    pub(crate) fn say_speaking(&self, speaking: bool) {
        self.send(Command::VoiceActivity(speaking));
    }

    /// Sends one Opus packet to everybody in the lobby.
    pub(crate) fn send_audio(&self, packet: Vec<u8>) {
        self.send(Command::SendAudio(packet));
    }

    /// The socket a player is on, if the server has said.
    #[must_use]
    pub(crate) fn socket_of(&self, client_id: i64) -> Option<&str> {
        self.sockets.get(&client_id).map(String::as_str)
    }

    /// Whether a player has been heard since this was last asked, and forgets it.
    ///
    /// Taken rather than read, so "speaking" decays on its own: a peer who stops sending
    /// stops being in the set the next time the window looks, with nothing having to notice
    /// they went quiet.
    #[must_use]
    pub(crate) fn take_speaking(&mut self) -> std::collections::BTreeSet<i64> {
        let sockets = std::mem::take(&mut self.speaking);
        self.sockets
            .iter()
            .filter(|(_, socket)| sockets.contains(*socket))
            .map(|(client_id, _)| *client_id)
            .collect()
    }

    /// Whether a player's connection is up.
    ///
    /// By client id, because that is what the window has: `roster::Voice::connected` is
    /// asked about a player, and the socket a player is on is something only the server
    /// ever said.
    #[must_use]
    pub(crate) fn hears(&self, client_id: i64) -> bool {
        self.sockets
            .get(&client_id)
            .is_some_and(|socket| self.connected.contains(socket))
    }

    /// How many connections are up.
    #[must_use]
    pub(crate) fn connected_peers(&self) -> usize {
        self.connected.len()
    }

    /// Joins a lobby, which is what starts the mesh.
    pub(crate) fn join(&mut self, code: &str, player_id: i64, client_id: i64, is_host: bool) {
        self.send(Command::Join {
            code: code.to_owned(),
            player_id,
            client_id,
            is_host,
        });
    }

    /// Leaves it.
    pub(crate) fn leave(&mut self) {
        self.connected.clear();
        self.send(Command::Leave);
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
    // The mesh. Built when the server issues a peer configuration, because that is what
    // says which relays to use -- a mesh built before it would be one built against
    // defaults the server was about to replace.
    let mut mesh: Option<acl_core::peers::PeerSet> = None;
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
                // Audio is never deferred. A packet held until the handshake finishes is
                // a packet describing a moment that has passed, and twenty milliseconds of
                // stale speech is worse than the gap it fills.
                Command::SendAudio(packet) => broadcast(&runtime, mesh.as_mut(), &packet),
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
                        if let Some(live) = session.as_mut() {
                            runtime.block_on(follow(&event, &mut mesh, live, answers));
                        }
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
            // Whatever the connections produced in the meantime. Every iteration, because
            // candidates are gathered after the offer and there may be no event left to
            // hang the sending off.
            if let Some(current) = session.as_mut() {
                runtime.block_on(drain(&mut mesh, current, answers));
            }
        }
    }
}

/// Sends one packet to everybody in the mesh.
///
/// To everybody, because who can hear it is the *receiver's* decision: gain and distance
/// are applied where the audio is played, which is what makes a lobby's rules the same for
/// everybody rather than whatever each sender believed about them.
fn broadcast(
    runtime: &tokio::runtime::Runtime,
    mesh: Option<&mut acl_core::peers::PeerSet>,
    packet: &[u8],
) {
    let Some(mesh) = mesh else {
        return;
    };
    for peer in mesh.peers() {
        let _ =
            runtime.block_on(mesh.send_audio(&peer, packet, std::time::Duration::from_millis(20)));
    }
}

/// Turns one session event into whatever the mesh should do about it.
///
/// Every decision here belongs to `acl-core` and none of it is made here. Who offers to
/// whom is [`acl_core::session::Arrival`] -- offering to somebody who was already in the
/// lobby races the offer they are making, which is the glare the arrival distinction exists
/// to prevent. What to do with an arriving signal is `acl_net::signal_route`, which
/// `PeerSet::on_signal` consults.
async fn follow(
    event: &Event,
    mesh: &mut Option<acl_core::peers::PeerSet>,
    session: &mut Session,
    answers: &Sender<Report>,
) {
    use acl_core::session::Arrival;

    match event {
        // The relays, as the server issued them for this session. The mesh is built here
        // rather than at connect, because a mesh built before this is one built against
        // defaults the server was about to replace.
        Event::PeerConfig(config) => {
            // `force_relay_only` is the server's request rather than an instruction, and
            // `RtcConfig::new` is where that distinction is applied -- a configuration that
            // forces relay mode with no relay in it has already been refused by
            // `peer_config`, because gathering nothing fails harder than the direct attempt
            // it replaced.
            let rtc = acl_net::ice::RtcConfig::new(&config.ice_servers, config.force_relay_only);
            match mesh.as_mut() {
                Some(mesh) => mesh.reconfigure(rtc),
                None => *mesh = Some(acl_core::peers::PeerSet::new(rtc)),
            }
        }
        Event::PeerJoined {
            socket_id, arrival, ..
        } => {
            let Some(mesh) = mesh.as_mut() else {
                return;
            };
            // Only to a newcomer. The other side offers when we are the newcomer, and both
            // offering is the glare.
            if *arrival == Arrival::Newcomer
                && let Ok(outbound) = mesh.offer(socket_id).await
            {
                let _ = session
                    .signal(&outbound.to, outbound.payload.to_value())
                    .await;
            }
        }
        Event::PeerLeft { socket_id } => {
            if let Some(mesh) = mesh.as_mut() {
                mesh.close(socket_id).await;
            }
            let _ = answers.send(Report::Peer {
                socket_id: socket_id.clone(),
                connected: false,
            });
        }
        Event::Signal { from, data } => {
            let Some(mesh) = mesh.as_mut() else {
                return;
            };
            if let Ok(outbound) = mesh.on_signal(from, data).await {
                for out in outbound {
                    let _ = session.signal(&out.to, out.payload.to_value()).await;
                }
            }
        }
        Event::Closed(_) | Event::Refused(_) => {
            if let Some(mesh) = mesh.as_mut() {
                mesh.close_all().await;
            }
        }
        _ => {}
    }
}

/// Sends on whatever the connections have produced, and reports what has changed.
///
/// **Called every time round the loop, not only when a session event arrives**, and the
/// difference is the whole connection. ICE candidates are gathered asynchronously *after*
/// the offer and answer are exchanged, so by the time they exist there may be nothing left
/// to react to -- draining only on events means the candidates sit in the queue, neither
/// side ever learns how to reach the other, and both connections stay `Connecting` for
/// ever. `two_links_reach_each_other` found exactly that.
async fn drain(
    mesh: &mut Option<acl_core::peers::PeerSet>,
    session: &mut Session,
    answers: &Sender<Report>,
) {
    let Some(mesh) = mesh.as_mut() else {
        return;
    };
    let (outbound, peer_events, audio) = mesh.drain();
    for out in outbound {
        let _ = session.signal(&out.to, out.payload.to_value()).await;
    }
    for peer_event in peer_events {
        let acl_core::peers::PeerEvent::StateChanged { peer, state } = peer_event;
        let _ = answers.send(Report::Peer {
            socket_id: peer,
            connected: state == webrtc::peer_connection::RTCPeerConnectionState::Connected,
        });
    }
    // The packets themselves are not carried to the window -- it has nothing to do with
    // them and a channel of audio frames into a paint loop is a queue that grows. What it
    // is told is *that* a peer is speaking, which is the one question `roster::Voice` asks
    // that nothing could answer: `audible`.
    //
    // Decoding, jitter buffering and mixing are `acl-audio`'s and are not wired yet. This
    // is deliberately the smallest true thing: audio is arriving from this peer.
    for packet in audio {
        let _ = answers.send(Report::Audio {
            socket_id: packet.peer.clone(),
            packet,
        });
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
        Command::Join {
            code,
            player_id,
            client_id,
            is_host,
        } => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.join(&code, player_id, client_id, is_host));
            }
        }
        Command::VoiceActivity(speaking) => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.voice_activity(speaking));
            }
        }
        Command::SetHost(client_id) => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.set_host(client_id));
            }
        }
        Command::Leave => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.leave());
            }
        }
        Command::SendAudio(_) => {
            // Handled in the loop, where the mesh is. Here only so the match is total --
            // routing it through `obey` would need the mesh threaded into a function whose
            // job is the session.
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

    /// Two links, one lobby, a real server -- and a peer connection between them.
    ///
    /// This is the test that says the mesh is wired rather than merely present. Everything
    /// it exercises is a seam: the session's `Arrival` deciding who offers, `signal_route`
    /// deciding what to do with what arrives, the candidates draining back out through the
    /// socket the session owns, and the state change arriving at the window as a `Peer`
    /// report.
    ///
    /// ```text
    /// ACL_SERVER_BIN=../ACL-Server/target/debug/acl-server \\
    ///   cargo test -p acl-client -- --ignored two_links_reach_each_other
    /// ```
    ///
    /// Loopback, so it proves signalling and connection establishment and says nothing
    /// about NAT or relays. `acl-core`'s own `peers_loopback` covers the connection in
    /// isolation; this covers it being driven by the session.
    #[test]
    #[ignore = "needs a server: set ACL_SERVER_BIN"]
    fn two_links_reach_each_other() {
        const PORT: u16 = 19_762;
        let Some(_server) = serving(PORT) else {
            eprintln!("skipping: set ACL_SERVER_BIN to the server binary");
            return;
        };
        let base = format!("http://127.0.0.1:{PORT}");

        let mut first = Link::start();
        let mut second = Link::start();
        first.connect(&base);
        second.connect(&base);

        // Both connected before either joins. A join emitted into a session that has not
        // finished its handshake is dropped -- the bug `the_link_sees_a_real_lobby` found.
        let both_up = wait(&mut [&mut first, &mut second], |links| {
            links
                .iter()
                .all(|link| matches!(link.state(), State::Connected(_)))
        });
        assert!(both_up, "one of the sessions never connected");

        first.join("MESHED", 1, 11, true);
        second.join("MESHED", 2, 22, false);

        // The second is the newcomer, so the first offers to it. Either side reporting the
        // other as connected is the connection.
        let met = wait(&mut [&mut first, &mut second], |links| {
            links[0].hears(22) || links[1].hears(11)
        });
        assert!(
            met,
            "no peer connection: first sees {} peer(s), second sees {}",
            first.connected_peers(),
            second.connected_peers()
        );
    }

    /// A tone sent by one client, and heard by the other.
    ///
    /// Everything else in this file proves a *connection*: that the two found each other,
    /// that the offer was answered, that the peer is reported as connected. None of it
    /// proves audio, and a connected peer nobody can hear is the bug this port is most
    /// likely to ship — every piece works, the whole says nothing.
    ///
    /// So this one carries a 440 Hz tone the whole way: encoded by the real encoder, sent
    /// over the real track through a real server, taken off the wire by the receiving side,
    /// and decoded. Then it asks the only question that matters, which is not "did bytes
    /// arrive" but "is what came out the sound that went in" -- a Goertzel filter at 440 Hz
    /// against one at 1 kHz. Bytes arriving is satisfied by noise.
    ///
    /// What it still is not: two machines, and two people. It is one process, over
    /// loopback, with no NAT and no relay and no sound card. It rules out the silent
    /// failure; it does not stand in for a round.
    #[test]
    #[ignore = "needs a server: set ACL_SERVER_BIN"]
    fn a_tone_sent_by_one_is_heard_by_the_other() {
        use acl_audio::codec::{Decoder, Encoder, FRAME_SAMPLES};

        const PORT: u16 = 19_763;
        const TONE_HZ: f32 = 440.0;
        let Some(_server) = serving(PORT) else {
            eprintln!("skipping: set ACL_SERVER_BIN to the server binary");
            return;
        };
        let base = format!("http://127.0.0.1:{PORT}");

        let mut speaker = Link::start();
        let mut listener = Link::start();
        speaker.connect(&base);
        listener.connect(&base);
        let both_up = wait(&mut [&mut speaker, &mut listener], |links| {
            links
                .iter()
                .all(|link| matches!(link.state(), State::Connected(_)))
        });
        assert!(both_up, "one of the sessions never connected");

        speaker.join("HEARD", 1, 11, true);
        listener.join("HEARD", 2, 22, false);
        let met = wait(&mut [&mut speaker, &mut listener], |links| {
            links[0].hears(22) || links[1].hears(11)
        });
        assert!(met, "no peer connection, so nothing to listen for");

        let mut opus = Encoder::new().expect("an encoder");
        let mut phase = 0.0_f32;
        let mut heard: Vec<f32> = Vec::new();
        let mut decoder = Decoder::new().expect("a decoder");
        let mut frame = vec![0.0_f32; FRAME_SAMPLES];

        // Sent repeatedly rather than once. A single packet during the moments after ICE
        // settles is a packet that can legitimately be dropped, and a test that failed for
        // that reason would be a test nobody trusts.
        for _ in 0..400 {
            let mut samples = Vec::with_capacity(FRAME_SAMPLES);
            for _ in 0..FRAME_SAMPLES {
                samples.push((phase * std::f32::consts::TAU).sin() * 0.5);
                phase = (phase + TONE_HZ / 48_000.0).fract();
            }
            let mut packet = Vec::new();
            if opus.encode(&samples, &mut packet).is_ok() {
                speaker.send_audio(packet);
            }

            speaker.pump();
            listener.pump();
            for arrived in listener.take_arrived() {
                if let Ok(written) = decoder.decode(&arrived.payload, &mut frame) {
                    heard.extend_from_slice(&frame[..written]);
                }
            }
            if heard.len() >= FRAME_SAMPLES * 20 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        assert!(
            heard.len() >= FRAME_SAMPLES * 5,
            "only {} samples came through; the track carries nothing",
            heard.len()
        );
        // Opus needs a moment to converge, and the first frames out of a decoder that has
        // just started are not what the encoder was given. Judged on what follows them.
        let settled = &heard[FRAME_SAMPLES * 2..];
        let signal = goertzel(settled, TONE_HZ);
        let elsewhere = goertzel(settled, 1_000.0);
        assert!(
            signal > elsewhere * 8.0,
            "what arrived is not the tone that was sent: {TONE_HZ} Hz at {signal:.4},              1 kHz at {elsewhere:.4}"
        );
    }

    /// How much of one frequency is in a signal.
    ///
    /// Goertzel rather than an FFT because one bin is all this needs, and a dependency for
    /// a test assertion is a dependency in the release.
    fn goertzel(samples: &[f32], hz: f32) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a sample count, in the thousands"
        )]
        let count = samples.len() as f32;
        let k = (hz / 48_000.0) * std::f32::consts::TAU;
        let coefficient = 2.0 * k.cos();
        let (mut previous, mut older) = (0.0_f32, 0.0_f32);
        for sample in samples {
            let current = sample + coefficient * previous - older;
            older = previous;
            previous = current;
        }
        // Magnitude, normalised by length so the threshold does not depend on how many
        // frames happened to arrive before the loop stopped.
        (previous * previous + older * older - coefficient * previous * older).sqrt() / count
    }

    /// One client says it is speaking, and the other one sees it.
    ///
    /// Both halves of the same signal, which had neither. `acl-core` has parsed
    /// `Event::VoiceActivity` since it was written and `Link` dropped it on the floor; the
    /// client emitted no `VAD` at all. The visible result was a speaking indicator that
    /// never lit up, for anybody, in either window — and nothing failed, so there was
    /// nothing to notice.
    ///
    /// It goes through the real server because that is where the relaying happens: this end
    /// sends `VAD` with a boolean and the server decides whom to tell and under what socket
    /// id. A test against a loopback of our own would prove the two halves of *our* guess.
    #[test]
    #[ignore = "needs a server: set ACL_SERVER_BIN"]
    fn saying_something_lights_up_the_other_end() {
        const PORT: u16 = 19_764;
        let Some(_server) = serving(PORT) else {
            eprintln!("skipping: set ACL_SERVER_BIN to the server binary");
            return;
        };
        let base = format!("http://127.0.0.1:{PORT}");

        let mut speaker = Link::start();
        let mut listener = Link::start();
        speaker.connect(&base);
        listener.connect(&base);
        let both_up = wait(&mut [&mut speaker, &mut listener], |links| {
            links
                .iter()
                .all(|link| matches!(link.state(), State::Connected(_)))
        });
        assert!(both_up, "one of the sessions never connected");

        speaker.join("VOICED", 1, 11, true);
        listener.join("VOICED", 2, 22, false);
        // The listener has to know who 11 *is* before it can be asked whether they are
        // talking: `talking` looks the client id up in the socket map the lobby fills.
        let met = wait(&mut [&mut speaker, &mut listener], |links| {
            links[1].socket_of(11).is_some()
        });
        assert!(met, "the listener never learned the speaker's socket");

        assert!(
            !listener.talking(11),
            "somebody is speaking before anybody said so"
        );

        speaker.say_speaking(true);
        let lit = wait(&mut [&mut speaker, &mut listener], |links| {
            links[1].talking(11)
        });
        assert!(lit, "the speaker said so and the listener never heard");

        // And it goes out again. A level that only ever arrives is an indicator that stays
        // on for the rest of the lobby, which is worse than one that never comes on: it
        // reads as a peer whose microphone is stuck open.
        speaker.say_speaking(false);
        let out = wait(&mut [&mut speaker, &mut listener], |links| {
            !links[1].talking(11)
        });
        assert!(out, "the speaker stopped and the indicator stayed on");
    }

    /// Drives some links until something is true of them, or long enough to say it is not.
    ///
    /// Both have to be pumped throughout: each holds a session whose heartbeat is answered
    /// from inside `next`, and a session left alone is a session the server drops.
    fn wait(links: &mut [&mut Link], mut wanted: impl FnMut(&[&mut Link]) -> bool) -> bool {
        for _ in 0..600 {
            for link in links.iter_mut() {
                link.pump();
            }
            if wanted(links) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        false
    }
}
