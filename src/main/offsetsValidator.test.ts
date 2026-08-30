import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { OffsetsRejected, validateLookup, validateOffsets } from './offsetsValidator';

// Two halves that pull against each other: the validator must reject a corpus of bad
// bundles, and it must accept every real file unchanged. A validator that rejects real
// data is a self-inflicted outage, so the second half is tested first and against the
// whole corpus rather than a sample.

const fixtures = join(dirname(fileURLToPath(import.meta.url)), '../../test/fixtures/offsets');

function load(name: string): unknown {
	return JSON.parse(readFileSync(join(fixtures, name), 'utf8'));
}

const offsetFiles = readdirSync(fixtures).filter((name) => name.endsWith('__offsets.json'));
const realOffsets = load('lookup.json') && (load(offsetFiles[0]) as Record<string, unknown>);

/** A deep copy, so a mutation in one case cannot leak into the next. */
function mutate(change: (offsets: Record<string, unknown>) => void): unknown {
	const copy = JSON.parse(JSON.stringify(realOffsets));
	change(copy);
	return copy;
}

function rejection(value: unknown): OffsetsRejected {
	try {
		validateOffsets(value);
	} catch (error) {
		if (error instanceof OffsetsRejected) return error;
		throw error;
	}
	throw new Error('expected the bundle to be rejected, but it was accepted');
}

describe('the validator accepts real data', () => {
	it('has a corpus to check against', () => {
		// If this ever reads zero files the suite below passes vacuously, which is worse
		// than failing.
		expect(offsetFiles.length).toBeGreaterThanOrEqual(40);
	});

	it.each(offsetFiles)('accepts %s unchanged', (name) => {
		expect(() => validateOffsets(load(name), name)).not.toThrow();
	});

	it('accepts the real lookup unchanged', () => {
		expect(() => validateLookup(load('lookup.json'))).not.toThrow();
	});

	it('returns the same object it was given', () => {
		const input = load(offsetFiles[0]);
		expect(validateOffsets(input)).toBe(input);
	});
});

describe('the malicious bundle corpus', () => {
	it('rejects a truncated file as a missing field, not a type error', () => {
		// A response cut short mid-download still parses when the cut lands on a boundary.
		const { player: _player, ...truncated } = realOffsets as Record<string, unknown>;
		expect(rejection(truncated).code).toBe('missing-field');
	});

	it('rejects a structurally malformed file', () => {
		expect(rejection('not a bundle at all').code).toBe('not-an-object');
		expect(rejection(null).code).toBe('not-an-object');
		expect(rejection([]).code).toBe('not-an-object');
	});

	it('rejects an absurd bufferLength', () => {
		const error = rejection(mutate((o) => Object.assign(o.player as object, { bufferLength: 0x40000000 })));
		expect(error.code).toBe('buffer-length-absurd');
	});

	it('rejects a bufferLength of zero', () => {
		expect(rejection(mutate((o) => Object.assign(o.player as object, { bufferLength: 0 }))).code).toBe(
			'buffer-length-absurd'
		);
	});

	it('rejects a pointer chain step outside the module', () => {
		const error = rejection(mutate((o) => Object.assign(o, { allPlayersPtr: [0x10, 0x7ffffff0] })));
		expect(error.code).toBe('chain-out-of-range');
		expect(error.path).toBe('offsets.allPlayersPtr[1]');
	});

	it('rejects a chain long enough to be a walk of its own', () => {
		expect(rejection(mutate((o) => Object.assign(o, { allPlayersPtr: new Array(64).fill(8) }))).code).toBe(
			'chain-too-long'
		);
	});

	it('rejects a non-integer where an offset belongs', () => {
		expect(rejection(mutate((o) => Object.assign(o, { playerAddrPtr: 1.5 }))).code).toBe('wrong-type');
	});

	it('rejects oldMeetingHud flipped to a non-boolean', () => {
		// Flipping the flag itself is legitimate data and cannot be distinguished here;
		// what is caught is a value that is not a boolean at all.
		expect(rejection(mutate((o) => Object.assign(o, { oldMeetingHud: 'no' }))).code).toBe('wrong-type');
	});

	it('rejects a signature that is not a byte pattern', () => {
		const error = rejection(
			mutate((o) => Object.assign((o.signatures as Record<string, unknown>).playerControl as object, { sig: 'ZZ 90' }))
		);
		expect(error.code).toBe('bad-signature');
	});

	it('rejects a signature whose pattern offset is not a small step', () => {
		// patternOffset indexes into the matched bytes. A huge value turns "where the
		// pattern matched" into an arbitrary address, which is the whole point of
		// bounding it rather than merely typing it.
		const error = rejection(
			mutate((o) =>
				Object.assign((o.signatures as Record<string, unknown>).innerNetClient as object, { patternOffset: 0x7ffffff0 })
			)
		);
		expect(error.code).toBe('bad-signature');
	});

	it('rejects a signature whose address offset is not a small step', () => {
		const error = rejection(
			mutate((o) =>
				Object.assign((o.signatures as Record<string, unknown>).innerNetClient as object, { addressOffset: -0x10000 })
			)
		);
		expect(error.code).toBe('bad-signature');
	});

	it('rejects an unknown struct field type', () => {
		const error = rejection(
			mutate((o) => {
				const player = o.player as { struct: { type: string }[] };
				player.struct[0].type = 'EXEC';
			})
		);
		expect(error.code).toBe('bad-struct');
	});

	it('rejects a struct that describes more than its buffer holds', () => {
		const error = rejection(
			mutate((o) => {
				const player = o.player as { struct: unknown[]; bufferLength: number };
				player.struct = [{ type: 'SKIP', skip: 4096, name: 'over' }];
				player.bufferLength = 64;
			})
		);
		expect(error.code).toBe('bad-struct');
	});

	it('names the rule it broke in the message', () => {
		const error = rejection(mutate((o) => Object.assign(o, { playerAddrPtr: 0x7fffffff })));
		expect(error.message).toContain('chain-out-of-range');
		expect(error.message).toContain('offsets.playerAddrPtr');
	});
});

