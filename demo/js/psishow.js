// Formulas' "Show on panel" (imported by js/formulas.js on a device): the
// formula becomes the panel's live item, `@fx:psi?src=…` (POST /api/play),
// until the playlist moves on; changes made meanwhile follow it there as
// live edits (`"live":true`: the device swaps the formula in place, or says
// 409 once it has moved on), ~200 ms apart, one request at a time, the
// latest only.

import { app } from './state.js';
import { toast } from './util.js';

// The device's `fx_psi::path`: comments out, lines joined with `; `, lightly
// encoded (a space is `+`; `% & + # ?`, controls and non-ASCII as %XX).
export function fpath(f) {
  let s = '', p = '@fx:psi?src=';
  for (const l of f.src.split('\n')) {
    const c = l.replace(/(--|\/\/).*/, '').trim().split(/\s+/).join(' ');
    if (c) s += (s ? '; ' : '') + c;
  }
  for (const b of new TextEncoder().encode(s)) {
    const c = String.fromCharCode(b);
    p += b == 32 ? '+' : b > 32 && b < 127 && !'%&+#?'.includes(c) ? c : '%' + (b < 16 ? '0' : '') + b.toString(16).toUpperCase();
  }
  for (const k of 'abc') if (f[k]) p += `&${k}=${+f[k].toFixed(4)}`;
  return p;
}

let shown = null, next = null, busy = false, last = 0, btns = [];
export function init(b) { btns = b; mark(); }
// Each button says whether its formula is the one on the panel.
export function mark() {
  for (const [b, f] of btns) {
    const on = !!shown && f() === shown;
    b.textContent = on ? 'On the panel ✓' : 'Show on panel';
    b.setAttribute('aria-pressed', on);
  }
}
// (Nothing to show before the editor's text has compiled once.)
export function show(f) { if (f?.src) { shown = f; next = [f, false]; pump(); mark(); } }
export function follow(f) { if (f === shown && !next) { next = [f, true]; pump(); } }
export function drop(f) { if (shown === f) { shown = null; mark(); } }

async function pump() {
  if (busy || !next) return;
  const wait = last + 200 - Date.now();
  if (wait > 0) { busy = true; setTimeout(() => { busy = false; pump(); }, wait); return; }
  const [f, live] = next, name = f.name || 'Formula';
  next = null; busy = true; last = Date.now();
  try {
    await app.src.json('POST', '/api/play', { path: fpath(f), live }, 8000);
    if (!live) toast(`On the panel: ${name}`, 'ok');
  } catch (e) {
    if (shown === f) { shown = null; if (next?.[0] === f) next = null; }
    const s = e.status;
    // A change after the playlist moved on: it just stops following.
    if (s != 409) toast(s == 415 ? 'Update OpenDMD to show formulas on the panel' : s == 400 ? `The panel can’t show “${name}”: ${e.message.replace(/^400: /, '')}`
      : s == 413 ? 'This formula is too long to show on the panel' : s == 503 ? 'The panel has nothing playing to interrupt' : s ? 'Show on panel: ' + e.message : 'Couldn’t reach the panel', 'bad');
  }
  busy = false;
  mark();
  pump();
}
