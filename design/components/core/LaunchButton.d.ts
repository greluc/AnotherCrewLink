/**
 * The split "Open via <platform>" control on the waiting screen.
 */
export interface LaunchButtonProps {
  /** The selected platform's name, or "No platform detected". */
  label?: string;
  /** Platform names offered in the dropdown. */
  platforms?: string[];
  disabled?: boolean;
  onLaunch?: () => void;
  onSelect?: (platform: string) => void;
}
export function LaunchButton(props: LaunchButtonProps): JSX.Element;
