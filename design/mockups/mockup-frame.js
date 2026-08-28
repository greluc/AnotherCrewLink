/* Shared shell for every client mockup.
 *
 * Purpose: give an implementation something to compare against. A mockup is not a demo —
 * it renders one view at a stated size, with nothing around it that is not in the product.
 *
 * The client window is RESIZABLE (frameless, 250px minimum, ~400 typical, no maximum), so a
 * window mockup is never one fixed picture. Every window view here can be dragged, jumped to
 * a preset, or requested at any size by URL, and the reflow is part of what you verify.
 *
 * URL parameters, all optional:
 *   ?bare=1      no toolbar, no annotations — a clean pixel reference for screenshot diffing
 *   ?w=320&h=560 render at this size (clamped to the view's own minimum)
 *   ?annotate=0  hide the measurement overlay (on by default when not bare)
 *   ?grid=1      8px grid overlay, the client's spacing step
 *
 * Keyboard: a / g toggle annotations and grid, [ / ] step the width by 10px.
 */
(function () {
  const params = new URLSearchParams(location.search);
  const flag = (name, dflt) => (params.has(name) ? params.get(name) !== '0' : dflt);
  const bare = flag('bare', false);

  const css = `
  *{box-sizing:border-box}
  html,body{margin:0}
  body{background:#141218;color:#fff;font-family:var(--font-ui),system-ui,sans-serif;min-height:100vh}
  .mk{display:flex;gap:24px;padding:24px;align-items:flex-start}
  .mk[data-bare="1"]{padding:0;gap:0}
  .mk-bar{width:216px;flex:0 0 auto;display:flex;flex-direction:column;gap:10px;font-size:12px}
  .mk-bar h1{font-size:15px;margin:0;font-weight:400}
  .mk-bar p{margin:0;color:var(--text-quiet);line-height:1.55;font-size:11px}
  .mk-bar code{font-family:var(--font-mono);font-size:10px;color:#fff}
  .mk-row{display:flex;gap:6px;flex-wrap:wrap}
  .mk-btn{background:none;border:2px solid rgba(255,255,255,.25);color:#fff;border-radius:10px;padding:3px 9px;font-family:inherit;font-size:12px;cursor:pointer;transition:border-color 200ms ease-out}
  .mk-btn:hover{border-color:var(--accent-action)}
  .mk-btn[data-on="1"]{border-color:var(--accent-primary)}
  .mk-size{font-family:var(--font-mono);font-size:12px;color:var(--accent-primary)}
  .mk-hint{font-family:var(--font-mono);font-size:10px;color:var(--text-quiet);line-height:1.6}
  .mk-stage{position:relative;flex:0 0 auto}
  .mk-frame{position:relative;overflow:hidden;background:var(--surface-app);box-shadow:0 18px 40px rgba(0,0,0,.55)}
  .mk[data-bare="1"] .mk-frame{box-shadow:none}
  .mk-grid{position:absolute;inset:0;pointer-events:none;z-index:900;background-image:linear-gradient(to right,rgba(186,104,200,.16) 0 1px,transparent 1px 100%),linear-gradient(to bottom,rgba(186,104,200,.16) 0 1px,transparent 1px 100%);background-size:8px 8px}
  .mk-ann{position:absolute;inset:0;pointer-events:none;z-index:950}
  .mk-ann svg{position:absolute;inset:0;width:100%;height:100%;overflow:visible}
  .mk-ann text{font-family:var(--font-mono),monospace;font-size:9px;fill:#ffe98a;paint-order:stroke;stroke:rgba(0,0,0,.85);stroke-width:3px;stroke-linejoin:round}
  .mk-grip{position:absolute;right:-7px;bottom:-7px;width:16px;height:16px;border-radius:3px;background:var(--accent-primary);cursor:nwse-resize;z-index:960}
  .mk-grip::after{content:"";position:absolute;inset:4px;border-right:2px solid #1d1a23;border-bottom:2px solid #1d1a23}
  .mk-gripw{position:absolute;right:-7px;top:50%;margin-top:-14px;width:14px;height:28px;border-radius:3px;background:var(--accent-primary);cursor:ew-resize;z-index:960}
  `;

  function styleOnce() {
    if (document.getElementById('mk-style')) return;
    const el = document.createElement('style');
    el.id = 'mk-style';
    el.textContent = css;
    document.head.appendChild(el);
  }

  /** Measures a real element and returns a dimension callout. Annotations reference
   *  selectors rather than coordinates, so they cannot drift from the layout. */
  function measure(frame, ann) {
    const el = typeof ann.sel === 'string' ? frame.querySelector(ann.sel) : ann.sel;
    if (!el) return null;
    const f = frame.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    return { x: r.left - f.left, y: r.top - f.top, w: r.width, h: r.height, label: ann.label, edge: ann.edge || 'box', side: ann.side || 'auto' };
  }

  function drawAnnotations(frame, layer, list, attempt) {
    const boxes = list.map((a) => measure(frame, a)).filter(Boolean);
    if (!boxes.length && list.length && (attempt || 0) < 20) {
      requestAnimationFrame(() => drawAnnotations(frame, layer, list, (attempt || 0) + 1));
      return;
    }
    const parts = boxes.map((b) => {
      const round = (n) => Math.round(n * 10) / 10;
      if (b.edge === 'width') {
        const y = b.y + (b.side === 'below' ? b.h + 9 : -7);
        return `<line x1="${b.x}" y1="${y}" x2="${b.x + b.w}" y2="${y}" stroke="#ffe98a" stroke-width="1"/>
<line x1="${b.x}" y1="${y - 3}" x2="${b.x}" y2="${y + 3}" stroke="#ffe98a" stroke-width="1"/>
<line x1="${b.x + b.w}" y1="${y - 3}" x2="${b.x + b.w}" y2="${y + 3}" stroke="#ffe98a" stroke-width="1"/>
<text x="${b.x + b.w / 2}" y="${y - 3}" text-anchor="middle">${b.label || round(b.w) + 'px'}</text>`;
      }
      if (b.edge === 'height') {
        const x = b.x + (b.side === 'inside' ? 10 : b.w + 8);
        return `<line x1="${x}" y1="${b.y}" x2="${x}" y2="${b.y + b.h}" stroke="#ffe98a" stroke-width="1"/>
<line x1="${x - 3}" y1="${b.y}" x2="${x + 3}" y2="${b.y}" stroke="#ffe98a" stroke-width="1"/>
<line x1="${x - 3}" y1="${b.y + b.h}" x2="${x + 3}" y2="${b.y + b.h}" stroke="#ffe98a" stroke-width="1"/>
<text x="${x + 4}" y="${b.y + b.h / 2 + 3}">${b.label || round(b.h) + 'px'}</text>`;
      }
      return `<rect x="${b.x}" y="${b.y}" width="${b.w}" height="${b.h}" fill="none" stroke="#ffe98a" stroke-width="1" stroke-dasharray="3 2"/>
<text x="${b.x + 3}" y="${b.y - 3}">${b.label || round(b.w) + '×' + round(b.h)}</text>`;
    });
    layer.innerHTML = `<svg>${parts.join('')}</svg>`;
  }

  /**
   * mount({ name, note, width, height, minWidth, minHeight, resizable, presets,
   *         annotations, render })
   *
   * `render(frame, size)` fills the frame. It is called again after every resize, so a
   * view that reflows can rebuild — that is the point of a resizable mockup.
   */
  function mount(opts) {
    styleOnce();
    const minW = opts.minWidth || 1;
    const minH = opts.minHeight || 1;
    let w = Math.max(minW, Number(params.get('w')) || opts.width);
    let h = Math.max(minH, Number(params.get('h')) || opts.height);
    let annotate = flag('annotate', !bare);
    let grid = flag('grid', false);
    const resizable = opts.resizable !== false && !bare;

    document.title = `${opts.name} — ACL mockup`;

    const root = document.createElement('div');
    root.className = 'mk';
    root.dataset.bare = bare ? '1' : '0';

    const stage = document.createElement('div');
    stage.className = 'mk-stage';
    const frame = document.createElement('div');
    frame.className = 'mk-frame';
    const gridLayer = document.createElement('div');
    gridLayer.className = 'mk-grid';
    const annLayer = document.createElement('div');
    annLayer.className = 'mk-ann';
    stage.append(frame, gridLayer, annLayer);

    let bar, sizeEl;
    if (!bare) {
      bar = document.createElement('div');
      bar.className = 'mk-bar';
      bar.innerHTML = `<h1>${opts.name}</h1>
<div class="mk-size"></div>
<p>${opts.note || ''}</p>
<div class="mk-row" data-presets></div>
<div class="mk-row">
  <button class="mk-btn" data-toggle="annotate">measures</button>
  <button class="mk-btn" data-toggle="grid">8px grid</button>
</div>
<p class="mk-hint">?bare=1 clean reference<br>?w=&amp;h= any size<br>a · g · [ · ] keys</p>`;
      sizeEl = bar.querySelector('.mk-size');
      const presets = bar.querySelector('[data-presets]');
      (opts.presets || []).forEach((p) => {
        const b = document.createElement('button');
        b.className = 'mk-btn';
        b.textContent = p.label;
        b.onclick = () => resize(p.w, p.h || h);
        presets.appendChild(b);
      });
      bar.querySelector('[data-toggle="annotate"]').onclick = () => { annotate = !annotate; paint(); };
      bar.querySelector('[data-toggle="grid"]').onclick = () => { grid = !grid; paint(); };
      root.append(bar, stage);
    } else {
      root.append(stage);
    }
    document.body.appendChild(root);

    if (resizable) {
      const grip = document.createElement('div');
      grip.className = 'mk-grip';
      grip.title = 'drag to resize — the real window is resizable';
      const gripW = document.createElement('div');
      gripW.className = 'mk-gripw';
      gripW.title = 'drag to change width only';
      stage.append(grip, gripW);
      const drag = (el, axis) => {
        el.addEventListener('pointerdown', (e) => {
          e.preventDefault();
          el.setPointerCapture(e.pointerId);
          const x0 = e.clientX, y0 = e.clientY, w0 = w, h0 = h;
          const move = (ev) => resize(w0 + (ev.clientX - x0), axis === 'both' ? h0 + (ev.clientY - y0) : h0);
          const up = () => { el.removeEventListener('pointermove', move); el.removeEventListener('pointerup', up); };
          el.addEventListener('pointermove', move);
          el.addEventListener('pointerup', up);
        });
      };
      drag(grip, 'both');
      drag(gripW, 'x');
    }

    function paint() {
      frame.style.width = w + 'px';
      frame.style.height = h + 'px';
      stage.style.width = w + 'px';
      stage.style.height = h + 'px';
      gridLayer.style.display = grid ? 'block' : 'none';
      annLayer.style.display = annotate ? 'block' : 'none';
      if (sizeEl) sizeEl.textContent = `${w} × ${h}${w <= minW ? '  (minimum)' : ''}`;
      if (bar) {
        bar.querySelector('[data-toggle="annotate"]').dataset.on = annotate ? '1' : '0';
        bar.querySelector('[data-toggle="grid"]').dataset.on = grid ? '1' : '0';
      }
      opts.render(frame, { width: w, height: h });
      if (annotate && opts.annotations) {
        requestAnimationFrame(() => drawAnnotations(frame, annLayer, opts.annotations({ width: w, height: h }), 0));
      } else {
        annLayer.innerHTML = '';
      }
    }

    function resize(nw, nh) {
      w = Math.max(minW, Math.round(nw));
      h = Math.max(minH, Math.round(nh));
      paint();
    }

    addEventListener('keydown', (e) => {
      if (e.target.matches('input,textarea,select')) return;
      if (e.key === 'a') { annotate = !annotate; paint(); }
      if (e.key === 'g') { grid = !grid; paint(); }
      if (e.key === '[') resize(w - 10, h);
      if (e.key === ']') resize(w + 10, h);
    });

    paint();
    return { resize, repaint: paint };
  }

  window.ACLMockup = { mount, bare, params };
})();
