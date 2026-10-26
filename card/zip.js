// Reading a zip in the browser without a library: the central directory
// from the end of the file (File.slice), then any one entry's bytes,
// inflated with DecompressionStream('deflate-raw') and checked against the
// central directory's CRC-32. The central directory is the only authority
// for sizes, CRCs and offsets: zips written with data descriptors (the
// DMDoS pack is) carry zeros in their local headers. Zip64 is read; names
// are UTF-8 when flag bit 11 says so, else CP437. Runs in a page, a worker
// and node (Blob has slice/arrayBuffer/stream there too).

// A fault in the zip itself. `kind`: 'damaged' (cut short or corrupt: download
// it again) or 'unsupported' (a password, or packed in a way this reader
// doesn't know, Deflate64 say: the zip was re-made with another tool).
export class ZipError extends Error {
  constructor(message, kind = 'damaged') { super(message); this.kind = kind; }
}

const EOCD = 0x06054b50, EOCD64 = 0x06064b50, LOC64 = 0x07064b50, CEN = 0x02014b50, LOC = 0x04034b50;
const MAX_COMMENT = 0xffff;

// CP437's upper half (0x80-0xFF), for names without the UTF-8 flag.
const CP437 = 'ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ';
const utf8 = new TextDecoder('utf-8');

function decodeName(bytes, isUtf8) {
  if (isUtf8) return utf8.decode(bytes);
  let s = '';
  for (const b of bytes) s += b < 0x80 ? String.fromCharCode(b) : CP437[b - 0x80];
  return s;
}

async function bytesAt(blob, start, end) {
  if (start < 0 || end > blob.size || start > end) throw new ZipError('the file is cut short');
  return new Uint8Array(await blob.slice(start, end).arrayBuffer());
}

const u64 = (v, o) => {
  const lo = v.getUint32(o, true), hi = v.getUint32(o + 4, true);
  if (hi > 0x1fffff) throw new ZipError('a size too large to read');
  return hi * 0x100000000 + lo;
};

// The central directory: every entry as
// {name, dir, method, flags, encrypted, crc, csize, usize, lho}.
export async function readZip(blob) {
  const size = blob.size;
  if (size < 22) throw new ZipError('not a zip file');
  const tailLen = Math.min(size, 22 + MAX_COMMENT);
  const tail = await bytesAt(blob, size - tailLen, size);
  const tv = new DataView(tail.buffer);
  let at = -1;
  for (let i = tail.length - 22; i >= 0; i--) {
    if (tv.getUint32(i, true) === EOCD && i + 22 + tv.getUint16(i + 20, true) === tail.length) { at = i; break; }
  }
  if (at < 0) throw new ZipError('not a zip file, or the download stopped before the end');
  let count = tv.getUint16(at + 10, true);
  let cdSize = tv.getUint32(at + 12, true);
  let cdOff = tv.getUint32(at + 16, true);
  const eocdPos = size - tailLen + at;
  if (count === 0xffff || cdSize === 0xffffffff || cdOff === 0xffffffff) {
    // Zip64: the locator sits just before the end record.
    const loc = await bytesAt(blob, eocdPos - 20, eocdPos);
    const lv = new DataView(loc.buffer);
    if (lv.getUint32(0, true) !== LOC64) throw new ZipError('a damaged zip64 record');
    const off = u64(lv, 8);
    const rec = await bytesAt(blob, off, off + 56);
    const rv = new DataView(rec.buffer);
    if (rv.getUint32(0, true) !== EOCD64) throw new ZipError('a damaged zip64 record');
    count = u64(rv, 32);
    cdSize = u64(rv, 40);
    cdOff = u64(rv, 48);
  }
  if (cdOff + cdSize > eocdPos) throw new ZipError('the file is cut short');
  const cd = await bytesAt(blob, cdOff, cdOff + cdSize);
  const v = new DataView(cd.buffer);
  const entries = [];
  let p = 0;
  for (let n = 0; n < count; n++) {
    if (p + 46 > cd.length || v.getUint32(p, true) !== CEN) throw new ZipError('a damaged list of files');
    const flags = v.getUint16(p + 8, true);
    const method = v.getUint16(p + 10, true);
    const crc = v.getUint32(p + 16, true);
    let csize = v.getUint32(p + 20, true);
    let usize = v.getUint32(p + 24, true);
    const nlen = v.getUint16(p + 28, true), xlen = v.getUint16(p + 30, true), clen = v.getUint16(p + 32, true);
    let lho = v.getUint32(p + 42, true);
    if (p + 46 + nlen + xlen + clen > cd.length) throw new ZipError('a damaged list of files');
    const name = decodeName(cd.subarray(p + 46, p + 46 + nlen), (flags & 0x800) !== 0);
    // Zip64 extra field: the 64-bit values, in order, of the fields that are all ones.
    for (let x = p + 46 + nlen, end = x + xlen; x + 4 <= end;) {
      const id = v.getUint16(x, true), len = v.getUint16(x + 2, true);
      if (id === 1) {
        let q = x + 4;
        if (usize === 0xffffffff) { usize = u64(v, q); q += 8; }
        if (csize === 0xffffffff) { csize = u64(v, q); q += 8; }
        if (lho === 0xffffffff) { lho = u64(v, q); q += 8; }
      }
      x += 4 + len;
    }
    entries.push({ name, dir: name.endsWith('/'), method, flags, encrypted: (flags & 1) !== 0, crc, csize, usize, lho });
    p += 46 + nlen + xlen + clen;
  }
  return { entries, size };
}

