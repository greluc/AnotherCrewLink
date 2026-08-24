import type { IOffsets, IOffsetsLookup } from './offsetStore';

/**
 * Structural validation for the offsets bundle.
 *
 * The offsets decide where this client reads inside another process, and on 32-bit
 * Windows they decide where an injection stub writes. They arrive over the network from
 * a repository, and until H2 nothing between the fetch and their use looked at them at
 * all: a truncated response, a rate-limit page that happened to parse, or a hostile
 * merge on the mirror all reached `GameReader` unchanged.
 *
 * **What this catches:** shape, types, ranges, and the pattern syntax of a signature.
 * **What it cannot catch:** a structurally perfect bundle with the wrong numbers in it.
 * A plausible-but-wrong offset reads the wrong field, and no amount of validation here
 * distinguishes that from a game update. That limit is the honest boundary of this
 * layer; the answer to it is review on the mirror, and the write-side prologue check in
 * `GameReader`, not a longer function here.
 *
 * Every bound below is derived from the 44 real offsets files in
 * `test/fixtures/offsets`, not guessed. A validator that rejects real data is a
 * self-inflicted outage, and `offsetsValidator.test.ts` asserts against the whole corpus
 * for exactly that reason.
 */

/** Distinct reasons, so a rejection says which rule it broke rather than "invalid". */
export type OffsetsRejection =
	| 'not-an-object'
	| 'missing-field'
	| 'wrong-type'
	| 'chain-out-of-range'
	| 'chain-too-long'
	| 'buffer-length-absurd'
	| 'rva-out-of-module'
	| 'bad-signature'
	| 'bad-struct'
	| 'bundle-version-replayed'
	| 'client-too-old';

export class OffsetsRejected extends Error {
	constructor(
		readonly code: OffsetsRejection,
		readonly path: string,
		detail: string
	) {
		super(`offsets rejected (${code}) at ${path || '<root>'}: ${detail}`);
		this.name = 'OffsetsRejected';
	}
}

/**
 * The widest address this bundle may name, as a module-relative offset.
 *
 * The largest value in the real corpus is 0x238A6E0, roughly 37 MB into
 * `GameAssembly.dll`. This is fourteen times that, which leaves room for the module to
 * grow for years while still rejecting a value that could only be an absolute address,
 * a negative number reinterpreted, or noise.
 */
const MAX_MODULE_RVA = 0x20000000;

/**
 * Pointer chains use -1 as "not present on this build" — 20 of the 44 real files do, so
 * it is data rather than an error.
 */
const MIN_CHAIN_VALUE = -1;

/** The longest real chain is 5. */
const MAX_CHAIN_LENGTH = 16;

/**
 * `player.bufferLength` sizes an allocation in `GameReader`, which is why it is bounded
 * rather than merely typed. Real values are 56 to 136.
 */
const MIN_BUFFER_LENGTH = 8;
const MAX_BUFFER_LENGTH = 4096;

/** A pattern token is a byte or a wildcard. Both `?` and `??` appear in real data. */
const SIGNATURE_TOKEN = /^(?:[0-9A-Fa-f]{2}|\?{1,2})$/;

/** Real signatures are 8 to 75 tokens. This bounds the scan cost, not the data. */
const MAX_SIGNATURE_TOKENS = 256;

/**
 * Where in the matched pattern the address is taken from, and how far it is then stepped.
 * Real values are 0..10 and -5..4 respectively. These are deliberately close to the data:
 * a signature is the one field in the bundle that steers a pattern scan, and a scan
 * result is what the injection stub writes to on 32-bit Windows.
 */
const MAX_PATTERN_OFFSET = 256;
const MAX_ADDRESS_OFFSET = 64;

const STRUCT_TYPES = new Set([
	'INT',
	'INT_BE',
	'UINT',
	'UINT_BE',
	'SHORT',
	'SHORT_BE',
	'USHORT',
	'USHORT_BE',
	'FLOAT',
	'CHAR',
	'BYTE',
	'SKIP',
]);

