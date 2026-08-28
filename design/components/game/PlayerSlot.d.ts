/**
 * A player in the roster: crewmate, badge and name.
 */
export interface PlayerSlotProps {
  name?: string;
  color?: string;
  shadow?: string;
  size?: number;
  /** Slot width. 76 for others, 96 for your own (`own`). */
  slot?: number;
  talking?: boolean;
  alive?: boolean;
  /** A `StatusBadge` state, or nothing when the player is fine. */
  badge?: 'muted' | 'deafened' | 'novoice' | 'disconnected' | 'bugged';
  /** Your own slot: larger, and shown above the wrapped list. */
  own?: boolean;
  /** Cosmetic file names, passed through to \`Crewmate\`. */
  hat?: string;
  hatBack?: string;
  visor?: string;
  skin?: string;
  link?: 'connected' | 'novoice' | 'disconnected';
  usingRadio?: boolean;
  shape?: 'circle' | 'sprite';
  /** Clip cosmetics to the round frame — see \`Crewmate\`. */
  overflow?: boolean;
  /** Path to this design system's \`assets\` folder, relative to the page. */
  assetBase?: string;
  onClick?: () => void;
}
export function PlayerSlot(props: PlayerSlotProps): JSX.Element;
