const { Crewmate, LobbyCode, IconButton, Divider, StatusBadge, Tooltip, Slider } = window.ACL_9b5df9;

// A host page (the mockups) may sit at another depth; it sets window.ACL_ASSETS.
const ASSETS = window.ACL_ASSETS || '../../assets';

const LOBBY = [
  { id: 2, name: 'Dummy 1', crew: 'lime', talking: true, hat: 'pk04_MinerCap.png' },
  { id: 3, name: 'Dummy 2', crew: 'blue' },
  { id: 4, name: 'Dummy 3', crew: 'pink', badge: 'novoice', hat: 'flowerCrownHat.png' },
  { id: 5, name: 'Yellow', crew: 'yellow', talking: true, hat: 'pk02_Crown.png' },
  { id: 6, name: 'Black', crew: 'black', alive: false },
  { id: 7, name: 'Orange', crew: 'orange', badge: 'muted', hat: 'pk03_Fedora.png' },
  { id: 8, name: 'White', crew: 'white', hat: 'pk02_ToiletPaperHat.png' },
  { id: 9, name: 'Brown', crew: 'brown', badge: 'disconnected' },
];

/** The VOICE state: in a lobby or a game. Voice.tsx's UI half. */
function VoiceScreen({ code = 'XKJDPQ', hideCode = false, muted, deafened, onToggleMute, onToggleDeafen, talking }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', boxSizing: 'border-box' }}>
      <div style={{ display: 'flex', justifyContent: 'center', alignItems: 'center' }}>
        <div style={{ float: 'left', width: 100, paddingLeft: 8 }}>
          <Crewmate size={80} assetBase={ASSETS} hat="pk01_Astronaut.png" color="var(--crew-purple)" shadow="var(--crew-purple-shadow)" talking={talking && !muted && !deafened} />
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', flex: 1, minWidth: 0 }}>
          <span style={{ display: 'block', textAlign: 'center', fontSize: 20, whiteSpace: 'nowrap', maxWidth: '100%', overflow: 'hidden', textOverflow: 'ellipsis' }}>Greluc</span>
          {/* The shadow half of the pair, not the body half — white 28px text on a bright
            crew tint burns out. See guidelines/type-mono.card.html. */}
        <LobbyCode code={code} hidden={hideCode} background="var(--crew-purple-shadow)" />
        </div>
        <div style={{ paddingLeft: 5, paddingTop: 26, display: 'grid' }}>
          <IconButton icon={muted ? 'mic_off' : 'mic'} label="Mute" color={muted ? 'var(--state-muted)' : '#fff'} onClick={onToggleMute} />
          <IconButton icon={deafened ? 'volume_off' : 'volume_up'} label="Deafen" color={deafened ? 'var(--state-muted)' : '#fff'} onClick={onToggleDeafen} />
        </div>
      </div>
      <Divider spacing={8} />
      <div style={{ flex: '1 1 auto', minHeight: 0, overflowY: 'auto', display: 'flex', flexWrap: 'wrap', justifyContent: 'center', alignContent: 'flex-start', margin: '4px auto', width: '100%' }}>
        {LOBBY.map((p) => (
          <div key={p.id} style={{ width: '32%', minWidth: 60, maxWidth: 120, padding: 8, boxSizing: 'border-box' }}>
            <Tooltip title={<PeerControls name={p.name} />}>
              <div style={{ position: 'relative', width: '100%' }}>
                <Crewmate size={78} assetBase={ASSETS} hat={p.hat} color={'var(--crew-' + p.crew + ')'} shadow={'var(--crew-' + p.crew + '-shadow)'} talking={!!p.talking} alive={p.alive !== false} />
                {p.badge && <StatusBadge state={p.badge} style={{ position: 'absolute', left: '50%', top: '50%', transform: 'translate(-50%,-50%)' }} />}
              </div>
            </Tooltip>
            <div style={{ textAlign: 'center', fontSize: 12, marginTop: 2, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{p.name}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

/** What a player's tooltip holds: their name, a mute toggle and their volume. */
function PeerControls({ name }) {
  const [vol, setVol] = React.useState(100);
  const [off, setOff] = React.useState(false);
  return (
    <div style={{ textAlign: 'center', minWidth: 120 }}>
      <b>{name}</b>
      <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        <IconButton icon={off ? 'volume_off' : 'volume_up'} label="Mute peer" color="var(--accent-primary)" onClick={() => setOff(!off)} />
        <div style={{ flex: 1 }}><Slider value={vol} min={0} max={200} onChange={setVol} /></div>
      </div>
    </div>
  );
}
Object.assign(window, { VoiceScreen, PeerControls });
