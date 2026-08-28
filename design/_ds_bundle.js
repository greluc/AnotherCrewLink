/* @ds-bundle: {"format":4,"namespace":"ACL_9b5df9","components":[{"name":"Button","sourcePath":"components/core/Button.jsx"},{"name":"Divider","sourcePath":"components/core/Divider.jsx"},{"name":"Icon","sourcePath":"components/core/Icon.jsx"},{"name":"IconButton","sourcePath":"components/core/IconButton.jsx"},{"name":"LaunchButton","sourcePath":"components/core/LaunchButton.jsx"},{"name":"OutlineButton","sourcePath":"components/core/OutlineButton.jsx"},{"name":"SectionHeading","sourcePath":"components/core/SectionHeading.jsx"},{"name":"Alert","sourcePath":"components/feedback/Alert.jsx"},{"name":"Dialog","sourcePath":"components/feedback/Dialog.jsx"},{"name":"MeterBar","sourcePath":"components/feedback/MeterBar.jsx"},{"name":"StatusBadge","sourcePath":"components/feedback/StatusBadge.jsx"},{"name":"Tooltip","sourcePath":"components/feedback/Tooltip.jsx"},{"name":"Checkbox","sourcePath":"components/forms/Checkbox.jsx"},{"name":"RadioOption","sourcePath":"components/forms/RadioOption.jsx"},{"name":"SelectField","sourcePath":"components/forms/SelectField.jsx"},{"name":"Slider","sourcePath":"components/forms/Slider.jsx"},{"name":"TextField","sourcePath":"components/forms/TextField.jsx"},{"name":"HAT_COLLECTION_COMMIT","sourcePath":"components/game/Crewmate.jsx"},{"name":"HAT_COLLECTION_URL","sourcePath":"components/game/Crewmate.jsx"},{"name":"COSMETIC_DEFAULTS","sourcePath":"components/game/Crewmate.jsx"},{"name":"Crewmate","sourcePath":"components/game/Crewmate.jsx"},{"name":"LobbyCode","sourcePath":"components/game/LobbyCode.jsx"},{"name":"OverlayCapsule","sourcePath":"components/game/OverlayCapsule.jsx"},{"name":"PlayerSlot","sourcePath":"components/game/PlayerSlot.jsx"},{"name":"LobbyTable","sourcePath":"components/navigation/LobbyTable.jsx"},{"name":"TitleBar","sourcePath":"components/navigation/TitleBar.jsx"}],"sourceHashes":{"components/core/Button.jsx":"76d9f89176c7","components/core/Divider.jsx":"93727832c2fd","components/core/Icon.jsx":"dc8a3f442647","components/core/IconButton.jsx":"2aef416b6fd0","components/core/LaunchButton.jsx":"2134ff3af6d0","components/core/OutlineButton.jsx":"c729a20de904","components/core/SectionHeading.jsx":"1904c124bd2f","components/feedback/Alert.jsx":"8ba15a2f8f05","components/feedback/Dialog.jsx":"fdc5d28a5075","components/feedback/MeterBar.jsx":"a0b164c79dc1","components/feedback/StatusBadge.jsx":"987ae7e9a3a7","components/feedback/Tooltip.jsx":"74a487f78e35","components/forms/Checkbox.jsx":"0604ac70f8fa","components/forms/RadioOption.jsx":"25a90bd7ba00","components/forms/SelectField.jsx":"4c461179a3f6","components/forms/Slider.jsx":"e5efdbb94e82","components/forms/TextField.jsx":"0fe1bf5a48ea","components/game/Crewmate.jsx":"8c963e86f510","components/game/LobbyCode.jsx":"e27c52fd833a","components/game/OverlayCapsule.jsx":"ddd575851dc6","components/game/PlayerSlot.jsx":"de1f2a1534e2","components/navigation/LobbyTable.jsx":"ffbff34d0cb7","components/navigation/TitleBar.jsx":"e9479d12d4ee","mockups/mockup-frame.js":"557cb1455f0b","ui_kits/client/GameOverlay.jsx":"7c9198e5d2d1","ui_kits/client/LobbyBrowserScreen.jsx":"acbad2f721cf","ui_kits/client/MeetingOverlay.jsx":"d8a8e59998ee","ui_kits/client/SettingsScreen.jsx":"1199984801b2","ui_kits/client/VoiceScreen.jsx":"78c283e467e5","ui_kits/client/WaitingScreen.jsx":"d090527a2a68"},"inlinedExternals":[],"unexposedExports":[{"name":"cosmeticUrl","sourcePath":"components/game/Crewmate.jsx"}]} */

(() => {

const __ds_ns = (window.ACL_9b5df9 = window.ACL_9b5df9 || {});

const __ds_scope = {};

(__ds_ns.__errors = __ds_ns.__errors || []);

// components/core/Button.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
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
  transition: 'background-color var(--dur-base) var(--ease-out), color var(--dur-base) var(--ease-out)'
};
const palette = {
  primary: {
    main: 'var(--acl-purple-300)',
    hover: 'rgba(186,104,200,0.12)',
    contrast: '#fff',
    contained: 'var(--acl-purple-500)',
    containedHover: 'var(--acl-purple-700)'
  },
  secondary: {
    main: 'var(--acl-red-500)',
    hover: 'rgba(244,67,54,0.12)',
    contrast: '#fff',
    contained: 'var(--acl-red-500)',
    containedHover: 'var(--acl-red-700)'
  },
  grey: {
    main: 'var(--acl-grey-300)',
    hover: 'rgba(224,224,224,0.12)',
    contrast: '#1d1a23',
    contained: 'var(--acl-grey-300)',
    containedHover: 'var(--acl-grey-400)'
  }
};

/** MUI's Button as the client configures it: text buttons in dialogs, contained
 *  secondary buttons for destructive or navigational actions. */
function Button({
  children,
  variant = 'text',
  color = 'primary',
  disabled = false,
  onClick,
  style,
  ...rest
}) {
  const tone = palette[color] || palette.primary;
  const [hover, setHover] = React.useState(false);
  const contained = variant === 'contained';
  return /*#__PURE__*/React.createElement("button", _extends({
    type: "button",
    disabled: disabled,
    onClick: onClick,
    onMouseEnter: () => setHover(true),
    onMouseLeave: () => setHover(false),
    style: {
      ...base,
      background: contained ? hover && !disabled ? tone.containedHover : tone.contained : hover && !disabled ? tone.hover : 'transparent',
      color: contained ? tone.contrast : tone.main,
      boxShadow: contained ? '0 3px 1px -2px rgba(0,0,0,.2),0 2px 2px 0 rgba(0,0,0,.14),0 1px 5px 0 rgba(0,0,0,.12)' : 'none',
      opacity: disabled ? 0.38 : 1,
      cursor: disabled ? 'default' : 'pointer',
      ...style
    }
  }, rest), children);
}
Object.assign(__ds_scope, { Button });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/Button.jsx", error: String((e && e.message) || e) }); }

// components/core/Divider.jsx
try { (() => {
/** MUI Divider as Settings.tsx styles it: full width, 16px of air either side. */
function Divider({
  spacing = 16,
  style
}) {
  return /*#__PURE__*/React.createElement("hr", {
    style: {
      width: '100%',
      border: 0,
      borderTop: '1px solid rgba(255,255,255,0.12)',
      margin: `${spacing}px 0`,
      ...style
    }
  });
}
Object.assign(__ds_scope, { Divider });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/Divider.jsx", error: String((e && e.message) || e) }); }

// components/core/Icon.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** Material Symbols Rounded glyph. The client uses @mui/icons-material, which is
 *  the same icon set; this wrapper is the CDN-delivered stand-in. */
function Icon({
  name,
  size = 20,
  color = 'var(--text-icon)',
  style,
  ...rest
}) {
  return /*#__PURE__*/React.createElement("span", _extends({
    className: "acl-icon",
    "aria-hidden": "true",
    style: {
      fontSize: size,
      color,
      ...style
    }
  }, rest), name);
}
Object.assign(__ds_scope, { Icon });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/Icon.jsx", error: String((e && e.message) || e) }); }

// components/core/IconButton.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** MUI IconButton, size="small": a 30px circular hit area with a hover wash. */
function IconButton({
  icon,
  size = 'small',
  color = 'var(--text-icon)',
  onClick,
  label,
  style,
  ...rest
}) {
  const [hover, setHover] = React.useState(false);
  const box = size === 'small' ? 30 : 40;
  return /*#__PURE__*/React.createElement("button", _extends({
    type: "button",
    "aria-label": label,
    onClick: onClick,
    onMouseEnter: () => setHover(true),
    onMouseLeave: () => setHover(false),
    style: {
      width: box,
      height: box,
      display: 'grid',
      placeItems: 'center',
      border: 0,
      borderRadius: 'var(--radius-round)',
      cursor: 'pointer',
      background: hover ? 'rgba(255,255,255,0.08)' : 'transparent',
      padding: 0,
      ...style
    }
  }, rest), /*#__PURE__*/React.createElement(__ds_scope.Icon, {
    name: icon,
    size: size === 'small' ? 20 : 24,
    color: color
  }));
}
Object.assign(__ds_scope, { IconButton });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/IconButton.jsx", error: String((e && e.message) || e) }); }

// components/core/LaunchButton.jsx
try { (() => {
/** The split launch control from src/renderer/LaunchButton.tsx: a wide primary
 *  button and a dropdown toggle that share a 4px white border. */
function LaunchButton({
  label = 'Steam',
  platforms = [],
  disabled = false,
  onLaunch,
  onSelect
}) {
  const [open, setOpen] = React.useState(false);
  const [hoverMain, setHoverMain] = React.useState(false);
  const [hoverDrop, setHoverDrop] = React.useState(false);
  const green = 'var(--accent-action)';
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative',
      display: 'inline-flex',
      margin: '0 10px'
    }
  }, /*#__PURE__*/React.createElement("button", {
    type: "button",
    disabled: disabled,
    onClick: onLaunch,
    onMouseEnter: () => setHoverMain(true),
    onMouseLeave: () => setHoverMain(false),
    style: {
      color: '#fff',
      background: 'none',
      padding: '2px 10px',
      borderRadius: '10px 0 0 10px',
      borderWidth: '4px 2px 4px 4px',
      borderStyle: 'solid',
      borderColor: hoverMain && !disabled ? green : '#fff',
      fontSize: 24,
      fontWeight: 500,
      fontFamily: 'var(--font-ui)',
      outline: 'none',
      textTransform: 'none',
      cursor: disabled ? 'default' : 'pointer',
      opacity: disabled ? 0.5 : 1,
      transition: 'var(--transition-border)'
    }
  }, label), /*#__PURE__*/React.createElement("button", {
    type: "button",
    onClick: () => setOpen(o => !o),
    onMouseEnter: () => setHoverDrop(true),
    onMouseLeave: () => setHoverDrop(false),
    style: {
      color: '#fff',
      background: 'none',
      padding: 0,
      minWidth: 40,
      borderRadius: '0 10px 10px 0',
      borderWidth: '4px 4px 4px 2px',
      borderStyle: 'solid',
      borderColor: open || hoverDrop ? green : '#fff',
      cursor: 'pointer',
      outline: 'none',
      transition: 'var(--transition-border)',
      display: 'grid',
      placeItems: 'center',
      fontFamily: 'var(--font-icon)',
      fontSize: 24
    },
    "aria-label": "Choose platform"
  }, "arrow_drop_down"), open && /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      top: '100%',
      right: 0,
      marginTop: 2,
      zIndex: 5,
      maxHeight: 110,
      overflow: 'auto',
      minWidth: 140,
      border: '1px solid var(--acl-border-soft)',
      background: 'var(--surface-card)'
    }
  }, platforms.map(p => /*#__PURE__*/React.createElement("div", {
    key: p,
    onClick: () => {
      onSelect && onSelect(p);
      setOpen(false);
    },
    style: {
      padding: '6px 16px',
      fontFamily: 'var(--font-ui)',
      fontSize: 14,
      cursor: 'pointer'
    },
    onMouseEnter: e => {
      e.currentTarget.style.background = 'rgba(255,255,255,.08)';
    },
    onMouseLeave: e => {
      e.currentTarget.style.background = 'transparent';
    }
  }, p))));
}
Object.assign(__ds_scope, { LaunchButton });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/LaunchButton.jsx", error: String((e && e.message) || e) }); }

// components/core/OutlineButton.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** The reload / support button: 2px white outline, 10px radius, green on hover.
 *  From src/renderer/SupportLink.tsx. */
function OutlineButton({
  children,
  onClick,
  size = 19,
  disabled = false,
  style,
  ...rest
}) {
  const [hover, setHover] = React.useState(false);
  return /*#__PURE__*/React.createElement("button", _extends({
    type: "button",
    disabled: disabled,
    onClick: onClick,
    onMouseEnter: () => setHover(true),
    onMouseLeave: () => setHover(false),
    style: {
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
      ...style
    }
  }, rest), children);
}
Object.assign(__ds_scope, { OutlineButton });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/OutlineButton.jsx", error: String((e && e.message) || e) }); }

// components/core/SectionHeading.jsx
try { (() => {
/** MUI Typography variant h6 — the only heading level the client uses. */
function SectionHeading({
  children,
  align = 'center',
  style
}) {
  return /*#__PURE__*/React.createElement("h2", {
    style: {
      font: 'var(--text-heading)',
      margin: 0,
      textAlign: align,
      color: 'var(--text-body)',
      ...style
    }
  }, children);
}
Object.assign(__ds_scope, { SectionHeading });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/SectionHeading.jsx", error: String((e && e.message) || e) }); }

// components/feedback/Alert.jsx
try { (() => {
/** MUI Alert, dark variant. The client uses `error` for the voice-server warning,
 *  `info` for "Exit settings to apply changes" and `success` after resetting offsets. */
const tones = {
  error: {
    fg: '#f4c7c3',
    bg: 'rgba(244,67,54,.16)',
    icon: 'error'
  },
  info: {
    fg: '#c5e1f5',
    bg: 'rgba(41,182,246,.16)',
    icon: 'info'
  },
  success: {
    fg: '#c8e6c9',
    bg: 'rgba(102,187,106,.16)',
    icon: 'check_circle'
  },
  warning: {
    fg: '#ffe0b2',
    bg: 'rgba(230,126,34,.16)',
    icon: 'warning'
  }
};
function Alert({
  severity = 'info',
  children,
  style
}) {
  const tone = tones[severity] || tones.info;
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 10,
      padding: '6px 16px',
      borderRadius: 4,
      background: tone.bg,
      color: tone.fg,
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--size-body)',
      lineHeight: 1.43,
      ...style
    }
  }, /*#__PURE__*/React.createElement("span", {
    className: "acl-icon",
    style: {
      color: tone.fg,
      fontSize: 22
    }
  }, tone.icon), /*#__PURE__*/React.createElement("span", null, children));
}
Object.assign(__ds_scope, { Alert });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/Alert.jsx", error: String((e && e.message) || e) }); }

// components/feedback/Dialog.jsx
try { (() => {
/** MUI Dialog: a paper panel over a scrim, actions right-aligned. The client's
 *  dialogs are confirmations, the updater and the lobby-code reveal. */
function Dialog({
  open = true,
  title,
  children,
  actions,
  width = 320
}) {
  if (!open) return null;
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      background: 'rgba(0,0,0,.5)',
      display: 'grid',
      placeItems: 'center',
      zIndex: 40
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width,
      maxWidth: '90%',
      background: 'var(--surface-card)',
      borderRadius: 4,
      fontFamily: 'var(--font-ui)',
      boxShadow: '0 11px 15px -7px rgba(0,0,0,.2),0 24px 38px 3px rgba(0,0,0,.14)'
    }
  }, title && /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '16px 24px',
      fontSize: 'var(--size-h6)'
    }
  }, title), children !== undefined && /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '0 24px 20px',
      fontSize: 'var(--size-body)',
      color: 'rgba(255,255,255,.7)'
    }
  }, children), actions && /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      justifyContent: 'flex-end',
      gap: 8,
      padding: 8
    }
  }, actions)));
}
Object.assign(__ds_scope, { Dialog });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/Dialog.jsx", error: String((e && e.message) || e) }); }

