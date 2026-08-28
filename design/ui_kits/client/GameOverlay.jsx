const { OverlayCapsule, PlayerSlot } = window.ACL_9b5df9;

// A host page (the mockups) may sit at another depth; it sets window.ACL_ASSETS.
const ASSETS = window.ACL_ASSETS || '../../assets';

const PLAYERS = [
  { name: 'Lime', crew: 'lime', talking: true, hat: 'pk04_MinerCap.png' },
  { name: 'Blue', crew: 'blue' },
  { name: 'Pink', crew: 'pink', talking: true, hat: 'flowerCrownHat.png' },
  { name: 'Yellow', crew: 'yellow', hat: 'pk02_Crown.png' },
];

/** The in-game overlay, in each position the setting offers. Overlay.tsx +
 *  css/overlay.css. Nothing here is interactive: it is drawn over the game. */
function GameOverlay({ position = 'top', compact = false }) {
  const shown = compact ? PLAYERS.filter((p) => p.talking) : PLAYERS;
  // Overlay.tsx: showName = isOnSide && (!compact || the `1` variants). Names never
  // appear on the top or bottom-left positions.
  const showName = position === 'left' || position === 'right';
  const slots = shown.map((p) => (
    <PlayerSlot key={p.name} name={showName ? p.name : ''} size={44} slot={60} talking={!!p.talking}
      assetBase={ASSETS} hat={p.hat} color={'var(--crew-' + p.crew + ')'} shadow={'var(--crew-' + p.crew + '-shadow)'} />
  ));
  const place = {
    top: { top: 0, left: '50%', transform: 'translateX(-50%)' },
    bottom_left: { bottom: 0, left: 0 },
    left: { left: 0, top: '50%', transform: 'translateY(-50%)' },
    right: { right: 0, top: '50%', transform: 'translateY(-50%)' },
  }[position];
  return (
    <div style={{ position: 'absolute', ...place }}>
      <OverlayCapsule position={position} compact={compact}
        style={position === 'left' || position === 'right' ? { flexDirection: 'column' } : undefined}>
        {slots}
      </OverlayCapsule>
    </div>
  );
}
Object.assign(window, { GameOverlay });
