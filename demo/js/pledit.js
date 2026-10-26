// The playlist editor (#sets/edit; on a phone it replaces the list, on a
// laptop it is the right column), imported by sets.js the first time a
// playlist opens: the entries with their thumbnails (drag ⠿ or Alt+↑/↓,
// ✕, Arrange), **Add GIFs** — the card's folders as a tree (js/tree.js):
// tick GIFs or whole folders, or add everything a search shows; beside the
// entries on a laptop, in their place on a phone —, Add by filter (the
// core's Filter, a sheet), Save PUT /api/sets/<name>, play PUT /api/config.

import { app, playlistLabel } from './state.js';
import { Overrides, glyphs, menu, moreButton, actions, popup, closeMenu } from './overrides.js';
import { VList } from './vlist.js';
import { Tree, chipRow } from './tree.js';
import { attach, watch, thumbsOn } from './thumbs.js';
import { h, clear, on, store, toast, label, note, niceName, isSys, byNice, FLAGS } from './util.js';

// Not `filelist.*`: the firmware reads such a `set` as a root manifest.
const NAME_RE = /^(?!filelist\.)[A-Za-z0-9][A-Za-z0-9 _.-]{0,40}$/;
const picks = (box, k) => [...box.querySelectorAll('.on')].map((c) => c.dataset[k]);

