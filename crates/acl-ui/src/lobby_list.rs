//! The order public lobbies are listed in.
//!
//! Ported from `src/renderer/LobbyBrowser/sortLobbies.ts` — but from the corrected
//! version, not the one that shipped. The original was four lines inside the component
//! and was not a consistent ordering: it had a rule for full lobbies and applied it in
//! one direction only, so `compare(full, joinable)` and `compare(joinable, full)` both
//! returned "I come first". A sort given that produces an implementation-defined result,
//! and the list reshuffled between refreshes with full lobbies sometimes above the ones a
//! player could join.
//!
//! It is ported here rather than reimplemented because §4.8's lobby browser has to look
//! the same as the one it replaces, and "the same" includes the order. The Rust type
//! system makes the specific mistake harder — [`Ord`] must be a total order and the
//! standard library says so — but only if the comparison is written as one, which is why
//! this is a [`Ord`] implementation and not a free function taking two arguments.

use std::cmp::Ordering;

/// What the client knows about one advertised lobby.
///
/// A subset of the server's row: the fields the ordering reads. The rest belongs to
/// whatever draws the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LobbyRow {
    /// Whether the game has not started yet, so the lobby can be joined at all.
    pub waiting: bool,
    /// How many players are in it.
    pub players: u32,
    /// How many it holds.
    pub capacity: u32,
}

impl LobbyRow {
    /// Whether there is no room left.
    ///
    /// `>=`, not `==`. The server has been seen to report a lobby over its own limit, and
    /// a strict equality called that joinable — then put it at the top of the list,
    /// because it also had the most players.
    #[must_use]
    pub const fn is_full(self) -> bool {
        self.players >= self.capacity
    }
}

/// Why a lobby is listed but cannot be joined.
///
/// Three reasons, each with its own string, because "you cannot join this" without saying
/// why sends the player to try the next row and the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The game has already started.
    InProgress,
    /// There is no room.
    Full,
    /// It is running a different mod, so the two clients would not agree about the game.
    DifferentMod,
}

impl Refusal {
    /// The i18n key that says so.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::InProgress => "lobbybrowser.code_tooltips.in_progress",
            Self::Full => "lobbybrowser.code_tooltips.full_lobby",
            Self::DifferentMod => "lobbybrowser.code_tooltips.incompatible",
        }
    }
}

impl LobbyRow {
    /// Why this lobby cannot be joined, if it cannot.
    ///
    /// The order is the order the Electron browser asks in, and it is the order a player
    /// would: a game in progress is not worth mentioning the mod of.
    ///
    /// **Full is `is_full`, not `players == capacity`.** The Electron browser tests the
    /// strict equality here — the same mistake `sortLobbies` made, in the same feature —
    /// so a lobby the server reports as *over* its own limit is offered as joinable, and
    /// the join fails at the server. The ordering was corrected when it was ported; this
    /// is the second place it had to be.
    #[must_use]
    pub const fn refusal(self, mods_match: bool) -> Option<Refusal> {
        if !self.waiting {
            Some(Refusal::InProgress)
        } else if self.is_full() {
            Some(Refusal::Full)
        } else if mods_match {
            None
        } else {
            Some(Refusal::DifferentMod)
        }
    }

    /// Whether the join button is offered at all.
    #[must_use]
    pub const fn joinable(self, mods_match: bool) -> bool {
        self.refusal(mods_match).is_none()
    }
}

impl Ord for LobbyRow {
    /// Three keys, each applied in both directions.
    ///
    /// 1. **Not started first.** A game in progress cannot be joined, so it is the least
    ///    useful row on the screen.
    /// 2. **Then the ones with room**, for the same reason one step weaker: a full lobby
    ///    may empty.
    /// 3. **Then the fullest first**, because eight players is a game about to start and
    ///    two is a wait.
    ///
    /// Equal rows compare equal, and the caller sorts with a stable sort, so a tie keeps
    /// whatever order the server sent rather than being given an invented one.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .waiting
            .cmp(&self.waiting)
            .then_with(|| self.is_full().cmp(&other.is_full()))
            .then_with(|| other.players.cmp(&self.players))
    }
}

