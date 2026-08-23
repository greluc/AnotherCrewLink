import { describe, expect, it } from 'vitest';
import { validateClientPeerConfig } from './validateClientPeerConfig';

// This runs against whatever the connected server sends, which the user can point
// anywhere, so the interesting cases are the malformed ones.

const valid = {
	forceRelayOnly: false,
	iceServers: [
		{ urls: 'stun:stun.l.google.com:19302' },
		{ urls: 'turn:turn.example.com:3478', username: 'u', credential: 'c' },
	],
};

describe('validateClientPeerConfig', () => {
	it('accepts what the server actually sends', () => {
		expect(validateClientPeerConfig(valid)).toBe(true);
		expect(validateClientPeerConfig.errors).toHaveLength(0);
	});

	it('accepts an array of urls', () => {
		expect(validateClientPeerConfig({ ...valid, iceServers: [{ urls: ['stun:a:1', 'turn:b:2'] }] })).toBe(true);
	});

	it('rejects a missing forceRelayOnly', () => {
		expect(validateClientPeerConfig({ iceServers: [] })).toBe(false);
		expect(validateClientPeerConfig.errors.join()).toMatch(/forceRelayOnly/);
	});

	it('rejects forceRelayOnly of the wrong type', () => {
		expect(validateClientPeerConfig({ ...valid, forceRelayOnly: 'yes' })).toBe(false);
	});

	it('rejects iceServers that is not an array', () => {
		expect(validateClientPeerConfig({ forceRelayOnly: false, iceServers: {} })).toBe(false);
		expect(validateClientPeerConfig.errors.join()).toMatch(/iceServers/);
	});

	it('rejects an ice server without urls', () => {
		expect(validateClientPeerConfig({ forceRelayOnly: false, iceServers: [{ username: 'u' }] })).toBe(false);
		expect(validateClientPeerConfig.errors.join()).toMatch(/urls is required/);
	});

	it('rejects a non-uri url', () => {
		expect(validateClientPeerConfig({ forceRelayOnly: false, iceServers: [{ urls: 'not a uri' }] })).toBe(false);
	});

	it('rejects credentials of the wrong type', () => {
		expect(
			validateClientPeerConfig({ forceRelayOnly: false, iceServers: [{ urls: 'stun:a:1', credential: 42 }] })
		).toBe(false);
	});

	it('rejects null and non-objects outright', () => {
		expect(validateClientPeerConfig(null)).toBe(false);
		expect(validateClientPeerConfig('nope')).toBe(false);
	});

	it('reports the position of the offending server', () => {
		validateClientPeerConfig({ forceRelayOnly: false, iceServers: [{ urls: 'stun:a:1' }, { urls: 5 }] });
		expect(validateClientPeerConfig.errors.join()).toMatch(/\/iceServers\/1\/urls/);
	});

	it('clears errors between calls', () => {
		validateClientPeerConfig(null);
		expect(validateClientPeerConfig.errors.length).toBeGreaterThan(0);
		validateClientPeerConfig(valid);
		expect(validateClientPeerConfig.errors).toHaveLength(0);
	});
});
