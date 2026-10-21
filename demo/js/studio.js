// The Studio: the "Create Art" view. A picture or an animated GIF in (found
// on the web — Game art, GifCities —, your own file, drop, paste or
// address, or a GIF from the Library's "Edit in Create Art"), a 128x32 GIF
// out, in six steps on one page: Find, Frame, Look, Text, Preview (with the
// timeline for an animation), Save. Until a picture is in only Find shows;
// then Find folds to one line ("Change picture" opens it again, its search
// as it was) and the rest appear.
//
// An animated GIF is decoded in the art module (every frame composited as
// the player does, kept there), in ~12 ms slices with the page given back
// between them, as Smooth motion does. Frame, Look and Text apply to every
// frame; frames are rendered when shown (the preview is fed one frame at a
// time, at the GIF's own timing) and all of them when it is saved. The
// timeline trims, deletes, speeds up or slows down, reverses and ping-pongs;
// the order and delays are the module's (`setEdit`), not worked out here.
//
// Loaded when the view is first opened, with its stylesheet and its own
// wasm module (pkg-art/: the pixel work and the GIF writer,
// opendmd-art-web), so the app everyone loads does not grow. The picture is decoded by the browser into RGBA and handed to a
// wasm `Studio`, which keeps it; every change of a control re-renders the
// GIF there and the preview plays it. Save goes through Library's Add GIFs
// path (the GIF into a category, its prepared copy beside it) — or, with no
// device, downloads the GIF. The file name is built from the tag grammar
// and read back with the core's parser, so what the Library will list is
// what the user sees before saving.
//
// Find's web sources are js/find.js' (Game art: a name index per platform
// on the card, pictures from libretro-thumbnails; GifCities); a click on a
// result loads it. The Library's "Search the web" hands its words over as
// `app.artFind`. Nothing here says where anything is stored.
//
// Smooth motion (js/smooth.js, imported when the switch is first used or a
// listed GIF is opened): the switch beside speed and reverse analyses what
// Save would write — a draft through the same save path — and the preview
// then plays the smoothed frames; any edit makes the draft again (debounced).
// Save keeps the choice: the GIF listed or not, and its copy made to match.

import { app } from './state.js';
import { PanelPreview, fitScale } from './preview.js';
import { h, clear, toast, debounce, label, base, emit, niceName, byNice, note, store } from './util.js';
import { art as artModule } from './artmod.js';
import { addOne, offerPlaylist } from './library.js';
import { listings, pickName, renameGif } from './details.js';
import { prepareBytes } from './prepare.js';
import { PLATFORMS, sources, mountResults, style } from './find.js';

const SPEEDS = [['Slow', 50], ['Normal', 40], ['Fast', 30]];
const RATES = [25, 50, 75, 100, 150, 200, 300, 400];
const PLAYS = [['Plays forever', 0], ['Plays once', 1], ['Plays 2 times', 2], ['Plays 3 times', 3], ['Plays 5 times', 5], ['Plays 10 times', 10]];
const CW = 136; // a timeline cell, px (a 128x32 thumbnail at 1:1)
const later = () => new Promise((r) => setTimeout(r, 0));
const secs = (ms) => { const s = ms / 1000; return s < 10 ? s.toFixed(1) : String(Math.round(s)); };

// Call `step` (a little wasm work; true when done) until done, giving the
// page back every ~12 ms. False if `alive()` said stop.
async function run(step, alive, progress) {
  for (;;) {
    const t = performance.now();
    do { if (step()) return true; } while (performance.now() - t < 12);
    progress?.();
    await later();
    if (alive && !alive()) return false;
  }
}
const HOLDS = [['No pause', 0], ['Pause 1.5 s', 1500], ['Pause 3 s', 3000]];
const MAX_PX = 4e6; // bigger pictures are scaled down on the way in

let art = null; // the wasm module

// The view: built once, into `root`.
export async function mount(root) {
  [art] = await Promise.all([artModule(), css(), style()]);
  return build(root);
}

function css() {
  return new Promise((res, rej) => {
    const l = h('link', { rel: 'stylesheet', href: './studio.css', onload: res, onerror: () => rej(new Error('could not load the Studio')) });
    document.head.append(l);
  });
}

