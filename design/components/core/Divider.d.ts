import type { CSSProperties } from 'react';
/** A horizontal rule between settings sections. */
export interface DividerProps {
  /** Vertical margin in px. 16 (theme.spacing(2)) in the settings panel. */
  spacing?: number;
  style?: CSSProperties;
}
export function Divider(props: DividerProps): JSX.Element;
