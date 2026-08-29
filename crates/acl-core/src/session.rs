//! The driver that owns a socket and a membership at once.
//!
//! §4.6 item 3 is the last thing left in P4's mesh work, and it says where it goes: the
//! relay rules, the repair policy and `mesh::Membership` are all built and tested, and
//! "what is left is the driver that owns a socket, a membership and a set of connections
//! at once, and that belongs with `acl-core` in P5 rather than here".
//!
//! This is the socket and the membership. The set of connections is the half that needs a
//! `webrtc` peer per member and belongs on top of this rather than inside it — every event
//! it would act on comes out of [`Session::next`] already.
//!
//! # What it is for
//!
//! Turning a stream of Socket.IO events into a stream of statements about a lobby. The
//! Electron client does this across four files and a React component, and the part that
//! decides anything is spread through all of them; here the deciding is `acl-net`'s, which
//! has tests, and this is the translation.
//!
//! # The wire is not this crate's to change
//!
//! Every event name and payload shape below is read from `src/socket.rs` in the server
//! repository, which `CLAUDE.md` names as the only place that knows all of them. A
//! renamed event or a reshaped payload breaks every player who has not updated, so nothing
//! here invents one.

use acl_net::client::{Action, CloseReason};
use acl_net::ice::RtcConfig;
use acl_net::mesh::{self, Membership};
use acl_net::transport::{Connection, TransportError};
use serde_json::{Value, json};

/// What the server knows about one client in the lobby.
///
/// Two numbers, and the server's own names for them. `player_id` is the in-game player,
/// `client_id` the network one; the pair is what lets a voice stream be matched to
/// somebody on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Client {
    /// The in-game player id.
    pub player_id: i64,
    /// The network client id.
    pub client_id: i64,
}

impl Client {
    /// Reads one out of whatever the server sent.
    ///
    /// Lenient about the shape and strict about nothing, because a client that refused to
    /// see a peer over an unexpected field would drop them out of the lobby entirely. A
    /// missing id reads as zero, which is what the Electron client does with `undefined`
    /// in the same position.
    fn from_value(value: &Value) -> Self {
        Self {
            player_id: value
                .get("playerId")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            client_id: value
                .get("clientId")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        }
    }
}

/// Which of the two ends offers, when a peer appears.
///
/// The asymmetry is what keeps the mesh free of glare, and it is not derivable from the
/// membership — only from *which event* the peer appeared in. `Voice.tsx` gets it from
/// having two separate handlers: `socket.on('join', ...)` calls
/// `createPeerConnection(peer, true, ...)`, and `socket.on('setClients', ...)` records the
/// clients and creates nothing at all.
///
/// This driver reported both as one event until 2026-08-26, which was wrong in a way that
/// only shows up with three people in a lobby: a caller that offered on every arrival
/// would offer to everybody already there, and every one of them would be offering back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrival {
    /// They arrived after this client did. **This end offers.**
    ///
    /// From `join`, which the server sends to everybody already in the lobby.
    Newcomer,
    /// They were already here when this client arrived. **They offer.**
    ///
    /// From `setClients`, which the server sends to the arriving client alone. Recorded so
    /// that a signal from them is accepted when it comes, and so the UI can show them
    /// before a connection exists — but nothing is offered to them.
    Incumbent,
}

/// Something that happened in the lobby.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The session is up, and this is the id the server addresses it by.
    Connected(String),
    /// How to reach the relay, as the server issued it for this session.
    PeerConfig(Box<RtcConfig>),
    /// Somebody is in the lobby who was not.
    PeerJoined {
        /// Their socket id.
        socket_id: String,
        /// What the server said about them, when it said anything.
        client: Option<Client>,
        /// Which side offers.
        arrival: Arrival,
    },
    /// Somebody is gone.
    PeerLeft {
        /// Their socket id.
        socket_id: String,
    },
    /// A peer already known changed their in-game identity.
    ///
    /// Not a join. The Electron client treats `setClient` for a known peer as an update
    /// rather than a new connection, and rebuilding the peer connection every time
    /// somebody's colour changed would be an audible gap for no reason.
    PeerChanged {
        /// Their socket id.
        socket_id: String,
        /// The new identity.
        client: Client,
    },
    /// Who the server considers the host.
    HostChanged(i64),
    /// A signal from a peer, to be routed by [`acl_net::signal_route`].
    Signal {
        /// Who sent it.
        from: String,
        /// What they sent, untouched.
        data: Value,
    },
    /// A peer claiming or releasing the impostor radio.
    ///
    /// **A 2.x event, added 2026-08-27.** 1.x carries this over the WebRTC data channel
    /// (`Voice.tsx` 913 and 1290) and this client has none by design. §4.13 records the
    /// blocker as *moving* the claim to the socket, which would break 1.x peers; adding a
    /// second route breaks nobody. A 1.x client never sends this and never receives it, so
    /// a mixed lobby degrades exactly as far as it did before -- 2.x impostors hear each
    /// other on the radio and 1.x impostors do not -- rather than 2.x having no radio at
    /// all, which is where it was.
    ImpostorRadio {
        /// Whose.
        socket_id: String,
        /// Whether they are on it.
        on_radio: bool,
    },
    /// A peer's voice activity, as the server relays it.
    VoiceActivity {
        /// Whose.
        socket_id: String,
        /// Whether they are speaking.
        speaking: bool,
    },
    /// The session ended.
    Closed(CloseReason),
    /// The server answered and refused.
    Refused(Option<Value>),
    /// The public lobbies, in full.
    ///
    /// Sent once when the browser is opened, and never again — after this the server sends
    /// only the differences.
    Lobbies(Vec<PublicLobby>),
    /// One lobby appeared or changed.
    ///
    /// The same event for both: the server emits `update_lobby` whether or not the browser
    /// has seen this id before, so the caller inserts rather than replaces.
    LobbyUpdated(Box<PublicLobby>),
    /// One lobby is gone.
    LobbyRemoved(u64),
    /// The answer to [`Session::join_lobby`].
    ///
    /// The code is what the player types into the game: this client stopped writing it
    /// into the game's memory on 2026-08-24, when the write path was removed.
    LobbyCode {
        /// The lobby code.
        code: String,
        /// Which region it is on, which the player has to be on too.
        server: String,
    },
    /// A join that was refused, with whatever the server said.
    ///
    /// Refusals are ordinary here: a lobby that filled up or started between the browser
    /// showing it and the player clicking it is the common case, not an error.
    LobbyUnavailable(String),
    /// An event arrived that this build does not act on.
    ///
    /// Reported rather than dropped, because the alternative is a client that silently
    /// ignores half a protocol and a maintainer who finds out from a bug report.
    Ignored(String),
}

