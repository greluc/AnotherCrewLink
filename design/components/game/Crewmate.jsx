import React from 'react';

/** Where cosmetics come from, pinned exactly as src/common/hatCollection.ts pins them.
 *  A branch reference would let the artwork every user downloads change without a
 *  release, so the commit and the URL move together or not at all. */
export const HAT_COLLECTION_COMMIT = '14bb0cb592a23d2cee25a0c368506446abadaad8';
export const HAT_COLLECTION_URL = `https://cdn.jsdelivr.net/gh/greluc/AnotherCrewLink-Hats@${HAT_COLLECTION_COMMIT}/`;

/** hats.json's NONE defaults, which nearly every cosmetic uses unchanged. */
export const COSMETIC_DEFAULTS = { width: '130%', top: '-78%', left: '-14%' };

/** Resolve a cosmetic file name to its URL. Pass the file name as it appears in
 *  hats.json, e.g. `pk01_Astronaut.png`. */
export function cosmeticUrl(file) {
  return file ? `${HAT_COLLECTION_URL}NONE/${encodeURIComponent(file)}` : '';
}

const bases = { alive: 'player-base.png', dead: 'ghost-base.png' };
const cache = new Map();

function rgb2hsv(r, g, b) {
  const v = Math.max(r, g, b);
  const c = v - Math.min(r, g, b);
  const h = c && (v === r ? (g - b) / c : v === g ? 2 + (b - r) / c : 4 + (r - g) / c);
  return [60 * (h < 0 ? h + 6 : h), v && c / v, v];
}
function isBetween(h, h1, maxDifference) {
  return 180 - Math.abs(Math.abs(h - h1) - 180) < maxDifference;
}
function hex(c) {
  const s = c.trim().replace('#', '');
  return [parseInt(s.slice(0, 2), 16), parseInt(s.slice(2, 4), 16), parseInt(s.slice(4, 6), 16)];
}
function mix(a, b, amount) {
  return [0, 1, 2].map((i) => a[i] + (b[i] - a[i]) * amount);
}

/** The client's own recolour, ported from src/main/avatarGenerator.ts.
 *
 *  The template is authored in red / blue / green channels rather than in greys: a
 *  pixel's red says how much body colour it takes, its blue how much shadow, its
 *  green how much visor tint (#9acad5). Only pixels saturated enough and near those
 *  three hues are touched, which is what leaves the headset, the backpack straps and
 *  the outline alone. */
function recolour(image, body, shadow) {
  const canvas = document.createElement('canvas');
  canvas.width = image.naturalWidth;
  canvas.height = image.naturalHeight;
  const ctx = canvas.getContext('2d');
  ctx.drawImage(image, 0, 0);
  const frame = ctx.getImageData(0, 0, canvas.width, canvas.height);
  const data = frame.data;
  const bodyRgb = hex(body);
  const shadowRgb = hex(shadow);
  const visor = hex('#9acad5');
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i];
    const g = data[i + 1];
    const b = data[i + 2];
    const h = rgb2hsv(r, g, b);
    if (h[1] > 0.4 && (isBetween(h[0], 240, 30) || isBetween(h[0], 0, 100) || isBetween(h[0], 120, 40))) {
      let px = mix([0, 0, 0], shadowRgb, b / 255);
      px = mix(px, bodyRgb, r / 255);
      px = mix(px, visor, g / 255);
      data[i] = px[0];
      data[i + 1] = px[1];
      data[i + 2] = px[2];
    }
  }
  ctx.putImageData(frame, 0, 0);
  return canvas.toDataURL('image/png');
}

/** Recoloured base bodies are cached per colour pair — the real client generates
 *  them once into userData for the same reason. */
function useBody(body, shadow, alive, assetBase) {
  const key = `${assetBase}|${body}|${shadow}|${alive}`;
  const [src, setSrc] = React.useState(() => cache.get(key) || '');
  React.useEffect(() => {
    const hit = cache.get(key);
    if (hit) { setSrc(hit); return; }
    let live = true;
    const image = new Image();
    image.crossOrigin = 'anonymous';
    image.onload = () => {
      let url;
      try {
        url = recolour(image, body, shadow);
      } catch {
        url = image.src; // a tainted canvas: show the red template rather than nothing
      }
      cache.set(key, url);
      if (live) setSrc(url);
    };
    image.src = `${assetBase}/crewmates/${alive ? bases.alive : bases.dead}`;
    return () => { live = false; };
  }, [key, body, shadow, alive, assetBase]);
  return src;
}

/** One player, drawn the way the client draws them: a recoloured crewmate body with
 *  the hat, skin and visor composited over it.
 *
 *  The body template and the recolour are the client's own
 *  (static/images/generate/player.png + src/main/avatarGenerator.ts). Cosmetics are
 *  fetched from the pinned AnotherCrewLink-Hats CDN, exactly as
 *  src/renderer/cosmetics.ts fetches them — nothing is bundled here.
 *
 *  Cosmetics sit at `top: calc(22% + <top>)` and `left: calc(<left> + <offset>)` over
 *  a body drawn at 105% width from 22% down, which is Avatar.tsx's geometry. */
