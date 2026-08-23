import { describe, expect, it } from 'vitest';
import {
	RECONNECT_ANSWER_GRACE,
	RECONNECT_BASE_DELAY,
	RECONNECT_MAX_ATTEMPTS,
	RECONNECT_MAX_DELAY,
	initiatesReconnect,
	reconnectDelay,
	shouldGiveUp,
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
