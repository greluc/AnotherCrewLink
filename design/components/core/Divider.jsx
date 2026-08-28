import React from 'react';

/** MUI Divider as Settings.tsx styles it: full width, 16px of air either side. */
export function Divider({ spacing = 16, style }) {
  return <hr style={{ width: '100%', border: 0, borderTop: '1px solid rgba(255,255,255,0.12)', margin: `${spacing}px 0`, ...style }} />;
}
