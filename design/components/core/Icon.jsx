import React from 'react';

/** Material Symbols Rounded glyph. The client uses @mui/icons-material, which is
 *  the same icon set; this wrapper is the CDN-delivered stand-in. */
export function Icon({ name, size = 20, color = 'var(--text-icon)', style, ...rest }) {
  return (
    <span
      className="acl-icon"
      aria-hidden="true"
      style={{ fontSize: size, color, ...style }}
      {...rest}
    >
      {name}
    </span>
  );
}