function build(root) {
  const S = { studio: null, fit: 'pixels', motion: 'still', x: 0, y: 0, gif: null, opts: null, tok: 0, from: null, listing: null, cats: [] };
  const id = (x) => 'st-' + x;
  const seg = (name, items, onpick) => {
    const el = h('div.st-seg', { id: id(name), role: 'group' }, items.map(([v, t]) => h('button', { 'data-v': v, onclick: () => onpick(v) }, t)));
    el.set = (v) => { for (const b of el.children) b.classList.toggle('on', b.dataset.v === String(v)); };
    return el;
  };
  const select = (name, items, value) => h('select', { id: id(name) }, items.map(([t, v]) => h('option', { value: v, selected: v === value }, t)));
  const num = (name, attrs) => h('input', { type: 'number', id: id(name), inputMode: 'numeric', ...attrs });
  // A numbered step; 2-6 are hidden until there is a picture.
  const step = (n, t, sid, ...kids) => h('section.card' + (n > 1 ? '.st-step' : ''), { id: id(sid) }, h('h2', h('span.muted', n), ' ', t), ...kids);

  // --- 1 Find: the web (find.js' sources) or your own picture.
  const SRC = [['gameart', 'Game art'], ['gifcities', 'GifCities'], ['own', 'Your own']];
  const file = h('input', { type: 'file', accept: 'image/*', hidden: true, id: id('file'), onchange: () => { if (file.files[0]) load(file.files[0]); file.value = ''; } });
  const url = h('input', { type: 'text', id: id('url'), inputMode: 'url', placeholder: 'Picture address (https://…)' });
  const msg = h('span.cap', { id: id('msg'), 'aria-live': 'polite' });
  const seen = h('a', { id: id('seen'), target: '_blank', rel: 'noopener', hidden: true }, 'Seen on…');
  const search = () => { S.searched = q.value; R.search(q.value); };
  const q = h('input', { type: 'search', id: id('q'), enterKeyHint: 'search', onkeydown: (e) => e.key === 'Enter' && search() });
  const narrow = h('div', { id: id('narrow') });
  const results = h('div', { id: id('results') });
  const src = seg('src', SRC, (v) => useSource(v, true));
  const web = h('div', { id: id('web') }, narrow, h('div.row.st-q', h('div.grow', q), h('button', { id: id('go'), onclick: search }, 'Search')), results);
  const own = h('div', { id: id('own'), hidden: true },
    h('div.row', h('label.btn', 'Choose a picture or GIF…', file), h('span.cap', 'or drop or paste one here')),
    h('div.row.st-url', h('div.grow', url), h('button', { id: id('url-go'), onclick: () => fromUrl(url.value.trim()) }, 'Load')));
  // Folded, "Change picture"; open with a picture in, "Keep this picture".
  const change = h('button.sm', { id: id('change'), onclick: () => fold(!el.classList.contains('found')) }, 'Change picture');
  const imp = h('section.card', { id: id('import') }, h('h2', h('span.muted', 1), ' Find'),
    h('div.row.st-got', h('div.grow', msg, ' ', seen), change),
    h('div.st-finder', h('div.row', src), web, own));
  const R = mountResults(results, { narrow, onPick: async (x) => {
    if (!(await load(await x.fetch(), { platform: x.platform, title: x.hint || '', logo: x.logo, label: x.title, link: x.link }))) throw new Error();
  } });
  function useSource(v, keep) {
    if (!SRC.some(([x]) => x === v)) v = 'gameart';
    S.src = v;
    if (keep) store.set('art.src', v);
    src.set(v);
    web.hidden = v === 'own';
    own.hidden = v !== 'own';
    if (v === 'own') return;
    const s = sources.find((x) => x.id === v);
    R.use(s);
    q.placeholder = s.placeholder;
    if (q.value.trim()) search();
  }
  // Folded: Find is one line (what is loaded, "Change picture"). Opened
  // again, words put in meanwhile (Edit in Create Art's) are searched.
  function fold(on, focus = true) {
    el.classList.toggle('found', on);
    change.textContent = on ? 'Change picture' : 'Keep this picture';
    if (on) return;
    if (S.src !== 'own' && q.value.trim() && q.value !== S.searched) search();
    if (focus) (S.src === 'own' ? url : q).focus();
  }

  // --- 2 Frame
  const cv = h('canvas', { id: id('src') });
  const win = h('div.st-win', { id: id('win') });
  const band = h('div.st-band');
  const view = h('div.st-view', { id: id('view') }, cv, band, win);
  const fit = seg('fit', [['pixels', 'Pixels'], ['half', 'Pixels ½'], ['fit', 'Fit']], (v) => { S.fit = v; restage(true); });
  const motion = seg('motion', [['still', 'Still'], ['pan', 'Pan →'], ['scroll', 'Scroll ↓']], (v) => {
    S.motion = v;
    if (!S.moved) [S.x, S.y] = S.studio.autoWindow(v);
    frame(); render();
  });
  const speed = select('speed', SPEEDS, 40);
  const hold = select('hold', HOLDS, 1500);
  const fcap = h('div.cap', { id: id('fcap') });
  const moveRow = h('div.row', motion, speed, hold);
  const frm = step(2, 'Frame', 'frame', view, fcap,
    h('div.row', fit, h('button.sm', { id: id('auto'), onclick: () => restage(true) }, 'Auto')), moveRow);

  // --- 3 Look
  const colours = select('colours', [['Auto colours', 0], ['256 colours', 256], ['64 colours', 64], ['16 colours', 16]], 0);
  const key = h('input', { type: 'checkbox', id: id('key') });
  const lift = h('input', { type: 'checkbox', id: id('lift') });
  const gain = h('input', { type: 'range', id: id('gain'), min: 30, max: 150, step: 5, value: 100 });
  const gout = h('output.mono', '100%');
  const look = step(3, 'Look', 'look',
    h('div.row', colours, h('label', key, 'Background to black')),
    h('div.row', h('label', lift, 'Brighten dark outlines (logos)')),
    h('div.row', h('label', 'Brightness', gain, gout)));

  // --- 4 Text: a line in one of the panel's two fonts, at one of nine
  // places, over all frames or some.
  const tmsg = h('input', { type: 'text', id: id('text'), placeholder: 'Text (optional)', maxLength: 40 });
  const font = seg('font', [['small', 'Small'], ['normal', 'Normal']], (v) => { S.font = v; font.set(v); text(); });
  const tcol = h('input', { type: 'color', id: id('tcolour'), value: '#ffffff', 'aria-label': 'Text colour' });
  const anchor = h('div.st-anchor', { id: id('anchor'), role: 'group', 'aria-label': 'Where' },
    [...Array(9)].map((_, i) => h('button', { 'data-v': i, 'aria-label': ['Top left', 'Top', 'Top right', 'Left', 'Middle', 'Right', 'Bottom left', 'Bottom', 'Bottom right'][i], onclick: () => { S.anchor = i; setAnchor(); text(); } })));
  const setAnchor = () => { for (const b of anchor.children) b.classList.toggle('on', +b.dataset.v === S.anchor); };
  const dx = num('dx', { value: 0, min: -128, max: 128, 'aria-label': 'Nudge right' });
  const dy = num('dy', { value: 0, min: -32, max: 32, 'aria-label': 'Nudge down' });
  const trange = select('trange', [['On every frame', 'all'], ['On frames…', 'some']], 'all');
  const tfrom = num('tfrom', { value: 1, min: 1, 'aria-label': 'From frame' });
  const tto = num('tto', { value: 1, min: 1, 'aria-label': 'To frame' });
  const rangeRow = h('div.row', { hidden: true }, trange, h('span', { id: id('tspan') }, tfrom, ' to ', tto));
  const txt = step(4, 'Text', 'text-step',
    h('div.row', h('div.grow', tmsg), tcol),
    h('div.row.st-tpos', anchor, h('div', h('div.row', font), h('div.row', h('label', '→', dx), h('label', '↓', dy)))),
    rangeRow);
  Object.assign(S, { font: 'normal', anchor: 7 });
  font.set('normal'); setAnchor();

  // --- 5 Preview (and, for an animation, the timeline)
  const panel = h('canvas.panel', { id: id('panel') });
  const stats = h('div.cap', { id: id('stats') });
  const playBtn = h('button.sm', { id: id('play'), 'aria-label': 'Play or pause', onclick: () => playPause() }, '❚❚');
  const scrub = h('input', { type: 'range', id: id('scrub'), min: 0, max: 0, value: 0, 'aria-label': 'Frame' });
  const posOut = h('output.mono', { id: id('pos') });
  const track = h('div.st-track');
  const strip = h('div.st-strip', { id: id('strip'), tabIndex: 0, 'aria-label': 'Frames' }, track);
  const hin = h('div.st-h.in', { id: id('in'), title: 'Start' }), hout = h('div.st-h.out', { id: id('out'), title: 'End' });
  track.append(hin, hout);
  const rate = select('rate', RATES.map((r) => [`Speed ${r / 100}×`, r]), 100);
  const rev = h('input', { type: 'checkbox', id: id('rev') });
  const pp = h('input', { type: 'checkbox', id: id('pp') });
  const plays = select('plays', PLAYS, 0);
  const smoothOn = h('input.sw', { type: 'checkbox', id: id('smooth') });
  const smoothCap = h('div.cap', { id: id('smooth-cap'), 'aria-live': 'polite' });
  const smoothRow = h('label', { id: id('smooth-row'), hidden: true }, smoothOn, 'Smooth motion');
  const undoBtn = h('button.sm', { id: id('undo'), disabled: true, onclick: () => undo() }, 'Undo');
  const tlcap = h('span', { id: id('tlcap') });
  const grp = (t, ...kids) => h('div', h('div.lbl', t), h('div.row', ...kids));
  const tl = h('div.st-tl', { id: id('tl'), hidden: true },
    h('div.row.st-play', playBtn, h('div.grow', scrub), posOut),
    strip, note(tlcap, 'Click a frame to see it; Shift- or Ctrl-click to pick more, then Delete'),
    h('div.row.st-ctl',
      grp('Trim',
        h('button.sm', { id: id('set-in'), onclick: () => trim('from') }, 'Start here'),
        h('button.sm', { id: id('set-out'), onclick: () => trim('to') }, 'End here'),
        h('button.sm', { id: id('del'), onclick: () => del() }, 'Delete frame'), undoBtn),
      grp('Motion', rate, h('label', rev, 'Reverse'), h('label', pp, 'Ping-pong'), plays, smoothRow)),
    smoothCap);
  const prev = step(5, 'Preview', 'preview', panel, stats, tl);

  // --- 6 Save: the fields Add GIFs asks for; the name is built from them.
  const splat = h('select', { id: id('plat') });
  const title = h('input', { type: 'text', id: id('title'), placeholder: 'What it shows' });
  const author = h('input', { type: 'text', id: id('author'), placeholder: 'optional' });
  const logo = h('input', { type: 'checkbox', id: id('logo') });
  const cat = h('select', { id: id('cat') });
  const mode = seg('mode', [['replace', 'Replace the original'], ['new', 'Save as new']], (v) => { S.mode = v; mode.set(v); naming(); });
  const name = h('div.file.muted', { id: id('name') });
  const warn = h('div.cap.bad', { id: id('warn') });
  const saveBtn = h('button.pri', { id: id('save'), onclick: () => save() }, 'Save');
  const sv = step(6, 'Save', 'save-step',
    h('div.row', { id: id('mode-row'), hidden: true }, mode),
    h('div.form.f2',
      h('label.stack', 'Title', title), h('label.stack', 'Platform', splat),
      h('label.stack', { id: id('cat-l') }, 'Category', cat), h('label.stack', 'Author', author)),
    h('div.row.st-logo', h('label', logo, 'It is a logo')),
    name, warn, h('div.row.st-go', saveBtn));

  const box = h('div.st-box', h('h2', 'Create Art'),
    h('p.muted', 'Find a picture — a title screen, box art, a logo — or an animated GIF, and make it into a GIF for the panel'),
    h('div.st-grid', imp, h('div.st-col', frm), h('div.st-col', look, txt), prev, sv));
  const el = h('div.st-root', { id: 'studio' }, box);
  root.append(el);

  useSource(store.get('art.src', 'gameart'));
  fold(false, false);
  splat.append(h('option', { value: '' }, 'None'), ...art.platforms().sort().map((p) => h('option', { value: p }, art.consoleName(p) ? `${p} · ${art.consoleName(p)}` : p)));
  let pv = null;
  const A = { on: false }; // the animation's timeline (see `animate`)
  // Smooth motion: `on` the switch; `s` the Smoother over `bytes` (a draft of
  // what Save writes), `gif` its frames for the preview; `gen` ends a draft.
  const SM = { on: false, gen: 0, s: null, bytes: null, gif: null };

  // --- Loading a picture or an animation
  async function load(blob, from = {}) {
    const tok = ++S.tok;
    msg.textContent = 'Reading…';
    seen.hidden = true;
    const head = new Uint8Array(await blob.slice(0, 4).arrayBuffer());
    if (String.fromCharCode(...head) === 'GIF8') {
      const bytes = new Uint8Array(await blob.arrayBuffer());
      if (art.gifFrames(bytes) > 1) return loadGif(bytes, from, tok);
    }
    let bm;
    try { bm = await createImageBitmap(blob, { colorSpaceConversion: 'none', premultiplyAlpha: 'none' }); } catch {
      msg.textContent = 'That is not a picture this browser can read';
      return false;
    }
    if (tok !== S.tok) return false;
    const { width: ow, height: oh } = bm;
    let [w, hh] = [ow, oh];
    const k = Math.min(1, Math.sqrt(MAX_PX / (w * hh)));
    w = Math.max(1, Math.round(w * k)); hh = Math.max(1, Math.round(hh * k));
    const c = h('canvas', { width: w, height: hh });
    const g = c.getContext('2d', { willReadFrequently: true });
    g.imageSmoothingQuality = 'high';
    g.drawImage(bm, 0, 0, w, hh);
    bm.close?.();
    const data = g.getImageData(0, 0, w, hh).data;
    adopt(new art.Studio(data, w, hh), from);
    msg.textContent = `${from.label ? from.label + ':' : 'Picture'} ${ow} × ${oh}`;
    restage(true);
    return true;
  }

  // An animated GIF: decoded in slices, then the timeline.
  // (Or a live effect from the Library's Effects tab, `from.fx` its path:
  // one loop of it drawn by the art module's copy of the core's effects.)
  async function loadGif(bytes, from, tok) {
    const t0 = performance.now();
    let st;
    try { st = from.fx ? art.Studio.fromEffect(from.fx) : art.Studio.fromGif(bytes); } catch (e) {
      msg.textContent = `That GIF could not be read (${e.message || e})`;
      return false;
    }
    const all = st.inFile;
    const ok = await run(() => st.decode(2), () => tok === S.tok, () => { msg.textContent = `${from.fx ? 'Drawing' : 'Reading'} frames… ${st.sourceFrames} of ${all}`; });
    if (!ok) { st.free(); return false; }
    const [ow, oh, ww, wh, reduce, damaged, capped, , cut] = st.info();
    adopt(st, from);
    const n = st.sourceFrames;
    msg.textContent = `${from.label || 'GIF'}: ${ow} × ${oh} · ${n} frames`
      + (reduce > 1 ? ` · working at ${ww} × ${wh} (1/${reduce} size) to fit in memory` : '')
      + (capped ? ` · only the first ${n.toLocaleString()} frames are kept` : '')
      + (cut ? ` · the first ${secs(st.delays().reduce((a, b) => a + b, 0))} s of it` : from.fx ? ' · one loop' : '')
      + (damaged ? ' · the rest of the file is damaged' : '');
    el.dataset.tImport = Math.round(performance.now() - t0);
    restage(true);
    return true;
  }

  // A new picture in the Studio: its fields, the timeline for an animation.
  function adopt(st, from) {
    stop();
    S.studio?.free();
    S.studio = st;
    S.fit = st.autoFit();
    lift.checked = S.fit === 'fit';
    S.from = from.path ? from : null;
    S.mode = S.from ? 'replace' : 'new';
    mode.set(S.mode);
    mode.parentElement.hidden = !S.from;
    const f = from.fields || {};
    logo.checked = !!from.logo || (f.flags || []).includes('LOGO');
    if (from.platform) setPlatform(from.platform);
    if (from.title != null) title.value = from.title;
    author.value = f.author || ''; // another picture is not the last GIF's author's
    el.classList.add('has');
    seen.hidden = !from.link;
    if (from.link) seen.href = from.link;
    fold(true);
    imp.scrollIntoView?.({ block: 'nearest' });
    const anim = st.animated;
    el.classList.toggle('anim', anim);
    moveRow.hidden = anim;
    rangeRow.hidden = !anim;
    tl.hidden = !anim;
    smoothSet(false);
    if (anim) animate(st);
    else A.on = false;
    Object.assign(el.dataset, { srcFrames: st.sourceFrames, animated: anim ? 1 : '' });
    text(true);
    naming();
  }

  async function fromUrl(u) {
    if (!u) return;
    msg.textContent = 'Loading…';
    let r;
    try { r = await fetch(u); } catch {
      msg.textContent = 'That site doesn’t allow it — save the image and drop it here';
      return;
    }
    if (!r.ok) { msg.textContent = `That address did not give a picture (${r.status})`; return; }
    await load(await r.blob(), { label: 'From the address' });
  }

  // Build the stage for the fit; `auto` also re-picks window and motion.
  function restage(auto) {
    if (!S.studio) return;
    const st = S.studio;
    st.prepare(S.fit, lift.checked);
    if (auto) {
      S.motion = st.animated ? 'still' : st.autoMotion();
      [S.x, S.y] = st.autoWindow(S.motion);
      S.moved = false;
    }
    cv.width = st.width; cv.height = st.height;
    cv.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(st.pixels().buffer), st.width, st.height), 0, 0);
    const k = st.scale;
    fcap.textContent = `${st.width} × ${st.height}` + (S.fit === 'fit' ? ' · fitted to the panel' : k > 1 ? ` · the picture was enlarged ${k}×; using its real pixels` : '')
      + (st.animated ? ' · the first frame' : '');
    fit.set(S.fit);
    frame();
    render();
  }

  // Draw the window (and, when it moves, where it travels).
  function frame() {
    const st = S.studio;
    if (!st) return;
    const W = st.width, H = st.height;
    S.x = Math.max(0, Math.min(S.x, W - 128)); S.y = Math.max(0, Math.min(S.y, H - 32));
    const pct = (v, of) => (100 * v) / of + '%';
    const box = (e, x, y, w, hh) => Object.assign(e.style, { left: pct(x, W), top: pct(y, H), width: pct(w, W), height: pct(hh, H) });
    // The shared scale rule (a whole number of device pixels per pixel), else the width.
    const room = frm.clientWidth - 32 || 600, z = fitScale(room, W);
    view.style.width = (z ? (W * z) / devicePixelRatio : room) + 'px';
    box(win, S.motion === 'pan' ? 0 : S.x, S.motion === 'scroll' ? 0 : S.y, 128, 32);
    band.hidden = S.motion === 'still';
    if (S.motion === 'pan') box(band, 0, S.y, W, 32);
    else if (S.motion === 'scroll') box(band, S.x, 0, 128, H);
    motion.set(S.motion);
    Object.assign(el.dataset, { x: S.x, y: S.y, fit: S.fit, motion: S.motion, stage: `${W}x${H}`, scale: st.scale });
  }

  function render() {
    if (!S.studio) return;
    const st = S.studio;
    if (st.animated) {
      const t0 = performance.now();
      st.look(S.x, S.y, key.checked, +gain.value, +colours.value);
      refresh();
      el.dataset.tLook = (performance.now() - t0).toFixed(1);
      return;
    }
    S.gif = st.render(S.motion, S.x, S.y, +speed.value, +hold.value, key.checked, +gain.value, +colours.value);
    stats.textContent = `${st.frames} frame${st.frames > 1 ? 's' : ''} · ${secs(st.durationMs)} s · ${st.colours} colour${st.colours > 1 ? 's' : ''}`;
    el.dataset.frames = st.frames;
    pv?.show('studio.gif', async () => S.gif);
  }
  const later2 = debounce(render, 120);

  // The text, to the module (every frame of a still; a range of an animation's).
  function text(quiet) {
    const st = S.studio;
    if (!st) return;
    const some = st.animated && trange.value === 'some';
    const n = st.sourceFrames;
    tspanShow(some);
    const c = parseInt(tcol.value.slice(1), 16) || 0;
    st.setText(tmsg.value, S.font === 'small', c, S.anchor, +dx.value || 0, +dy.value || 0,
      some ? Math.max(0, (+tfrom.value || 1) - 1) : 0, some ? Math.min(n, +tto.value || n) - 1 : 0x7fffffff);
    el.dataset.text = tmsg.value.trim() ? 1 : '';
    if (!quiet) st.animated ? refresh() : render();
  }
  const tspanShow = (v) => { tfrom.parentElement.hidden = !v; tfrom.max = tto.max = S.studio?.sourceFrames || 1; };
  const retext = debounce(() => text(), 150);
  for (const c of [tmsg, dx, dy, tfrom, tto]) c.addEventListener('input', retext);
  for (const c of [tcol, trange]) c.addEventListener('change', () => text());

  // Dragging the window; a tap elsewhere on the picture moves it there.
  let drag = null;
  const toStage = (e) => { const r = cv.getBoundingClientRect(); return [(e.clientX - r.left) * (S.studio.width / r.width), (e.clientY - r.top) * (S.studio.height / r.height)]; };
  win.addEventListener('pointerdown', (e) => { e.preventDefault(); e.stopPropagation(); win.setPointerCapture(e.pointerId); drag = { p: toStage(e), x: S.x, y: S.y }; });
  win.addEventListener('pointermove', (e) => {
    if (!drag) return;
    const [sx, sy] = toStage(e);
    S.x = Math.round(drag.x + sx - drag.p[0]); S.y = Math.round(drag.y + sy - drag.p[1]);
    S.moved = true;
    frame();
  });
  win.addEventListener('pointerup', () => { if (drag) { drag = null; render(); } });
  view.addEventListener('click', (e) => {
    if (!S.studio || e.target === win) return;
    const [sx, sy] = toStage(e);
    S.x = Math.round(sx - 64); S.y = Math.round(sy - 16);
    S.moved = true;
    frame(); render();
  });

  for (const c of [speed, hold, colours, key]) c.onchange = render;
  lift.onchange = () => restage(false);
  gain.oninput = () => { gout.textContent = gain.value + '%'; later2(); };

  // --- The timeline ------------------------------------------------------------
  // Source frames are numbered from 1 in the UI. `items` are the frames not
  // deleted, in source order (the strip); `seq`/`ms` the play order and
  // delays from the module; `pos` the place in it; `peek` a frame outside
  // the trim that was clicked. Frames are rendered when shown.
  function animate(st) {
    const n = st.sourceFrames;
    Object.assign(A, { on: true, n, from: 0, to: n - 1, deleted: new Set(), sel: new Set(), hist: [], pos: 0, peek: null, playing: true, due: 0, done: 0, shown: -1, gen: 0, drawn: new Map() });
    rate.value = 100; rev.checked = pp.checked = false;
    const p = st.info()[7];
    plays.value = PLAYS.some(([, v]) => v === p) ? p : 0;
    tfrom.value = 1; tto.value = n; trange.value = 'all';
    track.style.width = n * CW + 'px';
    strip.scrollLeft = 0;
    edit();
    start();
  }

  function edit() {
    const flat = S.studio.setEdit(A.from, A.to, +rate.value, rev.checked, pp.checked, Uint32Array.from(A.deleted));
    A.seq = []; A.ms = [];
    for (let i = 0; i < flat.length; i += 2) { A.seq.push(flat[i]); A.ms.push(flat[i + 1]); }
    A.items = [];
    for (let i = 0; i < A.n; i++) if (!A.deleted.has(i)) A.items.push(i);
    track.style.width = A.items.length * CW + 'px';
    A.pos = Math.min(A.pos, Math.max(0, A.seq.length - 1));
    A.due = 0;
    scrub.max = Math.max(0, A.seq.length - 1);
    const total = A.ms.reduce((a, b) => a + b, 0);
    const p = +plays.value;
    stats.textContent = `${A.seq.length} frame${A.seq.length === 1 ? '' : 's'} · ${secs(total)} s · ${p ? (p === 1 ? 'plays once' : `plays ${p} times`) : 'plays forever'}`;
    const cut = A.n - A.items.length;
    tlcap.textContent = `Frames ${A.from + 1}–${A.to + 1} of ${A.n}` + (cut ? ` · ${cut} deleted` : '');
    undoBtn.disabled = !A.hist.length;
    Object.assign(el.dataset, { frames: A.seq.length, duration: total, from: A.from, to: A.to, deleted: A.deleted.size, plays: p });
    A.shown = -1;
    smoothRow.hidden = !app.device || A.seq.length < 2;
    smoothStale();
    drawStrip();
  }

  // Everything the preview shows is made again (a Look or Text change).
  function refresh() {
    if (!A.on) return;
    A.gen++; A.shown = -1;
    smoothStale();
    show();
    drawStrip();
  }

  let raf = 0;
  function start() { if (!raf && A.on) raf = requestAnimationFrame(tick); }
  function stop() { cancelAnimationFrame(raf); raf = 0; }
  function tick(now) {
    raf = 0;
    if (!A.on || !el.isConnected) return;
    if (A.playing && A.seq.length && el.offsetParent) {
      if (!A.due) A.due = now + A.ms[A.pos];
      while (now >= A.due) {
        if (++A.pos >= A.seq.length) {
          A.pos = 0;
          if (+plays.value && ++A.done >= +plays.value) { A.pos = A.seq.length - 1; A.playing = false; playBtn.textContent = '▶'; break; }
        }
        A.due += A.ms[A.pos];
        if (now - A.due > 500) A.due = now + A.ms[A.pos];
      }
    }
    show();
    raf = requestAnimationFrame(tick);
  }

  // The frame at `pos` (or the one peeked at) into the preview.
  function show() {
    if (!A.on || !pv || !A.seq.length) return;
    // Playing with Smooth motion on: the smoothed frames, at their delays.
    const src = SM.gif && A.playing && A.peek == null ? 'smooth' : A.peek ?? A.seq[A.pos];
    scrub.value = A.pos;
    posOut.textContent = `${A.pos + 1} / ${A.seq.length}`;
    if (src === A.shown) return;
    if (src === 'smooth') {
      A.shown = src;
      pv.p.load('studio-smooth.gif', SM.gif);
      el.dataset.shown = src;
      return;
    }
    A.shown = src;
    pv.p.load('studio.gif', S.studio.frameGif(src));
    Object.assign(el.dataset, { pos: A.pos, shown: src });
    for (const c of track.children) c.classList?.toggle('cur', +c.dataset.i === src);
  }

  function playPause() {
    A.playing = !A.playing;
    A.peek = null;
    if (A.playing && +plays.value && A.done >= +plays.value) { A.done = 0; A.pos = 0; }
    A.due = 0;
    playBtn.textContent = A.playing ? '❚❚' : '▶';
  }
  function pause() { A.playing = false; playBtn.textContent = '▶'; }
  scrub.oninput = () => { pause(); A.peek = null; A.pos = +scrub.value; show(); };
  for (const c of [rate, rev, pp]) c.onchange = () => edit();
  plays.onchange = () => { A.done = 0; edit(); };

  // Undo: the frames kept and the trim, as they were.
  const snap = () => { A.hist.push({ deleted: [...A.deleted], from: A.from, to: A.to }); if (A.hist.length > 100) A.hist.shift(); };
  function undo() {
    const s = A.hist.pop();
    if (!s) return;
    A.deleted = new Set(s.deleted); A.from = s.from; A.to = s.to; A.sel.clear();
    edit();
  }
  const current = () => A.peek ?? A.seq[A.pos] ?? A.from;
  function trim(end) {
    snap();
    const c = current();
    A[end] = c;
    if (A.from > A.to) end === 'from' ? (A.to = A.from) : (A.from = A.to);
    edit();
  }
  function del() {
    const pick = A.sel.size ? [...A.sel] : [current()];
    const left = A.items.filter((i) => i >= A.from && i <= A.to && !pick.includes(i));
    if (!left.length) { toast('Keep at least one frame', 'bad'); return; }
    snap();
    for (const i of pick) A.deleted.add(i);
    A.sel.clear(); A.peek = null;
    edit();
  }

  // The strip: only the cells in view exist; each thumbnail is rendered
  // when it first shows (in slices).
  const pool = [];
  let queue = [], painting = false;
  function drawStrip() {
    if (!A.on) return;
    const w = strip.clientWidth || 600;
    const a = Math.max(0, Math.floor(strip.scrollLeft / CW) - 1), b = Math.min(A.items.length, Math.ceil((strip.scrollLeft + w) / CW) + 1);
    while (pool.length < b - a) {
      const c = h('div.st-cell', h('canvas', { width: 128, height: 32 }), h('span'));
      c.onclick = (e) => pick(+c.dataset.i, e);
      pool.push(c); track.append(c);
    }
    const src = A.peek ?? A.seq[A.pos];
    queue = [];
    pool.forEach((c, j) => {
      const k = a + j;
      c.hidden = k >= b;
      if (c.hidden) return;
      const i = A.items[k];
      c.dataset.i = i;
      c.style.left = k * CW + 'px';
      c.lastChild.textContent = i + 1;
      c.classList.toggle('cur', i === src);
      c.classList.toggle('sel', A.sel.has(i));
      c.classList.toggle('out', i < A.from || i > A.to);
      if (A.drawn.get(c) !== `${i}@${A.gen}`) queue.push(c);
    });
    const at = (i, right) => { let k = A.items.findIndex((x) => (right ? x > i : x >= i)); if (k < 0) k = A.items.length; return k * CW; };
    hin.style.left = at(A.from) + 'px';
    hout.style.left = at(A.to, true) - 4 + 'px';
    paint();
  }
  async function paint() {
    if (painting) return;
    painting = true;
    await run(() => {
      const c = queue.shift();
      if (!c) return true;
      const i = +c.dataset.i;
      const px = S.studio.thumb(i);
      c.firstChild.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(px.buffer), 128, 32), 0, 0);
      A.drawn.set(c, `${i}@${A.gen}`);
      return !queue.length;
    });
    painting = false;
    if (queue.length) paint();
  }
  strip.addEventListener('scroll', () => drawStrip(), { passive: true });
  function pick(i, e) {
    if (e.shiftKey && A.last != null) {
      const [a, b] = [Math.min(A.last, i), Math.max(A.last, i)];
      for (const x of A.items) if (x >= a && x <= b) A.sel.add(x);
    } else if (e.ctrlKey || e.metaKey) A.sel.has(i) ? A.sel.delete(i) : A.sel.add(i);
    else { A.sel.clear(); A.sel.add(i); }
    A.last = i;
    pause();
    const k = A.seq.indexOf(i);
    A.peek = k < 0 ? i : null;
    if (k >= 0) A.pos = k;
    show();
    drawStrip();
    strip.focus({ preventScroll: true });
  }
  el.addEventListener('keydown', (e) => {
    if (!A.on || /INPUT|SELECT|TEXTAREA/.test(e.target.tagName)) return;
    if (e.key === 'Delete' || e.key === 'Backspace') { e.preventDefault(); del(); }
    else if ((e.ctrlKey || e.metaKey) && e.key === 'z') { e.preventDefault(); undo(); }
    else if (e.key === ' ' && e.target === strip) { e.preventDefault(); playPause(); }
  });
  // Trim handles: drag along the strip.
  for (const [hd, end] of [[hin, 'from'], [hout, 'to']]) {
    hd.addEventListener('pointerdown', (e) => { e.preventDefault(); e.stopPropagation(); hd.setPointerCapture(e.pointerId); snap(); hd.drag = true; });
    hd.addEventListener('pointermove', (e) => {
      if (!hd.drag) return;
      const r = track.getBoundingClientRect();
      const k = Math.max(0, Math.min(A.items.length - 1, Math.floor((e.clientX - r.left) / CW)));
      const i = A.items[k];
      if (end === 'from' ? i > A.to : i < A.from) return;
      if (A[end] !== i) { A[end] = i; edit(); }
    });
    hd.addEventListener('pointerup', () => { hd.drag = false; });
  }

  // --- Smooth motion ------------------------------------------------------------
  let smod = null;
  const smooth = async () => (smod ??= await import('./smooth.js'));
  const smoothSay = (state, t) => { el.dataset.smooth = state; smoothCap.textContent = t; };
  const smoothWanted = () => SM.on && A.on && !smoothRow.hidden;
  // The draft no longer matches (or is not wanted): drop it, stop one running.
  function smoothDrop() {
    SM.gen++;
    SM.s?.free();
    SM.s = SM.bytes = SM.gif = null;
    if (A.shown === 'smooth') A.shown = -1;
  }
  function smoothSet(on) {
    smoothOn.checked = SM.on = on;
    smoothDrop();
    smoothSay(on ? 'analysing' : 'off', on ? 'Looking at the motion…' : '');
  }
  // An edit changed the output: look again, once the edits settle.
  function smoothStale() {
    if (!SM.s && !SM.on) return;
    smoothDrop();
    if (smoothWanted()) { smoothSay('analysing', 'Looking at the motion…'); smoothLater(); }
  }
  // A draft of what Save would write (the same path, in slices), then the
  // analysis and the in-between frames over it.
  async function smoothLook() {
    const st = S.studio;
    if (!smoothWanted() || S.saving || SM.s) return;
    const g = ++SM.gen;
    const alive = () => g === SM.gen && S.studio === st;
    smoothSay('analysing', 'Looking at the motion…');
    try {
      st.saveBegin(+plays.value);
      if (!(await run(() => st.saveStep(8), alive))) return;
      const bytes = st.saved();
      const s = await (await smooth()).smoother(bytes, { tween: true, alive });
      if (!s) return;
      if (!alive()) { s.free(); return; }
      el.dataset.smoothAdded = s.added;
      if (!s.added) {
        // Nothing to smooth (as it is now): the switch stays on, says so, and
        // looks again after the next edit (a speed of 2x can be too fast to
        // smooth, 1x not).
        s.free();
        smoothSay('none', 'Nothing here moves in steps it can smooth');
        return;
      }
      Object.assign(SM, { s, bytes, gif: s.gif() });
      const n = (x) => x.toLocaleString();
      smoothSay('on', `Adds ${n(s.added)} frame${s.added === 1 ? '' : 's'} across ${n(s.tweened)} move${s.tweened === 1 ? '' : 's'}`);
      A.shown = -1;
      show();
    } catch (e) { if (alive()) smoothSay('error', `Smooth motion: ${e.message || e}`); }
  }
  const smoothLater = debounce(smoothLook, 400);
  smoothOn.onchange = () => { smoothSet(smoothOn.checked); smoothLook(); show(); };

  // --- Save
  function setPlatform(p) {
    if (![...splat.options].some((o) => o.value === p)) splat.append(h('option', { value: p }, p));
    splat.value = p;
  }

  // The fields as a name (Add GIFs' rules: the next free number for this
  // platform and title in the category; replacing keeps the original's),
  // and what the Library will list it as.
  async function naming() {
    const t = (naming.t = (naming.t || 0) + 1);
    const c = cat.value || 'Consoles';
    const flags = (S.from?.fields?.flags || []).filter((x) => x !== 'LOGO' && x !== S.from.fields.author);
    const f = { title: title.value.trim(), platform: splat.value || null, category: c, author: author.value.trim() || null, flags: logo.checked ? [...flags, 'LOGO'] : flags, ordinal: null };
    author.placeholder = f.platform ? 'optional' : 'optional · needs a platform';
    S.listing ??= listings(S.cats);
    const l = await S.listing(c);
    if (t !== naming.t) return;
    const keep = S.mode === 'replace' && S.from ? { ...S.from.fields, path: S.from.path } : null;
    const { path, why } = pickName(art, f, l, keep);
    S.path = path;
    saveBtn.disabled = !path || !S.studio;
    if (!path) {
      clear(name);
      warn.textContent = S.studio ? why : ''; // (no picture: step 1 says what to do)
      Object.assign(el.dataset, { name: '', title: '', platform: '' });
      return;
    }
    const e = app.wasm.parsePath(path);
    const n = base(path);
    clear(name, h('span', S.from && S.mode === 'replace' ? (path === S.from.path ? 'Replaces' : 'Replaces and renames to') : 'File'), h('span', h('span.mono', n), app.device ? ` in ${niceName(c)}` : '', ` · listed as “${label(e)}”`));
    warn.textContent = '';
    Object.assign(el.dataset, { name: n, title: e.title, platform: e.platform || '' });
  }
  const renaming = debounce(naming, 200);
  for (const c of [title, author]) c.addEventListener('input', renaming);
  for (const c of [splat, logo, cat]) c.addEventListener('change', naming);

  async function save() {
    const st = S.studio;
    if (!st || !S.path) return;
    saveBtn.disabled = true;
    // Smooth motion as the switch says (an animation of 2 frames or more, on
    // a device); a draft being made stops, a finished one is reused.
    const smoothOnSave = smoothWanted();
    const ready = SM.s;
    SM.gen++;
    S.saving = true;
    try {
      if (st.animated) {
        const t0 = performance.now();
        st.saveBegin(+plays.value);
        await run(() => st.saveStep(8), null, () => { const [d, of] = st.saveProgress(); clear(name, `Saving… ${Math.round((100 * d) / of)}%`); });
        S.gif = st.saved();
        el.dataset.tSave = Math.round(performance.now() - t0);
        el.dataset.colours = st.colours;
      }
      if (!S.gif) return;
      const path = S.path, f = base(path), c = path.split('/')[2];
      if (!app.device) {
        const a = h('a', { href: URL.createObjectURL(new Blob([S.gif], { type: 'image/gif' })), download: f });
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 5000);
        toast(`downloaded ${f}`, 'ok');
        return;
      }
      const t1 = performance.now();
      const problems = [];
      // The copy: Smooth motion's (and the list) when it is on, or when the
      // original was listed and is now off; else the plain one.
      const same = ready && SM.bytes?.length === S.gif.length && SM.bytes.every((b, i) => b === S.gif[i]);
      const keep = async (p, b) => { const w = await (await smooth()).keep(p, b, smoothOnSave, same ? ready : null); if (w) problems.push(w); };
      if (S.from && S.mode === 'replace') {
        // The same file, so every playlist keeps it; a changed name is a
        // rename, which playlists follow too (and so does Smooth motion's list).
        const orig = S.from.path;
        await app.src.writeFile(orig, S.gif);
        try { await (smoothOnSave || S.from.listed ? keep : prepareBytes)(orig, S.gif); } catch { await app.src.forget(orig).catch(() => {}); }
        if (path.toLowerCase() !== orig.toLowerCase()) await renameGif(orig, path);
        S.from = { ...S.from, path, fields: art.nameFields(path), listed: smoothOnSave };
        toast(`${f} saved`, 'ok');
        for (const m of problems) toast(m, 'bad');
        emit('gifs', c);
      } else {
        await S.opts.save(c, f, S.gif, (m) => problems.push(m), smoothOnSave ? keep : undefined);
        toast(`${f} saved to ${c}`, 'ok');
        for (const m of problems) toast(m, 'bad');
        await S.opts.saved(c, [path]);
      }
      el.dataset.tUpload = Math.round(performance.now() - t1);
      el.dataset.saved = path;
      S.listing = null;
    } catch (e) { toast(`saving: ${e.message || e}`, 'bad'); } finally {
      S.saving = false;
      if (smoothWanted() && !SM.s) smoothLook();
      await naming();
    }
  }

  // --- Drop and paste, while open
  const onPaste = (e) => {
    const f = [...(e.clipboardData?.files || [])].find((x) => x.type.startsWith('image/'));
    if (f) { e.preventDefault(); load(f, { label: 'Pasted' }); return; }
    const t = e.clipboardData?.getData('text/plain')?.trim();
    if (t && /^https?:\/\//.test(t) && e.target === document.body) { url.value = t; fromUrl(t); }
  };
  el.addEventListener('dragover', (e) => { e.preventDefault(); el.classList.add('drop'); });
  el.addEventListener('dragleave', (e) => { if (e.target === el) el.classList.remove('drop'); });
  el.addEventListener('drop', (e) => {
    e.preventDefault();
    el.classList.remove('drop');
    const f = [...e.dataTransfer.files].find((x) => x.type.startsWith('image/'));
    if (f) load(f, { label: 'Dropped' });
    else { const u = e.dataTransfer.getData('text/uri-list').split('\n')[0].trim(); if (u) { url.value = u; fromUrl(u); } }
  });

  // Where a new one saves: a category on the card (Add GIFs' path: the GIF,
  // its prepared copy), then the offer to add it to the playlist in play.
  S.opts = { save: addOne, saved: (_c, paths) => offerPlaylist(paths) };

  return {
    el,
    load,
    async show() {
      addEventListener('paste', onPaste);
      let cats = [];
      if (app.device) try { cats = (await app.src.list('/gifs')).filter((d) => d.dir).map((d) => d.name).sort(byNice); } catch { /* none yet */ }
      if (!cats.length) cats = ['Consoles'];
      S.cats = cats;
      S.listing = null;
      const was = cat.value;
      clear(cat, cats.map((c) => h('option', { value: c }, niceName(c))));
      cat.value = cats.includes(was) ? was : cats.includes('Consoles') ? 'Consoles' : cats[0];
      cat.parentElement.hidden = !app.device;
      saveBtn.textContent = app.device ? 'Save to the card' : 'Download GIF';
      if (!pv) {
        pv = new PanelPreview(panel, { onError: (m) => { stats.textContent = 'preview: ' + m; } });
        pv.p.setTransition('none', 0);
      }
      if (S.studio) frame();
      if (A.on) { A.shown = -1; start(); drawStrip(); }
      naming();
      // "Edit in Create Art" in the Library: that GIF — every frame — with its
      // title, platform, category and author, to replace or save as new.
      const from = app.artFrom;
      app.artFrom = null;
      // The Library's "Search the web": its words, in a web source.
      const find = app.artFind;
      app.artFind = null;
      if (find) {
        // The Library's one platform -> its game art; anything else, GifCities.
        const p = PLATFORMS.includes(find.platform) && find.platform;
        if (p) R.set('gameart', 'platform', p);
        q.value = '';
        useSource(p ? 'gameart' : 'gifcities');
        q.value = find.q;
        S.searched = null;
        fold(false);
      }
      if (from?.fx) {
        // "Save as GIF…" in the Effects tab: a new GIF, named as the demo
        // card's recordings are (platform FX, category Demoscene).
        if (app.device && !cats.includes('Demoscene')) cat.append(h('option', { value: 'Demoscene' }, 'Demoscene'));
        cat.value = 'Demoscene';
        author.value = '';
        const tok = ++S.tok;
        msg.textContent = 'Drawing…';
        await loadGif(null, { fx: from.fx, title: from.title, platform: 'FX', label: from.title }, tok);
      } else if (from) {
        const e = from.e, fields = art.nameFields(from.path);
        if (cats.includes(e.category)) cat.value = e.category;
        setPlatform(fields.platform || '');
        title.value = fields.title || '';
        author.value = fields.author || '';
        if (PLATFORMS.includes(e.platform)) R.set('gameart', 'platform', e.platform);
        q.value = e.title || '';
        try {
          // On Smooth motion's list: the switch starts on, and looks once loaded.
          const listed = app.device && (await (await smooth()).readList().catch(() => [])).some((p) => p.toLowerCase() === from.path.toLowerCase());
          if (await load(new Blob([await app.src.readBytes(from.path)], { type: 'image/gif' }), { path: from.path, fields, label: label(e), listed }) && listed && A.on) {
            smoothSet(true);
            smoothLook();
          }
        } catch (err) { msg.textContent = `Could not load ${label(e)}: ${err.message}`; }
      }
    },
    hide() { removeEventListener('paste', onPaste); stop(); },
  };
}
