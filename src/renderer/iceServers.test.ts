import { describe, expect, it } from 'vitest';
import { hasRelay, isRelayUrl, withTcpRelays, withTransportPolicy } from './iceServers';

// What the project's own server advertises, copied from a probe of it rather than
// written from memory: one entry naming TCP and one bare, which means UDP. The bare
// entry is why the deduplication below is needed at all -- without it this client adds
// a TCP form of it and ends up asking the same relay for two allocations per peer.
const LIVE: RTCIceServer[] = [
	{ urls: 'stun:stun.l.google.com:19302' },
	{ urls: 'turn:aucl.greluc.me:3478?transport=tcp', username: 'acl', credential: 'secret' },
	{ urls: 'turn:aucl.greluc.me:3478', username: 'acl', credential: 'secret' },
];

const urlsOf = (servers: RTCIceServer[]): string[] =>
	servers.flatMap((server) => [].concat(server.urls as never).map(String));

describe('withTcpRelays', () => {
	it('adds a TCP form of a relay advertised without a transport', () => {
		const out = withTcpRelays([{ urls: 'turn:relay.example:3478', username: 'u', credential: 'p' }]);
		expect(urlsOf(out)).toEqual(['turn:relay.example:3478', 'turn:relay.example:3478?transport=tcp']);
	});

	it('carries the credentials onto the entry it adds', () => {
		// A relay URL without them allocates nothing, so the added entry would be a
		// candidate that never gathers and a failure that looks like the relay is down.
		const [, added] = withTcpRelays([{ urls: 'turn:relay.example:3478', username: 'u', credential: 'p' }]);
		expect(added).toMatchObject({ username: 'u', credential: 'p' });
	});

	it('leaves UDP first', () => {
		// ICE tries candidates in the order it is given them, and a TCP relay is a worse
		// path for every player who can use the other one.
		const out = urlsOf(withTcpRelays([{ urls: 'turn:relay.example:3478' }]));
		expect(out.indexOf('turn:relay.example:3478')).toBeLessThan(out.indexOf('turn:relay.example:3478?transport=tcp'));
	});

	it('adds nothing when the server already advertises both', () => {
		// The regression this exists for. The browser allocates once per entry, so a
		// third entry naming a relay already on the list means every peer holds two TCP
		// allocations instead of one, and a relay's port range is finite.
		expect(urlsOf(withTcpRelays(LIVE))).toEqual(urlsOf(LIVE));
	});

	it('leaves a URL that names its own transport alone', () => {
		const out = urlsOf(withTcpRelays([{ urls: 'turn:relay.example:3478?transport=tcp' }]));
		expect(out).toEqual(['turn:relay.example:3478?transport=tcp']);
	});

	it('leaves TLS relays and STUN alone', () => {
		// `turns:` is TLS over TCP already, and a STUN server has no transport to force.
		const out = urlsOf(withTcpRelays([{ urls: 'turns:relay.example:5349' }, { urls: 'stun:stun.example:3478' }]));
		expect(out).toEqual(['turns:relay.example:5349', 'stun:stun.example:3478']);
	});

	it('handles a server that lists several URLs at once', () => {
		const out = urlsOf(withTcpRelays([{ urls: ['turn:a.example:3478', 'turn:b.example:3478?transport=tcp'] }]));
		expect(out).toEqual([
			'turn:a.example:3478',
			'turn:b.example:3478?transport=tcp',
			'turn:a.example:3478?transport=tcp',
		]);
	});

	it('does not add the same URL twice for two servers that name the same relay', () => {
		const out = urlsOf(withTcpRelays([{ urls: 'turn:relay.example:3478' }, { urls: 'turn:relay.example:3478' }]));
		expect(out.filter((url) => url.endsWith('transport=tcp'))).toHaveLength(1);
	});
});

describe('isRelayUrl', () => {
	it('recognises the TLS relay', () => {
		// `'turns:host'.includes('turn:')` is false — the `s` sits between the word and
		// the colon — so a deployment offering only `turns:` once read as having no relay.
		expect(isRelayUrl('turns:relay.example:5349')).toBe(true);
	});

	it('recognises a plain relay and rejects STUN', () => {
		expect(isRelayUrl('turn:relay.example:3478')).toBe(true);
		expect(isRelayUrl('stun:stun.example:3478')).toBe(false);
	});

	it('reads a list', () => {
		expect(isRelayUrl(['stun:stun.example:3478', 'turn:relay.example:3478'])).toBe(true);
	});
});

describe('hasRelay', () => {
	it('is false for a STUN-only server, which is what forcing relay mode must not do', () => {
		// Forcing relay with nothing to relay through gathers no candidate at all, which
		// fails harder than the direct attempt it replaced.
		expect(hasRelay({ iceServers: [{ urls: 'stun:stun.l.google.com:19302' }] })).toBe(false);
	});

	it('is false when no servers were sent at all', () => {
		expect(hasRelay({})).toBe(false);
	});

	it('is true for the live configuration', () => {
		expect(hasRelay({ iceServers: LIVE })).toBe(true);
	});
});

describe('withTransportPolicy', () => {
	it('bundles everything onto one transport', () => {
		// One set of connectivity checks instead of two, one DTLS handshake, and one relay
		// allocation per peer rather than two. In a fourteen-player lobby that is
		// ninety-one connections against a finite range of relay ports.
		expect(withTransportPolicy({}).bundlePolicy).toBe('max-bundle');
	});

	it('keeps everything the server decided', () => {
		// It must not quietly drop the relay list or the transport policy on its way
		// through, which is the only way this could do harm.
		const config: RTCConfiguration = { iceTransportPolicy: 'relay', iceServers: LIVE };
		expect(withTransportPolicy(config)).toMatchObject(config);
	});
});
