/**
 * Where an incoming signal goes.
 *
 * Four lines of branching in `Voice.tsx` used to decide this, and one of the four was
 * wrong in a way nothing could catch: every offer was treated as the start of a new
 * connection, so a renegotiation offer -- which exists to keep a connection alive --
 * destroyed it. The repair for a stalled connection was the thing that killed it.
 *
 * It lives here so it can be tested. `Voice.tsx` needs a DOM and an Electron renderer;
 * this needs neither, and the decision is a pure function of two facts.
 */

/** What the signalling layer should do with one incoming signal. */
export type SignalRoute =
	/** Apply it to the connection already running with this peer. */
	| 'existing'
	/** Build a connection to answer with, replacing anything there. */
	| 'create'
	/** Nothing to apply it to. */
	| 'drop';

/**
 * Decides where a signal goes.
 *
 * `hasSession` means this end already applied a remote description for that peer, so
 * there is a live session to continue rather than an empty shell.
 *
 * The two conditions for continuing are both required and mean different things. The
 * marker is the sender's intent, and a sender that predates it does not set it -- so an
 * older client's renegotiation still rebuilds, exactly as it does today, rather than being
 * applied to a session the sender may not think it has. `hasSession` is this end's own
 * state, and without it there is nothing to apply an offer to.
 */
export function routeSignal(
	signal: { isOffer: boolean; isRenegotiation: boolean },
	peer: { exists: boolean; hasSession: boolean }
): SignalRoute {
	if (signal.isOffer) {
		if (signal.isRenegotiation && peer.exists && peer.hasSession) return 'existing';
		// A first offer, an offer from a peer that rebuilt its side, or a renegotiation
		// this end has no session for. All of them want a fresh connection to answer with.
		return 'create';
	}
	// An answer or a trickled candidate. There is nothing sensible to do with either
	// without the connection they belong to.
	return peer.exists ? 'existing' : 'drop';
}
