// A QR code encoder, written for OpenDMD's share page (MIT OR Apache-2.0,
// like the rest): byte mode, error correction level L, versions 1-40 (up to
// 2,953 bytes), the mask with the lowest penalty. Follows ISO/IEC 18004;
// the tables and the layout steps are the standard's. `qr(text)` gives the
// modules as rows of booleans (true = dark), or null when it doesn't fit.

// Per version (index 1-40), level L: error-correction codewords per block
// and the number of blocks.
const ECC = [0, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30];
const BLOCKS = [0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25];

// The modules left for data and error correction once the patterns are drawn.
function rawModules(v) {
  let n = (16 * v + 128) * v + 64;
  if (v >= 2) {
    const a = Math.floor(v / 7) + 2;
    n -= (25 * a - 10) * a - 55;
    if (v >= 7) n -= 36;
  }
  return n;
}
const dataCodewords = (v) => Math.floor(rawModules(v) / 8) - ECC[v] * BLOCKS[v];

// GF(256) over x^8 + x^4 + x^3 + x^2 + 1.
function mul(x, y) {
  let z = 0;
  for (let i = 7; i >= 0; i--) {
    z = (z << 1) ^ ((z >>> 7) * 0x11d);
    z ^= ((y >>> i) & 1) * x;
  }
  return z;
}
function divisor(n) {
  const r = new Array(n).fill(0);
  r[n - 1] = 1;
  let root = 1;
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < n; j++) {
      r[j] = mul(r[j], root);
      if (j + 1 < n) r[j] ^= r[j + 1];
    }
    root = mul(root, 2);
  }
  return r;
}
function remainder(data, div) {
  const r = new Array(div.length).fill(0);
  for (const b of data) {
    const f = b ^ r.shift();
    r.push(0);
    div.forEach((c, i) => { r[i] ^= mul(c, f); });
  }
  return r;
}

