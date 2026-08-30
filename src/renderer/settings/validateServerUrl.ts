/**
 * Whether a string is a voice server this client will talk to.
 *
 * Lifted out of `ServerURLInput.tsx` for the reason `sortLobbies.ts` was lifted out of
 * `LobbyBrowser.tsx`: the component needs a DOM and MUI, the tests here run under node,
 * and this is a pure function of one string. It is also the one field in the settings
 * that decides where every signal this client sends goes, which makes it worth being able
 * to check.
 */

/**
 * Four rules, and each one is there for a different reason.
 *
 * **It has to parse.** `URL` replaced `valid-url`, which was unmaintained; the parser was
 * already there and the only thing the dependency added was the scheme check below.
 *
 * **The scheme has to be `http:` or `https:`.** Not because those are the only two that
 * parse — `javascript:` and `data:` parse — but because they are the only two this client
 * knows how to connect to, and the others reach code that never expected them.
 *
 * `http:` is accepted, which means signalling can run in cleartext. That is a known
 * weakness rather than something defended here; a local server on a LAN is the case it
 * exists for, and removing it is a decision with users behind it.
 *
 * **`discord.gg` is refused by name.** People paste invite links into text fields, and a
 * server URL field that accepted one would silently point every signal at Discord and
 * report nothing wrong.
 *
 * **The path has to be empty.** The client appends its own paths. A URL with one already
 * produces requests to somewhere nobody meant, and the failure looks like a server that
 * is down.
 */
export function validateServerUrl(uri: string): boolean {
	try {
		const url = new URL(uri);
		if (url.protocol !== 'http:' && url.protocol !== 'https:') return false;
		if (url.hostname === 'discord.gg') return false;
		if (url.pathname !== '/') return false;
		return true;
	} catch {
		return false;
	}
}