/** Top-level keys every one of the 44 real files carries. */
const REQUIRED_TOP_LEVEL = [
	'meetingHud',
	'objectCachePtr',
	'meetingHudState',
	'allPlayersPtr',
	'allPlayers',
	'playerCount',
	'playerAddrPtr',
	'shipStatus',
	'lightRadius',
	'shipStatus_systems',
	'shipStatus_map',
	'shipstatus_allDoors',
	'door_doorId',
	'door_isOpen',
	'deconDoorUpperOpen',
	'deconDoorLowerOpen',
	'hqHudSystemType_CompletedConsoles',
	'HudOverrideSystemType_isActive',
	'miniGame',
	'planetSurveillanceMinigame_currentCamera',
	'planetSurveillanceMinigame_camarasCount',
	'surveillanceMinigame_FilteredRoomsCount',
	'palette',
	'palette_shadowColor',
	'palette_playercolor',
	'gameoptionsData',
	'gameOptions_MapId',
	'gameOptions_MaxPLayers',
	'connectFunc',
	'fixedUpdateFunc',
	'showModStampFunc',
	'modLateUpdateFunc',
	'pingMessageString',
	'serverManager_currentServer',
	'innerNetClient',
	'player',
	'signatures',
	'oldMeetingHud',
	'disableWriting',
	'newGameOptions',
] as const;

/**
 * The five fields that become addresses. On 32-bit builds four of them are the target of
 * an `E9 rel32` write, so they are range-checked here even though `GameReader` overwrites
 * them with a pattern-scan result before use — see the note on `validateResolvedAddress`.
 */
const RVA_FIELDS = [
	'connectFunc',
	'fixedUpdateFunc',
	'showModStampFunc',
	'modLateUpdateFunc',
	'pingMessageString',
] as const;

const INNER_NET_CLIENT_NUMBERS = [
	'networkAddress',
	'networkPort',
	'onlineScene',
	'mainMenuScene',
	'gameMode',
	'gameId',
	'hostId',
	'clientId',
	'gameState',
] as const;

const PLAYER_CHAINS = [
	'isLocal',
	'localX',
	'localY',
	'remoteX',
	'remoteY',
	'roleTeam',
	'currentOutfit',
	'offsets',
	'inVent',
	'clientId',
	'isDummy',
] as const;

const OUTFIT_CHAINS = ['colorId', 'playerName', 'hatId', 'skinId', 'visorId'] as const;

const SIGNATURE_NAMES = [
	'innerNetClient',
	'meetingHud',
	'gameData',
	'shipStatus',
	'miniGame',
	'palette',
	'playerControl',
	'connectFunc',
	'fixedUpdateFunc',
	'pingMessageString',
	'serverManager',
	'showModStamp',
	'modLateUpdate',
	'gameOptionsManager',
] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function requireRecord(value: unknown, path: string): Record<string, unknown> {
	if (!isRecord(value)) {
		throw new OffsetsRejected('not-an-object', path, `expected an object, got ${describe(value)}`);
	}
	return value;
}

function describe(value: unknown): string {
	if (value === null) return 'null';
	if (Array.isArray(value)) return `an array of ${value.length}`;
	return typeof value;
}

function requirePresent(parent: Record<string, unknown>, key: string, path: string): unknown {
	if (!Object.hasOwn(parent, key)) {
		// A truncated download that still parses lands here rather than in a type error,
		// which is why this reason is distinct from `wrong-type`.
		throw new OffsetsRejected('missing-field', path, 'field is absent');
	}
	return parent[key];
}

function requireInteger(parent: Record<string, unknown>, key: string, path: string): number {
	const value = requirePresent(parent, key, path);
	if (typeof value !== 'number' || !Number.isInteger(value)) {
		throw new OffsetsRejected('wrong-type', path, `expected an integer, got ${describe(value)}`);
	}
	return value;
}

