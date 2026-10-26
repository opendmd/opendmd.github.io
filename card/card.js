// "Set up a card" (/card/): the DMDoS GIF pack's zip in, a card ready to
// play out. The zip is read where it is (nothing is uploaded): its list of
// files from the end of the file, each GIF inflated by a pool of workers
// (worker.js) that write it to the card the user picked, with its lighter
// version beside it, made by the web app's own encoder. The card is listed
// first, so a second run carries on where the first stopped and writes
// nothing that is already there; the playlists go last, so a card that was
// interrupted has none of its own yet. The words on the page are GIFs,
// playlists and the card: never how the player keeps its lighter versions.
//
// `?target=opfs` writes into the browser's own private storage instead of
// a picked folder (the same handle API): how the site's smoke test runs the
// page headless. `&pace=ms` (with it) slows each file, so a test can stop a
// run half-way.

import { readZip, entryBytes, ZipError } from './zip.js';
import { cardLayout, ordered, lastFiles, missingLines, planWork, failedText, failedReasons, insideCard, strangers, onDisk, playlistName } from './plan.js';

const $ = (id) => document.getElementById(id);
const app = $('cp');
const q = new URLSearchParams(location.search);
const OPFS = q.get('target') === 'opfs';
const PACE = OPFS ? Math.max(0, +q.get('pace') || 0) : 0;
const canWrite = OPFS ? !!navigator.storage?.getDirectory : typeof window.showDirectoryPicker === 'function';
const WORKERS = Math.max(1, Math.min(navigator.hardwareConcurrency || 4, 4));
const BATCH = 4;
const fmt = (n) => n.toLocaleString('en');
const plural = (n, one, many = one + 's') => `${fmt(n)} ${n === 1 ? one : many}`;
const gb = (n) => (n >= 1e9 ? `${(n / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(n / 1e6))} MB`);
const minutes = (s) => (s < 90 ? 'about a minute' : `about ${Math.round(s / 60)} minutes`);
// A GIF's lighter version is about 0.61 of it, on the pack.
const SHRINK = 0.61;

let zip = null, layout = null, texts = new Map(), raws = new Map(), wasm = null;
let card = null, listing = null, plan = null;

function state(s) { app.dataset.state = s; }
function show(id, on = true) { $(id).hidden = !on; }
function say(id, text) { $(id).textContent = text; }

// --- 1. the zip ---------------------------------------------------------------
async function takeZip(file) {
  if (run && !run.over) return;
  for (const id of ['cp-found', 'cp-zip-err', 'cp-card', 'cp-run', 'cp-done']) show(id, false);
  $('cp-write').disabled = true;
  if (!file) return;
  state('reading');
  say('cp-drop-name', file.name);
  try {
    const { entries } = await readZip(file);
    const l = cardLayout(entries);
    if (!l || !l.gifs.length) {
      return fail('cp-zip-err', 'This zip doesn\'t look like a card: it has no gifs folder with a filelist.txt beside it. ' +
        'Is it the GIF pack from dmdos.net?');
    }
    // The playlists are small: read them now, for the playing order and to write them at the end.
    texts = new Map(); raws = new Map();
    for (const pl of l.playlists) {
      const b = await entryBytes(file, pl.entry);
      raws.set(pl.path, b);
      texts.set(pl.path, new TextDecoder('latin1').decode(b));
    }
    zip = file; layout = l;
    const est = l.gifs.reduce((a, g) => a + onDisk(g.entry.usize) + onDisk(g.entry.usize * SHRINK), 0)
      + (l.boot ? onDisk(l.boot.entry.usize) : 0);
    const names = l.playlists.map((p) => playlistName(p.path));
    say('cp-found-text', `Found ${plural(l.gifs.length, 'GIF')} in ${plural(l.categories.size, 'category', 'categories')} ` +
      `and ${plural(l.playlists.length, 'playlist')}${names.length ? ` (${names.join(', ')})` : ''}. ` +
      `They take about ${gb(est)} on the card${est > 2.5e9 ? ': a 4 GB card holds them, 8 GB or more is comfortable' : ''}.`);
    app.dataset.gifs = l.gifs.length;
    show('cp-found');
    $('cp-write').disabled = !canWrite;
    state('read');
  } catch (e) {
    if (e instanceof ZipError) {
      return fail('cp-zip-err', `This file can't be read as a zip (${e.message}). If it's the GIF pack, download it again.`);
    }
    throw e;
  }
}