/// One publicly advertised lobby, as the server sends it.
///
/// A port of `PublicLobby` in the server's `state.rs`, which is where the shape is decided.
/// Every field is read tolerantly and defaulted, for the reason the server itself gives
/// about the *other* direction: "every field is whatever the sender chose". A row missing
/// its title is a row with an empty title, not a browser that shows nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PublicLobby {
    /// The server's id for it, and what [`Session::join_lobby`] is given.
    pub id: u64,
    /// What the host called it.
    pub title: String,
    /// The host's name in the game.
    pub host: String,
    /// How many are in it.
    pub current_players: i64,
    /// How many it holds.
    pub max_players: i64,
    /// The host's chosen language tag.
    pub language: String,
    /// The mod id it advertises.
    pub mods: String,
    /// Which region it is on.
    pub server: String,
    /// The game state, as the reader's numbering has it.
    pub game_state: i64,
    /// When it entered that state, in milliseconds since the epoch.
    pub state_time: i64,
}

impl PublicLobby {
    /// Reads one row.
    ///
    /// `None` only when the value is not an object at all. Everything else defaults, so a
    /// server that adds a field or omits one this build wants costs a column rather than
    /// the browser.
    #[must_use]
    pub fn read(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let text = |key: &str| {
            object
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let number = |key: &str| object.get(key).and_then(Value::as_i64).unwrap_or_default();
        Some(Self {
            id: object.get("id").and_then(Value::as_u64).unwrap_or_default(),
            title: text("title"),
            host: text("host"),
            current_players: number("current_players"),
            max_players: number("max_players"),
            language: text("language"),
            mods: text("mods"),
            server: text("server"),
            game_state: number("gameState"),
            state_time: number("stateTime"),
        })
    }
}

/// Everything a lobby is, minus the socket.
///
/// Split out so that the interpreting can be tested, which is the whole of what this
/// module contributes: `acl-net` decides, this translates, and a translation nobody can
/// reach is a translation nobody has checked. It is also the split `acl-net` itself uses —
/// its `client` has no socket in it either.
///
/// The first version of this was methods on [`Session`], and one test then had to reach
/// the stranger rule through a real server. The server refuses cross-lobby signals itself,
/// so that test passed by timing out, having never exercised the client's check at all.
#[derive(Debug, Default)]
pub struct Lobby {
    membership: Membership,
    socket_id: Option<String>,
    code: Option<String>,
    host: Option<i64>,
}

/// A live session on a signalling server.
pub struct Session {
    connection: Connection,
    lobby: Lobby,
}

impl Lobby {
    /// Nothing joined and nothing known.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The id the server addresses this session by, once it has issued one.
    #[must_use]
    pub fn socket_id(&self) -> Option<&str> {
        self.socket_id.as_deref()
    }

    /// The lobby code this session asked for.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// Who the server last said the host is.
    #[must_use]
    pub const fn host(&self) -> Option<i64> {
        self.host
    }

    /// The peers believed to be in the lobby.
    #[must_use]
    pub const fn membership(&self) -> &Membership {
        &self.membership
    }

    /// One action from the client state machine, as lobby events.
    pub fn interpret(&mut self, action: Action) -> Vec<Event> {
        let mut events = Vec::new();
        match action {
            Action::Connected(id) => {
                self.socket_id = Some(id.clone());
                events.push(Event::Connected(id));
            }
            Action::Event { name, args } => self.on_event(&name, &args, &mut events),
            Action::Closed { reason } => events.push(Event::Closed(reason)),
            Action::Refused { reason } => events.push(Event::Refused(reason)),
            // Sending is the connection's own business, and an acknowledgement is only
            // interesting for events this client sends with one -- which is none of them.
            Action::Acked { event, args } if event == "join_lobby" => {
                Self::on_join_lobby_ack(&args, &mut events);
            }
            Action::AckExpired { event } if event == "join_lobby" => {
                // A join whose answer never came is a join that did not happen, and the
                // button that started it is waiting for something. Saying so is the whole
                // difference between "the server refused" and "nothing happened".
                events.push(Event::LobbyUnavailable(
                    "the server did not answer".to_owned(),
                ));
            }
            Action::Send(_) | Action::Acked { .. } | Action::AckExpired { .. } => {}
        }
        events
    }

    /// One server event, dispatched.
    ///
    /// One arm per event and one method behind each, because the arms are unrelated: the
    /// only thing `setClients` and `VAD` have in common is arriving on the same socket.
    fn on_event(&mut self, name: &str, args: &[Value], events: &mut Vec<Event>) {
        match name {
            "clientPeerConfig" => Self::on_peer_config(args, events),
            "join" => self.on_join(args, events),
            "left" => self.on_left(args, events),
            "setClients" => self.on_set_clients(args, events),
            "setClient" => self.on_set_client(args, events),
            "setHost" => self.on_set_host(args, events),
            "signal" => self.on_signal(args, events),
            "VAD" => Self::on_voice_activity(args, events),
            "impostorRadio" => Self::on_impostor_radio(args, events),
            "new_lobbies" => Self::on_lobbies(args, events),
            "update_lobby" => Self::on_lobby_updated(args, events),
            "remove_lobby" => Self::on_lobby_removed(args, events),
            other => events.push(Event::Ignored(other.to_owned())),
        }
    }

