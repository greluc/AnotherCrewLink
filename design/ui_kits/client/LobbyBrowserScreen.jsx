const { LobbyTable, Button, Tooltip, Dialog } = window.ACL_9b5df9;

const LOBBIES = [
  { id: 1, Title: 'chill euro lobby', Host: 'Red', Players: '7/15', Mods: 'None', Language: 'English', Status: 'Lobby 00:42', joinable: true, code: 'XKJDPQ' },
  { id: 2, Title: 'no mic no talk', Host: 'Lime', Players: '15/15', Mods: 'None', Language: 'English', Status: 'Lobby 02:03', joinable: false, reason: 'Lobby is Full' },
  { id: 3, Title: 'hide and seek', Host: 'Cyan', Players: '9/15', Mods: 'Town Of Us', Language: 'Deutsch', Status: 'In game 04:18', joinable: false, reason: 'Game in Progress' },
  { id: 4, Title: 'proximity practice', Host: 'Pink', Players: '4/10', Mods: 'None', Language: 'Français', Status: 'Lobby 00:11', joinable: true, code: 'PQMRTV' },
];

/** The public lobby browser — its own window. LobbyBrowser.tsx. */
function LobbyBrowserScreen() {
  const [code, setCode] = React.useState('');
  return (
    <div style={{ height: '100%', width: '100%', paddingTop: 15, boxSizing: 'border-box', position: 'relative' }}>
      <div style={{ padding: 20, boxSizing: 'border-box', height: '100%' }}>
        <b style={{ fontSize: 14 }}>Public Lobbies</b>
        <div style={{ marginTop: 12, maxHeight: 'calc(100% - 40px)', overflow: 'auto' }}>
          <LobbyTable
            columns={['Title', 'Host', 'Players', 'Mods', 'Language', 'Status']}
            rows={LOBBIES}
            renderAction={(r) => (
              <Tooltip title={r.joinable ? '' : r.reason}>
                <span>
                  <Button variant="contained" color="secondary" disabled={!r.joinable} onClick={() => setCode('Lobby Code: ' + r.code + ' \n Region: Europe')}>Show code</Button>
                </span>
              </Tooltip>
            )}
          />
        </div>
      </div>
      <Dialog open={!!code} title="Lobby information" actions={<Button onClick={() => setCode('')}>Close</Button>}>
        {code.split('\n').map((l, i) => <div key={i}>{l}</div>)}
      </Dialog>
    </div>
  );
}
Object.assign(window, { LobbyBrowserScreen });
