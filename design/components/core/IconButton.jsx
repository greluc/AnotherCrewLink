import React from 'react';
import { Icon } from './Icon.jsx';

/** MUI IconButton, size="small": a 30px circular hit area with a hover wash. */
export function IconButton({ icon, size = 'small', color = 'var(--text-icon)', onClick, label, style, ...rest }) {
  const [hover, setHover] = React.useState(false);
  const box = size === 'small' ? 30 : 40;
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        width: box, height: box, display: 'grid', placeItems: 'center',
        border: 0, borderRadius: 'var(--radius-round)', cursor: 'pointer',
        background: hover ? 'rgba(255,255,255,0.08)' : 'transparent', padding: 0,
        ...style,
      }}
      {...rest}
    >
      <Icon name={icon} size={size === 'small' ? 20 : 24} color={color} />
    </button>
  );
}
