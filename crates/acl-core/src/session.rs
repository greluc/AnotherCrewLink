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
use acl_net::mesh::{self, Membership};
use acl_net::peer_config::PeerConfig;
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

/// Something that happened in the lobby.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The session is up, and this is the id the server addresses it by.
    Connected(String),
    /// How to reach the relay, as the server issued it for this session.
    PeerConfig(Box<PeerConfig>),
    /// Somebody is in the lobby who was not.
    ///
    /// Produced from [`mesh::Action::Connect`], so the caller is told to open a connection
    /// by the same rule whether the peer arrived one at a time or in a `setClients` batch.
    PeerJoined {
        /// Their socket id.
        socket_id: String,
        /// What the server said about them, when it said anything.
        client: Option<Client>,
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
    /// An event arrived that this build does not act on.
    ///
    /// Reported rather than dropped, because the alternative is a client that silently
    /// ignores half a protocol and a maintainer who finds out from a bug report. The lobby
    /// browser's three events land here until something wants them.
    Ignored(String),
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
            other => events.push(Event::Ignored(other.to_owned())),
        }
    }

    /// A peer config this client will not accept means no relay, which is a degradation.
    /// Refusing the whole session over it would be an outage, so the session continues.
    fn on_peer_config(args: &[Value], events: &mut Vec<Event>) {
        match args.first().map(acl_net::peer_config::validate_peer_config) {
            Some(Ok(config)) => events.push(Event::PeerConfig(Box::new(config))),
            Some(Err(complaints)) => events.push(Event::Ignored(format!(
                "clientPeerConfig rejected: {}",
                complaints.join("; ")
            ))),
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
            events.push(Event::PeerJoined {
                socket_id: peer,
                client: Some(client),
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

    fn on_voice_activity(args: &[Value], events: &mut Vec<Event>) {
        match (
            args.first().and_then(Value::as_str),
            args.get(1).and_then(Value::as_bool),
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

    use super::{Client, Event, Lobby};
    use acl_net::client::Action;
    use serde_json::{Value, json};

    /// One server event, as the client state machine would hand it over.
    fn event(name: &str, args: Vec<Value>) -> Action {
        Action::Event {
            name: name.to_owned(),
            args,
        }
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
    /// report -- the lobby browser's three events land here until something wants them.
    #[test]
    fn an_unhandled_event_is_named_rather_than_dropped() {
        let mut lobby = Lobby::new();
        let events = lobby.interpret(event("new_lobbies", vec![json!([])]));
        assert_eq!(events, vec![Event::Ignored("new_lobbies".to_owned())]);
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
