import type { ReactNode, CSSProperties } from 'react';

/**
 * A text or contained button, matching the client's MUI configuration.
 */
export interface ButtonProps {
  children?: ReactNode;
  /** `text` for dialog actions, `contained` for standalone actions. */
  variant?: 'text' | 'contained';
  /** `primary` is purple, `secondary` red, `grey` the updater's dismissals. */
  color?: 'primary' | 'secondary' | 'grey';
  disabled?: boolean;
  onClick?: () => void;
  style?: CSSProperties;
}
export function Button(props: ButtonProps): JSX.Element;
