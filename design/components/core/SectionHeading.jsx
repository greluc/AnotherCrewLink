import React from 'react';

/** MUI Typography variant h6 — the only heading level the client uses. */
export function SectionHeading({ children, align = 'center', style }) {
  return (
    <h2 style={{ font: 'var(--text-heading)', margin: 0, textAlign: align, color: 'var(--text-body)', ...style }}>{children}</h2>
  );
}
