import React from 'react';

/** MUI Dialog: a paper panel over a scrim, actions right-aligned. The client's
 *  dialogs are confirmations, the updater and the lobby-code reveal. */
export function Dialog({ open = true, title, children, actions, width = 320 }) {
  if (!open) return null;
  return (
    <div style={{ position: 'absolute', inset: 0, background: 'rgba(0,0,0,.5)', display: 'grid', placeItems: 'center', zIndex: 40 }}>
      <div style={{ width, maxWidth: '90%', background: 'var(--surface-card)', borderRadius: 4, fontFamily: 'var(--font-ui)', boxShadow: '0 11px 15px -7px rgba(0,0,0,.2),0 24px 38px 3px rgba(0,0,0,.14)' }}>
        {title && <div style={{ padding: '16px 24px', fontSize: 'var(--size-h6)' }}>{title}</div>}
        {children !== undefined && (
          <div style={{ padding: '0 24px 20px', fontSize: 'var(--size-body)', color: 'rgba(255,255,255,.7)' }}>{children}</div>
        )}
        {actions && <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, padding: 8 }}>{actions}</div>}
      </div>
    </div>
  );
}
