import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { KEY_NAMES, UIOHOOK_KEY_TABLE, bindingFor, matchesKey, matchesMouse } from './keyBindings';

/**
 * libuiohook's own table, read out of the vendored type declaration.
 *
 * The declaration is text, so this costs nothing and needs no native addon — which is the
 * point, because importing `uiohook-napi` itself loads one built for Electron's ABI and
 * this suite runs under node.
 */
function declaredKeycodes(): Record<string, number> {
	const declaration = readFileSync(join(__dirname, '../../native/uiohook-napi/dist/index.d.ts'), 'utf8');
	const table: Record<string, number> = {};
	for (const line of declaration.split('\n')) {
		const match = /^\s*readonly (\w+): (\d+);$/.exec(line);
		if (match) table[match[1]] = Number(match[2]);
	}
	return table;
}

// Every name the settings file may already hold. Taken from the shortcut table the
// previous watcher used, so a value written by any released version resolves to something.
const STORED_NAMES = [
	'Space',
	'Backspace',
	'Delete',
	'Enter',
	'Up',
	'Down',
	'Left',
	'Right',
	'Home',
	'CapsLock',
	'End',
	'PageUp',
	'PageDown',
	'Escape',
	'Control',
	'LShift',
	'RShift',
	'RAlt',
	'LAlt',
	'RControl',
	'LControl',
	'Shift',
	'Alt',
	'F1',
	'F2',
	'F3',
	'F4',
	'F5',
	'F6',
	'F7',
	'F8',
	'F9',
	'F10',
	'F11',
	'F12',
	'MouseButton4',
	'MouseButton5',
	'Numpad0',
	'Numpad1',
	'Numpad2',
	'Numpad3',
	'Numpad4',
	'Numpad5',
	'Numpad6',
	'Numpad7',
	'Numpad8',
	'Numpad9',
	'Disabled',
];

describe('the copied keycode table', () => {
	it('agrees with the vendored module', () => {
		// The reason this file exists. Upgrading `uiohook-napi` with a renumbered key would
		// otherwise reach a player as a shortcut that quietly stopped working, with nothing
		// in any log to say so.
		const declared = declaredKeycodes();
		for (const [name, keycode] of Object.entries(UIOHOOK_KEY_TABLE)) {
			expect(declared[name], `${name} is not declared by the vendored module`).toBeDefined();
			expect(declared[name], `${name} has moved`).toBe(keycode);
		}
	});

	it('read something, rather than passing on an empty file', () => {
		// A regex that matched nothing would make the check above vacuous.
		expect(Object.keys(declaredKeycodes()).length).toBeGreaterThan(80);
	});
});

describe('bindingFor', () => {
	it('resolves every name a settings file may already hold', () => {
		for (const name of STORED_NAMES) {
			const binding = bindingFor(name);
			if (name === 'Disabled') {
				expect(binding.kind).toBe('none');
			} else {
				expect(binding.kind, `${name} resolved to nothing`).not.toBe('none');
			}
		}
	});

	it('offers no name the resolver does not know', () => {
		expect(KEY_NAMES.every((name) => STORED_NAMES.includes(name))).toBe(true);
	});

	it('makes the unsided modifiers match either side', () => {
		// `GetAsyncKeyState(VK_SHIFT)` was true for either shift key, so somebody who bound
		// `Shift` years ago meant either one. libuiohook reports them separately.
		for (const [unsided, left, right] of [
			['Shift', 'LShift', 'RShift'],
			['Control', 'LControl', 'RControl'],
			['Alt', 'LAlt', 'RAlt'],
		]) {
			const either = bindingFor(unsided);
			const leftOnly = bindingFor(left);
			const rightOnly = bindingFor(right);
			expect(leftOnly.kind === 'keys' && matchesKey(either, leftOnly.keycodes[0])).toBe(true);
			expect(rightOnly.kind === 'keys' && matchesKey(either, rightOnly.keycodes[0])).toBe(true);
		}
	});

	it('keeps a sided modifier to its own side', () => {
		// The other half of the same behaviour, and the one a test could accidentally lose
		// by making everything match everything.
		const left = bindingFor('LShift');
		const right = bindingFor('RShift');
		expect(right.kind === 'keys' && matchesKey(left, right.keycodes[0])).toBe(false);
	});

	it('reads a letter either way round', () => {
		// The old code compared against a virtual-key code, which is the uppercase value,
		// so a lowercase letter matched nothing and looked like a shortcut that was set.
		expect(bindingFor('v')).toEqual(bindingFor('V'));
	});

	it('resolves a digit', () => {
		expect(bindingFor('7')).toEqual({ kind: 'keys', keycodes: [8] });
	});

	it('sends the extra mouse buttons down the mouse path', () => {
		// They sat in a table of keys before only because `GetAsyncKeyState` does not
		// distinguish. libuiohook raises them as mouse events, and a binding that claimed
		// to be a key would wait for an event that never comes.
		expect(bindingFor('MouseButton4')).toEqual({ kind: 'mouse', button: 4 });
		expect(matchesMouse(bindingFor('MouseButton5'), 5)).toBe(true);
		expect(matchesMouse(bindingFor('MouseButton5'), 4)).toBe(false);
		expect(matchesKey(bindingFor('MouseButton4'), 4)).toBe(false);
	});

	it('binds nothing for a name it does not know, rather than throwing', () => {
		// This runs while the app starts. A bad value in a settings file should cost one
		// shortcut, not the client.
		expect(bindingFor('NotAKey')).toEqual({ kind: 'none' });
		expect(bindingFor(undefined)).toEqual({ kind: 'none' });
		expect(bindingFor('')).toEqual({ kind: 'none' });
	});

	it('matches nothing when nothing is bound', () => {
		const none = bindingFor('Disabled');
		expect(matchesKey(none, 47)).toBe(false);
		expect(matchesMouse(none, 4)).toBe(false);
	});
});
