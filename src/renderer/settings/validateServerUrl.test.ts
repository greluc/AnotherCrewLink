import { describe, expect, it } from 'vitest';
import { validateServerUrl } from './validateServerUrl';

// This decides where every signal this client sends goes, and the user can type anything
// into it, so the interesting cases are the ones that are nearly right.

describe('validateServerUrl', () => {
	it('accepts the server this client ships with', () => {
		expect(validateServerUrl('https://aucl.greluc.me')).toBe(true);
	});

	it('accepts a trailing slash, because the parser produces one anyway', () => {
		expect(validateServerUrl('https://aucl.greluc.me/')).toBe(true);
	});

	it('accepts a port', () => {
		expect(validateServerUrl('https://192.168.1.10:9736')).toBe(true);
	});

	it('accepts cleartext, which is a decision and not an oversight', () => {
		// A local server on a LAN is the case it exists for. A known weakness rather than
		// something defended here.
		expect(validateServerUrl('http://192.168.1.10:9736')).toBe(true);
	});

	it('refuses a scheme this client cannot connect to', () => {
		// These all parse. `URL` is not the check — the scheme is.
		for (const uri of ['ws://example.com', 'ftp://example.com', 'file:///etc/passwd']) {
			expect(validateServerUrl(uri)).toBe(false);
		}
	});

	it('refuses the schemes that would be worse than useless', () => {
		expect(validateServerUrl('javascript:alert(1)')).toBe(false);
		expect(validateServerUrl('data:text/html,<b>x</b>')).toBe(false);
	});

	it('refuses a Discord invite', () => {
		// People paste them into text fields. A server URL field that accepted one would
		// point every signal at Discord and report nothing wrong.
		expect(validateServerUrl('https://discord.gg/abcdef')).toBe(false);
		expect(validateServerUrl('https://discord.gg')).toBe(false);
	});

	it('refuses a Discord invite whatever case it was pasted in', () => {
		// `URL` lowercases the hostname, so the comparison holds. Asserted rather than
		// assumed, because it is the whole reason a plain string compare is enough.
		expect(validateServerUrl('https://DISCORD.GG/abcdef')).toBe(false);
	});

	it('refuses a URL that already has a path', () => {
		// The client appends its own. One already there produces requests to somewhere
		// nobody meant, and it looks like a server that is down.
		expect(validateServerUrl('https://example.com/voice')).toBe(false);
		expect(validateServerUrl('https://example.com/a/b')).toBe(false);
	});

	it('accepts a query or a fragment, which the client ignores', () => {
		// Neither changes where the requests go, and refusing them would reject a URL
		// somebody copied out of a browser bar for no benefit.
		expect(validateServerUrl('https://example.com?x=1')).toBe(true);
		expect(validateServerUrl('https://example.com#top')).toBe(true);
	});

	it('refuses what is not a URL at all', () => {
		for (const uri of ['', 'aucl.greluc.me', 'not a url', '://x', 'https://']) {
			expect(validateServerUrl(uri)).toBe(false);
		}
	});
});
