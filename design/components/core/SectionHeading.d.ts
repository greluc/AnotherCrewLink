import type { ReactNode, CSSProperties } from 'react';
/** A settings section title. */
export interface SectionHeadingProps {
  children?: ReactNode;
  align?: 'left' | 'center';
  style?: CSSProperties;
}
export function SectionHeading(props: SectionHeadingProps): JSX.Element;
