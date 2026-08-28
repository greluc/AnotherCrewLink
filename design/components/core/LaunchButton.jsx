import React from 'react';

/** The split launch control from src/renderer/LaunchButton.tsx: a wide primary
 *  button and a dropdown toggle that share a 4px white border. */
export function LaunchButton({ label = 'Steam', platforms = [], disabled = false, onLaunch, onSelect }) {
  const [open, setOpen] = React.useState(false);
  const [hoverMain, setHoverMain] = React.useState(false);
  const [hoverDrop, setHoverDrop] = React.useState(false);
  const green = 'var(--accent-action)';
  return (
    <div style={{ position: 'relative', display: 'inline-flex', margin: '0 10px' }}>
      <button
        type="button"
        disabled={disabled}
        onClick={onLaunch}
        onMouseEnter={() => setHoverMain(true)}
        onMouseLeave={() => setHoverMain(false)}
        style={{
          color: '#fff', background: 'none', padding: '2px 10px',
          borderRadius: '10px 0 0 10px', borderWidth: '4px 2px 4px 4px', borderStyle: 'solid',
          borderColor: hoverMain && !disabled ? green : '#fff',
          fontSize: 24, fontWeight: 500, fontFamily: 'var(--font-ui)', outline: 'none',
          textTransform: 'none', cursor: disabled ? 'default' : 'pointer',
          opacity: disabled ? 0.5 : 1, transition: 'var(--transition-border)',
        }}
      >
        {label}
      </button>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        onMouseEnter={() => setHoverDrop(true)}
        onMouseLeave={() => setHoverDrop(false)}
        style={{
          color: '#fff', background: 'none', padding: 0, minWidth: 40,
          borderRadius: '0 10px 10px 0', borderWidth: '4px 4px 4px 2px', borderStyle: 'solid',
          borderColor: open || hoverDrop ? green : '#fff',
          cursor: 'pointer', outline: 'none', transition: 'var(--transition-border)',
          display: 'grid', placeItems: 'center', fontFamily: 'var(--font-icon)', fontSize: 24,
        }}
        aria-label="Choose platform"
      >
        arrow_drop_down
      </button>
      {open && (
        <div style={{
          position: 'absolute', top: '100%', right: 0, marginTop: 2, zIndex: 5,
          maxHeight: 110, overflow: 'auto', minWidth: 140,
          border: '1px solid var(--acl-border-soft)', background: 'var(--surface-card)',
        }}>
          {platforms.map((p) => (
            <div
              key={p}
              onClick={() => { onSelect && onSelect(p); setOpen(false); }}
              style={{ padding: '6px 16px', fontFamily: 'var(--font-ui)', fontSize: 14, cursor: 'pointer' }}
              onMouseEnter={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,.08)'; }}
              onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent'; }}
            >
              {p}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
