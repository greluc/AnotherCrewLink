import { describe, expect, it } from 'vitest';
import { HAT_COLLECTION_COMMIT, HAT_COLLECTION_URL } from './hatCollection';

// The file's own comment states the constraint: "Moving the pin means moving both lines.
// The commit alone points at a tree the new repository does not have." Nothing checked it.

describe('hatCollection', () => {
	it('builds the URL from the pinned commit', () => {
		// Move one line and not the other and the URL points at a tree that does not
		// exist. Every hat then fails to load, and the failure is a missing image rather
		// than an error anybody sees in a log.
		expect(HAT_COLLECTION_URL).toContain(HAT_COLLECTION_COMMIT);
	});

	it('pins a commit rather than a branch', () => {
		// The whole point. jsDelivr serves whatever a branch holds at request time with no
		// integrity check, so a branch pin lets the artwork every user downloads change
		// without a release on this side.
		expect(HAT_COLLECTION_COMMIT).toMatch(/^[0-9a-f]{40}$/);
	});

	it('serves the artwork from this project’s own fork, over TLS', () => {
		// Until 2026-08-24 it came from an account this project does not control, and the
		// images are decoded by a client that also reads another process's memory.
		expect(HAT_COLLECTION_URL).toMatch(
			/^https:\/\/cdn\.jsdelivr\.net\/gh\/greluc\/AnotherCrewLink-Hats@[0-9a-f]{40}\/$/
		);
	});

	it('ends in a separator, because callers append a filename', () => {
		// Without it the first segment of every filename is glued to the commit.
		expect(HAT_COLLECTION_URL.endsWith('/')).toBe(true);
	});
});
