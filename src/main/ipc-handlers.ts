import { app, dialog, ipcMain, shell } from 'electron';
import registry from 'registry-js';
const { enumerateValues, enumerateKeys, HKEY } = registry;
import {
	DefaultGamePlatforms,
	GamePlatform,
	type GamePlatformInstance,
	type GamePlatformMap,
	PlatformRunType,
} from '../common/GamePlatform';
import spawn from 'cross-spawn';
import path from 'node:path';
import fs from 'node:fs';

import { IpcHandlerMessages, IpcMessages, type IpcOverlayMessages } from '../common/ipc-messages';

// Listeners are fire and forget, they do not have "responses" or return values
export const initializeIpcListeners = (): void => {
	ipcMain.on(IpcMessages.SHOW_ERROR_DIALOG, (_e, opts: { title: string; content: string }) => {
		if (typeof opts === 'object' && opts && typeof opts.title === 'string' && typeof opts.content === 'string') {
			dialog.showErrorBox(opts.title, opts.content);
		}
	});

	ipcMain.on(IpcMessages.OPEN_AMONG_US_GAME, (_, platform: GamePlatformInstance) => {
		const error = () => dialog.showErrorBox('Error', 'Could not start the game.');

		if (platform.launchType === PlatformRunType.URI) {
			// openExternal rejects rather than throwing, so the EXE branch's try/catch
			// would not have caught this. Same dialog either way: from here the two
			// failures are the same thing, the game did not start.
			shell.openExternal(platform.runPath).catch(error);
		} else if (platform.launchType === PlatformRunType.EXE) {
			try {
				const process = spawn(path.join(platform.runPath, platform.execute[0]), platform.execute.slice(1), {
					detached: true,
					stdio: 'ignore',
				});
				process.on('error', error);
				process.unref();
			} catch {
				error();
			}
		}
	});

	ipcMain.on(IpcMessages.RESTART_CREWLINK, () => {
		app.relaunch();
		app.quit();
	});

	ipcMain.on(IpcMessages.SEND_TO_OVERLAY, (_, event: IpcOverlayMessages, ...args: unknown[]) => {
		try {
			if (global.overlay) global.overlay.webContents.send(event, ...args);
		} catch {
			/*empty*/
		}
	});

	ipcMain.on(IpcMessages.SEND_TO_MAINWINDOW, (_, event: IpcOverlayMessages, ...args: unknown[]) => {
		console.log('SEND TO MAINWINDOW CALLLED');
		try {
			if (global.mainWindow) global.mainWindow.webContents.send(event, ...args);
		} catch {
			/*empty*/
		}
	});

	ipcMain.on(IpcMessages.QUIT_CREWLINK, () => {
		try {
			const mainWindow = global.mainWindow;
			const overlay = global.overlay;
			global.mainWindow = null;
			global.overlay = null;
			mainWindow?.close();
			overlay?.close();
			mainWindow?.destroy();
			overlay?.destroy();
		} catch {
			/* empty */
		}
		app.quit();
	});
};

// Handlers are async cross-process instructions, they should have a return value
// or the caller should be "await"'ing them.  If neither of these are the case
// consider making it a "listener" instead for performance and readability
export const initializeIpcHandlers = (): void => {
	// The same ACL_RECORD that drives the memory recorder, so one variable turns on both
	// halves of what gate G1 and G2 need and they cannot be captured out of step.
	ipcMain.handle(IpcHandlerMessages.REQUEST_VOICE_RECORDING, () => {
		const name = process.env.ACL_RECORD;
		return name ? { userData: app.getPath('userData'), name } : null;
	});

	ipcMain.handle(IpcMessages.REQUEST_PLATFORMS_AVAILABLE, (_, customPlatforms: GamePlatformMap) => {
		// Assume all game platforms are unavailable unless proven otherwise
		const availableGamePlatforms: GamePlatformMap = {};

		// Windows only since 2026-08-25. There used to be a Linux arm here that read
		// ~/.steam/registry.vdf, and the VDF parser existed for it alone.

		// Steam
		if (
			enumerateValues(HKEY.HKEY_CLASSES_ROOT, 'steam').find((value) => (value ? value.name === 'URL Protocol' : false))
		) {
			availableGamePlatforms[GamePlatform.STEAM] = DefaultGamePlatforms[GamePlatform.STEAM];
		}

		// Epic Games
		if (
			enumerateValues(HKEY.HKEY_CLASSES_ROOT, 'com.epicgames.launcher').find((value) =>
				value ? value.name === 'URL Protocol' : false
			)
		) {
			availableGamePlatforms[GamePlatform.EPIC] = DefaultGamePlatforms[GamePlatform.EPIC];
		}

		// Microsoft Store
		// Search for 'Innersloth.Among Us....' key and grab it
		const microsoft_regkey = enumerateKeys(
			HKEY.HKEY_CURRENT_USER,
			'SOFTWARE\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages'
		).find((reg_key) => reg_key.startsWith('Innersloth.AmongUs' as string));

		if (microsoft_regkey) {
			// Grab the game path from the above key
			const value_found = enumerateValues(
				HKEY.HKEY_CURRENT_USER,
				'SOFTWARE\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages' +
					'\\' +
					microsoft_regkey
			).find((value) => (value ? value.name === 'PackageRootFolder' : false));
			if (value_found) {
				availableGamePlatforms[GamePlatform.MICROSOFT] = DefaultGamePlatforms[GamePlatform.MICROSOFT];
				availableGamePlatforms[GamePlatform.MICROSOFT].runPath = value_found.data as string;
			}
		}

		// Deal with custom client-added platforms
		for (const key in customPlatforms) {
			const game_platform = customPlatforms[key];

			if (game_platform.launchType === PlatformRunType.URI) {
				// I really have no clue how to check this, so we're trusting they exist
				availableGamePlatforms[key] = game_platform;
			} else if (game_platform.launchType === PlatformRunType.EXE) {
				try {
					fs.accessSync(path.join(game_platform.runPath, game_platform.execute[0]), fs.constants.X_OK);
					availableGamePlatforms[key] = game_platform;
				} catch {}
			}
		}

		return availableGamePlatforms;
	});
};
