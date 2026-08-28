import React from 'react';

/** MUI Alert, dark variant. The client uses `error` for the voice-server warning,
 *  `info` for "Exit settings to apply changes" and `success` after resetting offsets. */
const tones = {
  error: { fg: '#f4c7c3', bg: 'rgba(244,67,54,.16)', icon: 'error' },
  info: { fg: '#c5e1f5', bg: 'rgba(41,182,246,.16)', icon: 'info' },
  success: { fg: '#c8e6c9', bg: 'rgba(102,187,106,.16)', icon: 'check_circle' },
  warning: { fg: '#ffe0b2', bg: 'rgba(230,126,34,.16)', icon: 'warning' },
};

export function Alert({ severity = 'info', children, style }) {
  const tone = tones[severity] || tones.info;
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 10, padding: '6px 16px', borderRadius: 4,
      background: tone.bg, color: tone.fg, fontFamily: 'var(--font-ui)', fontSize: 'var(--size-body)',
      lineHeight: 1.43, ...style,
    }}>
      <span className="acl-icon" style={{ color: tone.fg, fontSize: 22 }}>{tone.icon}</span>
      <span>{children}</span>
    </div>
  );
}
