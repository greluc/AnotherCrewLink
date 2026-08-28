import React from 'react';
import { IconButton } from '../core/IconButton.jsx';

/** The frameless window's own title bar: 24px tall, #1d1a23, the app name centred
 *  in purple, three #777 icon buttons — settings and reload at the left, close at
 *  the right — and a 4px non-draggable strip above it so the window stays resizable. */
export function TitleBar({ version = '', onSettings, onReload, onClose, title = 'AnotherCrewLink' }) {
  return (
    <div style={{ position: 'relative', width: '100%', height: 'var(--titlebar-h)', background: 'var(--surface-chrome)', zIndex: 100 }}>
      <div style={{ position: 'absolute', top: 0, left: 0, right: 0, height: 'var(--resize-strip-h)' }} />
      <span style={{
        display: 'block', width: '100%', textAlign: 'center', height: 'var(--titlebar-h)',
        lineHeight: 'var(--titlebar-h)', color: 'var(--text-title)', fontFamily: 'var(--font-ui)', fontSize: 'var(--size-body)',
      }}>{title}{version ? ` v${version}` : ''}</span>
      <div style={{ position: 'absolute', top: -3, left: 0, display: 'flex' }}>
        <IconButton icon="settings" label="Settings" onClick={onSettings} />
        <IconButton icon="refresh" label="Reload" onClick={onReload} />
      </div>
      <div style={{ position: 'absolute', top: -3, right: 0 }}>
        <IconButton icon="close" label="Close" onClick={onClose} />
      </div>
    </div>
  );
}
