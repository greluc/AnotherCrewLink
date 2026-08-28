import type { ReactNode } from 'react';
/**
 * A modal confirmation or notice.
 */
export interface DialogProps {
  open?: boolean;
  title?: ReactNode;
  children?: ReactNode;
  /** Buttons, right-aligned. Confirm first, then Cancel. */
  actions?: ReactNode;
  width?: number;
}
export function Dialog(props: DialogProps): JSX.Element;
