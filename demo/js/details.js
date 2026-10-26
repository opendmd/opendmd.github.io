// A GIF's details are its file name: `/gifs/<Category>/<PLATFORM>_<Title_
// Words>_<NN>[_<author>].gif` is the whole of its metadata, read by the
// core's parser everywhere (Library, playlists, the device's filters). So
// giving a GIF a title, a platform, a category or an author means naming
// it, and changing them means renaming it. This module (loaded when Add
// GIFs, Edit details or Delete is first used) holds:
//
// - addDialog(files, opts): a title, platform, category and author for
//   each picked GIF, the name they make shown as it is typed. The title is
//   guessed from the file's own name (its tags when it follows the grammar;
//   else separators to spaces, a trailing number and download noise gone),
//   and left empty — and required — when the name is a hash or a random
//   string. Resolves to [{file, category, name}] for Library's upload.
// - editDialog(path, opts): the same fields for a GIF on the card; saving a
//   change renames it (renameGif).
// - renameGif(from, to): POST /api/files/move — the device moves the GIF,
//   its prepared copy, its Smooth motion line and its failure line — then
//   every playlist that names it and every playlist's overrides.
// - removeGif(path, name): after asking, the GIF (the device takes its copy),
//   its playlist lines, overrides and Smooth motion line.
//
// The name is built by the core (`NameFields`, through the art module's
// `buildName`: the main module has no room for it, and these dialogs load
// the art module anyway to prepare what they add), which refuses anything
// that would not read back as the same fields. The number is the next free
// one for that platform and title in the category. The copy says title,
// platform, category, author — never tag, bucket or a copy's path.

import { app, setValue } from './state.js';
import { art } from './artmod.js';
import { fileFor } from './overrides.js';
import { authorOf, savePlaylist } from './gifs.js';
import { h, clear, toast, base, dir, enc, label, niceName, byNice } from './util.js';

const WHY = {
  title: 'Give it a title',
  'title-char': 'A title can’t have _ " * / : < > ? \\ | or accented letters',
  platform: 'That is not a platform name',
  category: 'A category name can’t have / " * : < > ? \\ | or accented letters',
  author: 'An author is one word of letters and digits, 3 or more, like “sephirot68”',
  'needs-platform': 'An author needs a platform',
  'too-long': 'That name is too long: shorten the title',
};
const lc = (s) => s.toLowerCase();

// --- Guessing a title from a file's own name ---------------------------------

// Hash-like or random names (`QT6JDNGFS3Q4OO6GBKO4LBW2ZZ4MO375`, a UUID,
// hex, a long run with hardly a vowel): nothing to guess from.
export function looksRandom(s) {
  const t = s.replace(/[\s_.-]+/g, '');
  if (t.length < 10) return false;
  const letters = t.replace(/[^a-z]/gi, '');
  const vowels = letters.replace(/[^aeiouy]/gi, '').length;
  return /^[0-9a-f]{8}-?[0-9a-f]{4}-?/i.test(s)
    || (/^[A-Z0-9]+$/.test(t) && /\d/.test(t))
    || (/^[0-9a-f]+$/i.test(t) && /\d/.test(t))
    || (letters.length >= 8 && vowels / letters.length < 0.2)
    || (!/[\s_-]/.test(s) && t.length >= 16 && (t.match(/\d/g) || []).length >= 3);
}

const NOISE = /\((\d+)\)|@\dx|\b(copy|final|ezgif|giphy|downloaded|download|animated|optimi[sz]ed|compressed|resized|min|hd|\d+x\d+|\d+px)\b/gi;

// {title, platform?, author?, flags?} from a picked file's name.
export function guess(name) {
  const stem = name.replace(/\.gif$/i, '');
  if (looksRandom(stem)) return { title: '' };
  const e = app.wasm.parsePath('/gifs/x/' + name);
  if (e.platform) {
    const author = authorOf(e);
    return { title: e.title, platform: e.platform, author, flags: e.flags.filter((f) => f !== author) };
  }
  let t = stem.replace(/[_.-]+/g, ' ').replace(NOISE, ' ').replace(/\s+\d+\s*$/, '').replace(/\s+/g, ' ').trim();
  if (looksRandom(t)) t = '';
  return { title: t && t[0].toUpperCase() + t.slice(1) };
}

// --- Names on the card --------------------------------------------------------

