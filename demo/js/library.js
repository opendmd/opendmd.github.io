// Library: the GIFs on the player as a tree of folders (js/tree.js), and
// Details for the one picked (finding new art on the web is Create Art's,
// js/studio.js). See "The Library's layout" in CLAUDE.md.

import { app, playlistLabel } from './state.js';
import { Overrides, glyphs, menu, moreButton, actions, fav } from './overrides.js';
import { PanelPreview } from './preview.js';
import { Tree, chipRow, forget as forgetDir } from './tree.js';
import { forget as forgetThumb, check as checkThumb } from './thumbs.js';
import { authorOf, addOne, appendTo, offerPlaylist } from './gifs.js';
import { h, clear, emit, store, toast, label, niceName, flagName, isFlag } from './util.js';

export async function mount(el) {
  let current = null, selecting = false;
  const plats = new Set();

  const search = h('input', { type: 'search', id: 'lib-search', placeholder: 'Search GIFs', enterKeyHint: 'search', oninput: () => apply() });
  const count = h('span.muted', { id: 'lib-count' });
  const fold = h('button.sm', { id: 'lib-fold', title: 'Close every folder', 'aria-label': 'Close every folder', onclick: () => tree.closeAll() }, '⊟');
  // Checkboxes only in Select mode (the button, or a long press on a row).
  const selBtn = h('button.sm', { id: 'lib-select', onclick: () => selMode(!selecting) }, 'Select');
  const chips = h('div.chips.scroll', { id: 'lib-chips' });
  const addInput = h('input', { type: 'file', multiple: true, accept: '.gif,image/gif', hidden: true, id: 'lib-add-files', onchange: () => { addGifs([...addInput.files]); addInput.value = ''; } });
  const addBtn = app.device && h('button', { id: 'lib-add-btn', onclick: () => addInput.click() }, '+ Add GIFs');
  // Nothing on the player matches: the same words in Create Art (its Game
  // art on the one platform chip picked, else GifCities).
  const webQ = h('b');
  const webHint = h('div.lib-web', { id: 'lib-web', hidden: true }, 'Nothing here matches “', webQ, '”. ',
    h('button.txt', { id: 'lib-web-go', onclick: () => { app.artFind = { q: search.value.trim(), platform: plats.size === 1 ? [...plats][0] : '' }; app.go('create'); } }, 'Search the web in Create Art →'));
  const addBar = h('i');
  const addMsg = h('span', { id: 'lib-add-progress' });
  const addErr = h('div.log.bad', { id: 'lib-add-errors' });
  const addCard = h('div.lib-progress', { id: 'lib-add', hidden: true }, h('div.bar', addBar), h('div.cap', addMsg), addErr);

  const selCount = h('b');
  const toSetBtn = h('button.pri', { id: 'lib-to-set', onclick: toSet }, 'Add to playlist');
  const selbar = h('div.selbar.row', { hidden: true, id: 'lib-selbar' },
    h('span.grow', selCount, ' selected'),
    h('button', { id: 'lib-all', onclick: selectAll }, 'Select all shown'),
    toSetBtn,
    h('button', { id: 'lib-sel-done', onclick: () => selMode(false) }, 'Done'));

  const ov = new Overrides();
  ov.onChange = () => tree.refresh();

  // Live and Effects: their items (and the text's form, an effect's options, in liveBox).
  const liveBox = h('div', { id: 'lib-live', hidden: true });
  const liveOn = () => { liveBox.hidden = search.value.trim() ? true : !(current?.path[0] == '@' || tree.open.has('@') || tree.open.has('@fx')); };
  const tree = new Tree({
    picked: app.selection,
    ticking: () => selecting,
    open: store.get('lib.open', null) ?? ['Consoles'],
    onOpen: (o) => { store.set('lib.open', o); liveOn(); },
    live: async (cat) => (await import('./live.js'))[cat == '@' ? 'items' : 'effects'](liveBox, preview, () => tree.refresh()),
    onPick: (it) => preview(it),
    onMenu: (it, x, y) => menu(ov, it.path, x, y),
    onHold: (it) => { selMode(true); toggle(it.path, true); },
    onTick: () => sync(),
    onError: (m) => toast(m, 'bad'),
    extra(it, row) {
      const o = ov.get(it.path);
      row.classList.toggle('hid', !!o?.never);
      return [glyphs(o), it.e.platform && h('span.tag', it.e.platform), moreButton(ov, it.path)];
    },
    onUpdate: counted,
  });
  tree.el.id = 'lib-list';

  const canvas = h('canvas.panel', { id: 'lib-panel' });
  const cap = h('div.cap', { id: 'lib-cap' }, 'Pick a GIF to see it here');
  const dTitle = h('h2', { id: 'lib-d-title' });
  const meta = h('div.cap', { id: 'lib-d-meta' });
  const sendBtn = app.device && h('button.pri', { id: 'lib-play', disabled: true, onclick: () => app.play(current.path) }, 'Send to panel');
  const plBtn = h('button', { id: 'lib-d-add', onclick: (e) => pickPlaylist(e.currentTarget) }, 'Add to playlist…');
  const editArt = h('button', { id: 'lib-d-art', onclick: () => { app.artFrom = { path: current.path, e: current.e }; app.go('create'); } }, '✎ Edit in Create Art');
  const moreBtn = h('button.more', { id: 'lib-d-more', 'aria-label': 'More', 'aria-haspopup': 'menu', onclick: (e) => detailMenu(e.currentTarget) }, '…');
  const smoothBox = app.device && h('div', { id: 'lib-smooth', hidden: true });
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

  // On a phone the chips scroll sideways: fade an edge while more is off that side.
  const r2 = h('div.lib-r2', { id: 'lib-r2' }, chips);
  const edge = () => {
    r2.classList.toggle('more', r2.scrollLeft + r2.clientWidth < r2.scrollWidth - 2);
    r2.classList.toggle('less', r2.scrollLeft > 2);
  };
  r2.addEventListener('scroll', edge, { passive: true });
  const ro = new ResizeObserver(edge);
  for (const x of [r2, chips]) ro.observe(x);

  const grid = h('div.libgrid',
    h('div',
      h('div.card.libhead',
        h('div.lib-q', search), count, fold, selBtn, addBtn, addInput,
        r2, addCard),
      webHint, tree.el, selbar),
    side);
  el.append(grid);

  const pv = new PanelPreview(canvas, { onError: (m) => { cap.textContent = 'Preview: ' + m; } });

  async function preview(it) {
    current = it;
    tree.cur = it.path;
    it.pick?.();
    liveOn();
    tree.refresh();
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
        if (gif) checkThumb(it.path, bytes);
        if (smoothBox && gif) import('./smooth.js').then((m) => m.detail(smoothBox, it, bytes, pv, cap)).catch(() => {});
      }
    } catch (err) { cap.textContent = 'Preview: ' + err.message; }
  }

  function detailMenu(btn) {
    const o = ov.get(current.path) || {};
    actions(btn, 'lib-d-menu', [
      ['lib-d-fav', (o.w > 1 ? '★' : '☆') + ' Favourite in…', () => inList(btn, fav, (x) => x.w > 1)],
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
      ov.set(it.path, (x) => ({ ...x, ...patch(x) }));
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
  function selectAll() { for (const it of tree.res) app.selection.add(it.path); sync(); }
  function selMode(v) {
    selecting = v;
    if (!v) app.selection.clear();
    selBtn.classList.toggle('on', v);
    sync();
  }
  function sync() {
    if (app.selection.size) selecting = true;
    selCount.textContent = app.selection.size;
    selbar.hidden = !selecting;
    toSetBtn.disabled = !app.selection.size;
    tree.refresh();
  }
  function toSet() {
    const have = new Set(app.draft.paths), n = app.selection.size;
    for (const p of app.selection) if (!have.has(p)) app.draft.paths.push(p);
    toast(`${n} GIF${n > 1 ? 's' : ''} added to “${app.draft.path ? playlistLabel(app.draft.path) : app.draft.name || 'New playlist'}”`, 'ok');
    selMode(false);
    emit('draft');
    app.go('sets/edit');
  }

  // The search, the platforms: the tree filters; counted() shows the result.
  // Clearing the box clears the platforms picked with it.
  let was = '';
  function apply() {
    const q = search.value.trim();
    if (was && !q) plats.clear();
    was = q;
    tree.filter(search.value, plats);
    liveOn();
  }
  function counted(st) {
    const text = search.value.trim();
    count.textContent = st.busy ? (st.text ? 'Searching…' : 'Loading…') : `${st.shown.toLocaleString()} of ${st.pool.toLocaleString()}${st.bad ? ' · some folders couldn’t be read: type again to retry' : ''}`;
    grid.classList.toggle('noplat', chipRow(chips, st.plats, plats, 'data-platform', apply) < 2);
    grid.classList.toggle('lq', !!text);
    webQ.textContent = text;
    webHint.hidden = !text || st.busy || st.matched > 0;
    tree.el.hidden = !webHint.hidden; // no empty box under the hint
  }

  async function reveal(path) {
    search.value = '';
    apply();
    await tree.load();
    const it = await tree.show(path);
    if (it) preview(it);
  }

  const opts = () => {
    // The folder open (one), else the picked GIF's, else the first open.
    const open = [...tree.open].filter((x) => x[0] != '@'), cur = current?.path.startsWith('/gifs/') && current.path.split('/')[2];
    const c = (open.length === 1 ? open[0] : cur) || open[0] || '';
    return { category: c, categories: tree.real(), platforms: [...new Set(tree.rows.filter((r) => r.path).map((r) => r.e.platform).filter(Boolean))] };
  };
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
    if (to) { forgetThumb(current.path); forgetThumb(to); forgetDir(current.path.split('/')[2]); await ov.load(ov.path); await reveal(to); }
  }
  async function remove() {
    if (!(await (await import('./details.js')).removeGif(current.path, label(current.e)))) return;
    forgetThumb(current.path);
    forgetDir(current.path.split('/')[2]);
    current = null;
    tree.cur = '';
    body.hidden = true;
    side.classList.remove('open');
    pv.clear();
    cap.textContent = 'Pick a GIF to see it here';
    await ov.load(ov.path);
    await tree.load();
  }

  async function playlists() {
    const lists = await app.playlists().catch(() => []);
    const has = (p) => p && lists.some((x) => x.path === p);
    const want = [ov.path, store.get('lib.ov'), app.activePath].find(has) || lists[0]?.path || '/filelist.txt';
    await ov.load(want); // each time: Now's ☆ or the Playlists editor may have changed it
  }

  await tree.load();
  await playlists();
  sync();
  return {
    show() {
      sync();
      playlists();
      if (tree.dirty) tree.load().then(() => { const it = current && tree.rows.find((x) => x.path === current.path); if (it) preview(it); });
      else tree.refresh(); // the thumbnails switch may have changed
    },
    // #library/<Category>: that folder open at the top, the others closed.
    async sub(sub) {
      if (!sub) return;
      if (search.value) { search.value = ''; apply(); }
      await tree.only(decodeURIComponent(sub));
    },
  };
}
