// The card format dialog (Now, when the panel can't read its card: exFAT,
// as most cards over 32 GB come). Imported when first opened; its own size
// line. POST /api/card/format, then /api/status polled until sd.formatting
// is over: sd.readable says how it went, sd.format_error why not.

import { app } from './state.js';
import { h } from './util.js';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function formatCard() {
  if (document.getElementById('fmt-dlg')) return;
  const msg = h('p.cap', { id: 'fmt-msg' });
  const prog = h('progress', { id: 'fmt-prog', hidden: true, style: 'width:100%' });
  const go = h('button.danger', { id: 'fmt-go', onclick: run }, 'Format card');
  const close = h('button', { id: 'fmt-close', onclick: () => d.close() }, 'Cancel');
  const d = h('dialog.card', { id: 'fmt-dlg', style: 'color:inherit;border:0;margin:auto;width:min(400px,90vw)' },
    h('h2', 'Format this card?'),
    h('p', 'Everything on it is erased. Cards over 32 GB usually come formatted for computers (exFAT); the panel needs FAT32.'),
    prog, msg, h('p.row', { style: 'margin-top:12px' }, go, close));
  let busy = false;
  d.oncancel = (e) => busy && e.preventDefault();
  d.onclose = () => d.remove();
  const say = (t, bad) => { msg.textContent = t; msg.className = bad ? 'cap bad' : 'cap'; };
  document.body.append(d);
  d.showModal();

  async function run() {
    busy = go.disabled = close.disabled = true;
    prog.hidden = false;
    say('Formatting… this can take a minute');
    try {
      await app.src.json('POST', '/api/card/format', { confirm: true });
      for (const t = Date.now(); ;) {
        await sleep(1000);
        let sd;
        try { sd = (await app.src.status()).sd; } catch { continue; }
        if (!sd.formatting) {
          if (!sd.readable) throw new Error(`The card couldn’t be formatted: ${sd.format_error || sd.error || 'it still can’t be read'}`);
          break;
        }
        say(`Formatting… ${Math.round((Date.now() - t) / 1000)} s`);
        if (Date.now() - t > 300000) throw new Error('The card is still formatting after 5 minutes. Check the panel, then reload this page');
      }
      go.hidden = true;
      say('Done: the card is ready. Add GIFs from the Library.');
      app.refreshStatus().catch(() => {});
    } catch (e) { say(e.message, 1); go.disabled = false; }
    busy = close.disabled = false;
    close.textContent = 'Close';
    prog.hidden = true;
  }
}
