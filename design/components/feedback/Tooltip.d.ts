import type { ReactNode } from 'react';
/** A hover tooltip. 15px text — larger than MUI's default, set in theme.ts. */
export interface TooltipProps {
  title?: ReactNode;
  children?: ReactNode;
  placement?: 'top' | 'bottom';
  /** Force the open state, for specimens. */
  open?: boolean;
}
export function Tooltip(props: TooltipProps): JSX.Element;
