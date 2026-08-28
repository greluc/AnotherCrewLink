# UI kit — AnotherCrewLink Server

The server has one human-facing surface: `GET /`.

| File | What it is |
| --- | --- |
| `verbatim.html` | That page exactly as it ships — one string literal in `src/web.rs`: `system-ui`, a 32rem measure, a definition list. Operator-facing and deliberately plain. |
| `index.html` | A **branded proposal**: the same content in the client's own visual language — Varela Round, the violet-black shell, 4px-bordered panels, monospace counters, the address as a lobby-code chip, and real crewmates along the header. |

`index.html` is a design, not a recreation: nothing in the repository implies a styled
status page today. It exists because a page a player might land on should look like the
product. The content is unchanged from `src/web.rs` and the README's own three refusals
(WebSocket-only transport, lobby-scoped signalling, first-claim host).

Both are Rust: the server was rewritten in Rust on 2026-08-24 and the Node implementation
is gone — the same direction the client is taking.

The other endpoints (`/health`, `/lobbies`, `/lobbies/{id}/code`, `/lobbies/stream`) are
JSON and SSE, with no visual surface.
