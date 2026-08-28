/**
 * A single-line outlined input.
 */
export interface TextFieldProps {
  label?: string;
  value?: string;
  placeholder?: string;
  /** Red border and label — the server URL field while the URL is invalid. */
  error?: boolean;
  helperText?: string;
  /** Shortcut fields are read-only: they capture a key press instead of text. */
  readOnly?: boolean;
  onChange?: (value: string) => void;
  onKeyDown?: (event: React.KeyboardEvent) => void;
  fullWidth?: boolean;
}
export function TextField(props: TextFieldProps): JSX.Element;