// components/feedback/MeterBar.jsx
try { (() => {
/** MUI LinearProgress. Two uses: the 200×8 microphone level meter (secondary,
 *  determinate, transform .05s linear) and the updater's download progress. */
function MeterBar({
  value = 0,
  indeterminate = false,
  color = 'secondary',
  width = 200,
  height = 8
}) {
  const track = color === 'secondary' ? 'var(--acl-red-500)' : 'var(--accent-primary)';
  return /*#__PURE__*/React.createElement("div", {
    style: {
      width,
      height,
      borderRadius: 0,
      overflow: 'hidden',
      background: 'rgba(244,67,54,.35)',
      margin: '5px auto'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      height: '100%',
      background: track,
      width: indeterminate ? '40%' : `${Math.max(0, Math.min(100, value))}%`,
      transition: 'var(--transition-meter)'
    }
  }));
}
Object.assign(__ds_scope, { MeterBar });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/MeterBar.jsx", error: String((e && e.message) || e) }); }

// components/feedback/StatusBadge.jsx
try { (() => {
/** The round badge centred on an avatar when something is wrong with that player.
 *  Fill and border are a pair per state — from Avatar.tsx. */
const states = {
  muted: {
    icon: 'mic_off',
    bg: 'var(--acl-muted)',
    edge: 'var(--acl-muted-edge)'
  },
  deafened: {
    icon: 'volume_off',
    bg: 'var(--acl-muted)',
    edge: 'var(--acl-muted-edge)'
  },
  novoice: {
    icon: 'link_off',
    bg: 'var(--acl-novoice)',
    edge: 'var(--acl-novoice-edge)'
  },
  disconnected: {
    icon: 'wifi_off',
    bg: 'var(--acl-muted)',
    edge: 'var(--acl-muted-edge)'
  },
  bugged: {
    icon: 'error',
    bg: 'red',
    edge: 'transparent'
  }
};
function StatusBadge({
  state = 'muted',
  size = 20,
  style
}) {
  const tone = states[state] || states.muted;
  return /*#__PURE__*/React.createElement("span", {
    style: {
      display: 'grid',
      placeItems: 'center',
      background: tone.bg,
      border: `var(--border-badge) solid ${tone.edge}`,
      borderRadius: 'var(--radius-round)',
      padding: 2,
      zIndex: 10,
      ...style
    }
  }, /*#__PURE__*/React.createElement(__ds_scope.Icon, {
    name: tone.icon,
    size: size,
    color: "#fff"
  }));
}
Object.assign(__ds_scope, { StatusBadge });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/StatusBadge.jsx", error: String((e && e.message) || e) }); }

// components/feedback/Tooltip.jsx
try { (() => {
/** MUI Tooltip with the client's 15px override. Hover-only, 300ms leave delay so
 *  the volume slider inside a player tooltip stays reachable. */
function Tooltip({
  title,
  children,
  placement = 'top',
  open: forced
}) {
  const [hover, setHover] = React.useState(false);
  const open = forced === undefined ? hover : forced;
  const pos = placement === 'bottom' ? {
    top: '100%',
    marginTop: 6
  } : {
    bottom: '100%',
    marginBottom: 6
  };
  return /*#__PURE__*/React.createElement("span", {
    style: {
      position: 'relative',
      display: 'inline-flex'
    },
    onMouseEnter: () => setHover(true),
    onMouseLeave: () => setHover(false)
  }, children, open && title && /*#__PURE__*/React.createElement("span", {
    style: {
      position: 'absolute',
      left: '50%',
      transform: 'translateX(-50%)',
      ...pos,
      background: 'var(--acl-bg-tooltip)',
      border: '1px solid gray',
      borderRadius: 'var(--radius-sm)',
      padding: '4px 8px',
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--size-tooltip)',
      color: '#fff',
      whiteSpace: 'pre-line',
      zIndex: 30,
      textAlign: 'center',
      minWidth: 'max-content'
    }
  }, title));
}
Object.assign(__ds_scope, { Tooltip });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/Tooltip.jsx", error: String((e && e.message) || e) }); }

// components/forms/Checkbox.jsx
try { (() => {
/** A settings toggle: MUI Checkbox + FormControlLabel with the hairline top
 *  border Settings.tsx's `formLabel` class adds. Full width, label on the right. */
function Checkbox({
  label,
  checked = false,
  disabled = false,
  onChange,
  divided = true
}) {
  return /*#__PURE__*/React.createElement("label", {
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 0,
      width: '100%',
      borderTop: divided ? '1px solid var(--border-hairline)' : 'none',
      marginRight: 0,
      opacity: disabled ? 0.38 : 1,
      cursor: disabled ? 'default' : 'pointer',
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--size-body)'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 42,
      height: 42,
      display: 'grid',
      placeItems: 'center',
      flex: '0 0 auto'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 18,
      height: 18,
      borderRadius: 2,
      display: 'grid',
      placeItems: 'center',
      border: checked ? 'none' : '2px solid rgba(255,255,255,.6)',
      background: checked ? 'var(--accent-primary)' : 'transparent',
      color: '#1d1a23',
      fontFamily: 'var(--font-icon)',
      fontSize: 16,
      lineHeight: 1
    }
  }, checked ? 'check' : '')), /*#__PURE__*/React.createElement("input", {
    type: "checkbox",
    checked: checked,
    disabled: disabled,
    onChange: e => onChange && onChange(e.target.checked),
    style: {
      position: 'absolute',
      opacity: 0,
      width: 0,
      height: 0
    }
  }), /*#__PURE__*/React.createElement("span", null, label));
}
Object.assign(__ds_scope, { Checkbox });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Checkbox.jsx", error: String((e && e.message) || e) }); }

// components/forms/RadioOption.jsx
try { (() => {
/** One option of a MUI RadioGroup — the three microphone modes. */
function RadioOption({
  label,
  value,
  selected = false,
  onSelect
}) {
  return /*#__PURE__*/React.createElement("label", {
    style: {
      display: 'flex',
      alignItems: 'center',
      cursor: 'pointer',
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--size-body)'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 42,
      height: 42,
      display: 'grid',
      placeItems: 'center'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 18,
      height: 18,
      borderRadius: '50%',
      border: `2px solid ${selected ? 'var(--accent-primary)' : 'rgba(255,255,255,.6)'}`,
      display: 'grid',
      placeItems: 'center'
    }
  }, selected && /*#__PURE__*/React.createElement("span", {
    style: {
      width: 9,
      height: 9,
      borderRadius: '50%',
      background: 'var(--accent-primary)'
    }
  }))), /*#__PURE__*/React.createElement("input", {
    type: "radio",
    checked: selected,
    onChange: () => onSelect && onSelect(value),
    style: {
      position: 'absolute',
      opacity: 0,
      width: 0,
      height: 0
    }
  }), /*#__PURE__*/React.createElement("span", null, label));
}
Object.assign(__ds_scope, { RadioOption });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/RadioOption.jsx", error: String((e && e.message) || e) }); }

// components/forms/SelectField.jsx
try { (() => {
/** MUI TextField select variant="outlined" color="secondary" with a shrunk label —
 *  microphone, speaker, overlay position, language. */
function SelectField({
  label,
  value,
  options = [],
  onChange,
  fullWidth = true
}) {
  return /*#__PURE__*/React.createElement("label", {
    style: {
      display: 'block',
      position: 'relative',
      width: fullWidth ? '100%' : 220,
      marginTop: 8,
      fontFamily: 'var(--font-ui)'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      position: 'absolute',
      top: -8,
      left: 9,
      padding: '0 4px',
      fontSize: 12,
      background: 'var(--surface-app)',
      color: 'var(--text-quiet)'
    }
  }, label), /*#__PURE__*/React.createElement("select", {
    value: value,
    onChange: e => onChange && onChange(e.target.value),
    style: {
      width: '100%',
      appearance: 'none',
      color: '#fff',
      background: 'transparent',
      border: '1px solid var(--acl-border-soft)',
      borderRadius: 4,
      padding: '14px 32px 14px 12px',
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--size-body)'
    }
  }, options.map(o => /*#__PURE__*/React.createElement("option", {
    key: typeof o === 'string' ? o : o.value,
    value: typeof o === 'string' ? o : o.value,
    style: {
      background: 'var(--surface-card)'
    }
  }, typeof o === 'string' ? o : o.label))), /*#__PURE__*/React.createElement("span", {
    className: "acl-icon",
    style: {
      position: 'absolute',
      right: 8,
      top: '50%',
      transform: 'translateY(-50%)',
      pointerEvents: 'none',
      color: 'var(--text-quiet)'
    }
  }, "arrow_drop_down"));
}
Object.assign(__ds_scope, { SelectField });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/SelectField.jsx", error: String((e && e.message) || e) }); }

// components/forms/Slider.jsx
try { (() => {
/** MUI Slider, size="small", as used for voice distance and every volume. */
function Slider({
  label,
  value = 50,
  min = 0,
  max = 100,
  step = 1,
  disabled = false,
  color = 'primary',
  suffix = '',
  onChange
}) {
  const pct = (value - min) / (max - min) * 100;
  const track = color === 'secondary' ? 'var(--acl-red-500)' : 'var(--accent-primary)';
  return /*#__PURE__*/React.createElement("div", {
    style: {
      width: '100%',
      fontFamily: 'var(--font-ui)',
      opacity: disabled ? 0.38 : 1
    }
  }, label !== undefined && /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 'var(--size-body)',
      marginBottom: 4
    }
  }, label, suffix), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative',
      height: 20,
      display: 'flex',
      alignItems: 'center',
      padding: '0 6px',
      boxSizing: 'border-box'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      left: 6,
      right: 6,
      height: 2,
      background: 'rgba(255,255,255,.26)',
      borderRadius: 1
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      left: 6,
      width: `calc((100% - 12px) * ${pct / 100})`,
      height: 2,
      background: track,
      borderRadius: 1
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      left: `calc(6px + (100% - 12px) * ${pct / 100})`,
      width: 12,
      height: 12,
      transform: 'translateX(-50%)',
      borderRadius: '50%',
      background: track
    }
  }), /*#__PURE__*/React.createElement("input", {
    type: "range",
    min: min,
    max: max,
    step: step,
    value: value,
    disabled: disabled,
    onChange: e => onChange && onChange(Number(e.target.value)),
    style: {
      position: 'absolute',
      left: 6,
      right: 6,
      width: 'auto',
      opacity: 0,
      height: 20,
      margin: 0,
      cursor: disabled ? 'default' : 'pointer'
    }
  })));
}
Object.assign(__ds_scope, { Slider });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Slider.jsx", error: String((e && e.message) || e) }); }

// components/forms/TextField.jsx
try { (() => {
/** MUI TextField variant="outlined". Used for the server URL, the public lobby
 *  title, and the four shortcut capture fields. */
function TextField({
  label,
  value = '',
  placeholder,
  error = false,
  helperText,
  readOnly = false,
  onChange,
  onKeyDown,
  fullWidth = true
}) {
  const border = error ? 'var(--acl-red-500)' : 'var(--acl-border-soft)';
  return /*#__PURE__*/React.createElement("label", {
    style: {
      display: 'block',
      position: 'relative',
      width: fullWidth ? '100%' : 220,
      marginTop: 8,
      fontFamily: 'var(--font-ui)'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      position: 'absolute',
      top: -8,
      left: 9,
      padding: '0 4px',
      fontSize: 12,
      background: 'var(--surface-app)',
      color: error ? 'var(--acl-red-500)' : 'var(--text-quiet)'
    }
  }, label), /*#__PURE__*/React.createElement("input", {
    value: value,
    placeholder: placeholder,
    readOnly: readOnly,
    spellCheck: false,
    onChange: e => onChange && onChange(e.target.value),
    onKeyDown: onKeyDown,
    style: {
      width: '100%',
      color: '#fff',
      background: 'transparent',
      boxSizing: 'border-box',
      border: `1px solid ${border}`,
      borderRadius: 4,
      padding: '14px 12px',
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--size-body)'
    }
  }), helperText && /*#__PURE__*/React.createElement("span", {
    style: {
      display: 'block',
      marginTop: 3,
      marginLeft: 12,
      fontSize: 12,
      color: error ? 'var(--acl-red-500)' : 'var(--text-quiet)'
    }
  }, helperText));
}
Object.assign(__ds_scope, { TextField });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/TextField.jsx", error: String((e && e.message) || e) }); }

// components/game/Crewmate.jsx
try { (() => {
/** Where cosmetics come from, pinned exactly as src/common/hatCollection.ts pins them.
 *  A branch reference would let the artwork every user downloads change without a
 *  release, so the commit and the URL move together or not at all. */
const HAT_COLLECTION_COMMIT = '14bb0cb592a23d2cee25a0c368506446abadaad8';
const HAT_COLLECTION_URL = `https://cdn.jsdelivr.net/gh/greluc/AnotherCrewLink-Hats@${HAT_COLLECTION_COMMIT}/`;

/** hats.json's NONE defaults, which nearly every cosmetic uses unchanged. */
const COSMETIC_DEFAULTS = {
  width: '130%',
  top: '-78%',
  left: '-14%'
};

/** Resolve a cosmetic file name to its URL. Pass the file name as it appears in
 *  hats.json, e.g. `pk01_Astronaut.png`. */
function cosmeticUrl(file) {
  return file ? `${HAT_COLLECTION_URL}NONE/${encodeURIComponent(file)}` : '';
}
const bases = {
  alive: 'player-base.png',
  dead: 'ghost-base.png'
};
const cache = new Map();
function rgb2hsv(r, g, b) {
  const v = Math.max(r, g, b);
  const c = v - Math.min(r, g, b);
  const h = c && (v === r ? (g - b) / c : v === g ? 2 + (b - r) / c : 4 + (r - g) / c);
  return [60 * (h < 0 ? h + 6 : h), v && c / v, v];
}
function isBetween(h, h1, maxDifference) {
  return 180 - Math.abs(Math.abs(h - h1) - 180) < maxDifference;
}
function hex(c) {
  const s = c.trim().replace('#', '');
  return [parseInt(s.slice(0, 2), 16), parseInt(s.slice(2, 4), 16), parseInt(s.slice(4, 6), 16)];
}
function mix(a, b, amount) {
  return [0, 1, 2].map(i => a[i] + (b[i] - a[i]) * amount);
}

/** The client's own recolour, ported from src/main/avatarGenerator.ts.
 *
 *  The template is authored in red / blue / green channels rather than in greys: a
 *  pixel's red says how much body colour it takes, its blue how much shadow, its
 *  green how much visor tint (#9acad5). Only pixels saturated enough and near those
 *  three hues are touched, which is what leaves the headset, the backpack straps and
 *  the outline alone. */
function recolour(image, body, shadow) {
  const canvas = document.createElement('canvas');
  canvas.width = image.naturalWidth;
  canvas.height = image.naturalHeight;
  const ctx = canvas.getContext('2d');
  ctx.drawImage(image, 0, 0);
  const frame = ctx.getImageData(0, 0, canvas.width, canvas.height);
  const data = frame.data;
  const bodyRgb = hex(body);
  const shadowRgb = hex(shadow);
  const visor = hex('#9acad5');
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i];
    const g = data[i + 1];
    const b = data[i + 2];
    const h = rgb2hsv(r, g, b);
    if (h[1] > 0.4 && (isBetween(h[0], 240, 30) || isBetween(h[0], 0, 100) || isBetween(h[0], 120, 40))) {
      let px = mix([0, 0, 0], shadowRgb, b / 255);
      px = mix(px, bodyRgb, r / 255);
      px = mix(px, visor, g / 255);
      data[i] = px[0];
      data[i + 1] = px[1];
      data[i + 2] = px[2];
    }
  }
  ctx.putImageData(frame, 0, 0);
  return canvas.toDataURL('image/png');
}

/** Recoloured base bodies are cached per colour pair — the real client generates
 *  them once into userData for the same reason. */
function useBody(body, shadow, alive, assetBase) {
  const key = `${assetBase}|${body}|${shadow}|${alive}`;
  const [src, setSrc] = React.useState(() => cache.get(key) || '');
  React.useEffect(() => {
    const hit = cache.get(key);
    if (hit) {
      setSrc(hit);
      return;
    }
    let live = true;
    const image = new Image();
    image.crossOrigin = 'anonymous';
    image.onload = () => {
      let url;
      try {
        url = recolour(image, body, shadow);
      } catch {
        url = image.src; // a tainted canvas: show the red template rather than nothing
      }
      cache.set(key, url);
      if (live) setSrc(url);
    };
    image.src = `${assetBase}/crewmates/${alive ? bases.alive : bases.dead}`;
    return () => {
      live = false;
    };
  }, [key, body, shadow, alive, assetBase]);
  return src;
}

/** One player, drawn the way the client draws them: a recoloured crewmate body with
 *  the hat, skin and visor composited over it.
 *
 *  The body template and the recolour are the client's own
 *  (static/images/generate/player.png + src/main/avatarGenerator.ts). Cosmetics are
 *  fetched from the pinned AnotherCrewLink-Hats CDN, exactly as
 *  src/renderer/cosmetics.ts fetches them — nothing is bundled here.
 *
 *  Cosmetics sit at `top: calc(22% + <top>)` and `left: calc(<left> + <offset>)` over
 *  a body drawn at 105% width from 22% down, which is Avatar.tsx's geometry. */
function Crewmate({
  color = 'var(--crew-red)',
  shadow = 'var(--crew-red-shadow)',
  size = 52,
  talking = false,
  alive = true,
  lookLeft = false,
  link = 'connected',
  hat,
  hatBack,
  visor,
  skin,
  showBorder = false,
  usingRadio = false,
  shape = 'circle',
  overflow = false,
  assetBase = '../../assets',
  style
}) {
  const resolved = useResolvedPair(color, shadow);
  const src = useBody(resolved.body, resolved.shadow, alive, assetBase);
  // Attributes arrive as strings from markup-driven consumers; a string width is not a
  // CSS length React will unit-ise, so the avatar would collapse to nothing.
  const px = Number(size) || 52;
  const border = Math.max(2, px / 40);
  const padLeft = -px * 0.07;
  const ringColour = talking ? 'var(--state-talking)' : showBorder ? '#ccbdcc86' : 'transparent';
  const cosmetic = (file, z) => file && {
    position: 'absolute',
    pointerEvents: 'none',
    width: COSMETIC_DEFAULTS.width,
    top: `calc(22% + ${COSMETIC_DEFAULTS.top})`,
    left: `calc(${COSMETIC_DEFAULTS.left} + ${border / 2 + padLeft}px)`,
    display: alive ? 'block' : 'none',
    zIndex: z
  };
  const cosmetics = /*#__PURE__*/React.createElement(React.Fragment, null, hat && /*#__PURE__*/React.createElement("img", {
    src: cosmeticUrl(hat),
    alt: "",
    style: {
      ...cosmetic(hat, 4)
    }
  }), visor && /*#__PURE__*/React.createElement("img", {
    src: cosmeticUrl(visor),
    alt: "",
    style: {
      ...cosmetic(visor, 3)
    }
  }), hatBack && /*#__PURE__*/React.createElement("img", {
    src: cosmeticUrl(hatBack),
    alt: "",
    style: {
      ...cosmetic(hatBack, 1)
    }
  }));
  return (
    /*#__PURE__*/
    // `isolation` gives the artwork its own stacking context, so a body at z-index 2
    // cannot paint over a status badge drawn by the caller above this avatar.
    React.createElement("div", {
      style: {
        position: 'relative',
        width: px,
        height: px,
        boxSizing: 'border-box',
        isolation: 'isolate',
        ...style
      }
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        position: 'absolute',
        inset: 0,
        borderRadius: '50%',
        borderStyle: 'solid',
        borderWidth: border,
        borderColor: ringColour,
        boxSizing: 'border-box',
        transition: 'var(--transition-border)',
        zIndex: 6,
        pointerEvents: 'none'
      }
    }), /*#__PURE__*/React.createElement("div", {
      style: {
        position: 'absolute',
        inset: 0,
        borderRadius: shape === 'circle' ? '50%' : 0,
        overflow: shape === 'circle' ? 'hidden' : 'visible',
        transform: lookLeft ? 'scaleX(-1)' : 'none',
        opacity: alive ? 1 : 0.55
      }
    }, src && /*#__PURE__*/React.createElement("img", {
      src: src,
      alt: "",
      style: {
        width: '105%',
        position: 'absolute',
        top: '22%',
        left: padLeft,
        zIndex: 2
      }
    }), skin && /*#__PURE__*/React.createElement("img", {
      src: cosmeticUrl(skin),
      alt: "",
      style: {
        ...cosmetic(skin, 3)
      }
    }), overflow && cosmetics), !overflow && /*#__PURE__*/React.createElement("div", {
      style: {
        position: 'absolute',
        inset: 0,
        transform: lookLeft ? 'scaleX(-1)' : 'none',
        opacity: alive ? 1 : 0.55,
        pointerEvents: 'none',
        zIndex: 4
      }
    }, cosmetics), link !== 'connected' && /*#__PURE__*/React.createElement("div", {
      style: {
        position: 'absolute',
        inset: 1,
        borderRadius: '50%',
        boxSizing: 'border-box',
        border: `2px solid ${link === 'disconnected' ? 'var(--acl-link-down)' : 'var(--acl-link-silent)'}`,
        zIndex: 7
      }
    }), usingRadio && /*#__PURE__*/React.createElement("img", {
      src: `${assetBase}/icons/radio.svg`,
      alt: "",
      style: {
        position: 'absolute',
        left: '70%',
        top: '80%',
        width: px * 0.3,
        transform: 'translate(-50%, -50%)',
        zIndex: 12
      }
    }))
  );
}

/** Resolves the pair, and keeps trying while the stylesheet is still on its way.
 *
 *  A canvas cannot use `var(--crew-lime)`: the value has to be read off the document.
 *  When the bundle executes before its stylesheet has loaded — which is what
 *  ds-base.js does, appending both at once — every custom property reads as empty and
 *  every crewmate would be recoloured to the red fallback, cached, and stay red. */
function useResolvedPair(color, shadow) {
  const [, retry] = React.useState(0);
  const pair = resolvePair(color, shadow);
  React.useEffect(() => {
    if (pair.resolved) return;
    let attempts = 0;
    const timer = setInterval(() => {
      attempts += 1;
      if (resolvePair(color, shadow).resolved || attempts > 40) {
        clearInterval(timer);
        retry(n => n + 1);
      }
    }, 50);
    return () => clearInterval(timer);
  }, [color, shadow, pair.resolved]);
  return pair;
}

/** Accepts either a hex pair or the crew custom properties, which have to be read off
 *  the document before a canvas can use them. */
function resolvePair(color, shadow) {
  let resolved = true;
  const read = (value, fallback) => {
    if (typeof value !== 'string') return fallback;
    const match = /var\((--[^)]+)\)/.exec(value.trim());
    if (!match) return value;
    if (typeof getComputedStyle !== 'function') {
      resolved = false;
      return fallback;
    }
    const resolvedValue = getComputedStyle(document.documentElement).getPropertyValue(match[1]).trim();
    if (!resolvedValue) resolved = false;
    return resolvedValue || fallback;
  };
  const body = read(color, '#C51111');
  const shadowValue = read(shadow, '#7A0838');
  return {
    body,
    shadow: shadowValue,
    resolved
  };
}
Object.assign(__ds_scope, { HAT_COLLECTION_COMMIT, HAT_COLLECTION_URL, COSMETIC_DEFAULTS, cosmeticUrl, Crewmate });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/game/Crewmate.jsx", error: String((e && e.message) || e) }); }

// components/game/LobbyCode.jsx
try { (() => {
/** The lobby code: Source Code Pro, 28px, 5px radius, tinted with the local
 *  player's crew colour. Reads "LOBBY" when the streaming setting hides it. */
function LobbyCode({
  code = 'ABCDEF',
  background = 'var(--crew-purple)',
  hidden = false
}) {
  return /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: 'var(--font-mono)',
      fontWeight: 500,
      fontSize: 'var(--size-code)',
      display: 'block',
      width: 'fit-content',
      margin: '5px auto',
      padding: 5,
      borderRadius: 'var(--radius-sm)',
      background,
      color: '#fff',
      letterSpacing: 'var(--tracking-code)'
    }
  }, hidden ? 'LOBBY' : code);
}
Object.assign(__ds_scope, { LobbyCode });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/game/LobbyCode.jsx", error: String((e && e.message) || e) }); }

// components/game/OverlayCapsule.jsx
try { (() => {
/** The in-game overlay container. Drawn over the running game, so its background is
 *  a veil rather than a surface: rgba(0,0,0,.5) in game, .35 bottom-left, and the
 *  compact side positions use the #25232ac0 capsule with one rounded end. */
function OverlayCapsule({
  children,
  position = 'top',
  compact = false,
  style
}) {
  const veil = {
    top: {
      background: 'var(--acl-veil-mid)',
      borderRadius: 'var(--radius-md)'
    },
    bottom_left: {
      background: 'var(--acl-veil-soft)',
      borderRadius: 'var(--radius-md)'
    },
    left: {
      background: 'var(--acl-veil-window)',
      borderTopRightRadius: 'var(--radius-capsule)',
      borderBottomRightRadius: 'var(--radius-capsule)'
    },
    right: {
      background: 'var(--acl-veil-window)',
      borderTopLeftRadius: 'var(--radius-capsule)',
      borderBottomLeftRadius: 'var(--radius-capsule)'
    },
    menu: {
      background: 'var(--acl-veil-strong)',
      borderRadius: 'var(--radius-md)'
    }
  }[position];
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexWrap: 'wrap',
      alignItems: 'center',
      justifyContent: position === 'bottom_left' ? 'flex-start' : 'center',
      gap: 10,
      padding: compact ? 5 : 8,
      ...veil,
      ...style
    }
  }, children);
}
Object.assign(__ds_scope, { OverlayCapsule });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/game/OverlayCapsule.jsx", error: String((e && e.message) || e) }); }

// components/game/PlayerSlot.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** A crewmate, their name and any state badge, in a fixed 76px slot (acl-ui SLOT).
 *  The name is under the crewmate and clipped: names run to ten characters. */
function PlayerSlot({
  name,
  color,
  shadow,
  size: sizeProp = 52,
  slot: slotProp = 76,
  talking = false,
  alive = true,
  badge,
  own = false,
  onClick,
  hat,
  hatBack,
  visor,
  skin,
  link = 'connected',
  usingRadio = false,
  assetBase,
  shape,
  overflow
}) {
  const size = Number(sizeProp) || 52;
  const slot = Number(slotProp) || 76;
  return /*#__PURE__*/React.createElement("div", {
    onClick: onClick,
    style: {
      width: own ? 96 : slot,
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      gap: 2,
      cursor: onClick ? 'pointer' : 'default'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative'
    }
  }, /*#__PURE__*/React.createElement(__ds_scope.Crewmate, _extends({
    color: color,
    shadow: shadow,
    size: own ? 68 : size,
    talking: talking,
    alive: alive,
    hat: hat,
    hatBack: hatBack,
    visor: visor,
    skin: skin,
    link: link,
    usingRadio: usingRadio
  }, assetBase ? {
    assetBase
  } : {}, shape ? {
    shape
  } : {}, overflow ? {
    overflow
  } : {})), badge && /*#__PURE__*/React.createElement(__ds_scope.StatusBadge, {
    state: badge,
    style: {
      position: 'absolute',
      left: '50%',
      top: '50%',
      transform: 'translate(-50%,-50%)',
      zIndex: 10
    }
  })), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: 'var(--font-ui)',
      fontSize: own ? 'var(--size-caption)' : 'var(--size-name-overlay)',
      maxWidth: '100%',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
      whiteSpace: 'nowrap',
      color: 'var(--text-body)'
    }
  }, name));
}
Object.assign(__ds_scope, { PlayerSlot });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/game/PlayerSlot.jsx", error: String((e && e.message) || e) }); }

