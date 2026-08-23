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
			if (typeof key === 'object') Object.assign(stores[this.name], key);
			else stores[this.name][key] = value;
		}
		get store() {
			return stores[this.name];
		}
	},
}));

const LOOKUP = { patterns: [{ signature: 'x', versions: ['2024.1.1'] }] };

const { fetchOffsetLookup } = await import('./offsetStore');

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
function fetchFailingTimes(failures: number, status = 429) {
	vi.stubGlobal('fetch', (url: string) => {
		attempts.push(url);
		if (attempts.length <= failures) {
			return Promise.resolve({ ok: false, status, json: () => Promise.resolve({}) } as Response);
		}
		return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(LOOKUP) } as unknown as Response);
	});
}

describe('fetchOffsetLookup', () => {
	it('takes the first answer', async () => {
		fetchFailingTimes(0);
		await expect(fetchOffsetLookup()).resolves.toEqual(LOOKUP);
		expect(attempts).toHaveLength(1);
	});

	it('falls back to the second host when the first one refuses', async () => {
		fetchFailingTimes(1);
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		await expect(result).resolves.toEqual(LOOKUP);
		expect(attempts[0]).toContain('raw.githubusercontent.com');
		expect(attempts[1]).toContain('cdn.jsdelivr.net');
	});

	it('keeps trying after both hosts refuse, which is what a rate limit looks like', async () => {
		// Both hosts turned this install away once. Before, that was the end of it and
		// the user was told to check their internet connection.
		fetchFailingTimes(2);
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		await expect(result).resolves.toEqual(LOOKUP);
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
		// The assertion is attached before the timers run: the rejection happens while
		// they are being advanced, and an unhandled one would be reported as an error.
		const assertion = expect(fetchOffsetLookup()).rejects.toMatch(/internet connection/);
		await vi.runAllTimersAsync();
		await assertion;
	});

	it('serves the cached lookup rather than failing, once one has been stored', async () => {
		fetchFailingTimes(0);
		await fetchOffsetLookup();

		vi.stubGlobal('fetch', () => Promise.reject(new Error('offline')));
		const result = fetchOffsetLookup();
		await vi.runAllTimersAsync();
		await expect(result).resolves.toEqual(LOOKUP);
	});
});
