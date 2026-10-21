// Library: the GIFs on the player by category, and Details for the one
// picked (finding new art on the web is Create Art's, js/studio.js). See
// "The Library's layout" in CLAUDE.md.

import { app, playlistLabel } from './state.js';
import { Overrides, glyphs, menu, moreButton, actions } from './overrides.js';
import { PanelPreview } from './preview.js';
import { prepareBytes } from './prepare.js';
import { VList } from './vlist.js';
import { h, clear, emit, on, store, toast, label, lines, niceName, flagName, isFlag, isSys, byNice } from './util.js';

export const authorOf = (e) => e.flags.find((f) => !isFlag(f)) || '';

export async function addOne(category, name, bytes, warn, prep = prepareBytes) {
  const path = `/gifs/${category}/${name}`;
  await app.src.writeFile(path, bytes);
  try { await prep(path, bytes); } catch (e) {
    warn?.(`${name}: added, but it could not be prepared (${e.message || e}); it will play as it is`);
    try { await app.src.forget(path); } catch { /* nothing to forget */ }
  }
  emit('gifs', category);
  return path;
}

export async function savePlaylist(path, text) {
  if (path.startsWith('/sets/')) await app.src.saveSet(path.slice(6).replace(/\.txt$/, ''), text);
  else await app.src.writeFile(path, text);
  app.texts.set(path, text);
  if (path === app.activePath) await app.refreshStatus().catch(() => {});
}

export async function appendTo(path, paths) {
  const name = playlistLabel(path);
  try {
    const text = await app.text(path, true);
    const have = new Set(lines(text || ''));
    const add = paths.filter((p) => !have.has(p));
    if (!add.length) { toast(`already in “${name}”`); return; }
    await savePlaylist(path, (text && !text.endsWith('\n') ? text + '\n' : text || '') + add.join('\n') + '\n');
    toast(`${add.length} added to “${name}”`, 'ok');
  } catch (e) { toast(`adding to “${name}”: ${e.message}`, 'bad'); }
}

export async function offerPlaylist(paths) {
  const n = paths.length;
  if (confirm(`Add ${n} new GIF${n > 1 ? 's' : ''} to the playlist “${playlistLabel(app.activePath)}”?`)) await appendTo(app.activePath, paths);
}

// Thumbnails: first frames, two fetches at a time, rows gone skipped, 200 kept.
const TH = new Map(), thBusy = new Set();
let thQ = [];
export const thumbsOn = () => store.get('thumbs', !app.device);
function paint(cv) {
  const b = TH.get(cv._p);
  TH.delete(cv._p);
  TH.set(cv._p, b);
  if (b) {
    const s = Math.min(128 / b.width, 32 / b.height), w = b.width * s, y = b.height * s;
    cv.getContext('2d').drawImage(b, (128 - w) / 2, (32 - y) / 2, w, y);
  }
  cv.dataset.done = b ? 1 : 0;
}
// On a device only a prepared copy is a thumbnail, never the GIF itself.
function thumbCopy(p) {
  const c = app.wasm.cachePathFor(p);
  return c ? app.src.readBytes(c) : Promise.reject();
}
function thumb(cv, p) {
  cv._p = p;
  if (TH.has(p)) paint(cv); else { thQ.push(cv); pump(); }
}
function pump() {
  while (thBusy.size < 2 && thQ.length) {
    const cv = thQ.shift(), p = cv._p;
    if (!cv.isConnected || cv.parentNode.hidden || thBusy.has(p)) continue;
    if (TH.has(p)) { paint(cv); continue; }
    thBusy.add(p);
    (app.device ? thumbCopy(p) : app.previewBytes(p)).then((b) => createImageBitmap(new Blob([b]))).catch(() => null).then((b) => {
      TH.set(p, b);
      if (TH.size > 200) { const [k, v] = TH.entries().next().value; TH.delete(k); v?.close(); }
      thBusy.delete(p);
      for (const c of document.querySelectorAll('canvas.th')) if (c._p === p && !c.dataset.done) paint(c);
      thQ = thQ.filter((c) => c.isConnected && !c.dataset.done);
      pump();
    });
  }
}

