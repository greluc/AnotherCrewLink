import { app } from 'electron';
import Store from 'electron-store';
import Errors from '../common/Errors';
import { EMBEDDED_GAME_VERSION, EMBEDDED_LOOKUP, EMBEDDED_OFFSETS, EMBEDDED_OFFSETS_FILE } from './embeddedOffsets';
import { OffsetsRejected, validateLookup, validateOffsets } from './offsetsValidator';

export interface IOffsetsLookup {
	patterns: {
		x64: { broadcastVersion: ISignature };
		x86: { broadcastVersion: ISignature };
	};
	versions: {
		[innerNetClientId: string]: {
			version: string;
			file: string;
			offsetsVersion: number;
		};
	};
}

interface ISignature {
	sig: string;
	addressOffset: number;
	patternOffset: number;
}

export interface IOffsets {
	meetingHud: number[];
	objectCachePtr: number[];
	meetingHudState: number[];
	allPlayersPtr: number[];
	allPlayers: number[];
	playerCount: number[];
	playerAddrPtr: number;
	shipStatus: number[];
	lightRadius: number[];
	shipStatus_systems: number[];
	shipStatus_map: number[];
	shipstatus_allDoors: number[];
	door_doorId: number;
	door_isOpen: number;
	deconDoorUpperOpen: number[];
	deconDoorLowerOpen: number[];
	hqHudSystemType_CompletedConsoles: number[];
	HudOverrideSystemType_isActive: number[];
	miniGame: number[];
	planetSurveillanceMinigame_currentCamera: number[];
	planetSurveillanceMinigame_camarasCount: number[];
	surveillanceMinigame_FilteredRoomsCount: number[];
	palette: number[];
	palette_shadowColor: number[];
	palette_playercolor: number[];
	gameoptionsData: number[];
	gameOptions_MapId: number[];
	gameOptions_MaxPLayers: number[];
	serverManager_currentServer: number[];
	innerNetClient: {
		base: number[];
		networkAddress: number;
		networkPort: number;
		onlineScene: number;
		mainMenuScene: number;
		gameMode: number;
		gameId: number;
		hostId: number;
		clientId: number;
		gameState: number;
	};
	player: {
		isLocal: number[];
		localX: number[];
		localY: number[];
		remoteX: number[];
		remoteY: number[];
		roleTeam: number[];
		nameText?: number[];
		currentOutfit: number[];
		outfit: {
			colorId: number[];
			playerName: number[];
			hatId: number[];
			skinId: number[];
			visorId: number[];
		};
		bufferLength: number;
		offsets: number[];
		inVent: number[];
		clientId: number[];
		isDummy: number[]; // used for muting
		struct: {
			type:
				| 'INT'
				| 'INT_BE'
				| 'UINT'
				| 'UINT_BE'
				| 'SHORT'
				| 'SHORT_BE'
				| 'USHORT'
				| 'USHORT_BE'
				| 'FLOAT'
				| 'CHAR'
				| 'BYTE'
				| 'SKIP';
			skip?: number;
			name: string;
		}[];
	};
	signatures: {
		innerNetClient: ISignature;
		meetingHud: ISignature;
		gameData: ISignature;
		shipStatus: ISignature;
		miniGame: ISignature;
		palette: ISignature;
		playerControl: ISignature;
		serverManager: ISignature;
		gameOptionsManager: ISignature;
	};
	oldMeetingHud: boolean;
	newGameOptions: boolean;
}

interface IOffsetsStore {
	filename: string;
	is_64bit: boolean;
	offsetsVersion: number;
	IOffsets: IOffsets;
}

interface ILookupStore extends IOffsetsLookup {
	/** The `bundle_version` this cache was written from, for replay detection. */
	bundle_version?: number;
}

// The offsets decide where this client reads inside another process. They used to come
// from a branch of a repository this project does not control; they now come from our own
// fork, which is the only part of that chain we can review before it reaches users.
const BASE_URL = 'https://raw.githubusercontent.com/greluc/AnotherCrewlink-Offsets/main';
const BASE_URL_error = 'https://cdn.jsdelivr.net/gh/greluc/AnotherCrewlink-Offsets@main';

const store = new Store<IOffsetsStore>({ name: 'offsets' });
const lookupStore = new Store<ILookupStore>({ name: 'lookup' });