function fail(id, text) {
  say(id, text);
  show(id);
  state('error');
}

// --- 2. the card ----------------------------------------------------------------
// Every file under the card's gifs/, cache/ and logos/ and at its top, with
// its size (lowercased path -> size); the swap files an interrupted write
// can leave (*.crswap) are removed on the way.
async function listCard(root, tick) {
  const have = new Map();
  let failed = '', swaps = 0, n = 0;
  const top = [];
  // A few folders at a time: thousands of small requests, not all at once.
  const limit = (() => {
    let busy = 0; const wait = [];
    return async (fn) => {
      while (busy >= 8) await new Promise((r) => wait.push(r));
      busy++;
      try { return await fn(); } finally { busy--; wait.shift()?.(); }
    };
  })();
  async function walk(dir, prefix) {
    const subs = [];
    await limit(async () => {
      for await (const [name, h] of dir.entries()) {
        if (h.kind === 'directory') { subs.push([h, prefix + name + '/']); continue; }
        if (/\.crswap$/i.test(name)) {
          await dir.removeEntry(name).catch(() => {});
          swaps++;
          continue;
        }
        const f = await h.getFile();
        have.set((prefix + name).toLowerCase(), f.size);
        if ((prefix + name).toLowerCase() === 'cache/.failed') failed = await f.text();
        if (++n % 200 === 0) tick(n);
      }
    });
    await Promise.all(subs.map(([h, p]) => walk(h, p)));
  }
  for await (const [name, h] of root.entries()) {
    top.push(name);
    if (h.kind === 'directory') {
      if (/^(gifs|cache|logos)$/i.test(name)) await walk(h, name + '/');
    } else if (/\.crswap$/i.test(name)) {
      await root.removeEntry(name).catch(() => {});
      swaps++;
    } else {
      have.set(name.toLowerCase(), (await h.getFile()).size);
    }
  }
  return { have, failed, swaps, top };
}

async function pickCard() {
  let root;
  try {
    root = OPFS ? await navigator.storage.getDirectory()
      : await window.showDirectoryPicker({ mode: 'readwrite', id: 'opendmd-card', startIn: 'desktop' });
  } catch (e) {
    if (e?.name === 'AbortError') return;
    return fail('cp-card-err', `The card couldn't be opened: ${e.message}`);
  }
  for (const id of ['cp-card-err', 'cp-run', 'cp-done', 'cp-eject']) show(id, false);
  show('cp-card');
  show('cp-start', false);
  say('cp-card-text', 'Looking at what\'s on the card…');
  state('listing');
  try {
    wasm ??= await import('../demo/pkg/opendmd_web.js').then(async (m) => { await m.default(); return m; });
    listing = await listCard(root, (n) => say('cp-card-text', `Looking at what's on the card… ${fmt(n)} files`));
  } catch (e) {
    return fail('cp-card-err', `The card couldn't be read: ${e.message}`);
  }
  card = root;
  plan = planWork(ordered(layout, texts), lastFiles(layout), listing.have, failedReasons(listing.failed), (p) => wasm.cachePathFor(p));
  const odd = listing.top.some((n) => /^gifs$/i.test(n)) ? [] : strangers(listing.top);
  const total = layout.gifs.length;
  let text;
  if (!plan.items.length && !plan.last.length) {
    text = 'Everything is already on this card: it\'s ready to play.';
  } else if (plan.gifs >= total) {
    const bytes = plan.items.reduce((a, it) => a + it.e.usize * (1 + SHRINK), 0);
    text = `${plural(plan.items.length, 'GIF')} to write, about ${gb(bytes)}.`;
  } else if (!plan.items.length) {
    text = 'The GIFs are all on this card: only the playlists to write.';
  } else if (plan.retries === plan.items.length && !plan.last.length) {
    text = `Everything is already on this card: it's ready to play. ${plural(plan.retries, 'GIF')} couldn't be got ` +
      `ready before; Start tries ${plan.retries === 1 ? 'it' : 'them'} again.`;
  } else if (plan.gifs === 0) {
    text = `The GIFs are already on this card: ${plural(plan.items.length, 'GIF')} to get ready to play.`;
  } else {
    text = `Picking up where it stopped: ${plural(plan.items.length, 'GIF')} to go.`;
  }
  if (insideCard(root.name)) {
    text = `This is a folder called “${root.name}”: is it a folder on the card? Pick the card itself, the drive, ` +
      `so the GIFs go where the player looks for them. ` + text;
  } else if (odd.length) {
    const names = odd.slice(0, 3).join(', ') + (odd.length > 3 ? ', …' : '');
    text = `This folder has other things in it (${names}): is it the SD card? Writing adds a gifs folder and the ` +
      `playlists here, and deletes nothing. ` + text;
  }
  say('cp-card-text', text);
  app.dataset.todo = plan.items.length;
  app.dataset.swaps = listing.swaps;
  if (plan.items.length || plan.last.length) {
    show('cp-start');
    $('cp-start').textContent = odd.length || insideCard(root.name) ? 'Write here anyway' : 'Start';
    state('listed');
  } else {
    state('ready');
    show('cp-eject');
  }
}

