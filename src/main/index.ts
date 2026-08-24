import { autoUpdater } from 'electron-updater';
import { app, BrowserWindow, ipcMain, session, shell } from 'electron';
import { windowStateKeeper } from './windowState';
import { platform } from 'node:os';
import { join as joinPath, dirname, resolve as resolvePath, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { format as formatUrl } from 'node:url';
import './hook';
import overlayWindowModule from 'electron-overlay-window';
const { overlayWindow } = overlayWindowModule;
import { initializeIpcHandlers, initializeIpcListeners } from './ipc-handlers';
import { GenerateHat } from './avatarGenerator';
import { captureMainConsole, logDirectory, writeLog } from './logFile';
import { HAT_COLLECTION_URL } from '../common/hatCollection';
import { gameReader } from './hook';
import { IpcRendererMessages, IpcHandlerMessages } from '../common/ipc-messages';
import { resetOffsetsToEmbedded } from './offsetStore';
import { startRecordingIfAsked } from './recorder';
import type { ProgressInfo, UpdateInfo } from 'builder-util-runtime';
import { protocol } from 'electron';
import Store from 'electron-store';
import type { ISettings } from '../common/ISettings';
import installExtension, { REACT_DEVELOPER_TOOLS } from 'electron-devtools-installer';
import minimist from 'minimist';
const args = minimist(process.argv);
// __dirname does not exist in an ES module.
const moduleDir = dirname(fileURLToPath(import.meta.url));
// electron-vite only defines ELECTRON_RENDERER_URL while the dev server runs, which
// is a more reliable signal here than NODE_ENV.
const isDevelopment = !!process.env.ELECTRON_RENDERER_URL;
const devTools = (isDevelopment || args.dev === 1) && true;
const appVersion: string = isDevelopment ? 'DEV' : autoUpdater.currentVersion.version;

// global reference to mainWindow (necessary to prevent window from being garbage collected)
global.mainWindow = null;
global.overlay = null;
const store = new Store<ISettings>();
app.commandLine.appendSwitch('disable-pinch');

if (platform() === 'linux' || !store.get('hardware_acceleration', true)) {
	app.disableHardwareAcceleration();
}

if (platform() === 'linux') {
	app.commandLine.appendSwitch('disable-gpu-sandbox');
}

// The layout was designed at this size, so it stays the floor and the default.
const MAIN_WINDOW_MIN_WIDTH = 250;
const MAIN_WINDOW_MIN_HEIGHT = 350;

function createMainWindow() {
	// Without defaults the keeper reports 800x600, and its width/height were never
	// passed to the window anyway, so the saved size was dead weight.
	const mainWindowState = windowStateKeeper({
		name: 'main',
		defaultWidth: MAIN_WINDOW_MIN_WIDTH,
		defaultHeight: MAIN_WINDOW_MIN_HEIGHT,
	});

	const window = new BrowserWindow({
		title: 'AnotherCrewLink',
		width: mainWindowState.width,
		height: mainWindowState.height,
		minWidth: MAIN_WINDOW_MIN_WIDTH,
		minHeight: MAIN_WINDOW_MIN_HEIGHT,
		x: mainWindowState.x,
		y: mainWindowState.y,
		resizable: true,
		frame: false,
		fullscreenable: false,
		maximizable: false,
		webPreferences: {
			nodeIntegration: true,
			contextIsolation: false,
		},
	});
	mainWindowState.manage(window);
	hardenWindow(window);
	captureConsole(window, 'renderer');

	if (devTools) {
		//Force devtools into detached mode otherwise they are unusable
		window.on('ready-to-show', () => {
			window.webContents.openDevTools({
				mode: 'detach',
			});
		});
	}

	if (isDevelopment) {
		window.loadURL(`${process.env.ELECTRON_RENDERER_URL}?version=DEV&view=app`);
	} else {
		window.loadURL(
			formatUrl({
				pathname: joinPath(moduleDir, '../renderer/index.html'),
				protocol: 'file',
				query: {
					version: appVersion,
					view: 'app',
				},
				slashes: true,
			})
		);
	}
	//window.webContents.userAgent = `CrewLink/${crewlinkVersion} (${process.platform})`;
	window.webContents.userAgent = `AnotherCrewLink/${appVersion} (win32)`;

	window.on('closed', () => {
		try {
			const mainWindow = global.mainWindow;
			const overlay = global.overlay;
			global.mainWindow = null;
			global.overlay = null;
			overlay?.close();
			mainWindow?.destroy();
			overlay?.destroy();
		} catch {
			/* empty */
		}
	});

	window.webContents.on('devtools-opened', () => {
		window.focus();
		setImmediate(() => {
			window.focus();
		});
	});
	console.log('Opened app version: ', appVersion);
	return window;
}

function createLobbyBrowser() {
	const window = new BrowserWindow({
		title: 'AnotherCrewLink Browser',
		width: 900,
		height: 500,
		minWidth: 250,
		minHeight: 350,
		resizable: true,
		frame: false,
		fullscreenable: false,
		closable: true,
		maximizable: false,
		webPreferences: {
			nodeIntegration: true,
			contextIsolation: false,
		},
	});

	hardenWindow(window);
	captureConsole(window, 'lobbybrowser');

	window.on('closed', () => {
		global.lobbyBrowser = null;
	});
	// if (devTools) {
	// 	// Force devtools into detached mode otherwise they are unusable
	// 	window.webContents.openDevTools({
	// 		mode: 'detach',
	// 	});
	// }
	if (isDevelopment) {
		window.loadURL(`${process.env.ELECTRON_RENDERER_URL}?version=DEV&view=lobbies`);
	} else {
		window.loadURL(
			formatUrl({
				pathname: joinPath(moduleDir, '../renderer/index.html'),
				protocol: 'file',
				query: {
					version: appVersion,
					view: 'lobbies',
				},
				slashes: true,
			})
		);
	}
	window.webContents.userAgent = `AnotherCrewLink/${appVersion} (win32)`;
	console.log('Opened app version: ', appVersion);
	return window;
}

function createOverlay() {
	const overlay = new BrowserWindow({
		title: 'AnotherCrewLink Overlay',
		width: 400,
		height: 300,
		webPreferences: {
			nodeIntegration: true,
			contextIsolation: false,
		},
		fullscreenable: true,
		skipTaskbar: true,
		frame: false,
		show: false,
		transparent: true,
		resizable: true,
		focusable: false,

		//	...overlayWindow.WINDOW_OPTS,
	});

	if (devTools) {
		overlay.webContents.openDevTools({
			mode: 'detach',
		});
	}

	if (isDevelopment) {
		overlay.loadURL(`${process.env.ELECTRON_RENDERER_URL}?version=${appVersion}&view=overlay`);
	} else {
		overlay.loadURL(
			formatUrl({
				pathname: joinPath(moduleDir, '../renderer/index.html'),
				protocol: 'file',
				query: {
					version: appVersion,
					view: 'overlay',
				},
				slashes: true,
			})
		);
	}
	hardenWindow(overlay);
	captureConsole(overlay, 'overlay');
	overlay.setIgnoreMouseEvents(true);
	overlayWindow.attachTo(overlay, 'Among Us');
	overlay.setBackgroundColor('#00000000');
	return overlay;
}

/**
 * With nodeIntegration enabled, any navigation away from our own pages would hand a
 * remote document full Node access, so navigation and window creation are refused
 * outright. Links are opened in the user's browser instead.
 */
function captureConsole(window: BrowserWindow, source: string): void {
	window.webContents.on('console-message', (details) => {
		writeLog(source, details.level, `${details.message} (${details.sourceId}:${details.lineNumber})`);
	});
	window.webContents.on('render-process-gone', (_event, details) => {
		writeLog(source, 'error', `render process gone: ${details.reason} (exit ${details.exitCode})`);
	});
	window.webContents.on('unresponsive', () => writeLog(source, 'error', 'window became unresponsive'));
}

function hardenWindow(window: BrowserWindow): void {
	const isOwnPage = (target: string) =>
		target.startsWith('file://') ||
		(!!process.env.ELECTRON_RENDERER_URL && target.startsWith(process.env.ELECTRON_RENDERER_URL));

	window.webContents.on('will-navigate', (event, target) => {
		if (!isOwnPage(target)) {
			event.preventDefault();
			console.warn('Blocked navigation to', target);
		}
	});
	window.webContents.setWindowOpenHandler(({ url }) => {
		// Only ever hand http(s) to the system browser; never open a second renderer.
		if (url.startsWith('https://') || url.startsWith('http://')) {
			shell.openExternal(url);
		}
		return { action: 'deny' };
	});
	window.webContents.on('will-attach-webview', (event) => {
		event.preventDefault();
	});
}

const gotTheLock = app.requestSingleInstanceLock();
if (!gotTheLock) {
	app.quit();
} else {
	captureMainConsole();
	console.log(
		`AnotherCrewLink ${appVersion} starting on ${process.platform} ${process.arch}, electron ${process.versions.electron}`
	);
	console.log('Logging to', logDirectory());

	autoUpdater.autoDownload = false;
	// The returned promise rejects alongside the error event, and nothing was waiting on
	// it, so every failed check also produced an unhandled rejection warning. The error
	// handler below is what reports it to the user.
	autoUpdater.checkForUpdates().catch(() => {
		/* reported through the error event */
	});
	autoUpdater.on('update-available', (info: UpdateInfo) => {
		try {
			global.mainWindow?.webContents.send(IpcRendererMessages.AUTO_UPDATER_STATE, {
				state: 'available',
				info: info,
			});
		} catch {
			/* Empty block */
		}
	});
	// electron-updater 6 hands the handler an Error, not a string. Send the message
	// across IPC because Error instances do not survive structured cloning intact.
	autoUpdater.on('error', (err: Error) => {
		try {
			global.mainWindow?.webContents.send(IpcRendererMessages.AUTO_UPDATER_STATE, {
				state: 'error',
				error: err?.message ?? String(err),
			});
		} catch {
			/*empty*/
		}
	});
	autoUpdater.on('download-progress', (progress: ProgressInfo) => {
		try {
			global.mainWindow?.webContents.send(IpcRendererMessages.AUTO_UPDATER_STATE, {
				state: 'downloading',
				progress,
			});
		} catch {
			/*empty*/
		}
	});
	autoUpdater.on('update-downloaded', () => {
		autoUpdater.quitAndInstall();
	});

	// quit application when all windows are closed
	app.on('window-all-closed', () => {
		// on macOS it is common for applications to stay open until the user explicitly quits
		try {
			const mainWindow = global.mainWindow;
			const overlay = global.overlay;
			global.mainWindow = null;
			global.overlay = null;
			overlay?.close();
			mainWindow?.destroy();
			overlay?.destroy();
		} catch {
			/* empty */
		}
		app.quit();
	});

	app.on('activate', () => {
		console.log('ACTIVATE???');
		// on macOS it is common to re-create a window even after all windows have been closed
		if (global.mainWindow === null) {
			global.mainWindow = createMainWindow();
		}

		session.fromPartition('default').setPermissionRequestHandler((_webContents, permission, callback) => {
			const allowedPermissions = ['audioCapture']; // Full list here: https://developer.chrome.com/extensions/declare_permissions#manifest
			console.log('permission requested ', permission);
			if (allowedPermissions.includes(permission)) {
				callback(true); // Approve permission request
			} else {
				console.error(
					`The application tried to request permission for '${permission}'. This permission was not whitelisted and has been blocked.`
				);

				callback(false); // Deny
			}
		});
	});

	// create main BrowserWindow when electron is ready
	app.whenReady().then(() => {
		const staticRoot = joinPath(app.getPath('userData'), 'static');
		protocol.registerFileProtocol('static', (request, callback) => {
			// Contain the path inside userData/static. The URL is built from data read out
			// of game memory, and the previous string concatenation let ../ escape it.
			const requested = decodeURIComponent(request.url.replace('static:///', '')).split('?')[0];
			const resolved = resolvePath(staticRoot, requested);
			if (resolved !== staticRoot && !resolved.startsWith(staticRoot + sep)) {
				console.warn('Blocked static: request outside of the static directory:', requested);
				callback({ error: -6 }); // FILE_NOT_FOUND
				return;
			}
			callback(resolved);
		});

		protocol.registerFileProtocol('generate', async (request, callback) => {
			try {
				const url = new URL(request.url.replace('generate:///', ''));
				// Only the pinned hat collection may be fetched and recoloured. Without this
				// the renderer could make the main process retrieve any URL and write the
				// result into userData.
				if (`${url.origin}${url.pathname}`.startsWith(HAT_COLLECTION_URL) !== true) {
					console.warn('Blocked generate: request for an unexpected origin:', url.origin);
					callback({ error: -6 });
					return;
				}
				if (!gameReader?.playercolors?.length) {
					callback({ error: -6 });
					return;
				}
				const path = await GenerateHat(url, gameReader.playercolors, Number(url.searchParams.get('color')));
				callback(path || { error: -6 });
			} catch (error) {
				console.warn('generate: handler failed', error);
				callback({ error: -6 });
			}
		});

		// No CSP was ever set, which Electron warns about at runtime. Scripts and styles
		// come from the bundle; images additionally from the pinned hat CDN and our own
		// protocols; connections go to the configured server and STUN/TURN.
		session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
			callback({
				responseHeaders: {
					...details.responseHeaders,
					'Content-Security-Policy': [
						[
							"default-src 'self'",
							// 'unsafe-eval' is required by electron-store: conf validates its schema
							// with ajv, which compiles schemas by generating and evaluating code.
							// Remote scripts stay blocked, which is the vector that matters here.
							// Dropping it means moving the settings store behind IPC in the main
							// process, which is worth doing but is its own change.
							"script-src 'self' 'unsafe-eval'",
							"style-src 'self' 'unsafe-inline'",
							"img-src 'self' data: static: generate: https://cdn.jsdelivr.net",
							"media-src 'self' data:",
							"font-src 'self' data:",
							'connect-src *',
							"object-src 'none'",
							"frame-src 'none'",
						].join('; '),
					],
				},
			});
		});

		initializeIpcListeners();
		initializeIpcHandlers();
		global.mainWindow = createMainWindow();

		if (isDevelopment)
			installExtension(REACT_DEVELOPER_TOOLS)
				.then((extension) => console.log(`Added Extension:  ${extension.name}`))
				.catch((err: string) => console.log('An error occurred: ', err));
	});

	app.on('second-instance', () => {
		// Someone tried to run a second instance, we should focus our window.
		if (global.mainWindow) {
			if (global.mainWindow.isMinimized()) global.mainWindow.restore();
			global.mainWindow.focus();
		}
	});

	ipcMain.on('update-app', () => {
		autoUpdater.downloadUpdate();
	});

	// The manual recovery path. Without a signature there is no signed floor to supersede
	// a bad bundle from, so reverting the mirror fixes the next download but not the copy
	// already cached on a player's machine. This is what they have instead, and it is a
	// button rather than an instruction to delete a file out of `userData`.
	// Gate G1 of the Rust port compares the Rust reader against this one, frame for
	// frame. Recording is off unless ACL_RECORD names a session.
	startRecordingIfAsked();

	ipcMain.handle(IpcHandlerMessages.RESET_OFFSETS, async () => {
		const status = resetOffsetsToEmbedded();
		// The reader holds the old offsets in memory, and those are the ones being
		// recovered from, so clearing the caches on disk is only half of it.
		await gameReader?.reloadOffsets();
		return status;
	});

	ipcMain.on(IpcHandlerMessages.OPEN_LOBBYBROWSER, () => {
		if (!global.lobbyBrowser) {
			global.lobbyBrowser = createLobbyBrowser();
		} else {
			global.lobbyBrowser.show();
			global.lobbyBrowser.moveTop();
		}
	});

	ipcMain.on('enableOverlay', async (_event, enable) => {
		setTimeout(() => {
			try {
				if (enable) {
					if (!global.overlay) {
						global.overlay = createOverlay();
					}
					overlayWindow.show();
				} else {
					overlayWindow.hide();
					if (global.overlay?.closable) {
						overlayWindow.stop();
						global.overlay?.close();
						global.overlay = null;
					}
				}
			} catch {
				global.overlay?.hide();
				global.overlay?.close();
			}
		}, 1000);
	});

	ipcMain.on('setAlwaysOnTop', async (_event, enable) => {
		console.log('SETALWAYSONTOP?');
		if (global.mainWindow) {
			console.log('SETALWAYSONTOP?1');
			global.mainWindow.setAlwaysOnTop(enable, 'screen-saver');
		}
	});
}
