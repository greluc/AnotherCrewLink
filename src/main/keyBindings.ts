/**
 * Turns a stored shortcut name into the physical key or mouse button it means.
 *
 * The shortcuts players have saved are names — `'V'`, `'RControl'`, `'MouseButton4'` —
 * and they are stored that way, so this file is the only thing that has to change when
 * the hook underneath it does. That has now happened once: the previous watcher polled
 * `GetAsyncKeyState` and spoke in Windows virtual-key codes, and libuiohook speaks in its
 * own scancodes on every platform. Nobody's settings needed migrating.
 *
 * # Why the numbers are written out here
 *
 * `uiohook-napi` exports the same table, but importing anything from it loads the native
 * addon on the first line of its entry point, and the addon is built against Electron's
 * ABI. A node-environment test that imported it would not start. So the subset this
 * client uses is copied, and `keyBindings.test.ts` reads the vendored `index.d.ts` — a
 * text file, no addon — and fails if any number here has drifted from it. Upgrading the
 * vendored module without noticing a renumbered key is the failure that would otherwise
 * reach a player as a shortcut that silently stopped working.
 */

/**
 * libuiohook's scancodes, for the keys this client can bind.
 *
 * Checked against `native/uiohook-napi/dist/index.d.ts` by the test beside this file.
 */
const UIOHOOK_KEY = {
	Backspace: 14,
	Enter: 28,
	CapsLock: 58,
	Escape: 1,
	Space: 57,
	PageUp: 3657,
	PageDown: 3665,
	End: 3663,
	Home: 3655,
	ArrowLeft: 57419,
	ArrowUp: 57416,
	ArrowRight: 57421,
	ArrowDown: 57424,
	Delete: 3667,
	0: 11,
	1: 2,
	2: 3,
	3: 4,
	4: 5,
	5: 6,
	6: 7,
	7: 8,
	8: 9,
	9: 10,
	A: 30,
	B: 48,
	C: 46,
	D: 32,
	E: 18,
	F: 33,
	G: 34,
	H: 35,
	I: 23,
	J: 36,
	K: 37,
	L: 38,
	M: 50,
	N: 49,
	O: 24,
	P: 25,
	Q: 16,
	R: 19,
	S: 31,
	T: 20,
	U: 22,
	V: 47,
	W: 17,
	X: 45,
	Y: 21,
	Z: 44,
	Numpad0: 82,
	Numpad1: 79,
	Numpad2: 80,
	Numpad3: 81,
	Numpad4: 75,
	Numpad5: 76,
	Numpad6: 77,
	Numpad7: 71,
	Numpad8: 72,
	Numpad9: 73,
	F1: 59,
	F2: 60,
	F3: 61,
	F4: 62,
	F5: 63,
	F6: 64,
	F7: 65,
	F8: 66,
	F9: 67,
	F10: 68,
	F11: 87,
	F12: 88,
	Ctrl: 29,
	CtrlRight: 3613,
	Alt: 56,
	AltRight: 3640,
	Shift: 42,
	ShiftRight: 54,
} as const;

type UiohookKeyName = keyof typeof UIOHOOK_KEY;

// A string-keyed view of the same table. `keyof typeof UIOHOOK_KEY` is a union of numeric
// and string literals, so indexing it with a plain string needs a cast that TypeScript
// widens to `any` -- which would quietly disable the checking this file is here for.
const BY_NAME: Record<string, number> = UIOHOOK_KEY;

/**
 * What one shortcut resolves to.
 *
 * `keycodes` is a list rather than a single code because of the three unsided names.
 * `GetAsyncKeyState(VK_SHIFT)` was true for either shift key, so a player who bound
 * `Shift` meant either one; libuiohook reports the two separately, and collapsing that
 * back is this list's whole purpose. Binding `LShift` still means only the left one.
 */
export type Binding = { kind: 'keys'; keycodes: number[] } | { kind: 'mouse'; button: number } | { kind: 'none' };

