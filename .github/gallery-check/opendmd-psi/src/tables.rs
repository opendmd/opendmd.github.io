//! The tables Psi reads: the sine, the palettes and the panel's fonts
//! (lent by the caller, see [`Tables`]), and its own arctangent.
//!
//! All `static`, built at compile time by `const fn`s (f64 maths in const
//! eval), so on the device they are in DROM like `opendmd_core::fx`'s.

/// What a frame reads besides the program: the Q14 sine (256 entries, a
/// turn), the five 32-entry palettes, in `opendmd_core::fx::Palette`
/// order (fire, ice, chrono, amber, rainbow), and the panel's fonts
/// (`digit`, `glyph`): `opendmd_core::text`'s misc-fixed 4x6 (`small`) and
/// 5x7 (`normal`), printable ASCII 32..=126, and the bold 3 × 5 digits
/// (`bold`, 0..9 only), a byte a row, the most significant bit leftmost.
/// opendmd-core lends its own (`fx::SIN`, `fx::PALETTES`, `fonts.rs`'s
/// `FONT_4X6`/`FONT_5X7`/`FONT_BOLD`) so the firmware holds one
/// copy; with the `tables` feature Psi has identical ones of its own
/// ([`OWN`]; the fonts are core's very file, `include!`d).
#[derive(Clone, Copy)]
pub struct Tables<'a> {
    pub sin: &'a [i16; 256],
    pub palettes: [&'a [[u8; 3]; 32]; 5],
    pub small: &'a [[u8; 6]; 95],
    pub normal: &'a [[u8; 7]; 95],
    pub bold: &'a [[u8; 5]; 10],
}

/// The fonts, as [`Tables::glyphs`] takes them.
pub const NORMAL: u8 = 0;
pub const SMALL: u8 = 1;
/// Digits only.
pub const BOLD: u8 = 2;

/// A font as the glyph step reads it (see [`crate::fixed::glyph`]): `rows`
/// glyph after glyph, `h` bytes each, `w` columns wide; a character `c` is
/// glyph `c + off`, when `lo <= c < lo + n`.
#[derive(Clone, Copy)]
pub struct Glyphs<'a> {
    pub rows: &'a [u8],
    pub w: u32,
    pub h: u32,
    pub lo: i32,
    pub n: u32,
    pub off: i32,
}

impl<'a> Tables<'a> {
    /// The font for `digit` (`n` 0..9 is the glyph of `'0' + n`) or
    /// `glyph` (a character code, 32..126): [`NORMAL`] (a 5 × 7 cell),
    /// [`SMALL`] (4 × 6) or [`BOLD`] (4 × 6, the digits 3 × 5; its table
    /// is digits alone, so it is always `digit`'s).
    pub fn glyphs(&self, font: u8, digit: bool) -> Glyphs<'a> {
        let (rows, w, h) = match font {
            NORMAL => (self.normal.as_flattened(), 5, 7),
            SMALL => (self.small.as_flattened(), 4, 6),
            // Five rows in the table: the sixth (the gap) is outside it.
            _ => (self.bold.as_flattened(), 4, 5),
        };
        let (lo, n, off) = match (font, digit) {
            (BOLD, _) => (0, 10, 0),
            (_, true) => (0, 10, 16),
            _ => (32, 95, -32),
        };
        Glyphs {
            rows,
            w,
            h,
            lo,
            n,
            off,
        }
    }
}

/// Psi's own copies of core's tables (the `tables` feature).
#[cfg(feature = "tables")]
pub static OWN: Tables<'static> = Tables {
    sin: &SIN,
    palettes: [
        &PALETTES[0],
        &PALETTES[1],
        &PALETTES[2],
        &PALETTES[3],
        &PALETTES[4],
    ],
    small: &FONT_4X6,
    normal: &FONT_5X7,
    bold: &FONT_BOLD,
};

#[cfg(feature = "tables")]
mod fonts {
    include!("../../opendmd-core/src/fonts.rs");
}
#[cfg(feature = "tables")]
pub use fonts::{FONT_4X6, FONT_5X7, FONT_BOLD};

const PI: f64 = core::f64::consts::PI;
#[cfg(feature = "tables")]
const TAU: f64 = core::f64::consts::TAU;

const fn round(v: f64) -> i32 {
    if v >= 0.0 {
        (v + 0.5) as i32
    } else {
        (v - 0.5) as i32
    }
}

