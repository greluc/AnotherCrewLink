import React from 'react';

/** The public lobby browser table: #1d1a23 head, rows alternating #25232a /
 *  #1d1a23, 14px body, sticky header, and a right-aligned action cell. */
export function LobbyTable({ columns = [], rows = [], renderAction }) {
  return (
    <div style={{ width: '100%', overflow: 'auto', background: 'var(--surface-card)' }}>
      <table style={{ width: '100%', minWidth: 700, borderCollapse: 'collapse', fontFamily: 'var(--font-ui)', fontSize: 'var(--size-body)' }}>
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c} style={{ background: 'var(--acl-bg-titlebar)', color: '#fff', textAlign: 'left', padding: '16px', fontWeight: 400, position: 'sticky', top: 0 }}>{c}</th>
            ))}
            {renderAction && <th style={{ background: 'var(--acl-bg-titlebar)', position: 'sticky', top: 0 }} />}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={r.id ?? i} style={{ background: i % 2 === 0 ? 'var(--acl-bg-row-odd)' : 'var(--acl-bg-row-even)' }}>
              {columns.map((c) => (
                <td key={c} style={{ padding: '16px', color: 'var(--text-body)', whiteSpace: 'nowrap' }}>{r[c]}</td>
              ))}
              {renderAction && <td style={{ padding: '8px 16px', textAlign: 'right', whiteSpace: 'nowrap' }}>{renderAction(r)}</td>}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
