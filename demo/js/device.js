// Settings › Device (imported when the tab is first open; its own size
// line): the player's status, the playback cache, network (its own Save; a
// new name is live at once), reload and reboot, and About. Panel timing
// (js/panel.js), Firmware and Security (js/firmware.js) are modules of
// their own, loaded with it.

import { app, playlistLabel, setPath } from './state.js';
import * as prep from './prepare.js';
import { h, clear, on, toast, base, note, saver, bytes, uptime, SITE } from './util.js';

// The HTTP API this firmware speaks, on the site: a release's own page
// (dev/api/v<version>.html, frozen when it was published) when the player
// runs a release — its firmware version a plain release number and its web
// app that same release (a build from source reports `git describe`) —,
// else the latest (dev/api.html).
export const apiDoc = (s) => SITE + (/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(s?.version) && s.www_version === 'v' + s.version
  ? `dev/api/v${s.version}.html` : 'dev/api.html');

// ctx: {f, save, banner, sched, thumbs, live} from settings.js; `live()`
// while the tab is open and the view up.
export function deviceTab(el, { f, save, banner, sched, thumbs, live }) {
  const c = app.config, netH = h('h3', 'Network'), net = saver(netH);
  Object.assign(f, {
    wifi_ssid: h('input', { type: 'text', id: 'st-ssid', value: c.wifi_ssid, autocomplete: 'off' }),
    wifi_pass: h('input', { type: 'password', id: 'st-pass', placeholder: c.wifi_pass_set ? 'unchanged (one is saved)' : 'none saved', autocomplete: 'new-password' }),
    hostname: h('input', { type: 'text', id: 'st-hostname', value: c.hostname, maxLength: 32, oninput: (e) => { const t = e.target, i = t.selectionStart; t.value = t.value.toLowerCase(); t.setSelectionRange(i, i); }, pattern: '[a-z0-9]([a-z0-9\\-]*[a-z0-9])?' }),
  });
  const nets = h('div.chips', { id: 'st-nets' });
  // A name another display already answers to: said under the field when
  // it is left and before a save (GET /api/name-check; `taken` null —
  // couldn't check — says nothing), and the device's 409 if it slips by.
  const taken = h('p.cap.bad', { id: 'st-name-taken', hidden: true });
  const say = (t) => { taken.textContent = t || ''; taken.hidden = !t; };
  async function nameCheck() {
    const n = f.hostname.value.trim().toLowerCase();
    if (!n || n === app.config.hostname || !f.hostname.checkValidity()) return !say();
    const r = await app.src.json('GET', '/api/name-check?name=' + encodeURIComponent(n), undefined, 5000).catch(() => null);
    if (f.hostname.value.trim().toLowerCase() !== n) return true;
    say(r?.taken && `Another display on this network (${r.by}) is already called “${r.name}”. Pick a different name.`);
    return !r?.taken;
  }
  f.hostname.addEventListener('change', nameCheck);
  f.hostname.addEventListener('input', () => say());
  const netRun = (fn) => net(() => fn().catch((e) => {
    const m = e.status == 409 && /name taken: (.+?)\.local already answers from (\S+)/.exec(e.message);
    if (!m) throw e;
    say(`“${m[1]}” is taken by another display (${m[2]}); nothing was saved.`);
    throw Error('the name is taken');
  }));
  const moved = h('div.banner', { hidden: true, id: 'st-moved', style: 'margin:12px 0 0' });
  const facts = h('dl.facts', { id: 'st-facts' });
  // One quiet line, only when the card has had trouble (sdhealth on the device).
  const cardNote = h('p.cap', { id: 'st-card-note', hidden: true, style: 'margin-top:12px' });
  // Card speed (card_speed): Safe (20 MHz) by default; Fast (40 MHz) drops a
  // card with errors to Safe by itself, then one plain line and "Try Fast
  // again". `full` (never slows) is API-only: no option, its own caption.
  const csH = h('h3', 'Card speed'), csRun = saver(csH), csCap = h('p.cap', { id: 'st-cs-cap' });
  const csSel = h('select', { id: 'st-card-speed', onchange: () => csRun(async () => {
    const r = await app.src.putConfig({ card_speed: csSel.value });
    await app.refreshConfig(); csSay(); csDone(r.card_speed_applied);
  }) }, [['safe', 'Safe (recommended)'], ['fast', 'Fast']].map(([v, l]) => h('option', { value: v }, l)));
  const csSlow = h('p.cap', { id: 'st-card-slow', hidden: true }, 'This card had errors at Fast, so it runs at Safe now. Playback is unaffected. ',
    h('button.sm', { id: 'st-card-full', onclick: fullSpeed }, 'Try Fast again'));
  const csSay = () => {
    const v = csSel.value = app.config.card_speed || 'safe';
    csCap.className = 'cap' + (v == 'full' ? ' bad' : '');
    csCap.textContent = { safe: 'Plenty fast for every GIF, and safe on every card.',
      fast: 'Faster card reads. Some cards return wrong data at this speed without an error; use Safe if GIFs look damaged.',
      full: 'Full speed, set through the API: never slows down, even after errors.' }[v];
  };
  // "restart": the clock could not change in place, so the player restarts.
  const csDone = async (a) => a == 'restart' ? waitForBoot(toast) : renderFacts(await app.refreshStatus());
  async function fullSpeed() {
    try {
      const r = await app.src.json('POST', '/api/card/full-speed');
      toast(r.rebooting ? 'restarting…' : 'Fast again', 'ok'); csDone(r.applied);
    } catch (e) { toast('Fast: ' + e.message, 'bad'); }
  }
  // Where and what this player is: its address, its firmware and this page's version; the API reference
  // (on the site, see apiDoc) and the third-party licences (licenses.txt, from licenses/ by make web).
  const about = h('dl.facts', { id: 'st-about' });

  banner.append('Saved. WiFi changes take effect after a reboot. ', h('button.sm', { onclick: reboot }, 'Reboot now'));
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

  const fw = h('div.grp', { id: 'st-fw', hidden: true }), sec = h('div.grp', { id: 'st-sec', hidden: true }), pt = h('div.grp', { id: 'st-panel', hidden: true }), ps = h('div.grp', { id: 'st-sync', hidden: true });
  import('./firmware.js').then((m) => m.fwCards(fw, sec));
  import('./panel.js').then((m) => { m.syncCard(ps); m.panelCard(pt); });
  el.append(h('div.grp', { id: 'st-status' }, h('h3', 'Status'), facts, cardNote,
    h('div.row', { style: 'margin-top:12px' },
      h('button', { id: 'st-reindex', title: 'Read the playlist and the card again (after changing the card elsewhere)', onclick: reindex }, 'Reload playlist'),
      h('button', { id: 'st-reboot-btn', onclick: reboot }, 'Reboot'))),
    h('div.grp', { id: 'st-cs', hidden: c.card_speed == null }, csH, h('div.form.f2', h('label.stack', 'Speed', csSel)), csCap, csSlow),
    h('div.grp', { id: 'pc' }, pcH,
      note('Prepared copies play with less memory and start faster', 'Originals stay as they are.'),
      h('div.row.sp', h('span', cacheLine), cacheBtn),
      h('div.bar', { style: 'margin:12px 0' }, cacheBar), cacheMsg, cacheSum, cacheErr, autoBox, thumbs()),
    h('div.grp', { id: 'st-net' }, netH, note('WiFi changes need a reboot', 'With no saved network the player raises the open OpenDMD-Setup access point.'),
      h('div.form.f2',
        h('label.stack', 'WiFi network (SSID)', f.wifi_ssid),
        h('label.stack', 'WiFi password', f.wifi_pass),
        h('label.stack', 'Display name', f.hostname)),
      taken,
      h('p.cap', 'Give each display its own name: it becomes http://<name>.local/'),
      h('div.row', { style: 'margin-top:12px' }, h('button', { id: 'st-scan', onclick: scan }, 'Scan for networks'), nets),
      h('div.row', { style: 'margin-top:12px' }, h('button', { id: 'st-save-net', onclick: async () => f.hostname.reportValidity() && await nameCheck() && save(['wifi_ssid', 'wifi_pass', 'hostname'], netRun) }, 'Save network')), moved),
    ps, pt, fw, sec, h('div.grp', h('h3', 'About'), about));

  const late = (m, n) => m?.steady === undefined ? n.toLocaleString() : `${m.steady.toLocaleString()} during play · ${m.switch.toLocaleString()} at GIF changes`;
  function renderFacts(s) {
    if (!s) return;
    const fact = (k, v, bad) => [h('dt', k), h('dd' + (bad ? '.bad' : ''), v)], m = s.misses, sd = s.sd;
    clear(facts,
      fact('Playlist', `${playlistLabel(s.manifest || setPath(app.config?.set))} · ${s.entries.toLocaleString()} GIFs` + (s.index_state === 'building' ? ' · updating…' : '')),
      fact('Card', !sd.mounted ? 'Not mounted' : sd.formatting ? 'Formatting…' : sd.readable === false ? 'Not readable: needs FAT32'
        : bytes(sd.capacity_bytes) + (sd.clock_khz ? sd.clock_khz < 4e4 ? ' · Safe speed' : ' · Fast speed' : ''), sd.readable === false || !sd.mounted),
      fact('Running for', uptime(s.uptime_secs)),
      fact('Free memory', `${bytes(s.heap.free)} · lowest ${bytes(s.heap.floor)}`, s.heap.floor < 20000),
      [h('dt', 'Late frames'), h('dd', { id: 'st-late', title: m?.threshold_ms === undefined ? '' : `Frames ${m.threshold_ms} ms or more late. GIF changes: the new GIF's first frame, its transition and its first ${m.window_ms / 1000} s` }, late(m, s.deadline_misses))],
      fact('Late frames, any size', `${s.deadline_misses.toLocaleString()} · worst ${s.worst_lateness_ms} ms`),
      fact('Refresh', s.refresh_hz + ' Hz'),
      fact('GIFs started', s.gifs_started.toLocaleString()));
    const mm = s.mdns && s.mdns !== s.hostname;
    clear(about, fact('Address', [`${mm ? s.mdns : s.hostname}.local · ${s.ip}`, mm && h('div.bad', { id: 'st-mdns' }, `“${s.hostname}” is taken; this display is at ${s.mdns}.local. Give it its own name.`)]), fact('Firmware', `${s.version} (${s.variant})`), fact('Web app', s.www_version ?? '–'),
      fact('API', h('a', { id: 'st-api', href: apiDoc(s), target: '_blank', rel: 'noopener' }, 'API reference')),
      fact('Licences', h('a', { id: 'st-licences', href: './licenses.txt', target: '_blank', rel: 'noopener' }, 'Open-source licences')));
    const errs = sd.failures || (!sd.mounted && sd.reboots) || 0;
    cardNote.textContent = errs ? `Card errors: ${errs}, retrying` : '';
    cardNote.hidden = !errs;
    csSlow.hidden = !(sd.mounted && sd.fallback && app.config.card_speed == 'fast');
  }
  on('status', renderFacts);

  // After a network save: the password field emptied, the reboot banner, the new name.
  f.after = (r, patch, keys) => {
    f.wifi_pass.value = '';
    f.wifi_pass.placeholder = app.config.wifi_pass_set ? 'unchanged (one is saved)' : 'none saved';
    if (r.reboot_required) banner.hidden = false;
    if (keys.includes('hostname')) { moved.hidden = true; rn++; }
    if (patch.hostname) rename(app.config.hostname, rn);
  };

  // A new name is live at once, once the device's network worker gets to
  // it (it may be busy for half a minute). Say where the display is then:
  // the name mDNS answers to (`<name>-2` if another display has it), read
  // twice the same; from a .local address, offer to go there (the old one
  // stops resolving; an IP keeps working). No mDNS (setup network): no
  // address. A later Save network (`rn`) ends this one.
  let rn = 0;
  async function rename(name, g) {
    const re = RegExp(`^${name}(-\\d+)?$`);
    let s, m, last;
    clear(moved, 'Applying the new name…');
    moved.hidden = false;
    for (let i = 0; i < 30 && !(m && m === last); i++) {
      last = m;
      await new Promise((r) => setTimeout(r, 1000));
      if (g !== rn) return;
      s = await app.refreshStatus().catch(() => s);
      if (s?.hostname === name && !s.mdns) break;
      m = s?.hostname === name && re.test(s.mdns) && s.mdns;
    }
    if (g !== rn) return;
    const at = `${m}.local`, u = new URL(location);
    u.hostname = at;
    clear(moved, 'Saved. ', m ? [`This display is now at ${at}. `, /\.local$/.test(location.hostname) && h('a', { id: 'st-goto', href: u }, `Go to ${at}`)]
      : s?.mdns && `For now it still answers at ${s.mdns}.local.`);
  }

  async function scan() {
    clear(nets, h('span.muted', 'Scanning…'));
    try {
      const aps = await app.src.wifiScan();
      // "Enterprise" networks (a username as well as a password): no 802.1X in the firmware.
      const ent = (ap) => /-Ent$/.test(ap.auth);
      clear(nets, aps.sort((a, b) => b.rssi - a.rssi).map((ap) => h('button.chip', ent(ap)
        ? { disabled: true, title: 'Needs a work or school login: not supported' }
        : { onclick: () => { f.wifi_ssid.value = ap.ssid; f.wifi_pass.focus(); } },
      ap.ssid || '(hidden)', h('small', ent(ap) ? 'not supported' : `${ap.rssi} dBm${ap.secure ? ' · secure' : ' · open'}`))));
    } catch (e) { clear(nets, h('span.bad', e.message)); }
  }

  async function reindex() {
    try {
      toast('reading the playlist again… the panel holds its frame');
      const r = await app.src.reindex();
      toast(`Playlist ready: ${r.entries.toLocaleString()} GIFs`, 'ok');
    } catch (e) { toast('reload: ' + e.message, 'bad'); }
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
  let cachePoll = 0;
  async function loadCache() {
    clearTimeout(cachePoll);
    try {
      const r = await app.src.cache();
      if (r.prepared == null) cacheLine.textContent = `${r.total.toLocaleString()} files · counting prepared files…`;
      else cacheLine.textContent = `${r.prepared.toLocaleString()} of ${r.total.toLocaleString()} files prepared` + (r.state === 'counting' ? ' · recounting…' : '');
      const a = r.auto, run = a?.state === 'running';
      autoBox.hidden = !a;
      if (a) { autoLine.textContent = autoText(a, r) + '. '; always.checked = a.mode === 'always'; }
      if ((run || r.state === 'counting') && live()) cachePoll = setTimeout(loadCache, run ? 4000 : 2000);
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
  const show = () => { csSay(); renderFacts(app.status); renderJob(prep.job); loadCache(); };
  show();
  // Leaving the tab: no more cache polls, and a panel test in progress ends.
  return { show, hide() { clearTimeout(cachePoll); app.endPanelTest?.(); } };
}