// --- 3. the run -------------------------------------------------------------------
async function writeFile(root, path, bytes) {
  const parts = path.split('/');
  const name = parts.pop();
  let d = root;
  for (const p of parts) d = await d.getDirectoryHandle(p, { create: true });
  const w = await (await d.getFileHandle(name, { create: true })).createWritable();
  try { await w.write(bytes); await w.close(); } catch (e) { await w.abort().catch(() => {}); throw e; }
}

let run = null;

function guard(e) { e.preventDefault(); e.returnValue = ''; }
async function wake() {
  try { if (run && !run.over && document.visibilityState === 'visible') run.lock = await navigator.wakeLock?.request('screen'); } catch { /* not offered */ }
}

async function start() {
  show('cp-start', false);
  show('cp-card', false);
  show('cp-done', false);
  show('cp-run');
  $('cp-stop').disabled = false;
  for (const id of ['cp-file', 'cp-write']) $(id).disabled = true;
  state('running');
  const items = plan.items.slice();
  const r = run = {
    t0: performance.now(), total: items.length, done: 0, samples: [], over: false, stopping: false, fatal: null,
    gifs: 0, copies: 0, bytes: 0, asIs: [], made: [], damaged: [], bad: [], unsupported: [], failed: [], lock: null,
  };
  addEventListener('beforeunload', guard);
  document.addEventListener('visibilitychange', wake);
  await wake();
  progress(true);
  const timer = setInterval(() => progress(false), 1000);
  const workers = [];
  let spawned = 0;
  await new Promise((resolve) => {
    const settle = () => { if (workers.every((x) => x.idle)) resolve(); };
    const next = (w) => {
      if (r.stopping || !items.length) { w.idle = true; w.batch = null; settle(); return; }
      w.batch = items.splice(0, BATCH);
      w.postMessage({ type: 'work', items: w.batch });
    };
    // Stop: each writer finishes the file in hand; one still busy after a
    // grace period (an encoder that never returns) is cut off. Its write
    // was either finished or never moved into place.
    const stopAll = () => {
      if (r.stopping) return;
      r.stopping = true;
      for (const w of workers) w.postMessage({ type: 'stop' });
      setTimeout(() => {
        for (const w of workers) if (!w.idle) { w.terminate(); w.idle = true; }
        settle();
      }, 5000);
    };
    r.stopAll = stopAll;
    const spawn = () => {
      spawned++;
      const w = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
      w.idle = false;
      w.batch = null;
      workers.push(w);
      w.onmessage = ({ data: m }) => {
        if (m.type === 'ready') { next(w); return; }
        if (m.type !== 'batch' || w.idle) return;
        for (const d of m.done) tally(d);
        // What the writer didn't get to goes back on the queue.
        const got = new Set(m.done.map((d) => d.path));
        const left = (w.batch || []).filter((it) => !got.has(it.path));
        if (!r.stopping && !m.fatal) items.unshift(...left);
        progress(false);
        if (m.fatal) {
          r.fatal ??= m.fatal;
          w.idle = true;
          stopAll();
          settle();
        } else if (m.dead) {
          // A writer whose wasm went wrong: replace it (a few times at most).
          w.terminate();
          w.idle = true;
          if (spawned < WORKERS + 8 && !r.stopping) spawn();
          else if (!r.stopping) { r.fatal ??= { name: 'LoadError', message: 'the page\'s writers keep failing' }; stopAll(); }
          settle();
        } else next(w);
      };
      w.onerror = (e) => {
        e.preventDefault?.();
        r.fatal ??= { name: 'LoadError', message: e.message || 'a writer stopped' };
        w.idle = true;
        stopAll();
        settle();
      };
      w.postMessage({ type: 'init', zip, root: card, pace: PACE });
    };
    for (let i = 0; i < Math.min(WORKERS, items.length); i++) spawn();
    if (!items.length) resolve();
  });
  // The boot logo and the playlists last (after the player's list of GIFs
  // with no lighter version): only when everything before them was written.
  // (cache/.failed after a stop too, so the next run knows those GIFs are done.)
  let lists = 0;
  $('cp-stop').disabled = true;
  if (!r.fatal) {
    try {
      if (!r.stopping) say('cp-status', 'Writing the playlists…');
      const ft = failedText(listing.failed, [...r.asIs, ...r.damaged].map(([p, why]) => ['/' + p, why]), r.made.map((p) => '/' + p));
      if (ft != null) await writeFile(card, 'cache/.failed', new TextEncoder().encode(ft));
      for (const f of r.stopping || r.done < r.total ? [] : plan.last) {
        await writeFile(card, f.path, raws.get(f.path) ?? await entryBytes(zip, f.entry));
        if (raws.has(f.path)) lists++;
      }
    } catch (e) {
      r.fatal = { name: e.name, message: e.message };
    }
  }
  for (const w of workers) w.terminate();
  clearInterval(timer);
  r.over = true;
  removeEventListener('beforeunload', guard);
  document.removeEventListener('visibilitychange', wake);
  r.lock?.release().catch(() => {});
  for (const id of ['cp-file', 'cp-write']) $(id).disabled = false;
  done(r, lists);
}