// ctx: {el (the view), closeEd(), activate(p), loadSets(), known() (playlist paths), lists() (playlists)}
export function editor(se, ctx) {
  const { el } = ctx;
  const tog = (e) => { e.currentTarget.classList.toggle('on'); count(); };
  const bSeedL = h('label', { hidden: true }, 'Seed');
  const baseSel = h('select', { id: 'b-base', onchange: () => platChips() });
  const bPlats = h('div.chips', { id: 'b-plats' });
  const bPlat = h('input', { type: 'text', id: 'b-platforms', placeholder: 'MD,SNES', hidden: true });
  const bCats = h('div.chips', { id: 'b-cats' });
  const bTitle = h('input', { type: 'text', id: 'b-title', placeholder: 'mario, sonic' });
  const bFlags = h('div.chips', { id: 'b-flags' }, Object.entries(FLAGS).map(([k, n]) => h('button.chip', { 'data-flag': k, onclick: tog }, n)));
  const bShuffle = h('input', { type: 'checkbox', id: 'b-shuffle', onchange: () => { bSeedL.hidden = !bShuffle.checked; } });
  const bSeed = h('input', { type: 'number', id: 'b-seed', value: 1, min: 0, style: 'width:90px' });
  const bLimit = h('input', { type: 'number', id: 'b-limit', placeholder: 'all', min: 1, style: 'width:90px' });
  const bCount = h('span.cap.bad', { id: 'b-count' });
  const bAdd = h('button.pri', { id: 'b-append', onclick: () => build(true) }), bUse = h('button', { id: 'b-use', onclick: () => build(false) });
  const name = h('input', { type: 'text', id: 'set-name', placeholder: 'e.g. consoles', value: app.draft.name });
  const nameL = h('span'), nameRow = h('label.stack', { id: 'set-name-row', style: 'margin-bottom:12px' }, nameL, name);
  const title = h('h2.grow', { id: 'set-title' });
  const saveB = h('button.pri', { id: 'set-save', onclick: () => save(false) }), playB = app.device && h('button', { id: 'set-save-activate', onclick: () => save(true) });
  const empty = h('p.cap', { id: 'set-empty' }, 'Nothing in it yet. + Add GIFs picks them from your folders; Add by filter takes every GIF that matches');
  const eCount = h('span.muted', { id: 'set-count' });
  const eSearch = h('input', { type: 'search', id: 'set-search', placeholder: 'Filter GIFs', oninput: () => refresh() });
  let view = [], have = new Set();
  const picked = new Set();

  const ov = new Overrides();
  ov.onChange = () => list.refresh();
  const list = new VList({
    rowHeight: 40,
    render(row, i) {
      const p = app.draft.paths[i];
      const e = app.wasm.parsePath(p);
      const o = ov.get(p), cv = thumbsOn() && p[0] != '@' && h('canvas.th', { width: 128, height: 32 });
      clear(row, h('button.grip', { title: 'Drag to move (or Alt+↑/↓)', 'aria-label': 'Move', onpointerdown: (ev) => drag(ev, row, i) }, '⠿'),
        cv, h('span.t', label(e)), glyphs(o), e.platform && h('span.tag', e.platform),
        h('button.sm.rm', { title: 'Remove', 'aria-label': 'Remove', onclick: (ev) => { ev.stopPropagation(); remove(i); } }, '✕'),
        moreButton(ov, p, ['Remove from playlist', () => remove(i)]));
      if (cv) attach(cv, p);
      row.classList.toggle('cur', picked.has(i));
      row.classList.toggle('hid', !!o?.never);
      row.dataset.path = p;
    },
    onClick: (i, _k, e) => { if (e.target.closest('.grip')) return; if (!picked.delete(i)) picked.add(i); list.refresh(); },
    onMenu: (i, _k, x, y) => menu(ov, app.draft.paths[i], x, y),
  });
  list.el.id = 'set-entries';
  watch(list.el);
  list.el.addEventListener('keydown', (e) => {
    const row = e.target.closest('.vrow'), d = { ArrowUp: -1, ArrowDown: 1 }[e.key];
    if (!e.altKey || !d || !row) return;
    e.preventDefault();
    const k = row._i, t = k + d;
    if (t < 0 || t >= view.length) return;
    moveTo(view[k], view[t]);
    list.scrollTo(t);
    [...list.el.querySelectorAll('.vrow')].find((r) => r._i === t && !r.hidden)?.querySelector('.grip').focus();
  });
  const arrange = (b) => actions(b, 'set-arrange-menu', [
    ['set-shuffle', 'Shuffle', shuffle],
    ['set-sort', 'Sort A–Z', sortTitle],
    ['set-top', 'Picked to top', movePicked, '', !picked.size],
    ['set-rm-picked', 'Remove picked', removePicked, '.danger', !picked.size],
    ['set-clear', 'Clear', () => { if (confirm('Remove every GIF from this playlist?')) { app.draft.paths = []; picked.clear(); refresh(); } }, '.danger']]);

  // Add GIFs: the folders, ticks, a search (and its platforms).
  const ticked = new Set(), aPl = new Set();
  const addQ = h('input', { type: 'search', id: 'set-add-q', placeholder: 'Search GIFs', enterKeyHint: 'search', oninput: () => tree.filter(addQ.value, aPl) });
  const aChips = h('div.chips.scroll', { id: 'set-add-chips' });
  const addTicked = h('button.pri', { id: 'set-add-ticked', onclick: () => add([...ticked]) });
  const addAll = h('button', { id: 'set-add-all', onclick: () => add(tree.res.map((x) => x.path)) });
  const tree = new Tree({
    picked: ticked,
    ticking: () => true,
    rowHeight: 40,
    open: store.get('pl.open', []),
    onOpen: (o) => store.set('pl.open', o),
    live: async (cat) => (await import('./live.js'))[cat == '@' ? 'items' : 'effects'](h('div'), () => {}, () => tree.refresh()),
    mark: (it) => have.has(it.path),
    extra: (it) => it.e.platform && h('span.tag', it.e.platform),
    onTick: () => foot(),
    onError: (m) => toast(m, 'bad'),
    onUpdate: (st) => { chipRow(aChips, st.plats, aPl, 'data-platform', () => tree.filter(addQ.value, aPl)); foot(st); },
  });
  tree.el.id = 'set-add-list';
  let shown = 0;
  function foot(st) {
    if (st) shown = st.busy ? 0 : st.shown;
    addTicked.textContent = `Add ${ticked.size.toLocaleString()} ticked`;
    addTicked.disabled = !ticked.size;
    addAll.textContent = `Add all ${shown.toLocaleString()} shown`;
    addAll.disabled = !shown;
  }
  const pane = h('div', { id: 'set-add-pane' },
    h('div.row', { style: 'margin-bottom:8px' }, h('h3.pane-h.grow', 'Add GIFs'), h('button.sm', { id: 'set-add-done', onclick: () => adding(false) }, 'Done')),
    h('div', { style: 'margin-bottom:8px' }, addQ), aChips, tree.el,
    h('div.row', { id: 'set-add-foot' }, addTicked, addAll));
  let treeQ = null;
  function adding(v) {
    el.classList.toggle('adding', v);
    if (v && (!treeQ || tree.dirty)) treeQ = tree.load();
    if (v) tree.refresh();
  }
  function add(paths) {
    const n = paths.length, fresh = paths.filter((p) => !have.has(p) && have.add(p));
    app.draft.paths.push(...fresh);
    ticked.clear();
    foot();
    refresh();
    list.scrollTo(app.draft.paths.length - 1);
    toast(`${fresh.length.toLocaleString()} added${n > fresh.length ? ` · ${(n - fresh.length).toLocaleString()} already in it` : ''}`, 'ok');
  }

  const builder = h('div.menu.wide', { id: 'builder', role: 'dialog', 'aria-label': 'Add by filter' }, h('div.mh', h('b', 'Add GIFs by filter')),
    h('div.form',
      h('label.stack', 'Take GIFs from', baseSel),
      h('label.stack', 'Title has any of', bTitle)),
    h('div.lbl', 'Platforms'), bPlats, bPlat,
    h('div.lbl', 'Categories'), bCats,
    h('div.lbl', 'Leave out'), bFlags,
    h('div.row', { style: 'margin-top:16px' }, h('label', bShuffle, 'Shuffle'), bSeedL, h('label', 'At most', bLimit)), bCount,
    h('div.row', { style: 'margin-top:12px' }, bAdd, bUse));
  bSeedL.append(bSeed);
  function openBuilder(b) {
    initBuilder();
    const r = b.getBoundingClientRect();
    popup(builder, r.right - 440, r.bottom + 4);
  }

  se.append(h('div.card', { id: 'set-editor' },
    h('div.row', { style: 'margin-bottom:8px' }, h('button.sm.back', { id: 'set-back', onclick: ctx.closeEd }, '‹ Playlists'), title, eCount),
    nameRow,
    h('div.edpanes',
      h('div', { id: 'set-ent-pane' },
        h('div.row', { style: 'margin-bottom:8px' }, h('div.grow', { style: 'min-width:140px' }, eSearch),
          h('button', { id: 'set-add', onclick: () => adding(true) }, '+ Add GIFs'),
          h('button', { id: 'set-build', 'aria-haspopup': 'dialog', onclick: (e) => openBuilder(e.currentTarget) }, 'Add by filter…'),
          h('button', { id: 'set-arrange', 'aria-haspopup': 'menu', onclick: (e) => arrange(e.currentTarget) }, 'Arrange ▾')),
        note('Drag ⠿ to reorder; tap GIFs to pick them', 'Alt+↑/↓ moves the focused GIF; ✕ removes it; its … menu favourites, hides or plays it longer'),
        empty, list.el),
      pane),
    h('div.edfoot.row', saveB, playB)));

  // The header: the playlist's name; a built-in one is saved as a copy.
  function header() {
    const d = app.draft, root = d.path && !d.path.startsWith('/sets/');
    title.textContent = d.path ? playlistLabel(d.path) : 'New playlist';
    nameRow.hidden = d.path && !root;
    nameL.textContent = root ? 'Save a copy as' : 'Name';
    name.value = root ? (d.copyName ??= `${playlistLabel(d.path)} copy`) : d.name;
    saveB.textContent = root ? 'Save copy' : 'Save';
    if (playB) playB.textContent = root ? 'Save copy & play' : 'Save & play';
  }
  // A playlist (or a new one) is in app.draft: show it.
  function opened() {
    ov.load(app.draft.path);
    picked.clear();
    refresh();
  }

  // The builder reads the base playlist and /gifs only once it is first opened.
  let built = false;
  function initBuilder() {
    const keep = baseSel.value, known = ctx.known();
    clear(baseSel, ctx.lists().map((p) => h('option', { value: p.path }, p.label)));
    baseSel.value = keep && known.includes(keep) ? keep : '/filelist.txt';
    if (built) return;
    built = true;
    categories();
    platChips();
  }
  async function categories() {
    let dirs = [];
    try { dirs = (await app.src.list('/gifs')).filter((d) => d.dir).map((d) => d.name).sort(byNice); } catch { /* none */ }
    clear(bCats, dirs.map((d) => h('button.chip' + (isSys(d) ? '.sys' : ''), { 'data-cat': d, onclick: tog }, niceName(d))));
  }
  async function platChips() {
    const text = await app.text(baseSel.value).catch(() => null), n = new Map(), keep = picks(bPlats, 'platform');
    if (text) for (const e of app.wasm.parseManifest(text)) if (e.platform) n.set(e.platform, (n.get(e.platform) || 0) + 1);
    bPlat.hidden = !!n.size;
    clear(bPlats, [...n].sort((a, b) => b[1] - a[1]).map(([p, c]) => h('button.chip' + (keep.includes(p) ? '.on' : ''), { 'data-platform': p, onclick: tog }, p, h('small', c.toLocaleString()))));
    count();
  }
  const spec = () => ({
    platforms: bPlat.hidden ? picks(bPlats, 'platform') : bPlat.value, titles: bTitle.value, exclude_flags: picks(bFlags, 'flag'),
    categories: picks(bCats, 'cat'),
    shuffle: bShuffle.checked, seed: +bSeed.value || 0, ...(+bLimit.value > 0 ? { limit: +bLimit.value } : {}),
  });
  async function run() {
    const text = await app.text(baseSel.value);
    if (text == null) throw new Error(`${baseSel.value} is not served by this source`);
    return app.wasm.filterManifest(text, JSON.stringify(spec()));
  }
  async function count() {
    try {
      const n = (await run()).length, t = n.toLocaleString() + (n == 1 ? ' GIF' : ' GIFs');
      bAdd.textContent = 'Add ' + t;
      bUse.textContent = 'Replace with ' + t;
      bAdd.disabled = bUse.disabled = !n;
      bCount.textContent = '';
    } catch (e) { bCount.textContent = e.message; }
  }
  async function build(append) {
    try {
      const paths = await run();
      if (append) app.draft.paths.push(...paths);
      else app.draft.paths = paths; // the same playlist, new contents
      picked.clear();
      closeMenu();
      header();
      refresh();
      toast(`${paths.length.toLocaleString()} GIFs ${append ? 'added' : 'in the list'}`, 'ok');
    } catch (e) { toast(e.message, 'bad'); }
  }
  for (const i of [baseSel, bPlat, bTitle, bShuffle, bSeed, bLimit]) i.addEventListener('change', count);

  function refresh() {
    const q = app.wasm.normalizeTitle(eSearch.value);
    const all = app.draft.paths.map((_, i) => i);
    view = q ? all.filter((i) => app.wasm.normalizeTitle(app.wasm.parsePath(app.draft.paths[i]).title).includes(q)) : all;
    eCount.textContent = `${app.draft.paths.length.toLocaleString()} GIFs${q ? ` · ${view.length} shown` : ''}`;
    empty.hidden = !!app.draft.paths.length;
    list.el.hidden = !empty.hidden;
    list.set(view);
    have = new Set(app.draft.paths);
    if (el.classList.contains('adding')) tree.refresh();
  }
  function moveTo(i, j) {
    const p = app.draft.paths;
    p.splice(j, 0, ...p.splice(i, 1));
    picked.clear();
    refresh();
  }
  function drag(ev, row, i) {
    if (ev.button) return;
    const g = ev.currentTarget, y0 = ev.clientY, k0 = row._i, at = row.style.transform;
    let k = k0;
    g.setPointerCapture(ev.pointerId);
    row.classList.add('drag');
    g.onpointermove = (e) => {
      const dy = e.clientY - y0;
      k = Math.max(0, Math.min(view.length - 1, k0 + Math.round(dy / list.rh)));
      row.style.transform = `translateY(${k0 * list.rh + dy}px)`;
    };
    g.onpointerup = g.onpointercancel = () => {
      g.onpointermove = g.onpointerup = g.onpointercancel = null;
      row.classList.remove('drag');
      row.style.transform = at;
      if (k !== k0) moveTo(i, view[k]);
    };
  }
  function movePicked() {
    const p = app.draft.paths;
    const top = [...picked].sort((a, b) => a - b).map((i) => p[i]);
    app.draft.paths = [...top, ...p.filter((_, i) => !picked.has(i))];
    picked.clear();
    refresh();
  }
  function remove(i) { app.draft.paths.splice(i, 1); picked.clear(); refresh(); }
  function removePicked() { app.draft.paths = app.draft.paths.filter((_, i) => !picked.has(i)); picked.clear(); refresh(); }
  function shuffle() {
    app.draft.paths = app.wasm.filterManifest(app.draft.paths.join('\n'), JSON.stringify({ shuffle: true, seed: Date.now() % 1e9 }));
    refresh();
  }
  function sortTitle() {
    const key = new Map(app.draft.paths.map((p) => [p, app.wasm.normalizeTitle(label(app.wasm.parsePath(p)))]));
    app.draft.paths.sort((a, b) => key.get(a).localeCompare(key.get(b)));
    refresh();
  }

  async function save(andActivate) {
    const n = name.value.trim().replace(/\.txt$/, '');
    if (!n) { toast('give the playlist a name', 'bad'); name.focus(); return; }
    if (!NAME_RE.test(n)) { toast('playlist names are letters, digits, space, _ . - (no slashes, not filelist.*)', 'bad'); return; }
    if (!app.draft.paths.length) { toast('the playlist is empty', 'bad'); return; }
    const path = `/sets/${n}.txt`;
    if (ctx.known().includes(path) && app.draft.name !== n && !confirm(`A playlist named "${n}" exists. Replace it?`)) return;
    try {
      const text = app.draft.paths.join('\n') + '\n';
      await app.src.saveSet(n, text);
      app.draft.name = n;
      app.texts.set(path, text);
      if (app.draft.path !== path) { app.draft.path = path; ov.load(path); }
      header();
      toast(`Saved “${n}” · ${app.draft.paths.length.toLocaleString()} GIFs`, 'ok');
      if (andActivate && app.activeSet !== n) await ctx.activate({ value: n, label: n });
      await ctx.loadSets();
    } catch (e) { toast('save: ' + e.message, 'bad'); }
  }

  name.oninput = () => { const d = app.draft, v = name.value.trim(); if (!d.path) d.name = v; else if (!d.path.startsWith('/sets/')) d.copyName = v; };
  on('draft', () => { header(); refresh(); });
  if (app.draft.path) ov.load(app.draft.path);
  header();
  refresh();
  return {
    header, opened, refresh,
    show() { refresh(); if (ov.path) ov.load(ov.path); },
    // Leaving the editor: the Add pane closes with it.
    left() { adding(false); list.refresh(); },
    shown() { list.refresh(); },
  };
}