// Per category: its file names (lowercased) and, per platform + title, the
// highest number in use. Listed once per dialog.
export function listings(known) {
  const got = new Map();
  return (c) => {
    if (!got.has(c)) {
      // A new category has nothing in it (and listing it would be a 404).
      got.set(c, (known.includes(c) ? app.src.list(`/gifs/${c}`).catch(() => []) : Promise.resolve([])).then((files) => {
        const names = new Set(), max = new Map();
        for (const f of files) {
          if (f.dir) continue;
          names.add(lc(f.name));
          const e = app.wasm.parsePath(`/gifs/${c}/${f.name}`);
          const k = key(e.platform, e.title);
          max.set(k, Math.max(max.get(k) || 0, e.variant ?? 1));
        }
        return { names, max };
      }));
    }
    return got.get(c);
  };
}
const key = (platform, title) => `${platform || ''}|${app.wasm.normalizeTitle(title)}`;

// --- The dialog -------------------------------------------------------------

const CSS = `.dlg{position:fixed;inset:0;z-index:25;background:#000b;overflow:auto;padding:40px 16px;overscroll-behavior:contain}
.dlg>.card{width:min(820px,100%);margin:0 auto}
.dlg h2{margin-bottom:4px}
.dl-item{display:grid;grid-template-columns:128px minmax(0,1fr);gap:12px;padding:12px 0;border-top:1px solid var(--line)}
.dl-item img{width:128px;aspect-ratio:4/1;object-fit:contain;background:#000;image-rendering:pixelated;border-radius:4px}
.dl-item .form{grid-template-columns:repeat(auto-fill,minmax(160px,1fr));gap:8px}
.dl-item .dl-title{width:100%}
.dl-name{margin-top:8px;word-break:break-all}
.dl-err{font-size:13px;margin-top:4px}
.dl-item input.bad{border-color:var(--bad)}
.dl-foot{justify-content:flex-end;margin-top:12px}
@media (max-width:600px){.dlg{padding:0}.dlg>.card{border-radius:0;min-height:100%;margin:0}.dl-item{grid-template-columns:minmax(0,1fr)}}`;

let styled = false;

// One dialog, one or more items; resolves to the items' results on OK, or
// null on Cancel.
async function dialog({ title, intro, files, inits, ok, opts, keep }) {
  const m = await art();
  if (!styled) { document.head.append(h('style', CSS)); styled = true; }
  const listing = listings(opts.categories);
  const urls = [];
  const plats = [...new Set([...m.platforms(), ...(opts.platforms || [])])].sort();
  let cats = [...new Set([...opts.categories, ...inits.map((i) => i.category).filter(Boolean)])].sort(byNice);
  const items = inits.map((init, i) => row(init, i, files?.[i]));
  const okBtn = h('button.pri', { id: 'dl-ok', disabled: true, onclick: () => done(items.map((it) => it.out)) }, ok);
  const el = h('div.dlg', { id: 'dl', role: 'dialog', 'aria-modal': 'true' },
    h('div.card', h('h2', title), h('p.muted', intro), items.map((it) => it.el),
      h('div.row.dl-foot', h('button', { id: 'dl-cancel', onclick: () => done(null) }, 'Cancel'), okBtn)));
  let done;
  const result = new Promise((res) => { done = (v) => { el.remove(); removeEventListener('keydown', esc); for (const u of urls) URL.revokeObjectURL(u); res(v); }; });
  const esc = (e) => e.key === 'Escape' && done(null);
  addEventListener('keydown', esc);
  document.body.append(el);
  items[0]?.title.focus();
  refresh();
  return result;

  function row(init, i, file) {
    const title = h('input', { type: 'text', className: 'dl-title', value: init.title || '', placeholder: 'What it shows', required: true, 'aria-label': 'Title' });
    const platform = h('select', { className: 'dl-plat', 'aria-label': 'Platform' }, h('option', { value: '' }, 'None'), plats.map((p) => h('option', { value: p }, m.consoleName(p) ? `${p} · ${m.consoleName(p)}` : p)));
    platform.value = init.platform || '';
    const category = h('select', { className: 'dl-cat', 'aria-label': 'Category' });
    const author = h('input', { type: 'text', className: 'dl-author', value: init.author || '', placeholder: 'optional', 'aria-label': 'Author' });
    const name = h('div.cap.mono.dl-name');
    const err = h('div.bad.dl-err');
    let src = '';
    if (file) urls.push(src = URL.createObjectURL(file));
    else if (init.path) src = enc(init.path);
    const it = {
      title, platform, category, author, init, file, out: null,
      el: h('div.dl-item', { 'data-i': i },
        src ? h('img', { src, alt: '' }) : h('div'),
        h('div',
          file && h('div.cap.mono', file.name),
          h('div.form',
            h('label.stack', 'Title', title), h('label.stack', 'Platform', platform),
            h('label.stack', 'Category', category), h('label.stack', 'Author', author)),
          name, err)),
      name, err,
    };
    fillCats(it, init.category);
    for (const c of [title, author]) c.addEventListener('input', refresh);
    platform.onchange = refresh;
    category.onchange = () => {
      if (category.value === '\n') {
        const c = (prompt('Name of the new category') || '').trim();
        if (c && !cats.includes(c)) cats = [...cats, c].sort(byNice);
        for (const x of items) fillCats(x, x === it ? c || x.cat : x.cat);
      }
      refresh();
    };
    return it;
  }

  function fillCats(it, value) {
    clear(it.category, cats.map((c) => h('option', { value: c }, niceName(c))), h('option', { value: '\n' }, 'New category…'));
    it.category.value = cats.includes(value) ? value : cats.includes(opts.category) ? opts.category : cats[0] || '';
    it.cat = it.category.value;
  }

  // Every item's name: the next free number, and no two alike in the dialog.
  async function refresh() {
    const t = (refresh.t = (refresh.t || 0) + 1);
    okBtn.disabled = true;
    const taken = new Set();
    let bad = 0;
    for (const it of items) {
      it.cat = it.category.value;
      const c = it.cat;
      const f = { title: it.title.value.trim(), platform: it.platform.value || null, category: c, author: it.author.value.trim() || null, flags: it.init.flags || [], ordinal: null };
      it.author.placeholder = f.platform ? 'optional' : 'optional · needs a platform';
      const l = await listing(c);
      if (t !== refresh.t) return;
      const { path = null, why = '' } = pickName(m, f, l, keep, taken);
      it.out = path && { file: it.file, category: c, name: base(path), path };
      it.title.classList.toggle('bad', !f.title);
      it.name.textContent = path ? `→ ${base(path)}${app.device ? ` in ${c}` : ''}` : '';
      it.err.textContent = why;
      Object.assign(it.el.dataset, { name: path ? base(path) : '', error: why ? 'yes' : '' });
      if (path) taken.add(lc(path)); else bad++;
    }
    okBtn.disabled = bad > 0;
  }
}

