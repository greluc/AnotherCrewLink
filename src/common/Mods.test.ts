import { describe, expect, it } from 'vitest';
import { modList } from './Mods';

// `GameReader.ts` picks a mod with
//
//     modList.find((o) => o.dllStartsWith && file.includes(o.dllStartsWith))
//
// so the first entry whose marker appears in the filename wins, and the order of this
// list is therefore load-bearing. Nothing else says so.

describe('modList', () => {
	it('does not let an earlier marker shadow a later one', () => {
		// `TownOfUsMira.dll` contains `TownOfUs` as well. Mira is listed first, so it wins
		// — but only because of the order. Swapping the two entries would report every
		// Mira lobby as Town of Us, in the lobby browser, to everybody, with nothing
		// anywhere looking wrong.
		const markers = modList.map((mod) => mod.dllStartsWith).filter((marker): marker is string => !!marker);

		for (const [index, marker] of markers.entries()) {
			for (const earlier of markers.slice(0, index)) {
				expect(
					marker.includes(earlier),
					`"${marker}" contains the earlier "${earlier}", so it can never be detected`
				).toBe(false);
			}
		}
	});

	it('finds each mod by its own plugin file', () => {
		const detect = (file: string) => modList.find((o) => o.dllStartsWith && file.includes(o.dllStartsWith))?.id;

		expect(detect('TownOfUsMira.dll')).toBe('TOWN_OF_US_MIRA');
		expect(detect('TownOfUs.dll')).toBe('TOWN_OF_US');
		expect(detect('TheOtherRoles.dll')).toBe('THE_OTHER_ROLES');
		expect(detect('LasMonjas.dll')).toBe('LAS_MONJAS');
		expect(detect('SomethingElse.dll')).toBeUndefined();
	});

	it('starts with the entry the reader falls back to', () => {
		// `GameReader.ts` returns `modList[0]` when there is no loader and when no plugin
		// matched. If that stopped being `NONE`, an unmodded game would report a mod.
		expect(modList[0].id).toBe('NONE');
		expect(modList[0].dllStartsWith).toBeUndefined();
	});
});
