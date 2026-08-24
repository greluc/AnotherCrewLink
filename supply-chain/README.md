# cargo-vet, and what it actually buys

`config.toml` and `audits.toml` are managed by `cargo vet` — it rewrites them and rejects
hand-added comments, which is why this note is a file of its own.

## The measurement

Taken 2026-08-24 with Mozilla's, Google's and the Bytecode Alliance's shared audit sets
imported:

| | |
| --- | --- |
| Crates in the tree | 316 |
| Covered by a shared audit | 38 |
| Exemptions — this workspace asserting it has not looked | 278 |

`docs/rust-port/08-dependency-review.md` predicted that ratio and asked that it be said
plainly rather than left for a supply-chain table to imply coverage that does not exist.
Of the crates that matter most here, **`sonora`, `eframe`, `egui`, `winit`, `x11rb`,
`windows-sys`, `kurbo`, `serde_json`, `tokio-tungstenite` and `webpki-roots` have no
audit in any shared set**. `postcard` does.

The count moved from 283 to 316 when the WebSocket transport landed, and the gate failed
until the 33 new crates were written down — which is the whole mechanism working, on its
first real test.

## So what is the gate for

Not assurance about the 245. It catches a **new** unaudited dependency arriving unnoticed:
adding one fails CI until somebody writes it into `config.toml`, which makes it a decision
instead of a default.

Read it that way and it is worth the minute it costs. Read it as "the dependencies have
been reviewed" and it is worse than not having it, because it answers a question nobody
asked with a number nobody checked.

## Working with it

```bash
cargo vet                 # check
cargo vet suggest         # what would need auditing to shrink the exemption list
cargo vet import mozilla  # refresh a shared set
```

The exemptions are generated. Do not curate them by hand — run `cargo vet` and commit what
it writes.