    /// The whole list, sent once when the browser opens.
    fn on_lobbies(args: &[Value], events: &mut Vec<Event>) {
        let Some(rows) = args.first().and_then(Value::as_array) else {
            events.push(Event::Ignored("new_lobbies with no list".to_owned()));
            return;
        };
        // A row that will not read is skipped rather than failing the batch: one malformed
        // lobby must not empty the browser.
        events.push(Event::Lobbies(
            rows.iter().filter_map(PublicLobby::read).collect(),
        ));
    }

    /// One lobby, appeared or changed.
    fn on_lobby_updated(args: &[Value], events: &mut Vec<Event>) {
        match args.first().and_then(PublicLobby::read) {
            Some(lobby) => events.push(Event::LobbyUpdated(Box::new(lobby))),
            None => events.push(Event::Ignored("update_lobby with no lobby".to_owned())),
        }
    }

    /// One lobby, gone.
    fn on_lobby_removed(args: &[Value], events: &mut Vec<Event>) {
        match args.first().and_then(Value::as_u64) {
            Some(id) => events.push(Event::LobbyRemoved(id)),
            None => events.push(Event::Ignored("remove_lobby with no id".to_owned())),
        }
    }

    /// The answer to a join.
    ///
    /// The server replies `(0, code, server, lobby)` on success and `(1, message)` on
    /// failure. Both shapes are read from a flat argument list *and* from a single array
    /// argument, because a Socket.IO acknowledgement carrying a tuple is one call away from
    /// either — and a client that reads only one of them fails silently on the other.
    fn on_join_lobby_ack(args: &[Value], events: &mut Vec<Event>) {
        let flat: &[Value] = match args.first() {
            Some(Value::Array(nested)) if args.len() == 1 => nested,
            _ => args,
        };
        let status = flat.first().and_then(Value::as_i64);
        let text = |at: usize| {
            flat.get(at)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        match status {
            Some(0) => events.push(Event::LobbyCode {
                code: text(1),
                server: text(2),
            }),
            // Anything that is not success is a refusal, including a status this build does
            // not know: the server has one success value and reserves the rest.
            Some(_) => events.push(Event::LobbyUnavailable(text(1))),
            None => events.push(Event::Ignored(
                "join_lobby answered without a status".to_owned(),
            )),
        }
    }

    /// A peer config this client will not accept means no relay, which is a degradation.
    /// Refusing the whole session over it would be an outage, so the session continues.
    ///
    /// `apply_client_peer_config` rather than `validate_peer_config`, and that is a fix of
    /// 2026-08-29. The first applies relay rule three -- a configuration that forces relay
    /// mode with no relay advertised is refused, because gathering nothing at all fails
    /// harder than the direct attempt it replaced. The second checks the shape and not the
    /// rule, and it was what this called; `apply_client_peer_config` had no callers at all,
    /// so the rule was tested and bypassed.
    ///
    /// A refused configuration leaves the client on its defaults, which is what
    /// `Voice.tsx:968-971` does with the same combination: it logs and returns, keeping
    /// `DEFAULT_ICE_CONFIG`.
    fn on_peer_config(args: &[Value], events: &mut Vec<Event>) {
        use acl_net::peer_config::Rejection;
        match args
            .first()
            .map(acl_net::peer_config::apply_client_peer_config)
        {
            Some(Ok(config)) => events.push(Event::PeerConfig(Box::new(config))),
            Some(Err(Rejection::Malformed(complaints))) => events.push(Event::Ignored(format!(
                "clientPeerConfig rejected: {}",
                complaints.join("; ")
            ))),
            Some(Err(Rejection::RelayForcedWithoutRelay)) => events.push(Event::Ignored(
                "clientPeerConfig asks for relay-only and advertises no relay, which would \
                 gather no candidates at all; keeping the default configuration"
                    .to_owned(),
            )),
            None => events.push(Event::Ignored(
                "clientPeerConfig with no payload".to_owned(),
            )),
        }
    }

    fn on_join(&mut self, args: &[Value], events: &mut Vec<Event>) {
        let Some(socket_id) = args.first().and_then(Value::as_str) else {
            events.push(Event::Ignored("join with no socket id".to_owned()));
            return;
        };
        let client = args.get(1).map(Client::from_value);
        if let Some(mesh::Action::Connect(peer)) = self.membership.join(socket_id) {
            events.push(Event::PeerJoined {
                socket_id: peer,
                client,
                arrival: Arrival::Newcomer,
            });
        }
    }

    fn on_left(&mut self, args: &[Value], events: &mut Vec<Event>) {
        let Some(socket_id) = args.first().and_then(Value::as_str) else {
            events.push(Event::Ignored("left with no socket id".to_owned()));
            return;
        };
        if let Some(mesh::Action::Disconnect(peer)) = self.membership.left(socket_id) {
            events.push(Event::PeerLeft { socket_id: peer });
        }
    }

    /// The whole lobby at once, so the membership is reconciled rather than added to.
    ///
    /// A `setClients` that omits somebody is the server saying they are gone; treating it
    /// as a list of arrivals would leave a connection open to a peer nobody else can see.
    fn on_set_clients(&mut self, args: &[Value], events: &mut Vec<Event>) {
        let Some(clients) = args.first().and_then(Value::as_object) else {
            events.push(Event::Ignored("setClients with no map".to_owned()));
            return;
        };
        let ids: Vec<&str> = clients.keys().map(String::as_str).collect();
        for action in self.membership.reconcile(ids) {
            events.push(match action {
                mesh::Action::Connect(peer) => {
                    let client = clients.get(&peer).map(Client::from_value);
                    Event::PeerJoined {
                        socket_id: peer,
                        client,
                        arrival: Arrival::Incumbent,
                    }
                }
                mesh::Action::Disconnect(peer) => Event::PeerLeft { socket_id: peer },
            });
        }
    }

    /// A known peer changed; an unknown one arrived.
    ///
    /// The server sends this for both, and the difference decides whether a connection is
    /// rebuilt. Rebuilding every time somebody's colour changed would be an audible gap
    /// for no reason.
    fn on_set_client(&mut self, args: &[Value], events: &mut Vec<Event>) {
        let Some(socket_id) = args.first().and_then(Value::as_str) else {
            events.push(Event::Ignored("setClient with no socket id".to_owned()));
            return;
        };
        let client = args.get(1).map(Client::from_value).unwrap_or_default();
        if self.membership.knows(socket_id) {
            events.push(Event::PeerChanged {
                socket_id: socket_id.to_owned(),
                client,
            });
        } else if let Some(mesh::Action::Connect(peer)) = self.membership.join(socket_id) {
            // `setClient` for somebody unknown is the server catching this client up on a
            // peer it had not heard of, not an announcement that they have just arrived.
            // Offering to them would race the offer they are already making.
            events.push(Event::PeerJoined {
                socket_id: peer,
                client: Some(client),
                arrival: Arrival::Incumbent,
            });
        }
    }

    fn on_set_host(&mut self, args: &[Value], events: &mut Vec<Event>) {
        let Some(host) = args.first().and_then(Value::as_i64) else {
            events.push(Event::Ignored("setHost with no id".to_owned()));
            return;
        };
        self.host = Some(host);
        events.push(Event::HostChanged(host));
    }

    fn on_signal(&mut self, args: &[Value], events: &mut Vec<Event>) {
        let Some(payload) = args.first() else {
            events.push(Event::Ignored("signal with no payload".to_owned()));
            return;
        };
        let Some(from) = payload.get("from").and_then(Value::as_str) else {
            events.push(Event::Ignored("signal with no sender".to_owned()));
            return;
        };
        // Refused before it is looked at, and by the rule §4.6 names rather than by one
        // written here: a signal from a socket that is not in this lobby is how a stranger
        // gets a peer connection out of a client.
        let known: Vec<&str> = self.membership.peers().collect();
        if !acl_net::peer::accepts_signal_from(&known, from) {
            events.push(Event::Ignored(format!("a signal from a stranger: {from}")));
            return;
        }
        events.push(Event::Signal {
            from: from.to_owned(),
            data: payload.get("data").cloned().unwrap_or(Value::Null),
        });
    }

    /// Reads the server's `impostorRadio`.
    ///
    /// The same shape as `VAD`, deliberately: it is the same kind of message -- one peer,
    /// one boolean, relayed to the lobby -- and giving it a second shape would be two
    /// parsers to keep in step for no reason.
    fn on_impostor_radio(args: &[Value], events: &mut Vec<Event>) {
        let Some(payload) = args.first() else {
            events.push(Event::Ignored("impostorRadio with no payload".to_owned()));
            return;
        };
        match (
            payload.get("socketId").and_then(Value::as_str),
            payload.get("onRadio").and_then(Value::as_bool),
        ) {
            (Some(socket_id), Some(on_radio)) => events.push(Event::ImpostorRadio {
                socket_id: socket_id.to_owned(),
                on_radio,
            }),
            _ => events.push(Event::Ignored(
                "impostorRadio with an unexpected shape".to_owned(),
            )),
        }
    }

    /// Reads the server's `VAD`.
    ///
    /// **One object, not two positional arguments — corrected 2026-08-27.** This read
    /// `args[0]` as a socket id string and `args[1]` as a boolean, which is a shape the
    /// server has never sent. `src/socket.rs` builds
    /// `{"activity": bool, "client": i64, "socketId": String}` and delivers that.
    ///
    /// So every relayed `VAD` became `Event::Ignored` and no speaking indicator ever lit up.
    /// Nothing failed: an ignored event is an ordinary outcome, the tests here asserted the
    /// shape this code invented, and the only way to find it was to put two clients either
    /// side of the real server and watch one fail to see the other.
    ///
    /// `client` is in the payload and is not read. The socket id is what every other event
    /// keys on and what `Link` already maps to a client id; taking the server's `client`
    /// here would be a second route to the same answer, and two routes disagree eventually.
    fn on_voice_activity(args: &[Value], events: &mut Vec<Event>) {
        let Some(payload) = args.first() else {
            events.push(Event::Ignored("VAD with no payload".to_owned()));
            return;
        };
        match (
            payload.get("socketId").and_then(Value::as_str),
            payload.get("activity").and_then(Value::as_bool),
        ) {
            (Some(socket_id), Some(speaking)) => events.push(Event::VoiceActivity {
                socket_id: socket_id.to_owned(),
                speaking,
            }),
            _ => events.push(Event::Ignored("VAD with an unexpected shape".to_owned())),
        }
    }
}

impl Session {
    /// Connects to a server. Nothing is joined yet.
    ///
    /// # Errors
    ///
    /// [`TransportError`] if the socket cannot be opened.
    pub async fn connect(base: &str) -> Result<Self, TransportError> {
        Ok(Self {
            connection: Connection::connect(base).await?,
            lobby: Lobby::new(),
        })
    }

