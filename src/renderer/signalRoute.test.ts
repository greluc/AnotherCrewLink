import { describe, expect, it } from 'vitest';
import { routeSignal } from './signalRoute';

const offer = { isOffer: true, isRenegotiation: false };
const renegotiation = { isOffer: true, isRenegotiation: true };
const answerOrCandidate = { isOffer: false, isRenegotiation: false };

const live = { exists: true, hasSession: true };
const halfBuilt = { exists: true, hasSession: false };
const nothing = { exists: false, hasSession: false };

describe('routeSignal', () => {
	it('applies a renegotiation to the connection it is renegotiating', () => {
		// The one this file exists for. An ICE restart sends this offer to keep a stalled
		// connection alive; rebuilding for it destroys exactly what was being repaired,
		// and the player hears a repair attempt in the log and silence in their ears.
		expect(routeSignal(renegotiation, live)).toBe('existing');
	});

	it('builds a connection for a first offer', () => {
		expect(routeSignal(offer, nothing)).toBe('create');
	});

	it('replaces a half-built connection when a first offer arrives', () => {
		// Both ends tried to open at once, or the far end gave up and started again. Its
		// new offer carries new ICE credentials and a new certificate, so answering it on
		// this end's abandoned attempt would be answering with the wrong session.
		expect(routeSignal(offer, halfBuilt)).toBe('create');
		expect(routeSignal(offer, live)).toBe('create');
	});

	it('rebuilds for a renegotiation this end has no session for', () => {
		// The marker says the far end thinks it is continuing something. If this end has
		// nothing to continue, believing it would answer from an empty connection.
		expect(routeSignal(renegotiation, halfBuilt)).toBe('create');
		expect(routeSignal(renegotiation, nothing)).toBe('create');
	});

	it('sends an answer or a candidate to the connection that is waiting for it', () => {
		expect(routeSignal(answerOrCandidate, live)).toBe('existing');
		expect(routeSignal(answerOrCandidate, halfBuilt)).toBe('existing');
	});

	it('drops an answer or a candidate with nothing to apply it to', () => {
		// A candidate for a connection that has already been torn down. Applying it
		// somewhere else would be worse than losing it.
		expect(routeSignal(answerOrCandidate, nothing)).toBe('drop');
	});

	it('never drops an offer', () => {
		// An offer is always actionable: either it continues a session or it starts one.
		// Dropping one leaves the far end waiting for an answer that never comes, and it
		// will keep offering until it gives up.
		for (const peer of [live, halfBuilt, nothing]) {
			for (const signal of [offer, renegotiation]) {
				expect(routeSignal(signal, peer)).not.toBe('drop');
			}
		}
	});

	it('treats an unmarked offer as a first offer however the session looks', () => {
		// A client older than the marker never sets it. Its renegotiations rebuild, which
		// is what they did before this existed -- not ideal, and not a regression.
		expect(routeSignal(offer, live)).toBe('create');
	});
});
