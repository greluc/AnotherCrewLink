# cargo-vet, and what it actually buys

`config.toml` and `audits.toml` are managed by `cargo vet` — it rewrites them and rejects
hand-added comments, which is why this note is a file of its own.

## The measurement

Taken 2026-08-25 with Mozilla's, Google's and the Bytecode Alliance's shared audit sets
imported:

| | |
| --- | --- |
| Crates in the tree | 556 |
| Covered by a shared audit | 62, and 3 partially |
| Exemptions — this workspace asserting it has not looked | 491 |

`docs/rust-port/08-dependency-review.md` predicted that ratio and asked that it be said
plainly rather than left for a supply-chain table to imply coverage that does not exist.
Of the crates that matter most here, **`sonora`, `eframe`, `egui`, `winit`, `x11rb`,
`windows-sys`, `kurbo`, `serde_json`, `tokio-tungstenite` and `webpki-roots` have no
audit in any shared set**. `postcard` does.

The count has moved four times while this phase was being written: 283 at first, 316 when
the WebSocket transport landed, 346 when the overlay probe named eframe's `x11` and
`wayland` features, and 556 when the P4 spike added `webrtc` `=0.20.3`. Each time the gate
failed until the new crates were written down, which is the whole mechanism working.

**That last jump is 141 crates from one dependency, and it deserves to be looked at rather
than absorbed.** Most of it is the `rtc-*` family the sans-IO core is split into — sixteen
crates for DTLS, SRTP, SCTP, ICE, STUN, TURN, mDNS, SDP and the interceptor chain — plus a
full RustCrypto software stack for the parts `ring` does not cover, and the ICU family
that arrives beneath `url`. `webrtc`, `rtc` and every `rtc-*` crate are exempted, not
audited: no shared set covers them. They are also the crates that terminate DTLS and parse
SRTP from the network, which is the least comfortable exemption in this store.

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