// --- CRC-32 (IEEE), slicing by 4 ------------------------------------------
const T = (() => {
  const t = new Uint32Array(1024);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  for (let n = 0; n < 256; n++) {
    for (let k = 1; k < 4; k++) t[k * 256 + n] = (t[(k - 1) * 256 + n] >>> 8) ^ t[t[(k - 1) * 256 + n] & 0xff];
  }
  return t;
})();

export function crc32(bytes, crc = 0) {
  let c = ~crc >>> 0, i = 0;
  const n = bytes.length, n4 = n - (n % 4);
  for (; i < n4; i += 4) {
    c ^= bytes[i] | (bytes[i + 1] << 8) | (bytes[i + 2] << 16) | (bytes[i + 3] << 24);
    c = T[768 + (c & 0xff)] ^ T[512 + ((c >>> 8) & 0xff)] ^ T[256 + ((c >>> 16) & 0xff)] ^ T[c >>> 24];
  }
  for (; i < n; i++) c = T[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return ~c >>> 0;
}

// One entry's bytes, whole, CRC-checked. Throws ZipError for anything the
// zip itself gets wrong (bad header, unsupported method, wrong size or CRC).
export async function entryBytes(blob, e) {
  if (e.encrypted) throw new ZipError('protected with a password', 'unsupported');
  if (e.method !== 0 && e.method !== 8) throw new ZipError(`packed in a way this page can't read (method ${e.method})`, 'unsupported');
  const head = await bytesAt(blob, e.lho, e.lho + 30);
  const hv = new DataView(head.buffer);
  if (hv.getUint32(0, true) !== LOC) throw new ZipError('damaged in the download');
  const start = e.lho + 30 + hv.getUint16(26, true) + hv.getUint16(28, true);
  const raw = blob.slice(start, start + e.csize);
  if (start + e.csize > blob.size) throw new ZipError('the file is cut short');
  let out;
  if (e.method === 0) {
    out = new Uint8Array(await raw.arrayBuffer());
  } else {
    out = new Uint8Array(e.usize);
    let n = 0;
    const reader = raw.stream().pipeThrough(new DecompressionStream('deflate-raw')).getReader();
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        if (n + value.length > out.length) throw new ZipError('damaged in the download');
        out.set(value, n);
        n += value.length;
      }
    } catch (err) {
      reader.cancel().catch(() => {});
      throw err instanceof ZipError ? err : new ZipError('damaged in the download');
    }
    if (n !== out.length) throw new ZipError('damaged in the download');
  }
  if (out.length !== e.usize || crc32(out) !== e.crc) throw new ZipError('damaged in the download');
  return out;
}
