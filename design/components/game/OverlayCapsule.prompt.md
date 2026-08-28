Use only for things drawn over the game itself. Nothing in the overlay is opaque, and nothing in it is interactive.

```jsx
<OverlayCapsule position="top">{players.map(p => <PlayerSlot key={p.id} {...p} />)}</OverlayCapsule>
```

In compact mode a silent player is not drawn — the overlay disappears entirely when nobody is speaking.


**Rust counterpart:** `crates/acl-ui/src/overlay_layout.rs`.
