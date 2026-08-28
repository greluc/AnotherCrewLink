import type { CSSProperties } from 'react';
/**
 * A circular icon-only button — the title bar, the settings back arrow, the
 * per-player mute toggle.
 */
export interface IconButtonProps {
  /** Material Symbols ligature name. */
  icon: string;
  size?: 'small' | 'medium';
  color?: string;
  onClick?: () => void;
  /** Accessible name — every icon button in the client is unlabelled visually. */
  label?: string;
  style?: CSSProperties;
}
export function IconButton(props: IconButtonProps): JSX.Element;