    /// What this session knows about the lobby it is in.
    #[must_use]
    pub const fn lobby(&self) -> &Lobby {
        &self.lobby
    }

    /// The id the server addresses this session by, once it has issued one.
    #[must_use]
    pub fn socket_id(&self) -> Option<&str> {
        self.lobby.socket_id()
    }

    /// The peers believed to be in the lobby.
    #[must_use]
    pub const fn membership(&self) -> &Membership {
        self.lobby.membership()
    }

    /// Joins a lobby.
    ///
    /// The four arguments and their order are the server's `on_join`, which takes
    /// `(code, player_id, client_id, is_host)` and disconnects a socket that sends
    /// anything else. There is no negotiation and no version field on this event.
    ///
    /// # Errors
    ///
    /// [`TransportError`] if the frame cannot be written.
    pub async fn join(
        &mut self,
        code: &str,
        player_id: i64,
        client_id: i64,
        is_host: bool,
    ) -> Result<(), TransportError> {
        self.lobby.code = Some(code.to_owned());
        self.emit(
            "join",
            vec![
                json!(code),
                json!(player_id),
                json!(client_id),
                json!(is_host),
            ],
        )
        .await
    }

    /// Sends a signal to one peer.
    ///
    /// # Errors
    ///
    /// [`TransportError`] if the frame cannot be written.
    pub async fn signal(&mut self, to: &str, data: Value) -> Result<(), TransportError> {
        self.emit("signal", vec![json!({ "to": to, "data": data })])
            .await
    }

