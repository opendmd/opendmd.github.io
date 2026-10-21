// Follow RePlayOS: Settings' card (host, the NET CONTROL code — sent, never
// shown back —, on/off, the attract playlist, a Test button) and the line
// Now shows under the title while it follows ("Following RePlayOS · Mega
// Drive · Sonic the Hedgehog 3 · 14 matching", or the sequence step).

import { app } from './state.js';
import { h, clear, on, note, saver, debounce } from './util.js';

const STEPS = { power_on: 'powering on', logo: 'console logo', title: 'title card', waiting: 'title card' };

// The Now line for status `context`, or null when not following.
export function followLine(s) {
  const c = s?.context;
  if (!c?.enabled) return null;
  const game = [c.console, c.game].filter(Boolean).join(' · ');
  switch (c.state) {
    case 'unreachable': return `RePlayOS not reachable${c.error ? ` (${c.error})` : ''} · playing as usual`;
    case 'attract': return 'Following RePlayOS · attract';
    case 'starting': return `Following RePlayOS · ${game} · starting: ${STEPS[c.step] ?? c.step}`;
    case 'game': return `Following RePlayOS · ${game}${c.matches != null ? ` · ${c.matches.toLocaleString()} matching` : ''}`;
    case 'cooldown': return `Following RePlayOS · ${game} · game over, back to attract soon`;
    default: return 'Following RePlayOS · connecting…';
  }
}

// Settings' card: {el}.
export function followCard() {
  const c = app.config || {};
  const on_ = h('input', { type: 'checkbox', id: 'fo-on', checked: !!c.follow });
  const host = h('input', { type: 'text', id: 'fo-host', value: c.follow_host ?? '', placeholder: '192.168.0.20', autocomplete: 'off' });
  const code = h('input', { type: 'password', id: 'fo-code', autocomplete: 'new-password' });
  const attract = h('select', { id: 'fo-attract' });
  const state = h('div.muted', { id: 'fo-state' });
  const result = h('div.muted', { id: 'fo-result' });
  const setPh = () => { code.placeholder = app.config?.follow_code_set ? 'unchanged (one is saved)' : 'NET CONTROL CODE'; };
  setPh();

  async function playlists() {
    const cur = app.config?.attract_set ?? null, keep = attract.value;
    const lists = await app.playlists().catch(() => []);
    clear(attract, h('option', { value: '*' }, 'The current playlist'), h('option', { value: '@' }, 'The clock'),
      lists.map((p) => h('option', { value: p.value }, p.label)));
    if (cur != null && ![...attract.options].some((o) => o.value === cur)) attract.append(h('option', { value: cur }, cur || 'All files'));
    attract.value = app.config?.attract == 'clock' ? '@' : cur ?? '*';
    if (keep && [...attract.options].some((o) => o.value === keep)) attract.value = keep;
  }
  playlists();
  // (The fields are the user's: a save in flight must not undo a later change.)
  on('config', () => { setPh(); playlists(); });
  on('status', (s) => { state.textContent = followLine(s) ?? 'Not following'; });

  // Saved as it changes: the switch and the playlist at once, the address
  // when typing settles, the code when the field is left (then cleared).
  const head = h('h3', 'Follow RePlayOS'), run = saver(head);
  const save = () => {
    const a = attract.value;
    const patch = { follow: on_.checked, follow_host: host.value.trim(), attract_set: a == '*' || a == '@' ? null : a, attract: a == '@' ? 'clock' : '' };
    if (code.value) patch.follow_code = code.value;
    return run(async () => {
      if (patch.follow && !patch.follow_host) throw new Error('enter the RePlayOS box’s address first');
      await app.src.putConfig(patch);
      code.value = '';
      await app.refreshConfig();
    });
  };
  on_.onchange = attract.onchange = code.onchange = save;
  host.oninput = debounce(save, 400);

  async function test() {
    result.className = 'muted';
    result.textContent = 'asking…';
    const body = { host: host.value.trim() };
    if (code.value) body.code = code.value;
    try {
      const r = await app.src.json('POST', '/api/follow/test', body, 25000);
      result.className = r.ok ? 'ok' : 'bad';
      result.textContent = r.ok
        ? `RePlayOS ${r.version || ''} answered · ${r.view === 'game_play' ? `playing ${r.game || r.system}` : r.view ? `in ${r.view.replace('_', ' ')}` : 'ok'}`
        : !r.reachable ? `No answer from ${body.host || 'the saved address'} (${r.error})`
          : `RePlayOS ${r.version || ''} answered, but: ${r.error}`;
    } catch (e) { result.className = 'bad'; result.textContent = 'test: ' + e.message; }
  }

  const el = h('div.grp', { id: 'fo' }, head,
    note('Show what your RePlayOS box is playing', 'When your RePlayOS box starts a game the panel shows the console’s logo and a title card, then art from that game and console. In the menus it plays the attract playlist. Turn on NET CONTROL on the box and copy its code from System → Information.'),
    h('div.form.f2',
      h('label', on_, 'Follow RePlayOS'),
      h('label.stack', 'RePlayOS address', host),
      h('label.stack', 'NET CONTROL code', code),
      h('label.stack', 'Attract playlist', attract)),
    h('div.row', { style: 'margin-top:12px' }, h('button', { id: 'fo-test', onclick: test }, 'Test')),
    result, state);
  state.textContent = followLine(app.status) ?? 'Not following';
  return { el };
}
