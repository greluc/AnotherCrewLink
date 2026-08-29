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
    /// List this lobby in the public browser, or take it out of it.
    ///
    /// One command for both, because the server's handler is one: a payload whose
    /// `isPublic` is false removes the listing rather than updating it.
    Advertise {
        /// The lobby's code.
        code: String,
        /// The listing, in the shape `PublicLobbyInput` deserialises.
        lobby: serde_json::Value,
    },
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
        /// When the window asked, so the worker can say how long it waited.
        ///
        /// Nothing else on this channel is timed and nothing else needs to be. This one is,
        /// because it is the command the whole peer handshake waits behind -- the server
        /// cannot tell anybody else this client is here until it lands -- and because a
        /// queue that delayed it was ruled out on 2026-08-28 by checking the wrong thing.
        /// The check was "there is no `Command::Signal`", which is true; the join is not a
        /// signal and it gates every signal there will be. Two testers measured thirty
        /// seconds to connect and this number says whether that was the queue or not.
        asked: std::time::Instant,
    },
    /// Say whether this player is speaking, for everybody else's indicator.
    VoiceActivity(bool),
    /// Claim the game host's role, after having become it.
    /// Correct this client's in-game identity.
    Identify {
        /// This player's in-game id.
        player_id: i64,
        /// This player's client id.
        client_id: i64,
    },
    SetHost(i64),
    /// Claim or release the impostor radio.
    ImpostorRadio(bool),
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
    /// A peer was heard from.
    ///
    /// The *name*, not the packet. Audio goes straight from the signalling worker to the
    /// mixing thread and does not pass through the window at all -- see `Link::start`.
    /// What the window needs is the one question `roster::Voice` asks that nothing else
    /// could answer: is this peer audible right now.
    Speaking(String),
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
    /// Where the worker delivers arriving audio.
    ///
    /// Behind a lock because it is replaced whenever the audio pipeline is rebuilt, and
    /// read by the worker between rounds. Contended once per rebuild against once per
    /// fiftieth of a second, which is not contention.
    to_mixer: std::sync::Arc<std::sync::Mutex<Sender<acl_core::peers::Incoming>>>,
    commands: Sender<Command>,
    /// Whether the player asked for every connection to go through a relay.
    ///
    /// `natFix` on the settings page. Shared rather than sent as a command because the
    /// worker reads it when the server's peer configuration arrives, which can be at any
    /// moment and is not a moment this side knows about.
    force_relay: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
    /// Who is on the impostor radio, by socket id.
    ///
    /// One at a time in practice -- `Voice.tsx` keeps a single `impostorRadioClientId` and
    /// the first claim wins -- but held as a set here rather than as an option, because two
    /// claims arriving before either is released is a state the wire can produce and an
    /// `Option` would silently drop one of them.
    on_radio: std::collections::BTreeSet<String>,
    /// Who the server says is speaking, by socket id.
    ///
    /// A level, not a moment, which is the difference between this and [`Self::speaking`].
    /// The server relays `VAD` when a peer starts and again when they stop, so this is held
    /// until told otherwise rather than decaying — a peer who talks for ten seconds sends
    /// two messages, not five hundred.
    vocal: std::collections::BTreeSet<String>,
    /// Which peers have sent audio since the window last looked.
    ///
    /// When each was last heard, so "recently" is a length of time rather than "since the
    /// window last asked".
    ///
    /// It was a set that the window emptied by reading it, and that made the meaning of
    /// "recently" the repaint interval. At five frames a second that was two hundred
    /// milliseconds and worked; with the pointer in the window the client draws at sixty,
    /// and a peer sending a packet every twenty milliseconds is absent from three looks out
    /// of four. Their ring flickered on and off several times a second, and it was the
    /// asking that made it flicker, not them.
    ///
    /// A moment rather than a count: what the window asks is whether somebody is speaking,
    /// and how many packets arrived is a question nothing here has.
    speaking: std::collections::BTreeMap<String, std::time::Instant>,
    /// Which peers have a connection that is up.
    ///
    /// The window shows this as the difference between a player who has arrived and one who
    /// can be heard — `roster::Link::Silent` against `Connected`, which is a distinction
    /// `views::main` already draws and had nothing to fill in.
    connected: std::collections::BTreeSet<String>,
}

/// How long after a packet somebody still counts as having been heard.
///
/// **Half a second, and the margin is the whole point.** This has to outlast the gap between
/// one *delivery* and the next, which is not the twenty milliseconds between packets: audio
/// still reaches the window through the repaint loop, so it arrives in bursts about two
/// hundred milliseconds apart. A window of two hundred was set here earlier today and was
/// exactly as wide as the gap it had to bridge -- no margin at all, so a burst arriving ten
/// milliseconds late dropped the peer out of the set and their ring went amber for a frame
/// while they were being heard perfectly well.
///
/// Five hundred survives two missed deliveries and still clears within half a second of
/// somebody genuinely going. It is deliberately a duration and not the repaint interval:
/// tying the two together is the mistake this whole evening has been made of.
///
/// **The audio itself no longer comes this way**, as of 2026-08-29 -- it goes from the
/// worker straight to the mixing thread. What still arrives through the repaint loop is
/// `Report::Speaking`, which is a name and a moment, so the gap this has to bridge is
/// unchanged and so is the number. It could come down only if the window learnt about
/// speech on a clock of its own, which it does not and has no reason to.
const RECENTLY: std::time::Duration = std::time::Duration::from_millis(500);

/// Where encoded frames go on their way out.
///
/// A named type rather than a bare `Sender<Command>`, so the capture callback can be given
/// the one thing it is allowed to do with the worker's channel and nothing else.
#[derive(Clone)]
pub(crate) struct AudioSink(std::sync::mpsc::Sender<Command>);

impl AudioSink {
    /// Hands one encoded frame over. `false` means the worker has gone.
    pub(crate) fn send(&self, packet: Vec<u8>) -> bool {
        self.0.send(Command::SendAudio(packet)).is_ok()
    }
}

