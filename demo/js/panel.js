// Settings › Device › Panel timing (loaded with the Device tab; its own size
// line): /api/config `panel`, each change a trial (docs/firmware.md); and
// Moving pictures: `panel_sync`, `panel_split`, saved at once.

import { app } from './state.js';
import { h, on, note, saver, toast } from './util.js';

export function syncCard(el) {
  if (!('panel_sync' in app.config)) return;
  const hd = h('h3', 'Moving pictures'), run = saver(hd);
  const box = (id, key, text) => {
    const i = h('input', { type: 'checkbox', id, onchange: () => run(async () => { await app.src.putConfig({ [key]: i.checked }); await app.refreshConfig(); }) });
    return [i, h('label', { style: 'display:block' }, i, text)];
  };
  const [a, la] = box('ps-sync', 'panel_sync', 'Sync to the panel’s refresh (stops tearing)');
  const [b, lb] = box('ps-split', 'panel_split', 'Delay the bottom half one refresh (no step at mid-height on fast scrollers)');
  const sync = (c) => {
    if (!c || !('panel_sync' in c)) return;
    a.checked = c.panel_sync;
    b.checked = c.panel_split;
    b.disabled = !c.panel_sync;
    lb.classList.toggle('muted', !c.panel_sync);
  };
  el.append(hd, la, lb);
  el.hidden = false;
  on('config', sync);
  sync(app.config);
}

const D = { clk_phase: true, latch_blanking: 1, driver: 'shiftreg', i2s_speed: 8, min_refresh: 60 };
const n = (a, u) => a.map((v) => [v, v + u]);
const F = [
  ['clk_phase', 'Clock edge', [[true, 'Rising'], [false, 'Falling']]],
  ['latch_blanking', 'Latch blanking (clocks)', n([1, 2, 3, 4], '')],
  ['driver', 'Driver chip', [['shiftreg', 'Standard'], ...['fm6126a', 'fm6124', 'icn2038s', 'mbi5124', 'dp3246'].map((v) => [v, v.toUpperCase()])]],
  ['i2s_speed', 'Data clock', n([8, 16, 20], ' MHz')],
  ['min_refresh', 'Minimum refresh', n([30, 60, 90, 120], ' Hz')],
];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function panelCard(el) {
  const sel = {};
  const msg = h('span', { id: 'pt-msg' });
  const btn = (id, text, fn) => h('button.sm', { id, onclick: fn }, text);
  const keep = btn('pt-keep', 'Keep', () => send({ panel_confirm: true }));
  // A trial needing a restart (`panel_next`) is undone by one.
  const boot = () => left != null && !!app.config.panel_next;
  const undo = btn('pt-undo', 'Undo', () => { const b = busy = boot(); send({ panel_confirm: false }).then(() => b && back()); });
  const restart = btn('pt-restart', 'Restart now', () => app.src.reboot().then(back, (e) => toast('restart: ' + e.message, 'bad')));
  const bar = h('div.banner', { id: 'pt-bar', hidden: true, style: 'margin:12px 0 0' }, msg, ' ', keep, ' ', undo, restart);
  const tb = h('div.banner', { id: 'pt-box', hidden: true, style: 'margin:12px 0 0' });
  let left = null, tick = 0, busy = 0;

  async function send(patch) {
    try {
      await app.src.putConfig(patch);
      await app.refreshConfig();
    } catch (e) { busy || toast('panel timing: ' + e.message, 'bad'); app.refreshConfig().catch(() => {}); }
  }
  // Through a restart: until nothing waits for one, or a trial runs.
  async function back() {
    busy = 1;
    show();
    for (let i = 0; i < 45; i++) {
      await sleep(2000);
      try { const c = await app.refreshConfig(); if (!c.panel_next || c.panel_trial_s != null) break; } catch { /* restarting */ }
    }
    busy = 0;
    show();
  }
  function show() {
    clearTimeout(tick);
    const trial = left != null && !busy;
    bar.hidden = !busy && !trial && !app.config.panel_next;
    keep.hidden = undo.hidden = !trial;
    restart.hidden = trial || busy;
    msg.textContent = busy ? 'Restarting…' : trial ? `Trying these settings: they go back in ${left} s unless you keep them.`
      : 'The driver chip, data clock and refresh rate change after a restart.';
    if (!trial) return;
    tick = setTimeout(() => {
      if (--left > 0) return show();
      const b = boot();
      left = null;
      show();
      b ? back() : sleep(1500).then(() => app.refreshConfig()).catch(() => {});
    }, 1000);
  }
  function sync(c) {
    if (!c?.panel) return;
    left = c.panel_trial_s;
    const v = left != null ? c.panel_now : c.panel_next || c.panel_now;
    for (const [k] of F) {
      const s = sel[k], j = JSON.stringify(v[k]);
      if (![...s.options].some((o) => o.value === j)) s.append(h('option', { value: j }, j));
      s.value = j;
    }
    show();
  }
  const fields = F.map(([k, label, list]) => h('label.stack', label, sel[k] = h('select', {
    id: `pt-${k}`,
    onchange: () => send({ panel: { [k]: JSON.parse(sel[k].value) } }),
  }, list.map(([v, l]) => h('option', { value: JSON.stringify(v) }, v === D[k] ? `${l} (default)` : l)))));
  el.append(h('h3', 'Panel timing'),
    note('If some LEDs show wrong colours on certain pictures, press Test panel timing, or try these one at a time',
      'Each change is tried for a minute, then undone unless you press Keep (if the panel goes dark, wait). The driver chip, data clock and refresh rate are tried after a restart; after another driver chip, switch the player off and on if colours stay odd.'),
    h('div.form.f2', fields), bar, tb,
    h('div.row', { style: 'margin-top:12px' }, h('button.sm', { id: 'pt-test', hidden: app.src.kind !== 'device', onclick: () => import('./paneltest.js').then((m) => m.panelTest(tb)) }, 'Test panel timing'), btn('pt-reset', 'Reset to defaults', () => send({ panel: null }))));
  el.hidden = false;
  on('config', sync);
  sync(app.config);
}
