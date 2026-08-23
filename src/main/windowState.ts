import { app, BrowserWindow, screen } from 'electron';
import Store from 'electron-store';
import fs from 'fs';
import path from 'path';

// Replaces electron-window-state, which was last published in 2022 and pulled in
// jsonfile and mkdirp 0.5. The store this app already uses does the persistence.

interface WindowState {
	width: number;
	height: number;
	x?: number;
	y?: number;
}

interface Options {
	defaultWidth: number;
	defaultHeight: number;
	/** Key the bounds are stored under, so several windows can be tracked. */
	name: string;
}

const store = new Store<Record<string, WindowState>>({ name: 'windows' });

/**
 * electron-window-state wrote a flat {width,height,x,y} into window-state.json.
 * Read it once so upgrading users keep the size they had.
 */
function readLegacyState(): WindowState | undefined {
	try {
		const legacyPath = path.join(app.getPath('userData'), 'window-state.json');
		if (!fs.existsSync(legacyPath)) return undefined;
		const parsed = JSON.parse(fs.readFileSync(legacyPath, 'utf8'));
		if (typeof parsed?.width !== 'number' || typeof parsed?.height !== 'number') return undefined;
		return { width: parsed.width, height: parsed.height, x: parsed.x, y: parsed.y };
	} catch {
		return undefined;
	}
}

/** True if the saved rectangle still overlaps a connected display. */
function isVisibleOnSomeDisplay(state: WindowState): boolean {
	if (state.x === undefined || state.y === undefined) return false;
	return screen.getAllDisplays().some(({ bounds }) => {
		return (
			state.x! < bounds.x + bounds.width &&
			state.x! + state.width > bounds.x &&
			state.y! < bounds.y + bounds.height &&
			state.y! + state.height > bounds.y
		);
	});
}

export function windowStateKeeper(options: Options) {
	const saved = (store.get(options.name) as WindowState | undefined) ?? readLegacyState();
	// Fall back to the defaults if the window would open off-screen, which happens
	// after a monitor is disconnected or the layout changes.
	const state: WindowState =
		saved && isVisibleOnSomeDisplay(saved) ? saved : { width: options.defaultWidth, height: options.defaultHeight };

	let saveTimer: NodeJS.Timeout | undefined;

	return {
		get width() {
			return state.width;
		},
		get height() {
			return state.height;
		},
		get x() {
			return state.x;
		},
		get y() {
			return state.y;
		},
		manage(window: BrowserWindow) {
			const persist = () => {
				if (window.isDestroyed() || window.isMinimized() || window.isMaximized() || window.isFullScreen()) {
					return;
				}
				const bounds = window.getBounds();
				state.width = bounds.width;
				state.height = bounds.height;
				state.x = bounds.x;
				state.y = bounds.y;
				store.set(options.name, state);
			};
			// Debounced: resize and move fire continuously while dragging.
			const schedule = () => {
				if (saveTimer) clearTimeout(saveTimer);
				saveTimer = setTimeout(persist, 200);
			};
			window.on('resize', schedule);
			window.on('move', schedule);
			window.on('close', () => {
				if (saveTimer) clearTimeout(saveTimer);
				persist();
			});
		},
	};
}
