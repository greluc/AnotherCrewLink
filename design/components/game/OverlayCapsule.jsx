import React from 'react';

/** The in-game overlay container. Drawn over the running game, so its background is
 *  a veil rather than a surface: rgba(0,0,0,.5) in game, .35 bottom-left, and the
 *  compact side positions use the #25232ac0 capsule with one rounded end. */
export function OverlayCapsule({ children, position = 'top', compact = false, style }) {
  const veil = {
    top: { background: 'var(--acl-veil-mid)', borderRadius: 'var(--radius-md)' },
    bottom_left: { background: 'var(--acl-veil-soft)', borderRadius: 'var(--radius-md)' },
    left: { background: 'var(--acl-veil-window)', borderTopRightRadius: 'var(--radius-capsule)', borderBottomRightRadius: 'var(--radius-capsule)' },
    right: { background: 'var(--acl-veil-window)', borderTopLeftRadius: 'var(--radius-capsule)', borderBottomLeftRadius: 'var(--radius-capsule)' },
    menu: { background: 'var(--acl-veil-strong)', borderRadius: 'var(--radius-md)' },
  }[position];
  return (
    <div style={{
      display: 'flex', flexWrap: 'wrap', alignItems: 'center',
      justifyContent: position === 'bottom_left' ? 'flex-start' : 'center',
      gap: 10, padding: compact ? 5 : 8, ...veil, ...style,
    }}>{children}</div>
  );
}
