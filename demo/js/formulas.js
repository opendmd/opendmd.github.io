// The Formulas view (`#formulas`; mounted when first opened or a
// `#formula=` link opens): a few lines of maths that draw on the panel,
// written in Psi (Ψ), OpenDMD's per-pixel language. The examples
// (formulas.json) and the formulas saved in this browser, a details pane
// like the Library's, and the editor (`#formulas/edit`). Drawn in the page
// by their own wasm module (pkg-psi: the firmware's crate, so a frame here
// is the panel's byte for byte) and shown through the main module's LED
// preview (`Preview.showRgb`). Phase 1: nothing is sent to the panel; a
// formula reaches it as a GIF ("Save as GIF…" → Create Art).

import { app } from './state.js';
import { PanelPreview } from './preview.js';
import { h, clear, store, toast, debounce, saver } from './util.js';
import { popup, closeMenu } from './overrides.js';

let P = null, mod = null, gen = 0;
const psi = () => (mod ??= import(`../pkg-psi/opendmd_psi_web.js${gen ? '?' + gen : ''}`).then(async (m) => { P = { ...m, mem: (await m.default()).memory }; })
  .catch((e) => { mod = null; throw e; }));
// A trap in the module (a bug: it should never happen) leaves it unusable:
// a fresh instance (a module of its own, by its address) takes over.
function restart() {
  P = mod = held = null;
  gen++;
  psi().then(() => { toast('Formulas restarted'); B?.ed.again(); for (const v of views) v.dirty = true; kick(); });
}
function T(fn, d) {
  try { return P ? fn() : d; } catch (e) {
    if (!(e instanceof WebAssembly.RuntimeError)) throw e;
    restart();
    return d;
  }
}

// The frame clock is the panel's: 50 frames a second, `t` a whole number of 20 ms frames.
const D = 20;
// What the module holds: compiling is cheap (tens of µs), so whoever draws compiles first.
let held = null;
const take = (src) => T(() => src != null && (src === held || (P.compile(src.slice(0, 4096)) && (held = src) != null)), false);
const Q = (v) => Math.round((v || 0) * 65536);
function draw(f, ms) {
  if (!take(f.src)) return null;
  const d = new Date();
  return T(() => {
    P.frame(ms, d.getHours(), d.getMinutes(), d.getSeconds(), Q(f.a), Q(f.b), Q(f.c));
    return new Uint8Array(P.mem.buffer, P.pixels(), 12288);
  }, null);
}
const info = (src) => (take(src) && T(() => P.info(), null)) || [];

// A formula's own slider names: `-- a: speed`.
const sliderName = (src, k) => (src.match(new RegExp(`^\\s*(?:--|//)\\s*${k}\\s*[:=]\\s*(.+)$`, 'm')) || [])[1]?.trim() || k;
const cost = (us) => (us <= 5000 ? 'light' : us <= 8000 ? 'medium' : 'heavy');
const about = (i) => i.length ? [i[5] & 8 && `Loops every ${i[4] / 1000} s`, i[5] & 16 && 'Follows the clock', !(i[5] & 24) && 'Still', `${cost(i[6])} for the panel`].filter(Boolean).join(' · ') : '';
const TIP = (i) => `${i[0]} steps, about ${(i[6] / 1000).toFixed(1)} ms a frame on the panel (light: under 5 ms; medium: under 8)`;
const NOTE = () => (app.device ? 'Formulas play here in the browser for now. Save one as a GIF to play it on the panel.' : 'Formulas play here in the browser. Save one as a GIF to play it on an OpenDMD panel.');

// A preview: the LED canvas, the formula it plays and its own clock.
const views = new Set();
let raf = 0;
function loop(now) {
  let on = false;
  for (const v of views) {
    if (!v.f || v.canvas.offsetParent === null) continue;
    on = true;
    const n = Math.max(1, Math.floor((v.loopMs || 8000) / D)), i = Math.floor((now - v.t0) / D) % n;
    if (i === v.i && !v.dirty) continue;
    v.i = i; v.dirty = false;
    const rgb = draw(v.f, i * D);
    if (rgb) { v.pv.p.showRgb(rgb); v.canvas.dataset.frames = -~v.canvas.dataset.frames; }
  }
  // Nothing on screen: stop (kick() starts it again).
  raf = on ? requestAnimationFrame(loop) : 0;
}
export const kick = () => { raf ||= requestAnimationFrame(loop); };
function view(canvas) {
  const v = { canvas, pv: new PanelPreview(canvas), f: null, t0: performance.now(), i: -1, dirty: true };
  v.pv.p.setTransition('none', 0);
  views.add(v);
  return v;
}
function play(v, f, i) {
  if (v.f?.src !== f.src) v.t0 = performance.now();
  v.f = f; v.loopMs = i[4]; v.dirty = true;
  kick();
}