// components/navigation/LobbyTable.jsx
try { (() => {
/** The public lobby browser table: #1d1a23 head, rows alternating #25232a /
 *  #1d1a23, 14px body, sticky header, and a right-aligned action cell. */
function LobbyTable({
  columns = [],
  rows = [],
  renderAction
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      width: '100%',
      overflow: 'auto',
      background: 'var(--surface-card)'
    }
  }, /*#__PURE__*/React.createElement("table", {
    style: {
      width: '100%',
      minWidth: 700,
      borderCollapse: 'collapse',
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--size-body)'
    }
  }, /*#__PURE__*/React.createElement("thead", null, /*#__PURE__*/React.createElement("tr", null, columns.map(c => /*#__PURE__*/React.createElement("th", {
    key: c,
    style: {
      background: 'var(--acl-bg-titlebar)',
      color: '#fff',
      textAlign: 'left',
      padding: '16px',
      fontWeight: 400,
      position: 'sticky',
      top: 0
    }
  }, c)), renderAction && /*#__PURE__*/React.createElement("th", {
    style: {
      background: 'var(--acl-bg-titlebar)',
      position: 'sticky',
      top: 0
    }
  }))), /*#__PURE__*/React.createElement("tbody", null, rows.map((r, i) => /*#__PURE__*/React.createElement("tr", {
    key: r.id ?? i,
    style: {
      background: i % 2 === 0 ? 'var(--acl-bg-row-odd)' : 'var(--acl-bg-row-even)'
    }
  }, columns.map(c => /*#__PURE__*/React.createElement("td", {
    key: c,
    style: {
      padding: '16px',
      color: 'var(--text-body)',
      whiteSpace: 'nowrap'
    }
  }, r[c])), renderAction && /*#__PURE__*/React.createElement("td", {
    style: {
      padding: '8px 16px',
      textAlign: 'right',
      whiteSpace: 'nowrap'
    }
  }, renderAction(r)))))));
}
Object.assign(__ds_scope, { LobbyTable });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/navigation/LobbyTable.jsx", error: String((e && e.message) || e) }); }

