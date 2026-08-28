import React from 'react';

/** MUI LinearProgress. Two uses: the 200×8 microphone level meter (secondary,
 *  determinate, transform .05s linear) and the updater's download progress. */
export function MeterBar({ value = 0, indeterminate = false, color = 'secondary', width = 200, height = 8 }) {
  const track = color === 'secondary' ? 'var(--acl-red-500)' : 'var(--accent-primary)';
  return (
    <div style={{ width, height, borderRadius: 0, overflow: 'hidden', background: 'rgba(244,67,54,.35)', margin: '5px auto' }}>
      <div style={{
        height: '100%', background: track,
        width: indeterminate ? '40%' : `${Math.max(0, Math.min(100, value))}%`,
        transition: 'var(--transition-meter)',
      }} />
    </div>
  );
}
