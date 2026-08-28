import type { ReactNode, CSSProperties } from 'react';
/**
 * An inline notice.
 */
export interface AlertProps {
  severity?: 'error' | 'info' | 'success' | 'warning';
  children?: ReactNode;
  style?: CSSProperties;
}
export function Alert(props: AlertProps): JSX.Element;
