// The GIF tree: the card's folders (gifs/<Category>, then Live and Effects,
// then the system `_x` ones) opening into their GIFs, for the Library and
// the playlist editor's Add pane. One VList of folder rows (`.vdir`) and
// the open folders' GIF rows (`.vrow`), so only what is in view exists
// (Consoles' 3,781 rows are ~21 elements). A search filters every folder
// (the matches' folders open, each with its count); platform chips narrow
// it. Ticks (Select mode, the Add pane) work on GIFs and whole folders.
// Keys: ↑/↓, Home/End, PgUp/PgDn, → opens or steps in, ← closes or steps
// out, Enter, Space ticks. Each folder is listed once a session, shared by
// every tree. Imported by library.js and pledit.js; its own size line with
// thumbs.js and tree.css (loaded here).

import { app, listOr } from './state.js';
import { VList } from './vlist.js';
import { attach, watch, thumbsOn } from './thumbs.js';
import { h, clear, on, store, label, niceName, isSys, byNice } from './util.js';

await new Promise((res) => document.head.append(h('link', { rel: 'stylesheet', href: './tree.css', onload: res, onerror: res })));

const LIVE = { '@': 'Live', '@fx': 'Effects' };
const lists = new Map(), got = new Map(), trees = new Set();
// The last count seen per folder, shown (muted) until it is listed again.
const counts = store.get('lib.n', {});
const cmp = (a, b) => a.norm.localeCompare(b.norm) || (a.e.variant ?? 0) - (b.e.variant ?? 0);

// A folder's GIFs, sorted ({path, e, name, lname, norm}); null if it could not be read.
export function listCat(c) {
  const dir = `/gifs/${c}`;
  if (!lists.has(c)) lists.set(c, listOr(dir).then((fs) => {
    const l = fs.filter((f) => !f.dir && /\.gif$/i.test(f.name)).map(({ name }) => {
      const path = `${dir}/${name}`, e = app.wasm.parsePath(path);
      return { path, e, name, lname: name.toLowerCase(), norm: app.wasm.normalizeTitle(e.title) };
    }).sort(cmp);
    got.set(c, l);
    counts[c] = l.length;
    store.set('lib.n', counts);
    return l;
  }, () => { lists.delete(c); return null; }));
  return lists.get(c);
}
export function forget(c) {
  for (const k of c ? [c] : [...lists.keys()]) { lists.delete(k); got.delete(k); }
  for (const t of trees) t.dirty = true;
}
on('gifs', forget);
// A restart or a card out and in: list everything again.
let up = 0, sd;
on('status', (s) => {
  if (s.uptime_secs < up || (sd !== undefined && s.sd?.mounted !== sd)) forget();
  up = s.uptime_secs;
  sd = s.sd?.mounted;
});

// The chips of a facet, most first (those on, first): `counts` value -> n.
export function chipRow(el, n, on, key, apply, name = (v) => v) {
  for (const v of on) if (!n.has(v)) n.set(v, 0);
  const top = [...n].sort((a, b) => on.has(b[0]) - on.has(a[0]) || b[1] - a[1]).slice(0, 24);
  clear(el, top.map(([v, c]) => h('button.chip' + (on.has(v) ? '.on' : ''), {
    [key]: v, 'aria-pressed': String(on.has(v)), onclick: () => { if (!on.delete(v)) on.add(v); apply(); },
  }, name(v), h('small', c.toLocaleString()))));
  return top.length;
}

