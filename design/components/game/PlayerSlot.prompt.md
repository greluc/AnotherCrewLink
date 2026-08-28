The unit the in-game screen is built from. Slots wrap; at the 250px minimum window three fit per row.

```jsx
<PlayerSlot name="Red" color="var(--crew-red)" shadow="var(--crew-red-shadow)" talking />
<PlayerSlot name="You" own badge="muted" color="var(--crew-cyan)" shadow="var(--crew-cyan-shadow)" />
```

Your own slot goes above the others, not in the wrapped list — it is where you check the two states only you can be in.


**Rust counterpart:** `views/main.rs` `slot` / `draw_own` (`SLOT` 76, `OWN_SLOT` 96) and `roster.rs`, which decides who is shown.