// Sliders for the a, b, c a formula reads, in `box`; `set` on a change.
function sliders(box, f, mask, set) {
  clear(box, ['a', 'b', 'c'].map((k, j) => {
    if (!(mask & (1 << j))) return null;
    const out = h('output.mono', (f[k] || 0).toFixed(2));
    const r = h('input', { type: 'range', min: 0, max: 1, step: 0.01, value: f[k] || 0, id: `${box.id}-${k}`,
      oninput: () => { f[k] = +r.value; out.textContent = f[k].toFixed(2); set(); } });
    return h('label.psi-sl', h('span', sliderName(f.src, k)), r, out);
  }));
}

// --- Saving and sharing
const mine = () => { const m = store.get('psi.mine', []); return Array.isArray(m) ? m.filter((x) => typeof x?.src == 'string') : []; };
const keep = (list) => store.set('psi.mine', list);
const stored = () => { const d = store.get('psi.draft', null); return typeof d?.src == 'string' ? d : null; };
// A draft with changes not saved: its text or name differs from where it started.
const sig = (f) => f.src + '\n\0' + (f.name || '');
const dirty = (d) => d?.base != null && sig(d) !== d.base;

function link(f, base = location.origin + location.pathname) {
  const q = new URLSearchParams({ formula: f.src });
  if (f.name) q.set('name', f.name);
  const m = info(f.src)[5] || 0;
  for (const [j, k] of ['a', 'b', 'c'].entries()) if (m & (1 << j) && f[k]) q.set(k, +f[k].toFixed(2));
  return base + '#' + q;
}
// `#formula=…&name=…&a=…` (app.js keeps it in app.formula).
function fromLink(s) {
  const q = new URLSearchParams(s), src = q.get('formula') || '', name = q.get('name') || 'Shared formula';
  const f = { name: name.slice(0, 60), src: src.slice(0, 4096) };
  if (f.src !== src) toast('The shared formula was too long: only its first 4,096 characters are here', 'bad');
  for (const k of 'abc') if (q.has(k)) f[k] = Math.min(1, Math.max(0, +q.get(k) || 0));
  f.base = sig(f);
  return f;
}
async function copy(text) {
  try { await navigator.clipboard.writeText(text); return true; } catch { /* not a secure context: the old way */ }
  const t = h('textarea', { value: text, readOnly: true, style: 'position:fixed;opacity:0' });
  document.body.append(t);
  t.select();
  const ok = document.execCommand('copy');
  t.remove();
  return ok;
}
const DEMO = 'https://opendmd.github.io/demo/';
function share(btn, f) {
  const r = btn.getBoundingClientRect(), url = link(f, DEMO), here = app.device && link(f);
  const field = h('input', { type: 'text', id: 'psi-link', readOnly: true, value: url, onfocus: (e) => e.target.select() });
  const go = (u) => async () => { closeMenu(); toast((await copy(u)) ? 'Link copied' : 'Select the link and copy it', 'ok'); };
  popup(h('div.menu.wide', { id: 'psi-share', role: 'dialog', 'aria-label': 'Share' }, h('div.mh', 'Share this formula'), field,
    h('button.mi', { id: 'psi-copy', onclick: go(url) }, 'Copy link'),
    h('p.cap', 'It opens in the online demo, for anyone.'),
    url.length > 1500 && h('p.cap.bad', `This link is long (${url.length.toLocaleString()} characters): some apps cut long links`),
    here && h('button.mi', { id: 'psi-copy-here', 'data-url': here, onclick: go(here) }, 'Copy a link to this display')), r.left, r.bottom + 4);
}

