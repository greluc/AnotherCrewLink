Only the lobby browser uses a table. Columns are Title, Host, Players, Mods, Language, Status, plus an action cell.

```jsx
<LobbyTable columns={["Title","Host","Players","Mods","Language","Status"]} rows={lobbies} renderAction={(r) => <Button variant="contained" color="secondary">Show code</Button>} />
```

The action is disabled — with a tooltip saying which — when the game is in progress, the lobby is full, or the mods do not match.


**Rust counterpart:** `crates/acl-ui/src/lobby_list.rs` + `views/lobby_browser.rs`.
