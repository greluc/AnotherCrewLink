import type { BrowserWindow } from 'electron';

// NodeJS.Global was removed from @types/node in Node 16, so the old
// `declare global { namespace NodeJS { interface Global } }` form no longer applies.
declare global {
	// eslint-disable-next-line no-var
	var mainWindow: BrowserWindow | null;
	// eslint-disable-next-line no-var
	var overlay: BrowserWindow | null;
	// eslint-disable-next-line no-var
	var lobbyBrowser: BrowserWindow | null;
}
