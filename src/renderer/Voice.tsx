import type React from 'react';
import { useContext, useEffect, useMemo, useRef, useState } from 'react';
import { io, type Socket } from 'socket.io-client';
import Avatar from './Avatar';
import { GameStateContext, HostSettingsContext, PlayerColorContext, SettingsContext } from './contexts';
import {
	type AmongUsState,
	GameState,
	type Player,
	type SocketClientMap,
	type AudioConnected,
	type ClientBoolMap,
	type numberStringMap,
	type Client,
	type VoiceState,
} from '../common/AmongUsState';
import Peer, { type SignalData } from './peer';
import {
	RECONNECT_RELAY_AFTER,
	initiatesReconnect,
	reconnectDelay,
	shouldForceRelay,
	shouldGiveUp,
} from './reconnectPolicy';
import { ipcRenderer } from 'electron';
import VAD from './vad';
import { isVoiceRecording, noteVoice, startVoiceRecording } from './voiceRecorder';
import type { ISettings, playerConfigMap, ILobbySettings } from '../common/ISettings';
import { IpcRendererMessages, IpcMessages, IpcOverlayMessages, IpcHandlerMessages } from '../common/ipc-messages';
import Typography from '@mui/material/Typography';
import Grid from '@mui/material/Grid';
import { makeStyles } from 'tss-react/mui';
import SupportLink from './SupportLink';
import Divider from '@mui/material/Divider';
import { validateClientPeerConfig } from './validateClientPeerConfig';
import reverbOgxUrl from '../../static/sounds/reverb.ogx?url';
import radioOnSound from '../../static/sounds/radio_on.wav?url';

import { CameraLocation, AmongUsMaps, MapType } from '../common/AmongusMap';
import type { ObsVoiceState } from '../common/ObsOverlay';
import Button from '@mui/material/Button';
import IconButton from '@mui/material/IconButton';
import VolumeOff from '@mui/icons-material/VolumeOff';
import VolumeUp from '@mui/icons-material/VolumeUp';
import Mic from '@mui/icons-material/Mic';
import MicOff from '@mui/icons-material/MicOff';
import adapter from 'webrtc-adapter';
import type { VADOptions } from './vad';
import { pushToTalkOptions } from './settings/SettingsStore';
import { poseCollide } from '../common/ColliderMap';
import { hasRelay, isRelayUrl, withTcpRelays } from './iceServers';

console.log(adapter.browserDetails.browser);

export interface ExtendedAudioElement extends HTMLAudioElement {
	setSinkId: (sinkId: string) => Promise<void>;
}

interface PeerConnections {
	[peer: string]: Peer;
}

interface VadNode {
	connect: () => void;
	destroy: () => void;
	options: VADOptions;
	init: () => void;
}

interface AudioNodes {
	dummyAudioElement: HTMLAudioElement;
	audioElement: HTMLAudioElement;
	gain: GainNode;
	pan: PannerNode;
	reverb: ConvolverNode;
	muffle: BiquadFilterNode;
	destination: AudioNode;
	reverbConnected: boolean;
	muffleConnected: boolean;
}

interface AudioElements {
	[peer: string]: AudioNodes;
}

interface ConnectionStuff {
	socket?: Socket;
	overlaySocket?: Socket;
	stream?: MediaStream;
	instream?: MediaStream;

	microphoneGain?: GainNode;
	audioListener?: VadNode;
	pushToTalkMode: number;
	deafened: boolean;
	muted: boolean;
	impostorRadio: boolean | null;
	toggleMute: () => void;
	toggleDeafen: () => void;
}

interface SocketError {
	message?: string;
}

// biome-ignore lint/suspicious/noExplicitAny: mirrors Electron's own listener signature
type IpcListener = (event: Electron.IpcRendererEvent, ...args: any[]) => void;

// The Avatar `size` prop drives border width and the hat offsets, which are absolute
// pixels. The rendered size comes from the container, so with a resizable window the
// two drift apart unless the real width is measured.
function useElementWidth<T extends HTMLElement>(): [React.RefObject<T | null>, number] {
	const ref = useRef<T>(null);
	const [width, setWidth] = useState(0);
	useEffect(() => {
		const element = ref.current;
		if (!element) return;
		const observer = new ResizeObserver((entries) => {
			setWidth(entries[0].contentRect.width);
		});
		observer.observe(element);
		return () => observer.disconnect();
	}, []);
	return [ref, width];
}

interface ClientPeerConfig {
	forceRelayOnly: boolean;
	iceServers: RTCIceServer[];
}

const DEFAULT_ICE_CONFIG: RTCConfiguration = {
	iceTransportPolicy: 'all',
	iceServers: [
		{
			urls: 'stun:stun.l.google.com:19302',
		},
	],
};

// natFix forces every connection through a relay. It used to swap in a hardcoded
// turn.bettercrewl.ink with baked-in credentials, which ignored the ICE servers the
// connected server had just sent and routed audio through unrelated infrastructure.
// The server already advertises its own relay, so only the policy needs to change.
function forceRelay(config: RTCConfiguration): RTCConfiguration {
	return { ...config, iceTransportPolicy: 'relay' };
}

export interface VoiceProps {
	t: (key: string) => string;
	error: string;
}

const useStyles = makeStyles()((theme) => ({
	error: {
		position: 'absolute',
		top: '50%',
		transform: 'translateY(-50%)',
	},
	root: {
		paddingTop: theme.spacing(3),
		// Flex column so the avatar grid takes the slack and the footer stays pinned to
		// the bottom at any window size, instead of everything being sized in fixed px.
		height: '100vh',
		boxSizing: 'border-box',
		display: 'flex',
		flexDirection: 'column',
	},
	top: {
		display: 'flex',
		justifyContent: 'center',
		alignItems: 'center',
	},
	right: {
		display: 'flex',
		flexDirection: 'column',
		alignItems: 'center',
		justifyContent: 'center',
	},
	username: {
		display: 'block',
		textAlign: 'center',
		fontSize: 20,
		whiteSpace: 'nowrap',
		maxWidth: '100%',
		overflow: 'hidden',
		textOverflow: 'ellipsis',
	},
	code: {
		fontFamily: "'Source Code Pro', monospace",
		display: 'block',
		width: 'fit-content',
		margin: '5px auto',
		padding: 5,
		borderRadius: 5,
		fontSize: 28,
	},
	otherplayers: {
		width: '100%',
		flex: '1 1 auto',
		minHeight: 0,
		overflowY: 'auto',
		overflowX: 'hidden',
		margin: '4px auto',
		alignContent: 'flex-start',
	},
	avatarWrapper: {
		width: '32%',
		minWidth: 60,
		maxWidth: 120,
		padding: theme.spacing(1),
	},
	muteButtons: {
		paddingLeft: '5px',
		paddingTop: '26px',
		float: 'right',
		display: 'grid',
	},
	left: { float: 'left' },
}));

const defaultlocalLobbySettings: ILobbySettings = {
	maxDistance: 5.32,
	haunting: false,
	hearImpostorsInVents: false,
	impostersHearImpostersInvent: false,
	impostorRadioEnabled: false,
	commsSabotage: false,
	deadOnly: false,
	hearThroughCameras: false,
	wallsBlockAudio: false,
	meetingGhostOnly: false,
	visionHearing: false,
	publicLobby_on: false,
	publicLobby_title: '',
	publicLobby_language: 'en',
};
const radioOnAudio = new Audio();
radioOnAudio.src = radioOnSound;
radioOnAudio.volume = 0.02;

// const radiobeepAudio2 = new Audio();
// radiobeepAudio2.src = radioBeep2;
// radiobeepAudio2.volume = 0.2;

