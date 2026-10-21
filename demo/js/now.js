// Now: what the panel is doing. On a device, /api/status every 500 ms while
// this view is up, the remote control (skip, pause, brightness; sending a
// GIF to the panel is the Library's) and a panel-look preview of the file on
// screen, fetched once per change and kept in step with the panel by clock
// (elapsed_ms at the device's t_ms, offset estimated NTP-style; CLAUDE.md,
// "The Now preview is in step"). In local mode, a simulator over the picked
// folder that plays one file at a time with the firmware's dwell.

import { app, playlistLabel } from './state.js';
import { PanelPreview, lookControls } from './preview.js';
import { bannerFor } from './schedule.js';
import { followLine } from './follow.js';
import { h, clear, on, store, debounce, toast, bytes, uptime, label, niceName, flagName, brightSlider } from './util.js';

const POLL_MS = 500;

// "Exolon2 · Spectrum · Slide": the platform, category and flags by name.
const tags = (e) => [e.platform, e.category && niceName(e.category), ...e.flags.map(flagName)].filter(Boolean).map((t) => ' · ' + t).join('');

export async function mount(el) {
  return app.device ? mountDevice(el) : mountLocal(el);
}

function mountDevice(el) {
  const canvas = h('canvas.panel', { id: 'now-panel' });
  const follow = h('input', { type: 'checkbox', id: 'now-follow', checked: store.get('follow', true) });
  const cap = h('div.cap', { id: 'now-cap' });
  const drift = h('span.mono.muted', { id: 'now-drift' });
  const title = h('b', { id: 'now-title' }, '…');
  // The file's path: never shown (the title says what it is); kept for tools/web-smoke.py.
  const meta = h('span', { id: 'now-path', hidden: true });
  const followEl = h('span.tag.fl', { id: 'now-follow-line', hidden: true });
  const tagsEl = h('span', { id: 'now-tags' });
  const ord = h('span', { id: 'now-ord' });
  const pauseBtn = h('button', { id: 'now-pause', onclick: togglePause }, 'Pause');
  const bright = brightSlider('now-bright', app.config?.brightness ?? 96);
  bright.ceil(app.config?.max_brightness);
  const grid = h('span.figs', { id: 'now-stats' });
  const facts = h('dl.facts', { id: 'now-facts', style: 'margin-top:16px' });
  const conn = h('span.dot', { id: 'now-conn' });
  const more = h('details.card', { id: 'now-more', open: store.get('health', false), ontoggle: () => store.set('health', more.open) },
    h('summary', grid, h('span.muted', 'More')), facts);

  const banner = h('div.banner', { id: 'now-banner', hidden: true });
  // A card the panel can't read (exFAT, as most cards over 32 GB come): say
  // so, with the way out (js/card.js, imported when first used).
  const sdText = h('span.grow.bad', { id: 'now-sd-text' });
  const sdLine = h('div.row', { id: 'now-sd', hidden: true, style: 'margin-top:12px' }, sdText,
    h('button.sm', { id: 'now-format', onclick: () => import('./card.js').then((m) => m.formatCard()) }, 'Format card…'));
  el.append(
    banner,
    h('div.card', canvas, cap, sdLine,
      h('div.row.capl', h('span.grow', title, h('span.muted', tagsEl, ' · ', ord), ' ', followEl, ' ', conn, meta),
        h('label.cap', follow, 'Follow the panel')),
      h('div.row', { style: 'margin-top:12px' }, pauseBtn, h('button.pri', { id: 'now-skip', onclick: skip }, 'Skip'),
        h('label', 'Brightness', bright.el))),
    more,
    lookControls(drift));

  const preview = new PanelPreview(canvas, { wrap: el, onError: (m) => { cap.textContent = 'Preview: ' + m; } });
  let shownPath = '';
  let timer = 0;
  let failures = 0;
  let clocks = [];   // [{rtt, off}]: off = device ms - local ms
  let last = null;   // the last status sample: {path, elapsed, t, frame, rate} (device clock)
  let base = null;   // this file's first sample, for the rate its position runs at

  follow.onchange = () => { store.set('follow', follow.checked); if (follow.checked) render(app.status); else { preview.clear(); shownPath = ''; } };
  const sendBright = debounce(async (v) => {
    try { await app.src.putConfig({ brightness: v }); if (app.config) app.config.brightness = v; } catch (e) { toast('brightness: ' + e.message, 'bad'); }
  }, 300);
  bright.input.addEventListener('input', () => sendBright(+bright.input.value));
  on('config', (c) => { bright.ceil(c.max_brightness); if (document.activeElement !== bright.input) bright.set(c.brightness); });

  async function skip() {
    try { await app.src.skip(); setTimeout(poll, 400); } catch (e) { toast(e.message, 'bad'); }
  }

  async function togglePause() {
    try { const r = await app.src.pause(); app.status.paused = r.paused; render(app.status); } catch (e) { toast(e.message, 'bad'); }
  }

  // The preview fetches the file's prepared copy first (app.previewBytes):
  // fetching from gifs/Consoles costs the device a ~330 ms directory scan
  // that the panel feels; a copy's small directory costs ~6 ms.
  async function follow_(path) {
    if (!follow.checked || !path || path === shownPath) return;
    shownPath = path;
    cap.textContent = 'Loading…';
    try {
      if (await preview.show(path, (p) => app.previewBytes(p))) { cap.textContent = ''; sync(); poll(); }
    } catch (e) { cap.textContent = 'Preview: ' + e.message; }
  }

  // One status sample, requested at local t0 and answered at t1.
  function clock(s, t0, t1) {
    if (typeof s.t_ms !== 'number') { last = null; return; }
    const off = s.t_ms - (t0 + t1) / 2;
    // A reboot restarts the device clock: forget the old samples. (Not an
    // offset jump: one reply stuck behind a file transfer has a big one.)
    if (clocks.length && s.t_ms + 5000 < clocks.at(-1).t) clocks = [];
    clocks.push({ rtt: t1 - t0, off, t: s.t_ms });
    if (clocks.length > 8) clocks.shift();
    const np = s.now_playing;
    // The panel's position runs a little slower than its clock (10 ms
    // sleep ticks); measure how much from its own reports, over >= 2 s.
    const key = `${np.ordinal} ${np.path} ${s.paused}`;
    if (base?.key !== key) base = { key, t: s.t_ms, e: np.elapsed_ms };
    const span = s.t_ms - base.t;
    const rate = span >= 2000 ? Math.min(1.1, Math.max(0.5, (np.elapsed_ms - base.e) / span)) : 1;
    last = { path: np.path, elapsed: np.elapsed_ms, t: s.t_ms, frame: np.frame, rate };
  }

  // Put the preview in step with the last sample, if it is of this file.
  function sync() {
    if (!last || !last.frame || preview.path !== last.path) return;
    const best = clocks.reduce((a, b) => (b.rtt < a.rtt ? b : a));
    const d = preview.syncTo(last.elapsed, last.t - best.off, last.rate);
    if (Number.isNaN(d)) return;
    // data-*: for tools/web-smoke.py (k = syncs of this file so far).
    const ds = drift.dataset;
    ds.k = ds.path === last.path ? +ds.k + 1 : 1;
    Object.assign(ds, { path: last.path, drift: d.toFixed(1), rtt: best.rtt.toFixed(1), rate: last.rate.toFixed(4), paused: preview.paused });
    drift.textContent = `Drift ${Math.round(d)} ms · RTT ${Math.round(best.rtt)} ms`;
  }

  const stat = (name, value) => h('span.stat', h('b', value), h('span', name));
  // Late frames a person could see (5 ms or more), playing on and at a GIF
  // change (the new file's first frame and transition); every late frame,
  // any size, only under More. Firmware before 0.3.12 has just the total.
  // (One line on a laptop, two on a phone.)
  const late = (m, n) => m?.steady === undefined ? n.toLocaleString()
    : [h('span', `${m.steady.toLocaleString()} during play`), h('span.sep', ' · '), h('span', `${m.switch.toLocaleString()} at GIF changes`)];
  const lateTip = (m) => m?.threshold_ms === undefined ? ''
    : `Frames ${m.threshold_ms} ms or more late. GIF changes: the new GIF's first frame, its transition and its first ${m.window_ms / 1000} s`;
  const fact = (name, value, bad) => [h('dt', name), h('dd' + (bad ? '.bad' : ''), value)];

  function render(s) {
    if (!s) return;
    const np = s.now_playing;
    // A virtual file (`@…`: Follow RePlayOS's cards) shows its title only.
    const virt = np.path?.startsWith('@');
    const e = app.wasm.parsePath(virt ? '' : np.path || '');
    title.textContent = virt ? np.title : np.path ? label(e) : 'Nothing playing';
    tagsEl.textContent = np.path && !virt ? tags(e) : '';
    meta.textContent = virt ? '' : np.path;
    const fl = followLine(s);
    followEl.hidden = !fl;
    followEl.textContent = followEl.title = fl ?? '';
    ord.textContent = `${np.ordinal} of ${s.entries.toLocaleString()}`;
    pauseBtn.textContent = s.paused ? 'Resume' : 'Pause';
    const b = bannerFor(s, () => setTimeout(poll, 300));
    banner.hidden = !b;
    banner.dataset.mode = s.mode || 'normal';
    if (b) clear(banner, b);
    clear(grid,
      stat('Uptime', uptime(s.uptime_secs)),
      stat('Free memory', bytes(s.heap.free)),
      Object.assign(stat('Late frames', late(s.misses, s.deadline_misses)), { id: 'now-late', className: 'stat w', title: lateTip(s.misses) }));
    clear(facts,
      fact('Address', s.ip || '–'),
      fact('Lowest free memory', bytes(s.heap.floor), s.heap.floor < 20000),
      fact('Refresh', s.refresh_hz + ' Hz'),
      fact('Late frames, any size', `${s.deadline_misses.toLocaleString()} · worst ${s.worst_lateness_ms} ms`),
      fact('Files started', s.gifs_started.toLocaleString()),
      fact('Playlist', playlistLabel(s.manifest)),
      fact('Card', !s.sd.mounted ? 'Not mounted' : s.sd.formatting ? 'Formatting…' : s.sd.readable === false ? 'Not readable: needs FAT32' : bytes(s.sd.capacity_bytes), s.sd.readable === false || !s.sd.mounted));
    sdLine.hidden = !s.sd.mounted || s.sd.readable !== false;
    sdText.textContent = s.sd.formatting ? 'Formatting the card…' : 'This card can’t be read: it needs FAT32';
    if (preview.paused !== s.paused) preview.setPaused(s.paused);
    follow_(np.path);
  }

  // A failed poll leaves the preview playing on its last sync. One at a
  // time: the device has one httpd task, and a reply queued behind the
  // preview's file fetch must not have others piling up behind it.
  let busy = false;
  async function poll() {
    if (document.hidden || busy) return;
    busy = true;
    try {
      const t0 = performance.now();
      const s = await app.refreshStatus();
      clock(s, t0, performance.now());
      render(s);
      sync();
      if (failures && preview.path) cap.textContent = '';
      failures = 0;
      conn.className = 'dot ok';
      conn.title = 'Live';
    } catch (e) {
      conn.className = 'dot bad';
      conn.title = ++failures > 1 ? 'Offline' : 'Retrying';
      if (preview.path) cap.textContent = 'Panel not reachable';
    } finally { busy = false; }
  }

  render(app.status);
  return {
    show() { poll(); clearInterval(timer); timer = setInterval(poll, POLL_MS); },
    hide() { clearInterval(timer); },
  };
}

