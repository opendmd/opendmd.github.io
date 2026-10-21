// The second wasm module (pkg-art/: the Studio's pixel work and the
// prepared-copy encoder, kept out of the module every page load fetches).
// Loaded and initialised once, on first use, whoever asks first.
let mod = null;

export function art() {
  mod ??= import('../pkg-art/opendmd_art_web.js').then(async (m) => { await m.default(); return m; })
    .catch((e) => { mod = null; throw e; });
  return mod;
}
