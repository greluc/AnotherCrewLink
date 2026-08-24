import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// electron-store reaches for app.getPath at construction, which only exists inside
// Electron, so the two stores this module creates at import time are stubbed out.
const stores: Record<string, Record<string, unknown>> = {};
vi.mock('electron-store', () => ({
	default: class {
		private name: string;
		constructor(options: { name: string }) {
			this.name = options.name;
			stores[this.name] ??= {};
		}
		get(key: string) {
			return stores[this.name][key];
		}
		set(key: string | Record<string, unknown>, value?: unknown) {
			// The real store serialises to JSON on disk, so nothing it holds is a live
			// reference to what the caller passed. Modelling that matters here: without
			// it, tampering with the cache in one test mutates the shared fixture.
			if (typeof key === 'object') Object.assign(stores[this.name], structuredClone(key));
			else stores[this.name][key] = structuredClone(value);
		}
		clear() {
			stores[this.name] = {};
		}
		get store() {
			return stores[this.name];
		}
	},
}));

vi.mock('electron', () => ({ app: { getVersion: () => '1.0.3' } }));

const fixtures = join(dirname(fileURLToPath(import.meta.url)), '../../test/fixtures/offsets');
const REAL_LOOKUP = JSON.parse(readFileSync(join(fixtures, 'lookup.json'), 'utf8'));
const REAL_OFFSETS = JSON.parse(readFileSync(join(fixtures, 'offsets__x86__V2026.8.18__offsets.json'), 'utf8'));

const { fetchOffsetLookup, fetchOffsets, getOffsetsStatus, resetOffsetsToEmbedded } = await import('./offsetStore');
const { EMBEDDED_GAME_VERSION, EMBEDDED_OFFSETS_FILE } = await import('./embeddedOffsets');

let attempts: string[];

beforeEach(() => {
	attempts = [];
	for (const key of Object.keys(stores)) stores[key] = {};
	vi.useFakeTimers();
});

afterEach(() => {
	vi.useRealTimers();
	vi.unstubAllGlobals();
});

/** Replaces fetch with one that fails a given number of times before answering. */
function fetchFailingTimes(failures: number, body: unknown = REAL_LOOKUP, status = 429) {
	vi.stubGlobal('fetch', (url: string) => {
		attempts.push(url);
		if (attempts.length <= failures) {
			return Promise.resolve({ ok: false, status, json: () => Promise.resolve({}) } as Response);
		}
		// A fresh copy per call, as a real response body would be.
		return Promise.resolve({
			ok: true,
			status: 200,
			json: () => Promise.resolve(structuredClone(body)),
		} as unknown as Response);
	});
}

function fetchAlwaysFailing() {
	vi.stubGlobal('fetch', (url: string) => {
		attempts.push(url);
		return Promise.reject(new Error('offline'));
	});
}

describe('fetchOffsetLookup', () => {
	it('takes the first answer', async () => {
		fetchFailingTimes(0);
		await expect(fetchOffsetLookup()).resolves.toEqual(REAL_LOOKUP);
		expect(attempts).toHaveLength(1);
		expect(getOffsetsStatus().lookup).toBe('mirror');
	});

	it('falls back to the second host when the first one refuses', async () => {
		fetchFailingTimes(1);
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		await expect(result).resolves.toEqual(REAL_LOOKUP);
		expect(attempts[0]).toContain('raw.githubusercontent.com');
		expect(attempts[1]).toContain('cdn.jsdelivr.net');
	});

	it('keeps trying after both hosts refuse, which is what a rate limit looks like', async () => {
		// Both hosts turned this install away once. Before, that was the end of it and
		// the user was told to check their internet connection.
		fetchFailingTimes(2);
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		await expect(result).resolves.toEqual(REAL_LOOKUP);
		expect(attempts.length).toBeGreaterThan(2);
	});

	it('does not parse an error page as if it were data', async () => {
		vi.stubGlobal('fetch', () =>
			Promise.resolve({
				ok: false,
				status: 404,
				json: () => Promise.resolve({ nonsense: true }),
			} as unknown as Response)
		);
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		// It no longer gives up: it drops to the floor, and says why.
		await expect(result).resolves.toHaveProperty('versions.default');
		expect(getOffsetsStatus().lookup).toBe('embedded');
		expect(getOffsetsStatus().reason).toMatch(/internet connection|HTTP 404/);
	});

	it('refuses a lookup that does not validate and uses the floor instead', async () => {
		// A hostile or broken mirror answering with a 200 is the case the validator exists
		// for, and it must not reach the cache.
		fetchFailingTimes(0, {
			patterns: {},
			versions: { default: { version: 'x', file: '../../etc/x.json', offsetsVersion: 1 } },
		});
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		await expect(result).resolves.toHaveProperty('versions.default');
		expect(getOffsetsStatus().lookup).toBe('embedded');
		expect(stores.lookup.versions).toBeUndefined();
	});

	it('serves the cached lookup rather than the floor, once one has been stored', async () => {
		fetchFailingTimes(0);
		await fetchOffsetLookup();

		attempts = [];
		fetchAlwaysFailing();
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		await expect(result).resolves.toEqual(REAL_LOOKUP);
		expect(getOffsetsStatus().lookup).toBe('cache');
	});

	it('discards a cache that has been edited on disk between runs', async () => {
		// Gate G0 asks for validation at load and not only at download. This is that
		// criterion: the cache is written legitimately, then tampered with, then read.
		fetchFailingTimes(0);
		await fetchOffsetLookup();

		(stores.lookup.versions as Record<string, { file: string }>).default.file = 'https://evil.example/x.json';

		attempts = [];
		fetchAlwaysFailing();
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		await expect(result).resolves.toHaveProperty('versions.default');
		expect(getOffsetsStatus().lookup).toBe('embedded');
		// Discarded rather than carried forward on every subsequent start.
		expect(stores.lookup.versions).toBeUndefined();
	});

	it('refuses a bundle version older than the one already held', async () => {
		fetchFailingTimes(0, { ...REAL_LOOKUP, bundle_version: 9 });
		await fetchOffsetLookup();
		expect(getOffsetsStatus().bundleVersion).toBe(9);

		attempts = [];
		// The mirror was reverted, or someone replayed an old file at it.
		fetchFailingTimes(0, { ...REAL_LOOKUP, bundle_version: 4 });
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		// The held bundle stays in force.
		await expect(result).resolves.toHaveProperty('bundle_version', 9);
		expect(getOffsetsStatus().lookup).toBe('cache');
	});

	it('refuses a bundle that asks for a newer client than this one', async () => {
		fetchFailingTimes(0, { ...REAL_LOOKUP, min_client_version: '9.9.9' });
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		await expect(result).resolves.toHaveProperty('versions.default');
		expect(getOffsetsStatus().lookup).toBe('embedded');
		expect(getOffsetsStatus().reason).toContain('client-too-old');
	});

	it('accepts a bundle whose minimum this client meets', async () => {
		fetchFailingTimes(0, { ...REAL_LOOKUP, min_client_version: '1.0.0', bundle_version: 2 });
		await expect(fetchOffsetLookup()).resolves.toHaveProperty('bundle_version', 2);
		expect(getOffsetsStatus().lookup).toBe('mirror');
	});
});