function tally(d) {
  const r = run;
  r.done++;
  if (d.gif) r.gifs++;
  if (d.copy) { r.copies++; r.made.push(d.path); }
  r.bytes += d.gif + d.copy;
  if (d.asIs) r.asIs.push([d.path, d.asIs]);
  if (d.damaged) r.damaged.push([d.path, d.damaged]);
  if (d.bad) r.bad.push(d.path);
  if (d.unsupported) r.unsupported.push(d.path);
  if (d.failed) r.failed.push(`${d.path}: ${d.failed}`);
}

function progress(first) {
  const r = run;
  const now = performance.now();
  r.samples.push([now, r.done]);
  while (r.samples.length > 2 && now - r.samples[1][0] > 30000) r.samples.shift();
  const [t0, d0] = r.samples[0];
  const rate = (r.done - d0) / ((now - t0) / 1000);
  let left = '';
  if (!first && now - r.t0 > 10000 && rate > 0) left = ` · ${minutes((r.total - r.done) / rate)} left`;
  say('cp-status', `${r.stopping ? 'Stopping' : 'Writing GIFs and getting them ready'} · ${fmt(r.done)} of ${fmt(r.total)}${left}`);
  $('cp-bar').max = Math.max(1, r.total);
  $('cp-bar').value = r.done;
  app.dataset.done = r.done;
}

function stop() {
  if (!run || run.over) return;
  $('cp-stop').disabled = true;
  run.stopAll?.();
  progress(false);
}

// --- 4. done --------------------------------------------------------------------------
function li(list, items) {
  const ul = $(list);
  ul.replaceChildren(...items.slice(0, 200).map((t) => Object.assign(document.createElement('li'), { textContent: t })));
  if (items.length > 200) ul.append(Object.assign(document.createElement('li'), { textContent: `and ${fmt(items.length - 200)} more` }));
}

