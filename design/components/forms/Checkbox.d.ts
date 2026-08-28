/**
 * One boolean setting.
 */
export interface CheckboxProps {
  label?: string;
  checked?: boolean;
  /** Lobby rules are disabled unless this client is host and in a lobby. */
  disabled?: boolean;
  onChange?: (checked: boolean) => void;
  /** The 1px `--border-hairline` rule above the row. On by default. */
  divided?: boolean;
}
export function Checkbox(props: CheckboxProps): JSX.Element;
