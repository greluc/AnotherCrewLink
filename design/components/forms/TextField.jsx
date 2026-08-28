import React from 'react';

/** MUI TextField variant="outlined". Used for the server URL, the public lobby
 *  title, and the four shortcut capture fields. */
export function TextField({ label, value = '', placeholder, error = false, helperText, readOnly = false, onChange, onKeyDown, fullWidth = true }) {
  const border = error ? 'var(--acl-red-500)' : 'var(--acl-border-soft)';
  return (
    <label style={{ display: 'block', position: 'relative', width: fullWidth ? '100%' : 220, marginTop: 8, fontFamily: 'var(--font-ui)' }}>
      <span style={{
        position: 'absolute', top: -8, left: 9, padding: '0 4px', fontSize: 12,
        background: 'var(--surface-app)', color: error ? 'var(--acl-red-500)' : 'var(--text-quiet)',
      }}>{label}</span>
      <input
        value={value}
        placeholder={placeholder}
        readOnly={readOnly}
        spellCheck={false}
        onChange={(e) => onChange && onChange(e.target.value)}
        onKeyDown={onKeyDown}
        style={{
          width: '100%', color: '#fff', background: 'transparent', boxSizing: 'border-box',
          border: `1px solid ${border}`, borderRadius: 4, padding: '14px 12px',
          fontFamily: 'var(--font-ui)', fontSize: 'var(--size-body)',
        }}
      />
      {helperText && (
        <span style={{ display: 'block', marginTop: 3, marginLeft: 12, fontSize: 12, color: error ? 'var(--acl-red-500)' : 'var(--text-quiet)' }}>{helperText}</span>
      )}
    </label>
  );
}