export function qr(text) {
  const bytes = new TextEncoder().encode(text);
  let v = 1;
  const need = (v) => 4 + (v < 10 ? 8 : 16) + bytes.length * 8;
  while (v <= 40 && need(v) > dataCodewords(v) * 8) v++;
  if (v > 40) return null;
  const cap = dataCodewords(v) * 8;

  // The bit stream: byte mode, the count, the bytes, a terminator, padding.
  const bits = [];
  const put = (val, n) => { for (let i = n - 1; i >= 0; i--) bits.push((val >>> i) & 1); };
  put(4, 4);
  put(bytes.length, v < 10 ? 8 : 16);
  for (const b of bytes) put(b, 8);
  put(0, Math.min(4, cap - bits.length));
  put(0, (8 - (bits.length % 8)) % 8);
  for (let p = 0xec; bits.length < cap; p ^= 0xec ^ 0x11) put(p, 8);
  const data = [];
  for (let i = 0; i < bits.length; i += 8) data.push(bits.slice(i, i + 8).reduce((a, b) => (a << 1) | b, 0));

  // Blocks with their error correction, interleaved.
  const nb = BLOCKS[v], ec = ECC[v], raw = Math.floor(rawModules(v) / 8);
  const short = nb - (raw % nb), len = Math.floor(raw / nb), div = divisor(ec);
  const blocks = [];
  for (let i = 0, k = 0; i < nb; i++) {
    const d = data.slice(k, k + len - ec + (i < short ? 0 : 1));
    k += d.length;
    const e = remainder(d, div);
    if (i < short) d.push(0); // a gap, skipped below
    blocks.push(d.concat(e));
  }
  const words = [];
  for (let i = 0; i < blocks[0].length; i++) {
    blocks.forEach((b, j) => { if (i !== len - ec || j >= short) words.push(b[i]); });
  }

  // The grid: function patterns first (marked, so data and masks skip them).
  const size = v * 4 + 17;
  const m = Array.from({ length: size }, () => new Array(size).fill(false));
  const fn = Array.from({ length: size }, () => new Array(size).fill(false));
  const set = (x, y, dark) => { m[y][x] = dark; fn[y][x] = true; };
  for (let i = 0; i < size; i++) { set(6, i, i % 2 === 0); set(i, 6, i % 2 === 0); }
  const finder = (cx, cy) => {
    for (let dy = -4; dy <= 4; dy++) for (let dx = -4; dx <= 4; dx++) {
      const d = Math.max(Math.abs(dx), Math.abs(dy)), x = cx + dx, y = cy + dy;
      if (x >= 0 && x < size && y >= 0 && y < size) set(x, y, d !== 2 && d !== 4);
    }
  };
  finder(3, 3); finder(size - 4, 3); finder(3, size - 4);
  if (v > 1) {
    const n = Math.floor(v / 7) + 2, step = Math.floor((v * 8 + n * 3 + 5) / (n * 4 - 4)) * 2;
    const pos = [6];
    for (let p = size - 7; pos.length < n; p -= step) pos.splice(1, 0, p);
    pos.forEach((y, i) => pos.forEach((x, j) => {
      if ((i === 0 && j === 0) || (i === 0 && j === n - 1) || (i === n - 1 && j === 0)) return;
      for (let dy = -2; dy <= 2; dy++) for (let dx = -2; dx <= 2; dx++) set(x + dx, y + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1);
    }));
  }
  const format = (mask) => {
    const d = (1 << 3) | mask; // level L is 01
    let r = d;
    for (let i = 0; i < 10; i++) r = (r << 1) ^ ((r >>> 9) * 0x537);
    const b = ((d << 10) | r) ^ 0x5412, bit = (i) => ((b >>> i) & 1) === 1;
    for (let i = 0; i <= 5; i++) set(8, i, bit(i));
    set(8, 7, bit(6)); set(8, 8, bit(7)); set(7, 8, bit(8));
    for (let i = 9; i < 15; i++) set(14 - i, 8, bit(i));
    for (let i = 0; i < 8; i++) set(size - 1 - i, 8, bit(i));
    for (let i = 8; i < 15; i++) set(8, size - 15 + i, bit(i));
    set(8, size - 8, true);
  };
  format(0);
  if (v >= 7) {
    let r = v;
    for (let i = 0; i < 12; i++) r = (r << 1) ^ ((r >>> 11) * 0x1f25);
    const b = (v << 12) | r;
    for (let i = 0; i < 18; i++) {
      const dark = ((b >>> i) & 1) === 1, a = size - 11 + (i % 3), c = Math.floor(i / 3);
      set(a, c, dark); set(c, a, dark);
    }
  }

  // The codewords, in two-column zigzags from the bottom right.
  let i = 0;
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5;
    for (let vert = 0; vert < size; vert++) for (let j = 0; j < 2; j++) {
      const x = right - j, up = ((right + 1) & 2) === 0, y = up ? size - 1 - vert : vert;
      if (!fn[y][x] && i < words.length * 8) { m[y][x] = ((words[i >>> 3] >>> (7 - (i & 7))) & 1) === 1; i++; }
    }
  }

  // The mask with the lowest penalty.
  const MASKS = [(x, y) => (x + y) % 2, (x, y) => y % 2, (x) => x % 3, (x, y) => (x + y) % 3,
    (x, y) => (Math.floor(x / 3) + Math.floor(y / 2)) % 2, (x, y) => (x * y) % 2 + (x * y) % 3,
    (x, y) => ((x * y) % 2 + (x * y) % 3) % 2, (x, y) => ((x + y) % 2 + (x * y) % 3) % 2];
  const apply = (k) => { for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) if (!fn[y][x] && MASKS[k](x, y) === 0) m[y][x] = !m[y][x]; };
  let best = 0, low = Infinity;
  for (let k = 0; k < 8; k++) {
    apply(k); format(k);
    const p = penalty(m);
    if (p < low) { low = p; best = k; }
    apply(k); // undo (XOR)
  }
  apply(best); format(best);
  return m;
}

// The standard's four penalties (runs, 2×2 blocks, finder look-alikes, balance).
function penalty(m) {
  const n = m.length;
  let p = 0, dark = 0;
  const line = (get) => {
    let run = 1, s = '';
    for (let i = 0; i < n; i++) {
      s += get(i) ? '1' : '0';
      if (i > 0 && get(i) === get(i - 1)) { run++; if (run === 5) p += 3; else if (run > 5) p++; } else run = 1;
    }
    for (let k = s.indexOf('1011101'); k >= 0; k = s.indexOf('1011101', k + 1)) {
      if (s.slice(Math.max(0, k - 4), k) === '0000' || s.slice(k + 7, k + 11) === '0000') p += 40;
    }
  };
  for (let y = 0; y < n; y++) {
    line((x) => m[y][x]);
    line((x) => m[x][y]);
    for (let x = 0; x < n; x++) {
      if (m[y][x]) dark++;
      if (x < n - 1 && y < n - 1 && m[y][x] === m[y][x + 1] && m[y][x] === m[y + 1][x] && m[y][x] === m[y + 1][x + 1]) p += 3;
    }
  }
  return p + Math.floor(Math.abs(dark * 20 - n * n * 10) / (n * n)) * 10;
}