// components/navigation/TitleBar.jsx
try { (() => {
/** The frameless window's own title bar: 24px tall, #1d1a23, the app name centred
 *  in purple, three #777 icon buttons — settings and reload at the left, close at
 *  the right — and a 4px non-draggable strip above it so the window stays resizable. */
function TitleBar({
  version = '',
  onSettings,
  onReload,
  onClose,
  title = 'AnotherCrewLink'
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative',
      width: '100%',
      height: 'var(--titlebar-h)',
      background: 'var(--surface-chrome)',
      zIndex: 100
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      height: 'var(--resize-strip-h)'
    }
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      display: 'block',
      width: '100%',
      textAlign: 'center',
      height: 'var(--titlebar-h)',
      lineHeight: 'var(--titlebar-h)',
      color: 'var(--text-title)',
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--size-body)'
    }
  }, title, version ? ` v${version}` : ''), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      top: -3,
      left: 0,
      display: 'flex'
    }
  }, /*#__PURE__*/React.createElement(__ds_scope.IconButton, {
    icon: "settings",
    label: "Settings",
    onClick: onSettings
  }), /*#__PURE__*/React.createElement(__ds_scope.IconButton, {
    icon: "refresh",
    label: "Reload",
    onClick: onReload
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      top: -3,
      right: 0
    }
  }, /*#__PURE__*/React.createElement(__ds_scope.IconButton, {
    icon: "close",
    label: "Close",
    onClick: onClose
  })));
}
Object.assign(__ds_scope, { TitleBar });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/navigation/TitleBar.jsx", error: String((e && e.message) || e) }); }

