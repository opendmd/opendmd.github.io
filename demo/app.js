// Entry point: load the wasm module, find out whether a device is serving us
// (GET /api/status) or we are a static page on a laptop (local mode), and
// route between the six views (Create Art is the Studio, js/studio.js, and
// Formulas js/formulas.js, whose stylesheets and wasm load when they are
// first opened; `#studio`, Create Art's old address, still leads there). Each view is a module with
// `mount(el) -> {show, hide}`, mounted on first visit.

import init from './pkg/opendmd_web.js';
import { app } from './js/state.js';
import { detect, demoCard, LocalSource } from './js/source.js';
import { $, h, clear, emit, toast } from './js/util.js';
import { closeMenu } from './js/overrides.js';

await init();

const VIEWS = ['now', 'library', 'sets', 'formulas', 'create', 'settings'];
const MODULE = { create: 'studio' };
// name -> the mount promise; ready[name] once it has resolved.
const mounted = {}, ready = {};
let current = '', shown = '';

// `#view` or `#view/sub` (Settings' tabs, the Playlists editor): a view's
// `show(sub)` runs when it comes up, `sub(sub)` on every route to it. A
// hash change while a view is still mounting (or before the source is
// known) mounts whatever the hash says once it can.
async function route() {
  // Old addresses: the Studio's, and the editor's when Formulas were a Library tab.
  if (location.hash === '#studio') { location.replace('#create'); return; }
  if (location.hash === '#library/formula') { location.replace('#formulas/edit'); return; }
  // A shared formula: the Formulas editor (js/formulas.js reads it, and
  // replaces the address once the editor has it).
  const f = location.hash.startsWith('#formula=') && (app.formula = location.hash.slice(1));
  const [v, sub = ''] = f ? ['formulas', 'edit'] : location.hash.slice(1).split('/');
  const name = VIEWS.includes(v) ? v : 'now';
  if (name !== current) {
    if (shown) ready[shown]?.hide?.();
    shown = '';
    closeMenu(); // a menu or sheet belongs to the view it opened in
    // Safari keeps focus (and the keyboard) in a field a nav tap hides.
    document.activeElement?.blur?.();
    current = name;
    for (const x of VIEWS) $('#view-' + x).classList.toggle('on', x === name);
    for (const a of document.querySelectorAll('#nav a')) a.classList.toggle('on', a.dataset.view === name);
  }
  if (!app.src) return;
  const m = await (mounted[name] ??= import(`./js/${MODULE[name] || name}.js`).then((mod) => mod.mount($('#view-' + name))).then((r) => (ready[name] = r || {})));
  if (current !== name || app.src == null) return;
  if (shown !== name) { shown = name; m.show?.(sub); }
  m.sub?.(sub);
}

function setSource(src) {
  app.src = src;
  for (const k of Object.keys(mounted)) delete mounted[k], delete ready[k];
  for (const v of VIEWS) clear($('#view-' + v));
  current = shown = '';
  emit('source', src);
  route();
}

function localPicker() {
  const card = $('#t-local').content.cloneNode(true);
  const pick = (e) => {
    const files = e.target.files;
    if (!files.length) return;
    const src = new LocalSource(files, app.wasm.cardPath);
    $('#where').textContent = `${src.label || 'Picked files'} · ${src.count} files`;
    toast(`${src.count} files picked — nothing is read until it plays`);
    setSource(src);
  };
  card.querySelector('#pick-dir').addEventListener('change', pick);
  card.querySelector('#pick-files').addEventListener('change', pick);
  return card;
}

addEventListener('hashchange', route);
const found = await detect();
if (found) {
  app.src = found.src;
  app.status = found.status;
  const s = found.status;
  // The device's name; its address and version are in Settings → About.
  $('#where').textContent = s.hostname || 'OpenDMD player';
  app.refreshConfig().catch((e) => toast('config: ' + e.message, 'bad'));
  route();
} else if ((app.demo = await demoCard(app.wasm.cardPath))) {
  // The web demo: its own card, in memory; a card folder of yours can replace it.
  $('#where').textContent = 'Demo · runs in your browser, nothing is uploaded';
  const own = h('label.btn.sm.demo-own', 'Use your own card…', h('input', { type: 'file', webkitdirectory: true, hidden: true, onchange: (e) => {
    const files = e.target.files;
    if (!files.length) return;
    const src = new LocalSource(files, app.wasm.cardPath);
    app.demo = null;
    own.remove();
    $('#where').textContent = `${src.label || 'Picked files'} · ${src.count} files`;
    setSource(src);
  } }));
  $('#top').append(own);
  $('#top strong').replaceChildren(h('a', { href: '../', style: 'color:inherit;text-decoration:none' }, 'OpenDMD'));
  setSource(app.demo);
} else {
  $('#where').textContent = 'Local mode';
  for (const v of VIEWS) $('#view-' + v).append(h('div', localPicker()));
  route();
  // Re-offer the picker from any view until something is picked.
  addEventListener('hashchange', () => { if (!app.src) route(); });
}
