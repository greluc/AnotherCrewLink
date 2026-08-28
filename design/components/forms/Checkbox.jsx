import React from 'react';

/** A settings toggle: MUI Checkbox + FormControlLabel with the hairline top
 *  border Settings.tsx's `formLabel` class adds. Full width, label on the right. */
export function Checkbox({ label, checked = false, disabled = false, onChange, divided = true }) {
  return (
    <label
      style={{
        display: 'flex', alignItems: 'center', gap: 0, width: '100%',
        borderTop: divided ? '1px solid var(--border-hairline)' : 'none',
        marginRight: 0, opacity: disabled ? 0.38 : 1,
        cursor: disabled ? 'default' : 'pointer', fontFamily: 'var(--font-ui)', fontSize: 'var(--size-body)',
      }}
    >
      <span style={{ width: 42, height: 42, display: 'grid', placeItems: 'center', flex: '0 0 auto' }}>
        <span style={{
          width: 18, height: 18, borderRadius: 2, display: 'grid', placeItems: 'center',
          border: checked ? 'none' : '2px solid rgba(255,255,255,.6)',
          background: checked ? 'var(--accent-primary)' : 'transparent',
          color: '#1d1a23', fontFamily: 'var(--font-icon)', fontSize: 16, lineHeight: 1,
        }}>{checked ? 'check' : ''}</span>
      </span>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange && onChange(e.target.checked)}
        style={{ position: 'absolute', opacity: 0, width: 0, height: 0 }}
      />
      <span>{label}</span>
    </label>
  );
}
