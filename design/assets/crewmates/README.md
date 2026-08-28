# Crewmate artwork

| File | Where it came from |
| --- | --- |
| `player-base.png` | `static/images/generate/player.png` in greluc/AnotherCrewLink |
| `ghost-base.png` | `static/images/generate/ghost.png` |
| `red-alive.png` | `static/images/avatar/placeholder.png` — the fallback the client shows when a generated body is missing |

These are the client's own body templates. They are authored in red / blue / green
channels: red says how much body colour a pixel takes, blue how much shadow, green how
much visor tint (`#9acad5`). `components/game/Crewmate.jsx` ports the recolour from
`src/main/avatarGenerator.ts` and runs it on a canvas.

**Hats, skins and visors are not stored here.** They are fetched from the pinned
`AnotherCrewLink-Hats` CDN commit, exactly as `src/renderer/cosmetics.ts` fetches them.
That repository says of itself: "We are not taking any credits for the images in this
repository" — it is extracted game artwork, so this design system points at it rather
than copying it. `assets/icons/radio.svg` is the client's own file.
