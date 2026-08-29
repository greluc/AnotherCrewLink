//! What the client does with the ICE servers a server advertises.
//!
//! A straight port of `src/renderer/iceServers.ts` and its tests. It is the piece most
//! likely to decide whether a player on a restrictive network can be heard at all, and it
//! carries three of the four relay rules §4.6 names — one allocation per connection, do
//! not force relay-only without a relay in hand, and bundle so a peer costs one
//! allocation rather than two. The fourth, that a refusal is temporary, belongs to the
//! peer that sees the refusal.
//!
//! All of it is decided before any transport exists, which is why it is here and not
//! inside the `webrtc` layer: the numbers are what went wrong in 1.0.4, and they are
//! testable without a network.

/// One ICE server, as a server advertises it.
///
/// `urls` is a list even when the server sent a bare string. The wire format allows
/// either and the difference decides nothing, so it is normalised on the way in and every
/// rule below reads one shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IceServer {
    /// The URLs this entry offers, in the order the server gave them.
    pub urls: Vec<String>,
    /// The username for a relay. Absent for STUN.
    pub username: Option<String>,
    /// The credential for a relay. Absent for STUN.
    pub credential: Option<String>,
}

impl IceServer {
    /// A server offering one URL and no credentials, which is the STUN shape.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            urls: vec![url.into()],
            username: None,
            credential: None,
        }
    }

    /// The same entry carrying one different URL.
    ///
    /// The credentials come with it. A relay URL without them allocates nothing, so an
    /// entry added without them would gather no candidate and fail in a way that looks
    /// like the relay is down.
    #[must_use]
    fn with_url(&self, url: String) -> Self {
        Self {
            urls: vec![url],
            username: self.username.clone(),
            credential: self.credential.clone(),
        }
    }
}

/// Whether one URL names a relay.
///
/// Both `turn:` and `turns:` do, and checking only for a substring `turn:` misses the TLS
/// one — the `s` sits between the word and the colon. A deployment that offers only
/// `turns:` would read as having no relay at all, which is the configuration a cautious
/// administrator is most likely to choose.
#[must_use]
pub fn is_relay_url(url: &str) -> bool {
    url.starts_with("turn:") || url.starts_with("turns:")
}

/// Whether an entry offers a relay under any of its URLs.
#[must_use]
pub fn is_relay_server(server: &IceServer) -> bool {
    server.urls.iter().any(|url| is_relay_url(url))
}

/// Whether this transport can actually allocate through a URL.
///
/// `turn:` over either UDP or TCP, and not `turns:`.
///
/// **The TCP half was the whole reason this function exists.** `webrtc =0.20.3` skipped
/// every `?transport=tcp` URL before allocating anything, so a player behind a firewall
/// that blocks outbound UDP -- a corporate network, a school, a hotel, some mobile
/// carriers -- gathered no relay candidate and could reach nobody, which is precisely the
/// case a relay is deployed for. `vendor/webrtc` carries the patch that removes the skip;
/// see `docs/rust-port/12-turn-over-tcp.md`.
///
/// `turns:` is still refused. The relayer skips a secure URL for being secure, and it
/// would be worse to connect to one in plaintext than not to connect at all: a `turns:`
/// URL is an operator saying the relay traffic must be inside TLS. Adding it is a separate
/// piece of work, and until it is done this must keep answering no -- a client believing
/// in a fallback it cannot reach is how relay rule three gets broken.
#[must_use]
pub fn transport_can_use(url: &str) -> bool {
    // `turns:` is TLS, which the relayer still refuses outright.
    if !url.starts_with("turn:") {
        return false;
    }
    // A named transport has to be one of the two that are implemented. No transport means
    // UDP, which is the default in RFC 7065 and what the relayer's own parser produces.
    match url.split_once("transport=") {
        None => true,
        Some((_, rest)) => matches!(rest.split(['&', '#']).next(), Some("udp" | "tcp")),
    }
}

/// Whether any entry offers a relay this transport can allocate through.
///
/// The honest version of [`has_relay`], and the one a *decision* should be made on.
/// `has_relay` answers what the server advertised, which is what a server operator is told
/// about; this answers what will actually gather a candidate.
#[must_use]
pub fn usable_relay(servers: &[IceServer]) -> bool {
    servers
        .iter()
        .any(|server| server.urls.iter().any(|url| transport_can_use(url)))
}

