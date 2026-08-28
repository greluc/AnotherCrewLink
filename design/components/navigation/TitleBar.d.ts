/**
 * The client window's title bar.
 */
export interface TitleBarProps {
  /** Appended to the title as ` v1.0.6`. */
  version?: string;
  onSettings?: () => void;
  onReload?: () => void;
  onClose?: () => void;
  title?: string;
}
export function TitleBar(props: TitleBarProps): JSX.Element;
