/**
 * The order public lobbies are listed in.
 *
 * Lifted out of `LobbyBrowser.tsx` for the reason `signalRoute.ts` and `iceServers.ts`
 * were lifted out of `Voice.tsx`: the component needs a DOM and MUI, the tests here run
 * under node, and this is a pure function of two rows.
 *
 * # The bug this move exposed
 *
 * The comparator it replaces was not a consistent ordering. It had one rule for a full
 * lobby and applied it in one direction only:
 *
 * ```js
 * if (b.current_players === b.max_players && a.current_players !== a.max_players) return -1;
 * ```
 *
 * So a full lobby compared against a joinable one fell through to "more players first"
 * and won, while the joinable one compared against the full one hit the rule and also
 * won. Both orderings claimed to come first, which makes `Array.prototype.sort`'s result
 * implementation-defined: the same two lobbies came out in a different order depending on
 * the order the server happened to send them.
 *
 * What a player saw was a list that reshuffled between refreshes, with full lobbies
 * sometimes sitting above the ones they could actually join — which is the thing the rule
 * was written to prevent.
 */

import { GameState } from '../../common/AmongUsState';
import type { PublicLobby } from '../../common/PublicLobby';

/** Whether a lobby has no room left. */
function isFull(lobby: PublicLobby): boolean {
	return lobby.current_players >= lobby.max_players;
}

/**
 * Orders one pair of lobbies.
 *
 * Three keys, each applied in both directions so the result is a total order:
 *
 * 1. **Lobbies that have not started yet come first.** A game in progress cannot be
 *    joined, so it is the least useful row on the screen.
 * 2. **Then the ones with room.** Same reason, one step weaker: a full lobby may empty.
 * 3. **Then the fullest first**, because a lobby with eight players is a game about to
 *    start and one with two is a wait.
 *
 * Ties are left alone. The caller sorts a list that came from an object, and a stable
 * sort keeps whatever order that produced rather than inventing one.
 */
export function sortLobbies(a: PublicLobby, b: PublicLobby): number {
	const aWaiting = a.gameState === GameState.LOBBY;
	const bWaiting = b.gameState === GameState.LOBBY;
	if (aWaiting !== bWaiting) return aWaiting ? -1 : 1;

	const aFull = isFull(a);
	const bFull = isFull(b);
	if (aFull !== bFull) return aFull ? 1 : -1;

	return b.current_players - a.current_players;
}
