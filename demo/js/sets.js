// Playlists: the list (#sets), and the editor as a screen of its own
// (#sets/edit: on a phone it replaces the list, with a ‹ back; on a laptop
// the right column); Add by filter is a sheet over the editor. Save PUT
// /api/sets/<name>, play PUT /api/config {set}.

import { app, playlistLabel } from './state.js';
import { Overrides, glyphs, menu, moreButton, actions, popup, closeMenu } from './overrides.js';
import { VList } from './vlist.js';
import { h, clear, on, toast, label, lines, note, niceName, isSys, byNice, FLAGS } from './util.js';

// Not `filelist.*`: the firmware reads such a `set` as a root manifest.
const NAME_RE = /^(?!filelist\.)[A-Za-z0-9][A-Za-z0-9 _.-]{0,40}$/;
const picks = (box, k) => [...box.querySelectorAll('.on')].map((c) => c.dataset[k]);

export async function mount(el) {
  const setsList = h('div.list', { id: 'sets-list' });
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
  const empty = h('p.cap', { id: 'set-empty' }, 'Nothing in it yet. Add GIFs by filter, or pick them in the Library and choose Add to playlist');
  const eCount = h('span.muted', { id: 'set-count' });
  const eSearch = h('input', { type: 'search', id: 'set-search', placeholder: 'Filter GIFs' });
  let view = [];
  const picked = new Set();

  const ov = new Overrides();
  ov.onChange = () => list.refresh();
  const list = new VList({
    rowHeight: 40,
    render(row, i) {
      const p = app.draft.paths[i];
      const e = app.wasm.parsePath(p);
      const o = ov.get(p);
      clear(row, h('button.grip', { title: 'Drag to move (or Alt+↑/↓)', 'aria-label': 'Move', onpointerdown: (ev) => drag(ev, row, i) }, '⠿'),
        h('span.t', label(e)), glyphs(o), e.platform && h('span.tag', e.platform),
        moreButton(ov, p, ['Remove from playlist', () => remove(i)]));
      row.classList.toggle('cur', picked.has(i));
      row.classList.toggle('hid', !!o?.never);
      row.dataset.path = p;
    },
    onClick: (i, _k, e) => { if (e.target.closest('.grip')) return; if (!picked.delete(i)) picked.add(i); list.refresh(); },
    onMenu: (i, _k, x, y) => menu(ov, app.draft.paths[i], x, y),
  });
  list.el.id = 'set-entries';
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

  el.append(h('div.cols.sets', { id: 'sets' },
    h('div.sl', h('div.card', h('div.row.sp', { style: 'margin-bottom:8px' }, h('h2', { style: 'margin:0' }, 'Playlists'), h('button', { id: 'sets-new', onclick: newList }, '+ New playlist')), setsList,
      note('“All files” plays when no other playlist is chosen', 'Changing the playlist takes a few seconds; the panel holds its frame meanwhile.'))),
    h('div.se', h('p.muted.sehint', 'Open a playlist to see and change what is in it.'),
      h('div.card', { id: 'set-editor' },
        h('div.row', { style: 'margin-bottom:8px' }, h('button.sm.back', { id: 'set-back', onclick: closeEd }, '‹ Playlists'), title, eCount),
        nameRow,
        h('div.row', { style: 'margin-bottom:8px' }, h('div.grow', { style: 'min-width:140px' }, eSearch),
          h('button', { id: 'set-build', 'aria-haspopup': 'dialog', onclick: (e) => openBuilder(e.currentTarget) }, 'Add by filter…'),
          h('button', { id: 'set-arrange', 'aria-haspopup': 'menu', onclick: (e) => arrange(e.currentTarget) }, 'Arrange ▾')),
        note('Drag ⠿ to reorder; tap GIFs to pick them', 'Alt+↑/↓ moves the focused GIF; its … menu favourites, hides or plays it longer'),
        empty, list.el,
        h('div.edfoot.row', saveB, playB)))));

  // The editor's header: the playlist's name; a built-in one is saved as a copy.
  function header() {
    const d = app.draft, root = d.path && !d.path.startsWith('/sets/');
    title.textContent = d.path ? playlistLabel(d.path) : 'New playlist';
    nameRow.hidden = d.path && !root;
    nameL.textContent = root ? 'Save a copy as' : 'Name';
    name.value = root ? (d.copyName ??= `${playlistLabel(d.path)} copy`) : d.name;
    saveB.textContent = root ? 'Save copy' : 'Save';
    if (playB) playB.textContent = root ? 'Save copy & play' : 'Save & play';
  }
  // #sets/edit: pushed when opened from the list, so ‹ goes back.
  let pushed = false;
  function goEd() { header(); if (!el.classList.contains('ed')) { pushed = true; location.hash = '#sets/edit'; } }
  function closeEd() { if (pushed) history.back(); else location.replace('#sets'); }
  function newList() {
    app.draft = { name: '', paths: [], path: '' };
    ov.load('');
    picked.clear();
    refresh();
    goEd();
  }

  let known = [], ctok = 0;
  async function loadSets() {
    const lists = await app.playlists();
    known = lists.map((p) => p.path);
    const active = app.activeSet;
    const rows = lists.sort((a, b) => a.root - b.root || byNice(a.label, b.label)).map((p) => {
      const isActive = app.device && p.value === active, n = h('span.n');
      const acts = (app.device && !isActive) || !p.root;
      return [p, n, h('div' + (ed() && p.path === app.draft.path ? '.cur' : ''), { 'data-path': p.path, 'data-value': p.value },
        h('button.grow.pl' + (!p.root && isSys(p.value) ? '.sys' : ''), { onclick: () => open(p) },
          p.label, !p.value && h('span.tag', 'Default'), isActive && h('span.tag.act', 'Playing'), n),
        acts && h('button.more', { 'aria-label': 'More', 'aria-haspopup': 'menu', onclick: (e) => actions(e.currentTarget, 'sets-menu', [
          app.device && !isActive && [{ 'data-do': 'activate' }, 'Play', () => activate(p), '', app.building],
          !p.root && [{ 'data-do': 'delete' }, 'Delete…', () => del(p), '.danger']], p.label) }, '…'), !acts && h('i.more'))];
    });
    clear(setsList, rows.map((r) => r[2]));
    const keep = baseSel.value;
    clear(baseSel, lists.map((p) => h('option', { value: p.path }, p.label)));
    baseSel.value = keep && known.includes(keep) ? keep : '/filelist.txt';
    if (!keep && lists.length && built) platChips();
    counts(rows);
  }
  async function counts(rows) {
    // A device's root lists (~400 KB) are not fetched to be counted.
    for (const t = ++ctok; rows.length;) {
      const [p, n] = rows.shift();
      if (app.device && p.root && !app.texts.has(p.path)) {
        if (p.value === app.activeSet && app.status?.entries != null) n.textContent = app.status.entries.toLocaleString();
        continue;
      }
      const text = await app.text(p.path).catch(() => null);
      if (t !== ctok) return;
      if (text != null) n.textContent = lines(text).length.toLocaleString();
    }
  }

  async function open(p) {
    try {
      const text = await app.text(p.path, true);
      if (text == null) throw new Error('not served by this source');
      app.draft.paths = lines(text);
      app.draft.name = p.root ? '' : p.value;
      app.draft.path = p.path;
      delete app.draft.copyName;
      ov.load(p.path);
      picked.clear();
      refresh();
      for (const r of setsList.children) r.classList.toggle('cur', r.dataset.path === p.path);
      goEd();
    } catch (e) { toast(`${p.label}: ${e.message}`, 'bad'); }
  }

  async function activate(p) {
    if (app.building) { toast('the player is still reading a playlist; try again in a moment', 'bad'); return; }
    const set = p.value, n = p.label;
    try {
      toast(`switching to “${n}”…`);
      const r = await app.src.putConfig({ set });
      await app.refreshStatus();
      await app.refreshConfig();
      toast(`Playing “${n}” · ${(r.entries ?? app.status?.entries ?? 0).toLocaleString()} GIFs`, 'ok');
    } catch (e) { toast('play: ' + e.message, 'bad'); }
  }

  async function del(p) {
    const path = p.path, set = p.value;
    const isActive = app.device && app.activeSet === set;
    if (!confirm(isActive ? `"${p.label}" is playing. Switch the player back to All files and delete it?` : `Delete the playlist "${p.label}"?`)) return;
    try {
      if (isActive) { await app.src.putConfig({ set: '' }); await app.refreshStatus(); await app.refreshConfig(); }
      await app.src.deleteFile(path);
      app.texts.delete(path);
      toast(`deleted "${set}"`, 'ok');
      loadSets();
    } catch (e) { toast('delete: ' + e.message, 'bad'); }
  }

  // The builder reads the base playlist and /gifs only once it is first opened.
  let built = false;
  function initBuilder() {
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
    if (known.includes(path) && app.draft.name !== n && !confirm(`A playlist named "${n}" exists. Replace it?`)) return;
    try {
      const text = app.draft.paths.join('\n') + '\n';
      const r = await app.src.saveSet(n, text);
      app.draft.name = n;
      app.texts.set(path, text);
      if (app.draft.path !== path) { app.draft.path = path; ov.load(path); }
      header();
      toast(`Saved “${n}” · ${app.draft.paths.length.toLocaleString()} GIFs`, 'ok');
      if (andActivate && app.activeSet !== n) await activate({ value: n, label: n });
      await loadSets();
    } catch (e) { toast('save: ' + e.message, 'bad'); }
  }

  name.oninput = () => { const d = app.draft, v = name.value.trim(); if (!d.path) d.name = v; else if (!d.path.startsWith('/sets/')) d.copyName = v; };
  eSearch.oninput = refresh;
  on('draft', () => { header(); refresh(); });
  if (app.draft.path) ov.load(app.draft.path);
  on('config', () => loadSets());
  const ed = () => el.classList.contains('ed');

  await loadSets();
  refresh();
  return {
    show() { refresh(); if (ov.path) ov.load(ov.path); if (app.device) app.refreshStatus().catch(() => {}); },
    sub(sub) {
      el.classList.toggle('ed', sub == 'edit');
      if (!ed()) { pushed = false; closeMenu(); } else { header(); scrollTo(0, 0); }
      for (const r of setsList.children) r.classList.toggle('cur', ed() && r.dataset.path === app.draft.path);
      list.refresh();
    },
  };
}
