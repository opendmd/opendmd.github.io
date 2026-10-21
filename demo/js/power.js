// Settings → Power (imported by settings.js; its own size line): the
// Maximum brightness setting, which is also the ceiling marked on the
// brightness sliders, and a warning above the tested level. Saved when the
// field is left (a lower maximum lowers the brightness with it).

import { app } from './state.js';
import { h, on, saver } from './util.js';

export function powerCard(card) {
  const num = h('input', { type: 'number', min: 16, max: 255, id: 'st-maxbright', style: 'width:100px' });
  const safe = () => app.status?.brightness_safe_max;
  const q = h('span.q', { tabIndex: 0 }, '?');
  const note = h('p.cap', { id: 'st-maxbright-note' }, 'Limits how bright the panel can go', q);
  const warn = h('div.banner', { id: 'st-maxbright-warn', hidden: true, style: 'margin:12px 0 0' });
  const head = h('h2', 'Power'), run = saver(head);
  function show() {
    const s = safe();
    q.title = `Above ${s ?? 'the tested level'} the panel can reset on the supplied power supply, especially on white screens.`;
    warn.hidden = !(s && +num.value > s);
    warn.textContent = `Above the tested limit (${s}). The display may reset. Only raise this with a stronger power supply`;
  }
  function sync(c) {
    if (document.activeElement !== num) num.value = c.max_brightness ?? 255;
    show();
  }
  num.onchange = () => run(async () => {
    const v = +num.value;
    if (v === app.config.max_brightness) return false;
    if (!(v >= 16 && v <= 255)) throw new Error('maximum brightness must be 16 to 255');
    await app.src.putConfig({ max_brightness: v });
    await app.refreshConfig();
  });
  num.oninput = show;
  on('config', sync);
  on('status', show);
  card.append(head, note, h('div.form.f2', h('label.stack', 'Maximum brightness', num)), warn);
  sync(app.config);
}
