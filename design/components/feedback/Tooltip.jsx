import React from 'react';

/** MUI Tooltip with the client's 15px override. Hover-only, 300ms leave delay so
 *  the volume slider inside a player tooltip stays reachable. */
export function Tooltip({ title, children, placement = 'top', open: forced }) {
  const [hover, setHover] = React.useState(false);
  const open = forced === undefined ? hover : forced;
  const pos = placement === 'bottom'
    ? { top: '100%', marginTop: 6 }
    : { bottom: '100%', marginBottom: 6 };
  return (
    <span style={{ position: 'relative', display: 'inline-flex' }} onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}>
      {children}
      {open && title && (
        <span style={{
          position: 'absolute', left: '50%', transform: 'translateX(-50%)', ...pos,
          background: 'var(--acl-bg-tooltip)', border: '1px solid gray', borderRadius: 'var(--radius-sm)',
          padding: '4px 8px', fontFamily: 'var(--font-ui)', fontSize: 'var(--size-tooltip)',
          color: '#fff', whiteSpace: 'pre-line', zIndex: 30, textAlign: 'center', minWidth: 'max-content',
        }}>{title}</span>
      )}
    </span>
  );
}
