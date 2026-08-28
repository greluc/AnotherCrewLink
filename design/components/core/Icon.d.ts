import type { CSSProperties } from 'react';
/** A Material Symbols Rounded glyph, standing in for @mui/icons-material. */
export interface IconProps {
  /** Ligature name, e.g. `settings`, `mic_off`, `volume_off`, `wifi_off`. */
  name: string;
  size?: number;
  color?: string;
  style?: CSSProperties;
}
export function Icon(props: IconProps): JSX.Element;