#[cfg(feature = "tables")]
const fn sin_f(x: f64) -> f64 {
    let mut x = x - ((x / TAU) as i64 as f64) * TAU;
    if x > PI {
        x -= TAU;
    }
    if x < -PI {
        x += TAU;
    }
    let (mut term, mut sum, mut k) = (x, x, 1);
    while k < 14 {
        term *= -x * x / ((2 * k) as f64 * (2 * k + 1) as f64);
        sum += term;
        k += 1;
    }
    sum
}

/// `sin(2π i / 256)` in Q14: byte for byte `opendmd_core::fx::SIN`.
#[cfg(feature = "tables")]
pub static SIN: [i16; 256] = {
    let mut t = [0i16; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = round(sin_f(i as f64 * TAU / 256.0) * 16384.0) as i16;
        i += 1;
    }
    t
};

#[cfg(feature = "tables")]
const fn expand(k: [[u8; 3]; 9]) -> [[u8; 3]; 32] {
    let mut out = [[0u8; 3]; 32];
    let mut i = 0;
    while i < 32 {
        let (a, b, f) = (k[i / 4], k[i / 4 + 1], (i % 4) as i32);
        let mut c = 0;
        while c < 3 {
            out[i][c] = (a[c] as i32 + (b[c] as i32 - a[c] as i32) * f / 4) as u8;
            c += 1;
        }
        i += 1;
    }
    out
}

/// Byte for byte `opendmd_core::fx::PALETTES`.
#[cfg(feature = "tables")]
pub static PALETTES: [[[u8; 3]; 32]; 5] = [
    expand([
        [0, 0, 0],
        [0, 0, 0],
        [72, 0, 0],
        [150, 12, 0],
        [220, 56, 0],
        [255, 112, 0],
        [255, 184, 24],
        [255, 236, 120],
        [255, 255, 255],
    ]),
    expand([
        [0, 0, 0],
        [0, 0, 0],
        [0, 12, 64],
        [0, 40, 128],
        [0, 88, 192],
        [24, 150, 240],
        [110, 205, 255],
        [196, 238, 255],
        [255, 255, 255],
    ]),
    expand([
        [0, 0, 0],
        [48, 0, 88],
        [112, 0, 176],
        [72, 40, 232],
        [32, 120, 255],
        [176, 200, 255],
        [255, 255, 255],
        [128, 56, 224],
        [0, 0, 0],
    ]),
    expand([
        [0, 0, 0],
        [0, 0, 0],
        [48, 22, 0],
        [96, 46, 0],
        [148, 76, 6],
        [200, 108, 14],
        [240, 146, 36],
        [255, 188, 88],
        [255, 222, 160],
    ]),
    expand([
        [0, 0, 0],
        [255, 0, 0],
        [255, 160, 0],
        [200, 255, 0],
        [0, 255, 80],
        [0, 160, 255],
        [80, 0, 255],
        [255, 0, 200],
        [0, 0, 0],
    ]),
];

const fn sqrt_f(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut r = if x > 1.0 { x } else { 1.0 };
    let mut i = 0;
    while i < 40 {
        r = 0.5 * (r + x / r);
        i += 1;
    }
    r
}

/// atan of `t` in `[0, 1]`: halve the angle twice, then the series (as
/// core's tunnel table does it).
const fn atan01(t: f64) -> f64 {
    let t1 = t / (1.0 + sqrt_f(1.0 + t * t));
    let t2 = t1 / (1.0 + sqrt_f(1.0 + t1 * t1));
    let (mut term, mut sum, mut k) = (t2, t2, 1);
    while k < 16 {
        term *= -t2 * t2;
        sum += term / (2 * k + 1) as f64;
        k += 1;
    }
    4.0 * sum
}

/// `atan(i / 256)` in turns, Q16 (0..8,192 = an eighth of a turn), 257
/// entries so the last interpolates: 514 B.
pub static ATAN: [u16; 257] = {
    let mut t = [0u16; 257];
    let mut i = 0;
    while i < 257 {
        t[i] = round(atan01(i as f64 / 256.0) / (2.0 * PI) * 65536.0) as u16;
        i += 1;
    }
    t
};

/// `√(i · 2^22)` for i in 64..=256 (index i − 64): the square root of a
/// value normalised to 2^28..2^30 by its top byte, interpolated by
/// `fixed::sqrt30` (≈ 15 significant bits). 386 B.
pub static SQRT: [u16; 193] = {
    let mut t = [0u16; 193];
    let mut i = 0;
    while i < 193 {
        let v = round(sqrt_f((i + 64) as f64 * 4194304.0));
        t[i] = if v > 65535 { 65535 } else { v as u16 };
        i += 1;
    }
    t
};
