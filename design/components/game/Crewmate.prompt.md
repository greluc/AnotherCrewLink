A real crewmate, not a coloured circle: the client's body template recoloured to a crew colour pair, with cosmetics composited on top.

```jsx
<Crewmate color="var(--crew-cyan)" shadow="var(--crew-cyan-shadow)" size={80} hat="pk04_MinerCap.png" talking />
<Crewmate color="var(--crew-black)" shadow="var(--crew-black-shadow)" alive={false} />
```

- Always pass **both** halves of a crew colour pair — the shadow is what stops it reading as a flat sticker. The twelve pairs are in `tokens/crew.css`, in the game's colour-id order.
- `assetBase` must point at this design system's `assets` folder from wherever the page lives (default `../../assets`). The body PNGs are local; hats stream from the pinned `AnotherCrewLink-Hats` CDN, which is what the client does too — so hats need a network, bodies do not.
- Cosmetic names are hats.json file names, e.g. `pk01_Astronaut.png`, `flowerCrownHat.png`, `pk02_Crown.png`, `PizzaVisor.png`. `hatBack` is the behind-the-body half of a two-part hat.
- Hats overhang the round frame by default, as in the client. Pass \`overflow\` to clip them to it — the overlay's side positions do that.
- \`shape="circle"\` (default) is the Electron client's round frame; \`shape="sprite"\` is the Rust GUI's uncropped crewmate, legs and all.
- Dead players are drawn as the ghost body with cosmetics hidden. Never recolour the headset or backpack: the template's own channels decide what changes.


**Rust counterpart:** `crates/acl-ui/src/views/main.rs` (`shapes_at`, `dressed`) and `worn.rs`. The Rust GUI draws the sprite uncropped — use `shape="sprite"` for new client work; `circle` reproduces the retiring Electron frame.
