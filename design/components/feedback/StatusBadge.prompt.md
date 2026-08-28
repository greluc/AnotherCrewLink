One badge per player, and only when something is wrong — a good connection draws nothing. Precedence in the client: bugged beats everything, then disconnected, then no-audio, then deafened, then muted.

```jsx
<StatusBadge state="novoice" />
```


**Rust counterpart:** `views/main.rs` `indicators`, which draws a connection ring rather than a badge (`#d25a5a` disconnected, `#dcb450` no audio) and says every state in words on hover.