// "Save as GIF…": one loop drawn here, into Create Art (its timeline, text
// and save, as for an effect): 20 ms frames, 40 ms past 240 of them, cut at 240.
function toGif(f) {
  const ms = info(f.src)[4];
  if (!ms) { toast('Fix the formula first: it doesn’t work yet', 'bad'); return; }
  let d = D;
  if (ms / d > 240) d = 40;
  const all = Math.max(1, Math.floor(ms / d)), n = Math.min(all, 240);
  app.artFrom = { title: f.name || 'Formula', frames: { n, delay: d, cut: all > n, frame: (i) => draw(f, i * d) } };
  app.go('create');
}

// --- The tab: the gallery, the details, the editor
const STARTER = `-- x: 0-1 across, y: 0-1 up
-- t: seconds. Drag numbers!
loop 8
v = sin(x*2 - t/8)
hsv(x/2 + t/8, 1, v/2 + 0.5)
`;
let B = null; // the built elements and state
let EX = [];

export async function mount(el) {
  build(el);
  await B.ready;
  return {
    show() { if (!B.editing) B.tiles(); kick(); },
    sub(s) {
      if (s !== 'edit') { if (B.editing) close(false); } else if (!B.editing || app.formula) edit();
      kick();
    },
  };
}

// An in-page question (a sheet on a phone): `yes` or `no`, Escape is neither.
function ask(text, [y, yes], [n, no], at) {
  const r = at?.getBoundingClientRect?.() || { left: innerWidth / 2 - 125, bottom: 120 };
  const b = (t, fn, c = '') => h('button.mi' + c, { onclick: () => { closeMenu(); fn(); } }, t);
  popup(h('div.menu', { id: 'psi-ask', role: 'alertdialog', 'aria-label': text }, h('p', { style: 'margin:4px 8px' }, text), b(y, yes, '.danger'), b(n, no)), r.left, r.bottom + 4);
}
const unsaved = () => { const d = B.draft || stored(); return dirty(d) && d; };

