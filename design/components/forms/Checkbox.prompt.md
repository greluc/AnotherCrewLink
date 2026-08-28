Every lobby rule and every advanced/beta flag is one of these. Rows stack with no gap; the hairline above each row is what separates them.

```jsx
<Checkbox label="Walls Block Audio" checked={walls} onChange={setWalls} />
<Checkbox label="Hear Through Cameras" checked={false} disabled />
```

Disabled rows carry a tooltip explaining why — "Only the game host can change this!" or "You can only change this in the lobby!".


**Rust counterpart:** `settings_screen.rs` `Kind::Toggle`. There, a control never writes a setting — it emits a `Change` and the caller applies it, after any confirmation.