export class Tree {
  // o: {picked: Set of paths, ticking() -> bool, onPick(it), onMenu(it, x, y),
  // onHold(it), extra(it, row) -> nodes, mark(it) -> bool (✓ in the playlist),
  // live(cat) -> items (Live, Effects; none: no such folders), onUpdate(stats),
  // onOpen(open folders), open: [cats], rowHeight}
  constructor(o) {
    this.o = o;
    this.q = '';
    this.plats = new Set();
    this.open = new Set(o.open || []);
    this.shut = new Set(); // folders closed during this search
    this.folders = [];
    this.rows = [];
    this.res = [];
    this.liveGot = new Map();
    this.failed = new Set(); // folders whose listing failed: a row to retry
    this.cur = '';
    this.vl = new VList({
      rowHeight: o.rowHeight || 44,
      render: (r, it, i) => this.render(r, it, i),
      onClick: (it, i, e) => this.click(it, i, e),
      onMenu: o.onMenu && ((it, _i, x, y) => it.path && o.onMenu(it, x, y)),
      onHold: o.onHold && ((it) => it.path && o.onHold(it)),
    });
    const el = this.el = this.vl.el;
    el.setAttribute('role', 'tree');
    el.setAttribute('aria-label', 'GIFs by folder');
    el._tree = this; // (for the tests)
    el.addEventListener('keydown', (e) => this.key(e));
    watch(el);
    trees.add(this);
  }

  ticking() { return this.o.ticking?.() ?? false; }

  // The folders under /gifs (listed again each call), then the open ones' GIFs.
  async load() {
    this.dirty = false;
    let dirs = [];
    try { dirs = (await listOr('/gifs')).filter((d) => d.dir).map((d) => d.name).sort(byNice); } catch (e) { this.o.onError?.('listing the folders: ' + e.message); }
    const f = (cat) => this.folders.find((x) => x.cat === cat) || { dir: true, cat, name: LIVE[cat] || niceName(cat), live: !!LIVE[cat] };
    this.folders = [...dirs.filter((d) => !isSys(d)), ...(this.o.live ? ['@', '@fx'] : []), ...dirs.filter(isSys)].map(f);
    for (const c of this.open) this.fill(c);
    this.update();
    return this.folders;
  }
  real() { return this.folders.filter((f) => !f.live).map((f) => f.cat); }
  items(cat) { return LIVE[cat] ? this.liveGot.get(cat) : got.get(cat); }
  // Ask for a folder's GIFs; the tree updates when they come.
  fill(cat) {
    if (this.items(cat)) return Promise.resolve(this.items(cat));
    if (LIVE[cat]) return Promise.resolve(this.o.live?.(cat)).then((l) => { this.liveGot.set(cat, l || []); this.update(); return l; });
    this.failed.delete(cat);
    return listCat(cat).then((l) => { if (!l) this.failed.add(cat); this.update(); return l; });
  }

  // The search (title words) and the platforms on; update() after.
  filter(q, plats = this.plats) {
    if (q.trim() !== this.q.trim()) this.shut.clear();
    this.q = q;
    this.plats = plats;
    if (this.q.trim()) this.gather();
    this.update();
  }
  // A search is of every GIF: each folder listed (one at a time: the device
  // shares the card with the player), the tree updated as they come.
  async gather() {
    if (this.gathering) return;
    this.gathering = true;
    let bad = 0;
    for (const c of this.real()) if (!got.has(c)) { if (!await listCat(c)) { bad++; this.failed.add(c); } this.update(); }
    this.gathering = false;
    this.bad = bad;
    this.update();
  }