impl PartialOrd for LobbyRow {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Sorts a list of lobbies for display.
///
/// Stable, so equal rows keep the order they arrived in.
pub fn sort(lobbies: &mut [LobbyRow]) {
    lobbies.sort();
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    /// The three reasons are distinct and each has its own string. "You cannot join this"
    /// without saying why sends the player to try the next row, and the next.
    #[test]
    fn each_refusal_says_something_different() {
        let waiting_half = LobbyRow {
            waiting: true,
            players: 5,
            capacity: 10,
        };
        assert_eq!(waiting_half.refusal(true), None);
        assert!(waiting_half.joinable(true));
        assert_eq!(
            waiting_half.refusal(false),
            Some(Refusal::DifferentMod),
            "a lobby with room, running something else"
        );

        let started = LobbyRow {
            waiting: false,
            ..waiting_half
        };
        assert_eq!(started.refusal(true), Some(Refusal::InProgress));

        let full = LobbyRow {
            players: 10,
            ..waiting_half
        };
        assert_eq!(full.refusal(true), Some(Refusal::Full));

        let mut strings = vec![
            Refusal::InProgress.reason(),
            Refusal::Full.reason(),
            Refusal::DifferentMod.reason(),
        ];
        strings.sort_unstable();
        strings.dedup();
        assert_eq!(strings.len(), 3, "two refusals share a string");
    }

    /// A started game is not worth mentioning the mod of, and a full one is not worth
    /// mentioning either: the first reason asked is the one that is shown.
    #[test]
    fn the_first_reason_is_the_one_that_matters() {
        let hopeless = LobbyRow {
            waiting: false,
            players: 12,
            capacity: 10,
        };
        assert_eq!(hopeless.refusal(false), Some(Refusal::InProgress));
    }

    /// The one the Electron browser gets wrong.
    ///
    /// It disables the join button on `current_players === max_players`, so a lobby the
    /// server reports as over its own limit is offered as joinable and the join fails at
    /// the server. `is_full` is `>=` for exactly this reason, and the ordering was
    /// corrected when it was ported; the button is the second place it had to be.
    #[test]
    fn a_lobby_over_its_own_limit_is_full_here_too() {
        let overfull = LobbyRow {
            waiting: true,
            players: 11,
            capacity: 10,
        };
        assert!(overfull.is_full());
        assert_eq!(overfull.refusal(true), Some(Refusal::Full));
        assert!(!overfull.joinable(true));
    }

    const WAITING_HALF: LobbyRow = LobbyRow {
        waiting: true,
        players: 5,
        capacity: 10,
    };
    const WAITING_FULL: LobbyRow = LobbyRow {
        waiting: true,
        players: 10,
        capacity: 10,
    };
    const WAITING_NEARLY: LobbyRow = LobbyRow {
        waiting: true,
        players: 9,
        capacity: 10,
    };
    const PLAYING: LobbyRow = LobbyRow {
        waiting: false,
        players: 5,
        capacity: 10,
    };
    const BUSY_GAME: LobbyRow = LobbyRow {
        waiting: false,
        players: 9,
        capacity: 10,
    };

    const ALL: [LobbyRow; 5] = [
        WAITING_HALF,
        WAITING_FULL,
        WAITING_NEARLY,
        PLAYING,
        BUSY_GAME,
    ];

    #[test]
    fn the_ordering_is_total() {
        // The bug this port exists to not reproduce. The TypeScript returned -1 for both
        // `(full, half)` and `(half, full)`.
        for a in ALL {
            for b in ALL {
                assert_eq!(a.cmp(&b), b.cmp(&a).reverse(), "{a:?} against {b:?}");
            }
        }
    }

    #[test]
    fn the_ordering_is_transitive() {
        // The other half of a total order, and the half a two-element test never reaches.
        for a in ALL {
            for b in ALL {
                for c in ALL {
                    if a <= b && b <= c {
                        assert!(a <= c, "{a:?} <= {b:?} <= {c:?} but not {a:?} <= {c:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_result_does_not_depend_on_the_order_the_server_sent() {
        // The symptom: a list that reshuffled between refreshes.
        let mut forwards = [WAITING_FULL, WAITING_HALF];
        let mut backwards = [WAITING_HALF, WAITING_FULL];
        sort(&mut forwards);
        sort(&mut backwards);
        assert_eq!(forwards, backwards);
    }

    #[test]
    fn a_full_lobby_never_sits_above_one_with_room() {
        let mut rows = [WAITING_FULL, WAITING_HALF];
        sort(&mut rows);
        assert_eq!(rows, [WAITING_HALF, WAITING_FULL]);
    }

    #[test]
    fn a_lobby_that_has_not_started_comes_first_however_busy_the_game_is() {
        let mut rows = [BUSY_GAME, WAITING_HALF];
        sort(&mut rows);
        assert_eq!(rows, [WAITING_HALF, BUSY_GAME]);
    }

    #[test]
    fn the_fullest_joinable_lobby_is_first() {
        // Eight players is a game about to start; two is a wait.
        let mut rows = [WAITING_HALF, WAITING_NEARLY];
        sort(&mut rows);
        assert_eq!(rows, [WAITING_NEARLY, WAITING_HALF]);
    }

    #[test]
    fn a_lobby_over_its_own_limit_counts_as_full() {
        // The server has reported this. A strict equality called it joinable and put it at
        // the top, because it also had the most players.
        let over = LobbyRow {
            waiting: true,
            players: 11,
            capacity: 10,
        };
        assert!(over.is_full());
        let mut rows = [over, WAITING_HALF];
        sort(&mut rows);
        assert_eq!(rows, [WAITING_HALF, over]);
    }

    #[test]
    fn equal_rows_keep_the_order_they_arrived_in() {
        // `sort` is stable, so a tie is left as the server sent it rather than given an
        // order this client invented.
        let first = LobbyRow {
            waiting: true,
            players: 4,
            capacity: 10,
        };
        let second = first;
        let mut rows = [first, second];
        sort(&mut rows);
        assert_eq!(rows, [first, second]);
    }

    #[test]
    fn the_whole_list_comes_out_in_the_documented_order() {
        let mut rows = ALL;
        sort(&mut rows);
        assert_eq!(
            rows,
            [
                // Joinable, fullest first.
                WAITING_NEARLY,
                WAITING_HALF,
                // Waiting but full.
                WAITING_FULL,
                // In progress, fullest first.
                BUSY_GAME,
                PLAYING,
            ]
        );
    }
}