// mockups/mockup-frame.js
try { (() => {
/* Shared shell for every client mockup.
 *
 * Purpose: give an implementation something to compare against. A mockup is not a demo —
 * it renders one view at a stated size, with nothing around it that is not in the product.
 *
 * The client window is RESIZABLE (frameless, 250px minimum, ~400 typical, no maximum), so a
 * window mockup is never one fixed picture. Every window view here can be dragged, jumped to
 * a preset, or requested at any size by URL, and the reflow is part of what you verify.
 *
 * URL parameters, all optional:
 *   ?bare=1      no toolbar, no annotations — a clean pixel reference for screenshot diffing
 *   ?w=320&h=560 render at this size (clamped to the view's own minimum)
 *   ?annotate=0  hide the measurement overlay (on by default when not bare)
 *   ?grid=1      8px grid overlay, the client's spacing step
 *
 * Keyboard: a / g toggle annotations and grid, [ / ] step the width by 10px.
 */
(function () {
  const params = new URLSearchParams(location.search);
  const flag = (name, dflt) => params.has(name) ? params.get(name) !== '0' : dflt;
  const bare = flag('bare', false);
  const css = `
  *{box-sizing:border-box}
  html,body{margin:0}
  body{background:#141218;color:#fff;font-family:var(--font-ui),system-ui,sans-serif;min-height:100vh}
  .mk{display:flex;gap:24px;padding:24px;align-items:flex-start}
  .mk[data-bare="1"]{padding:0;gap:0}
  .mk-bar{width:216px;flex:0 0 auto;display:flex;flex-direction:column;gap:10px;font-size:12px}
  .mk-bar h1{font-size:15px;margin:0;font-weight:400}
  .mk-bar p{margin:0;color:var(--text-quiet);line-height:1.55;font-size:11px}
  .mk-bar code{font-family:var(--font-mono);font-size:10px;color:#fff}
  .mk-row{display:flex;gap:6px;flex-wrap:wrap}
  .mk-btn{background:none;border:2px solid rgba(255,255,255,.25);color:#fff;border-radius:10px;padding:3px 9px;font-family:inherit;font-size:12px;cursor:pointer;transition:border-color 200ms ease-out}
  .mk-btn:hover{border-color:var(--accent-action)}
  .mk-btn[data-on="1"]{border-color:var(--accent-primary)}
  .mk-size{font-family:var(--font-mono);font-size:12px;color:var(--accent-primary)}
  .mk-hint{font-family:var(--font-mono);font-size:10px;color:var(--text-quiet);line-height:1.6}
  .mk-stage{position:relative;flex:0 0 auto}
  .mk-frame{position:relative;overflow:hidden;background:var(--surface-app);box-shadow:0 18px 40px rgba(0,0,0,.55)}
  .mk[data-bare="1"] .mk-frame{box-shadow:none}
  .mk-grid{position:absolute;inset:0;pointer-events:none;z-index:900;background-image:linear-gradient(to right,rgba(186,104,200,.16) 0 1px,transparent 1px 100%),linear-gradient(to bottom,rgba(186,104,200,.16) 0 1px,transparent 1px 100%);background-size:8px 8px}
  .mk-ann{position:absolute;inset:0;pointer-events:none;z-index:950}
  .mk-ann svg{position:absolute;inset:0;width:100%;height:100%;overflow:visible}
  .mk-ann text{font-family:var(--font-mono),monospace;font-size:9px;fill:#ffe98a;paint-order:stroke;stroke:rgba(0,0,0,.85);stroke-width:3px;stroke-linejoin:round}
  .mk-grip{position:absolute;right:-7px;bottom:-7px;width:16px;height:16px;border-radius:3px;background:var(--accent-primary);cursor:nwse-resize;z-index:960}
  .mk-grip::after{content:"";position:absolute;inset:4px;border-right:2px solid #1d1a23;border-bottom:2px solid #1d1a23}
  .mk-gripw{position:absolute;right:-7px;top:50%;margin-top:-14px;width:14px;height:28px;border-radius:3px;background:var(--accent-primary);cursor:ew-resize;z-index:960}
  `;
  function styleOnce() {
    if (document.getElementById('mk-style')) return;
    const el = document.createElement('style');
    el.id = 'mk-style';
    el.textContent = css;
    document.head.appendChild(el);
  }

  /** Measures a real element and returns a dimension callout. Annotations reference
   *  selectors rather than coordinates, so they cannot drift from the layout. */
  function measure(frame, ann) {
    const el = typeof ann.sel === 'string' ? frame.querySelector(ann.sel) : ann.sel;
    if (!el) return null;
    const f = frame.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    return {
      x: r.left - f.left,
      y: r.top - f.top,
      w: r.width,
      h: r.height,
      label: ann.label,
      edge: ann.edge || 'box',
      side: ann.side || 'auto'
    };
  }
  function drawAnnotations(frame, layer, list, attempt) {
    const boxes = list.map(a => measure(frame, a)).filter(Boolean);
    if (!boxes.length && list.length && (attempt || 0) < 20) {
      requestAnimationFrame(() => drawAnnotations(frame, layer, list, (attempt || 0) + 1));
      return;
    }
    const parts = boxes.map(b => {
      const round = n => Math.round(n * 10) / 10;
      if (b.edge === 'width') {
        const y = b.y + (b.side === 'below' ? b.h + 9 : -7);
        return `<line x1="${b.x}" y1="${y}" x2="${b.x + b.w}" y2="${y}" stroke="#ffe98a" stroke-width="1"/>
<line x1="${b.x}" y1="${y - 3}" x2="${b.x}" y2="${y + 3}" stroke="#ffe98a" stroke-width="1"/>
<line x1="${b.x + b.w}" y1="${y - 3}" x2="${b.x + b.w}" y2="${y + 3}" stroke="#ffe98a" stroke-width="1"/>
<text x="${b.x + b.w / 2}" y="${y - 3}" text-anchor="middle">${b.label || round(b.w) + 'px'}</text>`;
      }
      if (b.edge === 'height') {
        const x = b.x + (b.side === 'inside' ? 10 : b.w + 8);
        return `<line x1="${x}" y1="${b.y}" x2="${x}" y2="${b.y + b.h}" stroke="#ffe98a" stroke-width="1"/>
<line x1="${x - 3}" y1="${b.y}" x2="${x + 3}" y2="${b.y}" stroke="#ffe98a" stroke-width="1"/>
<line x1="${x - 3}" y1="${b.y + b.h}" x2="${x + 3}" y2="${b.y + b.h}" stroke="#ffe98a" stroke-width="1"/>
<text x="${x + 4}" y="${b.y + b.h / 2 + 3}">${b.label || round(b.h) + 'px'}</text>`;
      }
      return `<rect x="${b.x}" y="${b.y}" width="${b.w}" height="${b.h}" fill="none" stroke="#ffe98a" stroke-width="1" stroke-dasharray="3 2"/>
<text x="${b.x + 3}" y="${b.y - 3}">${b.label || round(b.w) + '×' + round(b.h)}</text>`;
    });
    layer.innerHTML = `<svg>${parts.join('')}</svg>`;
  }

  /**
   * mount({ name, note, width, height, minWidth, minHeight, resizable, presets,
   *         annotations, render })
   *
   * `render(frame, size)` fills the frame. It is called again after every resize, so a
   * view that reflows can rebuild — that is the point of a resizable mockup.
   */
  function mount(opts) {
    styleOnce();
    const minW = opts.minWidth || 1;
    const minH = opts.minHeight || 1;
    let w = Math.max(minW, Number(params.get('w')) || opts.width);
    let h = Math.max(minH, Number(params.get('h')) || opts.height);
    let annotate = flag('annotate', !bare);
    let grid = flag('grid', false);
    const resizable = opts.resizable !== false && !bare;
    document.title = `${opts.name} — ACL mockup`;
    const root = document.createElement('div');
    root.className = 'mk';
    root.dataset.bare = bare ? '1' : '0';
    const stage = document.createElement('div');
    stage.className = 'mk-stage';
    const frame = document.createElement('div');
    frame.className = 'mk-frame';
    const gridLayer = document.createElement('div');
    gridLayer.className = 'mk-grid';
    const annLayer = document.createElement('div');
    annLayer.className = 'mk-ann';
    stage.append(frame, gridLayer, annLayer);
    let bar, sizeEl;
    if (!bare) {
      bar = document.createElement('div');
      bar.className = 'mk-bar';
      bar.innerHTML = `<h1>${opts.name}</h1>
<div class="mk-size"></div>
<p>${opts.note || ''}</p>
<div class="mk-row" data-presets></div>
<div class="mk-row">
  <button class="mk-btn" data-toggle="annotate">measures</button>
  <button class="mk-btn" data-toggle="grid">8px grid</button>
</div>
<p class="mk-hint">?bare=1 clean reference<br>?w=&amp;h= any size<br>a · g · [ · ] keys</p>`;
      sizeEl = bar.querySelector('.mk-size');
      const presets = bar.querySelector('[data-presets]');
      (opts.presets || []).forEach(p => {
        const b = document.createElement('button');
        b.className = 'mk-btn';
        b.textContent = p.label;
        b.onclick = () => resize(p.w, p.h || h);
        presets.appendChild(b);
      });
      bar.querySelector('[data-toggle="annotate"]').onclick = () => {
        annotate = !annotate;
        paint();
      };
      bar.querySelector('[data-toggle="grid"]').onclick = () => {
        grid = !grid;
        paint();
      };
      root.append(bar, stage);
    } else {
      root.append(stage);
    }
    document.body.appendChild(root);
    if (resizable) {
      const grip = document.createElement('div');
      grip.className = 'mk-grip';
      grip.title = 'drag to resize — the real window is resizable';
      const gripW = document.createElement('div');
      gripW.className = 'mk-gripw';
      gripW.title = 'drag to change width only';
      stage.append(grip, gripW);
      const drag = (el, axis) => {
        el.addEventListener('pointerdown', e => {
          e.preventDefault();
          el.setPointerCapture(e.pointerId);
          const x0 = e.clientX,
            y0 = e.clientY,
            w0 = w,
            h0 = h;
          const move = ev => resize(w0 + (ev.clientX - x0), axis === 'both' ? h0 + (ev.clientY - y0) : h0);
          const up = () => {
            el.removeEventListener('pointermove', move);
            el.removeEventListener('pointerup', up);
          };
          el.addEventListener('pointermove', move);
          el.addEventListener('pointerup', up);
        });
      };
      drag(grip, 'both');
      drag(gripW, 'x');
    }
    function paint() {
      frame.style.width = w + 'px';
      frame.style.height = h + 'px';
      stage.style.width = w + 'px';
      stage.style.height = h + 'px';
      gridLayer.style.display = grid ? 'block' : 'none';
      annLayer.style.display = annotate ? 'block' : 'none';
      if (sizeEl) sizeEl.textContent = `${w} × ${h}${w <= minW ? '  (minimum)' : ''}`;
      if (bar) {
        bar.querySelector('[data-toggle="annotate"]').dataset.on = annotate ? '1' : '0';
        bar.querySelector('[data-toggle="grid"]').dataset.on = grid ? '1' : '0';
      }
      opts.render(frame, {
        width: w,
        height: h
      });
      if (annotate && opts.annotations) {
        requestAnimationFrame(() => drawAnnotations(frame, annLayer, opts.annotations({
          width: w,
          height: h
        }), 0));
      } else {
        annLayer.innerHTML = '';
      }
    }
    function resize(nw, nh) {
      w = Math.max(minW, Math.round(nw));
      h = Math.max(minH, Math.round(nh));
      paint();
    }
    addEventListener('keydown', e => {
      if (e.target.matches('input,textarea,select')) return;
      if (e.key === 'a') {
        annotate = !annotate;
        paint();
      }
      if (e.key === 'g') {
        grid = !grid;
        paint();
      }
      if (e.key === '[') resize(w - 10, h);
      if (e.key === ']') resize(w + 10, h);
    });
    paint();
    return {
      resize,
      repaint: paint
    };
  }
  window.ACLMockup = {
    mount,
    bare,
    params
  };
})();
})(); } catch (e) { __ds_ns.__errors.push({ path: "mockups/mockup-frame.js", error: String((e && e.message) || e) }); }

