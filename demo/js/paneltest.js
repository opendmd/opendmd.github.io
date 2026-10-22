// Settings › Device › Panel timing › Test panel timing (its own size
// line, imported on the first click): POST /api/panel/test shows the
// device's test pictures with one live setting switching between A and B;
// the pick is saved through /api/config. The device ends a test after
// 120 s, so an open box starts it again every 100 s; leaving the Device
// tab or Settings ends it (`app.endPanelTest`, called by device.js).

import { app } from './state.js';
import { auth } from './source.js';
import { h, clear, on, toast } from './util.js';

const post = (b) => app.src.json('POST', '/api/panel/test', b);
const L = { clk_phase: ['Clock edge', (v) => (v ? 'Rising' : 'Falling')], latch_blanking: ['Latch blanking', (v) => v] };
let close = null;
// A trial started elsewhere ends the test on the device: close the box.
on('config', (c) => c?.panel_trial_s != null && close?.());
addEventListener('pagehide', () => close && fetch('/api/panel/test', {
  method: 'POST', keepalive: true, headers: auth({ 'Content-Type': 'application/json' }), body: '{"stop":true}',
}));

export function panelTest(box) {
  let again = 0;
  const say = (...k) => { clearInterval(again); close = app.endPanelTest = null; box.hidden = !k[0]; clear(box, ...k); };
  const b = (t, fn) => h('button.sm', { onclick: fn }, t);
  const stop = (m) => { say(m && h('p', m)); post({ stop: true }).catch(() => {}); };
  async function run(k, more) {
    say();
    let r;
    try { r = await post({ setting: k }); } catch (e) { return toast('panel test: ' + e.message, 'bad'); }
    // Saved at once, no trial: both values have just been seen side by side.
    // The save ends the test on the device.
    const pick = (s) => async () => {
      const v = r[s][k], [n, f] = L[k];
      say();
      try { await app.src.putConfig({ panel: { [k]: v }, panel_confirm: true }); } catch (e) { return stop(), toast('panel timing: ' + e.message, 'bad'); }
      say(h('p', `Saved: ${n} ${f(v)}.`));
      app.refreshConfig().catch(() => {});
    };
    say(h('p', (more ? 'One more setting. ' : '') + 'Watch the panel. Does A or B look cleaner?'),
      h('p.cap', 'Each picture shows under A, then B: the letter is in the bottom corner.'),
      h('div.row', b('A looks clean', pick('a')), b('B looks clean', pick('b')),
        b('Both look the same / fine', () => stop('Nothing changed.')),
        b('Both have dots', () => (k === 'clk_phase' ? run('latch_blanking', 1)
          : stop('Neither helped. Try Driver chip below, then switch the player off and on. If the dots stay, it may be the panel.'))),
        b('Stop', () => stop())));
    close = () => say();
    app.endPanelTest = () => stop();
    again = setInterval(() => post({ setting: k }).catch(() => {}), 100000);
  }
  // These chips always use the rising edge: only the blanking can change.
  run(['mbi5124', 'dp3246'].includes(app.config?.panel_now?.driver) ? 'latch_blanking' : 'clk_phase');
}