function build(root) {
  const gal = h('div', { id: 'psi-gal' });
  const canvas = h('canvas.panel', { id: 'psi-panel' });
  const cap = h('div.cap', { id: 'psi-cap' });
  const title = h('h2', { id: 'psi-d-title' }), desc = h('p.cap', { id: 'psi-d-desc' });
  const sl = h('div.psi-sls', { id: 'psi-sl' });
  const main = h('button.pri', { id: 'psi-edit' }), gifBtn = h('button', { id: 'psi-gif' }, 'Save as GIF…');
  const shareBtn = h('button', { id: 'psi-share-btn' }, 'Copy share link'), del = h('button.danger', { id: 'psi-del' }, 'Delete…');
  const side = h('div.side', { id: 'psi-side' }, h('div.card', { id: 'psi-detail' },
    h('button.sm.close', { 'aria-label': 'Close', onclick: () => side.classList.remove('open') }, '✕'),
    canvas, cap, h('div.dhead', h('div.grow', title, desc)), sl,
    h('div.row.acts', main, gifBtn), h('div.row.acts', shareBtn, del), h('p.cap', NOTE())));
  const ed = editor();
  root.append(h('h1.ptitle', 'Formulas'), h('div.libgrid', gal, side), ed.el);
  addEventListener('keydown', (e) => { if (e.key === 'Escape' && side.classList.contains('open') && !document.querySelector('.menu')) side.classList.remove('open'); });
  const pv = view(canvas);
  let cur = null;
  function pick(f, mineToo) {
    cur = f;
    side.classList.add('open');
    const i = info(f.src);
    title.textContent = f.name;
    desc.textContent = f.desc || '';
    cap.textContent = i.length ? about(i) : `This formula doesn’t work any more: ${T(() => P.error(), '')}`;
    cap.title = i.length ? TIP(i) : '';
    sliders(sl, f, i[5] || 0, () => { pv.dirty = true; });
    main.textContent = mineToo ? 'Edit' : 'Remix';
    main.onclick = (e) => go(mineToo ? { ...f } : { name: f.name + ' remix', src: f.src, a: f.a, b: f.b, c: f.c }, e.currentTarget);
    gifBtn.onclick = () => toGif(f);
    shareBtn.onclick = (e) => share(e.currentTarget, f);
    del.hidden = !mineToo;
    del.onclick = (e) => ask(`Delete “${f.name}”?`, ['Delete', () => { keep(mine().filter((x) => x.id !== f.id)); side.classList.remove('open'); cur = null; tiles(); }], ['Keep it', () => {}], e.currentTarget);
    for (const t of gal.querySelectorAll('.psi-tile')) t.classList.toggle('cur', t._f === f);
    if (i.length) play(pv, f, i); else { pv.f = null; pv.pv.clear(); }
  }
  function tile(f, m) {
    const c = h('canvas', { width: 128, height: 32 });
    const t = h('button.psi-tile', { 'data-name': f.name, onclick: () => pick(f, m) }, c, h('b', f.name), f.desc && h('span.cap', f.desc));
    t._f = f;
    const rgb = draw(f, Math.floor((info(f.src)[4] || 8000) * 0.3 / D) * D);
    if (rgb) {
      const img = new ImageData(128, 32);
      for (let p = 0, q = 0; p < 12288; p += 3, q += 4) img.data.set([rgb[p], rgb[p + 1], rgb[p + 2], 255], q);
      c.getContext('2d').putImageData(img, 0, 0);
    }
    return t;
  }
  function tiles() {
    const ms = mine(), d = unsaved();
    clear(gal,
      d && h('div.card.row', { id: 'psi-cont' }, h('span.grow', 'Not saved yet: “', h('b', d.name || 'Untitled formula'), '”'),
        h('button.pri', { id: 'psi-cont-go', onclick: () => { B.draft = d; B.pushed = true; app.go('formulas/edit'); } }, 'Continue editing'),
        h('button.txt', { id: 'psi-cont-drop', onclick: () => { B.draft = null; store.set('psi.draft', null); tiles(); } }, 'Discard')),
      h('div.card.psi-intro', h('p', 'Formulas are a few lines of maths that draw on the panel, every pixel at once. Pick one to play with it, change a number and watch.'),
        h('div.row', h('button.pri', { id: 'psi-new', onclick: (e) => go({ name: '', src: STARTER }, e.currentTarget) }, '+ New formula'),
          h('span.cap', 'Written in Psi ', h('span.psi-mark', { title: 'Psi, the language formulas are written in' }, 'Ψ')))),
      ms.length > 0 && h('div.card', h('h3', 'Your formulas'), h('div.psi-tiles', { id: 'psi-mine' }, ms.map((f) => tile(f, true)))),
      h('div.card', h('h3', 'Examples'), h('div.psi-tiles', { id: 'psi-ex' }, EX.map((f) => tile(f, false)))));
    if (cur) for (const t of gal.querySelectorAll('.psi-tile')) t.classList.toggle('cur', t._f.src === cur.src && t._f.name === cur.name);
    // Two columns: the details are never empty (a sheet on a phone waits for a pick).
    else if (EX[0] && matchMedia('(min-width:1001px)').matches) pick(EX[0], false);
  }
  B = { root, ed, tiles, editing: false, draft: null };
  B.ready = Promise.all([psi(), new Promise((res) => document.head.append(h('link', { rel: 'stylesheet', href: './formulas.css', onload: res, onerror: res }))), fetch('formulas.json').then((r) => r.json()).then((x) => { EX = Array.isArray(x) ? x : []; })]).then(tiles)
    .catch((e) => { clear(gal, h('div.card', h('p.bad', 'Formulas could not be loaded: ' + (e.message || e)))); });
}

// --- The editor (`#formulas/edit`)
const VARS = [['x, y', 'where the pixel is: 0 to 1 across, and up from the bottom'], ['px, py', 'the pixel’s column 0–127 and row 0–31 from the bottom'],
  ['t', 'seconds into the loop'], ['hour, minute, second', 'the time of day'], ['a, b, c', 'sliders, 0 to 1 (name one with -- a: speed)'],
  ['w, h', '128 and 32'], ['i', 'the pixel’s number, 0–4095']];
const FUNCS = [['sin(v) cos(v)', 'a wave: one turn per 1, between −1 and 1'], ['tri(v) saw(v) sqr(v)', 'triangle, sawtooth and square waves'],
  ['abs floor frac sqrt', 'size, whole part, fraction, square root'], ['min max clamp(v, lo, hi)', 'limits'], ['mix(a, b, k)', 'from a to b as k goes 0 to 1'],
  ['step(edge, v)', '1 past the edge, else 0'], ['smoothstep(e0, e1, v)', 'a soft step'], ['len(dx, dy)', 'distance'], ['atan2(dy, dx)', 'angle, in turns'],
  ['noise(x, y, z)', 'smooth random hills, 0 to 1'], ['hash(a, b)', 'random 0 to 1'], ['select(c, a, b)', 'a when c is true (not 0), else b; or c ? a : b'],
  ['< <= > >= == !=', 'compare: 1 or 0; and, or, not'], ['%', 'remainder: v % 1 is the fraction'], [';', 'a new line, on the same line'],
  ['loop 8', 'the loop’s length in seconds (first line)'], ['frame { … }', 'values worked out once a frame']];
