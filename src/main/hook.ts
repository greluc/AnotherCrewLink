import { app, ipcMain } from 'electron';
import GameReader from './GameReader';
import { uIOhook } from 'uiohook-napi';
import Store from 'electron-store';
import type { ISettings, playerConfigMap } from '../common/ISettings';
import { IpcHandlerMessages, IpcMessages, IpcRendererMessages, IpcSyncMessages } from '../common/ipc-messages';
import { type Binding, bindingFor, matchesKey, matchesMouse } from './keyBindings';

const store = new Store<ISettings>();

// Cap the per-player volume map, but prune instead of emptying it. The previous
// code deleted every entry once the map passed 50, so anyone who had met more than
// 50 players lost all of their per-player volumes on the next start, over and over.
const PLAYER_CONFIG_LIMIT = 200;
const currentPlayerConfigMap = store.get('playerConfigMap', {}) as playerConfigMap;
const playerConfigEntries = Object.entries(currentPlayerConfigMap);
console.log('CONFIG count: ', playerConfigEntries.length);
if (playerConfigEntries.length > PLAYER_CONFIG_LIMIT) {
	const keep = playerConfigEntries
		.sort(([, a], [, b]) => (b?.lastUsed ?? 0) - (a?.lastUsed ?? 0))
		.slice(0, PLAYER_CONFIG_LIMIT);
	const pruned: playerConfigMap = {};
	for (const [key, value] of keep) {
		pruned[key as unknown as number] = value;
	}
	store.set('playerConfigMap', pruned);
	console.log('CONFIG pruned to: ', keep.length);
}

let readingGame = false;
export let gameReader: GameReader;

let pushToTalkShortcut: Binding = { kind: 'none' };
let deafenShortcut: Binding = { kind: 'none' };
let muteShortcut: Binding = { kind: 'none' };
let impostorRadioShortcut: Binding = { kind: 'none' };

// Nothing is registered with the hook: libuiohook reports every key and the filtering
// happens here. The previous watcher had to be told which keys to poll for, which is why
// this used to clear and re-add hooks on every settings change.
function resetKeyHooks(): void {
	pushToTalkShortcut = bindingFor(store.get('pushToTalkShortcut', 'V') as string);
	deafenShortcut = bindingFor(store.get('deafenShortcut', 'RControl') as string);
	muteShortcut = bindingFor(store.get('muteShortcut', 'RAlt') as string);
	impostorRadioShortcut = bindingFor(store.get('impostorRadioShortcut', 'F') as string);
}

ipcMain.on(IpcHandlerMessages.RESET_KEYHOOKS, () => {
	resetKeyHooks();
});

ipcMain.on(IpcSyncMessages.GET_INITIAL_STATE, (event) => {
	if (!readingGame) {
		console.error('Recieved GET_INITIAL_STATE message before the START_HOOK message was received');
		event.returnValue = null;
		return;
	}
	event.returnValue = gameReader.lastState;
});

ipcMain.handle(IpcMessages.REQUEST_MOD, () => {
	return gameReader.loadedMod.id;
});

