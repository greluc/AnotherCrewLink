# cargo-vet, and what it actually buys

`config.toml` and `audits.toml` are managed by `cargo vet` — it rewrites them and rejects
hand-added comments, which is why this note is a file of its own.

## The measurement

Taken 2026-08-24 with Mozilla's, Google's and the Bytecode Alliance's shared audit sets
imported:

| | |
| --- | --- |
| Crates in the tree | 346 |
| Covered by a shared audit | 45 |
| Exemptions — this workspace asserting it has not looked | 301 |

`docs/rust-port/08-dependency-review.md` predicted that ratio and asked that it be said
plainly rather than left for a supply-chain table to imply coverage that does not exist.
Of the crates that matter most here, **`sonora`, `eframe`, `egui`, `winit`, `x11rb`,
`windows-sys`, `kurbo`, `serde_json`, `tokio-tungstenite` and `webpki-roots` have no
audit in any shared set**. `postcard` does.

The count has moved twice while this phase was being written: 283 at first, 316 when the
WebSocket transport landed, 346 when the overlay probe named eframe's `x11` and `wayland`
features. Each time the gate failed until the new crates were written down, which is the
whole mechanism working.

Note the third one in particular. Those features are gated by platform, so the crates
never build on Windows — but `cargo metadata --all-features`, which cargo-vet uses, lists
them anyway. That is what lets a Windows machine keep a store the Linux CI leg accepts.

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
