const { Crewmate } = window.ACL_9b5df9;
const ASSETS = window.ACL_ASSETS || '../../assets';

/** The discussion tablet's own layer. Overlay.tsx positions tiles against the game's
 *  meeting hud — two ratio regimes, the old iPad one at 854/579 and the current one at
 *  ~1.72 — and gives each talking player a glow keyed to their crew colour:
 *  `box-shadow: 0 0 h/100 h/100 <colour>`, faded in over 400ms. Nothing else is drawn. */
const SEATS = [
  { name: 'Dummy 1', crew: 'lime', talking: true, hat: 'pk04_MinerCap.png' },
  { name: 'Dummy 2', crew: 'blue' },
  { name: 'Dummy 3', crew: 'pink', talking: true, hat: 'flowerCrownHat.png' },
  { name: 'Yellow', crew: 'yellow', hat: 'pk02_Crown.png' },
  { name: 'Black', crew: 'black', alive: false },
  { name: 'Orange', crew: 'orange', hat: 'pk03_Fedora.png' },
  { name: 'White', crew: 'white', hat: 'pk02_ToiletPaperHat.png' },
  { name: 'Brown', crew: 'brown' },
  { name: 'Red', crew: 'red', hat: 'pk01_Astronaut.png' },
  { name: 'Purple', crew: 'purple' },
];

function MeetingOverlay({ height = 720, players = SEATS }) {
  // The glow is derived from the viewport height, not from a fixed px value: the tablet
  // scales with the game window and a fixed blur would swamp it at 1080p.
  const glow = height / 100;
  const avatar = height * 0.075;
  return (
    <div style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}>
      <div style={{
        position: 'absolute', left: '50%', top: '50%', transform: 'translate(-50%,-50%)',
        width: '58%', display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)',
        rowGap: height * 0.022, columnGap: '12%',
      }}>
        {players.map((p) => (
          <div key={p.name} style={{
            display: 'flex', alignItems: 'center', gap: 10, padding: 6,
            borderRadius: 'var(--radius-md)',
            boxShadow: p.talking ? `0 0 ${glow}px ${glow}px var(--crew-${p.crew})` : 'none',
            background: p.talking ? 'rgba(0,0,0,.28)' : 'transparent',
            opacity: p.talking ? 1 : 0.9,
            transition: 'var(--transition-fade)',
          }}>
            <Crewmate size={avatar} assetBase={ASSETS} hat={p.hat} alive={p.alive !== false}
              color={'var(--crew-' + p.crew + ')'} shadow={'var(--crew-' + p.crew + '-shadow)'} />
            <span style={{
              font: 'var(--text-ui)', fontSize: Math.max(11, height * 0.019), color: '#fff',
              background: 'rgba(0,0,0,.32)', borderRadius: 'var(--radius-pill)', padding: '2px 10px',
              whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: '9ch',
            }}>{p.name}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
Object.assign(window, { MeetingOverlay, MEETING_SEATS: SEATS });
