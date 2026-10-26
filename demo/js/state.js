// App-wide state shared by the views: the source, the device's last status
// and config, the manifests (fetched once, on demand), the playlists, the
// Library selection and the playlist being edited.
//
// Users see GIFs and playlists. A GIF's prepared copy (a pre-rendered file
// the player plays with less memory) is a cache: the device plays it when it
// exists, previews fetch it first and fall back to the GIF, and nothing here
// ever shows its path.

import * as wasm from '../pkg/opendmd_web.js';
import { emit, toast, base } from './util.js';

// Root manifests with a name people know them by. Anything else shows its
// file name.
const LABELS = { 'filelist.txt': 'All files', 'filelist.hall': 'Halloween', 'filelist.xmas': 'Christmas' };
// Manifests of prepared-copy paths from before copies were found
// automatically (filelist.odm.txt, and the like). The device ignores them;
// so does the UI.
const OBSOLETE = /^filelist\.odm\./i;

export function playlistLabel(name) {
  const n = base(name);
  if (LABELS[n]) return LABELS[n];
  // A set named on the card with underscores reads as words (`Sprites_and_Clock`).
  return /^filelist\./.test(n) ? n : n.replace(/\.txt$/, '').replace(/(?!^)_/g, ' ');
}

// The `set` value (config, schedule, GET /api/overrides) for a playlist's
// card path: "" for /filelist.txt, the file name for another root playlist,
// the name for /sets/<name>.txt.
export function setValue(path) {
  const n = base(path);
  if (!path.startsWith('/sets/')) return n === 'filelist.txt' ? '' : n;
  return n.replace(/\.txt$/, '');
}

// A `set` value's card path (the inverse).
export function setPath(set) {
  return !set ? '/filelist.txt' : set.startsWith('filelist.') ? '/' + set : `/sets/${set}.txt`;
}

// A directory's entries; a 404 (a picked folder without it, a directory
// gone) is []. A busy device's 503 or a dropped request is retried inside
// `list` (source.js `queued`, the one retry layer, ~45 s): an empty list
// would show Playlists or a Library folder with nothing in it until a reload.
export async function listOr(dir) {
  try { return await app.src.list(dir); } catch (e) {
    if (e && e.status === 404) return [];
    throw e;
  }
}

export const app = {
  wasm,
  src: null,
  status: null,
  config: null,
  selection: new Set(),
  draft: { name: '', paths: [], path: '' },
  texts: new Map(),

  get device() { return this.src?.kind === 'device'; },

  // A manifest's text, fetched once per session (filelist.txt is 450 KB);
  // callers at the same moment share the one fetch.
  tq: new Map(),
  async text(path, fresh = false) {
    if (fresh || !this.texts.has(path)) {
      let q = !fresh && this.tq.get(path);
      if (!q) this.tq.set(path, q = this.src.readText(path).finally(() => this.tq.delete(path)));
      this.texts.set(path, await q);
    }
    return this.texts.get(path);
  },

  // The playlists: the card's root manifests (filelist.txt, .hall, .xmas —
  // not their .idx or temp files, not the obsolete one) and /sets/*.txt.
  // /sets is only listed when the root shows it: a card that never had a
  // playlist has no directory to 404 on. Each is {path, value, label, root}:
  // `value` is what `set` holds to play it ("" for All files, the default).
  // Callers at the same moment (a config change refreshes several views)
  // share one listing: the device has four sockets.
  playlists() { return (this.pl ??= this.listPlaylists().finally(() => { this.pl = null; })).then((l) => [...l]); },
  async listPlaylists() {
    const root = await listOr('/');
    const sets = root.some((f) => f.dir && f.name === 'sets') ? await listOr('/sets') : [];
    const roots = root.filter((f) => !f.dir && /^filelist\./.test(f.name) && !OBSOLETE.test(f.name) && !/\.(idx|part|tmp|bak|meta)$/.test(f.name))
      .map((f) => f.name).sort((a, b) => (a === 'filelist.txt' ? -1 : b === 'filelist.txt' ? 1 : a.localeCompare(b)));
    const names = sets.filter((f) => !f.dir && /\.txt$/.test(f.name)).map((f) => f.name.replace(/\.txt$/, '')).sort();
    return [
      ...roots.map((n) => ({ path: '/' + n, value: n === 'filelist.txt' ? '' : n, label: playlistLabel(n), root: true })),
      ...names.map((n) => ({ path: `/sets/${n}.txt`, value: n, label: playlistLabel(n), root: false })),
    ];
  },

  // (A device, or the demo's player: js/demo.js.)
  async refreshStatus(recent) {
    if (!this.src?.config) return null;
    this.status = await this.src.status(recent);
    emit('status', this.status);
    return this.status;
  },
  async refreshConfig() {
    if (!this.src?.config) return null;
    this.config = await this.src.config();
    emit('config', this.config);
    return this.config;
  },
  // The configured `set`: '' = All files (/filelist.txt), 'filelist.*' =
  // that root manifest, anything else = /sets/<set>.txt.
  get activeSet() { return this.config?.set ?? ''; },
  // The card path of the playlist in play: status `manifest`, else the config.
  get activePath() {
    const m = this.status?.manifest?.replace(/^\/sdcard/, '');
    if (m) return m;
    return setPath(this.activeSet);
  },
  get building() { return this.status?.index_state === 'building'; },

  // A file's bytes for a preview: its prepared copy when there is one (the
  // same frames; on the device a ~15-file directory instead of, say, the
  // 3,781-file gifs/Consoles, whose scan the panel would feel), else the
  // file itself. A missing copy costs one 404.
  async previewBytes(path) {
    // A virtual file (a title card, the power-on effect: `@…`) is drawn by
    // the preview's own engine from its path; there is nothing to fetch.
    if (path.startsWith('@')) return new Uint8Array();
    const copy = wasm.cachePathFor(path);
    if (copy) {
      try { return await this.src.readBytes(copy); } catch (e) { if (e.status !== 404) throw e; }
    }
    return this.src.readBytes(path);
  },

  // POST /api/play: the panel shows `path` now, then resumes its schedule.
  async play(path) {
    try {
      await this.src.play(path);
      toast(`on the panel: ${path.startsWith('@fx:psi') ? 'Formula' : base(path)}`, 'ok');
      return true;
    } catch (e) {
      toast(e.status === 404 ? `${path}: not on the card` : e.status === 415 ? `${path}: not a GIF` : 'play: ' + e.message, 'bad');
      return false;
    }
  },

  go(view) { location.hash = '#' + view; },
};
