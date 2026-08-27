import { describe, expect, it } from 'vitest';
import { PROTOCOL_RETIRED, retirementMessage } from './protocolRetirement';

// The sentinel doubles as a sentence, and that is the ordering safeguard rather than a
// stylistic choice: a client that predates this file shows `error.message` verbatim, so the
// raw value has to be something a user can act on.
describe('retirementMessage', () => {
	const translate = (key: string) => (key === 'game.error_retired' ? 'Bitte aktualisieren.' : key);

	it('translates the retirement sentinel', () => {
		expect(retirementMessage(PROTOCOL_RETIRED, translate)).toBe('Bitte aktualisieren.');
	});

	it('leaves every other server error alone', () => {
		// The server sends real errors too. Replacing one with a translated guess would hide
		// the thing the user actually needs to read.
		for (const message of ['Lobby is full', 'rate limited', '', 'PROTOCOL something else']) {
			expect(retirementMessage(message, translate)).toBe(message);
		}
	});

	it('falls back to the sentence when the key is missing', () => {
		// i18next returns the key when nothing has it, and `game.error_retired` on screen is
		// worse than the English sentence the server sent.
		const untranslated = (key: string) => key;
		expect(retirementMessage(PROTOCOL_RETIRED, untranslated)).toBe(PROTOCOL_RETIRED);
	});

	it('is readable on a client that has never heard of it', () => {
		// The failure this guards: shipping the switch-off before the release that carries
		// this file. Those clients show the sentinel as it stands.
		expect(PROTOCOL_RETIRED).toContain('no longer supported');
		expect(PROTOCOL_RETIRED).toContain('update');
	});
});