describe('the lookup', () => {
	function lookupRejection(change: (lookup: Record<string, unknown>) => void): OffsetsRejected {
		const copy = JSON.parse(JSON.stringify(load('lookup.json')));
		change(copy);
		try {
			validateLookup(copy);
		} catch (error) {
			if (error instanceof OffsetsRejected) return error;
			throw error;
		}
		throw new Error('expected the lookup to be rejected');
	}

	it('rejects a file path that climbs out of the offsets tree', () => {
		expect(
			lookupRejection((l) => {
				(l.versions as Record<string, { file: string }>).default.file = '../../../etc/passwd.json';
			}).code
		).toBe('wrong-type');
	});

	it('rejects a file path that is an absolute URL', () => {
		expect(
			lookupRejection((l) => {
				(l.versions as Record<string, { file: string }>).default.file = 'https://evil.example/offsets.json';
			}).code
		).toBe('wrong-type');
	});

	it('rejects a lookup with no default entry', () => {
		// Every unrecognised game build falls back to `default`; losing it is an outage
		// for exactly the players a new Among Us release just created.
		const error = lookupRejection((l) => {
			delete (l.versions as Record<string, unknown>).default;
		});
		expect(error.code).toBe('missing-field');
		expect(error.path).toBe('lookup.versions.default');
	});

	it('carries the envelope the mirror publishes', () => {
		// The real file gained bundle_version, min_client_version and upstream_commit on
		// 2026-08-24. This asserts the fixture is the current shape rather than a snapshot
		// of the old one, so the cases below are testing something that exists.
		const lookup = load('lookup.json') as Record<string, unknown>;
		expect(typeof lookup.bundle_version).toBe('number');
		expect(typeof lookup.min_client_version).toBe('string');
		expect(lookup.upstream_commit).toMatch(/^[0-9a-f]{40}$/);
	});

	it('accepts the real lookup for a client that meets its minimum', () => {
		expect(() => validateLookup(load('lookup.json'), 'lookup', { clientVersion: '1.0.3' })).not.toThrow();
	});

	it('refuses the real lookup for a client older than it asks for', () => {
		expect(() => validateLookup(load('lookup.json'), 'lookup', { clientVersion: '0.9.0' })).toThrowError(
			/client-too-old/
		);
	});

	it('refuses a replay of an older bundle than the one held', () => {
		expect(() =>
			validateLookup(load('lookup.json'), 'lookup', { clientVersion: '1.0.3', heldBundleVersion: 99 })
		).toThrowError(/bundle-version-replayed/);
	});

	it('accepts a bundle newer than the one held', () => {
		expect(() =>
			validateLookup(load('lookup.json'), 'lookup', { clientVersion: '1.0.3', heldBundleVersion: 0 })
		).not.toThrow();
	});

	it('treats a missing envelope as data rather than an error', () => {
		// Clients in the field predate these fields, and a mirror mid-rollout is not an
		// outage.
		const { bundle_version: _v, min_client_version: _m, ...without } = load('lookup.json') as Record<string, unknown>;
		expect(() => validateLookup(without, 'lookup', { clientVersion: '0.0.1', heldBundleVersion: 99 })).not.toThrow();
	});

	it('rejects a negative offsetsVersion', () => {
		expect(
			lookupRejection((l) => {
				(l.versions as Record<string, { offsetsVersion: number }>).default.offsetsVersion = -1;
			}).code
		).toBe('wrong-type');
	});
});