/// Whether there is a relay to fall back to.
///
/// Forcing relay mode with no relay advertised produces a connection that cannot gather
/// any candidate at all, which fails faster and more completely than the direct attempt
/// it replaced. Checked rather than assumed: a lobby where nobody can reach anybody and
/// no relay is offered is a server configuration problem, not a client one, and the two
/// look identical from the player's side.
#[must_use]
pub fn has_relay(servers: &[IceServer]) -> bool {
    servers.iter().any(is_relay_server)
}

/// Adds a TCP form of any relay that was advertised without a transport.
///
/// A `turn:` URL with no `?transport=` means UDP. A player on a network that blocks
/// outbound UDP — most schools, many offices, some mobile carriers — cannot reach that
/// relay at all, and those are exactly the networks that needed a relay to begin with.
/// The symptom is a player who hears nobody and whom nobody hears while everything else
/// works, because the signalling runs over TLS and is fine.
///
/// The server should advertise both, and this project's server now does. This is for the
/// ones that do not: a relay that already answers on TCP costs nothing to try, and one
/// that does not simply produces no candidate from the extra entry.
///
/// UDP stays first. ICE tries candidates in the order it is given them, and a TCP relay
/// is a worse path for everyone who can use the other one.
#[must_use]
pub fn with_tcp_relays(servers: &[IceServer]) -> Vec<IceServer> {
    // What is already on offer, so a server that advertises both does not get a third
    // entry pointing at the relay it just named. A duplicate URL is not harmless: one
    // allocation is made per entry, so every peer would hold two relay allocations over
    // TCP instead of one, and a relay's port range is finite. This is relay rule one.
    let mut advertised: Vec<String> = servers
        .iter()
        .flat_map(|s| s.urls.iter().cloned())
        .collect();

    let mut out = Vec::with_capacity(servers.len());
    for server in servers {
        out.push(server.clone());
        for url in &server.urls {
            // `turns:` is TLS over TCP already, so it needs nothing, and a URL that names
            // its own transport has been decided by whoever wrote it.
            if !url.starts_with("turn:") || url.contains("transport=") {
                continue;
            }
            let over_tcp = format!("{url}?transport=tcp");
            if advertised.contains(&over_tcp) {
                continue;
            }
            advertised.push(over_tcp.clone());
            out.push(server.with_url(over_tcp));
        }
    }
    out
}

/// Which candidates a connection is allowed to gather.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IceTransportPolicy {
    /// Host, reflexive and relay. The default.
    All,
    /// Relay only, which the server can ask for.
    Relay,
}

/// How many transports a connection negotiates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundlePolicy {
    /// One transport for the voice track and the data channel together.
    ///
    /// There is deliberately no second variant. On a good network bundling saves a little
    /// setup time; on a restrictive one it halves the work — one set of connectivity
    /// checks rather than two, one DTLS handshake, and, the part that matters, one relay
    /// allocation per peer rather than two. A fourteen-player lobby is ninety-one
    /// connections against a finite range of relay ports. Both ends of every connection
    /// here are this same client, so there is no peer that might not support it.
    MaxBundle,
}

/// Everything one peer connection is opened with, decided before any transport exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RtcConfig {
    /// The servers to gather candidates from, TCP relay forms included.
    pub ice_servers: Vec<IceServer>,
    /// Whether the connection may use anything but a relay.
    pub ice_transport_policy: IceTransportPolicy,
    /// Always [`BundlePolicy::MaxBundle`]; see the type.
    pub bundle_policy: BundlePolicy,
}

/// What a client uses before a server has said otherwise.
///
/// `DEFAULT_ICE_CONFIG` in `Voice.tsx:142-150`, which is one public STUN server and
/// nothing else. It is not much, and it is the difference between a lobby where direct
/// connections work and a lobby with no peer connections at all: a client whose server
/// never sent a usable `clientPeerConfig` had no mesh whatsoever until 2026-08-29, because
/// the mesh was built in the handler for that message and nowhere else.
#[must_use]
pub fn default_servers() -> Vec<IceServer> {
    vec![IceServer::new("stun:stun.l.google.com:19302")]
}

