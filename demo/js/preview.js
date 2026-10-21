// Panel-look previews. Each <canvas> gets one wasm `Preview`, which owns the
// LED renderer for its life and swaps only the file underneath it. All
// previews share one look (gain/bloom/gamma/flat), kept in localStorage, and
// one requestAnimationFrame loop that ticks only the previews that are on
// screen.

import { Preview, defaultBloom } from '../pkg/opendmd_web.js';
import { h, store, on } from './util.js';
import { app } from './state.js';

const DEFAULT_LOOK = { gamma: 2.2, gain: 1, bloom: defaultBloom(), flat: false };
export const look = { ...DEFAULT_LOOK, ...store.get('look', {}) };
const live = new Set();
let raf = 0;

// Files come in with the device's transition (the engine's own, in wasm),
// and show in its colour and glow; local mode keeps the engine default,
// Dissolve 400 ms, full colour, no glow.
const trans = (p, c = app.config) => c && (p.setTransition(c.transition ?? 'dissolve', c.transition_ms ?? 400), p.setDisplay(c.look ?? '', c.glow ?? ''));
on('config', (c) => { for (const p of live) trans(p.p, c); });

function loop() {
  raf = 0;
  for (const p of live) if (p.canvas.offsetParent !== null) p.tick();
  if (live.size) raf = requestAnimationFrame(loop);
}

// The one scale rule (dot matrix, Create Art's frame): whole device pixels per
// pixel that fit `room` CSS px; 0 when none.
export const fitScale = (room, w) => Math.floor((room * devicePixelRatio) / w);

// The width to fill, or a guess while hidden (Library's closed sheet).
function room(c) {
  c.style.width = '';
  return c.getBoundingClientRect().width || Math.min(innerWidth, 1240) - 32;
}

export class PanelPreview {
  // Rendered at 6..10 px a dot (fixed here: below 6 no room for the bloom,
  // above 10 frame time for nothing), shown at the whole multiple that fits
  // (fitScale), so every view draws the same dots; too narrow: scaled down.
  // `wrap` (Now): the view is narrowed to the preview's card, so the dots
  // fill it with no gutter; the room is then #main's, not the card's.
  constructor(canvas, { onDwellEnd, onError, wrap } = {}) {
    this.canvas = canvas;
    const card = canvas.parentElement, out = wrap?.parentElement;
    const px = (e) => { const s = getComputedStyle(e); return parseFloat(s.paddingLeft) + parseFloat(s.paddingRight); };
    const roomOf = () => (wrap ? out.clientWidth - px(out) - px(card) : room(canvas));
    const s = fitScale(roomOf(), 128);
    let r = 6;
    for (let k = 10; k > 6; k--) if (k * Math.floor(s / k) > r * Math.floor(s / r)) r = k;
    canvas.width = 128 * r;
    const fit = () => {
      const m = Math.floor(fitScale(roomOf(), 128) / r), w = (128 * r * m) / devicePixelRatio;
      canvas.style.width = m ? w + 'px' : '';
      canvas.style.imageRendering = m ? '' : 'auto';
      if (wrap) wrap.style.maxWidth = m ? w + px(card) + 'px' : '';
    };
    // (Width changes only, in the next frame: no ResizeObserver loop.)
    let w = -1;
    new ResizeObserver(([e]) => { if (e.contentRect.width !== w) { w = e.contentRect.width; requestAnimationFrame(fit); } }).observe(out || card);
    this.p = new Preview(canvas, look.gamma, look.gain, look.bloom, look.flat);
    trans(this.p);
    this.onDwellEnd = onDwellEnd;
    this.onError = onError;
    this.token = 0;
    live.add(this);
    if (!raf) raf = requestAnimationFrame(loop);
  }

  tick() {
    const r = this.p.frame();
    if (r === 2) this.onDwellEnd?.();
    else if (r < 0) this.onError?.(this.p.error);
  }

  // Fetch-and-play with last-caller-wins: a slow fetch for an old path never
  // replaces a newer one. `get` returns the file's bytes.
  async show(path, get) {
    const t = ++this.token;
    const bytes = await get(path);
    if (t !== this.token) return false;
    this.p.load(path, bytes);
    return true;
  }
  clear() { this.token++; this.p.clear(); }
  get path() { return this.p.path; }
  setPaused(v) { this.p.setPaused(v); }
  get paused() { return this.p.paused; }
  // The panel was `elapsed` ms into this file at local time `at`
  // (performance.now()); returns the drift before correction (NaN: nothing loaded).
  syncTo(elapsed, at) { return this.p.syncTo(elapsed, at); }
  get synced() { return this.p.synced; }
  setDwell(min, max) { this.p.setDwell(min, max); }
  apply() { this.p.setLook(look.gamma, look.gain, look.bloom, look.flat); }
}

export function setLook(patch) {
  Object.assign(look, patch);
  store.set('look', look);
  for (const p of live) p.apply();
}

// The collapsible "panel look" section: the simulator's --led-gain, --bloom
// and --gamma, plus flat (nearest-neighbour, no LED simulation).
// `extra` (optional) goes at the end of the row: the Now view's drift readout.
export function lookControls(extra) {
  const slider = (key, min, max, step) => {
    const out = h('output.mono', look[key].toFixed(2));
    const input = h('input', {
      type: 'range', min, max, step, value: look[key], id: 'look-' + key,
      oninput: () => { out.textContent = (+input.value).toFixed(2); setLook({ [key]: +input.value }); },
    });
    return h('label', key[0].toUpperCase() + key.slice(1), input, out);
  };
  const flat = h('input', { type: 'checkbox', id: 'look-flat', checked: look.flat, onchange: () => setLook({ flat: flat.checked }) });
  return h('details.card.look', h('summary', 'Panel look'),
    h('div.row', slider('gain', 0, 4, 0.05), slider('bloom', 0, 1, 0.01), slider('gamma', 0.5, 4, 0.05),
      h('label', flat, 'Flat'),
      h('button.sm', { onclick: (e) => { setLook(DEFAULT_LOOK); const d = lookControls(extra); d.open = true; e.target.closest('details').replaceWith(d); } }, 'Reset'), extra));
}
