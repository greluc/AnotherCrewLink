Every client screen sits under this. It is 24px tall, the whole strip is the window drag handle, and the app name is the one place the purple accent appears as text.

```jsx
<TitleBar version="1.0.6" onSettings={open} />
```


**Rust counterpart:** `crates/acl-ui/src/window_state.rs`. The Electron chrome is the reference; the Rust GUI owns its own frame.