// ui_kits/client/GameOverlay.jsx
try { (() => {
const {
  OverlayCapsule,
  PlayerSlot
} = window.ACL_9b5df9;

// A host page (the mockups) may sit at another depth; it sets window.ACL_ASSETS.
const ASSETS = window.ACL_ASSETS || '../../assets';
const PLAYERS = [{
  name: 'Lime',
  crew: 'lime',
  talking: true,
  hat: 'pk04_MinerCap.png'
}, {
  name: 'Blue',
  crew: 'blue'
}, {
  name: 'Pink',
  crew: 'pink',
  talking: true,
  hat: 'flowerCrownHat.png'
}, {
  name: 'Yellow',
  crew: 'yellow',
  hat: 'pk02_Crown.png'
}];

/** The in-game overlay, in each position the setting offers. Overlay.tsx +
 *  css/overlay.css. Nothing here is interactive: it is drawn over the game. */
function GameOverlay({
  position = 'top',
  compact = false
}) {
  const shown = compact ? PLAYERS.filter(p => p.talking) : PLAYERS;
  // Overlay.tsx: showName = isOnSide && (!compact || the `1` variants). Names never
  // appear on the top or bottom-left positions.
  const showName = position === 'left' || position === 'right';
  const slots = shown.map(p => /*#__PURE__*/React.createElement(PlayerSlot, {
    key: p.name,
    name: showName ? p.name : '',
    size: 44,
    slot: 60,
    talking: !!p.talking,
    assetBase: ASSETS,
    hat: p.hat,
    color: 'var(--crew-' + p.crew + ')',
    shadow: 'var(--crew-' + p.crew + '-shadow)'
  }));
  const place = {
    top: {
      top: 0,
      left: '50%',
      transform: 'translateX(-50%)'
    },
    bottom_left: {
      bottom: 0,
      left: 0
    },
    left: {
      left: 0,
      top: '50%',
      transform: 'translateY(-50%)'
    },
    right: {
      right: 0,
      top: '50%',
      transform: 'translateY(-50%)'
    }
  }[position];
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      ...place
    }
  }, /*#__PURE__*/React.createElement(OverlayCapsule, {
    position: position,
    compact: compact,
    style: position === 'left' || position === 'right' ? {
      flexDirection: 'column'
    } : undefined
  }, slots));
}
Object.assign(window, {
  GameOverlay
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/client/GameOverlay.jsx", error: String((e && e.message) || e) }); }

// ui_kits/client/LobbyBrowserScreen.jsx
try { (() => {
const {
  LobbyTable,
  Button,
  Tooltip,
  Dialog
} = window.ACL_9b5df9;
const LOBBIES = [{
  id: 1,
  Title: 'chill euro lobby',
  Host: 'Red',
  Players: '7/15',
  Mods: 'None',
  Language: 'English',
  Status: 'Lobby 00:42',
  joinable: true,
  code: 'XKJDPQ'
}, {
  id: 2,
  Title: 'no mic no talk',
  Host: 'Lime',
  Players: '15/15',
  Mods: 'None',
  Language: 'English',
  Status: 'Lobby 02:03',
  joinable: false,
  reason: 'Lobby is Full'
}, {
  id: 3,
  Title: 'hide and seek',
  Host: 'Cyan',
  Players: '9/15',
  Mods: 'Town Of Us',
  Language: 'Deutsch',
  Status: 'In game 04:18',
  joinable: false,
  reason: 'Game in Progress'
}, {
  id: 4,
  Title: 'proximity practice',
  Host: 'Pink',
  Players: '4/10',
  Mods: 'None',
  Language: 'Français',
  Status: 'Lobby 00:11',
  joinable: true,
  code: 'PQMRTV'
}];

/** The public lobby browser — its own window. LobbyBrowser.tsx. */
function LobbyBrowserScreen() {
  const [code, setCode] = React.useState('');
  return /*#__PURE__*/React.createElement("div", {
    style: {
      height: '100%',
      width: '100%',
      paddingTop: 15,
      boxSizing: 'border-box',
      position: 'relative'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: 20,
      boxSizing: 'border-box',
      height: '100%'
    }
  }, /*#__PURE__*/React.createElement("b", {
    style: {
      fontSize: 14
    }
  }, "Public Lobbies"), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 12,
      maxHeight: 'calc(100% - 40px)',
      overflow: 'auto'
    }
  }, /*#__PURE__*/React.createElement(LobbyTable, {
    columns: ['Title', 'Host', 'Players', 'Mods', 'Language', 'Status'],
    rows: LOBBIES,
    renderAction: r => /*#__PURE__*/React.createElement(Tooltip, {
      title: r.joinable ? '' : r.reason
    }, /*#__PURE__*/React.createElement("span", null, /*#__PURE__*/React.createElement(Button, {
      variant: "contained",
      color: "secondary",
      disabled: !r.joinable,
      onClick: () => setCode('Lobby Code: ' + r.code + ' \n Region: Europe')
    }, "Show code")))
  }))), /*#__PURE__*/React.createElement(Dialog, {
    open: !!code,
    title: "Lobby information",
    actions: /*#__PURE__*/React.createElement(Button, {
      onClick: () => setCode('')
    }, "Close")
  }, code.split('\n').map((l, i) => /*#__PURE__*/React.createElement("div", {
    key: i
  }, l))));
}
Object.assign(window, {
  LobbyBrowserScreen
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/client/LobbyBrowserScreen.jsx", error: String((e && e.message) || e) }); }

// ui_kits/client/MeetingOverlay.jsx
try { (() => {
const {
  Crewmate
} = window.ACL_9b5df9;
const ASSETS = window.ACL_ASSETS || '../../assets';

/** The discussion tablet's own layer. Overlay.tsx positions tiles against the game's
 *  meeting hud — two ratio regimes, the old iPad one at 854/579 and the current one at
 *  ~1.72 — and gives each talking player a glow keyed to their crew colour:
 *  `box-shadow: 0 0 h/100 h/100 <colour>`, faded in over 400ms. Nothing else is drawn. */
const SEATS = [{
  name: 'Dummy 1',
  crew: 'lime',
  talking: true,
  hat: 'pk04_MinerCap.png'
}, {
  name: 'Dummy 2',
  crew: 'blue'
}, {
  name: 'Dummy 3',
  crew: 'pink',
  talking: true,
  hat: 'flowerCrownHat.png'
}, {
  name: 'Yellow',
  crew: 'yellow',
  hat: 'pk02_Crown.png'
}, {
  name: 'Black',
  crew: 'black',
  alive: false
}, {
  name: 'Orange',
  crew: 'orange',
  hat: 'pk03_Fedora.png'
}, {
  name: 'White',
  crew: 'white',
  hat: 'pk02_ToiletPaperHat.png'
}, {
  name: 'Brown',
  crew: 'brown'
}, {
  name: 'Red',
  crew: 'red',
  hat: 'pk01_Astronaut.png'
}, {
  name: 'Purple',
  crew: 'purple'
}];
function MeetingOverlay({
  height = 720,
  players = SEATS
}) {
  // The glow is derived from the viewport height, not from a fixed px value: the tablet
  // scales with the game window and a fixed blur would swamp it at 1080p.
  const glow = height / 100;
  const avatar = height * 0.075;
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      pointerEvents: 'none'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      left: '50%',
      top: '50%',
      transform: 'translate(-50%,-50%)',
      width: '58%',
      display: 'grid',
      gridTemplateColumns: 'repeat(2, 1fr)',
      rowGap: height * 0.022,
      columnGap: '12%'
    }
  }, players.map(p => /*#__PURE__*/React.createElement("div", {
    key: p.name,
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 10,
      padding: 6,
      borderRadius: 'var(--radius-md)',
      boxShadow: p.talking ? `0 0 ${glow}px ${glow}px var(--crew-${p.crew})` : 'none',
      background: p.talking ? 'rgba(0,0,0,.28)' : 'transparent',
      opacity: p.talking ? 1 : 0.9,
      transition: 'var(--transition-fade)'
    }
  }, /*#__PURE__*/React.createElement(Crewmate, {
    size: avatar,
    assetBase: ASSETS,
    hat: p.hat,
    alive: p.alive !== false,
    color: 'var(--crew-' + p.crew + ')',
    shadow: 'var(--crew-' + p.crew + '-shadow)'
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      font: 'var(--text-ui)',
      fontSize: Math.max(11, height * 0.019),
      color: '#fff',
      background: 'rgba(0,0,0,.32)',
      borderRadius: 'var(--radius-pill)',
      padding: '2px 10px',
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
      maxWidth: '9ch'
    }
  }, p.name)))));
}
Object.assign(window, {
  MeetingOverlay,
  MEETING_SEATS: SEATS
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/client/MeetingOverlay.jsx", error: String((e && e.message) || e) }); }

// ui_kits/client/SettingsScreen.jsx
try { (() => {
const {
  SectionHeading,
  Divider,
  Checkbox,
  RadioOption,
  Slider,
  SelectField,
  TextField,
  Button,
  Alert,
  IconButton,
  Tooltip,
  MeterBar
} = window.ACL_9b5df9;

/** The settings panel: a scrim over the whole window below the title bar, slid in
 *  from the left. Settings.tsx, in its own section order. */
function SettingsScreen({
  open,
  onClose
}) {
  const [distance, setDistance] = React.useState(5.3);
  const [rules, setRules] = React.useState({
    publicLobby: false,
    walls: true,
    vision: false,
    haunting: false,
    ventsHear: false,
    ventsPrivate: false,
    comms: false,
    cameras: false,
    radio: false,
    ghostOnly: false,
    meetingsOnly: false
  });
  const [mode, setMode] = React.useState(0);
  const [overlay, setOverlay] = React.useState(true);
  const set = k => v => setRules(r => ({
    ...r,
    [k]: v
  }));
  const hostOnly = 'Only the game host can change this!';
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      left: 0,
      top: 'var(--titlebar-h)',
      width: '100%',
      height: 'calc(100% - var(--titlebar-h))',
      background: 'var(--surface-scrim)',
      backdropFilter: 'var(--blur-scrim)',
      zIndex: 99,
      transition: 'var(--transition-panel)',
      transform: open ? 'translateX(0)' : 'translateX(-100%)',
      boxSizing: 'border-box'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center',
      height: 40,
      position: 'relative'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      right: 8
    }
  }, /*#__PURE__*/React.createElement(IconButton, {
    icon: "arrow_back",
    label: "Back",
    onClick: onClose
  })), /*#__PURE__*/React.createElement("span", {
    style: {
      font: 'var(--text-heading)'
    }
  }, "Settings")), /*#__PURE__*/React.createElement("div", {
    style: {
      height: 'calc(100% - 40px)',
      overflowY: 'auto',
      padding: '8px 16px 56px',
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      gap: 4,
      boxSizing: 'border-box'
    }
  }, /*#__PURE__*/React.createElement(SectionHeading, null, "Lobby Settings"), /*#__PURE__*/React.createElement(Slider, {
    label: "Voice Distance",
    suffix: ': ' + distance.toFixed(1),
    value: distance,
    min: 1,
    max: 10,
    step: 0.1,
    onChange: setDistance
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      width: '100%'
    }
  }, /*#__PURE__*/React.createElement(Checkbox, {
    label: "The Lobby is Public",
    checked: rules.publicLobby,
    onChange: set('publicLobby')
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Walls Block Audio",
    checked: rules.walls,
    onChange: set('walls')
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Hear People in Vision Only",
    checked: rules.vision,
    onChange: set('vision')
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Impostors Hear Dead",
    checked: rules.haunting,
    onChange: set('haunting')
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Hear Impostors in Vents",
    checked: rules.ventsHear,
    onChange: set('ventsHear')
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Private Talk in Vents",
    checked: rules.ventsPrivate,
    onChange: set('ventsPrivate')
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Comms Sabotage Disables Voice",
    checked: rules.comms,
    onChange: set('comms')
  }), /*#__PURE__*/React.createElement(Tooltip, {
    title: hostOnly
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      display: 'block',
      width: '100%'
    }
  }, /*#__PURE__*/React.createElement(Checkbox, {
    label: "Hear Through Cameras",
    checked: false,
    disabled: true
  }))), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Impostor Radio",
    checked: rules.radio,
    onChange: set('radio')
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Only Ghosts can Talk/Hear",
    checked: rules.ghostOnly,
    onChange: set('ghostOnly')
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Meetings/Lobby Only",
    checked: rules.meetingsOnly,
    onChange: set('meetingsOnly')
  })), /*#__PURE__*/React.createElement(Divider, null), /*#__PURE__*/React.createElement(SectionHeading, null, "Audio"), /*#__PURE__*/React.createElement(SelectField, {
    label: "Microphone",
    value: "default",
    options: [{
      value: 'default',
      label: 'Default'
    }, {
      value: 'usb',
      label: 'Yeti Nano (USB)'
    }]
  }), /*#__PURE__*/React.createElement(MeterBar, {
    value: 38
  }), /*#__PURE__*/React.createElement(SelectField, {
    label: "Speaker",
    value: "default",
    options: [{
      value: 'default',
      label: 'Default'
    }]
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      alignSelf: 'flex-start'
    }
  }, /*#__PURE__*/React.createElement(RadioOption, {
    label: "Voice Activity",
    value: 0,
    selected: mode === 0,
    onSelect: setMode
  }), /*#__PURE__*/React.createElement(RadioOption, {
    label: "Push to Talk",
    value: 1,
    selected: mode === 1,
    onSelect: setMode
  }), /*#__PURE__*/React.createElement(RadioOption, {
    label: "Push to Mute",
    value: 2,
    selected: mode === 2,
    onSelect: setMode
  })), /*#__PURE__*/React.createElement(Divider, null), /*#__PURE__*/React.createElement("div", {
    style: {
      width: '100%',
      display: 'flex',
      flexDirection: 'column',
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'grid',
      gridTemplateColumns: 'min-content minmax(0,1fr)',
      alignItems: 'center',
      gap: 8
    }
  }, /*#__PURE__*/React.createElement(Checkbox, {
    label: "",
    checked: true,
    divided: false,
    onChange: () => {}
  }), /*#__PURE__*/React.createElement(Slider, {
    label: "Microphone Volume",
    value: 100,
    max: 300,
    step: 2,
    onChange: () => {}
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'grid',
      gridTemplateColumns: 'min-content minmax(0,1fr)',
      alignItems: 'center',
      gap: 8
    }
  }, /*#__PURE__*/React.createElement(Checkbox, {
    label: "",
    checked: false,
    divided: false,
    onChange: () => {}
  }), /*#__PURE__*/React.createElement(Slider, {
    label: "Microphone Sensitivity",
    value: 30,
    max: 100,
    disabled: true,
    onChange: () => {}
  })), /*#__PURE__*/React.createElement(Slider, {
    label: "Master Volume",
    value: 100,
    max: 200,
    onChange: () => {}
  }), /*#__PURE__*/React.createElement(Slider, {
    label: "Crew Volume as Ghost",
    value: 100,
    onChange: () => {}
  })), /*#__PURE__*/React.createElement(Divider, null), /*#__PURE__*/React.createElement(SectionHeading, null, "Keyboard Shortcuts"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'grid',
      gridTemplateColumns: '1fr 1fr',
      gap: 8,
      width: '100%'
    }
  }, /*#__PURE__*/React.createElement(TextField, {
    label: "Push to Talk",
    value: "V",
    readOnly: true
  }), /*#__PURE__*/React.createElement(TextField, {
    label: "Impostor Radio",
    value: "B",
    readOnly: true
  }), /*#__PURE__*/React.createElement(TextField, {
    label: "Mute",
    value: "RControl",
    readOnly: true
  }), /*#__PURE__*/React.createElement(TextField, {
    label: "Deafen",
    value: "RAlt",
    readOnly: true
  })), /*#__PURE__*/React.createElement(Divider, null), /*#__PURE__*/React.createElement(SectionHeading, null, "Overlay"), /*#__PURE__*/React.createElement("div", {
    style: {
      width: '100%'
    }
  }, /*#__PURE__*/React.createElement(Checkbox, {
    label: "AnotherCrewLink on Top",
    checked: true,
    onChange: () => {}
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Enable Overlay",
    checked: overlay,
    onChange: setOverlay
  }), overlay && /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(Checkbox, {
    label: "Compact Overlay",
    checked: false,
    onChange: () => {}
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Meeting Overlay",
    checked: true,
    onChange: () => {}
  }), /*#__PURE__*/React.createElement(SelectField, {
    label: "Overlay Position",
    value: "top",
    options: [{
      value: 'hidden',
      label: 'Hidden'
    }, {
      value: 'top',
      label: 'Top Center'
    }, {
      value: 'bottom_left',
      label: 'Bottom Left'
    }, {
      value: 'right',
      label: 'Right'
    }, {
      value: 'left',
      label: 'Left'
    }]
  }))), /*#__PURE__*/React.createElement(Divider, null), /*#__PURE__*/React.createElement(SectionHeading, null, "Advanced"), /*#__PURE__*/React.createElement("div", {
    style: {
      width: '100%'
    }
  }, /*#__PURE__*/React.createElement(Checkbox, {
    label: "NAT Fix",
    checked: false,
    divided: false,
    onChange: () => {}
  })), /*#__PURE__*/React.createElement(Button, {
    variant: "contained",
    color: "secondary"
  }, "Change Voice Server"), /*#__PURE__*/React.createElement(Divider, null), /*#__PURE__*/React.createElement(SectionHeading, null, "BETA/DEBUG"), /*#__PURE__*/React.createElement("div", {
    style: {
      width: '100%'
    }
  }, /*#__PURE__*/React.createElement(Checkbox, {
    label: "VAD Enabled",
    checked: true,
    onChange: () => {}
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Hardware Acceleration",
    checked: true,
    onChange: () => {}
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Echo Cancellation",
    checked: false,
    onChange: () => {}
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Spatial Audio",
    checked: true,
    onChange: () => {}
  }), /*#__PURE__*/React.createElement(Checkbox, {
    label: "Noise Suppression",
    checked: false,
    onChange: () => {}
  })), /*#__PURE__*/React.createElement(SelectField, {
    label: "Language",
    value: "en",
    options: [{
      value: 'en',
      label: 'English'
    }, {
      value: 'de',
      label: 'Deutsch'
    }]
  }), /*#__PURE__*/React.createElement(Divider, null), /*#__PURE__*/React.createElement(SectionHeading, null, "Troubleshooting"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      gap: 8
    }
  }, /*#__PURE__*/React.createElement(Button, {
    variant: "contained",
    color: "secondary"
  }, "Restore to Default"), /*#__PURE__*/React.createElement(Button, {
    variant: "contained",
    color: "secondary"
  }, "Reset game offsets")), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 12,
      width: '100%'
    }
  }, /*#__PURE__*/React.createElement(Alert, {
    severity: "info"
  }, "Exit settings to apply changes"))));
}
Object.assign(window, {
  SettingsScreen
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/client/SettingsScreen.jsx", error: String((e && e.message) || e) }); }

// ui_kits/client/VoiceScreen.jsx
try { (() => {
const {
  Crewmate,
  LobbyCode,
  IconButton,
  Divider,
  StatusBadge,
  Tooltip,
  Slider
} = window.ACL_9b5df9;

// A host page (the mockups) may sit at another depth; it sets window.ACL_ASSETS.
const ASSETS = window.ACL_ASSETS || '../../assets';
const LOBBY = [{
  id: 2,
  name: 'Dummy 1',
  crew: 'lime',
  talking: true,
  hat: 'pk04_MinerCap.png'
}, {
  id: 3,
  name: 'Dummy 2',
  crew: 'blue'
}, {
  id: 4,
  name: 'Dummy 3',
  crew: 'pink',
  badge: 'novoice',
  hat: 'flowerCrownHat.png'
}, {
  id: 5,
  name: 'Yellow',
  crew: 'yellow',
  talking: true,
  hat: 'pk02_Crown.png'
}, {
  id: 6,
  name: 'Black',
  crew: 'black',
  alive: false
}, {
  id: 7,
  name: 'Orange',
  crew: 'orange',
  badge: 'muted',
  hat: 'pk03_Fedora.png'
}, {
  id: 8,
  name: 'White',
  crew: 'white',
  hat: 'pk02_ToiletPaperHat.png'
}, {
  id: 9,
  name: 'Brown',
  crew: 'brown',
  badge: 'disconnected'
}];

/** The VOICE state: in a lobby or a game. Voice.tsx's UI half. */
function VoiceScreen({
  code = 'XKJDPQ',
  hideCode = false,
  muted,
  deafened,
  onToggleMute,
  onToggleDeafen,
  talking
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      height: '100%',
      boxSizing: 'border-box'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      float: 'left',
      width: 100,
      paddingLeft: 8
    }
  }, /*#__PURE__*/React.createElement(Crewmate, {
    size: 80,
    assetBase: ASSETS,
    hat: "pk01_Astronaut.png",
    color: "var(--crew-purple)",
    shadow: "var(--crew-purple-shadow)",
    talking: talking && !muted && !deafened
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'center',
      flex: 1,
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      display: 'block',
      textAlign: 'center',
      fontSize: 20,
      whiteSpace: 'nowrap',
      maxWidth: '100%',
      overflow: 'hidden',
      textOverflow: 'ellipsis'
    }
  }, "Greluc"), /*#__PURE__*/React.createElement(LobbyCode, {
    code: code,
    hidden: hideCode,
    background: "var(--crew-purple-shadow)"
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      paddingLeft: 5,
      paddingTop: 26,
      display: 'grid'
    }
  }, /*#__PURE__*/React.createElement(IconButton, {
    icon: muted ? 'mic_off' : 'mic',
    label: "Mute",
    color: muted ? 'var(--state-muted)' : '#fff',
    onClick: onToggleMute
  }), /*#__PURE__*/React.createElement(IconButton, {
    icon: deafened ? 'volume_off' : 'volume_up',
    label: "Deafen",
    color: deafened ? 'var(--state-muted)' : '#fff',
    onClick: onToggleDeafen
  }))), /*#__PURE__*/React.createElement(Divider, {
    spacing: 8
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: '1 1 auto',
      minHeight: 0,
      overflowY: 'auto',
      display: 'flex',
      flexWrap: 'wrap',
      justifyContent: 'center',
      alignContent: 'flex-start',
      margin: '4px auto',
      width: '100%'
    }
  }, LOBBY.map(p => /*#__PURE__*/React.createElement("div", {
    key: p.id,
    style: {
      width: '32%',
      minWidth: 60,
      maxWidth: 120,
      padding: 8,
      boxSizing: 'border-box'
    }
  }, /*#__PURE__*/React.createElement(Tooltip, {
    title: /*#__PURE__*/React.createElement(PeerControls, {
      name: p.name
    })
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative',
      width: '100%'
    }
  }, /*#__PURE__*/React.createElement(Crewmate, {
    size: 78,
    assetBase: ASSETS,
    hat: p.hat,
    color: 'var(--crew-' + p.crew + ')',
    shadow: 'var(--crew-' + p.crew + '-shadow)',
    talking: !!p.talking,
    alive: p.alive !== false
  }), p.badge && /*#__PURE__*/React.createElement(StatusBadge, {
    state: p.badge,
    style: {
      position: 'absolute',
      left: '50%',
      top: '50%',
      transform: 'translate(-50%,-50%)'
    }
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      textAlign: 'center',
      fontSize: 12,
      marginTop: 2,
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis'
    }
  }, p.name)))));
}

