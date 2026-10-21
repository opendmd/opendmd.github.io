// A virtualised list: fixed row height, a pool of rows re-rendered on scroll.

import { h } from './util.js';

export class VList {
  // onMenu(item, index, x, y): right-click, or a long press on a touch screen.
  constructor({ rowHeight = 40, render, onClick, onMenu }) {
    this.rh = rowHeight;
    this.render = render;
    this.items = [];
    this.inner = h('div');
    this.el = h('div.vlist', { onscroll: () => this.draw() }, this.inner);
    this.pool = [];
    let pressed = 0, t0 = null;
    const fire = (row, x, y) => { pressed = Date.now(); onMenu(this.items[row._i], row._i, x, y); };
    this.el.addEventListener('click', (e) => {
      const row = e.target.closest('.vrow');
      if (Date.now() - pressed < 700) return; // the tap that ended a long press
      if (row && onClick) onClick(this.items[row._i], row._i, e);
    });
    if (onMenu) {
      this.el.addEventListener('contextmenu', (e) => {
        const row = e.target.closest('.vrow');
        if (!row) return;
        e.preventDefault();
        if (Date.now() - pressed > 700) fire(row, e.clientX, e.clientY);
      });
      // iOS has no contextmenu on a long press.
      let timer = 0;
      const cancel = () => clearTimeout(timer);
      this.el.addEventListener('pointerdown', (e) => {
        const row = e.target.closest('.vrow');
        if (e.pointerType !== 'touch' || !row || e.target.closest('button,input')) return;
        t0 = [e.clientX, e.clientY];
        timer = setTimeout(() => fire(row, t0[0], t0[1]), 550);
      });
      this.el.addEventListener('pointermove', (e) => { if (t0 && Math.hypot(e.clientX - t0[0], e.clientY - t0[1]) > 10) cancel(); });
      for (const ev of ['pointerup', 'pointercancel', 'scroll']) this.el.addEventListener(ev, cancel);
    }
    new ResizeObserver(() => this.draw()).observe(this.el);
  }

  set(items) {
    this.items = items;
    this.inner.style.height = items.length * this.rh + 'px';
    this.draw(true);
  }

  refresh() { this.draw(true); }

  scrollTo(i) {
    const top = i * this.rh;
    if (top < this.el.scrollTop || top > this.el.scrollTop + this.el.clientHeight - this.rh) this.el.scrollTop = top - this.rh * 2;
  }

  draw(force) {
    const first = Math.max(0, Math.floor(this.el.scrollTop / this.rh) - 4);
    const count = Math.ceil((this.el.clientHeight || 600) / this.rh) + 8;
    const last = Math.min(this.items.length, first + count);
    if (!force && first === this.first && last === this.last) return;
    this.first = first; this.last = last;
    while (this.pool.length < last - first) {
      const r = h('div.vrow', { style: `height:${this.rh}px` });
      this.pool.push(r);
      this.inner.append(r);
    }
    this.pool.forEach((r, k) => {
      const i = first + k;
      if (i >= last) { r.hidden = true; return; }
      r.hidden = false;
      r._i = i;
      r.style.transform = `translateY(${i * this.rh}px)`;
      this.render(r, this.items[i], i);
    });
  }
}