/** Where the bundle currently in use came from. */
export type OffsetsSource = 'mirror' | 'cache' | 'embedded';

export interface OffsetsStatus {
	lookup: OffsetsSource;
	offsets: OffsetsSource;
	/** The game build the offsets in use describe, when it is known. */
	gameVersion?: string;
	bundleVersion?: number;
	/**
	 * Why the mirror was not used, when it was not. A client quietly running on a
	 * two-year-old embedded bundle looks identical to one that is up to date, and the
	 * difference only shows up as "why can nobody hear me on the new map".
	 */
	reason?: string;
}

const status: OffsetsStatus = { lookup: 'embedded', offsets: 'embedded' };

export function getOffsetsStatus(): OffsetsStatus {
	return { ...status };
}

/** The running client, for a bundle's `min_client_version`. */
function clientVersion(): string {
	try {
		return app?.getVersion() ?? '0.0.0';
	} catch {
		// Reachable in tests and before `app` is ready. A version of 0.0.0 fails a minimum
		// check closed, which is the safe direction.
		return '0.0.0';
	}
}

function describeError(error: unknown): string {
	if (error instanceof OffsetsRejected) return error.message;
	if (error instanceof Error) return error.message;
	return String(error);
}

// One attempt per host and nothing else used to stand between a working install and
// an error message: raw.githubusercontent.com rate limits per IP, so a household where
// several people start the app at once, or anyone behind a shared address, could be
// turned away and told to check their internet connection. Both hosts are tried, then
// tried again after a pause, and a request that hangs is cut off rather than holding
// up the start.
const FETCH_TIMEOUT = 10000;
const FETCH_ROUNDS = 3;
const RETRY_DELAY = 1500;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function fetchJsonFromMirrors<T>(path: string, failure: string): Promise<T> {
	let lastError: unknown;
	for (let round = 0; round < FETCH_ROUNDS; round++) {
		for (const base of [BASE_URL, BASE_URL_error]) {
			try {
				const response = await fetch(`${base}/${path}`, { signal: AbortSignal.timeout(FETCH_TIMEOUT) });
				// Without this a rate limit page or a 404 body was parsed as if it were the
				// data, and whether that threw depended on what the host happened to return.
				if (!response.ok) {
					throw new Error(`${base}/${path} responded with HTTP ${response.status}`);
				}
				return (await response.json()) as T;
			} catch (error) {
				lastError = error;
				console.warn('Offset fetch failed:', describeError(error));
			}
		}
		if (round < FETCH_ROUNDS - 1) {
			await sleep(RETRY_DELAY * (round + 1));
		}
	}
	console.error('Giving up on', path, lastError);
	throw failure;
}

/**
 * The lookup, from the mirror if it can be reached and believed, otherwise from the
 * cache, otherwise from the bundle compiled into this build.
 *
 * Validation runs on **every** path, not only on the download. A cache lives in
 * `userData`, where anything running as this user can edit it, so validating only at
 * download time would check the one copy an attacker has least reason to touch.
 */
export async function fetchOffsetLookup(): Promise<IOffsetsLookup> {
	const context = { clientVersion: clientVersion(), heldBundleVersion: lookupStore.get('bundle_version') };

	try {
		const fetched = await fetchJsonFromMirrors<unknown>('lookup.json', Errors.LOOKUP_FETCH_ERROR);
		const lookup = validateLookup(fetched, 'lookup', context);
		lookupStore.set(lookup as ILookupStore);
		status.lookup = 'mirror';
		status.bundleVersion = (lookup as ILookupStore).bundle_version;
		status.reason = undefined;
		return lookup;
	} catch (error) {
		status.reason = describeError(error);
		console.warn('Falling back from the mirror:', status.reason);
	}

	if (lookupStore.get('patterns')) {
		try {
			// No replay check against itself: the cache *is* the held version.
			const cached = validateLookup(lookupStore.store, 'cached lookup', { clientVersion: context.clientVersion });
			status.lookup = 'cache';
			status.bundleVersion = (cached as ILookupStore).bundle_version;
			return cached;
		} catch (error) {
			// A cache that no longer validates is discarded rather than repaired. Keeping it
			// would mean carrying a rejected bundle forward on every start.
			console.error('The cached lookup was rejected and has been discarded:', describeError(error));
			lookupStore.clear();
			status.reason = describeError(error);
		}
	}

	status.lookup = 'embedded';
	status.bundleVersion = (EMBEDDED_LOOKUP as ILookupStore).bundle_version;
	console.warn(`Using the embedded offsets lookup for ${EMBEDDED_GAME_VERSION}: ${status.reason ?? 'no cache'}`);
	// Validated rather than trusted: a build that ships a malformed floor should fail its
	// own tests, and `offsetStore.test.ts` is where that happens.
	return validateLookup(EMBEDDED_LOOKUP, 'embedded lookup', { clientVersion: context.clientVersion });
}

