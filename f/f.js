// The share page (/f/#formula=…&name=…&a=…): one formula, playing in the
// web app's LED preview, its code, its sliders, and the ways on: open it on
// my display, try it in the demo, copy, submit to the gallery, a QR code.
// The demo's files do the work: the share link's encoder and decoder
// (js/psikit.js, the app's own), the LED preview (js/preview.js and its
// wasm) and Psi (pkg-psi), so a frame here is the panel's and looks like
// the app's. Everything a link brings is shown as text, never as HTML.

import init from '../demo/pkg/opendmd_web.js';
import { link, parse, sliderName, about, TIP, marks } from '../demo/js/psikit.js';
import { qr } from './qr.js';

const $ = (id) => document.getElementById(id);
const store = {
  get(k) { try { return localStorage.getItem('odmd.' + k); } catch { return null; } },
  set(k, v) { try { localStorage.setItem('odmd.' + k, v); } catch { /* private mode */ } },
};
// The gallery's repository, where Submit opens GitHub's "new file" page.
const REPO = 'https://github.com/opendmd/opendmd.github.io';
// GitHub refuses very long addresses: past this, the file is copied instead.
const MAX_URL = 6000;
const D = 20; // the panel's frame: 50 a second

const { f, cut, has } = parse(location.hash);
// Another link pasted into this tab: start again with it.
addEventListener('hashchange', () => location.reload());
const here = () => new URL('./', location.href).href;

if (!has || !f.src.trim()) {
  $('f-none').hidden = false;
} else {
  start().catch((e) => { $('f-cap').textContent = 'This page could not load: ' + (e.message || e); });
}

