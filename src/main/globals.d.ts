import type { BrowserWindow } from 'electron';

// NodeJS.Global was removed from @types/node in Node 16, so the old
// `declare global { namespace NodeJS { interface Global } }` form no longer applies.
declare global {
	var mainWindow: BrowserWindow | null;

	var overlay: BrowserWindow | null;

	var lobbyBrowser: BrowserWindow | null;
}