async function fetchOffsetsJson(is_64bit: boolean, filename: string): Promise<unknown> {
	return fetchJsonFromMirrors<unknown>(`offsets/${is_64bit ? 'x64' : 'x86'}/${filename}`, Errors.OFFSETS_FETCH_ERROR);
}

/** The floor, when it describes the build being asked for. */
function embeddedOffsetsFor(is_64bit: boolean, filename: string): IOffsets | undefined {
	if (filename !== EMBEDDED_OFFSETS_FILE) {
		// The floor carries the current build only. A player on an older Among Us with an
		// unreachable mirror gets an honest error rather than offsets for a different game.
		return undefined;
	}
	return is_64bit ? EMBEDDED_OFFSETS.x64 : EMBEDDED_OFFSETS.x86;
}

export async function fetchOffsets(is_64bit: boolean, filename: string, offsetsVersion: number): Promise<IOffsets> {
	// offsetsVersion in case we need to update people's cached file
	// >= version to allow testing with local file updates (eg remote vers 2, local vers 3)
	// no need to host local http server
	if (
		store.get('filename') === filename &&
		store.get('is_64bit') === is_64bit &&
		store.get('offsetsVersion') >= offsetsVersion
	) {
		try {
			const cached = validateOffsets(store.get('IOffsets'), 'cached offsets');
			console.log('Loading cached offsets');
			status.offsets = 'cache';
			return cached;
		} catch (error) {
			// Editing the cached bundle on disk between runs lands here.
			console.error('The cached offsets were rejected and have been discarded:', describeError(error));
			store.clear();
			status.reason = describeError(error);
		}
	}

	try {
		const offsets = validateOffsets(await fetchOffsetsJson(is_64bit, filename), filename);
		store.set('filename', filename);
		store.set('is_64bit', is_64bit);
		store.set('offsetsVersion', offsetsVersion ? offsetsVersion : 0);
		store.set('IOffsets', offsets);
		status.offsets = 'mirror';
		status.gameVersion = undefined;
		return offsets;
	} catch (error) {
		status.reason = describeError(error);
		const floor = embeddedOffsetsFor(is_64bit, filename);
		if (!floor) {
			console.error('No offsets for', filename, '-', status.reason);
			throw Errors.OFFSETS_FETCH_ERROR;
		}
		console.warn(`Using the embedded offsets for ${EMBEDDED_GAME_VERSION}: ${status.reason}`);
		status.offsets = 'embedded';
		status.gameVersion = EMBEDDED_GAME_VERSION;
		return validateOffsets(floor, 'embedded offsets');
	}
}

/**
 * Drops both caches so the next start reads the floor and then the mirror again.
 *
 * This is the manual recovery path, and it is the reason a bad merge on the mirror needs
 * a human on each affected machine rather than a republish. Without a signature there is
 * no signed floor to supersede a bad bundle from, so "reset and refetch" is what a user
 * has instead — which is why it is a button rather than a support instruction to delete a
 * file out of `userData`.
 */
export function resetOffsetsToEmbedded(): OffsetsStatus {
	store.clear();
	lookupStore.clear();
	status.lookup = 'embedded';
	status.offsets = 'embedded';
	status.gameVersion = EMBEDDED_GAME_VERSION;
	status.bundleVersion = (EMBEDDED_LOOKUP as ILookupStore).bundle_version;
	status.reason = 'reset by the user';
	console.warn('Offsets reset to the embedded bundle for', EMBEDDED_GAME_VERSION);
	return getOffsetsStatus();
}
