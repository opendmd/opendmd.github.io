// Small DOM and formatting helpers shared by every view. No framework: `h`
// builds elements, `bus` carries the few app-wide events, `store` wraps
// localStorage (which can throw in a private window).

export const $ = (sel, root = document) => root.querySelector(sel);
// The project site: releases, the HTTP API, formula links.
export const SITE = 'https://opendmd.github.io/';

// h('button.pri', {onclick}, 'Save') — tag with optional .classes, then
// attributes/properties, then children (strings, nodes, arrays, null).
export function h(spec, attrs, ...kids) {
  const [tag, ...cls] = spec.split('.');
  const el = document.createElement(tag || 'div');
  if (cls.length) el.className = cls.join(' ');
  if (attrs && (typeof attrs !== 'object' || attrs instanceof Node || Array.isArray(attrs))) {
    kids.unshift(attrs);
    attrs = null;
  }
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'style' || k.startsWith('data-') || k.startsWith('aria-') || k === 'for') el.setAttribute(k, v);
    else el[k] = v;
  }
  add(el, kids);
  return el;
}

function add(el, kids) {
  for (const k of kids) {
    if (k == null || k === false) continue;
    if (Array.isArray(k)) add(el, k);
    else el.append(k instanceof Node ? k : String(k));
  }
}

export const clear = (el, ...kids) => { el.replaceChildren(); add(el, kids); return el; };

export const bus = new EventTarget();
export const emit = (type, detail) => bus.dispatchEvent(new CustomEvent(type, { detail }));
export const on = (type, fn) => bus.addEventListener(type, (e) => fn(e.detail));

export const store = {
  get(key, fallback) {
    try { const v = localStorage.getItem('odmd.' + key); return v == null ? fallback : JSON.parse(v); } catch { return fallback; }
  },
  set(key, value) {
    try { localStorage.setItem('odmd.' + key, JSON.stringify(value)); return true; } catch { return false; /* private mode, full */ }
  },
};

export function debounce(fn, ms) {
  let t = 0;
  return (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); };
}

export function toast(msg, kind = '') {
  const el = h('div.toast' + (kind ? '.' + kind : ''), String(msg));
  $('#toasts').append(el);
  setTimeout(() => el.remove(), kind === 'bad' ? 8000 : 4000);
}

// Card paths go into URLs one segment at a time: `NEW 2021`, `+` and `#`
// all occur in real file names, and the firmware reads `+` as a space.
export const enc = (path) => path.split('/').map(encodeURIComponent).join('/');
export const base = (p) => p.slice(p.lastIndexOf('/') + 1);
export const dir = (p) => p.slice(0, p.lastIndexOf('/')) || '/';

export function bytes(n) {
  if (n == null) return '–';
  if (n < 1024) return n + ' B';
  if (n < 1048576) return (n / 1024).toFixed(1) + ' KB';
  if (n < 1073741824) return (n / 1048576).toFixed(1) + ' MB';
  return (n / 1073741824).toFixed(1) + ' GB';
}

export function uptime(s) {
  const d = Math.floor(s / 86400), hh = Math.floor(s / 3600) % 24, m = Math.floor(s / 60) % 60;
  return d ? `${d}d ${hh}h` : hh ? `${hh}h ${m}m` : `${m}m ${s % 60}s`;
}

// A manifest's non-blank lines, tolerant like the core (BOM, CRLF, trailing blanks).
export const lines = (text) => text.replace(/^﻿/, '').split('\n').map((l) => l.trimEnd()).filter(Boolean);

// The display name of an entry: "Title 03" plus nothing else; a name with
// no title (or only a number: N64_LOGO_01.gif) reads "N64 Logo 01".
export const label = (e) => ((!e.title || /^\d\d?$/.test(e.title) ? [e.platform, ...(e.flags || []).filter(isFlag).map(flagName), e.title].filter(Boolean).join(' ') : e.title) || base(e.path))
  + (e.variant != null ? ' ' + String(e.variant).padStart(2, '0') : '');

export function confirmBox(msg) { return window.confirm(msg); }

// Human names (values, paths and API calls keep the raw ones): a category
// `BEST_OF_TOP_30` "Best of Top 30", `NEW 2021` as it is; `_x` a system list.
const SMALL = /^(OF|THE|AND|A|AN|IN|ON|TO|FOR)$/;
export const isSys = (c) => c[0] === '_';
export function niceName(c) {
  let s = isSys(c) ? c.slice(1) : c;
  s = s[0].toUpperCase() + s.slice(1);
  if (!s.includes('_')) return s;
  const w = s.split('_').filter(Boolean);
  return (s === s.toUpperCase() ? w.map((x, i) => (i && SMALL.test(x) ? x.toLowerCase() : x[0] + x.slice(1).toLowerCase())) : w).join(' ');
}
// By name, system lists last.
export const byNice = (a, b) => isSys(a) - isSys(b) || niceName(a).localeCompare(niceName(b));

// opendmd_core::manifest KEYWORD_FLAGS + SEASONS (keep in step); others are authors.
export const FLAGS = { INVERT: 'Inverted', INTRO: 'Intro', LOGO: 'Logo', SLIDE: 'Slide', STORY: 'Story', ANIM: 'Animated', STATIC: 'Still', HALLOWEEN: 'Halloween', XMAS: 'Christmas' };
export const isFlag = (f) => f.toUpperCase() in FLAGS;
export const flagName = (f) => FLAGS[f.toUpperCase()] || f;
export const flagCode = (n) => Object.keys(FLAGS).find((k) => FLAGS[k].toLowerCase() === n.toLowerCase()) || n;

// A one-line caption; the rest in a "?" tooltip.
export const note = (text, more) => h('p.cap', text, more && h('span.q', { title: more, tabIndex: 0 }, '?'));

// Settings apply as they change: saver(heading) returns run(fn), which
// runs saves in order (fn returning false: nothing to save) and shows
// "Saved ✓" in the heading, fading out over 2.5 s, or "Couldn't save: why".
export function saver(head) {
  const tick = head.querySelector('.tick') || head.appendChild(h('span.tick'));
  let q, t;
  return (fn) => (q = Promise.resolve(q).then(fn).then((r) => {
    if (r === false) return;
    tick.dataset.n = -~tick.dataset.n;
    tick.className = 'tick';
    void tick.offsetWidth; // restart the fade
    tick.className = 'tick ok';
    tick.textContent = 'Saved ✓';
    clearTimeout(t);
    t = setTimeout(() => { tick.textContent = ''; }, 2500);
  }, (e) => { clearTimeout(t); tick.className = 'tick bad'; tick.textContent = 'Couldn’t save: ' + e.message; }));
}

// Brightness 1..255, the ceiling (Power's maximum) marked and enforced.
export function brightSlider(id, v) {
  const input = h('input', { type: 'range', min: 1, max: 255, id, value: v });
  const mark = h('i'), out = h('output.mono', v);
  let m = 255;
  const ceil = (x = 255) => {
    input.dataset.ceiling = m = x;
    mark.hidden = m > 254;
    mark.style.left = `calc(8px + (100% - 16px) * ${(m - 1) / 254})`;
    if (+input.value > m) input.value = m;
    out.textContent = input.value;
  };
  input.addEventListener('input', () => ceil(m));
  return { el: h('span.bri', h('span.rng', input, mark), out), input, ceil, set: (x) => { input.value = x; ceil(m); } };
}
