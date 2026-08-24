/**
 * When a peer connection that failed mid-lobby gets rebuilt, and by which of the two
 * ends.
 *
 * The connection itself is torn down by whichever side notices the failure, and both
 * ends then want it back. If both sent an offer they would each answer the other's and
 * neither would finish negotiating, so exactly one end offers: the one whose socket id
 * sorts higher. The other end still schedules an attempt, delayed by a grace period,
 * for the case where only one side saw anything go wrong.
 */

export const RECONNECT_BASE_DELAY = 2000;
export const RECONNECT_MAX_DELAY = 30000;
export const RECONNECT_ANSWER_GRACE = 6000;
export const RECONNECT_MAX_ATTEMPTS = 6;

/**
 * After this many failed attempts, stop trying to reach the peer directly.
 *
 * A direct path that failed twice will keep failing: the reason is the network between
 * the two ends, and waiting longer does not change it. Symmetric NAT and carrier-grade
 * NAT are the usual ones, and no amount of STUN gets through either — a relay does.
 *
 * Two, not one: a single failure is often a lost packet or a peer that was still
 * starting up, and routing a whole lobby through a relay that was not needed costs the
 * relay's bandwidth and adds a hop of latency to every voice.
 */
export const RECONNECT_RELAY_AFTER = 2;

/** True if this end is the one that sends the offer when rebuilding. */
export function initiatesReconnect(ownSocketId: string, peerSocketId: string): boolean {
	return ownSocketId > peerSocketId;
}

/**
 * Delay before the given attempt, counted from 1. It doubles per attempt so a peer
 * that is simply unreachable is not retried in a tight loop.
 */
export function reconnectDelay(attempt: number, initiates: boolean): number {
	const backoff = Math.min(RECONNECT_BASE_DELAY * 2 ** (attempt - 1), RECONNECT_MAX_DELAY);
	return initiates ? backoff : backoff + RECONNECT_ANSWER_GRACE;
}

/**
 * True once a direct connection has failed often enough to be worth giving up on.
 *
 * The caller still has to have a relay to escalate to; without one this says nothing
 * useful, which is itself worth logging, because a lobby that cannot reach anyone and has
 * no relay advertised is a server configuration problem rather than a client one.
 */
export function shouldForceRelay(attempt: number): boolean {
	return attempt >= RECONNECT_RELAY_AFTER;
}

/**
 * Whether the next attempt to one peer should be forced through the relay.
 *
 * `shouldForceRelay` alone waits for two failures before it tries the relay, and each of
 * those costs a connect timeout plus a backoff — the better part of a minute during which
 * a player is simply missing from the conversation. Two things let it decide sooner, and
 * both come from evidence the connection itself produced.
 *
 * **`relayCandidates` above zero means the relay answered.** The allocation succeeded, so
 * the relay is reachable from this machine, and the direct path failed anyway. There is
 * nothing to learn from failing at it a second time.
 *
 * **`relayCandidates` at zero means the allocation failed**, and forcing relay-only would
 * be worse than doing nothing: with no relay candidate to offer there would be no
 * candidate at all, so a connection that sometimes succeeds directly would stop succeeding
 * ever. The honest answer there is that this client cannot fix it, and the log says so.
 *
 * **`otherPeersNeededRelay` carries the lobby's experience across peers.** What blocks a
 * direct path is almost always the network at one end, not the pair, so the second peer to
 * fail is evidence about the eleventh. Once one connection has needed the relay, later
 * ones start there instead of each rediscovering it a minute at a time.
 *
 * `undefined` means nothing was observed — no failure yet, or a peer built before this
 * client learned to count. It falls back to the attempt count, which is what the previous
 * behaviour was.
 */
export function shouldUseRelay(options: {
	attempt: number;
	relayCandidates: number | undefined;
	otherPeersNeededRelay: boolean;
}): boolean {
	const { attempt, relayCandidates, otherPeersNeededRelay } = options;

	// Nothing gathered a relay candidate: relay-only would have no candidates at all.
	// Checked before the lobby-wide signal, because "the relay works for somebody else"
	// does not make it work from here.
	if (relayCandidates === 0) return false;

	if (otherPeersNeededRelay) return true;
	if (relayCandidates !== undefined && relayCandidates > 0) return true;
	return shouldForceRelay(attempt);
}

/** True once the attempt number is past what is worth trying. */
export function shouldGiveUp(attempt: number): boolean {
	return attempt > RECONNECT_MAX_ATTEMPTS;
}
