import redAliveimg from '../../static/images/avatar/placeholder.png?url';
import rainbowAliveimg from '../../static/images/avatar/rainbow-alive.png?url';
import rainbowDeadeimg from '../../static/images/avatar/rainbow-dead.png?url';
import { RainbowColorId } from '../common/playerColors';

export { RainbowColorId } from '../common/playerColors';
export const redAlive = redAliveimg;

// Hats, skins and visors used to be downloaded from a jsDelivr-hosted collection at
// runtime. That download was removed, so only the locally bundled base bodies are
// drawn and the app no longer contacts a third-party CDN on startup.
export function getCosmetic(color: number, isAlive: boolean): string {
	if (color === RainbowColorId) {
		return isAlive ? rainbowAliveimg : rainbowDeadeimg;
	}
	return `static:///generated/${isAlive ? 'player' : 'ghost'}/${color}.png`;
}
