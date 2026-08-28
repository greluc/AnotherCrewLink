For device, overlay-position and language pickers. The client uses `native: true`, so the list is the OS dropdown, and the label is always shrunk into the border.

```jsx
<SelectField label="Microphone" value={mic} options={mics} onChange={setMic} />
```


**Rust counterpart:** `settings_screen.rs` `Kind::Device` / `Kind::Language`, drawn as an egui `ComboBox`.