function requireBoolean(parent: Record<string, unknown>, key: string, path: string): boolean {
	const value = requirePresent(parent, key, path);
	if (typeof value !== 'boolean') {
		throw new OffsetsRejected('wrong-type', path, `expected a boolean, got ${describe(value)}`);
	}
	return value;
}

/** An offset chain: `[base, ...steps]`, each within the module. */
function requireChain(parent: Record<string, unknown>, key: string, path: string): void {
	const value = requirePresent(parent, key, path);
	if (!Array.isArray(value)) {
		throw new OffsetsRejected('wrong-type', path, `expected an array, got ${describe(value)}`);
	}
	if (value.length > MAX_CHAIN_LENGTH) {
		throw new OffsetsRejected('chain-too-long', path, `${value.length} steps, limit is ${MAX_CHAIN_LENGTH}`);
	}
	for (const [index, step] of value.entries()) {
		if (typeof step !== 'number' || !Number.isInteger(step)) {
			throw new OffsetsRejected('wrong-type', `${path}[${index}]`, `expected an integer, got ${describe(step)}`);
		}
		if (step < MIN_CHAIN_VALUE || step > MAX_MODULE_RVA) {
			throw new OffsetsRejected(
				'chain-out-of-range',
				`${path}[${index}]`,
				`${step} is outside ${MIN_CHAIN_VALUE}..0x${MAX_MODULE_RVA.toString(16)}`
			);
		}
	}
}

/**
 * A signature entry, or an empty object.
 *
 * 90 of the 616 signature entries in the real corpus are `{}` — the x64 files carry no
 * pattern for the four write-path functions, because writing is 32-bit only. Rejecting
 * an empty entry would reject every 64-bit file this project has ever shipped.
 */
function validateSignature(value: unknown, path: string): void {
	const entry = requireRecord(value, path);
	if (Object.keys(entry).length === 0) {
		return;
	}
	const sig = requirePresent(entry, 'sig', `${path}.sig`);
	if (typeof sig !== 'string') {
		throw new OffsetsRejected('wrong-type', `${path}.sig`, `expected a string, got ${describe(sig)}`);
	}
	const tokens = sig.trim().split(/\s+/);
	if (tokens.length === 0 || tokens.length > MAX_SIGNATURE_TOKENS) {
		throw new OffsetsRejected('bad-signature', `${path}.sig`, `${tokens.length} tokens`);
	}
	for (const [index, token] of tokens.entries()) {
		if (!SIGNATURE_TOKEN.test(token)) {
			throw new OffsetsRejected('bad-signature', `${path}.sig`, `token ${index} is ${JSON.stringify(token)}`);
		}
	}
	// Both are small adjustments applied to where the pattern matched, not addresses.
	// `patternOffset` indexes into the match and is 0 to 10 across the real corpus;
	// `addressOffset` steps relative to it and is -5, 0 or 4. Bounding them tightly is
	// what stops a signature from turning a match into an arbitrary address.
	const patternOffset = requireInteger(entry, 'patternOffset', `${path}.patternOffset`);
	if (patternOffset < 0 || patternOffset > MAX_PATTERN_OFFSET) {
		throw new OffsetsRejected(
			'bad-signature',
			`${path}.patternOffset`,
			`${patternOffset} is outside 0..${MAX_PATTERN_OFFSET}`
		);
	}
	const addressOffset = requireInteger(entry, 'addressOffset', `${path}.addressOffset`);
	if (addressOffset < -MAX_ADDRESS_OFFSET || addressOffset > MAX_ADDRESS_OFFSET) {
		throw new OffsetsRejected(
			'bad-signature',
			`${path}.addressOffset`,
			`${addressOffset} is outside ±${MAX_ADDRESS_OFFSET}`
		);
	}
}