ipcMain.handle(IpcHandlerMessages.START_HOOK, async (event) => {
	if (!readingGame) {
		readingGame = true;
		// Tracks which shortcuts are currently held down. A set instead of a counter,
		// because a counter can desync permanently: the impostor radio only counted up
		// while the local player was an impostor, so a state change between keydown and
		// keyup skipped the decrement and left the microphone open until restart.
		const speakingKeys = new Set<string>();
		// Kept apart from `speakingKeys`, because that set decides whether the microphone
		// is open. Deafen and mute are held keys too, and putting them in there would open
		// the microphone for as long as somebody holds the key that mutes them.
		const heldKeys = new Set<string>();
		const isLocalImpostor = (): boolean =>
			gameReader?.lastState.players?.find((value) => value.clientId === gameReader.lastState.clientId)?.isImpostor ===
			true;
		// Only on a change. A held key repeats at the operating system's repeat rate, and
		// the previous watcher polled for transitions, so sending on every event would put
		// about thirty identical messages a second across the IPC boundary for as long as
		// somebody holds push-to-talk.
		let lastSent: boolean | undefined;
		const sendPushToTalk = () => {
			const speaking = speakingKeys.size > 0;
			if (speaking === lastSent) return;
			lastSent = speaking;
			event.sender.send(IpcRendererMessages.PUSH_TO_TALK, speaking);
		};
		resetKeyHooks();

		// One implementation for both, because the extra mouse buttons are shortcuts like
		// any other and arrive on a different event only because libuiohook separates
		// them. `GetAsyncKeyState` did not, which is why they used to sit in a key table.
		const pressed = (isBound: (binding: Binding) => boolean) => {
			if (isBound(pushToTalkShortcut)) {
				speakingKeys.add('pushToTalk');
			}
			// Deafen and mute act on the release, so they have to see the press first. The
			// settings panel binds a shortcut on key *down* and re-reads the store
			// immediately, so without this the release of the very key you pressed to
			// assign mute was matched against the binding it had just created -- you were
			// muted the instant you bound mute, with no sound to say so, and the usual
			// conclusion is that the microphone is broken.
			if (isBound(deafenShortcut)) {
				heldKeys.add('deafen');
			}
			if (isBound(muteShortcut)) {
				heldKeys.add('mute');
			}
			// `has` first: the repeat would otherwise announce the radio again on every
			// repeated event for as long as the key is down.
			if (isBound(impostorRadioShortcut) && isLocalImpostor() && !speakingKeys.has('impostorRadio')) {
				speakingKeys.add('impostorRadio');
				event.sender.send(IpcRendererMessages.IMPOSTOR_RADIO, true);
			}

			sendPushToTalk();
		};

		const released = (isBound: (binding: Binding) => boolean) => {
			if (isBound(pushToTalkShortcut)) {
				speakingKeys.delete('pushToTalk');
			}
			// Only if this end saw the press. `delete` returning false means the key went
			// down before it was bound to this, which is what happens while assigning it.
			if (isBound(deafenShortcut) && heldKeys.delete('deafen')) {
				event.sender.send(IpcRendererMessages.TOGGLE_DEAFEN);
			}
			if (isBound(muteShortcut) && heldKeys.delete('mute')) {
				event.sender.send(IpcRendererMessages.TOGGLE_MUTE);
			}
			// Released unconditionally: the impostor state may have changed while held.
			if (isBound(impostorRadioShortcut) && speakingKeys.delete('impostorRadio')) {
				event.sender.send(IpcRendererMessages.IMPOSTOR_RADIO, false);
			}

			sendPushToTalk();
		};

		uIOhook.on('keydown', (e) => pressed((binding) => matchesKey(binding, e.keycode)));
		uIOhook.on('keyup', (e) => released((binding) => matchesKey(binding, e.keycode)));
		uIOhook.on('mousedown', (e) => pressed((binding) => matchesMouse(binding, Number(e.button))));
		uIOhook.on('mouseup', (e) => released((binding) => matchesMouse(binding, Number(e.button))));

		uIOhook.start();
		// A low-level hook that is still installed can hold up shutdown.
		app.once('will-quit', () => {
			try {
				uIOhook.stop();
			} catch (error) {
				console.error('Could not stop the input hook:', error);
			}
		});

		// Read game memory. If constructing it throws -- a module that will not load, a
		// platform check that fails -- the flag above has already been set, so the
		// renderer's retry takes the `else if (gameReader)` branch, finds nothing there,
		// and does nothing at all. The app then sits with the input hook running and no
		// game reader, for the rest of the session, with no way back short of a restart.
		try {
			gameReader = new GameReader(event.sender.send.bind(event.sender));
		} catch (error) {
			readingGame = false;
			console.error('Could not start reading the game:', error);
			event.sender.send(
				IpcRendererMessages.ERROR,
				`Could not start reading the game: ${error instanceof Error ? error.message : String(error)}`
			);
			return;
		}
		let gotError = false;
		const frame = async () => {
			const err = await gameReader.loop();
			if (err) {
				// readingGame = false;
				gotError = true;
				event.sender.send(IpcRendererMessages.ERROR, err);
				setTimeout(frame, 7500);
			} else {
				if (gotError) {
					event.sender.send(IpcRendererMessages.ERROR, '');
					gotError = false;
				}

				setTimeout(frame, 1000 / 5);
			}
		};
		await frame();
	} else if (gameReader) {
		gameReader.amongUs = null;
		gameReader.checkProcessDelay = 0;
	}
});

ipcMain.on('reload', async (_, lobbybrowser) => {
	if (!lobbybrowser) {
		global.mainWindow?.reload();
	}
	global.lobbyBrowser?.reload();
});

ipcMain.on('minimize', async (_, lobbybrowser) => {
	if (!lobbybrowser) {
		global.mainWindow?.minimize();
	}
	global.lobbyBrowser?.minimize();
});

ipcMain.handle('getlocale', () => {
	return app.getLocale();
});

ipcMain.on('relaunch', async () => {
	app.relaunch();
	app.exit();
});
