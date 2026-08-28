const { LaunchButton, OutlineButton } = window.ACL_9b5df9;

function Spinner({ size = 40 }) {
  return (
    <div style={{ width: size, height: size, borderRadius: '50%', border: '3.6px solid transparent', borderTopColor: 'var(--accent-primary)', borderRightColor: 'var(--accent-primary)', animation: 'acl-spin 1.4s linear infinite' }} />
  );
}

/** The MENU state: Among Us is not running. Menu.tsx + LaunchButton.tsx. */
function WaitingScreen({ error, onLaunch }) {
  if (error) {
    return (
      <div style={{ paddingTop: 32, textAlign: 'center' }}>
        <div style={{ font: 'var(--text-heading)', color: 'var(--text-danger)' }}>ERROR</div>
        <div style={{ whiteSpace: 'pre-wrap', fontSize: 14, marginTop: 8, padding: '0 16px' }}>{error}</div>
        <div style={{ marginTop: 16, fontSize: 14 }}>
          Need help?&nbsp;<a href="#" style={{ color: 'var(--acl-red-500)' }}>Get support</a>
        </div>
        <div style={{ marginTop: 8 }}><OutlineButton>Reload</OutlineButton></div>
      </div>
    );
  }
  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'flex-start', height: '100%' }}>
      <span style={{ fontSize: 20, marginTop: 12, marginBottom: 12 }}>Waiting for Among Us</span>
      <Spinner />
      <span style={{ fontSize: 24, marginTop: 15, marginBottom: 5 }}>Open via</span>
      <LaunchButton label="Steam" platforms={['Steam', 'Epic Games', 'Microsoft', 'Custom']} onLaunch={onLaunch} />
    </div>
  );
}
Object.assign(window, { WaitingScreen, Spinner });