function mountLocal(el) {
  const canvas = h('canvas.panel', { id: 'now-panel' });
  const title = h('b', { id: 'now-title' }, 'Local simulator');
  const line = h('span.muted', { id: 'now-path' });
  const listSel = h('select', { id: 'sim-list' });
  const plats = h('input', { type: 'text', id: 'sim-platforms', placeholder: 'MD,SNES — blank for all', size: 18 });
  const shuffle = h('input', { type: 'checkbox', id: 'sim-shuffle' });
  const seed = h('input', { type: 'number', id: 'sim-seed', value: 7, min: 0, style: 'width:90px' });
  const pauseBtn = h('button', { id: 'now-pause', disabled: true, onclick: () => { preview.setPaused(!preview.paused); pauseBtn.textContent = preview.paused ? 'Resume' : 'Pause'; } }, 'Pause');
  const nextBtn = h('button', { id: 'now-skip', disabled: true, onclick: () => next() }, 'Next');
  let list = [];
  let i = -1;

  el.append(
    h('div.card', canvas,
      h('div.row.capl', h('span.grow', title, ' ', line), h('span.cap', 'One file in memory at a time, with the firmware’s 3–30 s dwell')),
      h('div.row', { style: 'margin-top:12px' },
        h('label', 'Playlist', listSel), h('label', 'Platforms', plats), h('label', shuffle, 'Shuffle'), h('label', 'Seed', seed),
        h('button.pri', { id: 'sim-play', onclick: play }, 'Play'), pauseBtn, nextBtn)),
    lookControls());

  const preview = new PanelPreview(canvas, {
    wrap: el,
    onDwellEnd: () => next(),
    onError: (m) => { toast(m, 'bad'); next(); },
  });
  preview.setDwell(3, 30);

  const src = app.src;
  const manifests = [...src.files.keys()].filter((p) => /^\/filelist\.[^/]*$|^\/sets\/[^/]+\.txt$/.test(p) && !/\/filelist\.odm\.|\.(idx|part|tmp|bak|meta)$/.test(p));
  for (const m of manifests) listSel.append(h('option', { value: m }, playlistLabel(m)));
  listSel.append(h('option', { value: '' }, 'Every GIF in the folder'));

  async function play() {
    let text;
    if (listSel.value) text = await src.readText(listSel.value);
    else text = [...src.files.keys()].filter((p) => /\.gif$/i.test(p)).sort().join('\n');
    list = app.wasm.filterManifest(text, JSON.stringify({ platforms: plats.value, shuffle: shuffle.checked, seed: +seed.value }));
    if (!list.length) { toast('the filter matched nothing', 'bad'); return; }
    i = -1;
    nextBtn.disabled = pauseBtn.disabled = false;
    next();
  }

  // Skip entries the folder does not have (a manifest can name files that
  // were not extracted), but give up after a full lap.
  async function next(tries = 0) {
    if (!list.length || tries > list.length) return;
    i = (i + 1) % list.length;
    const path = list[i];
    if (path[0] !== '@' && !src.files.has(path)) return next(tries + 1);
    const e = app.wasm.parsePath(path);
    title.textContent = label(e);
    line.textContent = `· ${i + 1} of ${list.length}`;
    await preview.show(path, (p) => app.previewBytes(p));
  }

  if (app.demo) { listSel.value = '/filelist.txt'; play(); }
  return { show() {}, hide() {} };
}

