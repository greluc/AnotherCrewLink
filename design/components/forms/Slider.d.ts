/**
 * A continuous setting: voice distance (1–10, step 0.1), microphone gain
 * (0–300, step 2), master volume (0–200), sensitivity (0–1, step 0.05).
 */
export interface SliderProps {
  label?: string;
  value?: number;
  min?: number;
  max?: number;
  step?: number;
  disabled?: boolean;
  /** `secondary` is the red track the sensitivity slider switches to above 0.3. */
  color?: 'primary' | 'secondary';
  /** Appended to the label, e.g. `: 5.3`. */
  suffix?: string;
  onChange?: (value: number) => void;
}
export function Slider(props: SliderProps): JSX.Element;
