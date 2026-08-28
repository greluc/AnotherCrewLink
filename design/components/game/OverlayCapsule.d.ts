import type { ReactNode, CSSProperties } from 'react';
/**
 * The container for the in-game overlay.
 */
export interface OverlayCapsuleProps {
  children?: ReactNode;
  /** Matches the overlay position setting. */
  position?: 'top' | 'bottom_left' | 'left' | 'right' | 'menu';
  /** Compact overlay: only players who are talking are drawn at all. */
  compact?: boolean;
  style?: CSSProperties;
}
export function OverlayCapsule(props: OverlayCapsuleProps): JSX.Element;
