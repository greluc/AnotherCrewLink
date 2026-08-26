//! Who is in the lobby, and what that implies for connections.
//!
//! §4.6 item 3: join, leave, orphan cleanup, rebuild-on-failure. The bookkeeping half —
//! the peer objects themselves are [`crate::peer`]'s, and which end offers is
//! [`crate::reconnect`]'s.
//!
//! It holds no connections, only identities. A mesh that owned the connections could not
//! be tested without a transport, and the parts that go wrong here are all about
//! ordering: registering a peer after connecting to it, tearing down a peer that only
//! looked gone, and rebuilding one that the far end has already rebuilt.

use std::collections::BTreeSet;

/// What the caller should do about one peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// This peer belongs in the mesh and does not have a connection.
    ///
    /// It said "offering first" until 2026-08-26, and that is not this type's to say. Who
    /// offers depends on *which event* the peer appeared in, which membership cannot see:
    /// somebody announced by `join` arrived after this client and is offered to, somebody
    /// listed in `setClients` was already here and offers instead. Both produce this
    /// action, and a caller that read the old sentence would offer to everyone in the
    /// lobby while everyone in the lobby offered back. `acl-core::session::Arrival` is
    /// where the distinction is carried.
    Connect(String),
    /// Tear down whatever exists for this peer.
    Disconnect(String),
}

/// The peers this client believes are in the lobby with it.
///
/// A `BTreeSet` rather than a hash set so that [`Membership::reconcile`] returns actions
/// in a stable order. A test that depends on iteration order of a hash set passes until
/// it does not, and the order is also what a log line shows.
#[derive(Clone, Debug, Default)]
pub struct Membership {
    peers: BTreeSet<String>,
}

impl Membership {
    /// An empty lobby.
    #[must_use]
    pub fn new() -> Self {
        Self {
            peers: BTreeSet::new(),
        }
    }

    /// Whether this peer is known.
    ///
    /// The signal handler asks this before it acts on anything, which is why
    /// [`Membership::join`] registers before it returns a [`Action::Connect`] rather than
    /// after.
    #[must_use]
    pub fn knows(&self, peer: &str) -> bool {
        self.peers.contains(peer)
    }

    /// How many peers are in the lobby.
    #[must_use]
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Whether the lobby is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Every peer, in a stable order.
    pub fn peers(&self) -> impl Iterator<Item = &str> {
        self.peers.iter().map(String::as_str)
    }

    /// A peer joined.
    ///
    /// **The registration happens before the connect is returned, and the order is the
    /// point.** The far end's answer can arrive before this client has finished setting
    /// up, and the signal handler rejects a signal from a socket it does not know — so a
    /// mesh that connected first and registered afterwards would drop the answer to its
    /// own offer, intermittently, on exactly the fast connections.
    ///
    /// Returns `None` for a peer already known, so a duplicate `join` does not open a
    /// second connection beside the first.
    pub fn join(&mut self, peer: &str) -> Option<Action> {
        if !self.peers.insert(peer.to_owned()) {
            return None;
        }
        Some(Action::Connect(peer.to_owned()))
    }

    /// A peer left, and the server said so.
    ///
    /// Without this event a departure is only visible as a connection that failed, which
    /// is what a broken connection looks like too — so the two could not be told apart,
    /// and a player who left was retried for the rest of the round.
    ///
    /// Returns `None` for a peer that was not there.
    pub fn left(&mut self, peer: &str) -> Option<Action> {
        if !self.peers.remove(peer) {
            return None;
        }
        Some(Action::Disconnect(peer.to_owned()))
    }

