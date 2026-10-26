// Adding a GIF to the card and to a playlist: shared by the Library, its
// dialogs (js/details.js) and Create Art (js/studio.js), which so need not
// load the Library's tree and thumbnails.

import { app, playlistLabel } from './state.js';
import { prepareBytes } from './prepare.js';
import { emit, toast, lines, isFlag } from './util.js';

export const authorOf = (e) => e.flags.find((f) => !isFlag(f)) || '';

export async function addOne(category, name, bytes, warn, prep = prepareBytes) {
  const path = `/gifs/${category}/${name}`;
  await app.src.writeFile(path, bytes);
  import('./thumbs.js').then((m) => m.forget(path)); // its thumbnail, if one was kept
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

