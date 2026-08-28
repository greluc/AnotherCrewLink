import React from 'react';

/** MUI Slider, size="small", as used for voice distance and every volume. */
export function Slider({ label, value = 50, min = 0, max = 100, step = 1, disabled = false, color = 'primary', suffix = '', onChange }) {
  const pct = ((value - min) / (max - min)) * 100;
  const track = color === 'secondary' ? 'var(--acl-red-500)' : 'var(--accent-primary)';
  return (
    <div style={{ width: '100%', fontFamily: 'var(--font-ui)', opacity: disabled ? 0.38 : 1 }}>
      {label !== undefined && (
        <div style={{ fontSize: 'var(--size-body)', marginBottom: 4 }}>{label}{suffix}</div>
      )}
      {/* The 12px thumb is inset 6px each side, so it cannot hang past the track at
          either end — an overhang there gives every consumer a horizontal scrollbar. */}
      <div style={{ position: 'relative', height: 20, display: 'flex', alignItems: 'center', padding: '0 6px', boxSizing: 'border-box' }}>
        <div style={{ position: 'absolute', left: 6, right: 6, height: 2, background: 'rgba(255,255,255,.26)', borderRadius: 1 }} />
        <div style={{ position: 'absolute', left: 6, width: `calc((100% - 12px) * ${pct / 100})`, height: 2, background: track, borderRadius: 1 }} />
        <div style={{ position: 'absolute', left: `calc(6px + (100% - 12px) * ${pct / 100})`, width: 12, height: 12, transform: 'translateX(-50%)', borderRadius: '50%', background: track }} />
        <input
          type="range" min={min} max={max} step={step} value={value} disabled={disabled}
          onChange={(e) => onChange && onChange(Number(e.target.value))}
          style={{ position: 'absolute', left: 6, right: 6, width: 'auto', opacity: 0, height: 20, margin: 0, cursor: disabled ? 'default' : 'pointer' }}
        />
      </div>
    </div>
  );
}
