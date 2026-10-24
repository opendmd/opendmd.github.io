// Preparing files for playback: a GIF's copy, re-encoded here with the core's
// encoder (js/artmod.js) and uploaded to cachePathFor; the GIF is never touched.
// `start()` runs one file at a time over what the playlist in play is missing,
// emitting `prepare` events; damaged files are recorded and skipped, network
// failures retried; GIFs in /sets/.smooth are made smooth. A GIF past 256
// colours gets no copy (only a lossy one could be made; the panel plays the
// GIF itself, exactly): it is recorded as playing as it is and counts as done.
// CLAUDE.md, "Prepare remaining files", has the whole story.

import { app } from './state.js';
import { emit, store } from './util.js';
import { art } from './artmod.js';

export const job = {
  running: false, stopping: false, phase: 'idle',
  todo: 0, done: 0, current: '', errors: [], t0: 0,
  damaged: [], net: [], tried: 0,
};

const tell = () => emit('prepare', job);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The encoder's word for a GIF past 256 colours (gifslim::ORIGINAL_REASON,
// also what the device records): no copy, it plays as it is.
export const playsAsIs = (e) => /^\s*more than 256 colou?rs/i.test(String(e?.message ?? e ?? ''));

// A GIF that plays as it is: any copy it had goes (an old lossy one, or the
// copy of the file it replaced), and the device is told, so its count has it
// as done and nothing tries it again.
async function asIs(path, reason) {
  await app.src.forget(path).catch(() => {});
  try { await app.src.cacheFail(path, reason); } catch {
    store.set(asIsKey(), { ...store.get(asIsKey(), {}), [path]: reason }); // no POST: remember it here
  }
}

// A copy was made for `path` after all (the GIF was replaced): a "plays as it
// is" remembered here is about the old file. (The device drops its own line
// when the GIF is written.)
export function madeCopy(path) {
  const m = store.get(asIsKey(), {});
  const k = Object.keys(m).find((p) => p.toLowerCase() === path.toLowerCase());
  if (k) { delete m[k]; store.set(asIsKey(), m); }
}

// Encode one GIF's bytes and upload its prepared copy (none for one that
// plays as it is: 0). Throws on failure.
export async function prepareBytes(gifPath, bytes) {
  const dest = app.wasm.cachePathFor(gifPath);
  if (!dest) throw new Error('only GIFs can be prepared');
  try {
    const { copy } = (await art()).prepareGif(bytes);
    await app.src.writeFile(dest, copy);
    madeCopy(gifPath);
    return copy.length;
  } catch (e) {
    if (!playsAsIs(e)) throw e;
    await asIs(gifPath, e.message);
    return 0;
  }
}

// The entries of the playlist in play (under the device's platform filter,
// as its count is), those of them that have no prepared copy yet, and the
// copies there are (lowercased).
export async function missing() {
  const text = await app.text(app.activePath, true);
  if (text == null) throw new Error('the playlist could not be read');
  const paths = app.wasm.filterManifest(text, JSON.stringify({ platforms: app.config?.platforms || '' }));
  const have = new Set((await app.src.cacheList()).map((p) => p.toLowerCase()));
  return { paths, have, missing: paths.filter((p) => { const c = app.wasm.cachePathFor(p); return c && !have.has(c.toLowerCase()); }) };
}

// --- the damaged list -----------------------------------------------------
const localKey = () => 'damaged.' + location.host;
const asIsKey = () => 'asis.' + location.host;
let shared = false; // the device has /api/cache/failed

// Lowercased path -> reason, for every GIF known to be damaged; and the set
// of those (lowercased) that play as they are (done, not damaged).
async function damagedList() {
  const out = new Map(), plays = new Set();
  let rows = null;
  try { rows = await app.src.cacheFailed(); shared = true; } catch (e) {
    shared = false;
    if (e.status !== 404) throw e;
    const text = await app.src.readText('/cache/.failed').catch(() => null);
    rows = (text || '').split('\n').map((l) => { const [path, reason = ''] = l.split('\t'); return { path: path.trim(), reason }; });
  }
  for (const r of rows) {
    if (!r.path) continue;
    if (playsAsIs(r.reason)) plays.add(r.path.toLowerCase());
    // The builder also gives up on what this encoder can do: a file too
    // large (or needing more memory) to prepare there, one that vanished.
    else if (!/too large|not found/.test(r.reason)) out.set(r.path.toLowerCase(), r.reason);
  }
  for (const [p, why] of Object.entries(store.get(localKey(), {}))) out.set(p.toLowerCase(), why);
  for (const p of Object.keys(store.get(asIsKey(), {}))) plays.add(p.toLowerCase());
  return { bad: out, plays };
}

