import React from 'react';

/** One option of a MUI RadioGroup — the three microphone modes. */
export function RadioOption({ label, value, selected = false, onSelect }) {
  return (
    <label style={{ display: 'flex', alignItems: 'center', cursor: 'pointer', fontFamily: 'var(--font-ui)', fontSize: 'var(--size-body)' }}>
      <span style={{ width: 42, height: 42, display: 'grid', placeItems: 'center' }}>
        <span style={{
          width: 18, height: 18, borderRadius: '50%',
          border: `2px solid ${selected ? 'var(--accent-primary)' : 'rgba(255,255,255,.6)'}`,
          display: 'grid', placeItems: 'center',
        }}>
          {selected && <span style={{ width: 9, height: 9, borderRadius: '50%', background: 'var(--accent-primary)' }} />}
        </span>
      </span>
      <input type="radio" checked={selected} onChange={() => onSelect && onSelect(value)} style={{ position: 'absolute', opacity: 0, width: 0, height: 0 }} />
      <span>{label}</span>
    </label>
  );
}