    /// Advertises this lobby to the browser, or updates what it advertises.
    ///
    /// The payload is passed through rather than typed, because it is the *other* direction
    /// of `PublicLobby` and the server sanitises every field of it: "every field is
    /// optional and every field is whatever the sender chose, so each one is coerced rather
    /// than trusted". A type here would be a second opinion about a shape the server
    /// already refuses to trust, and `isPublic` false is how a lobby is withdrawn.
    ///
    /// # Errors
    ///
    /// [`TransportError`] if the frame cannot be written.
    pub async fn advertise(&mut self, code: &str, lobby: Value) -> Result<(), TransportError> {
        self.emit("lobby", vec![json!(code), lobby]).await
    }

    /// Opens or closes the public lobby browser.
    ///
    /// Opening puts this session in the server's browser room and makes it send the whole
    /// list at once; closing takes it out again. Closing matters: a session left watching
    /// receives every change to every public lobby for as long as it is connected, which
    /// is traffic nobody is looking at.
    ///
    /// # Errors
    ///
    /// [`TransportError`] if the frame cannot be written.
    pub async fn watch_lobbies(&mut self, open: bool) -> Result<(), TransportError> {
        self.emit("lobbybrowser", vec![json!(open)]).await
    }

    /// Asks for one lobby's code.
    ///
    /// The answer arrives as [`Event::LobbyCode`] or [`Event::LobbyUnavailable`], not as a
    /// return value: it is an acknowledgement, which comes back through the same stream as
    /// everything else.
    ///
    /// # Errors
    ///
    /// [`TransportError`] if the frame cannot be written.
    pub async fn join_lobby(&mut self, id: u64) -> Result<(), TransportError> {
        self.connection
            .emit("join_lobby", vec![json!(id)], true)
            .await
            .map(drop)
    }

    /// Corrects this client's in-game identity after the join.
    ///
    /// `id` is one of the eleven events the server registers a handler for, and this client
    /// never sent it. It exists because `join` carries an identity snapshotted at the
    /// moment the lobby code appeared -- and the code appears *before* the local player's
    /// record does: `InnerNetClient`'s `GameState` flips first, and the reader falls back
    /// to `-1` for a player it cannot see yet.
    ///
    /// A player who crossed that edge early was therefore announced to the whole lobby as
    /// client `-1`, which matches nobody, so `socket_of` found no socket for them and they
    /// were placed nowhere and heard by no one. For the rest of the lobby. `Voice.tsx`
    /// sends `id` whenever its own ids change, which is what makes the join's snapshot
    /// survivable.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] if the socket fails.
    pub async fn identify(&mut self, player_id: i64, client_id: i64) -> Result<(), TransportError> {
        self.connection
            .emit("id", vec![json!(player_id), json!(client_id)], false)
            .await
            .map(drop)
    }

    /// Claims the game host's role for this client.
    ///
    /// Sent when this player *becomes* the host, which is not the same moment as joining.
    /// `join` already carries `is_host`, so the case this covers is the one that happens
    /// later: the host leaves, Among Us promotes somebody, and the server is still routing
    /// host-dependent decisions to a socket that is gone.
    ///
    /// `Voice.tsx` line 827 does exactly this, on `gameState.isHost` changing. This client
    /// did not, and the gap is invisible from either end -- the server accepts the claim it
    /// never receives, and the client believes it made one.
    ///
    /// Nothing happens without a lobby: the server refuses a `setHost` for a lobby the
    /// socket is not in, so sending one before joining is noise it would log.
    ///
    /// # Errors
    ///
    /// [`TransportError`] if the frame cannot be written.
    pub async fn set_host(&mut self, client_id: i64) -> Result<(), TransportError> {
        let Some(code) = self.lobby.code.clone() else {
            return Ok(());
        };
        self.emit("setHost", vec![json!(code), json!(client_id)])
            .await
    }

    /// Claims or releases the impostor radio.
    ///
    /// Sent on the transition, like `voice_activity`: it is a level, and a message per
    /// frame while somebody holds a key would be more traffic than the audio.
    ///
    /// Whether the sender is *allowed* to is not decided here. Being an impostor, being
    /// alive and the lobby permitting it are the caller's checks, and the receiver applies
    /// its own -- `voice_params` only lifts the distance rule when both ends are impostors.
    /// A client that lied would be believed by nobody.
    ///
    /// # Errors
    ///
    /// [`TransportError`] if the frame cannot be written.
    pub async fn impostor_radio(&mut self, on_radio: bool) -> Result<(), TransportError> {
        self.emit("impostorRadio", vec![json!(on_radio)]).await
    }

    /// Tells the lobby whether this player is speaking.
    ///
    /// The other half of `Event::VoiceActivity`, which this session has parsed since it was
    /// written and nothing could produce. A client that only listens sees everyone else's
    /// speaking indicator and lights up nobody else's, which looks like everybody else being
    /// quiet rather than like a missing feature.
    ///
    /// # Not sent per frame
    ///
    /// The caller sends this on a *transition*. Speech is fifty frames a second and this is
    /// a socket message to every peer in the lobby; at that rate it would be more traffic
    /// than the audio. `Vad`'s hangover is what turns a level into something with edges
    /// worth reporting.
    ///
    /// # Errors
    ///
    /// [`TransportError`] if the frame cannot be written.
    pub async fn voice_activity(&mut self, speaking: bool) -> Result<(), TransportError> {
        self.emit("VAD", vec![json!(speaking)]).await
    }

    /// Leaves the lobby, staying connected.
    ///
    /// # Errors
    ///
    /// [`TransportError`] if the frame cannot be written.
    pub async fn leave(&mut self) -> Result<(), TransportError> {
        self.lobby.code = None;
        // Cleared here rather than on the server's say-so: leaving is this end's decision,
        // and the server sends nothing back to confirm it. A membership left standing
        // would have the caller holding connections to a lobby it is no longer in.
        self.lobby.membership.clear();
        self.emit("leave", Vec::new()).await
    }

