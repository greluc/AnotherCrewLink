The client puts the current value in the label rather than in a bubble for lobby settings, and uses MUI's value bubble for volumes.

```jsx
<Slider label="Voice Distance" suffix=": 5.3" value={5.3} min={1} max={10} step={0.1} onChange={setDistance} />
```

A gated slider (microphone volume, sensitivity) sits beside its enabling checkbox: checkbox at 3 columns, slider at 8.


**Rust counterpart:** `settings_screen.rs` `Kind::Slider`, whose `shown`/`stored` pair converts between what is displayed and what `config.json` holds.