  match() {
    const q = app.wasm.normalizeTitle(this.q), qn = this.q.trim().toLowerCase(), ps = this.plats;
    return [!!qn, (it) => !qn || it.norm.includes(q) || (it.lname || '').includes(qn), (it) => !ps.size || ps.has(it.e.platform)];
  }
  // One pass: the rows; every match (`res`); the platforms' counts.
  update() {
    const [text, okQ, okP] = this.match(), rows = [], res = [], pc = new Map();
    let pool = 0, matched = 0;
    for (const f of this.folders) {
      if (text && f.live) continue; // a search is of GIFs
      const items = this.items(f.cat), open = text ? !this.shut.has(f.cat) : this.open.has(f.cat);
      let hits = null;
      if (items && (text || open)) {
        hits = [];
        for (const it of items) {
          pool++;
          if (!okQ(it)) continue;
          matched++;
          const p = it.e.platform;
          if (p) pc.set(p, (pc.get(p) || 0) + 1);
          if (okP(it)) hits.push(it);
        }
        // What is shown: a folder closed during a search is not.
        if (open) res.push(...hits);
      }
      if (text && !hits?.length) continue;
      f.open = open;
      f.n = hits && (text || this.plats.size) ? hits.length : items ? items.length : counts[f.cat];
      f.old = !items;
      f.hits = hits;
      rows.push(f);
      if (open) rows.push(...(hits || (items ? [] : [{ ld: !this.failed.has(f.cat), err: this.failed.has(f.cat), cat: f.cat }])));
    }
    // ARIA position in each level; the focus kept on the same row.
    this.pos = [];
    let fp = 0, nf = rows.filter((r) => !r.path && !r.ld && !r.err).length, k = 0, nk = 0;
    for (const r of rows) {
      if (r.path) this.pos.push([++k, nk]);
      else if (r.ld || r.err) this.pos.push([1, 1]);
      else { k = 0; nk = r.open && r.hits ? r.hits.length : 0; this.pos.push([++fp, nf]); }
    }
    const a = document.activeElement, key = this.el.contains(a) && a._i != null && (a.dataset.path || 'd' + a.dataset.cat);
    this.rows = rows;
    this.res = res;
    this.vl.set(rows);
    if (key) {
      const i = rows.findIndex((r) => (r.path || 'd' + r.cat) === key);
      this.focusRow(i >= 0 ? i : Math.min(a._i, rows.length - 1));
    }
    this.o.onUpdate?.({ shown: res.length, pool, matched, plats: pc, busy: (text && this.gathering) || rows.some((r) => r.ld), bad: this.bad, text });
  }

  render(r, it, i) {
    const tick = this.ticking(), picked = this.o.picked, [pos, size] = this.pos[i] || [1, 1];
    this.el.setAttribute('aria-multiselectable', String(tick));
    r.setAttribute('role', 'treeitem');
    r.setAttribute('aria-level', it.path ? 2 : 1);
    r.setAttribute('aria-posinset', pos);
    r.setAttribute('aria-setsize', size);
    if (!it.path) {
      r.className = 'vdir' + (it.open ? ' open' : '') + (isSys(it.cat) ? ' sys' : '') + (it.err ? ' err' : '');
      r.dataset.cat = it.cat;
      delete r.dataset.path;
      if (it.ld) { r.removeAttribute('aria-expanded'); clear(r, h('span.t.muted', 'Loading…')); return; }
      if (it.err) {
        r.removeAttribute('aria-expanded');
        clear(r, h('span.t.muted', 'Couldn’t read this folder'), h('button.sm', { onclick: (e) => { e.stopPropagation(); this.fill(it.cat); this.update(); } }, 'Try again'));
        return;
      }
      r.setAttribute('aria-expanded', String(it.open));
      const fl = tick && !it.live && (it.hits || this.items(it.cat)), all = fl && fl.length && fl.every((x) => picked.has(x.path));
      const box = tick && !it.live && h('input', { type: 'checkbox', checked: !!all, indeterminate: !all && fl && fl.some((x) => picked.has(x.path)), 'aria-label': 'Tick every GIF in ' + it.name, onclick: (e) => { e.stopPropagation(); this.tickFolder(it); } });
      clear(r, box, h('i.tw', it.open ? '▾' : '▸'), h('span.t', it.name), it.n != null && h('span.n' + (it.old ? '.muted' : ''), it.n.toLocaleString()));
      return;
    }
    r.className = 'vrow in';
    r.dataset.path = it.path;
    delete r.dataset.cat;
    r.removeAttribute('aria-expanded');
    if (tick) r.setAttribute('aria-selected', String(picked.has(it.path))); else r.removeAttribute('aria-selected');
    const box = tick && h('input', { type: 'checkbox', checked: picked.has(it.path), 'aria-label': label(it.e), onclick: (e) => { e.stopPropagation(); this.tick(it, e.target.checked); } });
    const cv = thumbsOn() && it.path[0] != '@' && h('canvas.th', { width: 128, height: 32 });
    clear(r, box, cv, h('span.t', label(it.e)), this.o.mark?.(it) && h('i.inpl', { title: 'In this playlist' }, '✓'), this.o.extra?.(it, r));
    if (cv) attach(cv, it.path);
    r.classList.toggle('cur', it.path === this.cur);
  }