    /// Writes one event to the socket.
    ///
    /// A `false` from the transport means the session is not live yet, so there is no
    /// socket id for the server to attribute the event to. That is the client state
    /// machine's decision and not this layer's to second-guess -- it is reported as `Ok`,
    /// because nothing failed.
    async fn emit(&mut self, name: &str, args: Vec<Value>) -> Result<(), TransportError> {
        self.connection.emit(name, args, false).await.map(drop)
    }

    /// The next batch of things that happened.
    ///
    /// `None` once the session is over, which is the same honest report the reconnect
    /// policy is built on.
    ///
    /// **A session nobody awaits is a session that dies.** The Engine.IO heartbeat is
    /// answered from inside this call, so a caller that stops polling stops replying, and
    /// the server drops the socket when the deadline passes. Anything holding two of these
    /// has to drive both — `tests/session_conformance.rs` learned that the expensive way,
    /// by waiting on one while the server disconnected the other and then reporting the
    /// abandoned client's signal as coming from a stranger.
    pub async fn next(&mut self) -> Option<Vec<Event>> {
        let actions = self.connection.next().await?;
        Some(
            actions
                .into_iter()
                .flat_map(|action| self.lobby.interpret(action))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Arrival, Client, Event, Lobby};
    use acl_net::client::Action;
    use serde_json::{Value, json};

    /// One server event, as the client state machine would hand it over.
    fn event(name: &str, args: Vec<Value>) -> Action {
        Action::Event {
            name: name.to_owned(),
            args,
        }
    }

    /// The whole list, as the server sends it when the browser opens: one array argument.
    /// Confirmed against the real server by `the_browser_sees_a_lobby_and_gets_its_code`.
    #[test]
    fn the_list_arrives_as_one_array() {
        let mut lobby = Lobby::new();
        let events = lobby.interpret(event(
            "new_lobbies",
            vec![json!([
                {"id": 7, "title": "One", "host": "Red", "current_players": 3,
                 "max_players": 10, "language": "en", "mods": "NONE", "server": "eu",
                 "gameState": 0, "stateTime": 1_700_000_000_000_i64},
                {"id": 8, "title": "Two", "host": "Blue", "current_players": 10,
                 "max_players": 10, "language": "de", "mods": "TOWN_OF_US", "server": "na",
                 "gameState": 1, "stateTime": 1_700_000_000_001_i64}
            ])],
        ));
        let [Event::Lobbies(lobbies)] = events.as_slice() else {
            panic!("expected one list, got {events:?}");
        };
        assert_eq!(lobbies.len(), 2);
        assert_eq!(lobbies[0].id, 7);
        assert_eq!(lobbies[0].title, "One");
        assert_eq!(lobbies[1].mods, "TOWN_OF_US");
        assert_eq!(lobbies[1].game_state, 1);
        assert_eq!(lobbies[1].state_time, 1_700_000_000_001);
    }

    /// One malformed row must not empty the browser. The rest of the list is still a list.
    #[test]
    fn one_bad_row_costs_that_row_and_nothing_else() {
        let mut lobby = Lobby::new();
        let events = lobby.interpret(event(
            "new_lobbies",
            vec![json!([{"id": 7, "title": "One"}, 42, "not a lobby", null])],
        ));
        let [Event::Lobbies(lobbies)] = events.as_slice() else {
            panic!("expected one list, got {events:?}");
        };
        assert_eq!(lobbies.len(), 1, "the good row survived alone");
        assert_eq!(lobbies[0].id, 7);
        // And its missing fields are empty rather than absent, so the table still has a
        // row rather than a hole.
        assert_eq!(lobbies[0].host, "");
        assert_eq!(lobbies[0].max_players, 0);
    }

    /// An update and a removal, which is everything that happens after the first list.
    #[test]
    fn a_lobby_can_change_and_go_away() {
        let mut lobby = Lobby::new();
        let events = lobby.interpret(event(
            "update_lobby",
            vec![json!({"id": 7, "title": "One", "current_players": 4})],
        ));
        let [Event::LobbyUpdated(updated)] = events.as_slice() else {
            panic!("expected an update, got {events:?}");
        };
        assert_eq!(updated.id, 7);
        assert_eq!(updated.current_players, 4);

        assert_eq!(
            lobby.interpret(event("remove_lobby", vec![json!(7)])),
            vec![Event::LobbyRemoved(7)]
        );
    }

    /// Anything the browser events arrive without is reported rather than guessed at.
    #[test]
    fn a_browser_event_with_nothing_in_it_says_so() {
        let mut lobby = Lobby::new();
        for (name, args) in [
            ("new_lobbies", vec![]),
            ("new_lobbies", vec![json!("not a list")]),
            ("update_lobby", vec![]),
            ("update_lobby", vec![json!(7)]),
            ("remove_lobby", vec![]),
            ("remove_lobby", vec![json!("seven")]),
        ] {
            let events = lobby.interpret(event(name, args));
            assert!(
                matches!(events.as_slice(), [Event::Ignored(_)]),
                "{name} gave {events:?}"
            );
        }
    }

    /// The join acknowledgement, in both shapes it can arrive in.
    ///
    /// A Socket.IO acknowledgement carrying a tuple is one `ack.send` away from being a
    /// flat argument list or a single array, and a client that reads only one of them fails
    /// silently on the other. The real server sends the flat form — confirmed by the
    /// conformance test — and this reads both, because the cost of the second is a match
    /// arm.
    #[test]
    fn the_join_answer_is_read_in_either_shape() {
        let mut lobby = Lobby::new();
        let flat = lobby.interpret(Action::Acked {
            event: "join_lobby".to_owned(),
            args: vec![json!(0), json!("ABCDEF"), json!("eu"), json!({})],
        });
        let nested = lobby.interpret(Action::Acked {
            event: "join_lobby".to_owned(),
            args: vec![json!([0, "ABCDEF", "eu", {}])],
        });
        let expected = vec![Event::LobbyCode {
            code: "ABCDEF".to_owned(),
            server: "eu".to_owned(),
        }];
        assert_eq!(flat, expected);
        assert_eq!(nested, expected, "the tuple form was not read");
    }

    /// A refusal is ordinary: a lobby that filled up or started between the browser showing
    /// it and the player clicking it is the common case, not an error.
    #[test]
    fn a_refused_join_carries_what_the_server_said() {
        let mut lobby = Lobby::new();
        assert_eq!(
            lobby.interpret(Action::Acked {
                event: "join_lobby".to_owned(),
                args: vec![json!(1), json!("Lobby is not public anymore")],
            }),
            vec![Event::LobbyUnavailable(
                "Lobby is not public anymore".to_owned()
            )]
        );
        // A status this build does not know is a refusal too: the server has one success
        // value and reserves the rest.
        assert!(matches!(
            lobby
                .interpret(Action::Acked {
                    event: "join_lobby".to_owned(),
                    args: vec![json!(99), json!("something newer")],
                })
                .as_slice(),
            [Event::LobbyUnavailable(_)]
        ));
    }

    /// An answer that never came is not the same as a refusal, and is not silence either:
    /// the button that started the join is waiting for something.
    #[test]
    fn a_join_that_is_never_answered_still_answers_the_caller() {
        let mut lobby = Lobby::new();
        assert!(matches!(
            lobby
                .interpret(Action::AckExpired {
                    event: "join_lobby".to_owned(),
                })
                .as_slice(),
            [Event::LobbyUnavailable(_)]
        ));
    }

    /// An acknowledgement for something else is not a lobby answer. `join` is acknowledged
    /// by nothing today, but reading every ack as a join answer is the kind of thing that
    /// works until one is added.
    #[test]
    fn an_acknowledgement_for_something_else_is_not_a_lobby_answer() {
        let mut lobby = Lobby::new();
        assert!(
            lobby
                .interpret(Action::Acked {
                    event: "join".to_owned(),
                    args: vec![json!(0), json!("ABCDEF")],
                })
                .is_empty()
        );
    }

    /// A lobby with one peer already in it.
    fn with_one_peer(peer: &str) -> Lobby {
        let mut lobby = Lobby::new();
        lobby.interpret(Action::Connected("me".to_owned()));
        lobby.interpret(event("join", vec![json!(peer), json!({"playerId": 1})]));
        lobby
    }

    /// The two ids come out of the server's own field names, and getting either wrong
    /// matches a voice stream to the wrong person on screen.
    #[test]
    fn a_client_is_two_ids_by_the_names_the_server_uses() {
        let client = Client::from_value(&json!({"playerId": 3, "clientId": 91}));
        assert_eq!(
            client,
            Client {
                player_id: 3,
                client_id: 91
            }
        );
    }

    /// A field that is not there reads as zero rather than as a reason to drop the peer.
    /// A client that refused to see somebody over a missing id would take them out of the
    /// lobby entirely, which is a worse answer than a wrong number.
    #[test]
    fn a_missing_id_is_zero_and_not_a_refusal() {
        assert_eq!(Client::from_value(&json!({})), Client::default());
        assert_eq!(
            Client::from_value(&json!({"clientId": 7})),
            Client {
                player_id: 0,
                client_id: 7
            }
        );
    }

    /// A signal from a socket that is not in this lobby is how a stranger gets a peer
    /// connection out of a client.
    ///
    /// The rule is `acl_net::peer::accepts_signal_from` and it is tested there; what is
    /// checked here is that this driver asks it, because the way that goes wrong is not a
    /// wrong answer but a call nobody made. It is a unit test and not a conformance one on
    /// purpose: the server refuses cross-lobby signals itself, so a test that went through
    /// a real server passed by timing out and never reached this code at all.
    #[test]
    fn a_signal_from_a_stranger_never_reaches_the_caller() {
        let mut lobby = with_one_peer("friend");
        let events = lobby.interpret(event(
            "signal",
            vec![json!({"from": "stranger", "data": {"type": "offer"}})],
        ));
        assert!(
            matches!(&events[..], [Event::Ignored(reason)] if reason.contains("stranger")),
            "expected a refusal, got {events:?}"
        );
    }

    #[test]
    fn a_signal_from_a_member_arrives_untouched() {
        let mut lobby = with_one_peer("friend");
        let events = lobby.interpret(event(
            "signal",
            vec![json!({"from": "friend", "data": {"type": "offer", "sdp": "v=0"}})],
        ));
        match &events[..] {
            [Event::Signal { from, data }] => {
                assert_eq!(from, "friend");
                assert_eq!(data["sdp"], "v=0");
            }
            other => panic!("expected one signal, got {other:?}"),
        }
    }

    /// Which side offers is the whole reason a three-person lobby works.
    ///
    /// A peer announced by `join` arrived after this client, so this end offers. A peer
    /// listed in `setClients` was already here, so they offer. Reported as one event, a
    /// caller would offer to everybody already in the lobby while every one of them
    /// offered back — which is glare with as many peers as there are people.
    #[test]
    fn who_offers_depends_on_which_event_the_peer_arrived_in() {
        let mut lobby = Lobby::new();
        let listed = lobby.interpret(event(
            "setClients",
            vec![json!({"already-here": {"playerId": 1}})],
        ));
        assert!(
            matches!(
                &listed[..],
                [Event::PeerJoined {
                    arrival: Arrival::Incumbent,
                    ..
                }]
            ),
            "somebody already in the lobby must not be offered to: {listed:?}"
        );

        let announced = lobby.interpret(event("join", vec![json!("newcomer"), json!({})]));
        assert!(
            matches!(
                &announced[..],
                [Event::PeerJoined {
                    arrival: Arrival::Newcomer,
                    ..
                }]
            ),
            "somebody who has just arrived must be offered to: {announced:?}"
        );
    }

    /// `setClients` is the whole lobby, so somebody it omits has gone. Treating it as a
    /// list of arrivals leaves a connection open to a peer nobody else can see.
    #[test]
    fn set_clients_removes_as_well_as_adds() {
        let mut lobby = with_one_peer("first");
        let events = lobby.interpret(event(
            "setClients",
            vec![json!({"second": {"playerId": 2, "clientId": 902}})],
        ));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::PeerJoined { socket_id, .. } if socket_id == "second")),
            "the new peer was not reported: {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::PeerLeft { socket_id } if socket_id == "first")),
            "the departed peer was not reported: {events:?}"
        );
        assert!(!lobby.membership().knows("first"));
        assert!(lobby.membership().knows("second"));
    }