async function recordDamaged(path, reason) {
  if (shared) {
    try { await app.src.cacheFail(path, reason); return; } catch { shared = false; /* no POST: keep it here */ }
  }
  store.set(localKey(), { ...store.get(localKey(), {}), [path]: reason });
}

// --- the run ----------------------------------------------------------------
// One file: 'ok', 'asis' (plays as it is: recorded, done), 'damaged'
// (recorded) or 'net' (after its retry, if any).
async function one(path, retry, smooth) {
  for (let i = 0; ; i++) {
    let bytes;
    try {
      bytes = await app.src.readBytes(path);
      const m = await art();
      let copy;
      // (One the tweener cannot take is prepared plain.)
      try { copy = (smooth && await smooth.smoothCopy(bytes).catch(() => null)) || m.prepareGif(bytes).copy; } catch (e) {
        const reason = String(e.message || e);
        if (playsAsIs(e)) { await asIs(path, reason); return 'asis'; }
        job.damaged.push({ path, reason });
        await recordDamaged(path, reason);
        return 'damaged';
      }
      await app.src.writeFile(app.wasm.cachePathFor(path), copy);
      madeCopy(path);
      return 'ok';
    } catch (e) {
      if (!retry || i || job.stopping) return 'net';
      await sleep(2000);
    }
  }
}

export async function start() {
  if (job.running) return;
  Object.assign(job, { running: true, stopping: false, phase: 'checking', todo: 0, done: 0, current: '', errors: [], t0: 0, damaged: [], net: [], tried: 0 });
  tell();
  try {
    const [{ paths, have, missing: list }, { bad, plays }, sm] = await Promise.all([missing(), damagedList(), import('./smooth.js')]);
    // GIFs that play as they are need nothing (they count as done). One
    // that has a copy all the same is made again here: a lossy copy from
    // before goes (one() finds it past 256 colours), while the copy of a GIF
    // replaced since (the line is about the old file) is exact and stays.
    const recheck = paths.filter((p) => {
      const c = app.wasm.cachePathFor(p);
      return plays.has(p.toLowerCase()) && c && have.has(c.toLowerCase());
    });
    const { listed, redo } = await sm.pending(paths, list);
    const todo = [];
    for (const p of [...list, ...redo]) {
      const lp = p.toLowerCase(), why = bad.get(lp);
      if (plays.has(lp)) continue;
      if (why != null) job.damaged.push({ path: p, reason: why, known: true });
      else todo.push(p);
    }
    todo.push(...recheck);
    const how = (p) => (listed.has(p.toLowerCase()) ? sm : null);
    Object.assign(job, { phase: 'preparing', todo: todo.length, t0: performance.now() });
    tell();
    const later = [];
    for (const path of todo) {
      if (job.stopping) break;
      job.current = path;
      tell();
      const r = await one(path, true, how(path));
      job.tried++;
      if (r === 'ok' || r === 'asis') job.done++;
      else if (r === 'net') { later.push(path); job.net.push(path); }
    }
    // Network failures get one more go at the end, once the rest is done.
    for (const path of later) {
      if (job.stopping) break;
      job.current = path;
      tell();
      const r = await one(path, false, how(path));
      if (r !== 'net') job.net.splice(job.net.indexOf(path), 1);
      if (r === 'ok' || r === 'asis') job.done++;
      tell();
    }
  } catch (e) {
    job.errors.push(e.message || String(e));
  }
  Object.assign(job, { running: false, phase: job.stopping ? 'stopped' : 'finished', current: '' });
  tell();
}

export function stop() { if (job.running) { job.stopping = true; tell(); } }

// Seconds left at the pace so far, or null before the first file is done.
export function eta() {
  const n = job.tried;
  if (!n || !job.t0) return null;
  return ((performance.now() - job.t0) / 1000 / n) * (job.todo - n);
}
