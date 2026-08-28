import React from 'react';

/** The reload / support button: 2px white outline, 10px radius, green on hover.
 *  From src/renderer/SupportLink.tsx. */
export function OutlineButton({ children, onClick, size = 19, disabled = false, style, ...rest }) {
  const [hover, setHover] = React.useState(false);
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        color: '#fff',
        background: 'none',
        padding: '2px 10px',
        borderRadius: 'var(--radius-lg)',
        border: `var(--border-button) solid ${hover && !disabled ? 'var(--accent-action)' : '#fff'}`,
        fontSize: size,
        fontWeight: 500,
        fontFamily: 'var(--font-ui)',
        outline: 'none',
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.4 : 1,
        transition: 'var(--transition-border)',
        ...style,
      }}
      {...rest}
    >
      {children}
    </button>
  );
}
