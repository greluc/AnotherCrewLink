import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { OffsetsRejected, isAddressInModule, validateLookup, validateOffsets } from './offsetsValidator';

// Gate G0 has two halves and they pull against each other: the validator must reject a
// corpus of bad bundles, and it must accept every real file unchanged. A validator that
// rejects real data is a self-inflicted outage, so the second half is tested first and
// against the whole corpus rather than a sample.

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

	it('rejects an RVA outside the module', () => {
		const error = rejection(mutate((o) => Object.assign(o, { fixedUpdateFunc: 0x7fffffff })));
		expect(error.code).toBe('rva-out-of-module');
		expect(error.path).toBe('offsets.fixedUpdateFunc');
	});

	it('rejects a negative RVA', () => {
		expect(rejection(mutate((o) => Object.assign(o, { modLateUpdateFunc: -4096 }))).code).toBe('rva-out-of-module');
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

	it('rejects disableWriting flipped to a non-boolean', () => {
		// Flipping the flag to `false` is legitimate data and cannot be distinguished
		// here; what is caught is a value that is not a boolean at all. The real defence
		// for the write path is the prologue check in GameReader.
		expect(rejection(mutate((o) => Object.assign(o, { disableWriting: 'no' }))).code).toBe('wrong-type');
	});

	it('rejects a signature that is not a byte pattern', () => {
		const error = rejection(
			mutate((o) =>
				Object.assign((o.signatures as Record<string, unknown>).fixedUpdateFunc as object, { sig: 'ZZ 90' })
			)
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
		const error = rejection(mutate((o) => Object.assign(o, { fixedUpdateFunc: 0x7fffffff })));
		expect(error.message).toContain('rva-out-of-module');
		expect(error.message).toContain('offsets.fixedUpdateFunc');
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

	it('rejects a negative offsetsVersion', () => {
		expect(
			lookupRejection((l) => {
				(l.versions as Record<string, { offsetsVersion: number }>).default.offsetsVersion = -1;
			}).code
		).toBe('wrong-type');
	});
});

describe('the resolved-address check', () => {
	const base = 0x10000000;
	const size = 0x06000000;

	it('accepts an address inside the module', () => {
		expect(isAddressInModule(base + 0x1000, base, size)).toBe(true);
	});

	it('refuses an address below the module', () => {
		expect(isAddressInModule(base - 1, base, size)).toBe(false);
	});

	it('refuses the module base itself, which is a header rather than code', () => {
		expect(isAddressInModule(base, base, size)).toBe(false);
	});

	it('refuses an address past the end of the module', () => {
		expect(isAddressInModule(base + size, base, size)).toBe(false);
	});

	it('refuses a scan that found nothing', () => {
		// findPattern returns 0 when a signature matches nothing, and 0 must never be
		// treated as an address to write to.
		expect(isAddressInModule(0, base, size)).toBe(false);
	});

	it('falls back to a bounded window when the module size is unknown', () => {
		expect(isAddressInModule(base + 0x1000, base, 0)).toBe(true);
		expect(isAddressInModule(base + 0x40000000, base, 0)).toBe(false);
	});
});
