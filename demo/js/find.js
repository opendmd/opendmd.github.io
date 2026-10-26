// Finding GIFs and pictures on the web: Create Art's "Game art" and
// "GifCities" sources (a results grid with a row of ways to narrow it).
// Loaded with the Create Art view (js/studio.js), with find.css.
//
// A source is { id, label, search(q, opts) -> Promise<Hit[]>, filters,
// narrow?(hits, opts), caption }. A Hit is { id, title, w, h, preview (an
// image address), fetch() -> Promise<Blob>, link?, source, hint (a title
// for Create Art's Save step) }.
//
// GifCities: the Internet Archive's search over the GIFs of GeoCities (an
// old, undocumented JSON API: one answer per query, no pages, ~3 s, CORS for
// a plain GET). The GIFs themselves come from blob.gifcities.org, which lets
// pages read them; web.archive.org does not. Results are unfiltered.
//
// Game art: a name index per platform, published on the project site
// (art-index/v1/<PLATFORM>.json, from tools/art-index.py; not in the www
// pack: the feature needs the internet anyway), fetched on the first search
// of that platform and kept for the session; pictures from the
// libretro-thumbnails repositories on raw.githubusercontent.com, which lets
// pages read them.

import { app } from './state.js';
import { h, clear, SITE } from './util.js';
import { art as artModule } from './artmod.js';

const GC_API = 'https://gifcities.archive.org/api/v1/gifsearch';
const GC_BLOB = 'https://blob.gifcities.org/gifcities/';
const GC_LIMIT = 300;
export const PLATFORMS = ['MD', 'SNES', 'GB', 'GBC', 'GBA', 'SATURN', 'NDS', 'NES', 'SMS', 'PCE', 'PS1', 'N64', 'ARCADE'];
const RAW = 'https://raw.githubusercontent.com/libretro-thumbnails/';
// The index's format is in its address: a web app asks for the version it
// reads (tools/site.py ART_INDEX_FORMATS; a new shape is a new version).
const ART_INDEX = 'art-index/v1/';
const KINDS = [[1, 'Named_Titles', 'Title screen'], [2, 'Named_Snaps', 'In the game'], [4, 'Named_Boxarts', 'Box'], [8, 'Named_Logos', 'Logo']];

// --- Game art -----------------------------------------------------------------

// Fetch a picture from libretro-thumbnails. Some entries there are links:
// the file then holds the name of the real one (a few dozen bytes of text).
export async function thumb(url, hops = 3) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(r.status);
  const b = await r.blob();
  if (b.size < 512 && hops) {
    const head = new Uint8Array(await b.slice(0, 1).arrayBuffer());
    if (head[0] !== 0x89) return thumb(new URL((await b.text()).trim().split('/').map(encodeURIComponent).join('/'), url).href, hops - 1);
  }
  return b;
}

// Where the index is: the site, from a player (GitHub Pages answers with
// `Access-Control-Allow-Origin: *`); beside the demo, same-origin, when this
// page is the site's demo.
export const artIndexBase = () => (app.demo || location.origin === new URL(SITE).origin
  ? new URL('../' + ART_INDEX, location.href).href : SITE + ART_INDEX);

// A platform's index, fetched once a session (a failure is forgotten, so
// Try again fetches anew). The browser's HTTP cache keeps it between
// visits as the site's headers allow (Pages: 10 minutes, then revalidated).
const indexes = new Map();
function index(p) {
  if (!indexes.has(p)) {
    indexes.set(p, Promise.all([artModule(), fetch(artIndexBase() + `${p}.json`)]).then(async ([art, r]) => {
      if (!r.ok) throw new Error(r.status);
      const d = await r.json();
      if (!Array.isArray(d.names) || typeof d.kinds !== 'string') throw new Error('not an art index');
      d.norm = d.names.map((n) => app.wasm.normalizeTitle(art.cleanGameName(n)));
      return d;
    }).catch((e) => { indexes.delete(p); throw e; }));
  }
  return indexes.get(p);
}

// The games on platform `p` whose names match `q`, best first (at most 24):
// { d (the index), games: [{ i, game, region }] }. Throws when the list
// could not be loaded.
export async function findGames(p, q) {
  const [art, d] = await Promise.all([artModule(), index(p)]);
  const qn = app.wasm.normalizeTitle(q);
  const qw = q.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean).map((w) => app.wasm.normalizeTitle(w));
  const found = [];
  d.norm.forEach((n, i) => {
    const s = n === qn ? 0 : n.startsWith(qn) ? 1 : n.includes(qn) ? 2 : qw.every((w) => n.includes(w)) ? 3 : -1;
    if (s >= 0) found.push([s, /\((USA|World)/.test(d.names[i]) ? 0 : /\(Europe/.test(d.names[i]) ? 1 : 2, d.names[i].length, i]);
  });
  found.sort((a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2]);
  return { d, games: found.slice(0, 24).map(([, , , i]) => ({ i, game: art.cleanGameName(d.names[i]), region: d.names[i].match(/\(([^)]*)\)/)?.[1] || '' })) };
}

