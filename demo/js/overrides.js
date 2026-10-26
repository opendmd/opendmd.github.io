// Per-GIF overrides per playlist (favourite, play less, hide, play longer,
// loop N times): the row glyphs and the menu. On the card, a `.meta` file
// beside the playlist (wasm parseOverrides/renderOverrides; GET
// /api/overrides?set=); the page never says so.

import { app, playlistLabel, setValue } from './state.js';
import { h, toast, label, base } from './util.js';

const FAV = 3, LESS = 0.5;

// Favourite on/off (Library, the menu, Now's ★).
export const fav = (o = {}) => ({ ...o, w: o.w > 1 ? null : FAV, never: null });

export const fileFor = (p) => (p.endsWith('.txt') ? p.slice(0, -4) : p) + '.meta';

export class Overrides {
  constructor() {
    this.path = '';      // the playlist's card path
    this.map = new Map(); // gif path -> {path, never?, w?, dwell?, dwell_x?, loops?}
    this.saving = null;
    this.loading = null;
    this.ok = true;
    this.again = false;
    this.onChange = () => {};
  }

  get name() { return playlistLabel(this.path); }

  // The old map stays until the new list arrives; a change waits for it,
  // and is refused if the list could not be read (it would wipe the rest).
  async load(path) {
    this.path = path;
    const run = this.loading = (async () => {
      if (!path) return [];
      try {
        if (app.device) return (await app.src.overrides(setValue(path))).overrides;
        const text = await app.src.readText(fileFor(path));
        return text ? [...app.wasm.parseOverrides(text)] : [];
      } catch (e) { toast(`${this.name}: ${e.message}`, 'bad'); }
    })();
    const list = await run;
    if (this.loading === run && this.path === path) {
      this.ok = !!list;
      this.map = new Map(list?.map((o) => [o.path, o]));
      this.onChange();
    }
    return this;
  }

  get(p) { return this.map.get(p); }

  // Replace one GIF's override (an empty one removes it; `o` may be a
  // function of the current one) and save.
  async set(p, o) {
    await this.loading;
    if (!this.ok) { toast(`couldn’t read “${this.name}”: try again`, 'bad'); return this.load(this.path); }
    if (o.call) o = o(this.get(p) || {});
    const keep = Object.fromEntries(Object.entries(o).filter(([k, v]) => k !== 'path' && v != null && v !== false));
    if (Object.keys(keep).length) this.map.set(p, { path: p, ...keep });
    else this.map.delete(p);
    this.onChange();
    return this.save();
  }

  // One write at a time; changes made meanwhile go out in the next.
  async save() {
    if (this.saving) { this.again = true; return this.saving; }
    const file = fileFor(this.path);
    const text = app.wasm.renderOverrides([...this.map.values()]);
    this.saving = (async () => {
      try {
        if (text || !app.device) await app.src.writeFile(file, text);
        else await app.src.deleteFile(file);
        toast(`saved for “${this.name}”${app.device && app.activePath === this.path ? ' · the player is catching up (a few seconds)' : ''}`, 'ok');
      } catch (e) { toast(`saving for “${this.name}”: ${e.message}`, 'bad'); this.load(this.path); }
    })();
    await this.saving;
    this.saving = null;
    if (this.again) { this.again = false; return this.save(); }
  }
}

export function glyphs(o) {
  if (!o) return null;
  const g = [];
  if (o.never) g.push(['⊘', 'Hidden from this playlist', 'hid']);
  if (o.w > 1) g.push(['★', 'Favourite: plays more often', 'fav']);
  else if (o.w != null && o.w < 1) g.push(['▽', 'Plays less often', 'less']);
  if (o.dwell) g.push([`◔${o.dwell}s`, `Plays for ${o.dwell} seconds`, 'long']);
  else if (o.dwell_x) g.push([`◔${o.dwell_x}×`, `Plays ${o.dwell_x}× as long`, 'long']);
  if (o.loops) g.push([`↻${o.loops}`, `Loops ${o.loops} times`, 'loop']);
  return g.length ? h('span.ov', g.map(([t, title, c]) => h('i.' + c, { title }, t))) : null;
}

// The element that had the focus when a menu opened gets it back on close;
// `el.onclose` runs then.
let open = null, opener = null;
export function closeMenu() {
  if (!open) return;
  open.remove();
  open.onclose?.();
  open = null;
  if (opener?.isConnected) opener.focus({ preventScroll: true });
  opener = null;
}
addEventListener('keydown', (e) => { if (e.key === 'Escape') closeMenu(); });
addEventListener('pointerdown', (e) => { if (open && !open.contains(e.target)) closeMenu(); }, true);
// (Not a phone keyboard coming up: only a change of width.)
let wide = innerWidth;
addEventListener('resize', () => { if (wide != (wide = innerWidth)) closeMenu(); });

