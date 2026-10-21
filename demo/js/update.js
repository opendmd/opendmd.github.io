// Settings → Advanced → Firmware: "Check for updates" (imported on the first
// click; its own size line). Reads the published release, `release.json` on
// the project site (HTTPS, which GitHub Pages serves with CORS; `?updates=<url>`
// or localStorage `odmd.updates` points elsewhere, for testing), and compares
// it with the player: the firmware semver-ish (`older`), the web app by its
// `git describe` string (a release pack reads `v1.0.0`, a development build
// `v1.0.0-3-gabc1234-dirty`). Update downloads what is needed into memory
// first and checks its header, so a failed download installs nothing; then
// the firmware (the same PUT and wait as "Update from file…"), then the web
// app (PUT /api/www, docs/www-pack.md), then the page reloads: its code is
// the old web app's.

import { app } from './state.js';
import { h, store } from './util.js';
import { older, fwHead, sendFw, put, restart } from './firmware.js';

export const RELEASE = 'https://opendmd.github.io/install/release.json';
const WWW_MAX = 1310720; // the www partition
const td = new TextDecoder();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const bare = (v) => String(v).replace(/^v/i, '');

// A web app version: [the release it is or builds on (null for a bare
// commit hash), whether it is a development build].
export function wver(s) {
  const m = /^v?(\d+(?:\.\d+)*(?:-(?!\d+-g|dirty$)[0-9A-Za-z.]+)?)((?:-\d+-g[0-9a-f]+)?(?:-dirty)?)$/.exec(s || '');
  return m ? [m[1], !!m[2]] : [null, true];
}
const nice = (s) => { const [b, dev] = wver(s); return dev ? (b ? `${b}, development build` : 'a development build') : b; };

// GET `u` from the release site; with `progress`, the body as a Blob.
// Every failure in words: offline, unreachable (or no CORS: the browser
// cannot tell the page which), a 404, another status, a cut-off download.
async function get(u, what, progress) {
  const host = new URL(u).host;
  try {
    const r = await fetch(u, { cache: 'no-store' });
    if (r.status === 404) throw new Error(`${what} isn’t on ${host} (404)`);
    if (!r.ok) throw new Error(`${host} answered ${r.status} for ${what.toLowerCase()}`);
    if (!progress) return await r.json();
    const n = +r.headers.get('Content-Length') || 0, parts = [], rd = r.body.getReader();
    for (let got = 0; ;) {
      const { done, value } = await rd.read();
      if (done) break;
      parts.push(value);
      progress(n && Math.min(1, (got += value.length) / n));
    }
    return new Blob(parts, { type: 'application/octet-stream' });
  } catch (e) {
    if (e instanceof SyntaxError) throw new Error(`The release information on ${host} is not readable`);
    if (!(e instanceof TypeError)) throw e;
    throw new Error(navigator.onLine === false ? 'This computer is offline. Connect it to the internet and try again'
      : `Couldn’t reach ${host}. Check this computer is online, then try again`);
  }
}

export async function check(ui) {
  const { say, row } = ui;
  if (!app.device) return;
  row.querySelector('#fw-upd')?.remove();
  ui.busy(true);
  say('Checking for updates…');
  try {
    const u = new URL(new URLSearchParams(location.search).get('updates') || store.get('updates', '') || RELEASE, location.href).href;
    const rel = await get(u, 'The release information');
    if (typeof rel?.version !== 'string' || typeof rel.firmware !== 'string') throw new Error(`The release information on ${new URL(u).host} is not readable`);
    // One request at a time to the player.
    const fwv = (await app.src.json('GET', '/api/firmware')).version;
    const wv = (await app.src.json('GET', '/api/www')).version;
    const v = bare(rel.version), fwUp = older(fwv, v), ahead = older(v, fwv);
    // The web app: any other version is an update, unless the firmware is
    // ahead of the release, or this is a development build of the release or
    // later (then the release would be a step back).
    const [wb, dev] = wver(wv), [rb] = wver(rel.www_version);
    const wwwUp = !ahead && !!rel.www && !!rel.www_version && wv !== rel.www_version && !(dev && wb && rb && !older(wb, rb));
    if (!fwUp && !wwwUp) {
      say(ahead ? `This player’s firmware (${bare(fwv)}) is newer than the latest release (${v})` : `Up to date (${v}${dev ? ', development build' : ''})`);
    } else {
      say([fwUp && `Firmware ${v} is out (this player has ${bare(fwv)}).`,
        wwwUp && `Web app ${nice(rel.www_version)} is out (this one is ${nice(wv) || 'unknown'}).`,
        fwUp && 'The panel goes dark for about a minute while it installs.'].filter(Boolean).join(' '));
      row.prepend(h('button.pri', { id: 'fw-upd', onclick: (e) => { e.target.remove(); update(ui, rel, u, fwUp && v, wwwUp); } }, 'Update'));
    }
  } catch (e) { say(e.message, 1); }
  ui.busy(false);
}