// What the parser would make of it: a word in the title taken for a tag.
function readsBack(f) {
  const words = f.title.split(/\s+/);
  const e = app.wasm.parsePath('/gifs/x/' + [f.platform, ...words, String(f.ordinal ?? 1).padStart(2, '0'), f.author].filter(Boolean).join('_') + '.gif');
  return `That would read back as “${e.title}”: a word in the title is taken for a tag. Try other words`;
}

// The name for fields `f` (`nameFields`' shape) in a category's listing
// `l` (from `listings`): the next free number for that platform and title
// — the one it had when `keep` (a GIF being edited) has the same platform,
// title and category —, or, with no platform, the title suffixed until
// the name is free; never one in `taken`. {path} or {why} (the reason, in
// words). Add GIFs, Edit details and the Studio's Save all name through it.
export function pickName(m, f, l, keep, taken = new Set()) {
  f = { ...f };
  const unchanged = keep && f.category === keep.category && f.platform === keep.platform && app.wasm.normalizeTitle(f.title) === app.wasm.normalizeTitle(keep.title);
  const self = keep && lc(keep.path);
  const free = (p) => !taken.has(lc(p)) && (p.toLowerCase() === self || !l.names.has(lc(base(p))));
  try {
    if (f.platform) {
      f.ordinal = unchanged ? keep.ordinal : (l.max.get(key(f.platform, f.title)) || 0) + 1;
      let path = m.buildName(f);
      for (; !free(path); path = m.buildName(f)) f.ordinal = (f.ordinal || 1) + 1;
      return { path };
    }
    const t0 = f.title;
    for (let n = 2, p = m.buildName(f); ; p = m.buildName({ ...f, title: `${t0} ${n++}` })) if (free(p)) return { path: p };
  } catch (c0) {
    return { why: WHY[c0] || readsBack(f) };
  }
}

export function addDialog(files, opts) {
  const n = files.length;
  return dialog({
    title: n > 1 ? `Add ${n} GIFs` : 'Add a GIF',
    intro: 'A GIF’s title, platform and author are its file name on the card: that is what the Library lists, search finds and playlists filter by.',
    files, opts, ok: n > 1 ? `Add ${n} GIFs` : 'Add GIF',
    inits: files.map((f) => ({ ...guess(f.name), category: opts.category })),
  });
}

