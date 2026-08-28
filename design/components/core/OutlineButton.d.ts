import type { ReactNode, CSSProperties } from 'react';

/**
 * The outlined white button used outside MUI's own controls.
 */
export interface OutlineButtonProps {
  children?: ReactNode;
  onClick?: () => void;
  /** Font size in px. 19 in SupportLink, 24 in the launch group. */
  size?: number;
  disabled?: boolean;
  style?: CSSProperties;
}
export function OutlineButton(props: OutlineButtonProps): JSX.Element;
