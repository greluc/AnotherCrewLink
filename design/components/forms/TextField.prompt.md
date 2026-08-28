Use for the server URL, lobby title, and the four keybind fields (read-only, they capture a key).

```jsx
<TextField label="Voice Server" value={url} error={!valid} helperText={valid ? '' : 'Invalid URL'} onChange={setUrl} />
<TextField label="Push to Talk" value="V" readOnly fullWidth={false} />
```


**Rust counterpart:** `Kind::Text` (committed on losing focus — half a URL is a server that does not exist) and `Kind::Shortcut`, which shows `…` while capturing.