// A game's pictures (whichever of title screen, in-game, box and logo
// exist), as hits.
export function gamePictures(p, d, g) {
  const kinds = parseInt(d.kinds[g.i], 16);
  return KINDS.filter(([bit]) => kinds & bit).map(([bit, kind, what]) => {
    const url = `${RAW}${d.repo}/master/${kind}/${encodeURIComponent(d.names[g.i])}.png`;
    const name = g.game + (g.region ? ` (${g.region})` : '');
    return { id: url, title: `${name} · ${what}`, name, hint: g.game, what, kind, game: g.game, platform: p, logo: bit === 8, w: 0, h: 0, preview: url, fetch: () => thumb(url), source: 'gameart' };
  });
}

// --- GifCities ----------------------------------------------------------------

// "mario  arland back1" -> "Mario Arland Back1".
const tidy = (s) => String(s || '').replace(/[_+]+/g, ' ').replace(/\s+/g, ' ').trim().replace(/\b\p{L}/gu, (c) => c.toUpperCase());

const gcCache = new Map(); // query -> Promise<Hit[]>, the last few
function gifcities(q) {
  const k = q.toLowerCase();
  if (!gcCache.has(k)) {
    const p = fetch(`${GC_API}?q=${encodeURIComponent(q)}&limit=${GC_LIMIT}`).then((r) => {
      if (!r.ok) throw new Error(r.status);
      return r.json();
    }).then((rows) => rows.filter((x) => x && x.checksum).map((x) => {
      const url = `${GC_BLOB}${x.checksum}.gif`;
      const title = tidy(x.url_text) || 'GIF';
      return { id: x.checksum, title, hint: title.replace(/[^\p{L}\p{N} ]+/gu, ' ').replace(/(\s+\d+)+$/, '').replace(/\s+/g, ' ').trim(), w: +x.width || 0, h: +x.height || 0, rank: +x.weight || 0, preview: url, fetch: () => fetch(url, { cache: 'no-cache' }).then((r) => { if (!r.ok) throw new Error(r.status); return r.blob(); }), link: x.page, source: 'gifcities' };
    }));
    p.catch(() => gcCache.delete(k));
    gcCache.set(k, p);
    while (gcCache.size > 5) gcCache.delete(gcCache.keys().next().value);
  }
  return gcCache.get(k);
}

export const sources = [
  {
    id: 'gifcities',
    label: 'GifCities',
    placeholder: 'Search GifCities',
    search: (q) => gifcities(q),
    filters: [
      { id: 'shape', type: 'chips', value: 'any', items: [['any', 'Any'], ['wide', 'Wide (suits the panel)'], ['square', 'Square-ish']] },
      { id: 'min', type: 'check', value: true, label: 'At least 48 px wide' },
      { id: 'sort', type: 'select', value: 'rank', items: [['rank', 'Best match'], ['size', 'Biggest first']] },
    ],
    narrow(hits, o) {
      const out = hits.filter((x) => (!o.min || x.w >= 48) && (o.shape === 'any'
        || (o.shape === 'wide' ? x.w >= 2 * x.h : x.w < 2 * x.h && x.h < 2 * x.w)));
      return out.sort(o.sort === 'size' ? (a, b) => b.w * b.h - a.w * a.h : (a, b) => b.rank - a.rank);
    },
    fail: 'GifCities didn’t answer',
    caption: () => ['From the Internet Archive’s GeoCities collection. Results are unfiltered. ', h('a', { href: 'https://gifcities.org/', target: '_blank', rel: 'noopener' }, 'gifcities.org')],
  },
  {
    id: 'gameart',
    label: 'Game art',
    placeholder: 'Game title',
    async search(q, o) {
      const { d, games } = await findGames(o.platform, q);
      return games.slice(0, 12).flatMap((g) => gamePictures(o.platform, d, g));
    },
    filters: [
      { id: 'platform', type: 'select', value: PLATFORMS[0], search: true, label: 'Platform',
        items: async () => { const art = await artModule(); return PLATFORMS.map((p) => [p, art.consoleName(p) || p]); } },
    ],
    fail: 'Game art needs the internet: the list of games couldn’t be reached',
    caption: () => ['Title screens, in-game shots, boxes and logos from the ', h('a', { href: 'https://github.com/libretro-thumbnails', target: '_blank', rel: 'noopener' }, 'libretro-thumbnails'), ' collections'],
  },
];

// --- The results ----------------------------------------------------------------

// find.css, once (resolves when it has loaded).
let styled = null;
export const style = () => (styled ??= new Promise((res) => document.head.append(h('link', { rel: 'stylesheet', href: './find.css', onload: res, onerror: res }))));