function validatePlayer(value: unknown, path: string): void {
	const player = requireRecord(value, path);
	for (const key of PLAYER_CHAINS) {
		requireChain(player, key, `${path}.${key}`);
	}
	// Present on some builds only, and read through an optional chain by the caller.
	if (Object.hasOwn(player, 'nameText') && player.nameText !== undefined) {
		requireChain(player, 'nameText', `${path}.nameText`);
	}

	const outfit = requireRecord(requirePresent(player, 'outfit', `${path}.outfit`), `${path}.outfit`);
	for (const key of OUTFIT_CHAINS) {
		requireChain(outfit, key, `${path}.outfit.${key}`);
	}

	const bufferLength = requireInteger(player, 'bufferLength', `${path}.bufferLength`);
	if (bufferLength < MIN_BUFFER_LENGTH || bufferLength > MAX_BUFFER_LENGTH) {
		// This one sizes a read buffer, so an absurd value is an allocation rather than a
		// wrong answer.
		throw new OffsetsRejected(
			'buffer-length-absurd',
			`${path}.bufferLength`,
			`${bufferLength} is outside ${MIN_BUFFER_LENGTH}..${MAX_BUFFER_LENGTH}`
		);
	}

	const struct = requirePresent(player, 'struct', `${path}.struct`);
	if (!Array.isArray(struct)) {
		throw new OffsetsRejected('wrong-type', `${path}.struct`, `expected an array, got ${describe(struct)}`);
	}
	let described = 0;
	for (const [index, field] of struct.entries()) {
		const at = `${path}.struct[${index}]`;
		const entry = requireRecord(field, at);
		const type = requirePresent(entry, 'type', `${at}.type`);
		if (typeof type !== 'string' || !STRUCT_TYPES.has(type)) {
			throw new OffsetsRejected('bad-struct', `${at}.type`, `${JSON.stringify(type)} is not a known field type`);
		}
		if (typeof requirePresent(entry, 'name', `${at}.name`) !== 'string') {
			throw new OffsetsRejected('wrong-type', `${at}.name`, 'expected a string');
		}
		if (type === 'SKIP') {
			const skip = requireInteger(entry, 'skip', `${at}.skip`);
			if (skip < 0 || skip > MAX_BUFFER_LENGTH) {
				throw new OffsetsRejected('bad-struct', `${at}.skip`, `${skip} is outside 0..${MAX_BUFFER_LENGTH}`);
			}
			described += skip;
		} else {
			described += 1;
		}
	}
	// The struct is parsed out of a buffer of `bufferLength` bytes. A description longer
	// than the buffer is either a mistake or an attempt at an over-read, and it costs one
	// comparison to say so here rather than discover it in structron.
	if (described > bufferLength) {
		throw new OffsetsRejected(
			'bad-struct',
			`${path}.struct`,
			`describes at least ${described} bytes of a ${bufferLength} byte buffer`
		);
	}
}

/**
 * Validates one offsets file.
 *
 * Throws `OffsetsRejected` with a distinct `code` per rule; returns the value typed on
 * success. It never mutates what it is given.
 */
