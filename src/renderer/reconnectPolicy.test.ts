import { describe, expect, it } from 'vitest';
import {
	RECONNECT_ANSWER_GRACE,
	RECONNECT_RELAY_AFTER,
	RECONNECT_BASE_DELAY,
	RECONNECT_MAX_ATTEMPTS,
	RECONNECT_MAX_DELAY,
	initiatesReconnect,
	reconnectDelay,
	shouldForceRelay,
	shouldGiveUp,
	shouldUseRelay,
} from './reconnectPolicy';

// socket.io ids as they actually look, plus pairs that differ only late in the string.
const IDS = [
	'0NNRYKaxTPXTusamAAAD',
	'L3irzfbdl-cdX4KIAAAH',
	'qyocGexuH_Xzr19DAAAL',
	'U-fi5qR0rG4EsTRgAAAX',
	'aaaaaaaaaaaaaaaaAAAA',
	'aaaaaaaaaaaaaaaaAAAB',
];

describe('initiatesReconnect', () => {
	it('picks exactly one of the two ends', () => {
		for (const a of IDS) {
			for (const b of IDS) {
				if (a === b) continue;
				// Both ends run this with their own id first. Precisely one may offer,
				// or they would answer each other and neither connection would come up.
				expect(initiatesReconnect(a, b) !== initiatesReconnect(b, a)).toBe(true);
			}
		}
	});

	it('agrees with itself when called repeatedly', () => {
		expect(initiatesReconnect(IDS[0], IDS[1])).toBe(initiatesReconnect(IDS[0], IDS[1]));
	});

	it('does not offer to itself, whatever that would mean', () => {
		expect(initiatesReconnect(IDS[0], IDS[0])).toBe(false);
	});
});

describe('reconnectDelay', () => {
	it('starts at the base delay for the offering end', () => {
		expect(reconnectDelay(1, true)).toBe(RECONNECT_BASE_DELAY);
	});

	it('doubles per attempt', () => {
		expect(reconnectDelay(2, true)).toBe(RECONNECT_BASE_DELAY * 2);
		expect(reconnectDelay(3, true)).toBe(RECONNECT_BASE_DELAY * 4);
	});

	it('stops growing at the cap', () => {
		expect(reconnectDelay(20, true)).toBe(RECONNECT_MAX_DELAY);
	});

	it('makes the answering end wait longer, so the offering end gets there first', () => {
		for (let attempt = 1; attempt <= RECONNECT_MAX_ATTEMPTS; attempt++) {
			const lead = reconnectDelay(attempt, false) - reconnectDelay(attempt, true);
			expect(lead).toBe(RECONNECT_ANSWER_GRACE);
			expect(lead).toBeGreaterThan(0);
		}
	});

	it('leaves the answering end enough of a lead to notice an offer that already arrived', () => {
		// The grace has to outlast a round trip through the relay server plus ICE, or
		// both ends would rebuild and one connection would be thrown away every time.
		expect(RECONNECT_ANSWER_GRACE).toBeGreaterThanOrEqual(5000);
	});
});

describe('shouldGiveUp', () => {
	it('keeps trying up to the limit', () => {
		for (let attempt = 1; attempt <= RECONNECT_MAX_ATTEMPTS; attempt++) {
			expect(shouldGiveUp(attempt)).toBe(false);
		}
	});

	it('gives up past it', () => {
		expect(shouldGiveUp(RECONNECT_MAX_ATTEMPTS + 1)).toBe(true);
	});

	it('spends minutes, not hours, before giving up', () => {
		let total = 0;
		for (let attempt = 1; attempt <= RECONNECT_MAX_ATTEMPTS; attempt++) {
			total += reconnectDelay(attempt, false);
		}
		expect(total).toBeLessThan(5 * 60 * 1000);
	});
});

describe('shouldForceRelay', () => {
	it('lets the first attempt try a direct path', () => {
		// One failure is often a lost packet or a peer that had not finished starting up.
		// Routing a lobby through a relay it did not need costs the relay's bandwidth and
		// adds a hop to every voice.
		expect(shouldForceRelay(1)).toBe(false);
	});

	it('gives up on the direct path after two failures', () => {
		// What is in the way is the network between the two ends, and waiting longer does
		// not move it. Symmetric NAT and carrier-grade NAT are the usual reasons, and no
		// amount of STUN gets through either.
		expect(shouldForceRelay(2)).toBe(true);
		expect(shouldForceRelay(3)).toBe(true);
	});

	it('still escalates on the last attempt worth making', () => {
		// The escalation has to happen while there are attempts left to use it.
		expect(shouldForceRelay(RECONNECT_MAX_ATTEMPTS)).toBe(true);
		expect(shouldGiveUp(RECONNECT_MAX_ATTEMPTS)).toBe(false);
	});
});

describe('shouldUseRelay', () => {
	const base = { attempt: 1, relayCandidates: undefined, otherPeersNeededRelay: false };

	it('goes to the relay on the first failure when the relay answered', () => {
		// The allocation succeeded and the direct path failed anyway. Failing at it a
		// second time takes the better part of a minute and teaches nothing, and the
		// player is missing from the conversation for all of it.
		expect(shouldUseRelay({ ...base, relayCandidates: 2 })).toBe(true);
	});

	it('never forces the relay when the allocation produced nothing', () => {
		// The trap. With no relay candidate to offer, relay-only leaves the connection
		// with no candidates at all -- so a peer that sometimes connects directly would
		// stop connecting ever. This is the one case where the obvious escalation makes
		// things worse.
		for (const attempt of [1, 2, 3, 8]) {
			expect(shouldUseRelay({ ...base, attempt, relayCandidates: 0 })).toBe(false);
		}
	});

	it('does not let a working relay elsewhere override a failed allocation here', () => {
		// A relay that works for somebody else does not work from this machine, and the
		// lobby-wide signal must not talk this into a configuration that cannot connect.
		expect(shouldUseRelay({ attempt: 5, relayCandidates: 0, otherPeersNeededRelay: true })).toBe(false);
	});

	it('starts later peers on the relay once one has needed it', () => {
		// What blocks a direct path is the network at one end, not the pair. The second
		// peer to fail is evidence about the eleventh, and rediscovering it per peer costs
		// a minute each.
		expect(shouldUseRelay({ ...base, otherPeersNeededRelay: true })).toBe(true);
	});

	it('falls back to the attempt count when nothing was observed', () => {
		// No failure yet, or a peer built before this client counted candidates. This is
		// the behaviour that existed before, and it has to stay reachable.
		expect(shouldUseRelay({ ...base, attempt: 1 })).toBe(false);
		expect(shouldUseRelay({ ...base, attempt: RECONNECT_RELAY_AFTER })).toBe(true);
	});

	it('agrees with shouldForceRelay when it has nothing else to go on', () => {
		for (let attempt = 1; attempt <= 6; attempt += 1) {
			expect(shouldUseRelay({ ...base, attempt })).toBe(shouldForceRelay(attempt));
		}
	});
});