// A result tile: the picture (small GIFs at a whole-number zoom, as the panel
// would show pixels), its title and size (a game picture: the game, and which picture); `onPick(hit, tile)` on a click.
// Object URLs made for previews that will not load straight go in `urls`.
export function tile(x, onPick, urls) {
  const dims = h('small', x.what || (x.w ? `${x.w}×${x.h}` : ''));
  // crossOrigin: the same request as fetch() below, so the browser's
  // cache serves the bytes to it; a plain <img> entry (no Origin sent,
  // so no CORS header back from blob.gifcities.org) would fail that fetch.
  const img = h('img', { loading: 'lazy', alt: x.title, crossOrigin: 'anonymous', src: x.preview });
  img.onload = () => {
    if (!x.w) { x.w = img.naturalWidth; x.h = img.naturalHeight; if (!x.what) dims.textContent = `${x.w}×${x.h}`; }
    img.style.width = x.w * Math.max(1, Math.min(4, Math.floor(Math.min(136 / x.w, 72 / x.h)))) + 'px';
  };
  // A preview that will not load straight (a game-art link): its bytes.
  img.onerror = () => {
    img.onerror = () => t.remove();
    x.fetch().then((b) => { const u = URL.createObjectURL(b); urls.push(u); img.src = u; }, () => t.remove());
  };
  const t = h('button.find-hit', { 'data-id': x.id, 'data-kind': x.kind, title: x.title, onclick: () => onPick(x, t) }, h('span.find-img', img), h('span.find-t', x.name || x.title), dims);
  return t;
}

// A results grid into `el`, the narrowing row (with the platform select for
// Game art) into `narrow`; a click on a hit is `onPick(hit)` (awaited; it
// loads the picture into the editor, and throws if it could not).
// Returns { use(source), search(q), set(source id, filter id, value) }.
export function mountResults(el, { onPick, narrow }) {
  style();
  let src = null, all = [], tok = 0, q = '', urls = [];
  const opts = {}; // per source id: the filters' values
  const narrowRow = h('div.row.find-narrow', { id: 'find-narrow' });
  const count = h('span.muted', { id: 'find-count' });
  const status = h('div.cap', { id: 'find-status', 'aria-live': 'polite' });
  const grid = h('div.find-grid', { id: 'find-grid' });
  const caption = h('p.cap', { id: 'find-caption' });
  narrow.append(narrowRow);
  el.append(h('div.find', { id: 'find' }, h('div.find-top', count, status), grid, caption));

  const o = () => opts[src.id];
  function controls() {
    const v = o();
    clear(narrowRow, src.filters.map((f) => {
      const set = (x) => { v[f.id] = x; f.search ? run() : show(); };
      if (f.type === 'chips') {
        const box = h('div.chips', { id: 'find-' + f.id }, f.items.map(([x, t]) => h('button.chip' + (v[f.id] === x ? '.on' : ''), { 'data-v': x, onclick: () => { set(x); controls(); } }, t)));
        return box;
      }
      if (f.type === 'check') return h('label', h('input', { type: 'checkbox', id: 'find-' + f.id, checked: v[f.id], onchange: (e) => set(e.target.checked) }), f.label);
      const s = h('select', { id: 'find-' + f.id, 'aria-label': f.label || f.id, onchange: () => set(s.value) });
      const fill = (items) => { clear(s, items.map(([x, t]) => h('option', { value: x, selected: v[f.id] === x }, t))); };
      if (typeof f.items === 'function') { fill(PLATFORMS.map((p) => [p, p])); f.items().then(fill, () => {}); } else fill(f.items);
      return s;
    }));
  }

  function show() {
    for (const u of urls) URL.revokeObjectURL(u);
    urls = [];
    const hits = src.narrow ? src.narrow([...all], o()) : all;
    count.textContent = all.length ? `${hits.length.toLocaleString()} of ${all.length.toLocaleString()}` : '';
    el.dataset.shown = hits.length;
    clear(grid, hits.map((x) => tile(x, choose, urls)));
  }

  async function choose(x, t) {
    for (const c of grid.children) c.classList.toggle('on', c === t);
    status.textContent = 'Loading…';
    try { await onPick(x); status.textContent = ''; } catch { status.textContent = 'That one could not be loaded. Try another'; }
  }

  async function run() {
    const t = ++tok;
    all = [];
    show();
    if (!q) { status.textContent = ''; return; }
    status.textContent = 'Searching…';
    try {
      const hits = await src.search(q, o());
      if (t !== tok) return;
      all = hits;
      status.textContent = hits.length ? '' : `Nothing found for “${q}”`;
    } catch {
      if (t !== tok) return;
      clear(status, src.fail + ' ', h('button.sm', { id: 'find-retry', onclick: run }, 'Try again'));
    }
    show();
  }

  const init = (s) => (opts[s.id] ??= Object.fromEntries(s.filters.map((f) => [f.id, f.value])));
  return {
    use(s) {
      src = s;
      init(s);
      el.dataset.source = s.id;
      clear(caption, s.caption());
      controls();
      tok++;
      all = [];
      show();
      status.textContent = '';
    },
    search(text) { q = text.trim(); return run(); },
    set(id, f, v) {
      const s = sources.find((x) => x.id === id);
      init(s)[f] = v;
      if (s === src) controls();
    },
  };
}