impl Link {
    /// Starts the thread. Nothing is connected until [`Link::connect`] is called.
    ///
    /// `to_mixer` is where arriving audio goes -- straight from this worker to the mixing
    /// thread, with the window nowhere in between. **That is a fix of 2026-08-29 and it is
    /// structural rather than incidental.** Packets used to reach the mixer as
    /// `Report::Audio`, which `Link::pump` drains and `Client::carry_audio` forwards, both
    /// of them inside `eframe`'s `update` -- a loop whose floor is two hundred
    /// milliseconds when the pointer is not over the window, which is the whole time
    /// anybody is playing. Fifty packets a second arrived and were delivered ten at a time,
    /// five times a second, in both directions.
    ///
    /// Outgoing audio goes the other way for the same reason: the capture callback sends
    /// `Command::SendAudio` into the command channel itself, through [`Link::audio_sink`].
    /// Neither direction passes through a paint any more, and `take_arrived` and
    /// `take_encoded` are gone rather than merely unused -- a method that exists is a
    /// method something can start calling again.
    pub(crate) fn start() -> (Self, Receiver<acl_core::peers::Incoming>) {
        let (to_mixer, packets) = std::sync::mpsc::channel::<acl_core::peers::Incoming>();
        let to_mixer = std::sync::Arc::new(std::sync::Mutex::new(to_mixer));
        let for_worker = std::sync::Arc::clone(&to_mixer);
        let (commands, orders) = std::sync::mpsc::channel::<Command>();
        let (answers, reports) = std::sync::mpsc::channel::<Report>();
        let force_relay = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let relay_for_worker = std::sync::Arc::clone(&force_relay);
        std::thread::Builder::new()
            .name("signalling".to_owned())
            .spawn(move || run(&orders, &answers, &for_worker, &relay_for_worker))
            // A client that cannot start a thread has bigger problems than the lobby
            // browser, and there is nowhere to report them from here: the link simply
            // stays idle.
            .ok();
        let link = Self {
            commands,
            to_mixer,
            force_relay,
            reports,
            state: State::Idle,
            lobbies: std::collections::BTreeMap::new(),
            answer: None,
            connected: std::collections::BTreeSet::new(),
            sockets: std::collections::BTreeMap::new(),
            vocal: std::collections::BTreeSet::new(),
            on_radio: std::collections::BTreeSet::new(),
            speaking: std::collections::BTreeMap::new(),
        };
        (link, packets)
    }

    /// Points the media path at a new mixer, and hands back its end.
    ///
    /// The audio pipeline is rebuilt when a capture setting changes that can only be
    /// applied at the moment a device is opened. The old mixing thread goes with it, so the
    /// worker has to be told where to deliver instead -- otherwise every packet after the
    /// first rebuild would be sent to a receiver nobody is holding, and the client would go
    /// deaf the first time somebody ticked a box.
    pub(crate) fn rewire_audio(&self) -> Receiver<acl_core::peers::Incoming> {
        let (sender, packets) = std::sync::mpsc::channel::<acl_core::peers::Incoming>();
        if let Ok(mut held) = self.to_mixer.lock() {
            *held = sender;
        }
        packets
    }

    /// Where the microphone sends what it has encoded.
    ///
    /// Handed to the capture callback, which is the only thing that uses it. A packet goes
    /// from the callback into the worker's own command channel without a paint in between.
    pub(crate) fn audio_sink(&self) -> AudioSink {
        AudioSink(self.commands.clone())
    }

    /// Whether to force every connection through a relay.
    ///
    /// `natFix`, which reached nothing until 2026-08-27: the only thing that could force a
    /// relay was the server's own `forceRelayOnly`, so a player behind a NAT that needs one
    /// could tick the box and watch it do nothing.
    pub(crate) fn set_force_relay(&self, forced: bool) {
        self.force_relay
            .store(forced, std::sync::atomic::Ordering::Relaxed);
    }