// The menu for one GIF, at (x, y) — a sheet along the bottom on a phone.
// `extra`: an optional [text, action] item at the end (Playlists' Remove);
// with no saved playlist, the only one.
export function menu(ov, path, x, y, extra) {
  closeMenu();
  if (!ov.path && !extra) { toast('save the playlist first', 'bad'); return; }
  const o = ov.get(path) || {};
  const put = (patch) => { closeMenu(); ov.set(path, (x) => ({ ...x, ...patch })); };
  const secs = h('input', { type: 'number', min: 1, max: 86400, placeholder: 's', id: 'ov-secs', value: o.dwell || '', style: 'width:72px' });
  const loops = h('input', { type: 'number', min: 1, max: 1000, placeholder: '×', id: 'ov-loops', value: o.loops || '', style: 'width:64px' });
  const item = (id, on, text, act) => h('button.mi' + (on ? '.on' : ''), { id, onclick: act }, h('span.ck', on ? '✓' : ''), text);
  const e = app.wasm.parsePath(path);
  const head = h('div.mh', h('b', label(e)), ov.path && h('span.muted', ` in “${ov.name}”`));
  // Play longer and Loop: their fields in a second step of the same menu.
  const longer = () => open.replaceChildren(head,
    item('ov-2x', o.dwell_x === 2, 'Twice as long', () => put({ dwell_x: o.dwell_x === 2 ? null : 2, dwell: null })),
    h('div.mr', h('span', 'Play for'), secs, h('span.muted', 'seconds'),
      h('button.sm', { id: 'ov-secs-set', onclick: () => { const v = +secs.value; if (v >= 1 && v <= 86400) put({ dwell: Math.round(v), dwell_x: null }); else toast('seconds: 1 to 86400', 'bad'); } }, 'Set')),
    h('div.mr', h('span', 'Loop'), loops, h('span.muted', 'times'),
      h('button.sm', { id: 'ov-loops-set', onclick: () => { const v = +loops.value; if (v >= 1 && v <= 1000) put({ loops: Math.round(v) }); else toast('loops: 1 to 1000', 'bad'); } }, 'Set')));
  open = h('div.menu', { id: 'ov-menu', role: 'menu' }, head,
    ov.path && [item('ov-fav', o.w > 1, '★ Favourite — plays more often', () => put(fav(o))),
    item('ov-less', o.w != null && o.w < 1, '▽ Play less often', () => put({ w: o.w != null && o.w < 1 ? null : LESS, never: null })),
    item('ov-hide', o.never, '⊘ Hide from this playlist', () => put({ never: o.never ? null : true, w: null })),
    item('ov-longer', o.dwell || o.dwell_x || o.loops, '◔ Play longer or loop…', longer),
    item('ov-reset', false, 'Reset', () => put({ never: null, w: null, dwell: null, dwell_x: null, loops: null }))],
    extra && item('ov-extra', false, extra[0], () => { closeMenu(); extra[1](); }));
  const r = open.querySelector('#ov-reset');
  if (r) r.disabled = !ov.get(path);
  popup(open, x, y);
}

// Show a `.menu` element at (x, y), or as a sheet along the bottom on a
// phone; one at a time, closed by Escape or a tap elsewhere.
export function popup(el, x, y) {
  if (open !== el) closeMenu();
  opener ??= document.activeElement;
  open = el;
  document.body.append(el);
  // Focus inside: the first control, or (a touch screen: no keyboard popping up) the sheet itself.
  el.tabIndex = -1;
  (matchMedia('(pointer:coarse)').matches ? el : el.querySelector('input,select,button:not(:disabled)') || el).focus({ preventScroll: true });
  if (innerWidth > 600) {
    const r = el.getBoundingClientRect();
    el.style.left = Math.max(8, Math.min(x, innerWidth - r.width - 8)) + 'px';
    el.style.top = Math.max(8, Math.min(y, innerHeight - r.height - 8)) + 'px';
  } else el.classList.add('sheet');
}

// A menu of actions under `btn`: items [id or attributes, text, act,
// '.cls', disabled] (falsy ones skipped).
export function actions(btn, id, items, head) {
  const r = btn.getBoundingClientRect();
  popup(h('div.menu', { id, role: 'menu' }, head && h('div.mh', head),
    items.filter(Boolean).map(([i, t, act, c = '', off]) => h('button.mi' + c, { ...(i?.at ? { id: i } : i), disabled: off, onclick: () => { closeMenu(); act(); } }, t))), r.right - 250, r.bottom + 4);
}

export function moreButton(ov, path, extra) {
  return h('button.sm.more', {
    title: 'Favourite, hide, play longer…', 'aria-label': `options for ${base(path)}`,
    onclick: (ev) => { ev.stopPropagation(); const r = ev.currentTarget.getBoundingClientRect(); menu(ov, path, r.left - 200, r.bottom + 4, extra); },
  }, '…');
}
