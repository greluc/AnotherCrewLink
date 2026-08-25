import { describe, expect, it } from 'vitest';
import { GameState } from '../../common/AmongUsState';
import type { PublicLobby } from '../../common/PublicLobby';
import { sortLobbies } from './sortLobbies';

function lobby(overrides: Partial<PublicLobby> & { title: string }): PublicLobby {
	return {
		id: 0,
		host: 'host',
		current_players: 5,
		max_players: 10,
		language: 'en',
		mods: '',
		isPublic: true,
		server: 'https://aucl.greluc.me',
		gameState: GameState.LOBBY,
		stateTime: 0,
		...overrides,
	};
}

const waitingHalf = lobby({ title: 'waiting-half', current_players: 5 });
const waitingFull = lobby({ title: 'waiting-full', current_players: 10 });
const waitingNearlyFull = lobby({ title: 'waiting-nearly', current_players: 9 });
const playing = lobby({ title: 'playing', gameState: GameState.TASKS, current_players: 5 });

const order = (lobbies: PublicLobby[]): string[] => [...lobbies].sort(sortLobbies).map((l) => l.title);

describe('sortLobbies', () => {
	it('is a consistent ordering', () => {
		// The bug this file exists for. The comparator it replaces returned -1 for both
		// `(full, half)` and `(half, full)` — each claiming to come first — which makes
		// the result of `Array.prototype.sort` implementation-defined.
		const all = [waitingHalf, waitingFull, waitingNearlyFull, playing];
		for (const a of all) {
			for (const b of all) {
				const forward = sortLobbies(a, b);
				const backward = sortLobbies(b, a);
				// Written as two implications rather than `sign(x) === -sign(y)`, because
				// `-Math.sign(0)` is `-0` and `toBe` uses `Object.is`, where that is not `0`.
				expect(forward > 0).toBe(backward < 0);
				expect(forward < 0).toBe(backward > 0);
			}
		}
	});

	it('gives the same answer whichever order the server sent them in', () => {
		// The symptom: a list that reshuffled between refreshes, because the server's
		// iteration order decided the result.
		expect(order([waitingFull, waitingHalf])).toEqual(order([waitingHalf, waitingFull]));
	});

	it('never puts a full lobby above one with room', () => {
		// What the original rule was for, and what it did not achieve in one of the two
		// directions.
		expect(order([waitingFull, waitingHalf])).toEqual(['waiting-half', 'waiting-full']);
		expect(order([waitingHalf, waitingFull])).toEqual(['waiting-half', 'waiting-full']);
	});

	it('puts lobbies that have not started above ones in progress', () => {
		// A game in progress cannot be joined, so it is the least useful row on screen.
		expect(order([playing, waitingHalf])).toEqual(['waiting-half', 'playing']);
	});

	it('prefers a waiting lobby even when the one in progress has more players', () => {
		const busyGame = lobby({ title: 'busy-game', gameState: GameState.TASKS, current_players: 9 });
		expect(order([busyGame, waitingHalf])).toEqual(['waiting-half', 'busy-game']);
	});

	it('puts the fullest joinable lobby first', () => {
		// Eight players is a game about to start; two is a wait.
		expect(order([waitingHalf, waitingNearlyFull])).toEqual(['waiting-nearly', 'waiting-half']);
	});

	it('treats a lobby over its own limit as full', () => {
		// The server has reported this. A strict equality check called it joinable and put
		// it at the top, because it also had the most players.
		const over = lobby({ title: 'over', current_players: 11, max_players: 10 });
		expect(order([over, waitingHalf])).toEqual(['waiting-half', 'over']);
	});

	it('leaves equal lobbies in the order they arrived', () => {
		const first = lobby({ title: 'first' });
		const second = lobby({ title: 'second' });
		expect(order([first, second])).toEqual(['first', 'second']);
		expect(order([second, first])).toEqual(['second', 'first']);
	});
});