    /// Takes whatever the session has said. Cheap, and called once a frame.
    pub(crate) fn pump(&mut self) {
        while let Ok(report) = self.reports.try_recv() {
            match report {
                Report::State(state) => {
                    // On the transition. The state is reported when it changes, but a
                    // reconnect can report the same failure twice and a log that repeats
                    // itself is one nobody reads to the end of.
                    if self.state != state {
                        acl_core::log_info!("net", "{state:?}");
                    }
                    if !matches!(state, State::Connected(_)) {
                        // Every peer went with the socket they were signalled over.
                        self.connected.clear();
                        self.sockets.clear();
                        self.speaking.clear();
                        // A connection that has gone takes its lobbies with it. Leaving
                        // them on screen would offer a player a join that cannot be sent.
                        self.lobbies.clear();
                    }
                    self.state = state;
                }
                Report::Event(event) => self.absorb(*event),
                Report::Speaking(socket_id) => {
                    // A moment rather than a level. The window redraws five times a second
                    // and audio arrives fifty times a second, so what it needs is "recently"
                    // -- and `pump` running is what makes this decay.
                    self.speaking.insert(socket_id, std::time::Instant::now());
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
                self.on_radio.remove(&socket_id);
            }
            Event::ImpostorRadio {
                socket_id,
                on_radio,
            } => {
                if on_radio {
                    self.on_radio.insert(socket_id);
                } else {
                    self.on_radio.remove(&socket_id);
                }
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
            // Said out loud. `Event::Ignored` is how `acl-core` reports a payload of the
            // wrong shape -- a rejected `clientPeerConfig`, a `join` with no socket id, a
            // `join_lobby` answered without a status -- and it reached no logger and no
            // screen. A server whose configuration this client refuses is a server
            // operator's problem, and from a player's side it looked exactly like a broken
            // client.
            Event::Ignored(why) => acl_core::log_warn!("net", "{why}"),
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

    /// Which player is on the impostor radio, if any.
    ///
    /// The lowest client id when more than one has claimed it, which is arbitrary and
    /// deliberate: the alternative is a rule that depends on arrival order, and two clients
    /// disagreeing about who is on the radio is worse than both being arbitrary in the same
    /// way. `Voice.tsx` keeps one and lets the first claim win, which has the same problem
    /// and resolves it less predictably.
    #[must_use]
    pub(crate) fn on_radio(&self) -> Option<i64> {
        self.sockets
            .iter()
            .filter(|(_, socket)| self.on_radio.contains(*socket))
            .map(|(client_id, _)| *client_id)
            .min()
    }

    /// Claims or releases the impostor radio for this player.
    pub(crate) fn say_on_radio(&self, on_radio: bool) {
        self.send(Command::ImpostorRadio(on_radio));
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

    /// Corrects this client's in-game identity.
    ///
    /// Sent on a change rather than every frame: `id` is a broadcast to the whole lobby,
    /// and the ids move twice in a session. See `Session::identify` for why the join's own
    /// copy is not enough.
    pub(crate) fn say_identity(&self, player_id: i64, client_id: i64) {
        self.send(Command::Identify {
            player_id,
            client_id,
        });
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

    /// The socket a player is on, if the server has said.
    #[must_use]
    pub(crate) fn socket_of(&self, client_id: i64) -> Option<&str> {
        self.sockets.get(&client_id).map(String::as_str)
    }

    /// Who has been heard within [`RECENTLY`], and forgets anybody older.
    ///
    /// Decays on its own, so a peer who stops sending stops being in the set without
    /// anything having to notice they went quiet -- but on a clock of its own rather than on
    /// however often the window happens to look. That distinction is the whole of this
    /// function: see [`Link::speaking`] for what asking sixty times a second used to do to
    /// somebody's ring.
    #[must_use]
    pub(crate) fn take_speaking(&mut self) -> std::collections::BTreeSet<i64> {
        let now = std::time::Instant::now();
        self.speaking
            .retain(|_, heard| now.duration_since(*heard) < RECENTLY);
        self.sockets
            .iter()
            .filter(|(_, socket)| self.speaking.contains_key(*socket))
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
            asked: std::time::Instant::now(),
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

    /// Lists this lobby publicly, or takes it out of the list.
    ///
    /// There was no way to do this at all until 2026-08-27: the client could browse public
    /// lobbies and join one, and never announce its own. `publicLobby_on`, `_title` and
    /// `_language` were three settings with a page of their own and nowhere to go.
    pub(crate) fn advertise(&mut self, code: &str, lobby: serde_json::Value) {
        self.send(Command::Advertise {
            code: code.to_owned(),
            lobby,
        });
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

/// What the worker holds between rounds.
///
/// Gathered into one value rather than six locals so that a round can be handed to a
/// function instead of to eight arguments. Nothing here is shared with another thread:
/// the worker owns all of it, and the window reaches it only through commands.
#[derive(Default)]
struct Held {
    session: Option<Session>,
    /// Built when the server issues a peer configuration, because that is what says which
    /// relays to use -- a mesh built before it would be one built against defaults the
    /// server was about to replace.
    mesh: Option<acl_core::peers::PeerSet>,
    /// One entry per peer this client has offered to or been offered by, holding what is
    /// known about repairing that connection. Beside the mesh rather than inside it,
    /// because everything it decides is `acl_net`'s and `acl_core::peers` is the part that
    /// owns transports.
    repairs: std::collections::BTreeMap<String, Repair>,
    /// This client's own socket id, which decides which end of a pair offers a
    /// replacement. Both ends must agree, and comparing socket ids is how they do it
    /// without a round trip.
    own_id: String,
    /// Whether the handshake has completed. See the module documentation: a session that
    /// is connected but not live drops everything emitted to it.
    live: bool,
    /// Commands that arrived before it did.
    deferred: Vec<Command>,
}

/// Carries out one round's worth of commands. `false` means the window is gone.
fn carry_out(
    runtime: &tokio::runtime::Runtime,
    waiting: Vec<Command>,
    held: &mut Held,
    answers: &Sender<Report>,
) -> bool {
    for command in waiting {
        match command {
            // These two are about the connection itself, so they are never deferred:
            // one replaces it and the other ends it.
            connection @ (Command::Connect(_) | Command::Disconnect) => {
                held.live = false;
                held.deferred.clear();
                // The peers went with the socket they were signalled over. Handled
                // here rather than in `obey` because `obey` has no mesh -- which is
                // how they came to be left standing.
                forget_the_peers(runtime, &mut held.mesh, &mut held.repairs);
                if !obey(runtime, &mut held.session, connection, answers) {
                    return false;
                }
            }
            // Leaving a lobby closed nothing at all until 2026-08-29, and the damage
            // was not the lobby just left -- it was the next one.
            //
            // `Session::leave` clears the membership and *discards* the
            // `Action::Disconnect` list that `Membership::clear` returns; it is the one
            // call site that throws it away, where the other two turn it into
            // `Event::PeerLeft` and so into `mesh.close`. So the connection stayed in
            // the map with nothing to say it was dead. Meet the same person in the next
            // lobby and `PeerSet::offer` sees `holds(peer)`, skips the build, and
            // offers a *renegotiation* on a transport whose credentials are gone. That
            // pair never connects again for the life of the app.
            Command::Leave => {
                forget_the_peers(runtime, &mut held.mesh, &mut held.repairs);
                // A join still waiting for the handshake is a join for the lobby this
                // command is leaving. Replayed after the connect, it would put the
                // client back into a lobby the player has already walked out of, and
                // `follow_the_lobby` would not send another `leave` because its edge
                // has been and gone.
                held.deferred
                    .retain(|waiting| !matches!(waiting, Command::Join { .. }));
                if !obey(runtime, &mut held.session, Command::Leave, answers) {
                    return false;
                }
            }
            // Audio is never deferred. A packet held until the handshake finishes is
            // a packet describing a moment that has passed, and twenty milliseconds of
            // stale speech is worse than the gap it fills.
            Command::SendAudio(packet) => broadcast(runtime, held.mesh.as_mut(), &packet),
            other if !held.live => held.deferred.push(other),
            other => {
                if !obey(runtime, &mut held.session, other, answers) {
                    return false;
                }
            }
        }
    }
    true
}

/// The thread: a runtime, and a session that outlives individual connections.
fn run(
    orders: &Receiver<Command>,
    answers: &Sender<Report>,
    to_mixer: &std::sync::Mutex<Sender<acl_core::peers::Incoming>>,
    force_relay: &std::sync::atomic::AtomicBool,
) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        let _ = answers.send(Report::State(State::Failed(
            "no async runtime could be started".to_owned(),
        )));
        return;
    };

    let mut held = Held::default();
    loop {
        // Without a session there is nothing to await, so the command channel is blocked
        // on. With one, everything waiting is taken before the session is attended to.
        //
        // **Everything, and that is the fix of 2026-08-28.** It used to take exactly one
        // command per round and then wait up to fifty milliseconds on the session below. On
        // a quiet socket that wait always runs to its end, so the loop drained about twenty
        // commands a second -- and the microphone puts fifty a second in, because `FRAME_MS`
        // is twenty and every encoded frame travels down this same channel as
        // `Command::SendAudio`.
        //
        // Fifty in against twenty out is a queue that grows by thirty a second for as long
        // as anybody is connected, and both halves of what two testers reported come out of
        // that arithmetic. Speech left at 0.4x real time, so the far end heard it slowed
        // down. And a command queued *T* seconds after connecting came out about 1.5*T*
        // seconds later: twenty seconds spent on the connect screen made a thirty-second
        // join, which is what they measured.
        //
        // The module documentation above says these are `select!`ed against `next`. That is
        // the better shape and it is still not what this does -- a `select!` would drop a
        // half-polled `Session::next` when a command won the race, and whether that loses a
        // frame is a question to answer before relying on it, not during a live test.
        // Draining costs nothing and removes the growth; the wait below now bounds only how
        // long a command may sit, never how many get through.
        let Some(waiting) = take_waiting(orders, held.session.is_some()) else {
            return;
        };

        if !carry_out(&runtime, waiting, &mut held, answers) {
            return;
        }

        if let Some(current) = held.session.as_mut() {
            // A short wait rather than none: this is the only place the heartbeat is
            // answered from, and a busy loop would spin a core to answer it no sooner.
            let next = runtime.block_on(async {
                tokio::time::timeout(std::time::Duration::from_millis(50), current.next()).await
            });
            match next {
                // The session ended. The window is told, and the loop goes back to
                // blocking on commands rather than spinning on a dead session.
                Ok(None) => {
                    held.session = None;
                    held.live = false;
                    held.deferred.clear();
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
                        if let Some(live) = held.session.as_mut() {
                            runtime.block_on(follow(
                                &event,
                                &mut held.mesh,
                                &mut held.repairs,
                                live,
                                answers,
                                force_relay,
                            ));
                        }
                        if let Event::Connected(id) = &event {
                            held.live = true;
                            held.own_id.clone_from(id);
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
                    if held.live {
                        for command in held.deferred.drain(..) {
                            if !obey(&runtime, &mut held.session, command, answers) {
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
            if let Some(current) = held.session.as_mut() {
                runtime.block_on(drain(
                    &mut held.mesh,
                    &mut held.repairs,
                    current,
                    to_mixer,
                    answers,
                ));
                // Every round, because a repair is decided by elapsed time rather than by
                // an event: a link that has been quiet for four seconds has to be noticed
                // by something that looks, and a rebuild scheduled for two seconds hence
                // has to be fired by something that is awake. The loop already wakes at
                // least every fifty milliseconds.
                runtime.block_on(repair(
                    &mut held.mesh,
                    &mut held.repairs,
                    current,
                    &held.own_id,
                    answers,
                ));
            }
        }
    }
}

/// Closes every peer connection and forgets what was known about repairing them.
///
/// Both, and in that order. A repair entry left behind would schedule a rebuild towards
/// somebody who is no longer in the lobby, and an attempt count left behind would make the
/// next lobby start on the slow schedule.
fn forget_the_peers(
    runtime: &tokio::runtime::Runtime,
    mesh: &mut Option<acl_core::peers::PeerSet>,
    repairs: &mut std::collections::BTreeMap<String, Repair>,
) {
    repairs.clear();
    if let Some(mesh) = mesh.as_mut() {
        runtime.block_on(mesh.close_all());
    }
}

/// Everything the window has asked for since the last round.
///
/// `None` means the window has dropped its end, which is the worker's cue to stop.
///
/// With no session there is nothing else to do, so this blocks for the first command. With
/// one, it takes **everything waiting** and returns -- which is the whole of the fix
/// described in the loop above, and the reason it is a function of its own is that
/// `it_takes_every_command_that_is_waiting` can then say so in a test rather than in a
/// comment.
fn take_waiting(orders: &Receiver<Command>, connected: bool) -> Option<Vec<Command>> {
    let mut waiting = Vec::new();
    if connected {
        loop {
            match orders.try_recv() {
                Ok(command) => waiting.push(command),
                Err(TryRecvError::Empty) => return Some(waiting),
                Err(TryRecvError::Disconnected) => return None,
            }
        }
    } else {
        match orders.recv() {
            Ok(command) => waiting.push(command),
            Err(_) => return None,
        }
        Some(waiting)
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
    repairs: &mut std::collections::BTreeMap<String, Repair>,
    session: &mut Session,
    answers: &Sender<Report>,
    force_relay: &std::sync::atomic::AtomicBool,
) {
    use acl_core::session::Arrival;

    match event {
        // The relays, as the server issued them for this session. The mesh is built here
        // rather than at connect, because a mesh built before this is one built against
        // defaults the server was about to replace.
        // A mesh from the moment there is a socket, built on the defaults `Voice.tsx`
        // ships. It used to be built here and nowhere else, so a server that sent no
        // `clientPeerConfig`, or sent one this client refuses, left `mesh` as `None` --
        // and every arm below returns early on that. The client joined the lobby, appeared
        // in everyone's roster, and created no peer connections at all. Silently, for the
        // whole session.
        //
        // `PeerConfig` now only ever reconfigures, which is also the honest shape: the
        // configuration a connection was built with is fixed, and `reconfigure` says so.
        Event::Connected(_) => {
            if mesh.is_none() {
                *mesh = Some(acl_core::peers::PeerSet::new(acl_net::ice::RtcConfig::new(
                    &acl_net::ice::default_servers(),
                    false,
                )));
            }
        }
        Event::PeerConfig(config) => {
            // The server's `forceRelayOnly` is already applied -- and already refused when
            // it named no relay -- by `RtcConfig::new`, which `apply_client_peer_config`
            // built this with.
            //
            // The player's own `natFix` is an *or*, the way `Voice.tsx` writes it:
            // `settingsRef.current.natFix || relayedPeers.current[peer]`. Rebuilding
            // through `RtcConfig::new` is what applies relay rule three to it too, so
            // ticking the box on a server with no relay leaves the client on `All` rather
            // than gathering nothing. `with_tcp_relays` deduplicates, so passing a list
            // that has already been through it adds nothing.
            let asked = force_relay.load(std::sync::atomic::Ordering::Relaxed);
            let rtc = if asked {
                acl_net::ice::RtcConfig::new(&config.ice_servers, true)
            } else {
                (**config).clone()
            };
            acl_core::log_info!(
                "net",
                "peer configuration: {:?} ({:?})",
                rtc.urls(),
                rtc.ice_transport_policy
            );
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
            // Tracked whichever end offers, because either end can be the one that notices
            // the link go. An entry that already exists is left alone: a peer who rejoins
            // a lobby they never left keeps the attempt count that says how much trouble
            // this pair has been.
            repairs.entry(socket_id.clone()).or_insert_with(Repair::new);
            // Only to a newcomer. The other side offers when we are the newcomer, and both
            // offering is the glare.
            if *arrival == Arrival::Newcomer {
                match mesh.offer(socket_id).await {
                    Ok(outbound) => {
                        acl_core::log_info!("peer", "{socket_id} joined; offering");
                        let _ = session
                            .signal(&outbound.to, outbound.payload.to_value())
                            .await;
                    }
                    // Worth a line of its own. A connection that is never offered looks
                    // exactly like one that is offered and never answered, and the two are
                    // fixed in different places.
                    Err(why) => {
                        acl_core::log_warn!("peer", "could not offer to {socket_id}: {why}");
                    }
                }
            } else {
                // Said out loud because it is the *expected* half of the rule and its
                // absence is indistinguishable from a bug: somebody reading this log has to
                // be able to tell "we correctly did not offer" from "we forgot to".
                acl_core::log_info!("peer", "{socket_id} was already here; they offer");
            }
        }
        Event::PeerLeft { socket_id } => {
            acl_core::log_info!("peer", "{socket_id} left");
            // Before the close, so a rebuild scheduled a moment ago cannot fire against
            // somebody who has gone. The server announces departures, so a peer still in
            // the map is one still in the lobby -- which is the test `Voice.tsx` makes at
            // the top of `scheduleReconnect` and for the same reason.
            repairs.remove(socket_id);
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
            // A signal can be the first thing heard about a peer: the `join` that would
            // have created the entry is the *other* end's, and an offer can overtake the
            // membership event that announces its sender.
            repairs.entry(from.clone()).or_insert_with(Repair::new);
            match mesh.on_signal(from, data).await {
                Ok(outbound) => {
                    for out in outbound {
                        let _ = session.signal(&out.to, out.payload.to_value()).await;
                    }
                }
                // Said out loud, and that is a fix of its own. Offer glare -- both ends
                // offering at once, which happens when two players join within the same
                // server tick -- ends with each applying an answer to a connection already
                // in `stable`, which the crate refuses. Discarded silently, that was a
                // pair who could never hear each other and nothing in the log to say why.
                // The rebuild above is what gets them out of it; this is what makes it
                // legible when it happens.
                Err(why) => acl_core::log_warn!("peer", "a signal from {from} was refused: {why}"),
            }
        }
        Event::Closed(_) | Event::Refused(_) => {
            repairs.clear();
            if let Some(mesh) = mesh.as_mut() {
                mesh.close_all().await;
            }
        }
        _ => {}
    }
}

/// What is known about one peer's connection, and about repairing it.
///
/// **The whole of this existed and was never called.** `acl_net::reconnect` and
/// `acl_net::peer::RepairPolicy` were ported, tested, and had zero production callers
/// until 2026-08-29: `drain` logged the state, told the window the ring had gone out, and
/// did nothing else. A wifi roam, a NAT rebind or a relay reservation expiring silenced
/// that pair for the rest of the session while everyone else still heard both of them.
///
/// `Voice.tsx:1324` describes exactly that, at the place where 1.x fixed it -- "a player
/// who drops out of one conversation stays out of it until the app is restarted, while
/// everyone else still hears them". The port reintroduced it.
struct Repair {
    /// Whether ICE has started, so a connection that never begins can be given up on
    /// rather than waited for indefinitely.
    attempt: acl_net::peer::Attempt,
    /// What the transport last said about a link that was up.
    link: acl_net::peer::LinkState,
    /// When the current phase began. `Attempt::poll` and `RepairPolicy::poll` both take an
    /// elapsed time rather than reading a clock, which is what makes them testable.
    since: std::time::Instant,
    /// How many repairs have been attempted, counted from one.
    tries: u32,
    /// The one restart this connection is allowed.
    policy: acl_net::peer::RepairPolicy,
    /// When the next rebuild is due, once one has been scheduled.
    due: Option<std::time::Instant>,
    /// What the last attempt gathered before it was torn down.
    ///
    /// Snapshotted at the moment of failure, because tearing the connection down is what
    /// forgets it and it is the evidence the escalation decision is made from.
    /// `Voice.tsx` does the same in its `error` handler, and says why there.
    relay_candidates: Option<u32>,
}

impl Repair {
    /// A peer that has just been offered to.
    fn new() -> Self {
        Self {
            attempt: acl_net::peer::Attempt::new(),
            link: acl_net::peer::LinkState::Connected,
            since: std::time::Instant::now(),
            tries: 0,
            policy: acl_net::peer::RepairPolicy::new(),
            due: None,
            relay_candidates: None,
        }
    }

    /// Records a state the transport reported.
    fn observe(&mut self, state: webrtc::peer_connection::RTCPeerConnectionState) {
        use acl_net::peer::{Ended, LinkState};
        use webrtc::peer_connection::RTCPeerConnectionState as Reported;

        let link = match state {
            Reported::Connecting => {
                self.attempt.started();
                LinkState::Connected
            }
            Reported::Connected => {
                self.attempt.connected();
                // A fresh burst for a connection that worked. `Voice.tsx` never resets its
                // counter, so a peer that dropped once an hour ago is retried on the slow
                // forty-five-second schedule for ever after; a link that carried audio has
                // earned its fast attempts back.
                self.tries = 0;
                self.policy = acl_net::peer::RepairPolicy::new();
                self.due = None;
                LinkState::Connected
            }
            Reported::Disconnected => LinkState::Disconnected,
            Reported::Failed => {
                self.attempt.ended(Ended::Failed);
                LinkState::Failed
            }
            Reported::Closed => {
                self.attempt.ended(Ended::Closed);
                LinkState::Failed
            }
            // `New` is where every connection starts and says nothing about a live link.
            // The timeout on it belongs to `Attempt`, which is polled separately.
            _ => LinkState::Connected,
        };
        if link != self.link {
            self.link = link;
            self.since = std::time::Instant::now();
        }
    }
}

/// Long enough in one state that the state means something.
///
/// Named rather than repeated: `RepairPolicy` decides *what* on the strength of how long a
/// link has been unwell, and the tests below have to ask it about a duration past that
/// threshold without hard-coding the threshold a second time.
#[cfg(test)]
const PATIENCE: std::time::Duration = acl_net::peer::ICE_RESTART_AFTER_DISCONNECTED;

/// Tears a failed connection down and decides when, and how, to make its replacement.
///
/// Split out of [`repair`] only because it is long; it is the whole of what happens the
/// moment a link is declared dead, and the order of the four steps matters. The candidate
/// count is read before the close, the escalation is decided before the delay, and the
/// delay is armed rather than slept through.
async fn schedule_rebuild(
    mesh: &mut acl_core::peers::PeerSet,
    repair: &mut Repair,
    peer: &str,
    initiator: bool,
    anyone_relayed: bool,
    answers: &Sender<Report>,
) {
    use acl_net::reconnect::{RelaySignals, reconnect_delay, should_give_up, should_use_relay};

    // Before the connection goes, because closing it is what forgets the count.
    repair.relay_candidates = mesh.relay_candidates(peer);
    repair.tries = repair.tries.saturating_add(1);
    mesh.close(peer).await;
    let _ = answers.send(Report::Peer {
        socket_id: peer.to_owned(),
        connected: false,
    });

    // Whether to stop trying the direct path. `should_use_relay` reads the candidate count
    // first: above zero means the relay answered and the direct path failed anyway, so
    // there is nothing to learn from failing at it twice; zero means the allocation failed,
    // and forcing relay-only would leave the connection with no candidates at all.
    if !mesh.relaying(peer)
        && should_use_relay(RelaySignals {
            attempt: repair.tries,
            relay_candidates: repair.relay_candidates,
            other_peers_needed_relay: anyone_relayed,
        })
    {
        if mesh.has_relay() {
            acl_core::log_info!(
                "peer",
                "switching to the relay for {peer} after {} attempts, having gathered {:?} relay candidates",
                repair.tries,
                repair.relay_candidates
            );
            mesh.use_relay(peer);
        } else {
            acl_core::log_warn!(
                "peer",
                "the direct path to {peer} keeps failing and this server advertises no relay to fall back to"
            );
        }
    }

    // Past the fast burst the interval goes flat and long rather than stopping. Giving up
    // outright is what 1.0.4 removed: the reasons a connection cannot be made are
    // frequently not permanent -- a relay whose reservations are all taken frees one when
    // somebody leaves, and nothing was ever going to ask again.
    let wait = if should_give_up(repair.tries) {
        acl_net::reconnect::SLOW_DELAY
    } else {
        reconnect_delay(repair.tries, initiator)
    };
    acl_core::log_info!(
        "peer",
        "{peer} failed; rebuilding in {} ms, attempt {}",
        wait.as_millis(),
        repair.tries
    );
    repair.due = Some(std::time::Instant::now() + wait);
}

/// Repairs what the transport has reported broken, and gives up on what never started.
///
/// Called every round, which is at most every fifty milliseconds. Everything decided here
/// is decided by `acl_net::peer` and `acl_net::reconnect`, which are pure and tested
/// without a network; this is only the part that needs a connection to act on.
async fn repair(
    mesh: &mut Option<acl_core::peers::PeerSet>,
    repairs: &mut std::collections::BTreeMap<String, Repair>,
    session: &mut Session,
    own_id: &str,
    answers: &Sender<Report>,
) {
    use acl_net::peer::{Ended, Progress, Repair as Cure};
    use acl_net::reconnect::initiates_reconnect;

    let Some(mesh) = mesh.as_mut() else {
        return;
    };
    // Read once for the whole round: the lobby's experience of the relay is evidence about
    // every peer in it, and a peer escalated inside this loop should not change the answer
    // the peers after it get.
    let anyone_relayed = mesh.anyone_relayed();
    let now = std::time::Instant::now();

    for (peer, repair) in repairs.iter_mut() {
        // A connection that never left `new`. ICE reports nothing at all in that case, so
        // there is no state change to react to and the peer would wait for an event that
        // is not coming.
        if repair.due.is_none()
            && matches!(
                repair.attempt.poll(repair.since.elapsed()),
                Progress::GiveUp(Ended::NeverStarted)
            )
        {
            acl_core::log_warn!(
                "peer",
                "{peer} never started connecting; giving up on this attempt"
            );
            repair.attempt.ended(Ended::Failed);
            repair.link = acl_net::peer::LinkState::Failed;
            repair.since = now;
        }

        let initiator = initiates_reconnect(own_id, peer);
        match repair
            .policy
            .poll(repair.link, repair.since.elapsed(), initiator)
        {
            // The cheap repair: re-gather, keeping the connection, its tracks and its DTLS
            // session. Only the end that offers can do it, and only once -- both rules are
            // `RepairPolicy`'s and neither is repeated here.
            Cure::RestartIce => match mesh.restart_ice(peer).await {
                Ok(outbound) => {
                    acl_core::log_info!("peer", "{peer} went quiet; restarting ICE");
                    let _ = session
                        .signal(&outbound.to, outbound.payload.to_value())
                        .await;
                }
                Err(why) => acl_core::log_warn!("peer", "could not restart ICE for {peer}: {why}"),
            },
            Cure::Rebuild if repair.due.is_none() => {
                schedule_rebuild(mesh, repair, peer, initiator, anyone_relayed, answers).await;
            }
            Cure::Rebuild | Cure::None => {}
        }

        // The rebuild itself, when its delay has run out.
        if repair.due.is_some_and(|due| now >= due) {
            repair.due = None;
            repair.since = now;
            repair.attempt = acl_net::peer::Attempt::new();
            // The other end got there first. Both ends schedule -- either may be the only
            // one that noticed -- and the answering end waits `ANSWER_GRACE` longer, so in
            // the ordinary case exactly one offer is made.
            if mesh.holds(peer) {
                continue;
            }
            match mesh.rebuild(peer).await {
                Ok(outbound) => {
                    acl_core::log_info!("peer", "rebuilding the connection to {peer}");
                    let _ = session
                        .signal(&outbound.to, outbound.payload.to_value())
                        .await;
                }
                Err(why) => acl_core::log_warn!("peer", "could not rebuild {peer}: {why}"),
            }
        }
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
    repairs: &mut std::collections::BTreeMap<String, Repair>,
    session: &mut Session,
    to_mixer: &std::sync::Mutex<Sender<acl_core::peers::Incoming>>,
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
        // Every one of them, because this is the only thing that says where a handshake got
        // to. Two testers measured thirty seconds to connect and nothing in the log had a
        // word to say about it: not that an offer went out, not that an answer came back,
        // not which state ICE was sitting in. A connection walks New -> Connecting ->
        // Connected in well under a second when it works, so a line per step is a handful
        // per peer per session and tells the whole story when it does not.
        acl_core::log_info!("peer", "{peer} is now {state}");
        // The only place a link's health is learnt, so it is the only place the repair
        // bookkeeping can be kept up to date. `observe` records the phase and when it
        // started; `repair` decides what to do about it, on its own clock.
        repairs
            .entry(peer.clone())
            .or_insert_with(Repair::new)
            .observe(state);
        let _ = answers.send(Report::Peer {
            socket_id: peer,
            connected: state == webrtc::peer_connection::RTCPeerConnectionState::Connected,
        });
    }
    // Straight to the mixer. The window is told *that* a peer is speaking, which is the
    // one question `roster::Voice` asks that nothing else could answer, and nothing more:
    // a channel of audio frames into a paint loop is a queue that grows, and until
    // 2026-08-29 that is exactly what this was.
    // Locked once for the batch rather than once per packet: the only writer is a rebuild
    // of the audio pipeline, which happens when somebody changes a device setting.
    let Ok(to_mixer) = to_mixer.lock() else {
        return;
    };
    for packet in audio {
        let _ = answers.send(Report::Speaking(packet.peer.clone()));
        // A failed send means this mixer has gone. Not fatal and not a reason to stop: a
        // rebuild replaces the sender a moment later, and the packets lost in between are
        // twenty milliseconds each.
        let _ = to_mixer.send(packet);
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
        Command::Advertise { code, lobby } => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.advertise(&code, lobby));
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
            asked,
        } => {
            // How long it waited, which is the number that says whether the thirty seconds
            // two testers measured were this queue. Nothing can be signalled to anybody
            // before this lands: the server has not been told this client is in the lobby,
            // so no peer is offered to and no handshake begins. A few milliseconds here and
            // the delay is elsewhere; seconds and it was the backlog, which was fixed on
            // 2026-08-28 after being ruled out for the wrong reason.
            acl_core::log_info!(
                "lobby",
                "joining {code} after {} ms in the queue",
                asked.elapsed().as_millis()
            );
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.join(&code, player_id, client_id, is_host));
            }
        }
        Command::VoiceActivity(speaking) => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.voice_activity(speaking));
            }
        }
        Command::Identify {
            player_id,
            client_id,
        } => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.identify(player_id, client_id));
            }
        }
        Command::SetHost(client_id) => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.set_host(client_id));
            }
        }
        Command::ImpostorRadio(on_radio) => {
            if let Some(live) = session.as_mut() {
                let _ = runtime.block_on(live.impostor_radio(on_radio));
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

    use super::{Link, Repair, State};
    use acl_core::session::{Event, PublicLobby};

    use acl_net::peer::{LinkState, Repair as Cure};
    use webrtc::peer_connection::RTCPeerConnectionState as Reported;

    /// Every state the transport can report, and what it makes of a link.
    ///
    /// Until 2026-08-29 none of this ran: `acl_net::reconnect` and
    /// `acl_net::peer::RepairPolicy` were ported, tested and had no production caller, so
    /// a connection that failed stayed failed for the rest of the session. These assert
    /// the mapping the driver reads, which is the half that could get it wrong silently.
    #[test]
    fn a_failed_link_is_a_failed_link_and_a_connecting_one_is_not() {
        let mut repair = Repair::new();
        // The states a healthy handshake walks through. None of them is trouble, and a
        // repair triggered on one of them would tear down connections that were working.
        for state in [Reported::New, Reported::Connecting, Reported::Connected] {
            repair.observe(state);
            assert_eq!(
                repair.policy.poll(repair.link, super::PATIENCE, true),
                Cure::None,
                "{state} is not a fault"
            );
        }

        // `disconnected` means checks have stopped succeeding and ICE has not given up. It
        // frequently heals; only after `ICE_RESTART_AFTER_DISCONNECTED` is it worth the
        // cheap repair, and only at the end that offers.
        repair.observe(Reported::Disconnected);
        assert_eq!(repair.link, LinkState::Disconnected);
        assert_eq!(
            repair
                .policy
                .poll(repair.link, std::time::Duration::ZERO, true),
            Cure::None,
            "a moment of quiet is not a fault yet"
        );
        assert_eq!(
            repair.policy.poll(repair.link, super::PATIENCE, false),
            Cure::None,
            "the answering end cannot restart: a restart works by offering"
        );
        assert_eq!(
            repair.policy.poll(repair.link, super::PATIENCE, true),
            Cure::RestartIce
        );
        assert_eq!(
            repair.policy.poll(repair.link, super::PATIENCE, true),
            Cure::None,
            "one restart per connection, or a path that is gone re-gathers on a loop"
        );

        // `failed` is ICE giving up, and it costs the expensive repair whichever end sees
        // it. `closed` is treated the same: something ended the connection and nothing is
        // going to bring that one back.
        for state in [Reported::Failed, Reported::Closed] {
            let mut repair = Repair::new();
            repair.observe(state);
            assert_eq!(repair.link, LinkState::Failed);
            assert_eq!(
                repair
                    .policy
                    .poll(repair.link, std::time::Duration::ZERO, false),
                Cure::Rebuild,
                "{state} costs a rebuild at either end, immediately"
            );
        }
    }

    /// A link that came back gets its fast attempts back.
    #[test]
    fn a_connection_that_recovers_starts_its_burst_again() {
        let mut repair = Repair::new();
        repair.observe(Reported::Connecting);
        repair.observe(Reported::Failed);
        repair.tries = 4;
        repair.due = Some(std::time::Instant::now());

        repair.observe(Reported::Connected);
        assert_eq!(repair.tries, 0);
        assert!(repair.due.is_none(), "a scheduled rebuild is off");
        // And the restart budget with it, so the next spell of trouble may spend the cheap
        // repair rather than going straight to a rebuild.
        assert!(!repair.policy.has_restarted());

        // This is a deliberate difference from 1.x. `Voice.tsx` never resets its counter,
        // so a peer that dropped once an hour ago is retried on the flat forty-five-second
        // schedule for ever after -- and the whole point of the fast burst is the first
        // few seconds after something goes wrong.
        repair.observe(Reported::Failed);
        assert_eq!(
            acl_net::reconnect::reconnect_delay(repair.tries + 1, true),
            acl_net::reconnect::BASE_DELAY
        );
    }

    /// The clock only moves when the state does.
    #[test]
    fn a_state_repeated_does_not_restart_the_patience() {
        let mut repair = Repair::new();
        repair.observe(Reported::Disconnected);
        let first = repair.since;
        // The transport can report the same state more than once, and a link that has been
        // quiet for four seconds must stay four seconds quiet however many times it says
        // so -- otherwise the restart is postponed for as long as the trouble lasts, which
        // is exactly when it is wanted.
        repair.observe(Reported::Disconnected);
        assert_eq!(repair.since, first);
    }

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
        let (mut link, _packets) = Link::start();
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
        let (mut link, _packets) = Link::start();
        link.absorb(Event::Lobbies(vec![lobby(1, "Before")]));
        link.absorb(Event::LobbyUpdated(Box::new(lobby(1, "After"))));
        let titles: Vec<&str> = link.lobbies().map(|lobby| lobby.title.as_str()).collect();
        assert_eq!(titles, ["After"]);
    }

    /// A fresh list replaces what was there. It arrives when the browser opens and is the
    /// whole truth at that moment, so merging it would keep lobbies that have since gone.
    #[test]
    fn a_fresh_list_replaces_the_old_one() {
        let (mut link, _packets) = Link::start();
        link.absorb(Event::Lobbies(vec![lobby(1, "Old")]));
        link.absorb(Event::Lobbies(vec![lobby(2, "New")]));
        let titles: Vec<&str> = link.lobbies().map(|lobby| lobby.title.as_str()).collect();
        assert_eq!(titles, ["New"]);
    }

    /// A connection that has gone takes its lobbies with it. Leaving them on screen would
    /// offer a player a join that cannot be sent.
    #[test]
    fn losing_the_connection_empties_the_browser() {
        let (mut link, _packets) = Link::start();
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
        let (mut link, _packets) = Link::start();
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
        let (mut link, _packets) = Link::start();
        link.absorb(Event::Lobbies(vec![lobby(1, "One")]));
        link.watch_lobbies(false);
        assert_eq!(link.lobbies().count(), 0);
    }

    /// Everything else is dropped rather than queued. The voice mesh is not in this window
    /// yet, and a queue nobody reads is a leak with a long fuse.
    #[test]
    fn events_this_window_has_no_use_for_are_dropped() {
        let (mut link, _packets) = Link::start();
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
        let (mut link, _packets) = Link::start();
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

        let (mut first, _first_packets) = Link::start();
        let (mut second, _second_packets) = Link::start();
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

        let (mut speaker, _speaker_packets) = Link::start();
        let (mut listener, listener_packets) = Link::start();
        let sink = speaker.audio_sink();
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
                // Through the sink, which is what the capture callback holds. There is no
                // other way out any more: `send_audio` was removed with the paint-loop
                // path, because a second door is a second way for media to find it again.
                sink.send(packet);
            }

            speaker.pump();
            listener.pump();
            // Straight off the media channel rather than out of the window. This is the
            // receiver `Link::start` handed back, and it is the same one the mixing thread
            // holds in a real client.
            while let Ok(arrived) = listener_packets.try_recv() {
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

        let (mut speaker, _speaker_packets) = Link::start();
        let (mut listener, _listener_packets) = Link::start();
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

    /// One impostor claims the radio, and the other one hears about it.
    ///
    /// The whole point of `impostorRadio` being a socket event at all. 1.x carries the claim
    /// over the WebRTC data channel, which this client does not have -- §4.13 recorded that
    /// as blocking, on the grounds that *moving* the claim would break 1.x peers. Adding a
    /// second route breaks nobody: a 1.x client neither sends nor receives this, so a mixed
    /// lobby degrades exactly as far as it already did, and a lobby of 2.x clients gets a
    /// radio instead of none.
    ///
    /// Against a real server because the relaying is the server's, and because this is a
    /// *new* event: a test against our own loopback would prove both halves of one guess.
    #[test]
    #[ignore = "needs a server: set ACL_SERVER_BIN"]
    fn claiming_the_radio_reaches_the_other_impostor() {
        const PORT: u16 = 19_765;
        let Some(_server) = serving(PORT) else {
            eprintln!("skipping: set ACL_SERVER_BIN to the server binary");
            return;
        };
        let base = format!("http://127.0.0.1:{PORT}");

        let (mut claiming, _claiming_packets) = Link::start();
        let (mut listening, _listening_packets) = Link::start();
        claiming.connect(&base);
        listening.connect(&base);
        let both_up = wait(&mut [&mut claiming, &mut listening], |links| {
            links
                .iter()
                .all(|link| matches!(link.state(), State::Connected(_)))
        });
        assert!(both_up, "one of the sessions never connected");

        claiming.join("RADIO", 1, 11, true);
        listening.join("RADIO", 2, 22, false);
        let met = wait(&mut [&mut claiming, &mut listening], |links| {
            links[1].socket_of(11).is_some()
        });
        assert!(met, "the listener never learned the claimant's socket");

        assert_eq!(
            listening.on_radio(),
            None,
            "somebody is on the radio before anybody claimed it"
        );

        claiming.say_on_radio(true);
        let heard = wait(&mut [&mut claiming, &mut listening], |links| {
            links[1].on_radio() == Some(11)
        });
        assert!(heard, "the claim never arrived");

        // And letting go ends it. A radio that only ever switches on is an impostor
        // broadcasting to the other impostors after they thought they had stopped, which in
        // this game is the worst failure this feature has.
        claiming.say_on_radio(false);
        let released = wait(&mut [&mut claiming, &mut listening], |links| {
            links[1].on_radio().is_none()
        });
        assert!(released, "the radio stayed on after it was released");
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
    /// A round takes everything, not one thing.
    ///
    /// This is arithmetic, and it was wrong. The worker took a single command per round and
    /// then waited up to fifty milliseconds on the session, which on a quiet socket runs to
    /// its end every time: about twenty commands a second out. The microphone puts fifty a
    /// second in -- `FRAME_MS` is twenty, and every encoded frame is a `Command::SendAudio`
    /// on this channel. A queue that gains thirty entries a second sends speech at 0.4x real
    /// time and delays a join by half again as long as the client has been connected, which
    /// is what two testers measured as "slowed down" and "thirty seconds".
    ///
    /// One second of speech is the unit, because that is the rate that has to be survivable.
    #[test]
    fn it_takes_every_command_that_is_waiting() {
        let (commands, orders) = std::sync::mpsc::channel::<super::Command>();
        let a_second_of_speech = 1000 / acl_audio::codec::FRAME_MS;
        for frame in 0..a_second_of_speech {
            #[expect(clippy::cast_possible_truncation, reason = "a frame counter under 100")]
            commands
                .send(super::Command::SendAudio(vec![frame as u8]))
                .expect("the worker's end is open");
        }

        let taken = super::take_waiting(&orders, true).expect("the channel is open");
        assert_eq!(
            taken.len(),
            a_second_of_speech as usize,
            "a round left {} of {a_second_of_speech} frames in the queue, and a queue that \
             keeps some of every second is one that never empties",
            a_second_of_speech as usize - taken.len()
        );
        assert!(
            super::take_waiting(&orders, true).is_some_and(|left| left.is_empty()),
            "the next round should find nothing waiting"
        );
    }

    /// The window dropping its end stops the worker rather than spinning it.
    #[test]
    fn a_closed_channel_is_the_end_of_the_worker() {
        let (commands, orders) = std::sync::mpsc::channel::<super::Command>();
        drop(commands);
        assert!(super::take_waiting(&orders, true).is_none());
        assert!(super::take_waiting(&orders, false).is_none());
    }
}
