/**
 * A determinate bar.
 */
export interface MeterBarProps {
  /** 0–100. The mic meter feeds RMS × 2 × 100. */
  value?: number;
  indeterminate?: boolean;
  color?: 'primary' | 'secondary';
  width?: number;
  height?: number;
}
export function MeterBar(props: MeterBarProps): JSX.Element;
