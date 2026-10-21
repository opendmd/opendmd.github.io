// Entry point: load the wasm module, find out whether a device is serving us
// (GET /api/status) or we are a static page on a laptop (local mode), and
// route between the five views (Create Art is the Studio, js/studio.js, whose
// stylesheet and wasm load when it is first opened; `#studio`, its old
// address, still leads there). Each view is a module with
// `mount(el) -> {show, hide}`, mounted on first visit.

import init from './pkg/opendmd_web.js';
import { app } from './js/state.js';
import { detect, demoCard, LocalSource } from './js/source.js';
import { $, h, clear, emit, toast } from './js/util.js';

await init();

const VIEWS = ['now', 'library', 'create', 'sets', 'settings'];
const MODULE = { create: 'studio' };
const mounted = {};
let current = '';

async function route() {
  if (location.hash === '#studio') { location.replace('#create'); return; }
  const name = VIEWS.includes(location.hash.slice(1)) ? location.hash.slice(1) : 'now';
  if (name === current) return;
  if (current) mounted[current]?.hide?.();
  current = name;
  for (const v of VIEWS) $('#view-' + v).classList.toggle('on', v === name);
  for (const a of document.querySelectorAll('#nav a')) a.classList.toggle('on', a.dataset.view === name);
  if (!app.src) return;
  if (!mounted[name]) {
    const mod = await import(`./js/${MODULE[name] || name}.js`);
    mounted[name] = await mod.mount($('#view-' + name));
  }
  mounted[name].show?.();
}

function setSource(src) {
  app.src = src;
  for (const k of Object.keys(mounted)) delete mounted[k];
  for (const v of VIEWS) clear($('#view-' + v));
  current = '';
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
