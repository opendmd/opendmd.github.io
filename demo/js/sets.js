// Playlists: the list (#sets), and the editor as a screen of its own
// (#sets/edit: on a phone it replaces the list, with a ‹ back; on a laptop
// the right column), js/pledit.js, imported the first time a playlist
// opens. Play PUT /api/config {set}; Delete DELETE /api/files/sets/….

import { app } from './state.js';
import { actions, closeMenu } from './overrides.js';
import { h, clear, on, toast, lines, note, isSys, byNice } from './util.js';

export async function mount(el) {
  const setsList = h('div.list', { id: 'sets-list' });
  const se = h('div.se', h('p.muted.sehint', 'Open a playlist to see and change what is in it.'));
  el.append(h('div.cols.sets', { id: 'sets' },
    h('div.sl', h('div.card', h('div.row.sp', { style: 'margin-bottom:8px' }, h('h2', { style: 'margin:0' }, 'Playlists'), h('button', { id: 'sets-new', onclick: newList }, '+ New playlist')), setsList,
      note('“All files” plays when no other playlist is chosen', 'Changing the playlist takes a few seconds; the panel holds its frame meanwhile.'))),
    se));

  // The editor, once something opens.
  let edQ = null, edi = null, known = [], lists = [];
  const editor = () => (edQ ??= import('./pledit.js').then((m) => (edi = m.editor(se, { el, closeEd, activate, loadSets, known: () => known, lists: () => lists }))));

  // #sets/edit: pushed when opened from the list, so ‹ goes back.
  let pushed = false;
  async function goEd() { (await editor()).header(); if (!el.classList.contains('ed')) { pushed = true; location.hash = '#sets/edit'; } }
  function closeEd() { if (pushed) history.back(); else location.replace('#sets'); }
  async function newList() {
    app.draft = { name: '', paths: [], path: '' };
    (await editor()).opened();
    goEd();
  }

  let ctok = 0;
  async function loadSets() {
    lists = await app.playlists();
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
      (await editor()).opened();
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

  on('config', () => loadSets());
  const ed = () => el.classList.contains('ed');

  await loadSets();
  return {
    show() { edi?.show(); if (app.device) app.refreshStatus().catch(() => {}); },
    async sub(sub) {
      el.classList.toggle('ed', sub == 'edit');
      if (!ed()) { pushed = false; closeMenu(); edQ?.then((e) => e.left()); } else { (await editor()).header(); scrollTo(0, 0); }
      for (const r of setsList.children) r.classList.toggle('cur', ed() && r.dataset.path === app.draft.path);
      edQ?.then((e) => e.shown());
    },
  };
}
