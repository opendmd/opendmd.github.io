// Smooth motion: in-between frames for a GIF whose motion moves in steps of
// 2 px or more (a marquee, a scroll, a sprite crossing the screen) — the
// shift tweener of the core's `tween` (docs/tweening-experiment.md), run
// here in the art module (js/artmod.js) and never on the device. Opt-in per
// GIF: the GIF stays as it is; the file the panel plays in its place is made
// from the tweened frames (byte-identical to opendmd-pack's), and the choice
// is one line in the card's /sets/.smooth, outside the cache, so clearing
// the cache keeps it. Loaded on first use (a Library preview on a device, a
// prepare run, Create Art's switch or a listed GIF opened there), so it costs
// the page load nothing.
//
// The work runs in slices of ~12 ms with the page given back between them
// (no Worker: one wasm instance, the one the prepare job already loaded).

import { app } from './state.js';
import { art } from './artmod.js';
import { dfetch } from './source.js';
import { h, clear, enc, toast } from './util.js';

export const LIST = '/sets/.smooth';
const lc = (s) => s.toLowerCase();
const later = () => new Promise((r) => setTimeout(r, 0));

// Call `step` (a few frames of wasm work; true when done) until it is done,
// yielding every ~12 ms. False if `alive()` said stop.
async function run(step, alive) {
  for (;;) {
    const t = performance.now();
    do { if (step()) return true; } while (performance.now() - t < 12);
    await later();
    if (alive && !alive()) return false;
  }
}

// A Smoother over `bytes`, analysed (and tweened if asked); null if stopped.
export async function smoother(bytes, { tween = false, alive } = {}) {
  const s = new (await art()).Smoother(bytes);
  try {
    if ((await run(() => s.analyse(1), alive)) && (!tween || (await run(() => s.tween(2), alive)))) return s;
  } catch (e) { s.free(); throw e; }
  s.free();
  return null;
}

// The smoothed file the panel plays for this GIF (its bytes).
export async function smoothCopy(bytes) {
  const s = await smoother(bytes, { tween: true });
  try { return s.prepare(); } finally { s.free(); }
}

export async function readList() {
  return ((await app.src.readText(LIST)) || '').split('\n').map((l) => l.trim()).filter(Boolean);
}
const writeList = (list) => app.src.writeFile(LIST, list.map((p) => p + '\n').join(''));

// Is the file the panel plays for `path` the smooth one? null when there is
// none. Only its first KB is read: the stream is cancelled once the header
// is in (the device ignores Range; asking costs nothing).
async function isSmooth(path) {
  const m = await art();
  const res = await dfetch(enc(app.wasm.cachePathFor(path)), { headers: { Range: 'bytes=0-1023' } });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  const r = res.body.getReader();
  let head = new Uint8Array();
  try {
    while (head.length < 1024) {
      const { value, done } = await r.read();
      if (done) break;
      const n = new Uint8Array(head.length + value.length);
      n.set(head);
      n.set(value, head.length);
      head = n;
    }
  } finally { r.cancel().catch(() => {}); }
  return !!m.smoothMarker(head);
}

// For the prepare job, over the playlist in play (`paths`, of which
// `missing` have no file yet): the listed GIFs (lowercased), and those in
// the playlist whose file is there but not the smooth one (the device's own
// builder makes plain ones after the cache is cleared) — to be made again.
// One small GET per listed GIF in the playlist.
export async function pending(paths, missing) {
  const listed = new Set((await readList().catch(() => [])).map(lc));
  const miss = new Set(missing.map(lc));
  const redo = [];
  for (const p of paths) {
    if (!listed.has(lc(p)) || miss.has(lc(p))) continue;
    try { if ((await isSmooth(p)) !== true) redo.push(p); } catch { redo.push(p); }
  }
  return { listed, redo };
}

// Turn it on or off for one GIF. On: the list first, then the smooth file
// (if that upload fails, the next prepare run makes it). Off: the plain file
// first — made again, not deleted — then the list.
export async function set(path, on) {
  const gif = await app.src.readBytes(path);
  const rest = (await readList()).filter((p) => lc(p) !== lc(path));
  const dest = app.wasm.cachePathFor(path);
  if (on) {
    await writeList([...rest, path]);
    await app.src.writeFile(dest, await smoothCopy(gif));
  } else {
    await app.src.writeFile(dest, (await art()).prepareGif(gif).copy);
    await writeList(rest);
  }
}

