/**
 * A native select in an outlined field.
 */
export interface SelectFieldProps {
  label?: string;
  value?: string;
  /** Plain strings, or `{ value, label }` pairs. */
  options?: Array<string | { value: string; label: string }>;
  onChange?: (value: string) => void;
  fullWidth?: boolean;
}
export function SelectField(props: SelectFieldProps): JSX.Element;
