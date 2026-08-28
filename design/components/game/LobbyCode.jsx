import React from 'react';

/** The lobby code: Source Code Pro, 28px, 5px radius, tinted with the local
 *  player's crew colour. Reads "LOBBY" when the streaming setting hides it. */
export function LobbyCode({ code = 'ABCDEF', background = 'var(--crew-purple)', hidden = false }) {
  return (
    <span style={{
      fontFamily: 'var(--font-mono)', fontWeight: 500, fontSize: 'var(--size-code)',
      display: 'block', width: 'fit-content', margin: '5px auto', padding: 5,
      borderRadius: 'var(--radius-sm)', background, color: '#fff', letterSpacing: 'var(--tracking-code)',
    }}>{hidden ? 'LOBBY' : code}</span>
  );
}
