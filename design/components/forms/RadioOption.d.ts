/** One option in a radio group. The client has exactly one group: Voice Activity / Push to Talk / Push to Mute. */
export interface RadioOptionProps {
  label?: string;
  value?: string | number;
  selected?: boolean;
  onSelect?: (value: string | number) => void;
}
export function RadioOption(props: RadioOptionProps): JSX.Element;
