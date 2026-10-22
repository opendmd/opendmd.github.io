// Library → Live and Effects (loaded when either category is chosen):
// things the panel draws itself rather than GIFs — the clock, a scrolling
// text, and the four effects. They are items like any other: preview, send
// to the panel, add to a playlist (the playlist names them as `@clock…` /
// `@text?msg=…` / `@fx:…` lines, which the player and this preview draw
// with the same code).

import { app } from './state.js';
import { h } from './util.js';

// Each its own row title (the player's title for all of them is "Clock").
const CLOCKS = [['@clock', 'hours and minutes', 'Clock'], ['@clock?seconds=1', 'with seconds', 'Clock with seconds'], ['@clock?date=1', 'with the date', 'Clock with date'],
  ['@clock?style=small&seconds=1&date=1', 'small, seconds and date', 'Small clock with seconds and date']];

// The items, and the text's form in `box`; `preview(item)` shows one,
// `refresh()` redraws the list after the text changed.
export function items(box, preview, refresh) {
  const item = (path, name, t) => {
    const e = app.wasm.parsePath(path);
    if (t) e.title = t;
    return { path, e, name, norm: app.wasm.normalizeTitle(e.title) };
  };
  const msg = h('input', { type: 'text', id: 'live-msg', value: 'OpenDMD', maxLength: 200 });
  const speed = h('select', { id: 'live-speed' }, [[60, 'Slow'], [40, 'Normal'], [25, 'Fast']].map(([v, l]) => h('option', { value: v }, l)));
  speed.value = 40;
  const text = {};
  const set = () => Object.assign(text, item(`@text?msg=${encodeURIComponent(msg.value.trim() || 'OpenDMD')}${speed.value != 40 ? '&speed=' + speed.value : ''}`, 'scrolling text'));
  // The row follows the typing; the preview shows it on Enter (or a click
  // on the row). A refresh on blur would land between a click's mousedown
  // and mouseup on another row and swallow the click.
  msg.oninput = () => { set(); refresh(); };
  msg.onkeyup = (e) => e.key == 'Enter' && preview(text);
  speed.onchange = () => { set(); refresh(); preview(text); };
  set();
  box.replaceChildren(h('div.form', { style: 'margin-top:12px' }, h('label.stack', 'Scrolling text', msg), h('label.stack', 'Speed', speed)),
    h('p.cap', 'The clock shows the player’s own time; add these to a playlist like any GIF'));
  return [...CLOCKS.map(([p, n, t]) => item(p, n, t)), text];
}

// Effects: each row's controls (shown when it is picked) are its options;
// the row's path follows them, leaving out the defaults.
const PAL = ['chrono', 'fire', 'ice', 'amber', 'rainbow'];
const SPEED = [[1, 'slow'], [2, 'normal'], [3, 'fast'], [4, 'fastest']];
const ONOFF = [[0, 'off'], [1, 'on']];
const TEXTURE = ['checker', 'rings', 'noise'];
const FPS = [[100, '100'], [50, '50'], [25, '25']];
// The options whose value goes in a saved GIF's suggested title: "Tunnel Fire Rings".
const WORDS = ['palette', 'texture', 'shape', 'color', 'floor', 'warp'];
const cap = (t) => t[0].toUpperCase() + t.slice(1);
const FX = [
  ['tunnel', 'the time warp', { palette: [PAL, 'chrono'], texture: [TEXTURE, 'checker'], layers: [[[2, 'two'], [1, 'one']], 2], speed: [SPEED, 2], twist: [[0, 1, 2, 3], 1], 'colour cycling': [ONOFF, 1, 'cycle'], drift: [ONOFF, 0], 'frames/s': [FPS, 100, 'fps'] }],
  ['rotozoom', 'Mode 7 turn, zoom and warp', { palette: [PAL, 'fire'], texture: [TEXTURE, 'checker'], layers: [[[2, 'two'], [1, 'one']], 2], wobble: [[[0, 'none'], [1, 'gentle'], [2, 'wavy'], [3, 'wild']], 2], speed: [SPEED, 2], zoom: [[1, 2, 3, 4], 2], view: [[[0, 'flat'], [1, 'floor']], 0, 'floor'], 'colour cycling': [ONOFF, 0, 'cycle'], 'frames/s': [FPS, 100, 'fps'] }],
  ['starfield', 'flying through stars', { palette: [PAL, 'ice'], speed: [SPEED, 2], stars: [[[1, 'some'], [2, 'many'], [3, 'lots']], 2, 'density'], warp: [[[1, 'streaks'], [0, 'dots']], 1], 'frames/s': [FPS, 100, 'fps'] }],
  ['wire', 'turning wireframes', { shape: [['morph', 'cube', 'octahedron', 'icosahedron', 'torus', 'tesseract'], 'morph'], speed: [SPEED, 2], colour: [[['40c0ff', 'cyan'], ['ffb040', 'amber'], ['ff4040', 'red'], ['60ff60', 'green'], ['ffffff', 'white']], '40c0ff', 'color'], depth: [[[1, 'shaded'], [0, 'flat']], 1, 'shade'], 'frames/s': [FPS, 100, 'fps'] }],
];

export function effects(box, preview, refresh) {
  box.replaceChildren(h('p.cap', 'Pick an effect for its options'));
  return FX.map(([fx, name, opts]) => {
    const it = { name };
    const sels = Object.entries(opts).map(([label, [vals, def, key = label]]) => {
      const s = h('select', { 'data-key': key, 'data-def': def }, vals.map((v) => { const [val, text = val] = [].concat(v); return h('option', { value: val }, cap(String(text))); }));
      s.value = def;
      s.onchange = () => { set(); refresh(); preview(it); };
      return h('label.stack', cap(label), s);
    });
    const form = h('div.form', { id: 'fx-' + fx, style: 'margin-top:12px;grid-template-columns:repeat(auto-fill,minmax(110px,1fr))' }, sels);
    const set = () => {
      const q = sels.map((l) => l.lastChild).filter((s) => s.value != s.dataset.def).map((s) => s.dataset.key + '=' + s.value).join('&');
      const path = `@fx:${fx}${q ? '?' + q : ''}`;
      Object.assign(it, { path, e: app.wasm.parsePath(path) }).norm = app.wasm.normalizeTitle(it.e.title);
    };
    // "Save as GIF…": Create Art draws one loop of it (`app.artFrom.fx`), to
    // add text, trim and save like any animation; titled from what is set.
    const gif = h('button', { id: 'fx-gif', onclick: () => {
      const t = sels.map((l) => l.lastChild).filter((s) => WORDS.includes(s.dataset.key) && s.value != s.dataset.def).map((s) => cap(s.selectedOptions[0].text));
      app.artFrom = { fx: it.path, title: [it.e.title, ...t].join(' ') };
      app.go('create');
    } }, 'Save as GIF…');
    it.pick = () => box.firstChild != form && box.replaceChildren(form, h('div.row.acts', { style: 'margin-top:12px' }, gif),
      h('p.cap', 'Drawn live by the panel; add it to a playlist like any GIF, or save it as a GIF of your own to add text and trim it'));
    set();
    return it;
  });
}
