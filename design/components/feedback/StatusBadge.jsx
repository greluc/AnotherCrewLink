import React from 'react';
import { Icon } from '../core/Icon.jsx';

/** The round badge centred on an avatar when something is wrong with that player.
 *  Fill and border are a pair per state — from Avatar.tsx. */
const states = {
  muted: { icon: 'mic_off', bg: 'var(--acl-muted)', edge: 'var(--acl-muted-edge)' },
  deafened: { icon: 'volume_off', bg: 'var(--acl-muted)', edge: 'var(--acl-muted-edge)' },
  novoice: { icon: 'link_off', bg: 'var(--acl-novoice)', edge: 'var(--acl-novoice-edge)' },
  disconnected: { icon: 'wifi_off', bg: 'var(--acl-muted)', edge: 'var(--acl-muted-edge)' },
  bugged: { icon: 'error', bg: 'red', edge: 'transparent' },
};

export function StatusBadge({ state = 'muted', size = 20, style }) {
  const tone = states[state] || states.muted;
  return (
    <span style={{
      display: 'grid', placeItems: 'center', background: tone.bg,
      border: `var(--border-badge) solid ${tone.edge}`, borderRadius: 'var(--radius-round)',
      padding: 2, zIndex: 10, ...style,
    }}>
      <Icon name={tone.icon} size={size} color="#fff" />
    </span>
  );
}