impl RtcConfig {
    /// Builds the configuration from what a server advertised.
    ///
    /// `force_relay` is the server's request rather than an instruction, and it is refused
    /// here when there is no relay to force through. Relay rule three of §4.6: gathering
    /// nothing at all fails harder and more completely than the direct attempt it
    /// replaced, so a connection that sometimes succeeded stops succeeding ever.
    ///
    /// **This lived in [`crate::peer_config::apply_client_peer_config`] until 2026-08-29,
    /// which has no callers.** The doc here said the caller had already refused it; the
    /// caller that would have was never wired, and `Lobby::on_peer_config` used
    /// `validate_peer_config` instead — which checks the shape and not the rule. So a
    /// server sending `forceRelayOnly` with an empty or STUN-only `iceServers` produced a
    /// relay-only client with no relay, and nobody in that lobby could reach anybody.
    ///
    /// The rule is applied by the type now, so no caller can forget it and none has to
    /// remember. `apply_client_peer_config` still refuses the same combination, one layer
    /// earlier and with a message a server operator can read.
    #[must_use]
    pub fn new(servers: &[IceServer], force_relay: bool) -> Self {
        let ice_servers = with_tcp_relays(servers);
        Self {
            ice_transport_policy: if force_relay && usable_relay(&ice_servers) {
                IceTransportPolicy::Relay
            } else {
                IceTransportPolicy::All
            },
            ice_servers,
            bundle_policy: BundlePolicy::MaxBundle,
        }
    }