export async function editDialog(path, opts) {
  const f = (await art()).nameFields(path);
  const got = await dialog({
    title: 'Edit details',
    intro: 'Renames the GIF on the card. Playlists that have it are updated.',
    opts, ok: 'Save',
    inits: [{ ...f, path }],
    keep: { ...f, path },
  });
  const to = got?.[0]?.path;
  if (!to || to === path) return null;
  try {
    await renameGif(path, to);
    toast(`renamed to “${label(app.wasm.parsePath(to))}”`, 'ok');
    return to;
  } catch (e) { toast(`renaming: ${e.message}`, 'bad'); return null; }
}

// --- Renaming and deleting ----------------------------------------------------

// Rewrite every playlist line, and every playlist's overrides, through
// `map(path) -> path | null` (null drops it).
async function rewrite(map) {
  for (const pl of await app.playlists().catch(() => [])) {
    const text = await app.text(pl.path, true).catch(() => null);
    if (text) {
      let hit = false;
      const out = text.split('\n').map((l) => {
        const p = l.trim();
        if (!p) return l;
        const n = map(p);
        if (n === p) return l;
        hit = true;
        return n;
      }).filter((l) => l !== null);
      if (hit) await savePlaylist(pl.path, out.join('\n'));
    }
    const list = app.device ? (await app.src.overrides(setValue(pl.path)).catch(() => null))?.overrides || [] : [];
    const next = list.map((o) => ({ ...o, path: map(o.path) })).filter((o) => o.path);
    if (next.length !== list.length || next.some((o, i) => o.path !== list[i].path)) {
      const meta = app.wasm.renderOverrides(next);
      if (meta) await app.src.writeFile(fileFor(pl.path), meta); else await app.src.deleteFile(fileFor(pl.path));
    }
  }
}

const same = (a) => (p) => lc(p) === lc(a);

export async function renameGif(from, to) {
  await app.src.move(from, to);
  await rewrite((p) => (same(from)(p) ? to : p));
  // What the app holds by path follows too.
  for (const s of [app.draft.paths]) s.forEach((p, i) => { if (same(from)(p)) s[i] = to; });
  if (app.selection.delete(from)) app.selection.add(to);
  await dropEmpty(from);
}

export async function removeGif(path, name) {
  if (!confirm(`Delete “${name}” from the card? It is taken out of every playlist too. This can’t be undone.`)) return false;
  try {
    await app.src.deleteFile(path);
    // The device's DELETE takes the prepared copy with it; this is a no-op
    // then, and the cleanup on a device that did not.
    await app.src.forget(path).catch(() => {});
    await rewrite((p) => (same(path)(p) ? null : p));
    await unsmooth(path);
    app.draft.paths = app.draft.paths.filter((p) => !same(path)(p));
    app.selection.delete(path);
    await dropEmpty(path);
    toast(`deleted “${name}”`, 'ok');
    return true;
  } catch (e) { toast(`deleting “${name}”: ${e.message}`, 'bad'); return false; }
}

// Smooth motion's list (js/smooth.js) loses a deleted GIF's line. Read only
// when it is there: a missing list is a 404.
async function unsmooth(path) {
  const has = async (d, n) => (await app.src.list(d).catch(() => [])).some((f) => f.name === n);
  if (!(await has('/', 'sets')) || !(await has('/sets', '.smooth'))) return;
  const text = (await app.src.readText('/sets/.smooth')) || '';
  const keep = text.split('\n').map((l) => l.trim()).filter((l) => l && !same(path)(l));
  if (keep.length !== text.split('\n').filter((l) => l.trim()).length) await app.src.writeFile('/sets/.smooth', keep.map((p) => p + '\n').join(''));
}

// A category left empty goes (its folder, and its prepared copies' folder).
async function dropEmpty(path) {
  const d = dir(path);
  if (!/^\/gifs\/[^/]+$/.test(d) || (await app.src.list(d).catch(() => [{}])).length) return;
  try {
    await app.src.deleteFile(d);
    const c = base(d);
    if ((await app.src.list('/cache').catch(() => [])).some((f) => f.dir && f.name === c)) await app.src.json('DELETE', `/api/files${enc('/cache/' + c)}?recursive=1`);
  } catch { /* an older device keeps the empty folder */ }
}
