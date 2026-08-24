/**
 * What the client does with the ICE servers a server advertises.
 *
 * Lifted out of `Voice.tsx` because it is the piece most likely to decide whether a player
 * on a restrictive network can be heard at all, and `Voice.tsx` is not unit-tested: it
 * needs a DOM and an Electron renderer, and the tests here run under node.
 */

/**
 * Adds a TCP form of any relay that was advertised without a transport.
 *
 * A `turn:` URL with no `?transport=` means UDP. A player on a network that blocks
 * outbound UDP — most schools, many offices, some mobile carriers — cannot reach that
 * relay at all, and those are exactly the networks that needed a relay to begin with. The
 * symptom is a player who hears nobody and whom nobody hears while everything else works,
 * because the signalling runs over TLS and is fine.
 *
 * The server should advertise both, and this client's server now does. This is here for
 * the ones that do not: a relay that already answers on TCP costs nothing to try, and a
 * relay that does not simply produces no candidate from the extra entry.
 *
 * UDP stays first. ICE tries candidates in the order it is given them, and a TCP relay is
 * a worse path for everyone who can use the other one.
 */
export function withTcpRelays(servers: RTCIceServer[]): RTCIceServer[] {
	// What is already on offer, so a server that advertises both does not get a third
	// entry pointing at the relay it just named. A duplicate URL is not harmless: the
	// browser allocates once per entry, so every peer would hold two relay allocations
	// over TCP instead of one, and a relay's port range is finite.
	const advertised = new Set(servers.flatMap((server) => [].concat(server.urls as never).map(String)));

	const out: RTCIceServer[] = [];
	for (const server of servers) {
		out.push(server);
		const urls = [].concat(server.urls as never).map(String);
		for (const url of urls) {
			// `turns:` is TLS over TCP already, so it needs nothing. A URL that names its
			// transport has been decided by whoever wrote it.
			if (!url.startsWith('turn:') || url.includes('transport=')) continue;
			const overTcp = `${url}?transport=tcp`;
			if (advertised.has(overTcp)) continue;
			advertised.add(overTcp);
			out.push({ ...server, urls: overTcp });
		}
	}
	return out;
}

/**
 * Whether one advertised server is a relay.
 *
 * `turn:` and `turns:` both are, and checking only for `turn:` misses the TLS one —
 * `'turns:host'.includes('turn:')` is false, because the `s` sits between the word and
 * the colon. A deployment that offers only `turns:` would have been treated as having no
 * relay at all, which is the configuration a cautious admin is most likely to choose.
 */
export function isRelayUrl(urls: RTCIceServer['urls']): boolean {
	return [].concat(urls as never).some((url) => String(url).startsWith('turn:') || String(url).startsWith('turns:'));
}

/**
 * Whether there is a relay to fall back to.
 *
 * Forcing relay mode with no relay advertised produces a connection that cannot gather
 * any candidate at all, which fails faster and more completely than the direct attempt it
 * replaced. Checked rather than assumed, and logged when it is missing: a lobby where
 * nobody can reach anybody and no relay is offered is a server configuration problem, not
 * a client one, and the two look identical from the user's side.
 */
export function hasRelay(config: RTCConfiguration): boolean {
	return (config.iceServers ?? []).some((server) => isRelayUrl(server.urls));
}