export function validateOffsets(value: unknown, where = 'offsets'): IOffsets {
	const offsets = requireRecord(value, where);

	for (const key of REQUIRED_TOP_LEVEL) {
		requirePresent(offsets, key, `${where}.${key}`);
	}

	// Everything that is a chain in the type, checked as one.
	for (const key of REQUIRED_TOP_LEVEL) {
		const field = offsets[key];
		if (Array.isArray(field)) {
			requireChain(offsets, key, `${where}.${key}`);
		}
	}

	for (const key of ['playerAddrPtr', 'door_doorId', 'door_isOpen'] as const) {
		const at = `${where}.${key}`;
		const offset = requireInteger(offsets, key, at);
		if (offset < MIN_CHAIN_VALUE || offset > MAX_MODULE_RVA) {
			throw new OffsetsRejected('chain-out-of-range', at, `${offset} is outside the module`);
		}
	}

	for (const key of RVA_FIELDS) {
		const at = `${where}.${key}`;
		const rva = requireInteger(offsets, key, at);
		if (rva < 0 || rva > MAX_MODULE_RVA) {
			throw new OffsetsRejected('rva-out-of-module', at, `${rva} is outside 0..0x${MAX_MODULE_RVA.toString(16)}`);
		}
	}

	for (const key of ['oldMeetingHud', 'disableWriting', 'newGameOptions'] as const) {
		requireBoolean(offsets, key, `${where}.${key}`);
	}

	const innerNetClient = requireRecord(offsets.innerNetClient, `${where}.innerNetClient`);
	requireChain(innerNetClient, 'base', `${where}.innerNetClient.base`);
	for (const key of INNER_NET_CLIENT_NUMBERS) {
		const at = `${where}.innerNetClient.${key}`;
		const offset = requireInteger(innerNetClient, key, at);
		if (offset < MIN_CHAIN_VALUE || offset > MAX_MODULE_RVA) {
			throw new OffsetsRejected('chain-out-of-range', at, `${offset} is outside the module`);
		}
	}

	validatePlayer(offsets.player, `${where}.player`);

	const signatures = requireRecord(offsets.signatures, `${where}.signatures`);
	for (const name of SIGNATURE_NAMES) {
		validateSignature(requirePresent(signatures, name, `${where}.signatures.${name}`), `${where}.signatures.${name}`);
	}

	return value as IOffsets;
}

/** A relative path inside the offsets tree, ending in .json. Nothing else is fetched. */
const LOOKUP_FILE = /^[A-Za-z0-9][A-Za-z0-9._-]*(?:\/[A-Za-z0-9][A-Za-z0-9._-]*)*\.json$/;

/**
 * Compares two dotted numeric versions. Returns <0, 0 or >0.
 *
 * Deliberately not semver: it must answer "is the running client older than this bundle
 * asks for", and a pre-release suffix on our own version should not make a client look
 * older than it is. Anything after the numbers is ignored.
 */
export function compareVersions(left: string, right: string): number {
	const parse = (text: string) =>
		text
			.split('.')
			.map((part) => Number.parseInt(part, 10))
			.map((part) => (Number.isFinite(part) ? part : 0));
	const a = parse(left);
	const b = parse(right);
	for (let index = 0; index < Math.max(a.length, b.length); index++) {
		const difference = (a[index] ?? 0) - (b[index] ?? 0);
		if (difference !== 0) return difference;
	}
	return 0;
}

/** What the caller knows that the bundle itself cannot: who is reading it, and what came before. */
export interface BundleContext {
	/** The running client, for `min_client_version`. */
	clientVersion: string;
	/** The `bundle_version` already held, if any. A lower one arriving is a rollback. */
	heldBundleVersion?: number;
}

/**
 * Validates the lookup that maps a game build to an offsets file.
 *
 * `file` is interpolated into a URL, so it is checked against a conservative shape: a
 * traversal or an absolute URL here would redirect the offsets fetch to a host of the
 * attacker's choosing, which is a more direct route than any wrong number.
 *
 * The envelope fields are optional. A mirror that has not published them yet is not an
 * outage — this project's own mirror gained them on 2026-08-24 and the clients in the
 * field predate that. What is *present* is enforced.
 */