function done(r, lists) {
  show('cp-run', false);
  show('cp-done');
  const secs = (performance.now() - r.t0) / 1000;
  app.dataset.ms = Math.round(secs * 1000);
  app.dataset.written = r.gifs;
  app.dataset.made = r.copies;
  app.dataset.bytes = r.bytes;
  app.dataset.playlists = lists;
  const took = secs < 60 ? plural(Math.max(1, Math.round(secs)), 'second') : plural(Math.round(secs / 60), 'minute');
  const names = layout.playlists.map((p) => playlistName(p.path));
  let head, ok = false;
  if (r.fatal?.name === 'ZipGone') {
    head = `The zip couldn't be read any more after ${plural(r.done, 'GIF')} (was it moved, renamed or deleted?). Drop it ` +
      `again, then choose the card again to carry on where it stopped.`;
  } else if (r.fatal?.name === 'QuotaExceededError') {
    head = `The card is full after ${plural(r.done, 'GIF')}. Free some space or use a bigger card, then choose it again with ` +
      `Write to the card… to carry on.`;
  } else if (r.fatal && ['NotFoundError', 'NotAllowedError', 'SecurityError', 'InvalidStateError'].includes(r.fatal.name)) {
    head = `The card went away after ${plural(r.done, 'GIF')} (was it taken out?). Put it back, then choose it again with ` +
      `Write to the card… to carry on where it stopped.`;
  } else if (r.fatal) {
    head = `Writing stopped after ${plural(r.done, 'GIF')}: ${r.fatal.message}. Reload the page and choose the card again ` +
      `to carry on where it stopped.`;
  } else if (r.stopping || r.done < r.total) {
    head = `Stopped after ${fmt(r.done)} of ${plural(r.total, 'GIF')}. Nothing is half-written: choose the card again with ` +
      `Write to the card… to carry on where it stopped.`;
  } else {
    ok = true;
    const all = layout.gifs.length - r.bad.length - r.unsupported.length - r.failed.length;
    head = `Your card is ready: ${plural(all, 'GIF')} and ${plural(names.length, 'playlist')}` +
      `${names.length ? ` (${names.join(', ')})` : ''}, ready to play from the first boot. It took ${took}.`;
  }
  state(ok ? 'done' : r.stopping && !r.fatal ? 'stopped' : 'failed');
  say('cp-done-text', head);
  show('cp-eject', ok);
  const notes = [
    ['cp-n-bad', r.bad, `${plural(r.bad.length, 'GIF')} in the zip ${r.bad.length === 1 ? 'is' : 'are'} damaged and ` +
      `${r.bad.length === 1 ? 'wasn\'t' : 'weren\'t'} put on the card. Download the pack again and run this page again to add them.`],
    ['cp-n-unsupported', r.unsupported, `${plural(r.unsupported.length, 'GIF')} in the zip ${r.unsupported.length === 1 ? 'is' : 'are'} ` +
      `packed in a way this page can't read (was the zip re-made?) and ${r.unsupported.length === 1 ? 'wasn\'t' : 'weren\'t'} put on the card.`],
    ['cp-n-failed', r.failed, `${plural(r.failed.length, 'GIF')} couldn't be written to the card. Run this page again to try them again.`],
    ['cp-n-damaged', r.damaged, `${plural(r.damaged.length, 'GIF')} can't be played by anything: ` +
      `${r.damaged.length === 1 ? 'it\'s' : 'they\'re'} on the card as ${r.damaged.length === 1 ? 'it is' : 'they are'}.`],
  ];
  for (const [id, list, text] of notes) {
    show(id, list.length > 0);
    if (list.length) { say(id + '-s', text); li(id + '-l', list.map((x) => (Array.isArray(x) ? x[0] : x))); }
  }
  const miss = missingLines(layout, texts);
  show('cp-n-missing', ok && miss > 0);
  say('cp-n-missing', `${plural(miss, 'line')} of the playlists name${miss === 1 ? 's' : ''} GIFs the pack doesn't have ` +
    `(DMDoS has the same lines). The player skips them.`);
  show('cp-n-program', ok && layout.program);
  app.dataset.damaged = r.damaged.length;
  app.dataset.bad = r.bad.length;
  app.dataset.unsupported = r.unsupported.length;
  app.dataset.failed = r.failed.length;
}

// --- wiring -----------------------------------------------------------------------
const input = $('cp-file');
input.addEventListener('change', () => takeZip(input.files[0]).catch((e) => fail('cp-zip-err', String(e?.message ?? e))));
const drop = $('cp-drop');
drop.addEventListener('dragover', (e) => { e.preventDefault(); drop.classList.add('over'); });
drop.addEventListener('dragleave', () => drop.classList.remove('over'));
drop.addEventListener('drop', (e) => {
  e.preventDefault();
  drop.classList.remove('over');
  const f = e.dataTransfer?.files?.[0];
  if (f) takeZip(f).catch((err) => fail('cp-zip-err', String(err?.message ?? err)));
});
$('cp-write').addEventListener('click', () => pickCard().catch((e) => fail('cp-card-err', String(e?.message ?? e))));
$('cp-again').addEventListener('click', () => pickCard().catch((e) => fail('cp-card-err', String(e?.message ?? e))));
$('cp-start').addEventListener('click', () => start().catch((e) => fail('cp-card-err', String(e?.message ?? e))));
$('cp-stop').addEventListener('click', stop);
show('cp-write', canWrite);
show('cp-nowrite', !canWrite);
state('idle');
