import { createWriteStream, mkdirSync, type WriteStream } from 'node:fs';
import { join } from 'node:path';
import type { AmongUsState, Player } from '../common/AmongUsState';
import type { ILobbySettings, ISettings } from '../common/ISettings';

/**
 * Records what the voice decision was asked and what it answered.
 *
 * `calculateVoiceAudio` cannot be read back after the fact: it returns a gain and writes
 * everything else onto live Web Audio nodes, so the answer is spread across a graph. This
 * captures both halves — the inputs, and the node state the call left behind.
 *
 * Most outputs are read back off the nodes rather than captured inside the decision's own
 * branches: it leaves the decision untouched, and it records what actually reached the
 * graph rather than what the code meant to put there.
 *
 * The pan position is the exception, and it has to be. `setValueAtTime` *schedules* a
 * value — `positionX.value` keeps returning the previous one until the audio thread
 * reaches the next render quantum — so reading it back records the frame before. It is
 * noted where it is applied instead, and left null when the decision returned before
 * reaching that line, which is the honest answer for the three early returns: they leave
 * the panner alone, and a leftover position is not something the call decided.
 *
 * # Turning it on
 *
 * The same `ACL_RECORD` that drives the memory recorder. The file lands beside it, as
 * `<name>.voice.ndjson`.
 */

/** Everything the decision reads. */
interface VoiceInputs {
	gameState: number;
	map: number;
	closedDoors: number[];
	comsSabotaged: boolean;
	currentCamera: number;
	lightRadiusChanged: boolean;
	/** The hearing range, already derived from the lobby settings and the light radius. */
	maxDistance: number;
	/** The client id the impostor radio is tuned to, or null. */
	impostorRadio: number | null;
	ghostVolumeAsImpostor: number;
	enableSpatialAudio: boolean;
	lobby: Record<string, boolean | number>;
	me: RecordedPlayer;
	other: RecordedPlayer;
}

/** One player, reduced to what the decision reads. */
interface RecordedPlayer {
	clientId: number;
	x: number;
	y: number;
	isDead: boolean;
	isImpostor: boolean;
	inVent: boolean;
	disconnected: boolean;
	isDummy: boolean;
}

/** What the call left behind. */
interface VoiceOutputs {
	gain: number;
	/** Null when the decision returned before placing the peer, which is not an answer. */
	panX: number | null;
	panY: number | null;
	/** The muffle's settings if it is in the path, or null if it is not. */
	muffle: { type: string; frequency: number; q: number } | null;
	reverb: boolean;
}

let stream: WriteStream | undefined;
let recording = false;

/** Whether tuples are being recorded. Checked once per peer per frame. */
export function isVoiceRecording(): boolean {
	return recording;
}

/**
 * Starts recording to `<userData>/recordings/<name>.voice.ndjson`.
 *
 * Never throws: a recorder that takes the app down with it is worse than one that does
 * not record.
 */
export function startVoiceRecording(userData: string, name: string): void {
	try {
		const directory = join(userData, 'recordings');
		mkdirSync(directory, { recursive: true });
		const path = join(directory, `${name.replace(/[^\w.-]/g, '_')}.voice.ndjson`);
		stream = createWriteStream(path, { flags: 'a' });
		recording = true;
		console.log('Recording voice decisions to', path);
	} catch (error) {
		console.error('Could not start recording voice decisions:', error);
		recording = false;
	}
}

/** Stops recording and closes the file, once it has flushed. */
export function stopVoiceRecording(): Promise<void> {
	recording = false;
	const closing = stream;
	stream = undefined;
	if (!closing) return Promise.resolve();
	return new Promise((resolve) => {
		closing.end(() => resolve());
		closing.on('error', () => resolve());
	});
}

function reduce(player: Player): RecordedPlayer {
	return {
		clientId: player.clientId,
		x: player.x,
		y: player.y,
		isDead: player.isDead,
		isImpostor: player.isImpostor,
		inVent: player.inVent,
		disconnected: player.disconnected,
		isDummy: player.isDummy,
	};
}

/**
 * Notes one decision.
 *
 * Deduplicated on the inputs: a lobby of ten standing still produces the same tuple ninety
 * times a second, and a corpus of those measures one case very thoroughly. What is wanted
 * is every *distinct* case the session reached.
 */
const seen = new Set<string>();

/** How many distinct tuples to keep before giving up on new ones. */
const MAX_TUPLES = 20000;

export function noteVoice(
	state: AmongUsState,
	settings: ISettings,
	lobby: ILobbySettings,
	me: Player,
	other: Player,
	maxDistance: number,
	impostorRadio: number | undefined,
	outputs: VoiceOutputs
): void {
	if (!recording || seen.size >= MAX_TUPLES) return;

	const inputs: VoiceInputs = {
		gameState: state.gameState,
		map: state.map,
		closedDoors: state.closedDoors ?? [],
		comsSabotaged: state.comsSabotaged,
		currentCamera: state.currentCamera,
		lightRadiusChanged: state.lightRadiusChanged,
		maxDistance,
		impostorRadio: impostorRadio ?? null,
		ghostVolumeAsImpostor: settings.ghostVolumeAsImpostor,
		enableSpatialAudio: settings.enableSpatialAudio,
		lobby: {
			haunting: lobby.haunting,
			hearImpostorsInVents: lobby.hearImpostorsInVents,
			impostersHearImpostersInvent: lobby.impostersHearImpostersInvent,
			impostorRadioEnabled: lobby.impostorRadioEnabled,
			commsSabotage: lobby.commsSabotage,
			deadOnly: lobby.deadOnly,
			meetingGhostOnly: lobby.meetingGhostOnly,
			hearThroughCameras: lobby.hearThroughCameras,
			wallsBlockAudio: lobby.wallsBlockAudio,
			visionHearing: lobby.visionHearing,
			maxDistance: lobby.maxDistance,
		},
		me: reduce(me),
		other: reduce(other),
	};

	const key = JSON.stringify(inputs);
	if (seen.has(key)) return;
	seen.add(key);

	try {
		stream?.write(`${JSON.stringify({ inputs, outputs })}\n`);
	} catch (error) {
		console.error('Could not write a voice decision:', error);
	}
}
