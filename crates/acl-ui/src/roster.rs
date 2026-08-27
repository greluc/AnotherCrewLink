//! Who each view shows, in what order, and in what state.
//!
//! Two screens ask almost the same question and answer it differently, and both answers are
//! currently spread through the components that draw them — `Voice.tsx`'s main view and
//! `Overlay.tsx`'s row of avatars. The parts that decide anything come here, for the reason
//! this crate exists: a decision inside a paint function is a decision nobody can test, and
//! this project has already paid for that once with `sortLobbies`.
//!
//! # The two are not the same, and the differences are the interesting part
//!
//! The main view shows **everybody but you**, alive or not, connected or not — because it is
//! also the screen where you find out that somebody's connection is broken. The overlay
//! shows **who you can hear**, which is a smaller set: a peer with no connection is not in
//! it, and neither is a dead player while you are alive, because that is the game's own
//! rule about who can talk to whom.
//!
//! Both are ported rather than invented. During §4.10's rollout the two clients sit side by
//! side, and an overlay that shows a different set of people from the one beside it is a bug
//! report nobody can reproduce.

/// What the voice layer knows that the game state does not.
///
/// Passed as one borrow rather than six arguments, and named for what it is: the game says
/// where people are, this says what is happening to their audio.
///
/// No `Debug`: four of its fields are closures, and a derive that printed them would print
/// nothing useful. What a caller wants to see when something here is wrong is the [`Shown`]
/// list, which does derive it.
#[derive(Clone, Copy)]
pub struct Voice<'a> {
    /// Whether each client's stream is carrying speech, by client id.
    pub talking: &'a dyn Fn(i64) -> bool,
    /// Whether each client is dead *as the voice layer believes*, by client id.
    ///
    /// Not the game's `isDead`. The voice layer learns of a death when it becomes audible
    /// rather than when it happens, and it is this belief the views act on — `otherDead` in
    /// `Voice.tsx`, which is what decides who can be heard.
    pub dead: &'a dyn Fn(i64) -> bool,
    /// Whether there is a peer connection to this client at all.
    pub connected: &'a dyn Fn(i64) -> bool,
    /// Whether that connection is carrying audio.
    pub audible: &'a dyn Fn(i64) -> bool,
    /// Whether the local player is speaking.
    pub local_talking: bool,
    /// Whether the local player is alive, as the voice layer believes.
    pub local_alive: bool,
    /// The client the impostor radio is tuned to, if any.
    pub impostor_radio: Option<i64>,
    /// Whether the local player is an impostor.
    pub local_is_impostor: bool,
}

/// What a view knows about one player's connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// There is no peer connection.
    Disconnected,
    /// There is one, and it is not carrying audio.
    ///
    /// A distinct state and not a detail: it is the difference between "they have not
    /// arrived" and "they are here and you cannot hear them", which are fixed by different
    /// things.
    Silent,
    /// There is one and it is carrying audio.
    Connected,
}

/// One player as a view needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    /// Where they are in the list the caller passed in.
    ///
    /// An index rather than a copy: everything else about a player belongs to the game
    /// state, and duplicating it here would be a second place for it to go stale.
    pub at: usize,
    /// Whether to show them as speaking.
    pub talking: bool,
    /// Whether to show them as alive.
    pub alive: bool,
    /// What to show about their connection.
    pub link: Link,
    /// Whether they are the one the impostor radio is tuned to.
    pub using_radio: bool,
}

/// The minimum a view needs to know about a player.
///
/// A trait rather than a struct so that this crate does not depend on the reader's
/// `AmongUsState`: the ordering and the filtering are about four fields, and taking the
/// whole state would make every test here build one.
pub trait Roster {
    /// Their in-game id, which is what the ordering falls back to.
    fn id(&self) -> u8;
    /// Their network client id, which the voice layer keys on.
    fn client_id(&self) -> i64;
    /// Whether this is the local player.
    fn is_local(&self) -> bool;
    /// Whether the game says they have left.
    fn disconnected(&self) -> bool;
    /// Whether they are in a vent.
    fn in_vent(&self) -> bool;
    /// Whether the reader could make sense of them.
    fn bugged(&self) -> bool;
    /// Whether the game says they are dead.
    fn is_dead(&self) -> bool;
}