/**
 * The names the settings store may hold, and what each one is.
 *
 * The names are the ones already written to disk. Do not rename them without a migration:
 * an unrecognised name binds nothing, and a player would find push-to-talk simply dead.
 */
const NAMED: Record<string, UiohookKeyName[] | { button: number } | null> = {
	Space: ['Space'],
	Backspace: ['Backspace'],
	Delete: ['Delete'],
	Enter: ['Enter'],
	Up: ['ArrowUp'],
	Down: ['ArrowDown'],
	Left: ['ArrowLeft'],
	Right: ['ArrowRight'],
	Home: ['Home'],
	CapsLock: ['CapsLock'],
	End: ['End'],
	PageUp: ['PageUp'],
	PageDown: ['PageDown'],
	Escape: ['Escape'],
	// The three unsided ones. See `Binding`.
	Control: ['Ctrl', 'CtrlRight'],
	Shift: ['Shift', 'ShiftRight'],
	Alt: ['Alt', 'AltRight'],
	LShift: ['Shift'],
	RShift: ['ShiftRight'],
	LAlt: ['Alt'],
	RAlt: ['AltRight'],
	LControl: ['Ctrl'],
	RControl: ['CtrlRight'],
	F1: ['F1'],
	F2: ['F2'],
	F3: ['F3'],
	F4: ['F4'],
	F5: ['F5'],
	F6: ['F6'],
	F7: ['F7'],
	F8: ['F8'],
	F9: ['F9'],
	F10: ['F10'],
	F11: ['F11'],
	F12: ['F12'],
	// The extra mouse buttons arrive on a different event entirely. The old watcher saw
	// them as virtual-key codes because `GetAsyncKeyState` does not distinguish, which is
	// why they sat in a table of keys.
	MouseButton4: { button: 4 },
	MouseButton5: { button: 5 },
	Numpad0: ['Numpad0'],
	Numpad1: ['Numpad1'],
	Numpad2: ['Numpad2'],
	Numpad3: ['Numpad3'],
	Numpad4: ['Numpad4'],
	Numpad5: ['Numpad5'],
	Numpad6: ['Numpad6'],
	Numpad7: ['Numpad7'],
	Numpad8: ['Numpad8'],
	Numpad9: ['Numpad9'],
	Disabled: null,
};

const NONE: Binding = { kind: 'none' };

/**
 * Resolves a stored shortcut name.
 *
 * Single characters are uppercased first. The old code compared `charCodeAt(0)` against a
 * virtual-key code, and those are the uppercase values, so a lowercase letter never
 * matched anything — a shortcut that looked set and did nothing. Accepting either is
 * strictly more forgiving than what it replaces.
 *
 * An unknown name binds nothing rather than throwing. This runs while the app is starting
 * and a bad value in a settings file should cost one shortcut, not the whole client.
 */
export function bindingFor(name: string | undefined): Binding {
	if (!name) return NONE;

	const named = NAMED[name];
	if (named === null) return NONE;
	if (named !== undefined) {
		if (Array.isArray(named)) {
			return { kind: 'keys', keycodes: named.map((key) => UIOHOOK_KEY[key]) };
		}
		return { kind: 'mouse', button: named.button };
	}

	if (name.length === 1) {
		const keycode = BY_NAME[name.toUpperCase()];
		if (keycode !== undefined) return { kind: 'keys', keycodes: [keycode] };
	}

	console.error('Unknown shortcut, ignoring it:', name);
	return NONE;
}

/** Whether a keyboard event is the one this shortcut is waiting for. */
export function matchesKey(binding: Binding, keycode: number): boolean {
	return binding.kind === 'keys' && binding.keycodes.includes(keycode);
}

/** Whether a mouse button event is the one this shortcut is waiting for. */
export function matchesMouse(binding: Binding, button: number): boolean {
	return binding.kind === 'mouse' && binding.button === button;
}

export const KEY_NAMES = Object.keys(NAMED);
export const UIOHOOK_KEY_TABLE: Record<string, number> = BY_NAME;