const COLS = [['hsv(hue, sat, val)', 'colour: hue in turns, 0 to 1 each'], ['rgb(r, g, b)', 'red, green, blue: 0 to 1'],
  ['pal(fire, v)', 'a palette: fire, ice, ocean, chrono, amber, rainbow, gray'], ['gray(v)', 'white, 0 to 1']];

function editor() {
  const name = h('input', { type: 'text', id: 'psi-name', placeholder: 'Untitled formula', maxLength: 60, 'aria-label': 'Name', oninput: () => { B.draft.name = name.value; stash(); } });
  const head = h('div.card.psi-edhead', h('button', { id: 'psi-back', onclick: () => close(true) }, '‹ Formulas'), name,
    h('button.pri', { id: 'psi-save', onclick: save }, 'Save'));
  const said = saver(head);
  const ta = h('textarea', { id: 'psi-src', spellcheck: false, autocapitalize: 'off', autocomplete: 'off', wrap: 'off', 'aria-label': 'Formula', 'aria-describedby': 'psi-err' });
  ta.setAttribute('autocorrect', 'off');
  const hl = h('pre.psi-hl', { 'aria-hidden': 'true' }), gut = h('div.psi-gut', { 'aria-hidden': 'true' });
  const err = h('div.cap.bad', { id: 'psi-err', 'aria-live': 'polite' });
  const val = h('b.mono', { id: 'psi-num' });
  const pad = h('div.psi-pad', { id: 'psi-pad', role: 'slider', 'aria-label': 'Drag to change the number' }, '◀ ', val, ' ▶');
  const nudge = (k) => h('button.sm', { 'aria-label': k > 0 ? 'More' : 'Less', onpointerdown: (e) => e.preventDefault(), onclick: () => bump(k) }, k > 0 ? '+' : '−');
  const strip = h('div.psi-strip', { id: 'psi-scrub', hidden: true }, nudge(-1), pad, nudge(1));
  const help = h('button.sm', { id: 'psi-help', 'aria-haspopup': 'dialog', onclick: (e) => cheat(e.currentTarget) }, 'Cheat sheet');
  const canvas = h('canvas.panel', { id: 'psi-ed-panel' });
  const cap = h('div.cap', { id: 'psi-ed-cap' }), heavy = h('p.cap', { id: 'psi-heavy', hidden: true },
    'Heavy for the panel: noise, len, sqrt, atan2 and dividing by a changing value cost the most');
  const sl = h('div.psi-sls', { id: 'psi-ed-sl' });
  const touch = matchMedia('(pointer:coarse)').matches;
  // One card: the preview (stuck to the top on a phone), the sliders, the
  // error over the code, the code, the actions.
  const top = h('div.psi-top', canvas, cap);
  const el = h('div', { id: 'psi-ed' }, head, h('div.card.psi-edb', top, h('div.psi-sl2', sl, heavy), err,
    h('div.psi-code', h('div.psi-src', gut, h('div.psi-wrap', hl, ta)), strip,
      h('div.row.acts', help, h('span.cap', touch ? 'Tap a number, then drag the bar under the code to try other values' : 'Drag a number (or Alt + ↑ ↓) to try other values'))),
    h('div.psi-acts', h('div.row.acts', h('button', { id: 'psi-ed-gif', onclick: () => toGif(B.draft) }, 'Save as GIF…'),
      h('button', { id: 'psi-ed-share', onclick: (e) => share(e.currentTarget, B.draft) }, 'Copy share link')), h('p.cap', NOTE()))));
  let pv = null, toks = [], bad = null, num = null, good = null;
  // What the preview plays: the last text that compiled, with the sliders as they are.
  const live = { get src() { return good; }, get a() { return B.draft.a; }, get b() { return B.draft.b; }, get c() { return B.draft.c; } };

  const stash = debounce(() => store.set('psi.draft', B.draft), 300);
  // Numbers (a unary minus included), comments and the error, drawn under the (transparent) text.
  function paint() {
    const s = ta.value;
    toks = [];
    const cls = new Array(s.length).fill('');
    for (const m of s.matchAll(/(--|\/\/).*|[A-Za-z_]\w*|\d+\.?\d*|\.\d+/g)) {
      const k = m[1] ? 'c' : /[\d.]/.test(m[0][0]) ? 'n' : '';
      let a = m.index;
      if (k === 'n') {
        // `-2` after an operator, a bracket or the start of a line is one number.
        if (s[a - 1] === '-' && /[-(,=*/+?:<>!%;\n]|^$/.test(s.slice(0, a - 1).trimEnd().slice(-1))) a--;
        toks.push({ s: a, e: m.index + m[0].length });
      }
      if (k) cls.fill(k, a, m.index + m[0].length);
    }
    if (bad) for (let i = bad[0]; i < Math.max(bad[1], bad[0] + 1) && i < s.length; i++) cls[i] += ' e';
    const esc = (t) => t.replace(/&/g, '&amp;').replace(/</g, '&lt;');
    let out = '', i = 0;
    while (i < s.length) {
      let j = i;
      while (j < s.length && cls[j] === cls[i]) j++;
      out += cls[i] ? `<span class="${cls[i]}">${esc(s.slice(i, j))}</span>` : esc(s.slice(i, j));
      i = j;
    }
    if (bad && bad[0] >= s.length) out += '<span class=" e"> </span>';
    hl.innerHTML = out + '\n';
    const n = s.split('\n').length, el = bad && s.slice(0, bad[0]).split('\n').length;
    gut.innerHTML = Array.from({ length: n }, (_, k) => (k + 1 === el ? `<b>${k + 1}</b>` : k + 1)).join('\n');
    ta.style.height = `calc(${Math.max(8, n + 1) * 1.5}em + 18px)`;
  }
  const build = debounce(compile, 150);
  function compile() {
    const f = B.draft;
    f.src = ta.value;
    // (The module restarting: the next compile catches up.)
    const ok = T(() => P.compile(f.src), null);
    if (ok == null) return;
    held = ok ? f.src : null;
    if (ok) {
      bad = null;
      err.textContent = '';
      const i = info(f.src);
      cap.textContent = about(i);
      cap.title = TIP(i);
      heavy.hidden = i[6] <= 8000;
      el.dataset.steps = i[0];
      delete el.dataset.err;
      good = f.src;
      top.classList.remove('stale');
      sliders(sl, f, i[5], () => { stash(); pv.dirty = true; });
      play(pv, live, i);
    } else {
      bad = T(() => P.errorAt(), [0, 0]);
      top.classList.toggle('stale', good != null);
      const line = ta.value.slice(0, bad[0]).split('\n').length;
      el.dataset.err = line;
      clear(err, `Line ${line}: `, T(() => P.error(), '').split('`').map((t, k) => (k % 2 ? h('code', t) : t)));
      paint();
    }
    stash();
  }
  ta.addEventListener('input', () => { paint(); scrubbable(); build(); });
  ta.addEventListener('scroll', () => { hl.scrollLeft = ta.scrollLeft; });

  // Draggable numbers: under the pointer (a mouse or pen, straight in the
  // text), Alt+↑↓ at the caret, and on touch the strip under the code.
  // Edits go through insertText so the browser's undo has them.
  const at = (pos) => toks.find((t) => t.s <= pos && pos <= t.e);
  function put(t, text, quiet) {
    // Focused: as typing (the browser's undo keeps it); not (a phone's strip
    // with the keyboard down), or mid-drag: straight in, no keyboard popping up.
    if (document.activeElement === ta && !quiet) {
      ta.setSelectionRange(t.s, t.e);
      document.execCommand('insertText', false, text);
    } else {
      ta.setRangeText(text, t.s, t.e, 'end');
      ta.dispatchEvent(new Event('input'));
    }
    t.e = t.s + text.length;
    ta.setSelectionRange(t.e, t.e);
  }
  // Below zero is fine (a minus that would make `--`, a comment, gets brackets).
  function fmt(t, v, d) {
    const x = (Math.abs(v) < 10 ** -d / 2 ? 0 : v).toFixed(d);
    return x[0] === '-' && ta.value[t.s - 1] === '-' ? `(${x})` : x;
  }
  const decs = (t) => (ta.value.slice(t.s, t.e).split('.')[1] || '').replace(/\D/g, '').length;
  const num0 = (t) => parseFloat(ta.value.slice(t.s, t.e).replace(/[()]/g, ''));
  function bump(k, t = num) {
    if (!t) return;
    const d = decs(t), step = d ? 10 ** -d : 1;
    put(t, fmt(t, num0(t) + k * step, d));
    paint();
    num = at(t.s) || at(t.s + 1);
    scrubbable();
  }
  function scrubbable() {
    num = ta.selectionStart === ta.selectionEnd || document.activeElement !== ta ? at(ta.selectionStart) : null;
    strip.hidden = !num || !touch;
    if (num) pad.setAttribute('aria-valuenow', val.textContent = ta.value.slice(num.s, num.e));
  }
  document.addEventListener('selectionchange', () => { if (document.activeElement === ta) scrubbable(); });
  ta.addEventListener('keydown', (e) => {
    if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown') && at(ta.selectionStart)) {
      e.preventDefault();
      bump((e.key === 'ArrowUp' ? 1 : -1) * (e.shiftKey ? 10 : 1), at(ta.selectionStart));
    }
  });
  // Where a pointer is in the text (monospace, no wrapping, tabs of 2).
  function posAt(e) {
    const r = ta.getBoundingClientRect(), s = getComputedStyle(ta);
    const cw = measure(), lh = parseFloat(s.lineHeight);
    const x = (e.clientX - r.left - parseFloat(s.paddingLeft) - ta.clientLeft + ta.scrollLeft) / cw;
    const y = e.clientY - r.top - parseFloat(s.paddingTop) - ta.clientTop + ta.scrollTop;
    const lines = ta.value.split('\n'), row = Math.floor(y / lh);
    if (row < 0 || row >= lines.length || x < 0) return -1;
    let col = 0, k = 0;
    for (const l = lines[row]; k < l.length; k++) {
      const w = l[k] === '\t' ? 2 - (col % 2) : 1;
      if (x < col + w) break;
      col += w;
    }
    return k < lines[row].length ? lines.slice(0, row).reduce((a, l) => a + l.length + 1, 0) + k : -1;
  }
  let cw = 0, cwFont = '';
  function measure() {
    const f = getComputedStyle(ta).fontSize;
    if (f !== cwFont) {
      const m = h('span', { style: 'position:absolute;visibility:hidden;white-space:pre' }, '0'.repeat(20));
      hl.append(m);
      cw = m.getBoundingClientRect().width / 20;
      m.remove();
      cwFont = f;
    }
    return cw;
  }
  // A drag starts once the pointer has moved 4 px sideways: a click or a
  // selection is the browser's as usual.
  let drag = null;
  function dragFrom(t, e) {
    drag = { t, x0: e.clientX, was: ta.value.slice(t.s, t.e), v0: num0(t), d: decs(t), moved: false, id: e.pointerId, el: e.currentTarget };
  }
  function dragMove(e) {
    if (!drag || e.pointerId !== drag.id) return false;
    const dx = e.clientX - drag.x0;
    if (!drag.moved) {
      if (Math.abs(dx) < 4) return true;
      drag.moved = true;
      drag.el.setPointerCapture(e.pointerId);
    }
    const step = (drag.d ? 10 ** -drag.d : 1) * (e.shiftKey ? 10 : 1);
    const text = fmt(drag.t, drag.v0 + Math.round(dx / 4) * step, drag.d);
    if (text !== ta.value.slice(drag.t.s, drag.t.e)) put(drag.t, text, true);
    if (drag.el === pad) pad.setAttribute('aria-valuenow', val.textContent = text);
    return true;
  }
  ta.addEventListener('pointermove', (e) => {
    if (dragMove(e)) return;
    if (e.pointerType !== 'touch') { const p = posAt(e); ta.style.cursor = p >= 0 && at(p) ? 'ew-resize' : ''; }
  });
  ta.addEventListener('pointerdown', (e) => {
    if (e.pointerType === 'touch' || e.button) return;
    const p = posAt(e), t = p >= 0 && at(p);
    if (t) dragFrom(t, e);
  });
  const end = (e) => {
    if (!drag || e.pointerId !== drag.id) return;
    if (drag.moved) {
      // One undo step for the whole drag: back to where it started, then the result as typed.
      const t = drag.t, now = ta.value.slice(t.s, t.e);
      put(t, drag.was, true);
      ta.focus({ preventScroll: true });
      put(t, now);
    }
    drag = null;
    scrubbable();
  };
  ta.addEventListener('pointerup', end);
  ta.addEventListener('pointercancel', end);
  pad.addEventListener('pointerdown', (e) => { if (num) { e.preventDefault(); dragFrom(num, e); } });
  pad.addEventListener('pointermove', dragMove);
  pad.addEventListener('pointerup', end);
  pad.addEventListener('pointercancel', end);

  function cheat(btn) {
    const r = btn.getBoundingClientRect();
    const ins = (t) => () => { closeMenu(); const s = ta.selectionStart; ta.focus(); ta.setSelectionRange(s, ta.selectionEnd); document.execCommand('insertText', false, t); };
    const rows = (title, list) => [h('h3', title), h('dl.psi-dl', list.map(([k, v]) => [h('dt', h('button.txt.mono', { onclick: ins(k.split(/[ ,(]/)[0]) }, k)), h('dd', v)]))];
    popup(h('div.menu.wide', { id: 'psi-cheat', role: 'dialog', 'aria-label': 'Cheat sheet' }, h('div.mh', 'Cheat sheet'),
      rows('Values', VARS), rows('Functions', FUNCS), rows('Colour (the last line)', COLS),
      h('p.cap', 'Phases are in turns: sin(0.25) is 1. Tap a name to put it in. -- starts a comment.')), r.left, r.bottom + 4);
  }

  function save() {
    const f = B.draft, list = mine();
    f.name = name.value.trim() || `My formula ${list.length + 1}`;
    name.value = f.name;
    f.id ??= Date.now();
    const k = list.findIndex((x) => x.id === f.id);
    const rec = { id: f.id, name: f.name, src: ta.value, a: f.a, b: f.b, c: f.c };
    if (k < 0) list.unshift(rec); else list[k] = rec;
    said(() => {
      if (!keep(list)) { const m = 'this browser’s storage is full or blocked'; toast('Couldn’t save: ' + m, 'bad'); throw new Error(m); }
      f.base = sig({ src: ta.value, name: f.name });
      stash();
    });
  }

  return {
    el,
    open(f) {
      B.draft = f;
      f.base ??= sig(f);
      name.value = f.name || '';
      ta.value = f.src;
      bad = null;
      good = null;
      paint();
      pv ??= view(canvas);
      compile();
      scrubbable();
      if (matchMedia('(pointer:fine)').matches) ta.focus({ preventScroll: true });
    },
    // After a restart: the module is new, compile the text again.
    again() { if (B.editing) compile(); },
  };
}

// Into the editor from the gallery (the way back is the browser's); a
// draft not saved yet is only replaced when the user says so.
function go(f, at) {
  const d = unsaved(), to = (x) => () => { B.draft = x; B.pushed = true; app.go('formulas/edit'); };
  f.base = sig(f);
  if (d) ask(`Replace “${d.name || 'Untitled formula'}”? It isn’t saved yet.`, ['Replace it', to(f)], ['Keep editing it', to(d)], at);
  else to(f)();
}
function edit() {
  if (!P) return; // the module didn't load: the gallery says so; a link waits for a reload
  B.draft ??= stored() || { name: '', src: STARTER };
  let later;
  if (app.formula) {
    const f = fromLink(app.formula), d = unsaved();
    app.formula = null;
    B.pushed = false;
    if (location.hash.startsWith('#formula=')) location.replace('#formulas/edit');
    if (d && sig(d) !== sig(f)) later = () => ask(`Open the shared formula? “${d.name || 'Untitled formula'}” isn’t saved yet and will be replaced.`, ['Open it', () => B.ed.open(f)], ['Keep mine', () => {}]);
    else B.draft = f;
  }
  B.editing = true;
  B.root.classList.add('psied');
  B.ed.open(B.draft);
  scrollTo(0, 0);
  later?.();
}
function close(nav) {
  B.editing = false;
  B.root.classList.remove('psied');
  closeMenu();
  B.tiles();
  if (nav) { if (B.pushed) history.back(); else location.replace('#formulas'); }
  B.pushed = false;
}
