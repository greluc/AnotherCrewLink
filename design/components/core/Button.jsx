import React from 'react';

const base = {
  fontFamily: 'var(--font-ui)',
  fontSize: '0.875rem',
  fontWeight: 500,
  letterSpacing: '0.02857em',
  textTransform: 'uppercase',
  borderRadius: '4px',
  padding: '6px 16px',
  border: 0,
  cursor: 'pointer',
  lineHeight: 1.75,
  whiteSpace: 'nowrap',
  transition: 'background-color var(--dur-base) var(--ease-out), color var(--dur-base) var(--ease-out)',
};

const palette = {
  primary: { main: 'var(--acl-purple-300)', hover: 'rgba(186,104,200,0.12)', contrast: '#fff', contained: 'var(--acl-purple-500)', containedHover: 'var(--acl-purple-700)' },
  secondary: { main: 'var(--acl-red-500)', hover: 'rgba(244,67,54,0.12)', contrast: '#fff', contained: 'var(--acl-red-500)', containedHover: 'var(--acl-red-700)' },
  grey: { main: 'var(--acl-grey-300)', hover: 'rgba(224,224,224,0.12)', contrast: '#1d1a23', contained: 'var(--acl-grey-300)', containedHover: 'var(--acl-grey-400)' },
};

/** MUI's Button as the client configures it: text buttons in dialogs, contained
 *  secondary buttons for destructive or navigational actions. */
export function Button({ children, variant = 'text', color = 'primary', disabled = false, onClick, style, ...rest }) {
  const tone = palette[color] || palette.primary;
  const [hover, setHover] = React.useState(false);
  const contained = variant === 'contained';
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        ...base,
        background: contained ? (hover && !disabled ? tone.containedHover : tone.contained) : (hover && !disabled ? tone.hover : 'transparent'),
        color: contained ? tone.contrast : tone.main,
        boxShadow: contained ? '0 3px 1px -2px rgba(0,0,0,.2),0 2px 2px 0 rgba(0,0,0,.14),0 1px 5px 0 rgba(0,0,0,.12)' : 'none',
        opacity: disabled ? 0.38 : 1,
        cursor: disabled ? 'default' : 'pointer',
        ...style,
      }}
      {...rest}
    >
      {children}
    </button>
  );
}
