import redAliveimg from '../../static/images/avatar/placeholder.png?url';
import rainbowAliveimg from '../../static/images/avatar/rainbow-alive.png?url';
import rainbowDeadeimg from '../../static/images/avatar/rainbow-dead.png?url';

import { ModsType } from '../common/Mods';
import { RainbowColorId } from '../common/playerColors';

export { RainbowColorId } from '../common/playerColors';
export const redAlive = redAliveimg;

export enum cosmeticType {
	base,
	hat,
	hat_back,
}
interface hatData {
	image: string;
	back_image: string;
	top: string | undefined;
	width: string | undefined;
	left: string | undefined;
	multi_color: boolean | undefined;
	mod: ModsType | undefined;
}
let hatCollection: {
	[mod: string]: {
		defaultWidth: string;
		defaultTop: string;
		defaultLeft: string;
		hats: {
			[id: string]: hatData;
		};
	};
} = {};

export interface HatDementions {
	top: string;
	left: string;
	width: string;
}

let requestingHats = false;
export let initializedHats = false;

// Bumped whenever the collection changes, so components can depend on a real value
// instead of on `initializedHats`, whose mutation never triggers a React render.
let hatsRevision = 0;
let nextRetryAt = 0;
const hatListeners = new Set<() => void>();

export function subscribeHats(listener: () => void): () => void {
	hatListeners.add(listener);
	return () => {
		hatListeners.delete(listener);
	};
}

export function getHatsRevision(): number {
	return hatsRevision;
}

export function initializeHats(): void {
	if (initializedHats || requestingHats || Date.now() < nextRetryAt) {
		return;
	}
	requestingHats = true;
	fetch(`${HAT_COLLECTION_URL}/hats.json`)
		.then((response) => {
			if (!response.ok) {
				throw new Error(`hats.json responded with HTTP ${response.status}`);
			}
			return response.json();
		})
		.then((data) => {
			hatCollection = data;
			initializedHats = true;
			hatsRevision++;
			for (const listener of hatListeners) {
				listener();
			}
		})
		.catch((error) => {
			// Without this the rejection went unhandled and `requestingHats` stayed true
			// forever, so hats never loaded again for the rest of the session.
			nextRetryAt = Date.now() + 30_000;
			console.warn('Failed to load the hat collection, retrying later:', error);
		})
		.finally(() => {
			requestingHats = false;
		});
}

const HAT_COLLECTION_URL =  'https://cdn.jsdelivr.net/gh/OhMyGuus/BetterCrewLink-Hats@master/'; //'https://raw.githubusercontent.com/OhMyGuus/BetterCrewlink-Hats/master';
function getModHat(color: number, id = '', mod: ModsType, back = false) {
	if (!initializedHats) {
		return '';
	}
	const hatBase = getHat(id, mod);
	const hat = back ? hatBase?.back_image : hatBase?.image;
	const multiColor = hatBase?.multi_color;
	if (hat && hatBase) {
		if (!multiColor) return `${HAT_COLLECTION_URL}${hatBase.mod}/${hat}`;
		else return `generate:///${HAT_COLLECTION_URL}${hatBase.mod}/${hat}?color=${color}`;
	}
	return undefined;
}



function getHat(id: string, modType: ModsType): hatData | undefined {
	if (!initializedHats) {
		return undefined;
	}
	for (const mod of ['NONE' as ModsType, modType]) {
		const modHatList = hatCollection[mod];
		const hat = modHatList?.hats[id];
		if (hat) {
			hat.top = hat?.top ?? modHatList?.defaultTop;
			hat.width = hat?.width ?? modHatList?.defaultWidth;
			hat.left = hat?.left ?? modHatList?.defaultLeft;
			hat.mod = mod;
			return hat;
		}
	}
	return undefined;
}

export function getHatDementions(id: string, mod: ModsType): HatDementions {
	const hat = getHat(id, mod);
	return {
		top: hat?.top ?? '0',
		width: hat?.width ?? '0',
		left: hat?.left ?? '0',
	};
}

export function getCosmetic(
	color: number,
	isAlive: boolean,
	type: cosmeticType,
	id = '',
	mod: ModsType = 'NONE'
): string {
	if (type === cosmeticType.base) {
		if (color == RainbowColorId) {
			return isAlive ? rainbowAliveimg : rainbowDeadeimg;
		}
		return `static:///generated/${isAlive ? `player` : `ghost`}/${color}.png`;
	} else {
		const modHat = getModHat(color, id, mod, type === cosmeticType.hat_back);
		if (modHat) return modHat;
	}
	return '';
}