async function start() {
  $('f').hidden = false;
  $('f-name').textContent = f.name;
  document.title = `${f.name} · OpenDMD`;
  if (cut) $('f-cap').textContent = 'This link was too long: only the first 4,096 characters of the formula are here.';

  await init();
  const [{ PanelPreview }, P] = await Promise.all([import('../demo/js/preview.js'), import('../demo/pkg-psi/opendmd_psi_web.js')]);
  const mem = (await P.default()).memory;
  const ok = P.compile(f.src);
  const i = ok ? P.info() : [];
  const bad = ok ? null : P.errorAt();
  $('f-code').innerHTML = marks(f.src, bad).html + '\n'; // marks() escapes the text
  const mask = i[5] || 0;
  if (ok) {
    $('f-cap').textContent = (cut ? $('f-cap').textContent + ' ' : '') + about(i);
    $('f-cap').title = TIP(i);
  } else {
    const line = f.src.slice(0, bad[0]).split('\n').length;
    $('f-cap').textContent = `This formula doesn’t work: line ${line}: ${P.error().replace(/`/g, '')}`;
    $('f-cap').classList.add('bad');
  }

  // The preview: the frame at a whole number of 20 ms frames into the loop.
  const pv = new PanelPreview($('f-panel'));
  pv.p.setTransition('none', 0);
  const q = (v) => Math.round((v || 0) * 65536);
  const n = Math.max(1, Math.floor((i[4] || 8000) / D));
  let t0 = performance.now(), last = -1, dirty = true;
  const tick = (now) => {
    const k = Math.floor((now - t0) / D) % n;
    if (ok && (k !== last || dirty)) {
      last = k; dirty = false;
      const d = new Date();
      P.frame(k * D, d.getHours(), d.getMinutes(), d.getSeconds(), q(f.a), q(f.b), q(f.c));
      pv.p.showRgb(new Uint8Array(mem.buffer, P.pixels(), 12288));
      $('f-panel').dataset.frames = -~$('f-panel').dataset.frames;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  // Sliders for the a, b, c it reads.
  for (const [j, k] of ['a', 'b', 'c'].entries()) {
    if (!(mask & (1 << j))) continue;
    const out = document.createElement('output');
    const r = Object.assign(document.createElement('input'), { type: 'range', min: 0, max: 1, step: 0.01, value: f[k] || 0, id: 'f-sl-' + k });
    out.textContent = (+r.value).toFixed(2);
    r.oninput = () => { f[k] = +r.value; out.textContent = f[k].toFixed(2); dirty = true; links(); };
    const lab = document.createElement('label'), name = document.createElement('span');
    name.textContent = sliderName(f.src, k);
    lab.append(name, r, out);
    $('f-sl').append(lab);
  }

  // The links: this page, the demo, the display (the device's own share
  // links open the formula in its editor: `#formula=…`).
  const own = () => link(f, here(), mask);
  const hash = () => own().slice(here().length);
  let qrFor = '';
  function links() {
    $('f-demo').href = new URL('../demo/', location.href).href + hash();
    clearTimeout(links.t);
    links.t = setTimeout(() => { if (own() !== qrFor) drawQr(qrFor = own()); }, 300);
    sub();
  }

  // Open on my display: asked once, then remembered.
  const addr = () => store.get('display');
  const where = () => { const a = addr(); $('f-where').hidden = !a; $('f-where-addr').textContent = a || ''; };
  where();
  const go = (a) => { location.href = `http://${a}/${hash()}`; };
  $('f-open').onclick = () => { if (addr()) go(addr()); else ask(); };
  $('f-change').onclick = ask;
  function ask() {
    $('f-ask').hidden = false;
    $('f-addr').value = addr() || 'opendmd.local';
    $('f-addr').focus();
    $('f-addr').select();
  }
  $('f-ask').onsubmit = (e) => {
    e.preventDefault();
    // A host name or IPv4 address (an IPv6 one in brackets), maybe a port;
    // `http://` and a trailing slash typed with it are fine.
    const a = $('f-addr').value.trim().replace(/^https?:\/\//i, '').replace(/\/+$/, '');
    const okAddr = /^(\[[0-9a-f:.]+\]|[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)*)(:\d{1,5})?$/i.test(a);
    $('f-addr-err').hidden = okAddr;
    $('f-addr-err').textContent = okAddr ? '' : 'That doesn’t look like an address: a name like opendmd.local, or numbers like 192.168.1.50.';
    if (!okAddr) return;
    // On this network: a .local name, a name with no dots, a private or
    // link-local address. Anything else could be anyone's: ask once.
    const host = a.replace(/:\d+$/, '').toLowerCase();
    const near = /\.local$/.test(host) || /^[^[.]+$/.test(host) || /^(10\.|192\.168\.|169\.254\.|127\.|172\.(1[6-9]|2\d|3[01])\.)/.test(host)
      || /^\[(fe80|fc|fd|::1\])/.test(host);
    if (!near && $('f-ask').dataset.sure !== a) {
      $('f-ask').dataset.sure = a;
      $('f-addr-err').hidden = false;
      $('f-addr-err').textContent = `${a} doesn’t look like a display on your own network (like opendmd.local or 192.168.1.50): the formula would go to that site. Press Open again if you’re sure.`;
      return;
    }
    store.set('display', a);
    where();
    go(a);
  };

  const flash = (btn, text) => { const was = btn.textContent; btn.textContent = text; setTimeout(() => { btn.textContent = was; }, 1500); };
  $('f-copy-code').onclick = async (e) => flash(e.currentTarget, (await copy(f.src)) ? 'Copied' : 'Select and copy it');
  $('f-copy-link').onclick = async (e) => flash(e.currentTarget, (await copy(own())) ? 'Copied' : 'Select and copy it');

  // The QR code: this page's link (with the sliders as they are).
  function drawQr(url) {
    const m = qr(url), box = $('f-qr');
    box.replaceChildren();
    $('f-qr-cap').textContent = m ? 'Scan to open this page on your phone.' : 'This link is too long for a QR code: copy it instead.';
    if (!m) return;
    const NS = 'http://www.w3.org/2000/svg', s = m.length + 8;
    const svg = document.createElementNS(NS, 'svg'), path = document.createElementNS(NS, 'path');
    svg.setAttribute('viewBox', `0 0 ${s} ${s}`);
    svg.setAttribute('role', 'img');
    svg.setAttribute('aria-label', 'QR code of this page’s link');
    let d = '';
    m.forEach((row, y) => row.forEach((dark, x) => { if (dark) d += `M${x + 4} ${y + 4}h1v1h-1z`; }));
    path.setAttribute('d', d);
    svg.append(path);
    box.append(svg);
  }

  // Submit to the gallery: the file (the gallery's header, then the
  // formula), checked here as the gallery's checker will, then GitHub's
  // "new file" page with it filled in.
  const slug = (s) => s.normalize('NFKD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 48).replace(/-+$/, '');
  // The characters the checker refuses (controls, invisible, direction-changing, private use).
  const BAD = /[\0-\x08\x0a-\x1f\x7f-\xa0\xad\u034f\u061c\u115f\u1160\u17b4\u17b5\u180b-\u180e\u2000-\u200f\u2028-\u202f\u205f-\u206f\u3000\u3164\ufe00-\ufe0f\ufeff\uffa0\ufff0-\uffff\ue000-\uf8ff\udb40-\udb43\udb80-\udbff]/;
  let touched = false;
  $('s-name').value = f.name.slice(0, 40);
  $('s-author').value = store.get('author') || '';
  $('s-file').value = slug(f.name);
  for (const id of ['s-name', 's-desc', 's-author', 's-lic', 's-file']) $(id).addEventListener('input', () => { if (id === 's-file') touched = true; sub(); });
  $('s-name').addEventListener('input', () => { if (!touched) $('s-file').value = slug($('s-name').value); sub(); });
  $('s-author').addEventListener('change', () => store.set('author', $('s-author').value.trim()));
  function file() {
    const vals = ['a', 'b', 'c'].filter((k, j) => mask & (1 << j) && f[k]).map((k) => `${k}=${+f[k].toFixed(2)}`).join(' ');
    const src = f.src.replace(/\r\n?/g, '\n').replace(/\n*$/, '\n');
    const v = (id) => $(id).value.trim().replace(/\s+/g, ' ');
    return [`-- name: ${v('s-name')}`, `-- description: ${v('s-desc')}`, `-- author: ${v('s-author')}`,
      `-- licence: ${$('s-lic').value}`, vals && `-- values: ${vals}`].filter(Boolean).join('\n') + '\n' + src;
  }
  function problems() {
    const out = [], fields = [['name', 's-name', 40], ['description', 's-desc', 120], ['author', 's-author', 40]];
    for (const [what, id, max] of fields) {
      const v = $(id).value.trim();
      if (!v) out.push(`Give it a ${what === 'author' ? 'name or handle for you' : what}.`);
      else if ([...v].length > max) out.push(`The ${what} can be ${max} characters at most.`);
      if (/[<>]/.test(v)) out.push(`The ${what} can’t contain < or >.`);
      if (BAD.test(v)) out.push(`The ${what} has an invisible or special character in it.`);
    }
    if (!/^[a-z0-9]+(-[a-z0-9]+)*$/.test($('s-file').value) || ['readme', 'index'].includes($('s-file').value)) out.push('The file name can be lower-case letters, digits and single hyphens.');
    if (!ok) out.push('The formula has to work first.');
    else if (i[6] > 8000) out.push('This formula is too heavy for the panel: the gallery takes ones under 8 ms a frame.');
    if (f.src.replace(/\r\n?/g, '\n').split('\n').some((l) => BAD.test(l.replace(/\t/g, '')))) out.push('The formula has an invisible or special character in it.');
    if (new TextEncoder().encode(file()).length > 4096) out.push('The file would be over 4 KB: the gallery takes shorter formulas.');
    return out;
  }
  function sub() {
    if ($('f-sub').hidden) return;
    const text = file(), ps = problems();
    $('s-text').textContent = text;
    $('s-err').textContent = ps.join(' ');
    const url = `${REPO}/new/main/gallery?filename=${encodeURIComponent($('s-file').value + '.psi')}&value=${encodeURIComponent(text)}`;
    const long = url.length > MAX_URL;
    $('s-go').href = ps.length ? '#' : long ? `${REPO}/new/main/gallery?filename=${encodeURIComponent($('s-file').value + '.psi')}` : url;
    $('s-go').toggleAttribute('aria-disabled', ps.length > 0);
    $('s-long').hidden = !long;
  }
  $('s-go').onclick = (e) => { if (problems().length) e.preventDefault(); };
  $('s-copy').onclick = async (e) => flash(e.currentTarget, (await copy(file())) ? 'Copied' : 'Select and copy it');
  const openSub = () => { $('f-sub').hidden = false; sub(); };
  $('f-submit').onclick = () => { openSub(); $('f-sub').scrollIntoView({ behavior: 'smooth', block: 'start' }); $('s-desc').focus({ preventScroll: true }); };
  if (/(^#|&)submit(=|&|$)/.test(location.hash)) openSub();
  links();
}

async function copy(text) {
  try { await navigator.clipboard.writeText(text); return true; } catch { /* the old way */ }
  const t = Object.assign(document.createElement('textarea'), { value: text, readOnly: true });
  t.style.cssText = 'position:fixed;opacity:0';
  document.body.append(t);
  t.select();
  const done = document.execCommand('copy');
  t.remove();
  return done;
}