    /// Every URL this configuration offers, in order. For logging, and for tests.
    #[must_use]
    pub fn urls(&self) -> Vec<&str> {
        self.ice_servers
            .iter()
            .flat_map(|server| server.urls.iter().map(String::as_str))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn relay(url: &str) -> IceServer {
        IceServer {
            urls: vec![url.to_owned()],
            username: Some("u".to_owned()),
            credential: Some("p".to_owned()),
        }
    }

    /// What the project's own server advertises, copied from a probe of it rather than
    /// written from memory: one entry naming TCP and one bare, which means UDP. The bare
    /// entry is why the deduplication is needed at all — without it this client adds a
    /// TCP form of it and ends up asking the same relay for two allocations per peer.
    fn live() -> Vec<IceServer> {
        vec![
            IceServer::new("stun:stun.l.google.com:19302"),
            IceServer {
                urls: vec!["turn:aucl.greluc.me:3478?transport=tcp".to_owned()],
                username: Some("acl".to_owned()),
                credential: Some("secret".to_owned()),
            },
            IceServer {
                urls: vec!["turn:aucl.greluc.me:3478".to_owned()],
                username: Some("acl".to_owned()),
                credential: Some("secret".to_owned()),
            },
        ]
    }

    fn urls_of(servers: &[IceServer]) -> Vec<&str> {
        servers
            .iter()
            .flat_map(|s| s.urls.iter().map(String::as_str))
            .collect()
    }

    #[test]
    fn adds_a_tcp_form_of_a_relay_advertised_without_a_transport() {
        let out = with_tcp_relays(&[relay("turn:relay.example:3478")]);
        assert_eq!(
            urls_of(&out),
            [
                "turn:relay.example:3478",
                "turn:relay.example:3478?transport=tcp"
            ]
        );
    }

    #[test]
    fn carries_the_credentials_onto_the_entry_it_adds() {
        // A relay URL without them allocates nothing, so the added entry would be a
        // candidate that never gathers and a failure that looks like the relay is down.
        let out = with_tcp_relays(&[relay("turn:relay.example:3478")]);
        assert_eq!(out[1].username.as_deref(), Some("u"));
        assert_eq!(out[1].credential.as_deref(), Some("p"));
    }

    #[test]
    fn a_relay_over_tcp_counts_as_a_relay() {
        // It did not until 2026-08-29, because the transport threw the URL away before
        // allocating. `with_tcp_relays` had been adding the TCP form since the beginning
        // and every one of them gathered nothing, so a player whose network blocks
        // outbound UDP -- the one player a relay is deployed for -- had no way through at
        // all. See `docs/rust-port/12-turn-over-tcp.md`.
        assert!(transport_can_use("turn:relay.example:3478?transport=tcp"));

        let udp_blocked = [relay("turn:relay.example:3478?transport=tcp")];
        assert!(usable_relay(&udp_blocked));
        assert_eq!(
            RtcConfig::new(&udp_blocked, true).ice_transport_policy,
            IceTransportPolicy::Relay,
            "relay-only is honoured now, because there is a relay it can reach"
        );
    }

    #[test]
    fn a_relay_this_transport_cannot_reach_does_not_count_as_one() {
        // A deployment offering only `turns:` has, as far as this client is concerned, no
        // relay at all. Forcing relay mode onto it would gather nothing -- which is relay
        // rule three, and the rule cannot be applied by a check that believes the
        // advertisement.
        assert!(transport_can_use("turn:relay.example:3478"));
        assert!(transport_can_use("turn:relay.example:3478?transport=udp"));
        assert!(!transport_can_use("turns:relay.example:5349"));
        assert!(!transport_can_use("stun:stun.example:3478"));

        let tls_only = [relay("turns:relay.example:5349")];
        assert!(
            has_relay(&tls_only),
            "the server did advertise one, and its operator is told so"
        );
        assert!(
            !usable_relay(&tls_only),
            "and this transport cannot allocate through it"
        );
        assert_eq!(
            RtcConfig::new(&tls_only, true).ice_transport_policy,
            IceTransportPolicy::All,
            "so relay-only would leave the connection with no candidates at all"
        );
    }

    #[test]
    fn relay_only_is_refused_when_there_is_no_relay() {
        // Relay rule three. Forcing relay mode with nothing to relay through leaves the
        // connection unable to gather any candidate at all -- worse than the direct
        // attempt it replaced, because a pair that sometimes connected stops connecting
        // ever.
        //
        // The rule lived in `peer_config::apply_client_peer_config`, which has no callers,
        // and this type's own documentation said the caller had already applied it. A
        // server sending `forceRelayOnly` beside an empty or STUN-only list therefore got
        // exactly the configuration the rule forbids.
        let stun_only = RtcConfig::new(&[IceServer::new("stun:stun.example:3478")], true);
        assert_eq!(stun_only.ice_transport_policy, IceTransportPolicy::All);
        assert_eq!(
            RtcConfig::new(&[], true).ice_transport_policy,
            IceTransportPolicy::All
        );

        // And it is still honoured when there is something to honour it with.
        assert_eq!(
            RtcConfig::new(&[relay("turn:relay.example:3478")], true).ice_transport_policy,
            IceTransportPolicy::Relay
        );
        // A TLS-only deployment is a relay the *server* advertises and this transport
        // cannot allocate through, so relay-only is refused for it too. See
        // `a_relay_this_transport_cannot_reach_does_not_count_as_one`, which is where that
        // distinction lives.
        assert_eq!(
            RtcConfig::new(&[relay("turns:relay.example:5349")], true).ice_transport_policy,
            IceTransportPolicy::All
        );
    }

    #[test]
    fn the_default_is_the_one_the_electron_client_ships() {
        // `DEFAULT_ICE_CONFIG` in `Voice.tsx:142-150`. Used when a server sends no usable
        // peer configuration, which used to mean no mesh at all.
        let config = RtcConfig::new(&default_servers(), false);
        assert_eq!(config.urls(), ["stun:stun.l.google.com:19302"]);
        assert_eq!(config.ice_transport_policy, IceTransportPolicy::All);
        assert_eq!(config.bundle_policy, BundlePolicy::MaxBundle);
    }

    #[test]
    fn leaves_udp_first() {
        // ICE tries candidates in the order it is given them, and a TCP relay is a worse
        // path for every player who can use the other one.
        let out = with_tcp_relays(&[relay("turn:relay.example:3478")]);
        let urls = urls_of(&out);
        let udp = urls
            .iter()
            .position(|u| *u == "turn:relay.example:3478")
            .unwrap();
        let tcp = urls
            .iter()
            .position(|u| *u == "turn:relay.example:3478?transport=tcp")
            .unwrap();
        assert!(udp < tcp);
    }

    /// The regression this exists for. One allocation is made per entry, so a third entry
    /// naming a relay already on the list means every peer holds two TCP allocations
    /// instead of one, and a relay's port range is finite. Relay rule one of §4.6.
    #[test]
    fn one_allocation_per_connection_when_the_server_advertises_both() {
        let servers = live();
        assert_eq!(urls_of(&with_tcp_relays(&servers)), urls_of(&servers));
    }

    #[test]
    fn leaves_a_url_that_names_its_own_transport_alone() {
        let out = with_tcp_relays(&[relay("turn:relay.example:3478?transport=tcp")]);
        assert_eq!(urls_of(&out), ["turn:relay.example:3478?transport=tcp"]);
    }

    #[test]
    fn leaves_tls_relays_and_stun_alone() {
        // `turns:` is TLS over TCP already, and a STUN server has no transport to force.
        let out = with_tcp_relays(&[
            IceServer::new("turns:relay.example:5349"),
            IceServer::new("stun:stun.example:3478"),
        ]);
        assert_eq!(
            urls_of(&out),
            ["turns:relay.example:5349", "stun:stun.example:3478"]
        );
    }

    #[test]
    fn handles_a_server_that_lists_several_urls_at_once() {
        let out = with_tcp_relays(&[IceServer {
            urls: vec![
                "turn:a.example:3478".to_owned(),
                "turn:b.example:3478?transport=tcp".to_owned(),
            ],
            username: None,
            credential: None,
        }]);
        assert_eq!(
            urls_of(&out),
            [
                "turn:a.example:3478",
                "turn:b.example:3478?transport=tcp",
                "turn:a.example:3478?transport=tcp",
            ]
        );
    }

    #[test]
    fn does_not_add_the_same_url_twice_for_two_servers_that_name_the_same_relay() {
        let out = with_tcp_relays(&[
            IceServer::new("turn:relay.example:3478"),
            IceServer::new("turn:relay.example:3478"),
        ]);
        let added = urls_of(&out)
            .iter()
            .filter(|u| u.ends_with("transport=tcp"))
            .count();
        assert_eq!(added, 1);
    }

    #[test]
    fn recognises_the_tls_relay() {
        // A substring test for `turn:` is false for `turns:host` — the `s` sits between
        // the word and the colon — so a deployment offering only `turns:` once read as
        // having no relay.
        assert!(is_relay_url("turns:relay.example:5349"));
    }

    #[test]
    fn recognises_a_plain_relay_and_rejects_stun() {
        assert!(is_relay_url("turn:relay.example:3478"));
        assert!(!is_relay_url("stun:stun.example:3478"));
    }

    #[test]
    fn reads_a_list() {
        assert!(is_relay_server(&IceServer {
            urls: vec![
                "stun:stun.example:3478".to_owned(),
                "turn:relay.example:3478".to_owned()
            ],
            username: None,
            credential: None,
        }));
    }

    #[test]
    fn has_relay_is_false_for_a_stun_only_server() {
        // Forcing relay with nothing to relay through gathers no candidate at all, which
        // fails harder than the direct attempt it replaced.
        assert!(!has_relay(&[IceServer::new(
            "stun:stun.l.google.com:19302"
        )]));
    }

    #[test]
    fn has_relay_is_false_when_no_servers_were_sent_at_all() {
        assert!(!has_relay(&[]));
    }

    #[test]
    fn has_relay_is_true_for_the_live_configuration() {
        assert!(has_relay(&live()));
    }

    #[test]
    fn bundles_everything_onto_one_transport() {
        // One set of connectivity checks instead of two, one DTLS handshake, and one
        // relay allocation per peer rather than two.
        let config = RtcConfig::new(&[], false);
        assert_eq!(config.bundle_policy, BundlePolicy::MaxBundle);
    }

    #[test]
    fn keeps_everything_the_server_decided() {
        // It must not quietly drop the relay list or the transport policy on its way
        // through, which is the only way this could do harm.
        let config = RtcConfig::new(&live(), true);
        assert_eq!(config.ice_transport_policy, IceTransportPolicy::Relay);
        assert_eq!(config.urls(), urls_of(&live()));
    }

    #[test]
    fn a_configuration_that_was_not_forced_may_still_go_direct() {
        assert_eq!(
            RtcConfig::new(&live(), false).ice_transport_policy,
            IceTransportPolicy::All
        );
    }
}