/// Everyone the main view shows.
///
/// Everybody but the local player, in the order the game gave them, whatever state they are
/// in. That last part is deliberate: this is the screen where somebody finds out a peer is
/// disconnected, so hiding the disconnected would hide the thing it is for.
pub fn main_view<P: Roster>(players: &[P], voice: &Voice<'_>) -> Vec<Shown> {
    players
        .iter()
        .enumerate()
        .filter(|(_, player)| !player.is_local())
        .map(|(at, player)| Shown {
            at,
            talking: !player.in_vent() && (voice.talking)(player.client_id()),
            alive: !(voice.dead)(player.client_id()),
            link: link_for(player, voice),
            // An impostor's radio reaches one other impostor, and only the local player
            // being one makes it visible at all. `disconnected || bugged` because a player
            // the reader could not make sense of has no meaningful client id to match.
            using_radio: voice.local_is_impostor
                && !(player.disconnected() || player.bugged())
                && voice.impostor_radio == Some(player.client_id()),
        })
        .collect()
}

/// Everyone the overlay shows.
///
/// Narrower than the main view in three ways, each of them a rule about audibility rather
/// than about display:
///
/// * **A dead player is hidden while you are alive.** They can hear you and you cannot hear
///   them, so showing them in a list of voices would be showing something that is not there.
///   Once you are dead they come back, because then you can.
/// * **A peer with no connection is hidden**, unless it is you. There is nothing to hear.
/// * **In compact mode, only who is speaking.** The whole point of that mode is a row that
///   is empty when nobody is talking.
///
/// The order is not the game's. Anybody disconnected or dead goes last, and the rest keep
/// their in-game order — so the row does not reshuffle as people stop being audible.
pub fn overlay<P: Roster>(players: &[P], voice: &Voice<'_>, compact: bool) -> Vec<Shown> {
    let mut shown: Vec<(usize, bool)> = players
        .iter()
        .enumerate()
        .filter(|(_, player)| {
            let dead = (voice.dead)(player.client_id());
            // Hidden only while you are still alive. `!local_alive ||` first, so that a dead
            // player sees everybody.
            if voice.local_alive && dead && !player.is_local() {
                return false;
            }
            if !player.is_local() && !(voice.connected)(player.client_id()) {
                return false;
            }
            true
        })
        .map(|(at, player)| {
            // Sorted on "out of the conversation" rather than on either flag alone: a
            // disconnected player and a dead one are both people you cannot hear, and the
            // row should not distinguish them by position.
            (
                at,
                player.disconnected() || (voice.dead)(player.client_id()),
            )
        })
        .collect();

    // Stable, so equal rows keep the game's order rather than an invented one.
    shown.sort_by_key(|(at, out)| (*out, players.get(*at).map_or(u8::MAX, Roster::id)));

    shown
        .into_iter()
        .filter_map(|(at, _)| {
            let player = players.get(at)?;
            let talking = !player.in_vent()
                && ((voice.talking)(player.client_id())
                    || (player.is_local() && voice.local_talking));
            if compact && !talking {
                return None;
            }
            Some(Shown {
                at,
                talking,
                // The local player is the one case where the game's own flag wins: you know
                // whether you are dead before the voice layer does.
                alive: !(voice.dead)(player.client_id())
                    || (player.is_local() && !player.is_dead()),
                link: link_for(player, voice),
                using_radio: voice.local_is_impostor
                    && !(player.disconnected() || player.bugged())
                    && voice.impostor_radio == Some(player.client_id()),
            })
        })
        .collect()
}

