Use for any ordinary action in the client: dialog confirmations (text) and standalone actions like "Change Voice Server" or "Restore to Default" (contained secondary).

```jsx
<Button color="primary">Confirm</Button>
<Button variant="contained" color="secondary">Reset game offsets</Button>
```

Variants: `text` (default, dialog actions), `contained`. Colours: `primary` (purple), `secondary` (red — the client's default for contained buttons), `grey` (updater "Skip" / "Download Manually"). Labels are uppercased by MUI; write them in sentence case in source.


**Note:** MUI's uppercase, its 4px radius and its tint-wash hover are Electron-era details. In the Rust GUI use egui's own button affordances and keep the colour meanings: purple primary, red for standalone actions.