export async function mount(el) {
  let cat = store.get('lib.cat', 'Consoles');
  let cats = [];
  let items = [], shown = [], current = null, thumbs = thumbsOn();
  const platforms = new Set();

  const search = h('input', { type: 'search', id: 'lib-search', placeholder: 'Search GIFs', enterKeyHint: 'search', oninput: () => apply() });
  const count = h('span.muted', { id: 'lib-count' });
  const tabs = h('div.tabs', { id: 'lib-cats', role: 'tablist' });
  const chips = h('div.chips.scroll', { id: 'lib-chips' });
  const addInput = h('input', { type: 'file', multiple: true, accept: '.gif,image/gif', hidden: true, id: 'lib-add-files', onchange: () => { addGifs([...addInput.files]); addInput.value = ''; } });
  const addBtn = app.device && h('button', { id: 'lib-add-btn', onclick: () => addInput.click() }, '+ Add GIFs');
  // Nothing on the player matches: the same words in Create Art (its Game
  // art on the one platform chip picked, else GifCities).
  const webQ = h('b');
  const webHint = h('div.lib-web', { id: 'lib-web', hidden: true }, 'Nothing here matches “', webQ, '”. ',
    h('button.txt', { id: 'lib-web-go', onclick: () => { app.artFind = { q: search.value.trim(), platform: platforms.size === 1 ? [...platforms][0] : '' }; app.go('create'); } }, 'Search the web in Create Art →'));
  const addBar = h('i');
  const addMsg = h('span', { id: 'lib-add-progress' });
  const addErr = h('div.log.bad', { id: 'lib-add-errors' });
  const addCard = h('div.lib-progress', { id: 'lib-add', hidden: true }, h('div.bar', addBar), h('div.cap', addMsg), addErr);

  const selCount = h('b');
  const selbar = h('div.selbar.row.sp', { hidden: true, id: 'lib-selbar' },
    h('span', selCount, ' selected'),
    h('div.row',
      h('button.txt', { id: 'lib-all', onclick: selectAll }, 'Select shown'),
      h('button.txt', { onclick: () => { app.selection.clear(); sync(); } }, 'Clear'),
      h('button', { id: 'lib-to-set', onclick: toSet }, 'Add to playlist')));

  const ov = new Overrides();
  ov.onChange = () => list.refresh();

  const list = new VList({
    rowHeight: 44,
    render(row, it) {
      const box = h('input', { type: 'checkbox', checked: app.selection.has(it.path), 'aria-label': 'select', onclick: (e) => { e.stopPropagation(); toggle(it.path, box.checked); } });
      const o = ov.get(it.path), cv = thumbs && it.path[0] != '@' && h('canvas.th', { width: 128, height: 32 });
      clear(row, box, cv, h('span.t', label(it.e)), glyphs(o), it.e.platform && h('span.tag', it.e.platform), moreButton(ov, it.path));
      if (cv) thumb(cv, it.path);
      row.classList.toggle('cur', it.path === current?.path);
      row.classList.toggle('hid', !!o?.never);
      row.dataset.path = it.path;
    },
    onClick: (it) => preview(it),
    onMenu: (it, _i, x, y) => menu(ov, it.path, x, y),
  });
  list.el.id = 'lib-list';

  const canvas = h('canvas.panel', { id: 'lib-panel' });
  const cap = h('div.cap', { id: 'lib-cap' }, 'Pick a GIF to see it here');
  const dTitle = h('h2', { id: 'lib-d-title' });
  const meta = h('div.cap', { id: 'lib-d-meta' });
  const sendBtn = app.device && h('button.pri', { id: 'lib-play', disabled: true, onclick: () => app.play(current.path) }, 'Send to panel');
  const plBtn = h('button', { id: 'lib-d-add', onclick: (e) => pickPlaylist(e.currentTarget) }, 'Add to playlist…');
  const editArt = h('button', { id: 'lib-d-art', onclick: () => { app.artFrom = { path: current.path, e: current.e }; app.go('create'); } }, '✎ Edit in Create Art');
  const moreBtn = h('button.more', { id: 'lib-d-more', 'aria-label': 'More', 'aria-haspopup': 'menu', onclick: (e) => detailMenu(e.currentTarget) }, '…');
  const smoothBox = app.device && h('div', { id: 'lib-smooth', hidden: true });
  const liveBox = h('div', { id: 'lib-live', hidden: true });
  const dPath = h('span.mono');
  const dFile = h('div.file.muted', { id: 'lib-d-file' }, 'File ', dPath);
  const gifOnly = [meta, editArt, moreBtn, dFile];
  const body = h('div', { id: 'lib-d-body', hidden: true },
    h('div.dhead', h('div.grow', dTitle, meta), moreBtn),
    h('div.row.acts', sendBtn, plBtn),
    h('div.row.acts', editArt), smoothBox,
    dFile);
  const side = h('div.side', { id: 'lib-side' },
    h('div.card', { id: 'lib-detail' },
      h('button.sm.close', { id: 'lib-d-close', 'aria-label': 'Close', onclick: () => side.classList.remove('open') }, '✕'),
      canvas, cap, body, liveBox));

  // On a phone the tabs and chips scroll sideways: fade an edge while more is off that side.
  const r2 = h('div.lib-r2', { id: 'lib-r2' }, tabs, h('i.vsep'), chips);
  const edge = () => {
    r2.classList.toggle('more', r2.scrollLeft + r2.clientWidth < r2.scrollWidth - 2);
    r2.classList.toggle('less', r2.scrollLeft > 2);
  };
  r2.addEventListener('scroll', edge, { passive: true });
  const ro = new ResizeObserver(edge);
  for (const x of [r2, tabs, chips]) ro.observe(x);

  const grid = h('div.libgrid',
    h('div',
      h('div.card.libhead',
        h('div.lib-q', search), count, addBtn, addInput,
        r2, addCard),
      webHint, list.el, selbar),
    side);
  el.append(grid);

  const pv = new PanelPreview(canvas, { onError: (m) => { cap.textContent = 'Preview: ' + m; } });

  async function preview(it) {
    current = it;
    it.pick?.();
    list.refresh();
    side.classList.add('open');
    body.hidden = false;
    const e = it.e, gif = it.path[0] != '@';
    dTitle.textContent = label(e);
    for (const x of gifOnly) x.hidden = !gif;
    const a = authorOf(e), sp = (id, t) => t && h('span', { id }, t);
    clear(meta, [sp('lib-d-platform', e.platform), sp('lib-d-category', e.category && niceName(e.category)),
      a && ['by ', sp('lib-d-author', a)], ...e.flags.filter(isFlag).map(flagName)].filter(Boolean).flatMap((x, i) => (i ? [' · ', x] : [x])));
    dPath.textContent = it.path;
    if (sendBtn) { sendBtn.disabled = false; sendBtn.dataset.path = it.path; }
    cap.textContent = 'Loading…';
    delete cap.dataset.shown;
    if (smoothBox) Object.assign(smoothBox, { hidden: true }).dataset.state = 'analysing';
    let bytes;
    try {
      if (await pv.show(it.path, async (p) => (bytes = await app.previewBytes(p)))) {
        if (it.pick) pv.syncTo(0, performance.now());
        cap.textContent = '';
        cap.dataset.shown = it.name;
        if (smoothBox && gif) import('./smooth.js').then((m) => m.detail(smoothBox, it, bytes, pv, cap)).catch(() => {});
      }
    } catch (err) { cap.textContent = 'Preview: ' + err.message; }
  }

  function detailMenu(btn) {
    const o = ov.get(current.path) || {};
    actions(btn, 'lib-d-menu', [
      ['lib-d-fav', (o.w > 1 ? '★' : '☆') + ' Favourite in…', () => inList(btn, (x) => ({ w: x.w > 1 ? null : 3, never: null }), (x) => x.w > 1)],
      ['lib-d-hide', '⊘ Hide in…', () => inList(btn, (x) => ({ never: x.never ? null : true, w: null }), (x) => x.never)],
      app.device && ['lib-d-edit', 'Edit details…', edit],
      app.device && ['lib-d-del', 'Delete…', remove, '.danger']], label(current.e));
  }
  async function inList(btn, patch, is) {
    const it = current, lists = await app.playlists().catch(() => []);
    if (!lists.length) lists.push({ path: '/filelist.txt', label: 'All files' });
    const o = ov.get(it.path) || {};
    actions(btn, 'lib-in-menu', lists.map((p) => [{ 'data-path': p.path }, [h('span.ck', p.path === ov.path && is(o) ? '✓' : ''), p.label], async () => {
      if (p.path !== ov.path) { await ov.load(p.path); store.set('lib.ov', p.path); }
      const x = ov.get(it.path) || {};
      ov.set(it.path, { ...x, ...patch(x) });
    }]), 'In which playlist?');
  }

  async function pickPlaylist(btn) {
    const it = current, lists = await app.playlists().catch(() => []);
    actions(btn, 'lib-pl-menu', [...lists.map((p) => [{ 'data-path': p.path }, p.label, () => appendTo(p.path, [it.path])]),
      ['lib-pl-new', 'New playlist…', () => { app.selection.add(it.path); toSet(); }]], 'Add to');
  }

  function toggle(path, onOff) {
    if (onOff) app.selection.add(path); else app.selection.delete(path);
    sync();
  }
  function selectAll() { for (const it of shown) app.selection.add(it.path); sync(); }
  function sync() {
    selCount.textContent = app.selection.size;
    selbar.hidden = !app.selection.size;
    list.refresh();
  }
  function toSet() {
    const have = new Set(app.draft.paths);
    for (const p of app.selection) if (!have.has(p)) app.draft.paths.push(p);
    toast(`${app.selection.size} entries added to the playlist being edited`, 'ok');
    emit('draft');
    app.go('sets');
  }

  const real = () => cats.filter(([v]) => v[0] != '@').map(([v]) => v);
  async function categories() {
    let dirs = [];
    try { dirs = (await app.src.list('/gifs')).filter((d) => d.dir).map((d) => d.name).sort(byNice); } catch (e) { toast('listing /gifs: ' + e.message, 'bad'); }
    const tab = (d) => [d, niceName(d)];
    cats = [...dirs.filter((d) => !isSys(d)).map(tab), ['@', 'Live'], ['@fx', 'Effects'], ...dirs.filter(isSys).map(tab)];
    if (!cats.some(([v]) => v === cat)) cat = dirs.includes('Consoles') ? 'Consoles' : cats[0][0];
    renderTabs();
    await load();
  }
  function renderTabs() {
    clear(tabs, cats.map(([v, t]) => h('button.tab' + (v === cat ? '.on' : '') + (isSys(v) ? '.sys' : ''), {
      role: 'tab', 'data-cat': v, 'aria-selected': String(v === cat),
      onclick: () => { if (v !== cat) { cat = v; renderTabs(); load(); } },
    }, t)));
    const on = tabs.querySelector('.on');
    if (on) for (const s of [tabs, tabs.parentNode]) s.scrollLeft = on.offsetLeft - s.offsetLeft - 24;
  }

  async function load() {
    store.set('lib.cat', cat);
    if (!(liveBox.hidden = cat[0] != '@')) {
      items = (await import('./live.js'))[cat == '@' ? 'items' : 'effects'](liveBox, preview, () => list.refresh());
      platforms.clear();
      renderChips();
      return apply();
    }
    count.textContent = 'Loading…';
    const want = cat, dir = `/gifs/${cat}`;
    let files = [];
    try { files = (await app.src.list(dir)).filter((f) => !f.dir).map((f) => f.name); } catch (e) { toast(`listing ${dir}: ${e.message}`, 'bad'); }
    if (want != cat) return;
    items = files.filter((n) => /\.gif$/i.test(n)).map((name) => {
      const path = `${dir}/${name}`;
      const e = app.wasm.parsePath(path);
      return { path, e, name, norm: app.wasm.normalizeTitle(e.title) };
    }).sort((a, b) => a.norm.localeCompare(b.norm) || (a.e.variant ?? 0) - (b.e.variant ?? 0));
    platforms.clear();
    renderChips();
    apply();
  }

  function renderChips() {
    const counts = new Map();
    for (const it of items) if (it.e.platform) counts.set(it.e.platform, (counts.get(it.e.platform) || 0) + 1);
    const top = [...counts].sort((a, b) => b[1] - a[1]).slice(0, 24);
    grid.classList.toggle('noplat', top.length < 2);
    clear(chips, top.map(([p, n]) => h('button.chip' + (platforms.has(p) ? '.on' : ''), {
      'data-platform': p,
      onclick: () => { if (!platforms.delete(p)) platforms.add(p); renderChips(); apply(); },
    }, p, h('small', n))));
  }

  function apply() {
    const text = search.value;
    const q = app.wasm.normalizeTitle(text);
    const qn = text.trim().toLowerCase();
    shown = items.filter((it) => (!platforms.size || platforms.has(it.e.platform))
      && (!q || it.norm.includes(q) || (qn && it.name.toLowerCase().includes(qn))));
    count.textContent = `${shown.length.toLocaleString()} of ${items.length.toLocaleString()}`;
    list.set(shown);
    webQ.textContent = text.trim();
    webHint.hidden = shown.length || !text.trim() || cat[0] == '@';
    list.el.hidden = !webHint.hidden; // no empty box under the hint
  }

  async function reveal(path) {
    const c = path.slice(6, path.lastIndexOf('/'));
    search.value = '';
    if (cats.some(([v]) => v === c)) cat = c;
    await categories();
    if (c !== cat && cats.some(([v]) => v === c)) { cat = c; renderTabs(); await load(); }
    const it = items.find((x) => x.path === path);
    if (it) { list.el.scrollTop = shown.indexOf(it) * list.rh; preview(it); }
  }

  const opts = () => ({ category: cat[0] == '@' ? '' : cat, categories: real(), platforms: [...new Set(items.map((x) => x.e.platform).filter(Boolean))] });
  async function addGifs(picked) {
    const gifs = picked.filter((f) => /\.gif$/i.test(f.name));
    if (!gifs.length) { toast('choose .gif files', 'bad'); return; }
    const plan = await (await import('./details.js')).addDialog(gifs, opts());
    if (!plan) return;
    addCard.hidden = false;
    addErr.replaceChildren();
    const added = [];
    const warn = (m) => addErr.append(h('div', m));
    for (let i = 0; i < plan.length; i++) {
      const { file, category, name } = plan[i];
      addMsg.textContent = `Adding ${i + 1} of ${plan.length} · ${name}`;
      addBar.style.width = `${(100 * i) / plan.length}%`;
      try { added.push(await addOne(category, name, new Uint8Array(await file.arrayBuffer()), warn)); } catch (e) { warn(`${name}: ${e.message || e}`); }
    }
    addBar.style.width = '100%';
    const where = [...new Set(plan.map((p) => niceName(p.category)))].join(', ');
    addMsg.textContent = `${added.length} of ${plan.length} added to ${where}`;
    if (!addErr.firstChild) setTimeout(() => { addCard.hidden = true; }, 6000);
    if (added.length) {
      await reveal(added[0]);
      await offerPlaylist(added);
    }
  }

  async function edit() {
    const to = await (await import('./details.js')).editDialog(current.path, opts());
    if (to) { await ov.load(ov.path); await reveal(to); }
  }
  async function remove() {
    if (!(await (await import('./details.js')).removeGif(current.path, label(current.e)))) return;
    current = null;
    body.hidden = true;
    side.classList.remove('open');
    pv.clear();
    cap.textContent = 'Pick a GIF to see it here';
    await ov.load(ov.path);
    await categories();
  }

  async function playlists() {
    const lists = await app.playlists().catch(() => []);
    const has = (p) => p && lists.some((x) => x.path === p);
    const want = [ov.path, store.get('lib.ov'), app.activePath].find(has) || lists[0]?.path || '/filelist.txt';
    if (want !== ov.path) await ov.load(want);
  }

  let dirty = false;
  on('gifs', () => { dirty = true; });
  await categories();
  await playlists();
  sync();
  return {
    show() {
      thumbs = thumbsOn();
      sync();
      playlists();
      if (dirty) { dirty = false; const was = current; categories().then(() => { const it = was && current === was && items.find((x) => x.path === was.path); if (it) preview(it); }); }
    },
  };
}