export function validateLookup(value: unknown, where = 'lookup', context?: BundleContext): IOffsetsLookup {
	const lookup = requireRecord(value, where);

	if (Object.hasOwn(lookup, 'bundle_version')) {
		const bundleVersion = requireInteger(lookup, 'bundle_version', `${where}.bundle_version`);
		if (bundleVersion < 0) {
			throw new OffsetsRejected('wrong-type', `${where}.bundle_version`, `${bundleVersion} is negative`);
		}
		// A replayed older bundle is how an attacker who once had a bad file on the mirror
		// gets it back after it is reverted. Refusing it leaves the held bundle in force.
		if (context?.heldBundleVersion !== undefined && bundleVersion < context.heldBundleVersion) {
			throw new OffsetsRejected(
				'bundle-version-replayed',
				`${where}.bundle_version`,
				`${bundleVersion} is older than the ${context.heldBundleVersion} already held`
			);
		}
	}

	if (Object.hasOwn(lookup, 'min_client_version')) {
		const minimum = requirePresent(lookup, 'min_client_version', `${where}.min_client_version`);
		if (typeof minimum !== 'string' || !/^\d+(?:\.\d+)*/.test(minimum)) {
			throw new OffsetsRejected('wrong-type', `${where}.min_client_version`, 'expected a dotted version string');
		}
		if (context && compareVersions(context.clientVersion, minimum) < 0) {
			// The bundle describes a game this client cannot read correctly. Saying so is
			// better than reading the wrong fields and reporting nothing.
			throw new OffsetsRejected(
				'client-too-old',
				`${where}.min_client_version`,
				`bundle needs ${minimum}, this client is ${context.clientVersion}`
			);
		}
	}

	const patterns = requireRecord(requirePresent(lookup, 'patterns', `${where}.patterns`), `${where}.patterns`);
	for (const arch of ['x64', 'x86'] as const) {
		const forArch = requireRecord(
			requirePresent(patterns, arch, `${where}.patterns.${arch}`),
			`${where}.patterns.${arch}`
		);
		validateSignature(
			requirePresent(forArch, 'broadcastVersion', `${where}.patterns.${arch}.broadcastVersion`),
			`${where}.patterns.${arch}.broadcastVersion`
		);
	}

	const versions = requireRecord(requirePresent(lookup, 'versions', `${where}.versions`), `${where}.versions`);
	if (!Object.hasOwn(versions, 'default')) {
		// Every unknown build falls back to `default`. Without it an unrecognised game
		// build has nowhere to go, which is the outage this validator exists to prevent
		// rather than cause.
		throw new OffsetsRejected('missing-field', `${where}.versions.default`, 'the fallback entry is absent');
	}
	for (const [id, entry] of Object.entries(versions)) {
		const at = `${where}.versions.${id}`;
		const record = requireRecord(entry, at);
		if (typeof requirePresent(record, 'version', `${at}.version`) !== 'string') {
			throw new OffsetsRejected('wrong-type', `${at}.version`, 'expected a string');
		}
		const file = requirePresent(record, 'file', `${at}.file`);
		if (typeof file !== 'string' || !LOOKUP_FILE.test(file)) {
			throw new OffsetsRejected('wrong-type', `${at}.file`, `${JSON.stringify(file)} is not a relative .json path`);
		}
		const version = requireInteger(record, 'offsetsVersion', `${at}.offsetsVersion`);
		if (version < 0) {
			throw new OffsetsRejected('wrong-type', `${at}.offsetsVersion`, `${version} is negative`);
		}
	}

	return value as IOffsetsLookup;
}

/**
 * The range check that actually protects the write path.
 *
 * `GameReader` overwrites `connectFunc`, `fixedUpdateFunc`, `showModStampFunc` and
 * `modLateUpdateFunc` with the result of a pattern scan before using them, so the number
 * validated in the bundle is a placeholder — the real values in the corpus are 255 to
 * 4095, far too small to be functions. What the bundle actually controls is the
 * *signature*, and a hostile signature steers the scan to whatever address its bytes
 * happen to match. Bounding the resolved address is therefore the check that matters,
 * and it belongs where the address is produced.
 */
export function isAddressInModule(resolved: number, moduleBase: number, moduleSize: number): boolean {
	if (!Number.isInteger(resolved) || !Number.isInteger(moduleBase)) {
		return false;
	}
	if (resolved <= moduleBase) {
		return false;
	}
	const size = Number.isInteger(moduleSize) && moduleSize > 0 ? moduleSize : MAX_MODULE_RVA;
	return resolved < moduleBase + size;
}