  click(it, i, e) {
    if (e.target.closest('input,button')) return;
    if (!it.path) { if (!it.ld && !it.err) this.toggle(it.cat); return; }
    if (this.ticking()) this.tick(it, !this.o.picked.has(it.path));
    else this.o.onPick?.(it);
  }
  toggle(cat) {
    const s = this.q.trim() ? this.shut : this.open, v = this.q.trim() ? !s.has(cat) : s.has(cat);
    if (s.has(cat)) s.delete(cat); else s.add(cat);
    if (!this.q.trim()) { this.o.onOpen?.([...this.open]); if (!v) this.fill(cat); }
    this.update();
  }
  // One folder open, at the top; the others closed (a link to a folder, `reveal`).
  async only(cat) {
    this.open = new Set([cat]);
    this.o.onOpen?.([cat]);
    this.update();
    await this.fill(cat);
    this.el.scrollTop = this.rows.findIndex((r) => r.cat === cat && !r.path) * this.vl.rh;
  }
  closeAll() {
    if (this.q.trim()) for (const f of this.folders) this.shut.add(f.cat);
    else { this.open.clear(); this.o.onOpen?.([]); }
    this.update();
    this.el.scrollTop = 0;
  }
  // A GIF's row: its folder opened and listed again (it may be new), in view.
  async show(path) {
    const cat = path.slice(6, path.lastIndexOf('/'));
    forget(cat);
    if (!this.open.has(cat)) { this.open.add(cat); this.o.onOpen?.([...this.open]); }
    await this.fill(cat);
    const i = this.rows.findIndex((r) => r.path === path);
    if (i >= 0) this.el.scrollTop = Math.max(0, i - 2) * this.vl.rh;
    return this.rows[i];
  }

  tick(it, v) {
    if (v) this.o.picked.add(it.path); else this.o.picked.delete(it.path);
    this.o.onTick?.();
    this.vl.refresh();
  }
  async tickFolder(f) {
    const [, okQ, okP] = this.match(), l = (await this.fill(f.cat) || []).filter((x) => okQ(x) && okP(x)), p = this.o.picked;
    const all = l.every((x) => p.has(x.path));
    for (const x of l) if (all) p.delete(x.path); else p.add(x.path);
    this.o.onTick?.();
    this.vl.refresh();
  }

  key(e) {
    const row = e.target;
    if (row._i == null || e.altKey || e.ctrlKey || e.metaKey) return;
    const i = row._i, it = this.rows[i], page = Math.max(1, Math.floor(this.el.clientHeight / this.vl.rh) - 1);
    let t = null;
    switch (e.key) {
      case 'ArrowDown': t = i + 1; break;
      case 'ArrowUp': t = i - 1; break;
      case 'PageDown': t = i + page; break;
      case 'PageUp': t = i - page; break;
      case 'Home': t = 0; break;
      case 'End': t = this.rows.length - 1; break;
      case 'ArrowRight': if (!it.path && !it.ld && !it.err) { if (!it.open) this.toggle(it.cat); else t = i + 1; } break;
      case 'ArrowLeft':
        if (!it.path) { if (it.open) this.toggle(it.cat); } else { t = i; while (t > 0 && this.rows[t].path) t--; }
        break;
      case ' ': if (!this.ticking() || it.ld || it.err) return; if (it.path) this.tick(it, !this.o.picked.has(it.path)); else this.tickFolder(it); break;
      default: return;
    }
    e.preventDefault();
    if (t != null) this.focusRow(Math.max(0, Math.min(this.rows.length - 1, t)));
  }
  focusRow(i) {
    this.vl.scrollTo(i);
    this.vl.draw();
    [...this.el.querySelectorAll('.vrow,.vdir')].find((r) => r._i === i && !r.hidden)?.focus({ preventScroll: true });
  }
  refresh() { this.vl.refresh(); }
}