async function update(ui, rel, u, fw, www) {
  const { say, bar, barBox } = ui;
  const show = (t, p) => { bar.style.width = `${100 * p}%`; say(`${t}… ${Math.round(100 * p)}%`); };
  let done = '';
  ui.busy(true);
  barBox.hidden = false;
  try {
    // Everything down first: a failure here leaves the player as it was.
    const fb = fw && await get(new URL(rel.firmware, u).href, 'The firmware', (p) => show('Downloading the firmware', p));
    const wb = www && await get(new URL(rel.www, u).href, 'The web app', (p) => show('Downloading the web app', p));
    const head = async (b, n) => new Uint8Array(await b.slice(0, n).arrayBuffer());
    if (fb && fwHead(await head(fb, 140)) !== fw) throw new Error('The firmware download is not the release it should be. Nothing was installed');
    if (wb) {
      const b = await head(wb, 64);
      if (wb.size < 64 || wb.size > WWW_MAX || td.decode(b.subarray(0, 4)) !== 'ODWP' || new DataView(b.buffer).getUint32(8, true) !== wb.size
        || td.decode(b.subarray(20, 20 + b[18])) !== rel.www_version) throw new Error('The web app download is not the release it should be. Nothing was installed');
    }
    if (fb) {
      await sendFw(fb, (p) => show('Sending the firmware to the player', p));
      barBox.hidden = true;
      await restart(fw, say);
      done = `Firmware updated to ${fw}`;
      app.refreshStatus().catch(() => {});
      ui.load();
    }
    if (wb) {
      barBox.hidden = false;
      await sendWww(wb, rel.www_version, show, say);
      say('Updated. Reloading the page…');
      setTimeout(() => location.reload(), 1500);
      return;
    }
    say(done);
  } catch (e) { say(done ? `${done}, but the web app was not: ${e.message}` : e.message, 1); }
  barBox.hidden = true;
  ui.busy(false);
}

// PUT /api/www, a busy player (503) retried; the replies of docs/www-pack.md
// in words, and after a failure whether the old web app is still there.
async function sendWww(b, want, show, say) {
  for (let i = 0; ; i++) {
    const { s, why, text } = await put('/api/www', b, (p) => show('Sending the web app to the player', p));
    if (s === 200) {
      const w = JSON.parse(text);
      if (w.ok && w.version === want) return;
      throw new Error(`The player still reports web app ${nice(w.version) || 'none'}${w.error ? ` (${w.error})` : ''}`);
    }
    if (s === 503 && i < 4) { say('The player is busy, trying again…'); await sleep(3000); continue; }
    let m = s === 400 ? `The player refused the web app: ${why}` : s === 408 ? 'The upload was cut short'
      : s === 413 ? 'The web app is too big for this player' : s === 422 ? `The web app arrived damaged (${why})`
        : s === 500 ? `The player couldn’t write the web app (${why})` : `Couldn’t update the web app (${s}: ${why})`;
    try {
      m += (await app.src.json('GET', '/api/www')).ok ? '. The old web app is still in place; try again'
        : `. The player has no web app until one goes through: keep this page open and ${s === 500 ? 'restart the player (it finishes from the card) or ' : ''}try again`;
    } catch { /* the reason alone */ }
    throw new Error(m);
  }
}
