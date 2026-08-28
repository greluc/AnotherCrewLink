import type { CSSProperties } from 'react';
/**
 * A per-player state badge, drawn over the middle of their crewmate.
 */
export interface StatusBadgeProps {
  state?: 'muted' | 'deafened' | 'novoice' | 'disconnected' | 'bugged';
  size?: number;
  style?: CSSProperties;
}
export function StatusBadge(props: StatusBadgeProps): JSX.Element;
