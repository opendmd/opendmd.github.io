// A virtualised list: fixed row height, a pool of rows re-rendered on scroll.
// A row is `.vrow` (an item) or `.vdir` (a folder, js/tree.js); render sets it.

import { h } from './util.js';

const ROW = '.vrow,.vdir';

export class VList {
  // onMenu(item, index, x, y): right-click, or a long press on a touch
  // screen — unless there is onHold(item, index), which the long press is then.
  // Rows take the focus; Enter is a click.
  constructor({ rowHeight = 40, render, onClick, onMenu, onHold }) {
    this.rh = rowHeight;
    this.render = render;
    this.items = [];
    this.inner = h('div');
    this.el = h('div.vlist', { onscroll: () => this.draw() }, this.inner);
    this.pool = [];
    let pressed = 0, t0 = null;
    // A touch's long press may come as a contextmenu too (Android): once.
    let touch = false;
    const fire = (row, x, y, menu) => {
      if (Date.now() - pressed < 700) return;
      pressed = Date.now();
      (!menu && onHold || onMenu)(this.items[row._i], row._i, x, y);
    };
    this.el.addEventListener('click', (e) => {
      const row = e.target.closest(ROW);
      if (Date.now() - pressed < 700) return; // the tap that ended a long press
      if (row && onClick) onClick(this.items[row._i], row._i, e);
    });
    this.el.addEventListener('keydown', (e) => { if (e.key == 'Enter' && e.target.matches(ROW)) e.target.click(); });
    if (onMenu) {
      this.el.addEventListener('contextmenu', (e) => {
        const row = e.target.closest(ROW);
        if (!row) return;
        e.preventDefault();
        fire(row, e.clientX, e.clientY, !touch);
      });
      // iOS has no contextmenu on a long press.
      let timer = 0;
      const cancel = () => clearTimeout(timer);
      this.el.addEventListener('pointerdown', (e) => {
        const row = e.target.closest(ROW);
        touch = e.pointerType === 'touch';
        if (!touch || !row || e.target.closest('button,input')) return;
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
      const r = h('div.vrow', { style: `height:${this.rh}px`, tabIndex: 0 });
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