describe('fetchOffsets', () => {
	it('validates what the mirror sends before storing it', async () => {
		fetchFailingTimes(0, REAL_OFFSETS);
		await expect(fetchOffsets(false, 'V2026.8.18/offsets.json', 5)).resolves.toEqual(REAL_OFFSETS);
		expect(getOffsetsStatus().offsets).toBe('mirror');
	});

	it('refuses offsets that do not validate, and does not cache them', async () => {
		fetchFailingTimes(0, { ...REAL_OFFSETS, fixedUpdateFunc: 0x7fffffff });
		// The assertion is attached before the timers run: the rejection happens while they
		// are being advanced, and an unhandled one fails the whole suite even though every
		// test passes.
		const assertion = expect(fetchOffsets(false, 'nowhere/offsets.json', 1)).rejects.toBeTruthy();
		await vi.runAllTimersAsync();
		await assertion;
		expect(stores.offsets.IOffsets).toBeUndefined();
	});

	it('discards cached offsets that have been edited on disk', async () => {
		fetchFailingTimes(0, REAL_OFFSETS);
		await fetchOffsets(false, 'V2026.8.18/offsets.json', 5);

		(stores.offsets.IOffsets as { player: { bufferLength: number } }).player.bufferLength = 0x40000000;

		attempts = [];
		fetchFailingTimes(0, REAL_OFFSETS);
		const result = fetchOffsets(false, 'V2026.8.18/offsets.json', 5);
		await vi.runAllTimersAsync();
		// Refetched rather than used, and the fetch is what proves the cache was rejected.
		await expect(result).resolves.toEqual(REAL_OFFSETS);
		expect(attempts.length).toBeGreaterThan(0);
	});

	it('falls to the embedded bundle when the mirror is unreachable', async () => {
		fetchAlwaysFailing();
		const result = fetchOffsets(false, EMBEDDED_OFFSETS_FILE, 1);
		await vi.runAllTimersAsync();
		await expect(result).resolves.toHaveProperty('player');
		const reported = getOffsetsStatus();
		expect(reported.offsets).toBe('embedded');
		// Says which bundle it is using rather than falling back silently.
		expect(reported.gameVersion).toBe(EMBEDDED_GAME_VERSION);
		expect(reported.reason).toBeTruthy();
	});

	it('refuses rather than serving the floor for a build it does not describe', async () => {
		// The floor carries the current build only. Handing a player on a two-year-old
		// Among Us the current offsets would read the wrong fields and report nothing.
		fetchAlwaysFailing();
		const assertion = expect(fetchOffsets(false, 'V2021.3.31/offsets.json', 1)).rejects.toBeTruthy();
		await vi.runAllTimersAsync();
		await assertion;
	});
});

describe('the embedded floor itself', () => {
	it('validates, so a build cannot ship a malformed one', async () => {
		fetchAlwaysFailing();
		const lookup = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		await expect(lookup).resolves.toHaveProperty('versions.default');

		for (const is64 of [true, false]) {
			attempts = [];
			const offsets = fetchOffsets(is64, EMBEDDED_OFFSETS_FILE, 1);
			await vi.runAllTimersAsync();
			await expect(offsets).resolves.toHaveProperty('signatures');
		}
	});
});

describe('resetOffsetsToEmbedded', () => {
	it('drops both caches and reports the floor', async () => {
		fetchFailingTimes(0);
		await fetchOffsetLookup();
		expect(stores.lookup.versions).toBeDefined();

		const reported = resetOffsetsToEmbedded();
		expect(stores.lookup.versions).toBeUndefined();
		expect(stores.offsets.IOffsets).toBeUndefined();
		expect(reported.lookup).toBe('embedded');
		expect(reported.offsets).toBe('embedded');
		expect(reported.gameVersion).toBe(EMBEDDED_GAME_VERSION);
	});

	it('lets the next start reach the mirror again', async () => {
		fetchFailingTimes(0);
		await fetchOffsetLookup();
		resetOffsetsToEmbedded();

		attempts = [];
		fetchFailingTimes(0);
		await expect(fetchOffsetLookup()).resolves.toEqual(REAL_LOOKUP);
		expect(getOffsetsStatus().lookup).toBe('mirror');
	});
});