/// What to show about one player's connection.
fn link_for<P: Roster>(player: &P, voice: &Voice<'_>) -> Link {
    if !(voice.connected)(player.client_id()) {
        Link::Disconnected
    } else if (voice.audible)(player.client_id()) {
        Link::Connected
    } else {
        Link::Silent
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Link, Phase, Roster, Shown, Voice, follow_deaths, main_view, overlay};

    /// A player the game says is dead, with the given client id.
    fn dead(client_id: i64) -> Player {
        Player {
            client_id,
            is_dead: true,
            ..Player::default()
        }
    }

    /// And one it says is not.
    fn alive(client_id: i64) -> Player {
        Player {
            client_id,
            ..Player::default()
        }
    }

    /// A round in progress teaches this nothing, and that is the point.
    ///
    /// The game knows who has died. Acting on it would cut a player's voice the instant
    /// they were killed across the map -- which announces a body, to everybody, before
    /// anybody has found it. `Voice.tsx` guards this by not looking; so does this.
    #[test]
    fn nobody_learns_of_a_death_during_the_round() {
        let players = [dead(1), alive(2)];
        let mut believed = std::collections::BTreeMap::new();
        follow_deaths(Phase::Round, &players, &mut believed);
        assert!(
            believed.is_empty(),
            "the round leaked who is dead: {believed:?}"
        );

        // And it does not un-learn either: a map built at the last meeting survives into
        // the round, or every round would begin by forgetting who the ghosts are.
        believed.insert(1, true);
        follow_deaths(Phase::Round, &players, &mut believed);
        assert_eq!(believed.get(&1), Some(&true));
    }

    /// Leaving the round is when it becomes public, so that is when this reads it.
    #[test]
    fn a_meeting_is_where_the_deaths_are_learned() {
        let players = [dead(1), alive(2)];
        let mut believed = std::collections::BTreeMap::new();
        follow_deaths(Phase::Elsewhere, &players, &mut believed);
        assert_eq!(believed.get(&1), Some(&true));
        assert_eq!(believed.get(&2), Some(&false));
    }

    /// A new lobby starts with everybody alive.
    ///
    /// Without this the ghosts of the last round carry into the next one, and the players
    /// who were dead cannot be heard by anybody for a game they are alive in.
    #[test]
    fn the_lobby_forgets_the_last_round() {
        let players = [dead(1)];
        let mut believed = std::collections::BTreeMap::new();
        believed.insert(1, true);
        follow_deaths(Phase::Lobby, &players, &mut believed);
        assert!(believed.is_empty(), "{believed:?}");
    }

    /// Somebody who left counts as dead.
    ///
    /// `player.isDead || player.disconnected` in `Voice.tsx`. They cannot be heard from, and
    /// treating them as alive leaves a silent living player in the list -- which reads as a
    /// broken connection rather than as somebody who has gone.
    #[test]
    fn a_player_who_left_counts_as_dead() {
        let mut gone = alive(3);
        gone.disconnected = true;
        let mut believed = std::collections::BTreeMap::new();
        follow_deaths(Phase::Elsewhere, &[gone], &mut believed);
        assert_eq!(believed.get(&3), Some(&true));
    }

    // Five booleans, because the trait has five and a test double that summarised them
    // would be testing a summary.
    #[allow(clippy::struct_excessive_bools)]
    #[derive(Clone, Copy, Debug, Default)]
    struct Player {
        id: u8,
        client_id: i64,
        is_local: bool,
        disconnected: bool,
        in_vent: bool,
        bugged: bool,
        is_dead: bool,
    }

    impl Roster for Player {
        fn id(&self) -> u8 {
            self.id
        }
        fn client_id(&self) -> i64 {
            self.client_id
        }
        fn is_local(&self) -> bool {
            self.is_local
        }
        fn disconnected(&self) -> bool {
            self.disconnected
        }
        fn in_vent(&self) -> bool {
            self.in_vent
        }
        fn bugged(&self) -> bool {
            self.bugged
        }
        fn is_dead(&self) -> bool {
            self.is_dead
        }
    }

    fn player(id: u8, client_id: i64) -> Player {
        Player {
            id,
            client_id,
            ..Player::default()
        }
    }

    /// A voice layer where everybody is connected, audible, alive and silent.
    fn quiet() -> Voice<'static> {
        Voice {
            talking: &|_| false,
            dead: &|_| false,
            connected: &|_| true,
            audible: &|_| true,
            local_talking: false,
            local_alive: true,
            impostor_radio: None,
            local_is_impostor: false,
        }
    }

    fn at(shown: &[Shown]) -> Vec<usize> {
        shown.iter().map(|entry| entry.at).collect()
    }

    /// The main view is the screen where a broken connection is discovered, so it shows
    /// people it cannot hear. Filtering them out would hide the thing it exists for.
    #[test]
    fn the_main_view_shows_everyone_but_you_whatever_state_they_are_in() {
        let players = [
            Player {
                is_local: true,
                ..player(0, 900)
            },
            player(1, 901),
            Player {
                disconnected: true,
                ..player(2, 902)
            },
        ];
        let voice = Voice {
            dead: &|client| client == 902,
            connected: &|client| client != 902,
            ..quiet()
        };
        let shown = main_view(&players, &voice);
        assert_eq!(at(&shown), vec![1, 2]);
        assert_eq!(shown[1].link, Link::Disconnected);
        assert!(!shown[1].alive);
    }

    /// "Connected but silent" is its own state. It is the difference between a peer who has
    /// not arrived and one who is here and cannot be heard, and those are fixed by
    /// different things.
    #[test]
    fn a_connection_with_no_audio_is_not_the_same_as_no_connection() {
        let players = [player(1, 901), player(2, 902)];
        let voice = Voice {
            connected: &|_| true,
            audible: &|client| client == 901,
            ..quiet()
        };
        let shown = main_view(&players, &voice);
        assert_eq!(shown[0].link, Link::Connected);
        assert_eq!(shown[1].link, Link::Silent);
    }

    /// A player in a vent is not shown as talking however loudly they are.
    #[test]
    fn a_venting_player_is_never_shown_as_talking() {
        let players = [Player {
            in_vent: true,
            ..player(1, 901)
        }];
        let voice = Voice {
            talking: &|_| true,
            ..quiet()
        };
        assert!(!main_view(&players, &voice)[0].talking);
        assert!(!overlay(&players, &voice, false)[0].talking);
    }

    /// The overlay is a list of voices, so somebody you cannot hear is not in it — and once
    /// you are dead you can hear them, so they come back.
    #[test]
    fn the_overlay_hides_the_dead_only_while_you_are_alive() {
        let players = [player(1, 901), player(2, 902)];
        let voice = Voice {
            dead: &|client| client == 902,
            ..quiet()
        };
        assert_eq!(at(&overlay(&players, &voice, false)), vec![0]);

        let dead = Voice {
            local_alive: false,
            ..voice
        };
        assert_eq!(at(&overlay(&players, &dead, false)), vec![0, 1]);
    }

    /// And somebody with no peer connection is not in it either. There is nothing to hear.
    #[test]
    fn the_overlay_hides_a_peer_with_no_connection() {
        let players = [player(1, 901), player(2, 902)];
        let voice = Voice {
            connected: &|client| client == 901,
            ..quiet()
        };
        assert_eq!(at(&overlay(&players, &voice, false)), vec![0]);
    }

    /// Except yourself: there is no peer connection to you, and you are always in your own
    /// overlay.
    #[test]
    fn but_never_hides_you_for_having_no_connection_to_yourself() {
        let players = [Player {
            is_local: true,
            ..player(0, 900)
        }];
        let voice = Voice {
            connected: &|_| false,
            ..quiet()
        };
        assert_eq!(at(&overlay(&players, &voice, false)), vec![0]);
    }

    /// Compact mode is a row that is empty when nobody is talking. That is the whole
    /// feature, so a silent lobby produces nothing at all.
    #[test]
    fn compact_mode_shows_only_who_is_speaking() {
        let players = [player(1, 901), player(2, 902)];
        let talking = Voice {
            talking: &|client| client == 902,
            ..quiet()
        };
        assert_eq!(at(&overlay(&players, &talking, true)), vec![1]);
        assert!(overlay(&players, &quiet(), true).is_empty());
    }

    /// Anybody out of the conversation goes last, and the rest keep the game's order, so
    /// the row does not reshuffle as people stop being audible.
    #[test]
    fn the_overlay_puts_whoever_cannot_be_heard_at_the_end() {
        let players = [
            Player {
                disconnected: true,
                ..player(1, 901)
            },
            player(2, 902),
            player(3, 903),
        ];
        let voice = Voice {
            // Dead but the local player is dead too, so they are still shown -- which is
            // what makes this a test about order rather than about filtering.
            dead: &|client| client == 903,
            local_alive: false,
            ..quiet()
        };
        assert_eq!(at(&overlay(&players, &voice, false)), vec![1, 0, 2]);
    }

    /// The radio reaches one other impostor, and only if you are one. A player the reader
    /// could not make sense of has no client id worth matching.
    #[test]
    fn the_radio_marks_one_player_and_only_for_an_impostor() {
        let players = [
            player(1, 901),
            Player {
                bugged: true,
                ..player(2, 902)
            },
        ];
        let tuned = Voice {
            impostor_radio: Some(901),
            local_is_impostor: true,
            ..quiet()
        };
        assert!(main_view(&players, &tuned)[0].using_radio);
        assert!(!main_view(&players, &tuned)[1].using_radio);

        let crewmate = Voice {
            local_is_impostor: false,
            ..tuned
        };
        assert!(!main_view(&players, &crewmate)[0].using_radio);

        let bugged = Voice {
            impostor_radio: Some(902),
            ..tuned
        };
        assert!(!main_view(&players, &bugged)[1].using_radio);
    }

    /// You know whether you are dead before the voice layer does, so your own flag wins for
    /// you and nobody else's does for them.
    #[test]
    fn your_own_death_is_the_games_word_and_everyone_elses_is_the_voice_layers() {
        let players = [
            Player {
                is_local: true,
                is_dead: false,
                ..player(0, 900)
            },
            player(1, 901),
        ];
        let voice = Voice {
            // The voice layer has not caught up on either.
            dead: &|_| true,
            local_alive: false,
            ..quiet()
        };
        let shown = overlay(&players, &voice, false);
        assert!(shown[0].alive, "you are alive and the game says so");
        assert!(
            !shown[1].alive,
            "they are dead as far as the voice layer knows"
        );
    }
}

