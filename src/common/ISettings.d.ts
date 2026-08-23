import { GamePlatform } from './GamePlatform';

export interface ISettings {
	alwaysOnTop: boolean;
	language: string;
	microphone: string;
	speaker: string;
	/** Device labels, used to recover the ids after they change. */
	microphoneLabel: string;
	speakerLabel: string;
	pushToTalkMode: number;
	serverURL: string;
	pushToTalkShortcut: string;
	deafenShortcut: string;
	muteShortcut: string;
	impostorRadioShortcut: string;
	hideCode: boolean;
	natFix: boolean;
	compactOverlay: boolean;
	overlayPosition: string;
	enableOverlay: boolean;
	meetingOverlay: boolean;

	localLobbySettings: ILobbySettings;
	ghostVolumeAsImpostor: number;
	crewVolumeAsGhost: number;
	masterVolume: number;
	microphoneGain: number;
	microphoneGainEnabled: boolean;
	micSensitivity: number;
	micSensitivityEnabled: boolean;
	mobileHost: boolean;
	vadEnabled: boolean;
	hardware_acceleration: boolean;
	echoCancellation: boolean;
	noiseSuppression: boolean;
	oldSampleDebug: boolean;

	enableSpatialAudio: boolean;
	playerConfigMap: playerConfigMap;
	obsOverlay: boolean;
	obsSecret: string | undefined;

	launchPlatform: GamePlatform | string;
	customPlatforms: GamePlatformMap;
}

export interface ILobbySettings {
	maxDistance: number;
	visionHearing: boolean;
	haunting: boolean;
	hearImpostorsInVents: boolean;
	impostersHearImpostersInvent: boolean;
	impostorRadioEnabled: boolean;
	commsSabotage: boolean;
	deadOnly: boolean;
	meetingGhostOnly: boolean;
	hearThroughCameras: boolean;
	wallsBlockAudio: boolean;
	publicLobby_on: boolean;
	publicLobby_title: string;
	publicLobby_language: string;
}

export interface SocketConfig {
	volume: number;
	isMuted: boolean;
	/** Epoch ms of the last change, used to prune the oldest entries instead of all. */
	lastUsed?: number;
}

export interface playerConfigMap {
	[socketId: number]: SocketConfig;
}
