const { SectionHeading, Divider, Checkbox, RadioOption, Slider, SelectField, TextField, Button, Alert, IconButton, Tooltip, MeterBar } = window.ACL_9b5df9;

/** The settings panel: a scrim over the whole window below the title bar, slid in
 *  from the left. Settings.tsx, in its own section order. */
function SettingsScreen({ open, onClose }) {
  const [distance, setDistance] = React.useState(5.3);
  const [rules, setRules] = React.useState({ publicLobby: false, walls: true, vision: false, haunting: false, ventsHear: false, ventsPrivate: false, comms: false, cameras: false, radio: false, ghostOnly: false, meetingsOnly: false });
  const [mode, setMode] = React.useState(0);
  const [overlay, setOverlay] = React.useState(true);
  const set = (k) => (v) => setRules((r) => ({ ...r, [k]: v }));
  const hostOnly = 'Only the game host can change this!';
  return (
    <div style={{
      position: 'absolute', left: 0, top: 'var(--titlebar-h)', width: '100%',
      height: 'calc(100% - var(--titlebar-h))', background: 'var(--surface-scrim)',
      backdropFilter: 'var(--blur-scrim)', zIndex: 99, transition: 'var(--transition-panel)',
      transform: open ? 'translateX(0)' : 'translateX(-100%)', boxSizing: 'border-box',
    }}>
      <div style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: 40, position: 'relative' }}>
        <div style={{ position: 'absolute', right: 8 }}><IconButton icon="arrow_back" label="Back" onClick={onClose} /></div>
        <span style={{ font: 'var(--text-heading)' }}>Settings</span>
      </div>
      <div style={{ height: 'calc(100% - 40px)', overflowY: 'auto', padding: '8px 16px 56px', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 4, boxSizing: 'border-box' }}>
        <SectionHeading>Lobby Settings</SectionHeading>
        <Slider label="Voice Distance" suffix={': ' + distance.toFixed(1)} value={distance} min={1} max={10} step={0.1} onChange={setDistance} />
        <div style={{ width: '100%' }}>
          <Checkbox label="The Lobby is Public" checked={rules.publicLobby} onChange={set('publicLobby')} />
          <Checkbox label="Walls Block Audio" checked={rules.walls} onChange={set('walls')} />
          <Checkbox label="Hear People in Vision Only" checked={rules.vision} onChange={set('vision')} />
          <Checkbox label="Impostors Hear Dead" checked={rules.haunting} onChange={set('haunting')} />
          <Checkbox label="Hear Impostors in Vents" checked={rules.ventsHear} onChange={set('ventsHear')} />
          <Checkbox label="Private Talk in Vents" checked={rules.ventsPrivate} onChange={set('ventsPrivate')} />
          <Checkbox label="Comms Sabotage Disables Voice" checked={rules.comms} onChange={set('comms')} />
          <Tooltip title={hostOnly}><span style={{ display: 'block', width: '100%' }}><Checkbox label="Hear Through Cameras" checked={false} disabled /></span></Tooltip>
          <Checkbox label="Impostor Radio" checked={rules.radio} onChange={set('radio')} />
          <Checkbox label="Only Ghosts can Talk/Hear" checked={rules.ghostOnly} onChange={set('ghostOnly')} />
          <Checkbox label="Meetings/Lobby Only" checked={rules.meetingsOnly} onChange={set('meetingsOnly')} />
        </div>
        <Divider />
        <SectionHeading>Audio</SectionHeading>
        <SelectField label="Microphone" value="default" options={[{ value: 'default', label: 'Default' }, { value: 'usb', label: 'Yeti Nano (USB)' }]} />
        <MeterBar value={38} />
        <SelectField label="Speaker" value="default" options={[{ value: 'default', label: 'Default' }]} />
        <div style={{ alignSelf: 'flex-start' }}>
          <RadioOption label="Voice Activity" value={0} selected={mode === 0} onSelect={setMode} />
          <RadioOption label="Push to Talk" value={1} selected={mode === 1} onSelect={setMode} />
          <RadioOption label="Push to Mute" value={2} selected={mode === 2} onSelect={setMode} />
        </div>
        <Divider />
        <div style={{ width: '100%', display: 'flex', flexDirection: 'column', gap: 8 }}>
          <div style={{ display: 'grid', gridTemplateColumns: 'min-content minmax(0,1fr)', alignItems: 'center', gap: 8 }}>
            <Checkbox label="" checked divided={false} onChange={() => {}} />
            <Slider label="Microphone Volume" value={100} max={300} step={2} onChange={() => {}} />
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: 'min-content minmax(0,1fr)', alignItems: 'center', gap: 8 }}>
            <Checkbox label="" checked={false} divided={false} onChange={() => {}} />
            <Slider label="Microphone Sensitivity" value={30} max={100} disabled onChange={() => {}} />
          </div>
          <Slider label="Master Volume" value={100} max={200} onChange={() => {}} />
          <Slider label="Crew Volume as Ghost" value={100} onChange={() => {}} />
        </div>
        <Divider />
        <SectionHeading>Keyboard Shortcuts</SectionHeading>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, width: '100%' }}>
          <TextField label="Push to Talk" value="V" readOnly />
          <TextField label="Impostor Radio" value="B" readOnly />
          <TextField label="Mute" value="RControl" readOnly />
          <TextField label="Deafen" value="RAlt" readOnly />
        </div>
        <Divider />
        <SectionHeading>Overlay</SectionHeading>
        <div style={{ width: '100%' }}>
          <Checkbox label="AnotherCrewLink on Top" checked onChange={() => {}} />
          <Checkbox label="Enable Overlay" checked={overlay} onChange={setOverlay} />
          {overlay && (
            <>
              <Checkbox label="Compact Overlay" checked={false} onChange={() => {}} />
              <Checkbox label="Meeting Overlay" checked onChange={() => {}} />
              <SelectField label="Overlay Position" value="top" options={[{ value: 'hidden', label: 'Hidden' }, { value: 'top', label: 'Top Center' }, { value: 'bottom_left', label: 'Bottom Left' }, { value: 'right', label: 'Right' }, { value: 'left', label: 'Left' }]} />
            </>
          )}
        </div>
        <Divider />
        <SectionHeading>Advanced</SectionHeading>
        <div style={{ width: '100%' }}><Checkbox label="NAT Fix" checked={false} divided={false} onChange={() => {}} /></div>
        <Button variant="contained" color="secondary">Change Voice Server</Button>
        <Divider />
        <SectionHeading>BETA/DEBUG</SectionHeading>
        <div style={{ width: '100%' }}>
          <Checkbox label="VAD Enabled" checked onChange={() => {}} />
          <Checkbox label="Hardware Acceleration" checked onChange={() => {}} />
          <Checkbox label="Echo Cancellation" checked={false} onChange={() => {}} />
          <Checkbox label="Spatial Audio" checked onChange={() => {}} />
          <Checkbox label="Noise Suppression" checked={false} onChange={() => {}} />
        </div>
        <SelectField label="Language" value="en" options={[{ value: 'en', label: 'English' }, { value: 'de', label: 'Deutsch' }]} />
        <Divider />
        <SectionHeading>Troubleshooting</SectionHeading>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <Button variant="contained" color="secondary">Restore to Default</Button>
          <Button variant="contained" color="secondary">Reset game offsets</Button>
        </div>
        <div style={{ marginTop: 12, width: '100%' }}><Alert severity="info">Exit settings to apply changes</Alert></div>
      </div>
    </div>
  );
}
Object.assign(window, { SettingsScreen });