    /// The server sent the whole membership; make the local view match it.
    ///
    /// This is where orphans go. A peer whose `left` was missed — the event was lost, or
    /// this client was reconnecting when it was sent — otherwise stays in the map forever,
    /// holding a connection that will never come back and a reconnect timer that will
    /// never stop.
    ///
    /// Disconnects come before connects. A peer that is in both lists is untouched, so the
    /// ordering only matters when a socket id is reused, but a reused id that connected
    /// before it disconnected would tear down the connection it had just made.
    pub fn reconcile<'a>(&mut self, members: impl IntoIterator<Item = &'a str>) -> Vec<Action> {
        let wanted: BTreeSet<String> = members.into_iter().map(str::to_owned).collect();

        let mut actions: Vec<Action> = self
            .peers
            .difference(&wanted)
            .map(|gone| Action::Disconnect(gone.clone()))
            .collect();
        actions.extend(
            wanted
                .difference(&self.peers)
                .map(|new| Action::Connect(new.clone())),
        );

        self.peers = wanted;
        actions
    }

    /// Everyone is gone: the socket dropped, or this client left the lobby.
    pub fn clear(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.peers)
            .into_iter()
            .map(Action::Disconnect)
            .collect()
    }
}

/// What a scheduled rebuild has to check before it runs.
///
/// The delay between deciding to rebuild and rebuilding is seconds to a minute, and any
/// of these can change inside it. Each guard is one way the old client rebuilt something
/// it should not have.
// Four independent conditions, each of which is a separate way the old client rebuilt
// something it should not have. Naming them is the point; collapsing them into flags or a
// state enum would hide which one refused a rebuild, and that is the thing a log has to
// say.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RebuildContext {
    /// Whether the peer is still in the lobby.
    pub still_a_member: bool,
    /// Whether this client is still in a lobby at all.
    pub in_a_lobby: bool,
    /// Whether a connection to this peer already exists.
    pub already_connected: bool,
    /// Whether the signalling socket is up.
    pub socket_connected: bool,
}

