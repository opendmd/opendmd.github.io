// Settings: the device's NVS config (GET/PUT /api/config — a PUT is a
// partial patch, applied live except WiFi and hostname, which need a
// reboot). Four cards (Playback, Transition, Start-up, Power) that save as
// they change; everything else under Advanced: time, schedule, Follow
// RePlayOS, the playback cache, network (its own Save: it reboots), the
// panel timing (js/panel.js), the device's facts, reindex and reboot,
// firmware and security (js/firmware.js),
// and the About line.

import { app, playlistLabel, setPath } from './state.js';
import * as prep from './prepare.js';
import { timeCard, scheduleCard } from './schedule.js';
import { followCard } from './follow.js';
import { PanelPreview } from './preview.js';
import { h, clear, on, toast, base, note, store, debounce, saver, brightSlider } from './util.js';

const thumbs = () => h('label', { style: 'margin-top:12px' }, h('input', { type: 'checkbox', id: 'st-thumbs', checked: store.get('thumbs', !app.device), onchange: (e) => store.set('thumbs', e.target.checked) }), 'Thumbnails in the Library');

export async function mount(el) {
  if (!app.device) {
    el.append(h('div.card', h('h2', 'Settings'), h('p.muted', 'Device settings, WiFi and reboot live on the player. This page is in local mode: open it from the player (http://opendmd.local/) to use them'), thumbs()));
    return {};
  }
  const c = app.config || await app.refreshConfig();
  const bright = brightSlider('st-brightness', c.brightness);
  bright.ceil(c.max_brightness);
  const f = {
    brightness: bright.input,
    dwell_min: h('input', { type: 'number', min: 1, id: 'st-dwell-min', value: c.dwell_min, style: 'width:100px' }),
    dwell_max: h('input', { type: 'number', min: 1, id: 'st-dwell-max', value: c.dwell_max, style: 'width:100px' }),
    shuffle: h('input', { type: 'checkbox', id: 'st-shuffle', checked: c.shuffle }),
    platforms: h('input', { type: 'hidden', value: c.platforms }),
    set: h('select', { id: 'st-set' }),
    wifi_ssid: h('input', { type: 'text', id: 'st-ssid', value: c.wifi_ssid, autocomplete: 'off' }),
    wifi_pass: h('input', { type: 'password', id: 'st-pass', placeholder: c.wifi_pass_set ? 'unchanged (one is saved)' : 'none saved', autocomplete: 'new-password' }),
    hostname: h('input', { type: 'text', id: 'st-hostname', value: c.hostname, maxLength: 32, oninput: (e) => { const t = e.target, i = t.selectionStart; t.value = t.value.toLowerCase(); t.setSelectionRange(i, i); }, pattern: '[a-z0-9]([a-z0-9\\-]*[a-z0-9])?' }),
    transition: h('select', { id: 'st-trans' }, [['dissolve', 'Dissolve'], ['wipe_left', 'Wipe'], ['blinds', 'Blinds'], ['push', 'Push'], ['fade', 'Fade'], ['none', 'None']].map(([v, l]) => h('option', { value: v }, l))),
    transition_ms: h('select', { id: 'st-trans-ms', 'data-num': 1 }, [[250, 'Quick'], [400, 'Normal'], [700, 'Slow']].map(([v, l]) => h('option', { value: v }, l))),
    look: h('select', { id: 'st-look' }, [['colour', 'Full colour'], ['amber', 'Amber'], ['red', 'Red'], ['green', 'Green'], ['blue', 'Blue'], ['white', 'White']].map(([v, l]) => h('option', { value: v }, l))),
    glow: h('select', { id: 'st-glow' }, [['off', 'Off'], ['short', 'Short'], ['long', 'Long']].map(([v, l]) => h('option', { value: v }, l))),
  };
  f.look.value = c.look ?? 'colour';
  f.glow.value = c.glow ?? 'off';
  // A value set some other way (another wipe direction, another duration)
  // is shown as it is rather than silently replaced.
  const pick = (sel, v, label) => { if (![...sel.options].some((o) => o.value == v)) sel.append(h('option', { value: v }, label)); sel.value = v; };
  pick(f.transition, c.transition ?? 'dissolve', String(c.transition).replace('_', ' '));
  pick(f.transition_ms, c.transition_ms ?? 400, `${c.transition_ms} ms`);
  // "Preview" plays the choice in this page only: the panel's file, started
  // again from its first frame, transitioning in from the frame on screen.
  const pvCanvas = h('canvas.panel', { id: 'st-trans-panel' });
  const pvBox = h('div', { hidden: true, style: 'margin-top:12px' }, pvCanvas);
  let pv = null;
  async function previewTrans() {
    pvBox.hidden = false;
    pv ??= new PanelPreview(pvCanvas);
    pv.p.setTransition(f.transition.value, +f.transition_ms.value);
    const path = app.status?.now_playing?.path;
    if (!pv.path) {
      if (!path || !await pv.show(path, (p) => app.previewBytes(p))) return;
      await new Promise((r) => setTimeout(r, 800));
    }
    pv.p.replay();
  }
  const playH = h('h2', 'Playback'), transH = h('h2', 'Transition'), netH = h('h3', 'Network');
  const play = saver(playH), trans = saver(transH), net = saver(netH);
  // Saved as changed: the slider when it settles, typed fields when left
  // (a platforms change rebuilds the index: not on every keystroke).
  let held = 0; // a slider change not saved yet: another card's config must not undo it
  const later = debounce(() => save(['brightness'], play).then(() => { held = 0; }), 400);
  f.brightness.addEventListener('input', () => { held = 1; later(); });
  for (const k of ['look', 'glow', 'shuffle', 'set']) f[k].onchange = () => save([k], play);
  // Platform chips, counted per manifest text; a config must not undo a tap not yet saved (gen > done).
  const plats = h('div.chips', { id: 'st-plats', style: 'margin-top:4px' });
  let counted = [], gen = 0, done = 0;
  async function platChips() {
    const text = await app.text(setPath(app.config?.set)).catch(() => '');
    if (counted[0] !== text) {
      const n = new Map();
      for (const e of text ? app.wasm.parseManifest(text) : []) if (e.platform) { const p = e.platform.toUpperCase(); n.set(p, -~n.get(p)); }
      counted = [text, [...n].sort((a, b) => b[1] - a[1])];
    }
    const sel = new Set(f.platforms.value.toUpperCase().split(',').map((p) => p.trim()).filter(Boolean)), all = [...counted[1]];
    for (const p of sel) all.some(([q]) => q == p) || all.push([p, 0]);
    const pick = () => { gen++; f.platforms.value = [...sel].join(','); platChips(); platSave(); };
    clear(plats, h('button.chip' + (sel.size ? '' : '.on'), { 'data-platform': '', onclick: () => { sel.clear(); pick(); } }, 'All'),
      all.map(([p, k]) => h('button.chip' + (sel.has(p) ? '.on' : ''), { 'data-platform': p, onclick: () => { sel.delete(p) || sel.add(p); pick(); } }, p, h('small', k))));
  }
  const platSync = (cf = app.config) => { if (gen == done) { f.platforms.value = cf.platforms; platChips(); } };
  const platSave = debounce(() => { const g = gen; return save(['platforms'], play).finally(() => { done = g; platSync(); }); }, 700);
  on('config', platSync);
  platChips();
  f.dwell_min.onchange = f.dwell_max.onchange = () => save(['dwell_min', 'dwell_max'], play);
  f.transition.onchange = f.transition_ms.onchange = () => save(['transition', 'transition_ms'], trans);
  on('config', (cf) => { bright.ceil(cf.max_brightness); if (!held) bright.set(cf.brightness); });

  const nets = h('div.chips', { id: 'st-nets' });
  const banner = h('div.banner', { hidden: true, id: 'st-reboot' }, 'Saved. WiFi and name changes take effect after a reboot. ',
    h('button.sm', { onclick: reboot }, 'Reboot now'));
  const facts = h('dl.facts', { id: 'st-facts' });
  // One quiet line, only when the card has had trouble (sdhealth on the device).
  const cardNote = h('p.cap', { id: 'st-card-note', hidden: true, style: 'margin-top:12px' });
  // Where and what this player is: its address, its firmware and this page's version; the API reference (api.html, from docs/api.md).
  const about = h('dl.facts', { id: 'st-about' });

  // The playlist choices, rebuilt whenever the config changes or the view
  // is shown: a playlist made or deleted elsewhere must not linger here.
  async function loadPlaylists() {
    const set = app.config?.set ?? '';
    const lists = await app.playlists().catch(() => []);
    if (!lists.some((p) => p.value === '')) lists.unshift({ value: '', label: 'All files' });
    if (!lists.some((p) => p.value === set)) lists.push({ value: set, label: playlistLabel(set) });
    clear(f.set, lists.map((p) => h('option', { value: p.value }, p.value ? p.label : `${p.label} (default)`)));
    f.set.value = set;
  }
  await loadPlaylists();
  on('config', loadPlaylists);
  // --- playback cache
  const cacheLine = h('b', { id: 'pc-count' }, '…');
  const cacheBtn = h('button', { id: 'pc-go', onclick: () => (prep.job.running ? prep.stop() : prep.start()) }, 'Prepare remaining files');
  const cacheBar = h('i');
  const cacheMsg = h('div.muted', { id: 'pc-progress' });
  const cacheErr = h('div.log.bad', { id: 'pc-errors' });
  const cacheSum = h('div', { id: 'pc-summary' });
  // The device prepares files by itself when idle (auto_prepare): one
  // sentence, and whether it may also do so, slowly, while playing.
  const autoLine = h('span', { id: 'pc-auto' });
  const pcH = h('h3', 'Playback cache'), pcRun = saver(pcH);
  const always = h('input', { type: 'checkbox', id: 'pc-always', onchange: () => pcRun(async () => {
    await app.src.putConfig({ auto_prepare: always.checked ? 'always' : 'idle' }); await app.refreshConfig(); loadCache();
  }) });
  const autoBox = h('div', { hidden: true, style: 'margin-top:12px' }, autoLine, h('label', always, 'Also while playing (slower)'));

  // Start-up: its own module and size line (js/boot.js), loaded here.
  const boot = h('div.card', { id: 'st-boot-card' });
  import('./boot.js').then((m) => m.bootCard(boot));
  // Maximum brightness: its own module and size line (js/power.js).
  const power = h('div.card', { id: 'st-power-card' });
  const time = timeCard();
  const sched = scheduleCard();
  const follow = followCard();
  on('status', time.render);
  const fw = h('div.grp', { id: 'st-fw', hidden: true }), sec = h('div.grp', { id: 'st-sec', hidden: true }), pt = h('div.grp', { id: 'st-panel', hidden: true });
  let fwq;
  // Firmware, Security and Panel timing (js/panel.js): modules of their own, loaded with Advanced.
  const fwLoad = () => adv.open && (fwq ??= [import('./firmware.js').then((m) => m.fwCards(fw, sec)), import('./panel.js').then((m) => m.panelCard(pt))]);
  const adv = h('details.card', { id: 'st-adv', open: store.get('advanced', false), ontoggle: () => { store.set('advanced', adv.open); fwLoad(); } }, h('summary', 'Advanced'),
    time.el, sched.el, follow.el,
    h('div.grp', { id: 'pc' }, pcH,
      note('Prepared copies play with less memory and start faster', 'Originals stay as they are.'),
      h('div.row.sp', h('span', cacheLine), cacheBtn),
      h('div.bar', { style: 'margin:12px 0' }, cacheBar), cacheMsg, cacheSum, cacheErr, autoBox, thumbs()),
    h('div.grp', netH, note('WiFi and name changes need a reboot', 'With no saved network the player raises the open OpenDMD-Setup access point.'),
      h('div.form.f2',
        h('label.stack', 'WiFi network (SSID)', f.wifi_ssid),
        h('label.stack', 'WiFi password', f.wifi_pass),
        h('label.stack', 'Display name', f.hostname)),
      h('p.cap', 'Give each display its own name: it becomes http://<name>.local/'),
      h('div.row', { style: 'margin-top:12px' }, h('button', { id: 'st-scan', onclick: scan }, 'Scan for networks'), nets),
      h('div.row', { style: 'margin-top:12px' }, h('button', { id: 'st-save-net', onclick: () => f.hostname.reportValidity() && save(['wifi_ssid', 'wifi_pass', 'hostname'], net) }, 'Save network'))),
    pt, h('div.grp', h('h3', 'Device'), facts, cardNote,
      h('div.row', { style: 'margin-top:12px' },
        h('button', { id: 'st-reindex', onclick: reindex }, 'Rebuild index'),
        h('button', { id: 'st-reboot-btn', onclick: reboot }, 'Reboot'))),
    fw, sec, h('div.grp', h('h3', 'About'), about));

  el.append(banner,
    h('div.card', playH, note('A playlist, platform or shuffle change rebuilds the index', 'The card’s index is rebuilt in a few seconds; the panel holds its frame meanwhile.'),
      h('div.form.f2',
        h('label.stack', 'Brightness', bright.el),
        h('label.stack', 'Colour', f.look),
        h('label.stack', 'Phosphor glow', f.glow),
        h('label.stack', 'Dwell min / max (s)', h('div.row', f.dwell_min, f.dwell_max)),
        h('label.stack', 'Playlist', f.set),
        h('div', { style: 'grid-column:1/-1;color:var(--mut);font-size:13px' }, 'Platforms', plats),
        h('label', f.shuffle, 'Shuffle'))),
    h('div.card', { id: 'st-trans-card' }, transH, h('p.cap', 'How the panel moves from one GIF to the next'),
      h('div.form.f2', h('label.stack', 'Style', f.transition), h('label.stack', 'Speed', f.transition_ms)),
      h('div.row', { style: 'margin-top:12px' }, h('button', { id: 'st-trans-preview', onclick: previewTrans }, 'Preview')),
      pvBox),
    boot, power, adv);

  function renderFacts(s) {
    if (!s) return;
    const fact = (k, v) => [h('dt', k), h('dd', v)];
    // (Uptime, memory, playlist and card: Now's health line and More.)
    clear(facts, fact('Entries', s.entries.toLocaleString()), fact('Index', s.index_state ?? '–'));
    const m = s.mdns && s.mdns !== s.hostname;
    clear(about, fact('Address', [`${m ? s.mdns : s.hostname}.local · ${s.ip}`, m && h('div.bad', { id: 'st-mdns' }, `“${s.hostname}” is taken; this display is at ${s.mdns}.local. Give it its own name.`)]), fact('Firmware', `${s.version} (${s.variant})`), fact('Web app', s.www_version ?? '–'),
      fact('API', h('a', { id: 'st-api', href: './api.html', target: '_blank', rel: 'noopener' }, 'API reference')));
    const sd = s.sd;
    const errs = sd.failures || (!sd.mounted && sd.reboots) || 0;
    cardNote.textContent = errs ? `Card errors: ${errs}, retrying`
      : sd.fallback ? 'Card running at reduced speed after errors' : '';
    cardNote.hidden = !cardNote.textContent;
    // A set change while an index is being built would queue behind it.
    f.set.disabled = s.index_state === 'building';
    f.set.title = f.set.disabled ? 'the device is building an index' : '';
    clearTimeout(recheck);
    if (f.set.disabled) recheck = setTimeout(() => app.refreshStatus().catch(() => {}), 2000);
  }
  let recheck = 0;
  on('status', renderFacts);

  // The fields `keys`, read now (a config event may redraw them before the
  // save runs), PUT through `run` where they differ from the config then.
  function save(keys, run) {
    const vals = {};
    for (const k of keys) {
      const el_ = f[k];
      vals[k] = el_.type === 'checkbox' ? el_.checked : el_.type === 'number' || el_.type === 'range' || el_.dataset.num ? +el_.value : el_.value.trim();
    }
    return run(async () => {
      const cur = app.config;
      const patch = {};
      for (const k of keys) {
        if (k === 'wifi_pass') { if (f[k].value) patch[k] = f[k].value; } else if (vals[k] !== cur[k]) patch[k] = vals[k];
      }
      if (!Object.keys(patch).length) return false;
      if ((patch.dwell_max ?? cur.dwell_max) < (patch.dwell_min ?? cur.dwell_min)) throw new Error('dwell max must be at least dwell min');
      const r = await app.src.putConfig(patch);
      await app.refreshConfig();
      f.wifi_pass.value = '';
      f.wifi_pass.placeholder = app.config.wifi_pass_set ? 'unchanged (one is saved)' : 'none saved';
      if (r.reboot_required) banner.hidden = false;
    });
  }

  async function scan() {
    clear(nets, h('span.muted', 'Scanning…'));
    try {
      const aps = await app.src.wifiScan();
      clear(nets, aps.sort((a, b) => b.rssi - a.rssi).map((ap) => h('button.chip', {
        onclick: () => { f.wifi_ssid.value = ap.ssid; f.wifi_pass.focus(); },
      }, ap.ssid || '(hidden)', h('small', `${ap.rssi} dBm${ap.secure ? ' · secure' : ' · open'}`))));
    } catch (e) { clear(nets, h('span.bad', e.message)); }
  }

  async function reindex() {
    try {
      toast('rebuilding the index… the panel holds its frame');
      const r = await app.src.reindex();
      toast(`index ${r.index}: ${r.entries} entries in ${r.index_ms} ms`, 'ok');
    } catch (e) { toast('reindex: ' + e.message, 'bad'); }
  }

  async function waitForBoot(msg) {
    for (let i = 0; i < 45; i++) {
      await new Promise((r) => setTimeout(r, 2000));
      try { const s = await app.refreshStatus(); msg(`back up: ${s.version} ${s.variant}`); return; } catch { msg(`rebooting… ${2 * (i + 1)} s`); }
    }
    msg('the device has not come back after 90 s — check its serial log');
  }

  async function reboot() {
    if (!confirm('Reboot the player?')) return;
    try { await app.src.reboot(); toast('rebooting…'); banner.hidden = true; waitForBoot((m) => toast(m)); } catch (e) { toast('reboot: ' + e.message, 'bad'); }
  }

  // --- playback cache ---------------------------------------------------
  // GET /api/cache is cheap to ask; the device counts in the background and
  // answers "counting" meanwhile, so poll until it is ready.
  let cachePoll = 0, visible = true;
  async function loadCache() {
    clearTimeout(cachePoll);
    try {
      const r = await app.src.cache();
      if (r.prepared == null) cacheLine.textContent = `${r.total.toLocaleString()} files · counting prepared files…`;
      else cacheLine.textContent = `${r.prepared.toLocaleString()} of ${r.total.toLocaleString()} files prepared` + (r.state === 'counting' ? ' · recounting…' : '');
      const a = r.auto, run = a?.state === 'running';
      autoBox.hidden = !a;
      if (a) { autoLine.textContent = autoText(a, r) + '. '; always.checked = a.mode === 'always'; }
      if ((run || r.state === 'counting') && visible) cachePoll = setTimeout(loadCache, run ? 4000 : 2000);
    } catch (e) { cacheLine.textContent = 'cache: ' + e.message; }
  }
  function autoText(a, r) {
    const n = (x) => x.toLocaleString(), e = a.eta_s, nt = sched.night();
    if (a.mode === 'off') return 'Automatic preparing is off';
    if (a.state === 'done') return a.failed ? `Automatic preparing is done · ${n(a.failed)} could not be prepared` : 'All files are prepared';
    const m = (t) => +t.slice(0, 2) * 60 + +t.slice(3), len = nt?.mode == 'off' && ((m(nt.to) - m(nt.from) + 1440) % 1440 || 1440) * 60;
    return (a.state !== 'running' ? (a.reason && a.reason !== 'playing' ? `Automatic preparing is waiting (${a.reason})` : 'Files are prepared automatically when the panel is idle')
      : a.policy === 'drip' ? 'Preparing files slowly while playing' : 'Preparing files automatically while the panel is idle')
      + (r.prepared != null ? ` · ${n(r.prepared)} of ${n(r.total)}` : '')
      + (e == null ? '' : len ? ` · about ${Math.ceil(e / len)} night${e > len ? 's' : ''} left` : ` · about ${e < 5400 ? mins(e) : Math.round(e / 3600) + ' hours'} of idle time left`);
  }
  function mins(s) { return s < 90 ? `${Math.max(1, Math.round(s))} s` : s < 5400 ? `${Math.round(s / 60)} min` : `${(s / 3600).toFixed(1)} h`; }
  function renderJob(j) {
    cacheBtn.textContent = j.running ? (j.stopping ? 'Stopping after this file…' : 'Stop') : 'Prepare remaining files';
    cacheBtn.disabled = j.stopping;
    const n = j.tried, dmg = j.damaged.length, net = j.net.length, num = (x) => x.toLocaleString();
    const files = (k, what) => `${num(k)} ${what}${k === 1 ? '' : 's'}`;
    cacheBar.style.width = j.todo ? `${(100 * n) / j.todo}%` : j.phase === 'finished' ? '100%' : '0';
    const left = prep.eta();
    const skipped = dmg ? ` · ${files(dmg, 'damaged file')} skipped (${dmg === 1 ? 'it' : 'they'} can't be played either)` : '';
    cacheMsg.textContent = j.phase === 'checking' ? 'Finding the files that are not prepared yet…'
      : j.phase === 'preparing' ? `${num(n)} of ${num(j.todo)} · ${num(j.done)} prepared` + (dmg ? ` · ${num(dmg)} damaged skipped` : '')
        + (net ? ` · ${num(net)} couldn't download` : '') + (left != null && n < j.todo ? ` · about ${mins(left)} left` : '') + (j.current ? ` · ${base(j.current)}` : '')
      : j.phase === 'finished' ? (j.todo ? files(j.done, 'file') + ' prepared' : dmg ? 'Every other file is already prepared' : 'Every file is already prepared') + skipped
      : j.phase === 'stopped' ? `Stopped after ${num(n)} of ${num(j.todo)}` + skipped + '. Start again to carry on' : '';
    // Once a run is over: what was skipped, and what could not be fetched.
    const list = (id, head, rows) => h('details', { id }, h('summary', head), h('div.log', rows.map((r) => h('div', r))));
    clear(cacheSum, !j.running && dmg > 0 && list('pc-damaged', 'Damaged files', j.damaged.map((d) => `${d.path} — ${d.reason}`)),
      !j.running && net > 0 && h('div', { id: 'pc-net' }, h('span.bad', `Couldn't download ${files(net, 'file')} — `),
        h('button.sm', { onclick: () => prep.start() }, 'try again'), list('pc-net-list', 'Which files', j.net)));
    clear(cacheErr, j.errors.map((e) => h('div', e)));
    if (!j.running && j.phase !== 'idle') loadCache();
  }
  on('prepare', renderJob);

  import('./power.js').then((m) => m.powerCard(power));
  fwLoad();
  renderFacts(app.status);
  time.render(app.status);
  renderJob(prep.job);
  loadCache();
  return { show() { visible = true; app.refreshStatus().catch(() => {}); loadPlaylists(); loadCache(); sched.load(); }, hide() { visible = false; clearTimeout(cachePoll); } };
}