const Voice: React.FC<VoiceProps> = ({ t, error: initialError }: VoiceProps) => {
	const [error, setError] = useState('');
	const [settings, setSetting] = useContext(SettingsContext);

	const settingsRef = useRef<ISettings>(settings);
	const [lobbySettings, setHostLobbySettings] = useContext(HostSettingsContext);
	const lobbySettingsRef = useRef(lobbySettings);
	const maxDistanceRef = useRef(2);
	const gameState = useContext(GameStateContext);
	const playerColors = useContext(PlayerColorContext);

	const hostRef = useRef({
		map: MapType.UNKNOWN,
		mobileRunning: false,
		gamestate: gameState.gameState,
		code: gameState.lobbyCode,
		hostId: gameState.hostId,
		parsedHostId: gameState.hostId,
		isHost: gameState.isHost,
		serverHostId: 0,
	});
	let { lobbyCode: displayedLobbyCode } = gameState;
	if (displayedLobbyCode !== 'MENU' && settings.hideCode) displayedLobbyCode = 'LOBBY';
	const [talking, setTalking] = useState(false);
	const [socketClients, setSocketClients] = useState<SocketClientMap>({});
	const [playerConfigs] = useState<playerConfigMap>(settingsRef.current.playerConfigMap);
	const socketClientsRef = useRef(socketClients);
	const [peerConnections, setPeerConnections] = useState<PeerConnections>({});
	// Last reason a player was inaudible, per client id, so the log records the change
	// rather than the same line on every pass. See describeSilence.
	const silenceReason = useRef<Record<number, string>>({});
	// Pending rebuild of a peer connection that failed, and how many times it has been
	// Peers whose direct path has failed often enough to be worth relaying instead. Kept
	// per socket id because the answering end builds its connection from an incoming offer
	// and has to agree about the policy.
	const relayedPeers = useRef<Record<string, boolean>>({});
	// tried, per socket id. See scheduleReconnect.
	const reconnectTimers = useRef<Record<string, ReturnType<typeof setTimeout>>>({});
	const reconnectAttempts = useRef<Record<string, number>>({});
	const convolverBuffer = useRef<AudioBuffer | null>(null);
	const playerSocketIdsRef = useRef<numberStringMap>({});
	const { classes } = useStyles();
	const [ownAvatarRef, ownAvatarWidth] = useElementWidth<HTMLDivElement>();
	const [otherAvatarsRef, otherAvatarsWidth] = useElementWidth<HTMLDivElement>();

	const [connect, setConnect] = useState<{
		connect: (lobbyCode: string, playerId: number, clientId: number, isHost: boolean) => void;
	} | null>(null);
	const [otherTalking, setOtherTalking] = useState<ClientBoolMap>({});
	const [otherVAD, setOtherVAD] = useState<ClientBoolMap>({});

	const [otherDead, setOtherDead] = useState<ClientBoolMap>({});
	const impostorRadioClientId = useRef<number>(-1);

	const audioElements = useRef<AudioElements>({});
	const [audioConnected, setAudioConnected] = useState<AudioConnected>({});

	const [deafenedState, setDeafened] = useState(false);
	const [mutedState, setMuted] = useState(false);
	const [connected, setConnected] = useState(false);

	function applyEffect(gain: AudioNode, effectNode: AudioNode, destination: AudioNode, player: Player) {
		// A convolver with no impulse response outputs silence rather than passing audio
		// through, so routing voice into one that has not loaded yet makes the player
		// inaudible. That is what the impostors hear ghosts setting did whenever the
		// reverb had not finished decoding before the connection came up.
		if (effectNode instanceof ConvolverNode && !effectNode.buffer) {
			console.warn('Skipping the reverb for', player.name, ': its impulse response is not loaded');
			return false;
		}
		try {
			// Connect the effect before dropping the direct path: doing it the other way
			// round leaves the player silent if the second step throws.
			gain.connect(effectNode);
			effectNode.connect(destination);
			gain.disconnect(destination);
			return true;
		} catch {
			console.log('error with applying effect: ', player.name, effectNode);
			try {
				gain.connect(destination);
			} catch {
				/* already connected */
			}
			return false;
		}
	}

	function restoreEffect(gain: AudioNode, effectNode: AudioNode, destination: AudioNode, player: Player) {
		console.log('restore effect->', effectNode);
		try {
			effectNode.disconnect(destination);
			gain.disconnect(effectNode);
			gain.connect(destination);
		} catch {
			console.log('error with applying effect: ', player.name, effectNode);
		}
	}
	/**
	 * The pan position the last decision applied, or null if it returned before doing so.
	 *
	 * Written by the decision at the point it reaches the panner, because that value
	 * cannot be recovered afterwards: see the note there.
	 */
	let appliedPan: [number, number] | null = null;

	/**
	 * The voice decision, with its answer written down when recording.
	 *
	 * Gate G2's second criterion compares the Rust `voice_params` against this on every
	 * recorded tuple, and the comparison needs both halves. The answer is not the return
	 * value alone: pan position, the muffle's settings and whether reverb is in the path
	 * are all written onto live nodes. They are read back off those nodes here rather
	 * than captured inside the decision, which leaves the decision untouched and records
	 * what actually reached the graph rather than what the code meant to put there.
	 */
	function calculateVoiceAudio(
		state: AmongUsState,
		settings: ISettings,
		me: Player,
		other: Player,
		audio: AudioNodes
	): number {
		appliedPan = null;
		const gain = calculateVoiceAudioInner(state, settings, me, other, audio);
		if (isVoiceRecording()) {
			noteVoice(state, settings, lobbySettings, me, other, maxDistanceRef.current, impostorRadioClientId.current, {
				gain,
				panX: appliedPan ? appliedPan[0] : null,
				panY: appliedPan ? appliedPan[1] : null,
				muffle: audio.muffleConnected
					? {
							type: audio.muffle.type,
							frequency: audio.muffle.frequency.value,
							q: audio.muffle.Q.value,
						}
					: null,
				reverb: audio.reverbConnected === true,
			});
		}
		return gain;
	}

	function calculateVoiceAudioInner(
		state: AmongUsState,
		settings: ISettings,
		me: Player,
		other: Player,
		audio: AudioNodes
	): number {
		const { pan, gain, muffle, reverb, destination } = audio;
		const audioContext = pan.context;
		const useLightSource = true;
		const maxdistance = maxDistanceRef.current;
		let panPos = [other.x - me.x, other.y - me.y];
		let endGain: number;
		let collided = false;
		let skipDistanceCheck = false;
		let muffleEnabled = false;

		if (other.disconnected || other.isDummy) {
			return 0;
		}

		switch (state.gameState) {
			case GameState.MENU:
				return 0;
			case GameState.LOBBY:
				endGain = 1;
				break;

			case GameState.TASKS:
				endGain = 1;

				if (lobbySettings.meetingGhostOnly) {
					endGain = 0;
				}
				if (!me.isDead && lobbySettings.commsSabotage && state.comsSabotaged && !me.isImpostor) {
					endGain = 0;
				}

				// Mute other players which are in a vent
				if (
					other.inVent &&
					!(lobbySettings.hearImpostorsInVents || (lobbySettings.impostersHearImpostersInvent && me.inVent))
				) {
					endGain = 0;
				}
				if (
					lobbySettings.wallsBlockAudio &&
					!me.isDead &&
					poseCollide({ x: me.x, y: me.y }, { x: other.x, y: other.y }, gameState.map, gameState.closedDoors)
				) {
					collided = true;
				}
				if (
					me.isImpostor &&
					other.isImpostor &&
					lobbySettings.impostorRadioEnabled &&
					other.clientId === impostorRadioClientId.current
				) {
					skipDistanceCheck = true;
					muffle.type = 'highpass';
					muffle.frequency.value = 1000;
					muffle.Q.value = 10;
					muffleEnabled = true;
					if (!audio.muffleConnected) {
						audio.muffleConnected = applyEffect(gain, muffle, destination, other);
					}
				}

				if (!me.isDead && other.isDead && me.isImpostor && lobbySettings.haunting) {
					// Only record it as connected if it is: otherwise the restore below would
					// disconnect a path that was never established.
					if (!audio.reverbConnected) {
						audio.reverbConnected = applyEffect(gain, reverb, destination, other);
					}
					collided = false;
					endGain = settings.ghostVolumeAsImpostor / 100;
				} else {
					if (other.isDead && !me.isDead) {
						endGain = 0;
					}
				}
				break;
			case GameState.DISCUSSION:
				panPos = [0, 0];
				endGain = 1;
				if (!me.isDead && other.isDead) {
					endGain = 0;
				}
				break;

			default:
				endGain = 0;
				break;
		}

		if (useLightSource && state.lightRadiusChanged) {
			pan.maxDistance = maxDistanceRef.current;
		}

		if (!other.isDead || state.gameState !== GameState.TASKS || !me.isImpostor || me.isDead) {
			if (audio.reverbConnected && reverb) {
				audio.reverbConnected = false;
				restoreEffect(gain, reverb, destination, other);
			}
		}

		if (lobbySettings.deadOnly) {
			panPos = [0, 0];
			if (!me.isDead || !other.isDead) {
				endGain = 0;
			}
		}

		let isOnCamera = state.currentCamera !== CameraLocation.NONE;
		if (!skipDistanceCheck && Math.sqrt(panPos[0] * panPos[0] + panPos[1] * panPos[1]) > maxdistance) {
			if (lobbySettings.hearThroughCameras && state.gameState === GameState.TASKS) {
				// Reachable for the first time now that the map is read: while it was
				// undefined the camera was always NONE, so neither branch ever ran. A map
				// with no camera data, or a camera id the map does not list, would throw
				// out of here and take the whole audio pass with it.
				const cameras = AmongUsMaps[state.map]?.cameras ?? {};
				if (state.currentCamera !== CameraLocation.NONE && state.currentCamera !== CameraLocation.Skeld) {
					const camerapos = cameras[state.currentCamera];
					if (camerapos) {
						panPos = [other.x - camerapos.x, other.y - camerapos.y];
					}
				} else if (state.currentCamera === CameraLocation.Skeld) {
					let distance = 999;
					let camerapos = { x: 999, y: 999 };
					for (const camera of Object.values(cameras)) {
						const cameraDist = Math.sqrt((other.x - camera.x) ** 2 + (other.y - camera.y) ** 2);
						if (distance > cameraDist) {
							distance = cameraDist;
							camerapos = camera;
						}
					}
					if (distance != 999) {
						panPos = [other.x - camerapos.x, other.y - camerapos.y];
					}
				}

				if (Math.sqrt(panPos[0] * panPos[0] + panPos[1] * panPos[1]) > maxdistance) {
					return 0;
				}
			} else {
				return 0;
			}
		} else {
			if (collided && !skipDistanceCheck) {
				return 0;
			}
			isOnCamera = false;
		}

		// Muffling in vents
		if (
			((me.inVent && !me.isDead) || (other.inVent && !other.isDead) || isOnCamera) &&
			state.gameState === GameState.TASKS
		) {
			if (!audio.muffleConnected) {
				audio.muffleConnected = applyEffect(gain, muffle, destination, other);
			}
			// The two distance checks that read maxdistance both run earlier, so writing it
			// here had no effect. Left out rather than moved: shortening the hearing range
			// for cameras is an audible change that needs testing in game to get right.
			muffle.frequency.value = isOnCamera ? 2300 : 2000;
			muffle.Q.value = isOnCamera ? -15 : 20;
			if (endGain === 1) endGain = isOnCamera ? 0.8 : 0.5; // Too loud at 1
		} else {
			if (audio.muffleConnected && !muffleEnabled) {
				audio.muffleConnected = false;
				restoreEffect(gain, muffle, destination, other);
			}
		}

		if (!settings.enableSpatialAudio || skipDistanceCheck) {
			panPos = [0, 0];
		}

		// Noted here rather than read back afterwards. `setValueAtTime` *schedules* a
		// value: `positionX.value` keeps returning the previous one until the audio thread
		// reaches the next render quantum, so reading it after the call records the frame
		// before. Being null when this line is not reached is the other half of the
		// signal — the three early returns leave the panner alone, and a leftover position
		// is not an answer.
		appliedPan = [panPos[0], panPos[1]];
		pan.positionX.setValueAtTime(panPos[0], audioContext.currentTime);
		pan.positionY.setValueAtTime(panPos[1], audioContext.currentTime);
		pan.positionZ.setValueAtTime(-0.5, audioContext.currentTime);
		return endGain;
	}

	function notifyMobilePlayers() {
		if (
			settingsRef.current.mobileHost &&
			hostRef.current.gamestate !== GameState.MENU &&
			hostRef.current.gamestate !== GameState.UNKNOWN
		) {
			connectionStuff.current.socket?.emit('signal', {
				to: `${hostRef.current.code}_mobile`,
				data: { mobileHostInfo: { isHostingMobile: true, isGameHost: hostRef.current.isHost } },
			});
		}
		setTimeout(() => notifyMobilePlayers(), 5000);
	}

	function disconnectAudioHtmlElement(element: HTMLAudioElement) {
		console.log('disableing element?', element);
		element.pause();
		if (element.srcObject) {
			const mediaStream = element.srcObject as MediaStream;
			mediaStream.getTracks().forEach((track) => {
				track.stop();
			});
		}
		element.removeAttribute('srcObject');
		element.removeAttribute('src');
		element.srcObject = null;
		element.load();
		element.remove();
	}
	function disconnectAudioElement(peer: string) {
		if (audioElements.current[peer]) {
			console.log('removing element..');
			disconnectAudioHtmlElement(audioElements.current[peer].audioElement);
			disconnectAudioHtmlElement(audioElements.current[peer].dummyAudioElement);
			audioElements.current[peer].pan.disconnect();
			audioElements.current[peer].gain.disconnect();
			// if (audioElements.current[peer].reverbGain != null) audioElements.current[peer].reverbGain?.disconnect();
			if (audioElements.current[peer].reverb != null) audioElements.current[peer].reverb?.disconnect();
			delete audioElements.current[peer];
		}
	}

	function disconnectClient(client: Client) {
		if (!client?.clientId) return;
		const oldSocketId = playerSocketIdsRef.current[client.clientId];
		console.log('Checking for  old connection ....', client.clientId, oldSocketId);
		if (oldSocketId && audioElements.current[oldSocketId]) {
			console.log('found old connection disconnecting....', client.clientId);
			disconnectAudioElement(oldSocketId);
		}
	}

	function cancelReconnect(peer: string) {
		const timer = reconnectTimers.current[peer];
		if (timer !== undefined) {
			clearTimeout(timer);
			delete reconnectTimers.current[peer];
		}
		delete reconnectAttempts.current[peer];
		// The relay choice goes with the peer, not with the attempt counter: it is cleared
		// here because this runs when the peer has left, and kept across a successful
		// reconnect because the relay is what made that connection work.
		delete relayedPeers.current[peer];
	}

	function cancelAllReconnects() {
		for (const peer of Object.keys(reconnectTimers.current)) {
			clearTimeout(reconnectTimers.current[peer]);
		}
		reconnectTimers.current = {};
		reconnectAttempts.current = {};
		// A new lobby is a new set of paths between people, so nothing is assumed about it.
		relayedPeers.current = {};
	}

	function disconnectPeer(peer: string) {
		console.log('Disconnect peer: ', peer);
		const connection = peerConnections[peer];
		if (!connection) {
			return;
		}
		// Drop it from the map before destroying it. destroy() emits close synchronously,
		// and that handler treats a connection still listed here as one that broke on its
		// own and schedules a rebuild, which is wrong for a teardown we asked for.
		setPeerConnections((connections) => {
			delete connections[peer];
			return connections;
		});
		connection.destroy();
		disconnectAudioElement(peer);
		// Set when a stream arrives and never taken back, so a player whose connection
		// died still counted as having voice: the avatar kept showing them as connected
		// instead of falling back to the no-voice marker.
		setAudioConnected((old) => {
			if (!(peer in old)) return old;
			const { [peer]: _gone, ...rest } = old;
			return rest;
		});
	}
	// Handle pushToTalk, if set
	useEffect(() => {
		if (!connectionStuff.current.instream) return;
		connectionStuff.current.instream.getAudioTracks()[0].enabled =
			!connectionStuff.current.deafened &&
			!connectionStuff.current.muted &&
			settings.pushToTalkMode !== pushToTalkOptions.PUSH_TO_TALK;
		connectionStuff.current.pushToTalkMode = settings.pushToTalkMode;
	}, [settings.pushToTalkMode]);

	// Emit lobby settings to connected peers
	useEffect(() => {
		if (hostRef.current.isHost !== true) return;
		Object.values(peerConnections).forEach((peer) => {
			try {
				console.log('sendxx > ', JSON.stringify(settings.localLobbySettings));
				peer.send(JSON.stringify(settings.localLobbySettings));
			} catch (e) {
				console.warn('failed to update lobby settings: ', e);
			}
		});

		setHostLobbySettings(settings.localLobbySettings);
	}, [settings.localLobbySettings, hostRef.current.isHost]);

	useEffect(() => {
		for (const peer in audioElements.current) {
			audioElements.current[peer].pan.maxDistance = maxDistanceRef.current;
		}
	}, [lobbySettings.maxDistance, lobbySettings.visionHearing]);

	useEffect(() => {
		// Hosting for mobile players is opt-in, and the opt-in is this setting. The
		// broadcast below used to depend only on mobileRunning, which any lobby member can
		// switch on remotely, and it carries the whole game state: every player's position,
		// impostor flag and vent state, to a room name anyone can join. That made a
		// six-character lobby code enough to watch the entire lobby through the wall.
		const hostingMobile = hostRef.current.mobileRunning && settings.mobileHost;
		if (!gameState?.players || !connectionStuff.current.socket || (!hostingMobile && !settings.obsOverlay)) {
			return;
		}
		if (hostingMobile) {
			connectionStuff.current.socket?.emit('signal', {
				to: `${gameState.lobbyCode}_mobile`,
				data: { gameState, lobbySettings },
			});
		}

		if (
			settings.obsOverlay &&
			settings.obsSecret &&
			// A minimum, not an equality: secrets generated from now on are longer.
			settings.obsSecret.length >= 9 &&
			((gameState.gameState !== GameState.UNKNOWN && gameState.gameState !== GameState.MENU) ||
				gameState.oldGameState !== gameState.gameState)
		) {
			connectionStuff.current.overlaySocket = connectionStuff.current.socket;

			const obsvoiceState: ObsVoiceState = {
				overlayState: {
					gameState: gameState.gameState,
					players: gameState.players.map((o) => ({
						id: o.id,
						clientId: o.clientId,
						inVent: o.inVent,
						isDead: o.isDead,
						name: o.name,
						colorId: o.colorId,
						hatId: o.hatId,
						petId: o.petId,
						skinId: o.skinId,
						visorId: o.visorId,
						disconnected: o.disconnected,
						isLocal: o.isLocal,
						shiftedColor: o.shiftedColor,
						bugged: o.bugged,
						realColor: playerColors[o.colorId],
						usingRadio: o.clientId === impostorRadioClientId.current && myPlayer?.isImpostor,
						connected:
							(playerSocketIdsRef.current[o.clientId] &&
								socketClients[playerSocketIdsRef.current[o.clientId]]?.clientId === o.clientId) ||
							false,
					})),
				},
				otherTalking,
				otherDead,
				localTalking: talking,
				localIsAlive: !myPlayer?.isDead,
				mod: gameState.mod,
				oldMeetingHud: gameState.oldMeetingHud,
			};
			connectionStuff.current.overlaySocket?.emit('signal', {
				to: settings.obsSecret,
				data: obsvoiceState,
			});
		}
	}, [gameState]);

	// Add settings to settingsRef
	useEffect(() => {
		settingsRef.current = settings;
	}, [settings]);

	// Add socketClients to socketClientsRef
	useEffect(() => {
		socketClientsRef.current = socketClients;
	}, [socketClients]);

	// Writes the ref as well as the state. The ref used to be updated only by the effect
	// above, so a signal arriving in the same tick as the join was measured against a
	// stale map, hit the "unknown socket" guard and was dropped for good. That left
	// exactly one pair of players unable to hear each other while everyone else was fine.
	const updateSocketClients = (update: (old: SocketClientMap) => SocketClientMap) => {
		socketClientsRef.current = update(socketClientsRef.current);
		setSocketClients(socketClientsRef.current);
	};

	// Turned on by the same ACL_RECORD that drives the memory recorder, so gate G1's
	// frames and gate G2's tuples are captured from the same session and cannot be out of
	// step with each other.
	useEffect(() => {
		ipcRenderer
			.invoke(IpcHandlerMessages.REQUEST_VOICE_RECORDING)
			.then((where: { userData: string; name: string } | null) => {
				if (where) startVoiceRecording(where.userData, where.name);
			})
			.catch((error: unknown) => console.warn('Could not start recording voice decisions:', error));
	}, []);

	useEffect(() => {
		if (
			connectionStuff.current?.microphoneGain?.gain &&
			(settingsRef.current.microphoneGainEnabled || settingsRef.current.micSensitivityEnabled)
		) {
			if (!settingsRef.current.micSensitivityEnabled)
				connectionStuff.current.microphoneGain.gain.value = settings.microphoneGainEnabled
					? settings.microphoneGain / 100
					: 1;

			if (connectionStuff.current?.audioListener?.options) {
				connectionStuff.current.audioListener.options.minNoiseLevel = settings.micSensitivity;
				connectionStuff.current.audioListener.init();
			}
		}
	}, [
		settings.microphoneGain,
		settings.micSensitivity,
		settings.microphoneGainEnabled,
		settings.micSensitivityEnabled,
	]);

	const updateLobby = () => {
		console.log(gameState);
		if (
			!gameState ||
			!hostRef.current.isHost ||
			!gameState.lobbyCode ||
			gameState.gameState === GameState.MENU ||
			!gameState.players
		) {
			return;
		}
		connectionStuff.current.socket?.emit('lobby', gameState.lobbyCode, {
			id: -1,
			title: lobbySettings.publicLobby_title,
			host: myPlayer?.name,
			current_players: gameState.players.length,
			max_players: gameState.maxPlayers,
			server: gameState.currentServer,
			language: lobbySettings.publicLobby_language,
			mods: gameState.mod,
			isPublic: lobbySettings.publicLobby_on,
			gameState: gameState.gameState,
		});
	};

	useEffect(() => {
		if (gameState.isHost && gameState.hostId > 0) {
			connectionStuff.current.socket?.emit('setHost', gameState.lobbyCode, gameState.clientId);
			hostRef.current.serverHostId = gameState.hostId;
		}
	}, [gameState.isHost]);

	useEffect(() => {
		updateLobby();
	}, [
		gameState.gameState,
		gameState?.players?.length,
		lobbySettings.publicLobby_title,
		lobbySettings.publicLobby_language,
		lobbySettings.publicLobby_on,
	]);

	// Add lobbySettings to lobbySettingsRef
	useEffect(() => {
		lobbySettingsRef.current = lobbySettings;
	}, [lobbySettings]);

	// Set dead player data
	useEffect(() => {
		if (gameState.gameState === GameState.LOBBY) {
			setOtherDead({});
		} else if (gameState.gameState !== GameState.TASKS) {
			if (!gameState.players) return;
			setOtherDead((old) => {
				for (const player of gameState.players) {
					old[player.clientId] = player.isDead || player.disconnected;
				}
				return { ...old };
			});
		}
	}, [gameState.gameState]);

	// const [audioContext] = useState<AudioContext>(() => new AudioContext());
	const connectionStuff = useRef<ConnectionStuff>({
		pushToTalkMode: settings.pushToTalkMode,
		deafened: false,
		muted: false,
		impostorRadio: null,
		toggleMute: () => {
			/*empty*/
		},
		toggleDeafen: () => {
			/*empty*/
		},
	});

	useEffect(() => {
		(async () => {
			const context = new AudioContext();
			try {
				// Vite has no arraybuffer-loader equivalent, so fetch the emitted asset.
				const reverbData = await (await fetch(reverbOgxUrl)).arrayBuffer();
				convolverBuffer.current = await context.decodeAudioData(reverbData);
				// Connections that came up before the file finished decoding hold a convolver
				// with no impulse response, and it is never assigned again. Nothing reported
				// that, it just meant no reverb for those players.
				for (const element of Object.values(audioElements.current)) {
					if (element.reverb && !element.reverb.buffer) {
						element.reverb.buffer = convolverBuffer.current;
					}
				}
			} catch (error) {
				console.error('Could not load the reverb impulse response:', error);
			} finally {
				await context.close();
			}
		})();
	}, []);

	useEffect(() => {
		const pressing = connectionStuff.current.impostorRadio;
		if (
			pressing == null ||
			!myPlayer ||
			!myPlayer.isImpostor ||
			myPlayer.isDead ||
			!(impostorRadioClientId.current === myPlayer.clientId || impostorRadioClientId.current === -1) ||
			!lobbySettingsRef.current.impostorRadioEnabled
		) {
			return;
		}
		radioOnAudio.play();
		connectionStuff.current.impostorRadio = pressing;
		impostorRadioClientId.current = pressing ? myPlayer.clientId : -1;
		for (const player of otherPlayers.filter((o) => o.isImpostor && !o.bugged && !o.isDead)) {
			const peer = playerSocketIdsRef.current[player.clientId];
			const connection = peerConnections[peer];
			if (connection?.writable)
				connection?.send(JSON.stringify({ impostorRadio: connectionStuff.current.impostorRadio }));
		}
	}, [connectionStuff.current.impostorRadio]);

	useEffect(() => {
		// (async function anyNameFunction() {
		let currentLobby = '';
		// Connect to voice relay server
		connectionStuff.current.socket = io(settings.serverURL, {
			transports: ['websocket'],
		});

		const { socket } = connectionStuff.current;

		socket.on('error', (error: SocketError) => {
			if (error.message) {
				setError(error.message);
			}
			console.error('socketIO error:', error);
			currentLobby = 'MENU';
		});
		socket.on('connect', () => {
			setConnected(true);
			console.log('CONNECTED??');
		});

		socket.on('setHost', (hostId: number) => {
			hostRef.current.serverHostId = hostId;
		});

		socket.on('disconnect', () => {
			setConnected(false);
			currentLobby = 'MENU';
			console.log('DISCONNECTED??');
		});

		notifyMobilePlayers();

		let iceConfig: RTCConfiguration = DEFAULT_ICE_CONFIG;
		socket.on('clientPeerConfig', (clientPeerConfig: ClientPeerConfig) => {
			if (!validateClientPeerConfig(clientPeerConfig)) {
				const errorsFormatted = validateClientPeerConfig.errors.join('\n');
				alert(
					`Server sent a malformed peer config. Default config will be used. See errors below:\n${errorsFormatted}`
				);
				return;
			}

			if (clientPeerConfig.forceRelayOnly && !clientPeerConfig.iceServers.some((server) => isRelayUrl(server.urls))) {
				alert('Server has forced relay mode enabled but provides no relay servers. Default config will be used.');
				return;
			}

			// Logged once, without credentials. Two separate reports of "nobody can hear
			// me" have now been diagnosed by guessing what the server was advertising at
			// the time, because the log did not say. It costs one line.
			const advertised = withTcpRelays(clientPeerConfig.iceServers);
			console.log(
				'ICE servers offered by the server:',
				advertised.flatMap((server) => [].concat(server.urls as never).map(String)).join(', '),
				clientPeerConfig.forceRelayOnly ? '(relay forced)' : ''
			);

			iceConfig = {
				iceTransportPolicy: clientPeerConfig.forceRelayOnly ? 'relay' : 'all',
				iceServers: advertised,
			};
		});

		socket.on('VAD', (data: { activity: boolean; client: Client; socketId: string }) => {
			setOtherVAD((old) => ({
				...old,
				[data.client.clientId]: data.activity,
			}));
		});

		socket.on('setClient', (socketId: string, client: Client) => {
			updateSocketClients((old) => ({ ...old, [socketId]: client }));
		});

		socket.on('setClients', (clients: SocketClientMap) => {
			updateSocketClients(() => clients);
		});

		// Sent when a socket leaves the lobby. Without it a departure is only visible as a
		// peer connection that failed, which is exactly what a broken connection looks
		// like, so the two cases could not be told apart.
		socket.on('left', (peer: string) => {
			console.log('Peer left the lobby:', peer);
			cancelReconnect(peer);
			updateSocketClients((old) => {
				const { [peer]: _gone, ...rest } = old;
				return rest;
			});
			disconnectPeer(peer);
		});

		// Initialize variables
		let audioListener: VadNode;
		// Held so the effect cleanup can tear these down. Voice unmounts every time the
		// game closes, so without this the microphone stream, the AudioContext and the
		// IPC listeners below accumulate one set per game session.
		let localStream: MediaStream | undefined;
		let audioContext: AudioContext | undefined;
		const ipcListeners: [string, IpcListener][] = [];
		const addIpcListener = (channel: string, listener: IpcListener) => {
			ipcListeners.push([channel, listener]);
			ipcRenderer.on(channel, listener);
		};

		// Intersection with an index signature: latency and the goog* keys are
		// non-standard Chrome constraints that lib.dom does not declare.
		const audio: MediaTrackConstraintSet & Record<string, unknown> = {
			deviceId: undefined as unknown as string,
			autoGainControl: false,
			channelCount: 2,
			echoCancellation: settings.echoCancellation,
			latency: 0,
			noiseSuppression: settings.noiseSuppression, // @ts-ignore-line
			googNoiseSuppression: settings.noiseSuppression, // @ts-ignore-line
			googEchoCancellation: settings.echoCancellation, // @ts-ignore-line
			googTypingNoiseDetection: settings.noiseSuppression, // @ts-ignore-line
			sampleRate: settings.oldSampleDebug ? 48000 : undefined,
			sampleSize: settings.oldSampleDebug ? 16 : undefined,
		};

		// Get microphone settings
		if (settingsRef.current.microphone.toLowerCase() !== 'default') audio.deviceId = settingsRef.current.microphone;
		navigator.mediaDevices.getUserMedia({ video: false, audio }).then(
			async (inStream) => {
				let stream = inStream;
				const ac = new AudioContext();
				localStream = inStream;
				audioContext = ac;
				let microphoneGain: GainNode | undefined;
				const source = ac.createMediaStreamSource(inStream);
				if (settings.microphoneGainEnabled || settings.micSensitivityEnabled) {
					console.log('Microphone volume or sensitivityEnabled..');
					stream = (() => {
						microphoneGain = ac.createGain();
						const destination = ac.createMediaStreamDestination();
						source.connect(microphoneGain);
						microphoneGain.gain.value = settings.microphoneGainEnabled ? settings.microphoneGain / 100 : 1;
						microphoneGain.connect(destination);
						connectionStuff.current.microphoneGain = microphoneGain;
						return destination.stream;
					})();
				}

				if (settingsRef.current.vadEnabled) {
					audioListener = VAD(ac, source, undefined, {
						onVoiceStart: () => {
							if (microphoneGain && settingsRef.current.micSensitivityEnabled) {
								microphoneGain.gain.value = settingsRef.current.microphoneGainEnabled
									? settingsRef.current.microphoneGain / 100
									: 1;
							}
							setTalking(true);
						},
						onVoiceStop: () => {
							if (microphoneGain && settingsRef.current.micSensitivityEnabled) {
								microphoneGain.gain.value = 0;
							}
							setTalking(false);
						},
						noiseCaptureDuration: 0,
						stereo: false,
					});

					audioListener.options.minNoiseLevel = settingsRef.current.micSensitivityEnabled
						? settingsRef.current.micSensitivity
						: 0.15;
					audioListener.options.maxNoiseLevel = 1;

					audioListener.init();
					connectionStuff.current.audioListener = audioListener;
					connectionStuff.current.microphoneGain = microphoneGain;
				}
				connectionStuff.current.stream = stream;
				connectionStuff.current.instream = inStream;

				inStream.getAudioTracks()[0].enabled = settings.pushToTalkMode !== pushToTalkOptions.PUSH_TO_TALK;

				connectionStuff.current.toggleDeafen = () => {
					connectionStuff.current.deafened = !connectionStuff.current.deafened;
					inStream.getAudioTracks()[0].enabled =
						!connectionStuff.current.deafened &&
						!connectionStuff.current.muted &&
						connectionStuff.current.pushToTalkMode !== pushToTalkOptions.PUSH_TO_TALK;
					setDeafened(connectionStuff.current.deafened);
				};

				connectionStuff.current.toggleMute = () => {
					connectionStuff.current.muted = !connectionStuff.current.muted;
					if (connectionStuff.current.deafened) {
						connectionStuff.current.deafened = false;
						connectionStuff.current.muted = false;
					}
					inStream.getAudioTracks()[0].enabled =
						!connectionStuff.current.muted &&
						!connectionStuff.current.deafened &&
						connectionStuff.current.pushToTalkMode !== pushToTalkOptions.PUSH_TO_TALK;
					setMuted(connectionStuff.current.muted);
					setDeafened(connectionStuff.current.deafened);
				};

				addIpcListener(IpcRendererMessages.TOGGLE_DEAFEN, connectionStuff.current.toggleDeafen);

				addIpcListener(IpcRendererMessages.IMPOSTOR_RADIO, (_: unknown, pressing: boolean) => {
					connectionStuff.current.impostorRadio = pressing;
				});

				addIpcListener(IpcRendererMessages.TOGGLE_MUTE, connectionStuff.current.toggleMute);
				addIpcListener(IpcRendererMessages.PUSH_TO_TALK, (_: unknown, pressing: boolean) => {
					if (connectionStuff.current.pushToTalkMode === pushToTalkOptions.VOICE) return;
					if (!connectionStuff.current.deafened && !connectionStuff.current.muted) {
						inStream.getAudioTracks()[0].enabled =
							connectionStuff.current.pushToTalkMode === pushToTalkOptions.PUSH_TO_TALK ? pressing : !pressing;
					}
				});

				audioElements.current = {};

				const connect = (lobbyCode: string, playerId: number, clientId: number, isHost: boolean) => {
					console.log('connect called..', lobbyCode);
					setOtherVAD({});
					setOtherTalking({});
					if (lobbyCode === 'MENU') {
						cancelAllReconnects();
						Object.keys(peerConnections).forEach((k) => {
							disconnectPeer(k);
						});
						updateSocketClients(() => ({}));
						currentLobby = lobbyCode;
					} else if (currentLobby !== lobbyCode) {
						console.log('Currentlobby', currentLobby, lobbyCode);
						socket.emit('leave');
						socket.emit('id', playerId, clientId);
						socket.emit('join', lobbyCode, playerId, clientId, isHost);
						currentLobby = lobbyCode;
					}
				};

				setConnect({ connect });

				function createPeerConnection(peer: string, initiator: boolean, client: Client) {
					console.log('CreatePeerConnection: ', peer, initiator);
					disconnectClient(client);
					// Replacing an entry without destroying the previous instance leaked it and
					// left it able to fire close for a peer it no longer owns.
					const previous = peerConnections[peer];
					if (previous) {
						delete peerConnections[peer];
						previous.destroy();
					}
					// Relay when the user asked for it, or when this peer's direct path has
					// already failed twice and there is a relay to fall back to.
					const relay = settingsRef.current.natFix || relayedPeers.current[peer] === true;
					const connection = new Peer({
						stream,
						initiator,
						config: relay ? forceRelay(iceConfig) : iceConfig,
					});
					if (relay) {
						console.log('Connecting to', peer, 'through a relay');
					}

					setPeerConnections((connections) => {
						connections[peer] = connection;
						return connections;
					});

					connection.on('connect', () => {
						// The connection came up, so the next failure starts its backoff from
						// the beginning rather than from wherever the last one left off.
						delete reconnectAttempts.current[peer];
						setTimeout(() => {
							if (hostRef.current.isHost && connection.writable) {
								try {
									console.log('sending settings..');
									connection.send(JSON.stringify(lobbySettingsRef.current));
								} catch (e) {
									console.warn('failed to update lobby settings: ', e);
								}
							}
						}, 1000);
					});

					connection.on('stream', async (stream: MediaStream) => {
						console.log('ONSTREAM');

						setAudioConnected((old) => ({ ...old, [peer]: true }));
						const dummyAudio = new Audio();
						dummyAudio.srcObject = stream;
						const context = new AudioContext();
						const source = context.createMediaStreamSource(stream);
						const dest = context.createMediaStreamDestination();

						const gain = context.createGain();
						const pan = context.createPanner();
						gain.gain.value = 0;
						pan.refDistance = 0.1;
						pan.panningModel = 'equalpower';
						pan.distanceModel = 'linear';
						pan.maxDistance = maxDistanceRef.current;
						pan.rolloffFactor = 1;

						const muffle = context.createBiquadFilter();
						muffle.type = 'lowpass';

						source.connect(pan);
						pan.connect(gain);

						const reverb = context.createConvolver();
						reverb.buffer = convolverBuffer.current;
						const destination: AudioNode = dest;
						// if (settingsRef.current.vadEnabled) {
						// 	VAD(context, gain, undefined, {
						// 		onVoiceStart: () => setTalking(true),
						// 		onVoiceStop: () => setTalking(false),
						// 		stereo: false,
						// 	});
						// }
						gain.connect(destination);
						const audio = document.createElement('audio') as ExtendedAudioElement;
						document.body.appendChild(audio);
						audio.setAttribute('autoplay', '');
						audio.srcObject = dest.stream;
						if (settingsRef.current.speaker.toLowerCase() !== 'default') {
							audio.setSinkId(settingsRef.current.speaker);
						}

						audioElements.current[peer] = {
							dummyAudioElement: dummyAudio,
							audioElement: audio,
							gain,
							pan,
							reverb,
							muffle,
							muffleConnected: false,
							reverbConnected: false,
							destination,
						};
					});

					connection.on('signal', (data) => {
						socket.emit('signal', {
							data,
							to: peer,
						});
					});

					connection.on('data', (data) => {
						const parsedData = JSON.parse(data);
						if (Object.hasOwn(parsedData, 'impostorRadio')) {
							const clientId = socketClientsRef.current[peer]?.clientId;
							if (impostorRadioClientId.current === -1 && parsedData.impostorRadio) {
								impostorRadioClientId.current = clientId;
							} else if (impostorRadioClientId.current === clientId && !parsedData.impostorRadio) {
								impostorRadioClientId.current = -1;
							}
							console.log('Recieved impostor radio request', parsedData);
						}
						if (Object.hasOwn(parsedData, 'maxDistance')) {
							if (!hostRef.current || hostRef.current.parsedHostId !== socketClientsRef.current[peer]?.clientId) return;
							const newSettings = { ...defaultlocalLobbySettings, ...parsedData };
							setHostLobbySettings(newSettings);
						}
					});
					connection.on('close', () => {
						console.log('Disconnected from', peer, 'Initiator:', initiator);
						// Only tear down if this is still the live connection. On offer glare a
						// replacement is created for the same peer, and the old instance closing
						// afterwards used to destroy that replacement, permanently muting the pair.
						if (peerConnections[peer] === connection) {
							disconnectPeer(peer);
							scheduleReconnect(peer);
						}
					});
					connection.on('error', (error: Error) => {
						console.warn('Peer connection error for', peer, error);
					});
					return connection;
				}

				// A connection that fails is gone for the rest of the session: new ones are
				// only created when a socket joins the lobby or sends an offer, and a peer that
				// is already in the lobby does neither. That is why a player who drops out of
				// one conversation stays out of it until the app is restarted, while everyone
				// else still hears them. Rebuild it instead.
				function scheduleReconnect(peer: string) {
					if (reconnectTimers.current[peer] !== undefined) return;
					if (currentLobby === '' || currentLobby === 'MENU') return;
					// The server announces departures, so a peer still in the map is one that is
					// still in the lobby and worth reconnecting to.
					if (!socketClientsRef.current[peer]) return;

					const attempt = (reconnectAttempts.current[peer] ?? 0) + 1;
					if (shouldGiveUp(attempt)) {
						// Loud, and saying which of the two situations this is. Until now the
						// app went quiet here: the avatars still lit up, because voice activity
						// travels over the socket rather than over the audio path, so a player
						// with no audio at all in either direction saw a lobby that looked
						// entirely healthy and had nothing to report but silence.
						console.error(
							`No connection to ${peer} after ${attempt - 1} attempts. Voice with this player will not work.`,
							relayedPeers.current[peer]
								? 'The relay was tried and did not help.'
								: hasRelay(iceConfig)
									? 'A relay was available but did not help.'
									: 'This server advertises no relay, so a restrictive NAT cannot be worked around.'
						);
						return;
					}
					reconnectAttempts.current[peer] = attempt;

					// A direct path that has failed twice will keep failing: what is in the way
					// is the network between the two ends, and waiting longer does not move it.
					if (shouldForceRelay(attempt) && !relayedPeers.current[peer]) {
						if (hasRelay(iceConfig)) {
							console.log('Direct connection to', peer, 'keeps failing; switching to the relay');
							relayedPeers.current[peer] = true;
						} else if (attempt === RECONNECT_RELAY_AFTER) {
							console.warn(
								'Direct connection to',
								peer,
								'keeps failing and this server advertises no relay to fall back to'
							);
						}
					}

					const delay = reconnectDelay(attempt, initiatesReconnect(socket.id ?? '', peer));

					reconnectTimers.current[peer] = setTimeout(() => {
						delete reconnectTimers.current[peer];
						const client = socketClientsRef.current[peer];
						if (!client || currentLobby === '' || currentLobby === 'MENU') return;
						// The other end got there first, or the socket is down and rejoining the
						// lobby will produce a fresh connection anyway.
						if (peerConnections[peer]) return;
						if (!socket.connected) return;
						console.log('Rebuilding the connection to', peer, '- attempt', attempt);
						createPeerConnection(peer, true, client);
					}, delay);
				}

				socket.on('join', async (peer: string, client: Client) => {
					// Register before connecting: the answer can come back before the state
					// update lands, and the signal handler rejects unknown sockets.
					updateSocketClients((old) => ({ ...old, [peer]: client }));
					createPeerConnection(peer, true, client);
				});

				// The server only sends { data, from }; `client` was always undefined here, so
				// disconnectClient never cleaned up the stale audio element on this side.
				socket.on('signal', ({ data, from }: { data: SignalData; from: string }) => {
					const client = socketClientsRef.current[from];
					if (!client) {
						console.warn('SIGNAL FROM UNKOWN SOCKET..');
						return;
					}
					// Below the sender check, not above it: this branch used to run for anyone
					// who could name this socket, whether or not they were in the lobby.
					if (Object.hasOwn(data, 'mobilePlayerInfo')) {
						const mobiledata = data as unknown as mobileHostInfo;
						if (
							settingsRef.current.mobileHost &&
							mobiledata.mobilePlayerInfo.code === hostRef.current.code &&
							hostRef.current.gamestate !== GameState.MENU
						) {
							hostRef.current.mobileRunning = true;
							console.log('setting mobileRunning to true..');
						}
						return;
					}
					let connection: Peer;
					// Only signals carrying a `type` used to be forwarded, so trickled ICE
					// candidates, which have no type, were dropped outright. Connections then
					// depended on whatever candidates happened to be in the initial SDP.
					const existing = peerConnections[from];
					const isOffer = 'type' in data && data.type === 'offer';
					if (isOffer) {
						connection = createPeerConnection(from, false, client);
					} else if (existing) {
						connection = existing;
					} else {
						// An answer or candidate with no connection left to apply it to.
						return;
					}
					void connection.signal(data);
				});
			},
			(error) => {
				console.error(error);
				setError(`Couldn't connect to your microphone:\n${error}`);
				// ipcRenderer.send(IpcMessages.SHOW_ERROR_DIALOG, {
				// 	title: 'Error',
				// 	content: 'Couldn\'t connect to your microphone:\n' + error
				// });
			}
		);

		return () => {
			hostRef.current.mobileRunning = false;
			cancelAllReconnects();
			socket.emit('leave');
			Object.keys(peerConnections).forEach((k) => {
				disconnectPeer(k);
			});
			connectionStuff.current.socket?.close();

			audioListener?.destroy();

			for (const [channel, listener] of ipcListeners) {
				ipcRenderer.off(channel, listener);
			}
			ipcListeners.length = 0;

			// Release the microphone, otherwise the OS keeps showing the app as recording
			// and every re-mount opens another capture on top of the old one.
			localStream?.getTracks().forEach((track) => {
				track.stop();
			});
			localStream = undefined;
			audioContext?.close().catch(() => {
				/* already closed */
			});
			audioContext = undefined;
		};
		// })();
	}, []);

	interface mobileHostInfo {
		mobilePlayerInfo: {
			code: string;
			askingForHost: boolean;
		};
	}

	//data: { mobilePlayerInfo: { code: this.gamecode, askingForHost: true }
	const myPlayer = useMemo(() => {
		if (!gameState?.players) {
			return undefined;
		} else {
			return gameState.players.find((p) => p.isLocal);
		}
	}, [gameState.players]);

	const otherPlayers = useMemo(() => {
		let otherPlayers: Player[];
		if (!gameState?.players || !myPlayer) return [];
		else otherPlayers = gameState.players.filter((p) => !p.isLocal);
		maxDistanceRef.current = lobbySettings.visionHearing
			? myPlayer.isImpostor
				? lobbySettings.maxDistance
				: gameState.lightRadius + 0.5
			: lobbySettings.maxDistance;
		if (maxDistanceRef.current <= 0.6) {
			maxDistanceRef.current = 1;
		}
		hostRef.current = {
			map: gameState.map,
			mobileRunning: hostRef.current.mobileRunning,
			gamestate: gameState.gameState,
			code: gameState.lobbyCode,
			hostId: gameState.hostId,
			isHost: gameState.hostId > 0 ? gameState.isHost : hostRef.current.serverHostId === gameState.clientId,
			parsedHostId: gameState.hostId > 0 ? gameState.hostId : hostRef.current.serverHostId,
			serverHostId: hostRef.current.serverHostId,
		};
		const playerSocketIds: numberStringMap = {};
		for (const k of Object.keys(socketClients)) {
			playerSocketIds[socketClients[k].clientId] = k;
		}
		playerSocketIdsRef.current = playerSocketIds;
		const handledPeerIds: string[] = [];
		let foundRadioUser = false;
		const tempTalking = { ...otherTalking };
		let talkingUpdate = false;
		for (const player of otherPlayers) {
			const peerId = playerSocketIds[player.clientId];
			const audio = player.clientId === myPlayer.clientId ? undefined : audioElements.current[peerId];
			// Why a specific player cannot be heard is decided from several places at once,
			// and afterwards there is nothing left to look at. Report every change, so a
			// report of "we could not hear him" comes with the state that caused it.
			if (player.clientId !== myPlayer.clientId) {
				const reason = player.isDummy
					? 'the game reports the player as a dummy'
					: player.disconnected
						? 'the game reports the player as disconnected'
						: player.bugged
							? 'the player record could not be read'
							: !peerId
								? 'no socket is mapped to this player'
								: !audio
									? 'no audio element for the socket'
									: player.isDead && !myPlayer.isDead
										? 'the player is dead and you are not'
										: playerConfigs[player.nameHash]?.isMuted
											? 'you muted this player'
											: playerConfigs[player.nameHash]?.volume === 0
												? 'you set this player to zero volume'
												: '';
				if (reason !== (silenceReason.current[player.clientId] ?? '')) {
					silenceReason.current[player.clientId] = reason;
					console.log(
						reason
							? `Not hearing ${player.name}: ${reason} (state ${gameState.gameState}, distance ${
									Math.round(Math.hypot(player.x - myPlayer.x, player.y - myPlayer.y) * 10) / 10
								} of ${Math.round(maxDistanceRef.current * 10) / 10})`
							: `Hearing ${player.name} again`
					);
				}
			}
			if (
				player.clientId === impostorRadioClientId.current &&
				player.isImpostor &&
				!player.isDead &&
				!player.disconnected &&
				!player.bugged
			) {
				foundRadioUser = true;
			}
			// Talking is derived fresh every pass, including for players we cannot hear.
			// It used to be written only inside the branch below, so a player whose
			// connection dropped mid-sentence kept their last state: the avatar showed the
			// green speaking ring next to the offline icon for the rest of the session,
			// with no audio behind it.
			let nowTalking = false;
			if (audio) {
				handledPeerIds.push(peerId);
				let gain = calculateVoiceAudio(gameState, settingsRef.current, myPlayer, player, audio);
				if (connectionStuff.current.deafened || playerConfigs[player.nameHash]?.isMuted) {
					gain = 0;
				}

				if (gain > 0) {
					const playerVolume = playerConfigs[player.nameHash]?.volume;
					gain = playerVolume === undefined ? gain : gain * playerVolume;

					if (myPlayer.isDead && !player.isDead) {
						gain = gain * (settings.crewVolumeAsGhost / 100);
					}
					gain = gain * (settings.masterVolume / 100);
				}
				audio.gain.gain.value = gain;
				nowTalking = otherVAD[player.clientId] === true && gain > 0;
			}
			// Compared as strict booleans: an entry that has never been set is undefined,
			// and undefined != false would mark an update on every pass for every player
			// who has never spoken.
			if (nowTalking !== (otherTalking[player.clientId] === true)) {
				talkingUpdate = true;
			}
			tempTalking[player.clientId] = nowTalking;
		}
		if (talkingUpdate) {
			setOtherTalking(tempTalking);
		}

		if (
			((!foundRadioUser && impostorRadioClientId.current !== myPlayer.clientId) || !myPlayer.isImpostor) &&
			impostorRadioClientId.current !== -1
		) {
			impostorRadioClientId.current = -1;
		}
		// for...in over an array walks its indices, so this read audioElements.current['0']
		// and never touched a peer. Peers with no player in the current game state are
		// meant to be silenced, which is what someone still connected to the voice server
		// after leaving the game should be.
		for (const peerId of Object.keys(audioElements.current).filter((id) => !handledPeerIds.includes(id))) {
			const audio = audioElements.current[peerId];
			if (audio?.gain) {
				audio.gain.gain.value = 0;
			}
		}

		return otherPlayers;
	}, [gameState]);

	// Connect to P2P negotiator, when lobby and connect code change
	useEffect(() => {
		if (connect?.connect) {
			connect.connect(gameState?.lobbyCode ?? 'MENU', myPlayer?.id ?? 0, gameState.clientId, gameState.isHost);
			updateLobby();
		}
	}, [connect?.connect, gameState?.lobbyCode, connected]);

	useEffect(() => {
		if (myPlayer?.shiftedColor != -1) {
			connectionStuff.current.socket?.emit('VAD', false);
			setTalking(false);
		}
	}, [myPlayer?.shiftedColor]);

	useEffect(() => {
		if (myPlayer?.shiftedColor == -1 || !talking) connectionStuff.current.socket?.emit('VAD', talking);
	}, [talking]);

	// Connect to P2P negotiator, when game mode change
	useEffect(() => {
		if (
			connect?.connect &&
			gameState.lobbyCode &&
			myPlayer?.clientId !== undefined &&
			gameState.gameState === GameState.LOBBY &&
			(gameState.oldGameState === GameState.DISCUSSION || gameState.oldGameState === GameState.TASKS)
		) {
			hostRef.current.mobileRunning = false;
			connect.connect(gameState.lobbyCode, myPlayer.clientId, gameState.clientId, gameState.isHost);
		} else if (
			gameState.oldGameState !== GameState.UNKNOWN &&
			gameState.oldGameState !== GameState.MENU &&
			gameState.gameState === GameState.MENU
		) {
			console.log('DISCONNECT TO MENU!');
			// On change from a game to menu, exit from the current game properly
			hostRef.current.mobileRunning = false; // On change from a game to menu, exit from the current game properly
			connectionStuff.current.socket?.emit('leave');
			Object.keys(peerConnections).forEach((k) => {
				disconnectPeer(k);
			});
			setOtherDead({});
		}
	}, [gameState.gameState]);

	// Emit player id to socket
	useEffect(() => {
		if (connectionStuff.current.socket && myPlayer && myPlayer.clientId !== undefined) {
			connectionStuff.current.socket.emit('id', myPlayer.id, gameState.clientId);
		}
	}, [myPlayer?.id, myPlayer?.clientId]);

	// Pass voice state to overlay
	useEffect(() => {
		if (!settings.enableOverlay) {
			return;
		}
		ipcRenderer.send(IpcMessages.SEND_TO_OVERLAY, IpcOverlayMessages.NOTIFY_VOICE_STATE_CHANGED, {
			otherTalking,
			playerSocketIds: playerSocketIdsRef.current,
			otherDead,
			socketClients,
			audioConnected,
			localTalking: talking,
			localIsAlive: !myPlayer?.isDead,
			impostorRadioClientId: !myPlayer?.isImpostor ? -1 : impostorRadioClientId.current,
			muted: mutedState,
			deafened: deafenedState,
			mod: gameState.mod,
		} as VoiceState);
	}, [
		otherTalking,
		otherDead,
		socketClients,
		audioConnected,
		talking,
		mutedState,
		deafenedState,
		impostorRadioClientId.current,
	]);

	// Derive the avatar pixel size from the measured column width so borders and hat
	// offsets keep matching the rendered avatar when the window is resized.
	const otherPlayersSpan = getPlayersPerRow(otherPlayers.length);
	const GRID_GUTTER = 8; // matches spacing={1}
	const otherAvatarSize = Math.max(24, Math.round((otherAvatarsWidth * otherPlayersSpan) / 12) - GRID_GUTTER) || 50;

	return (
		<div className={classes.root}>
			{(error || initialError) && (
				<div className={classes.error}>
					<Typography align="center" variant="h6" color="error">
						ERROR
					</Typography>
					<Typography align="center" style={{ whiteSpace: 'pre-wrap' }}>
						{error}
						{initialError}
					</Typography>
					<SupportLink />
				</div>
			)}
			{!error && !initialError && (
				<>
					<div className={classes.top}>
						{myPlayer && gameState.lobbyCode !== 'MENU' && (
							<div className={classes.avatarWrapper} ref={ownAvatarRef}>
								<Avatar
									deafened={deafenedState}
									muted={mutedState}
									player={myPlayer}
									borderColor={myPlayer?.shiftedColor == -1 ? '#2ecc71' : 'gray'}
									connectionState={connected ? 'connected' : 'disconnected'}
									isUsingRadio={myPlayer?.isImpostor && impostorRadioClientId.current === myPlayer.clientId}
									talking={talking}
									isAlive={!myPlayer.isDead}
									size={Math.max(24, Math.round(ownAvatarWidth)) || 100}
									mod={gameState.mod}
								/>
							</div>
						)}
						<div className={classes.right}>
							<div>
								<div className={classes.left}>
									{myPlayer && gameState?.gameState !== GameState.MENU && (
										<span className={classes.username}>{myPlayer.name}</span>
									)}
									<span
										className={classes.code}
										style={{
											background: gameState.lobbyCode === 'MENU' ? 'transparent' : '#3e4346',
										}}
									>
										{displayedLobbyCode === 'MENU' ? t('game.menu') : displayedLobbyCode}
									</span>
								</div>
								{gameState.lobbyCode !== 'MENU' && (
									<div className={classes.muteButtons}>
										<IconButton onClick={connectionStuff.current.toggleMute} size="small">
											{mutedState || deafenedState ? <MicOff /> : <Mic />}
										</IconButton>
										<IconButton onClick={connectionStuff.current.toggleDeafen} size="small">
											{deafenedState ? <VolumeOff /> : <VolumeUp />}
										</IconButton>
									</div>
								)}
							</div>
						</div>
					</div>
					{lobbySettings.deadOnly && (
						<div className={classes.top}>
							<small style={{ padding: 0 }}>{t('settings.lobbysettings.ghost_only_warning2')}</small>
						</div>
					)}
					{lobbySettings.meetingGhostOnly && (
						<div className={classes.top}>
							<small style={{ padding: 0 }}>{t('settings.lobbysettings.meetings_only_warning2')}</small>
						</div>
					)}
					{gameState.lobbyCode && <Divider />}
					{displayedLobbyCode === 'MENU' && (
						<div className={classes.top}>
							<Button
								style={{ margin: '10px' }}
								onClick={() => {
									ipcRenderer.send(IpcHandlerMessages.OPEN_LOBBYBROWSER);
								}}
								color="primary"
								variant="outlined"
							>
								{t('buttons.public_lobby')}
							</Button>
						</div>
					)}
					{myPlayer && gameState.lobbyCode !== 'MENU' && (
						<Grid
							container
							spacing={1}
							ref={otherAvatarsRef}
							className={classes.otherplayers}
							sx={{ alignItems: 'flex-start', alignContent: 'flex-start', justifyContent: 'flex-start' }}
						>
							{otherPlayers.map((player) => {
								const peer = playerSocketIdsRef.current[player.clientId];
								const connected = socketClients[peer]?.clientId === player.clientId || false;
								const audio = audioConnected[peer];

								if (!playerConfigs[player.nameHash]) {
									playerConfigs[player.nameHash] = { volume: 1, isMuted: false };
								}
								const socketConfig = playerConfigs[player.nameHash];

								return (
									<Grid key={player.id} size={otherPlayersSpan}>
										<Avatar
											connectionState={!connected ? 'disconnected' : audio ? 'connected' : 'novoice'}
											player={player}
											talking={!player.inVent && otherTalking[player.clientId]}
											borderColor="#2ecc71"
											isAlive={!otherDead[player.clientId]}
											isUsingRadio={
												myPlayer?.isImpostor &&
												!(player.disconnected || player.bugged) &&
												impostorRadioClientId.current === player.clientId
											}
											size={otherAvatarSize}
											socketConfig={socketConfig}
											onConfigChange={() => {
												playerConfigs[player.nameHash].lastUsed = Date.now();
												setSetting(`playerConfigMap.${player.nameHash}`, playerConfigs[player.nameHash]);
											}}
											mod={gameState.mod}
										/>
									</Grid>
								);
							})}
						</Grid>
					)}
				</>
			)}
		</div>
	);
};

type ValidPlayersPerRow = 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12;
function getPlayersPerRow(playerCount: number): ValidPlayersPerRow {
	if (playerCount <= 9) return (12 / 3) as ValidPlayersPerRow;
	// max(1, ...): the grid span must never reach 0, which MUI rejects.
	else return Math.max(1, Math.floor(12 / Math.ceil(Math.sqrt(playerCount)))) as ValidPlayersPerRow;
}

export default Voice;