    /// A `setClient` for somebody already known is a change of identity, not an arrival.
    /// Rebuilding the connection every time a colour changed would be an audible gap.
    #[test]
    fn set_client_for_a_known_peer_is_a_change_and_not_a_join() {
        let mut lobby = with_one_peer("friend");
        let events = lobby.interpret(event(
            "setClient",
            vec![json!("friend"), json!({"playerId": 5, "clientId": 55})],
        ));
        assert!(
            matches!(
                &events[..],
                [Event::PeerChanged { socket_id, client }]
                    if socket_id == "friend" && client.player_id == 5
            ),
            "expected a change, got {events:?}"
        );
    }

    /// And for somebody unknown it is an arrival, because the server sends it for both.
    #[test]
    fn set_client_for_an_unknown_peer_is_a_join() {
        let mut lobby = Lobby::new();
        let events = lobby.interpret(event(
            "setClient",
            vec![json!("newcomer"), json!({"playerId": 5, "clientId": 55})],
        ));
        assert!(
            matches!(&events[..], [Event::PeerJoined { socket_id, .. }] if socket_id == "newcomer"),
            "expected a join, got {events:?}"
        );
    }

    /// Every event this build does not act on still says so. The alternative is a client
    /// that silently ignores half a protocol and a maintainer who finds out from a bug
    /// report.
    ///
    /// This used to be checked with `new_lobbies`, which was the honest example while the
    /// lobby browser's three events were unhandled. They are handled now, so it is checked
    /// with a name the server does not send at all -- which is what the arm is actually
    /// for.
    #[test]
    fn an_unhandled_event_is_named_rather_than_dropped() {
        let mut lobby = Lobby::new();
        let events = lobby.interpret(event("somethingNewer", vec![json!([])]));
        assert_eq!(events, vec![Event::Ignored("somethingNewer".to_owned())]);
    }