export function Crewmate({
  color = 'var(--crew-red)',
  shadow = 'var(--crew-red-shadow)',
  size = 52,
  talking = false,
  alive = true,
  lookLeft = false,
  link = 'connected',
  hat,
  hatBack,
  visor,
  skin,
  showBorder = false,
  usingRadio = false,
  shape = 'circle',
  overflow = false,
  assetBase = '../../assets',
  style,
}) {
  const resolved = useResolvedPair(color, shadow);
  const src = useBody(resolved.body, resolved.shadow, alive, assetBase);
  // Attributes arrive as strings from markup-driven consumers; a string width is not a
  // CSS length React will unit-ise, so the avatar would collapse to nothing.
  const px = Number(size) || 52;
  const border = Math.max(2, px / 40);
  const padLeft = -px * 0.07;
  const ringColour = talking ? 'var(--state-talking)' : showBorder ? '#ccbdcc86' : 'transparent';
  const cosmetic = (file, z) => file && ({
    position: 'absolute',
    pointerEvents: 'none',
    width: COSMETIC_DEFAULTS.width,
    top: `calc(22% + ${COSMETIC_DEFAULTS.top})`,
    left: `calc(${COSMETIC_DEFAULTS.left} + ${border / 2 + padLeft}px)`,
    display: alive ? 'block' : 'none',
    zIndex: z,
  });

  const cosmetics = (
    <>
      {hat && <img src={cosmeticUrl(hat)} alt="" style={{ ...cosmetic(hat, 4) }} />}
      {visor && <img src={cosmeticUrl(visor)} alt="" style={{ ...cosmetic(visor, 3) }} />}
      {hatBack && <img src={cosmeticUrl(hatBack)} alt="" style={{ ...cosmetic(hatBack, 1) }} />}
    </>
  );

  return (
    // `isolation` gives the artwork its own stacking context, so a body at z-index 2
    // cannot paint over a status badge drawn by the caller above this avatar.
    <div style={{ position: 'relative', width: px, height: px, boxSizing: 'border-box', isolation: 'isolate', ...style }}>
      <div style={{
        position: 'absolute', inset: 0, borderRadius: '50%', borderStyle: 'solid',
        borderWidth: border, borderColor: ringColour, boxSizing: 'border-box',
        transition: 'var(--transition-border)', zIndex: 6, pointerEvents: 'none',
      }} />
      {/* `circle` is the Electron client: the body is clipped to a round frame.
          `sprite` is the Rust GUI (crates/acl-ui): the whole crewmate, uncropped. */}
      <div style={{
        position: 'absolute', inset: 0, borderRadius: shape === 'circle' ? '50%' : 0,
        overflow: shape === 'circle' ? 'hidden' : 'visible',
        transform: lookLeft ? 'scaleX(-1)' : 'none', opacity: alive ? 1 : 0.55,
      }}>
        {src && <img src={src} alt="" style={{ width: '105%', position: 'absolute', top: '22%', left: padLeft, zIndex: 2 }} />}
        {skin && <img src={cosmeticUrl(skin)} alt="" style={{ ...cosmetic(skin, 3) }} />}
        {/* Avatar.tsx nests the hat group inside the clipped frame only when `overflow`
            is set; by default it renders after it, so a hat overhangs the circle
            instead of being sliced flat by it. */}
        {overflow && cosmetics}
      </div>
      {!overflow && (
        <div style={{ position: 'absolute', inset: 0, transform: lookLeft ? 'scaleX(-1)' : 'none', opacity: alive ? 1 : 0.55, pointerEvents: 'none', zIndex: 4 }}>
          {cosmetics}
        </div>
      )}
      {link !== 'connected' && (
        <div style={{
          position: 'absolute', inset: 1, borderRadius: '50%', boxSizing: 'border-box',
          border: `2px solid ${link === 'disconnected' ? 'var(--acl-link-down)' : 'var(--acl-link-silent)'}`,
          zIndex: 7,
        }} />
      )}
      {usingRadio && (
        <img src={`${assetBase}/icons/radio.svg`} alt="" style={{
          position: 'absolute', left: '70%', top: '80%', width: px * 0.3,
          transform: 'translate(-50%, -50%)', zIndex: 12,
        }} />
      )}
    </div>
  );
}

/** Resolves the pair, and keeps trying while the stylesheet is still on its way.
 *
 *  A canvas cannot use `var(--crew-lime)`: the value has to be read off the document.
 *  When the bundle executes before its stylesheet has loaded — which is what
 *  ds-base.js does, appending both at once — every custom property reads as empty and
 *  every crewmate would be recoloured to the red fallback, cached, and stay red. */
function useResolvedPair(color, shadow) {
  const [, retry] = React.useState(0);
  const pair = resolvePair(color, shadow);
  React.useEffect(() => {
    if (pair.resolved) return;
    let attempts = 0;
    const timer = setInterval(() => {
      attempts += 1;
      if (resolvePair(color, shadow).resolved || attempts > 40) {
        clearInterval(timer);
        retry((n) => n + 1);
      }
    }, 50);
    return () => clearInterval(timer);
  }, [color, shadow, pair.resolved]);
  return pair;
}

/** Accepts either a hex pair or the crew custom properties, which have to be read off
 *  the document before a canvas can use them. */
function resolvePair(color, shadow) {
  let resolved = true;
  const read = (value, fallback) => {
    if (typeof value !== 'string') return fallback;
    const match = /var\((--[^)]+)\)/.exec(value.trim());
    if (!match) return value;
    if (typeof getComputedStyle !== 'function') { resolved = false; return fallback; }
    const resolvedValue = getComputedStyle(document.documentElement).getPropertyValue(match[1]).trim();
    if (!resolvedValue) resolved = false;
    return resolvedValue || fallback;
  };
  const body = read(color, '#C51111');
  const shadowValue = read(shadow, '#7A0838');
  return { body, shadow: shadowValue, resolved };
}