/// Whether a rebuild scheduled earlier should still happen.
///
/// `already_connected` is the one worth naming: the far end got there first. Rebuilding
/// over its connection is offer glare that this end caused on purpose, and
/// `offer_glare_does_not_destroy_replacement` only protects the peer that receives it.
#[must_use]
pub fn should_rebuild(context: RebuildContext) -> bool {
    context.still_a_member
        && context.in_a_lobby
        && context.socket_connected
        && !context.already_connected
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn connect(peer: &str) -> Action {
        Action::Connect(peer.to_owned())
    }

    fn disconnect(peer: &str) -> Action {
        Action::Disconnect(peer.to_owned())
    }

    #[test]
    fn a_peer_is_known_before_the_connect_is_returned() {
        // The ordering this type exists to enforce. The far end's answer can arrive before
        // this client finishes setting up, and the signal handler rejects a signal from a
        // socket it does not know -- so registering afterwards drops the answer to this
        // client's own offer, intermittently, on exactly the fastest connections.
        let mut lobby = Membership::new();
        let action = lobby.join("alice");
        assert!(lobby.knows("alice"));
        assert_eq!(action, Some(connect("alice")));
    }

    #[test]
    fn a_duplicate_join_does_not_open_a_second_connection() {
        let mut lobby = Membership::new();
        assert_eq!(lobby.join("alice"), Some(connect("alice")));
        assert_eq!(lobby.join("alice"), None);
        assert_eq!(lobby.len(), 1);
    }

    #[test]
    fn a_departure_is_told_apart_from_a_failure() {
        // Without the server's event, a peer who left looks exactly like a peer whose
        // connection broke -- so the client retried them for the rest of the round.
        let mut lobby = Membership::new();
        lobby.join("alice");
        assert_eq!(lobby.left("alice"), Some(disconnect("alice")));
        assert!(!lobby.knows("alice"));
    }

    #[test]
    fn a_departure_nobody_announced_twice_is_not_an_event() {
        let mut lobby = Membership::new();
        lobby.join("alice");
        lobby.left("alice");
        assert_eq!(lobby.left("alice"), None);
        assert_eq!(lobby.left("never-here"), None);
    }

    #[test]
    fn reconcile_removes_the_orphans() {
        // A peer whose `left` was missed -- the event was lost, or this client was
        // reconnecting when it was sent -- otherwise holds a connection that will never
        // come back and a reconnect timer that will never stop.
        let mut lobby = Membership::new();
        lobby.join("alice");
        lobby.join("bob");
        assert_eq!(
            lobby.reconcile(["alice"]),
            [Action::Disconnect("bob".into())]
        );
        assert!(!lobby.knows("bob"));
    }

    #[test]
    fn reconcile_connects_to_anyone_new() {
        let mut lobby = Membership::new();
        lobby.join("alice");
        assert_eq!(
            lobby.reconcile(["alice", "bob"]),
            [Action::Connect("bob".into())]
        );
    }

    #[test]
    fn reconcile_leaves_the_unchanged_alone() {
        // The common case, and the one that must produce nothing: a membership list that
        // agrees with what this client already has must not churn every connection.
        let mut lobby = Membership::new();
        lobby.join("alice");
        lobby.join("bob");
        assert!(lobby.reconcile(["alice", "bob"]).is_empty());
    }

    #[test]
    fn reconcile_disconnects_before_it_connects() {
        // Only matters when a socket id is reused, and then it matters completely: a
        // reused id that connected before it disconnected would tear down the connection
        // it had just made.
        let mut lobby = Membership::new();
        lobby.join("alice");
        let actions = lobby.reconcile(["bob"]);
        assert_eq!(
            actions,
            [
                Action::Disconnect("alice".into()),
                Action::Connect("bob".into())
            ]
        );
    }

    #[test]
    fn reconcile_is_stable_in_its_ordering() {
        // A hash set would pass this test until it did not, and the order is also what a
        // log line shows.
        let mut first = Membership::new();
        let mut second = Membership::new();
        for peer in ["delta", "alpha", "charlie", "bravo"] {
            first.join(peer);
        }
        for peer in ["bravo", "charlie", "alpha", "delta"] {
            second.join(peer);
        }
        assert_eq!(first.reconcile([]), second.reconcile([]));
    }

    #[test]
    fn clearing_disconnects_everyone_and_empties_the_lobby() {
        let mut lobby = Membership::new();
        lobby.join("alice");
        lobby.join("bob");
        assert_eq!(lobby.clear().len(), 2);
        assert!(lobby.is_empty());
        assert!(lobby.clear().is_empty());
    }

    fn ready() -> RebuildContext {
        RebuildContext {
            still_a_member: true,
            in_a_lobby: true,
            already_connected: false,
            socket_connected: true,
        }
    }

    #[test]
    fn a_rebuild_runs_when_nothing_changed_under_it() {
        assert!(should_rebuild(ready()));
    }

    #[test]
    fn a_rebuild_does_not_race_the_connection_the_far_end_made() {
        // The far end got there first. Rebuilding over its connection is offer glare this
        // end caused deliberately, and the protection against glare only helps the peer
        // that receives it.
        assert!(!should_rebuild(RebuildContext {
            already_connected: true,
            ..ready()
        }));
    }

    #[test]
    fn a_rebuild_is_abandoned_when_the_peer_left() {
        assert!(!should_rebuild(RebuildContext {
            still_a_member: false,
            ..ready()
        }));
    }

    #[test]
    fn a_rebuild_is_abandoned_when_this_client_left() {
        // Back at the menu. The delay between scheduling and firing is seconds to a
        // minute, which is long enough to finish a round in.
        assert!(!should_rebuild(RebuildContext {
            in_a_lobby: false,
            ..ready()
        }));
    }

    #[test]
    fn a_rebuild_waits_for_the_signalling_socket() {
        // Rejoining the lobby produces a fresh connection anyway, so an offer sent while
        // the socket is down is one that reaches nobody and is then replaced.
        assert!(!should_rebuild(RebuildContext {
            socket_connected: false,
            ..ready()
        }));
    }
}
