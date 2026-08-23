/**
 * Validates the peer configuration a server sends on connect.
 *
 * This used to be an Ajv schema. Ajv compiles schemas by generating JavaScript and
 * evaluating it at runtime, which a Content-Security-Policy without 'unsafe-eval'
 * forbids, so the choice was between weakening the policy and writing the check out.
 * The shape is small enough that writing it out is the better trade, and it drops
 * two dependencies.
 */

export interface ValidatedIceServer {
	urls: string | string[];
	username?: string;
	credential?: string;
}

export interface ValidatedClientPeerConfig {
	forceRelayOnly: boolean;
	iceServers: ValidatedIceServer[];
}

const errors: string[] = [];

function isUri(value: unknown): value is string {
	if (typeof value !== 'string' || value.length === 0) return false;
	try {
		// STUN and TURN URLs are not parseable by URL(), so accept their scheme directly.
		if (/^(stun|stuns|turn|turns):/i.test(value)) return true;
		new URL(value);
		return true;
	} catch {
		return false;
	}
}

function checkIceServer(server: unknown, index: number): boolean {
	const where = `/iceServers/${index}`;
	if (typeof server !== 'object' || server === null) {
		errors.push(`${where} must be an object`);
		return false;
	}
	const candidate = server as Record<string, unknown>;

	if (candidate.urls === undefined) {
		errors.push(`${where}/urls is required`);
		return false;
	}
	if (Array.isArray(candidate.urls)) {
		if (!candidate.urls.every(isUri)) {
			errors.push(`${where}/urls must contain only URIs`);
			return false;
		}
	} else if (!isUri(candidate.urls)) {
		errors.push(`${where}/urls must be a URI`);
		return false;
	}

	for (const key of ['username', 'credential'] as const) {
		if (candidate[key] !== undefined && typeof candidate[key] !== 'string') {
			errors.push(`${where}/${key} must be a string`);
			return false;
		}
	}
	return true;
}

/** Mirrors the Ajv-compiled validator it replaces: returns a boolean, exposes `errors`. */
export function validateClientPeerConfig(value: unknown): value is ValidatedClientPeerConfig {
	errors.length = 0;

	if (typeof value !== 'object' || value === null) {
		errors.push(' must be an object');
		return false;
	}
	const config = value as Record<string, unknown>;

	if (typeof config.forceRelayOnly !== 'boolean') {
		errors.push('/forceRelayOnly must be a boolean');
	}
	if (!Array.isArray(config.iceServers)) {
		errors.push('/iceServers must be an array');
		return false;
	}
	config.iceServers.forEach(checkIceServer);

	return errors.length === 0;
}

validateClientPeerConfig.errors = errors;