/// Where the game is, as the death rule cares about it.
///
/// Three cases rather than the reader's five: this rule asks one question — is the round
/// running, is it over, or has a new one not started — and mapping the states onto it at the
/// call site keeps this crate free of the reader's enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Before a round. Everybody is alive again.
    Lobby,
    /// A round in progress.
    Round,
    /// A meeting, the end of a round, the menu — anywhere the deaths are already public.
    Elsewhere,
}

/// Updates who the voice layer believes is dead.
///
/// # This is a rule about information, not about bookkeeping
///
/// `Voice.tsx` lines 847-860. During a round the map is **not touched**: the game knows who
/// has died and this client must not act on it, or a player's voice would cut out the
/// instant they were killed across the map — announcing a body before anybody found it, to
/// everybody, every time.
///
/// What it learns from is leaving the round. A meeting is called or the game ends, and at
/// that point the deaths are public knowledge anyway. [`Phase::Lobby`] clears the map,
/// because the next round starts with everybody alive.
///
/// `disconnected` counts as dead, exactly as it does there: somebody who has left cannot be
/// heard from, and treating them as alive leaves a silent living player in the list.
///
/// Call it on the state *transition*. Every frame would be the same answer recomputed sixty
/// times a second, and during a round it is the thing that must not happen at all.
pub fn follow_deaths<P: Roster>(
    phase: Phase,
    players: &[P],
    believed: &mut std::collections::BTreeMap<i64, bool>,
) {
    match phase {
        Phase::Lobby => believed.clear(),
        Phase::Round => {}
        Phase::Elsewhere => {
            for player in players {
                believed.insert(
                    player.client_id(),
                    player.is_dead() || player.disconnected(),
                );
            }
        }
    }
}
