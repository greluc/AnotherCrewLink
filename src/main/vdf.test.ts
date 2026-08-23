import { describe, expect, it } from 'vitest';
import { parseVdf } from './vdf';

interface SteamApp {
	installed?: string;
	Name?: string;
}
interface SteamRegistry {
	Registry: { HKCU: { Software: { Valve: { Steam: { Apps: Record<string, SteamApp>; language?: string } } } } };
}

// The shape Steam actually writes, trimmed to what platform detection reads.
const STEAM_REGISTRY = `"Registry"
{
	"HKCU"
	{
		"Software"
		{
			"Valve"
			{
				"Steam"
				{
					"Apps"
					{
						"945360"
						{
							"installed"		"1"
							"Name"		"Among Us"
						}
						"570"
						{
							"installed"		"0"
						}
					}
					"language"		"german"
				}
			}
		}
	}
}
`;

describe('parseVdf', () => {
	it("reads a nested value out of Steam's registry format", () => {
		const parsed = parseVdf(STEAM_REGISTRY);
		// The parser returns a recursive union, so navigating it in a test needs a cast.
		const apps = (parsed as unknown as SteamRegistry).Registry.HKCU.Software.Valve.Steam.Apps;
		expect(apps['945360'].installed).toBe('1');
		expect(apps['945360'].Name).toBe('Among Us');
	});

	it('keeps sibling app ids apart', () => {
		const parsed = parseVdf(STEAM_REGISTRY);
		// The parser returns a recursive union, so navigating it in a test needs a cast.
		const apps = (parsed as unknown as SteamRegistry).Registry.HKCU.Software.Valve.Steam.Apps;
		expect(apps['570'].installed).toBe('0');
	});

	it('reports values as strings, since KeyValues has no number type', () => {
		const parsed = parseVdf('"a" { "n" "1" }');
		expect((parsed.a as Record<string, unknown>).n).toBe('1');
		expect((parsed.a as Record<string, unknown>).n).not.toBe(1);
	});

	it('unescapes backslash sequences', () => {
		const parsed = parseVdf('"path" "C:\\\\Program Files\\\\Steam"');
		expect(parsed.path).toBe('C:\\Program Files\\Steam');
	});

	it('ignores comments', () => {
		const parsed = parseVdf('// leading comment\n"k" "v" // trailing');
		expect(parsed.k).toBe('v');
	});

	it('rejects an unterminated block rather than returning a half-parsed tree', () => {
		expect(() => parseVdf('"a" {')).toThrow(/unterminated/i);
	});

	it('rejects a stray closing brace', () => {
		expect(() => parseVdf('}')).toThrow(/unbalanced/i);
	});

	it('rejects a block that is not preceded by a key', () => {
		expect(() => parseVdf('"a" "b" { }')).toThrow(/without a key/i);
	});

	it('returns an empty object for empty input', () => {
		expect(parseVdf('')).toEqual({});
	});
});