// Create Art saved `bytes` at `path` (js/studio.js): list it or not, and
// write the file the panel plays to match, so the copy is right at once.
// `ready` is a Smoother already tweened over these very bytes (optional).
// On: the list first, then the smooth file (plain, with a warning returned,
// if the tweener refuses it). Off: the plain file, then the list.
export async function keep(path, bytes, on, ready) {
  const list = await readList().catch(() => []);
  const rest = list.filter((p) => lc(p) !== lc(path));
  const dest = app.wasm.cachePathFor(path);
  const m = await art();
  if (!on) {
    await app.src.writeFile(dest, m.prepareGif(bytes).copy);
    if (rest.length < list.length) await writeList(rest);
    return null;
  }
  if (rest.length === list.length) await writeList([...rest, path]);
  let copy = null, warn = null;
  try { copy = ready?.prepare(); } catch { /* made again below */ }
  try { copy ||= await smoothCopy(bytes); } catch (e) {
    warn = `Smooth motion could not be made for this one (${e.message || e}); it plays as it is`;
    copy = m.prepareGif(bytes).copy;
  }
  await app.src.writeFile(dest, copy);
  return warn;
}

// --- Library: the control under the preview --------------------------------
// Shown only when the analysis finds motion to smooth in at least 10% of the
// GIF's frame pairs (or it is on already); otherwise nothing is shown.
let token = 0;
let held = null; // the Smoother behind what is shown (freed on the next)

export async function detail(box, it, bytes, pv, cap) {
  const t = ++token;
  const alive = () => t === token;
  held?.free();
  held = null;
  box.hidden = true;
  box.dataset.state = 'analysing';
  clear(box);
  let s, on;
  const t0 = performance.now();
  try {
    on = (await readList().catch(() => [])).some((p) => lc(p) === lc(it.path));
    // What the preview fetched is the panel's file; a smooth one already
    // has its in-betweens, so the GIF itself is analysed instead.
    if (on || (await art()).smoothMarker(bytes.subarray(0, 1024))) bytes = await app.src.readBytes(it.path);
    if (!alive()) return;
    s = await smoother(bytes, { alive });
  } catch { if (alive()) box.dataset.state = 'error'; return; }
  if (!s) return;
  if (!alive()) { s.free(); return; }
  held = s;
  box.dataset.analyseMs = Math.round(performance.now() - t0);
  if (!s.offered && !on) { box.dataset.state = 'none'; return; }

  let showing = false;
  const msg = h('div.cap', { id: 'lib-smooth-msg' });
  const sw = h('input.sw', { type: 'checkbox', role: 'switch', id: 'lib-smooth-on', checked: on, onchange: toggle });
  const pvBtn = h('button.sm.txt', { id: 'lib-smooth-preview', onclick: preview }, 'Preview');
  clear(box,
    h('div.swrow', h('label', sw, 'Smooth motion'), pvBtn),
    h('div.cap', { id: 'lib-smooth-text' },
      `${s.percent}% of this animation moves in steps of 2 px or more. Smooth motion adds ${s.added.toLocaleString()} frames`),
    msg);
  box.dataset.state = on ? 'on' : 'off';
  box.hidden = false;

  // Play the smooth version in the preview above, or go back.
  async function preview() {
    if (showing) {
      showing = false;
      pvBtn.textContent = 'Preview';
      await pv.show(it.path, async () => bytes);
      cap.textContent = '';
      cap.dataset.shown = it.name;
      return;
    }
    pvBtn.disabled = true;
    msg.textContent = 'Making the preview…';
    try {
      if (!(await run(() => s.tween(2), alive))) return;
      const g = s.gif();
      if (!alive()) return;
      await pv.show(it.path, async () => g);
      showing = true;
      pvBtn.textContent = 'Show original';
      cap.textContent = 'Smooth motion preview';
      cap.dataset.shown = `${it.name} · smooth`;
      msg.textContent = '';
    } catch (e) { msg.textContent = 'Preview: ' + (e.message || e); } finally { pvBtn.disabled = false; }
  }

  async function toggle() {
    const want = sw.checked;
    sw.disabled = true;
    msg.textContent = want ? 'Smoothing…' : 'Turning it off…';
    try {
      await set(it.path, want);
      box.dataset.state = want ? 'on' : 'off';
      msg.textContent = want ? 'On: the panel plays it smoothly from the next time it starts' : 'Off: the panel plays the original frames';
    } catch (e) {
      sw.checked = !want;
      msg.textContent = '';
      toast(`Smooth motion: ${e.message || e}`, 'bad');
    } finally { sw.disabled = false; }
  }
}
