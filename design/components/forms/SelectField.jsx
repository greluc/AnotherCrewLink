import React from 'react';

/** MUI TextField select variant="outlined" color="secondary" with a shrunk label —
 *  microphone, speaker, overlay position, language. */
export function SelectField({ label, value, options = [], onChange, fullWidth = true }) {
  return (
    <label style={{ display: 'block', position: 'relative', width: fullWidth ? '100%' : 220, marginTop: 8, fontFamily: 'var(--font-ui)' }}>
      <span style={{
        position: 'absolute', top: -8, left: 9, padding: '0 4px', fontSize: 12,
        background: 'var(--surface-app)', color: 'var(--text-quiet)',
      }}>{label}</span>
      <select
        value={value}
        onChange={(e) => onChange && onChange(e.target.value)}
        style={{
          width: '100%', appearance: 'none', color: '#fff', background: 'transparent',
          border: '1px solid var(--acl-border-soft)', borderRadius: 4,
          padding: '14px 32px 14px 12px', fontFamily: 'var(--font-ui)', fontSize: 'var(--size-body)',
        }}
      >
        {options.map((o) => (
          <option key={typeof o === 'string' ? o : o.value} value={typeof o === 'string' ? o : o.value} style={{ background: 'var(--surface-card)' }}>
            {typeof o === 'string' ? o : o.label}
          </option>
        ))}
      </select>
      <span className="acl-icon" style={{ position: 'absolute', right: 8, top: '50%', transform: 'translateY(-50%)', pointerEvents: 'none', color: 'var(--text-quiet)' }}>arrow_drop_down</span>
    </label>
  );
}
