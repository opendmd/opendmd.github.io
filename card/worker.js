// One of the page's writers: it gets the zip (a File) and the card's folder
// (a FileSystemDirectoryHandle) once, then batches of work. For each GIF:
// inflate it from the zip and check its CRC, write it, make its lighter
// version with the web app's own encoder (prepareGif, the one the player
// and opendmd-pack run: the same bytes) and write that. Each write goes
// through createWritable, which writes a swap file and moves it into place
// on close(): an interrupted run leaves no half-written file under the
// real name (but may leave an empty one, or a .crswap, which the next run's
// listing deals with). The bytes never leave this thread.

import { entryBytes, ZipError } from './zip.js';
import { playsAsIs } from './plan.js';

let zip, root, prepareGif, stopping = false, pace = 0;
const dirs = new Map();

async function dirFor(parts) {
  let d = root, key = '';
  for (const part of parts) {
    key += '/' + part.toLowerCase();
    const known = dirs.get(key);
    if (known) { d = known; continue; }
    // Another writer may be making the same folder at the same moment.
    for (let tries = 0; ; tries++) {
      try { d = await d.getDirectoryHandle(part, { create: true }); break; } catch (e) {
        if (tries >= 3) throw e;
        await new Promise((r) => setTimeout(r, 30 * (tries + 1)));
      }
    }
    dirs.set(key, d);
  }
  return d;
}

async function writeFile(path, bytes) {
  const parts = path.split('/');
  const name = parts.pop();
  const d = await dirFor(parts);
  const fh = await d.getFileHandle(name, { create: true });
  const w = await fh.createWritable();
  try {
    await w.write(bytes);
    await w.close();
  } catch (e) {
    await w.abort().catch(() => {});
    throw e;
  }
}

// A write, retried once; the card being full or gone is not retried.
const FATAL = new Set(['QuotaExceededError', 'NotFoundError', 'NotAllowedError', 'SecurityError', 'InvalidStateError']);
async function write(path, bytes) {
  try { await writeFile(path, bytes); } catch (e) {
    if (FATAL.has(e?.name)) throw e;
    dirs.clear();
    await new Promise((r) => setTimeout(r, 200));
    await writeFile(path, bytes);
  }
}

// The zip itself can't be read any more (moved, renamed, deleted or
// changed on disk after it was dropped): nothing after this can work.
class ZipGone extends Error { constructor(m) { super(m); this.name = 'ZipGone'; } }

async function one(it) {
  const r = { path: it.path, gif: 0, copy: 0 };
  let bytes;
  try { bytes = await entryBytes(zip, it.e); } catch (e) {
    if (e instanceof ZipError) {
      if (e.kind === 'unsupported') r.unsupported = e.message; else r.bad = e.message;
      return r;
    }
    throw new ZipGone(`${e?.name || 'Error'}: ${e?.message ?? e}`);
  }
  if (it.writeGif) { await write(it.path, bytes); r.gif = bytes.length; }
  if (it.copy) {
    let copy = null;
    try { copy = prepareGif(bytes).copy; } catch (e) {
      const why = String(e?.message ?? e);
      // The encoder's own verdict on the file is a plain Error (its JsError):
      // past 256 colours, or nothing can decode it. Anything else (a wasm
      // trap, out of memory) is this writer's trouble: retried next run, and
      // this writer is replaced.
      if (e?.name === 'Error') {
        if (playsAsIs(why)) r.asIs = why; else r.damaged = why;
      } else {
        r.failed = `${e?.name || 'Error'}: ${why}`;
        r.dead = true;
      }
    }
    if (copy) { await write(it.copy, copy); r.copy = copy.length; }
  }
  if (pace) await new Promise((res) => setTimeout(res, pace));
  return r;
}

self.onmessage = async ({ data: m }) => {
  if (m.type === 'init') {
    zip = m.zip; root = m.root; pace = m.pace || 0;
    try {
      const art = await import('../demo/pkg-art/opendmd_art_web.js');
      await art.default();
      prepareGif = art.prepareGif;
      self.postMessage({ type: 'ready' });
    } catch (e) {
      self.postMessage({ type: 'batch', done: [], fatal: { name: 'LoadError', message: String(e?.message ?? e) } });
    }
  } else if (m.type === 'stop') {
    stopping = true;
  } else if (m.type === 'work') {
    // One message back per batch: what was done, and whether this writer
    // can go on (`dead`) or the run can't (`fatal`). Items not in `done` were
    // not started.
    const done = [];
    let fatal = null, dead = false;
    for (const it of m.items) {
      if (stopping) break;
      try {
        const r = await one(it);
        done.push(r);
        if (r.dead) { dead = true; break; }
      } catch (e) {
        if (FATAL.has(e?.name) || e?.name === 'ZipGone') {
          fatal = { name: e.name, message: String(e.message), path: it.path };
          break;
        }
        done.push({ path: it.path, gif: 0, copy: 0, failed: `${e?.name || 'Error'}: ${e?.message ?? e}` });
      }
    }
    self.postMessage({ type: 'batch', done, fatal, dead });
  }
};