/** What a player's tooltip holds: their name, a mute toggle and their volume. */
function PeerControls({
  name
}) {
  const [vol, setVol] = React.useState(100);
  const [off, setOff] = React.useState(false);
  return /*#__PURE__*/React.createElement("div", {
    style: {
      textAlign: 'center',
      minWidth: 120
    }
  }, /*#__PURE__*/React.createElement("b", null, name), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 4
    }
  }, /*#__PURE__*/React.createElement(IconButton, {
    icon: off ? 'volume_off' : 'volume_up',
    label: "Mute peer",
    color: "var(--accent-primary)",
    onClick: () => setOff(!off)
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1
    }
  }, /*#__PURE__*/React.createElement(Slider, {
    value: vol,
    min: 0,
    max: 200,
    onChange: setVol
  }))));
}
Object.assign(window, {
  VoiceScreen,
  PeerControls
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/client/VoiceScreen.jsx", error: String((e && e.message) || e) }); }

// ui_kits/client/WaitingScreen.jsx
try { (() => {
const {
  LaunchButton,
  OutlineButton
} = window.ACL_9b5df9;
function Spinner({
  size = 40
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      width: size,
      height: size,
      borderRadius: '50%',
      border: '3.6px solid transparent',
      borderTopColor: 'var(--accent-primary)',
      borderRightColor: 'var(--accent-primary)',
      animation: 'acl-spin 1.4s linear infinite'
    }
  });
}

/** The MENU state: Among Us is not running. Menu.tsx + LaunchButton.tsx. */
function WaitingScreen({
  error,
  onLaunch
}) {
  if (error) {
    return /*#__PURE__*/React.createElement("div", {
      style: {
        paddingTop: 32,
        textAlign: 'center'
      }
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        font: 'var(--text-heading)',
        color: 'var(--text-danger)'
      }
    }, "ERROR"), /*#__PURE__*/React.createElement("div", {
      style: {
        whiteSpace: 'pre-wrap',
        fontSize: 14,
        marginTop: 8,
        padding: '0 16px'
      }
    }, error), /*#__PURE__*/React.createElement("div", {
      style: {
        marginTop: 16,
        fontSize: 14
      }
    }, "Need help?\xA0", /*#__PURE__*/React.createElement("a", {
      href: "#",
      style: {
        color: 'var(--acl-red-500)'
      }
    }, "Get support")), /*#__PURE__*/React.createElement("div", {
      style: {
        marginTop: 8
      }
    }, /*#__PURE__*/React.createElement(OutlineButton, null, "Reload")));
  }
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'flex-start',
      height: '100%'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 20,
      marginTop: 12,
      marginBottom: 12
    }
  }, "Waiting for Among Us"), /*#__PURE__*/React.createElement(Spinner, null), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 24,
      marginTop: 15,
      marginBottom: 5
    }
  }, "Open via"), /*#__PURE__*/React.createElement(LaunchButton, {
    label: "Steam",
    platforms: ['Steam', 'Epic Games', 'Microsoft', 'Custom'],
    onLaunch: onLaunch
  }));
}
Object.assign(window, {
  WaitingScreen,
  Spinner
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/client/WaitingScreen.jsx", error: String((e && e.message) || e) }); }

__ds_ns.Button = __ds_scope.Button;

__ds_ns.Divider = __ds_scope.Divider;

__ds_ns.Icon = __ds_scope.Icon;

__ds_ns.IconButton = __ds_scope.IconButton;

__ds_ns.LaunchButton = __ds_scope.LaunchButton;

__ds_ns.OutlineButton = __ds_scope.OutlineButton;

__ds_ns.SectionHeading = __ds_scope.SectionHeading;

__ds_ns.Alert = __ds_scope.Alert;

__ds_ns.Dialog = __ds_scope.Dialog;

__ds_ns.MeterBar = __ds_scope.MeterBar;

__ds_ns.StatusBadge = __ds_scope.StatusBadge;

__ds_ns.Tooltip = __ds_scope.Tooltip;

__ds_ns.Checkbox = __ds_scope.Checkbox;

__ds_ns.RadioOption = __ds_scope.RadioOption;

__ds_ns.SelectField = __ds_scope.SelectField;

__ds_ns.Slider = __ds_scope.Slider;

__ds_ns.TextField = __ds_scope.TextField;

__ds_ns.HAT_COLLECTION_COMMIT = __ds_scope.HAT_COLLECTION_COMMIT;

__ds_ns.HAT_COLLECTION_URL = __ds_scope.HAT_COLLECTION_URL;

__ds_ns.COSMETIC_DEFAULTS = __ds_scope.COSMETIC_DEFAULTS;

__ds_ns.Crewmate = __ds_scope.Crewmate;

__ds_ns.LobbyCode = __ds_scope.LobbyCode;

__ds_ns.OverlayCapsule = __ds_scope.OverlayCapsule;

__ds_ns.PlayerSlot = __ds_scope.PlayerSlot;

__ds_ns.LobbyTable = __ds_scope.LobbyTable;

__ds_ns.TitleBar = __ds_scope.TitleBar;

})();