    /// The `VAD` the server actually sends, in the shape it actually sends it.
    ///
    /// Built from `src/socket.rs` in the server repository rather than from what this side
    /// expected. That distinction is the whole of this test: the parser read two positional
    /// arguments until 2026-08-27 and the server has always sent one object, so every
    /// relayed `VAD` became `Ignored` — an ordinary outcome, no error anywhere, and a
    /// speaking indicator that never lit up for anybody in either window.
    ///
    /// The tests around it all passed, because they asserted the shape this code invented.
    /// It took two clients either side of a real server, watching one fail to see the other.
    #[test]
    fn the_vad_the_server_sends_is_understood() {
        let mut lobby = Lobby::new();
        let events = lobby.interpret(event(
            "VAD",
            vec![json!({"activity": true, "client": 42, "socketId": "abc123"})],
        ));
        assert_eq!(
            events,
            vec![Event::VoiceActivity {
                socket_id: "abc123".to_owned(),
                speaking: true,
            }]
        );

        // And the other edge, which is the one that matters more: a level that only ever
        // arrives leaves an indicator on for the rest of the lobby, which reads as a peer
        // whose microphone is stuck open.
        let events = lobby.interpret(event(
            "VAD",
            vec![json!({"activity": false, "client": 42, "socketId": "abc123"})],
        ));
        assert_eq!(
            events,
            vec![Event::VoiceActivity {
                socket_id: "abc123".to_owned(),
                speaking: false,
            }]
        );
    }

    /// And everything the server sends is handled, which is what says the `Ignored` arm is
    /// a backstop rather than where half the protocol goes.
    ///
    /// **Eleven, not twelve.** `CLAUDE.md` listed `lobbybrowser` as a server-to-client
    /// event and the server never emits it: `const BROWSER_ROOM: &str = "lobbybrowser"` is
    /// a *room name*, and the only handler for that string is the one this client calls
    /// through `watch_lobbies`. This test found it, by failing on an event that cannot
    /// arrive.
    #[test]
    fn every_event_the_server_sends_is_acted_on() {
        let sent_by_the_server = [
            "join",
            "left",
            "signal",
            "setHost",
            "setClient",
            "setClients",
            "clientPeerConfig",
            "VAD",
            "new_lobbies",
            "update_lobby",
            "remove_lobby",
        ];
        for name in sent_by_the_server {
            let mut lobby = Lobby::new();
            let events = lobby.interpret(event(name, vec![]));
            // Every one of them either does something or complains about the empty
            // payload. What none of them may do is come back as `Ignored(name)`, which is
            // this build saying it has never heard of the event.
            assert_ne!(
                events,
                vec![Event::Ignored(name.to_owned())],
                "{name} is not handled at all"
            );
        }
    }

    /// A malformed event is reported and does not take the session with it. Every one of
    /// these arrives from a server this client did not write.
    #[test]
    fn a_payload_of_the_wrong_shape_is_reported_and_survived() {
        let mut lobby = Lobby::new();
        for (name, args) in [
            ("join", vec![json!(7)]),
            ("left", vec![]),
            ("setClients", vec![json!("not a map")]),
            ("setHost", vec![json!("not a number")]),
            ("signal", vec![json!({"data": {}})]),
            ("VAD", vec![json!("someone")]),
        ] {
            let events = lobby.interpret(event(name, args));
            assert!(
                matches!(&events[..], [Event::Ignored(_)]),
                "{name} should have been reported as ignored, got {events:?}"
            );
        }
    }

    #[test]
    fn the_host_is_remembered_as_well_as_reported() {
        let mut lobby = Lobby::new();
        assert_eq!(lobby.host(), None);
        let events = lobby.interpret(event("setHost", vec![json!(42)]));
        assert_eq!(events, vec![Event::HostChanged(42)]);
        assert_eq!(lobby.host(), Some(42));
    }
}
