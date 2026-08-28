import type { CSSProperties } from 'react';

/** Where cosmetics are fetched from — the pinned commit, as the client pins it. */
export const HAT_COLLECTION_COMMIT: string;
export const HAT_COLLECTION_URL: string;
/** hats.json's NONE defaults: width 130%, top -78%, left -14%. */
export const COSMETIC_DEFAULTS: { width: string; top: string; left: string };
/** A cosmetic file name from hats.json to its CDN URL. */
export function cosmeticUrl(file?: string): string;

/**
 * One player's avatar: the client's own crewmate body, recoloured to a crew colour
 * pair, with hat / skin / visor artwork composited over it.
 */
export interface CrewmateProps {
  /** Body colour — a hex string or `var(--crew-*)`. */
  color?: string;
  /** Shadow colour — always the pair's second value, never a tint of the first. */
  shadow?: string;
  /** Diameter in px. 52 in a roster slot, 68–80 for your own. */
  size?: number;
  /** Green ring outside the body. */
  talking?: boolean;
  /** Dead players lose their cosmetics and fade, as the ghost body does in game. */
  alive?: boolean;
  /** Mirrored, as the overlay does on the right-hand side. */
  lookLeft?: boolean;
  /** `connected` draws no outline at all. */
  link?: 'connected' | 'novoice' | 'disconnected';
  /** Cosmetic file names from hats.json, e.g. `pk01_Astronaut.png`. */
  hat?: string;
  hatBack?: string;
  visor?: string;
  skin?: string;
  /** The faint idle ring the overlay's side positions draw (#ccbdcc86). */
  showBorder?: boolean;
  /** \`circle\` clips to a round frame (the Electron client); \`sprite\` shows the whole
   *  crewmate uncropped (the Rust GUI). */
  shape?: 'circle' | 'sprite';
  /** Nest the cosmetics inside the round frame so they are clipped with the body.
   *  Avatar.tsx's own prop, and off by default: a hat is meant to overhang. */
  overflow?: boolean;
  /** Draws radio.svg over the lower right — this impostor is holding the radio. */
  usingRadio?: boolean;
  /** Path to this design system's `assets` folder, relative to the page. */
  assetBase?: string;
  style?: CSSProperties;
}
export function Crewmate(props: CrewmateProps): JSX.Element;
