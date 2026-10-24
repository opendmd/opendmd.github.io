//! Q16.16 arithmetic: the one definition of every operation, shared by the
//! constant folder, the scalar (per-frame, per-row) evaluator and the lane
//! loops, so a value is the same bits however it was computed — on the
//! ESP32, on the host and in wasm (integer maths only; no float anywhere).
//!
//! **Overflow.** Values are i32 Q16.16: −32,768 .. 32,767.99998. `+ - *
//! /`, negation and `abs` *saturate* (clamp to the ends) rather than wrap,
//! so `t*t` after three minutes is "very big", not a sudden negative; the
//! phase functions (`sin`, `tri`, ...) only read the fractional turn, so
//! they never overflow. Division by zero gives the saturated value of the
//! numerator's sign (0 for 0 / 0); `%` by zero gives 0.

use crate::tables::{Tables, ATAN, SQRT};

pub const ONE: i32 = 1 << 16;

#[inline(always)]
pub fn sat(v: i64) -> i32 {
    if v > i32::MAX as i64 {
        i32::MAX
    } else if v < i32::MIN as i64 {
        i32::MIN
    } else {
        v as i32
    }
}

/// The saturated value for a result whose sign is `sign`'s: the ends of
/// the range. Out of line and cold, so the compiler lays the common
/// (no-overflow) path out as the fall-through.
#[cold]
#[inline(never)]
fn ends(sign: i32) -> i32 {
    (sign >> 31) ^ i32::MAX
}

#[inline(always)]
pub fn add(a: i32, b: i32) -> i32 {
    // As `saturating_add`, written so the (rare) overflow is one
    // predictable branch on the ESP32 rather than several.
    let r = a.wrapping_add(b);
    if (a ^ r) & (b ^ r) < 0 {
        ends(a)
    } else {
        r
    }
}

#[inline(always)]
pub fn sub(a: i32, b: i32) -> i32 {
    let r = a.wrapping_sub(b);
    if (a ^ b) & (a ^ r) < 0 {
        ends(a)
    } else {
        r
    }
}

#[inline(always)]
pub fn mul(a: i32, b: i32) -> i32 {
    // sat(a·b >> 16): the product's high word says whether it fits (it
    // does iff hi is in −32,768..32,767), one test instead of two 64-bit
    // compares.
    let p = a as i64 * b as i64;
    let hi = (p >> 32) as i32;
    if (hi as u32).wrapping_add(0x8000) >> 16 != 0 {
        ends(hi)
    } else {
        (p >> 16) as i32
    }
}

#[inline(always)]
pub fn div(a: i32, b: i32) -> i32 {
    if b == 0 {
        return match a {
            0 => 0,
            a if a > 0 => i32::MAX,
            _ => i32::MIN,
        };
    }
    sat(((a as i64) << 16) / b as i64)
}

/// `1 / a`, what a division by a per-frame or per-row value becomes (one
/// division per frame or row, then a multiply per pixel).
#[inline(always)]
pub fn recip(a: i32) -> i32 {
    div(ONE, a)
}

/// Floored modulo (the result takes the divisor's sign, as GLSL's `mod`):
/// both are Q16.16, so it is the raw integers' remainder.
#[inline(always)]
pub fn modulo(a: i32, b: i32) -> i32 {
    if b == 0 {
        return 0;
    }
    let r = a.wrapping_rem(b);
    if r != 0 && ((r < 0) != (b < 0)) {
        r.wrapping_add(b)
    } else {
        r
    }
}

/// `a % m` for a positive power-of-two `m` (raw): `a & (m - 1)`, which is
/// the floored modulo. `b` is the mask.
#[inline(always)]
pub fn mask(a: i32, b: i32) -> i32 {
    a & b
}

#[inline(always)]
fn b2f(b: bool) -> i32 {
    if b {
        ONE
    } else {
        0
    }
}

#[inline(always)]
pub fn lt(a: i32, b: i32) -> i32 {
    b2f(a < b)
}
#[inline(always)]
pub fn le(a: i32, b: i32) -> i32 {
    b2f(a <= b)
}
#[inline(always)]
pub fn gt(a: i32, b: i32) -> i32 {
    b2f(a > b)
}
#[inline(always)]
pub fn ge(a: i32, b: i32) -> i32 {
    b2f(a >= b)
}
#[inline(always)]
pub fn eq(a: i32, b: i32) -> i32 {
    b2f(a == b)
}
#[inline(always)]
pub fn ne(a: i32, b: i32) -> i32 {
    b2f(a != b)
}
#[inline(always)]
pub fn and(a: i32, b: i32) -> i32 {
    b2f(a != 0 && b != 0)
}
#[inline(always)]
pub fn or(a: i32, b: i32) -> i32 {
    b2f(a != 0 || b != 0)
}
#[inline(always)]
pub fn not(a: i32) -> i32 {
    b2f(a == 0)
}
#[inline(always)]
pub fn min(a: i32, b: i32) -> i32 {
    a.min(b)
}
#[inline(always)]
pub fn max(a: i32, b: i32) -> i32 {
    a.max(b)
}
/// GLSL's `step(edge, x)`: 1 where `x >= edge`.
#[inline(always)]
pub fn step(e: i32, x: i32) -> i32 {
    b2f(x >= e)
}
#[inline(always)]
pub fn neg(a: i32) -> i32 {
    a.saturating_neg()
}
#[inline(always)]
pub fn abs(a: i32) -> i32 {
    a.saturating_abs()
}
#[inline(always)]
pub fn floor(a: i32) -> i32 {
    a & !0xffff
}
#[inline(always)]
pub fn frac(a: i32) -> i32 {
    a & 0xffff
}

/// The sine of a phase in turns (the fraction only: 65,536 steps a turn,
/// core's `sin16`), −1..1.
#[inline(always)]
pub fn sin(t: &Tables, a: i32) -> i32 {
    let ph = a as u32 & 0xffff;
    let (i, f) = ((ph >> 8) as usize, (ph & 255) as i32);
    let s0 = t.sin[i] as i32;
    let s1 = t.sin[(i + 1) & 255] as i32;
    (s0 + (((s1 - s0) * f) >> 8)) << 2
}

#[inline(always)]
pub fn cos(t: &Tables, a: i32) -> i32 {
    sin(t, a.wrapping_add(ONE / 4))
}

/// The triangle wave in step with `sin`: 0 at 0, 1 at ¼, −1 at ¾.
#[inline(always)]
pub fn tri(a: i32) -> i32 {
    let p = a & 0xffff;
    if p < 0x4000 {
        p << 2
    } else if p < 0xc000 {
        (2 * ONE) - (p << 2)
    } else {
        (p << 2) - 4 * ONE
    }
}

/// The sawtooth: −1 at 0 rising to 1 at the end of the turn.
#[inline(always)]
pub fn saw(a: i32) -> i32 {
    ((a & 0xffff) << 1) - ONE
}

/// The square wave: 1 for the first half turn, −1 for the second.
#[inline(always)]
pub fn sqr(a: i32) -> i32 {
    if a & 0x8000 == 0 {
        ONE
    } else {
        -ONE
    }
}

/// √v for `v` in 2^28..2^30 (≈ 2^14..2^15): the table at the top byte,
/// interpolated by the next 16 bits. Linear interpolation of √ over a
/// 1/64 step is good to ~2^−17; the table's rounding makes it ~15 bits.
/// Ten-odd instructions where a bit-by-bit root takes fifteen rounds.
#[inline(always)]
fn sqrt30(v: u32) -> u32 {
    let i = (v >> 22) as usize - 64; // 0..192
    let f = (v >> 6) & 0xffff;
    let (a, b) = (SQRT[i] as u32, SQRT[(i + 1).min(192)] as u32);
    a + (((b - a) * f) >> 16)
}

/// √ of a Q32 value (u64) as Q16, ~15 significant bits: normalised to
/// 2^28..2^30 by an even shift, the table root, shifted back.
#[inline(always)]
fn sqrt_q32(v: u64) -> i32 {
    if v == 0 {
        return 0;
    }
    // Shift left by s (even) so the top bit lands at 28 or 29: then
    // v' = (v << s) >> 34 is in 2^28..2^30.
    let lz = v.leading_zeros() as i32; // 0..63
    let s = lz & !1; // even, ≤ lz
    let w = ((v << s) >> 34) as u32;
    let r = sqrt30(w) as u64;
    // √v = √(w · 2^34 / 2^s) = √w · 2^17 / 2^(s/2)
    let sh = 17 - s / 2;
    let out = if sh >= 0 { r << sh } else { r >> -sh };
    sat(out as i64)
}

#[inline(always)]
pub fn sqrt(a: i32) -> i32 {
    if a <= 0 {
        return 0;
    }
    // a is Q16; a << 16 is Q32.
    sqrt_q32((a as u64) << 16)
}

/// √(a² + b²).
#[inline(always)]
pub fn len(a: i32, b: i32) -> i32 {
    let (a, b) = (a.unsigned_abs() as u64, b.unsigned_abs() as u64);
    sqrt_q32((a * a).saturating_add(b * b))
}

/// The angle of (x, y) in turns, −½..½ (0 along +x, ¼ along +y): the
/// eighth-turn table, interpolated, folded into the octant.
#[inline(always)]
pub fn atan2(y: i32, x: i32) -> i32 {
    let (ax, ay) = (x.unsigned_abs(), y.unsigned_abs());
    let (mut lo, mut hi) = if ay > ax { (ax, ay) } else { (ay, ax) };
    if hi == 0 {
        return 0;
    }
    let bits = 32 - hi.leading_zeros();
    if bits > 15 {
        lo >>= bits - 15;
        hi >>= bits - 15;
        if hi == 0 {
            return 0;
        }
    }
    let r = (lo << 16) / hi; // 0..=65536
    let (i, f) = ((r >> 8) as usize, (r & 255) as i32);
    let (t0, t1) = (ATAN[i] as i32, ATAN[(i + 1).min(256)] as i32);
    let mut ang = t0 + (((t1 - t0) * f) >> 8);
    if ay > ax {
        ang = ONE / 4 - ang;
    }
    if x < 0 {
        ang = ONE / 2 - ang;
    }
    if y < 0 {
        -ang
    } else {
        ang
    }
}

/// lowbias32: a small integer hash.
#[inline(always)]
pub fn mix32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// A pseudo-random 0..1 from two values (their exact bits).
#[inline(always)]
pub fn hash(a: i32, b: i32) -> i32 {
    (mix32(a as u32 ^ mix32(b as u32 ^ 0x9e37_79b9)) >> 16) as i32
}

/// The lattice value at a corner, 0..65,535, from the corner's three
/// coordinate terms (`x·P1`, `y·P2`, `z·P3`, so a neighbour is one add
/// away): a xor and a one-multiply finaliser.
#[inline(always)]
fn corner(hx: u32, hy: u32, hz: u32) -> i32 {
    let mut h = hx ^ hy ^ hz;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h >> 16) as i32
}

/// The smooth weight 3f² − 2f³ of a fraction 0..1 (Q16), as 0..32,768
/// (Q15: so a weighted difference of two 16-bit values fits an i32).
#[inline(always)]
fn fade15(f: i32) -> i32 {
    let f2 = ((f as u32 * f as u32) >> 16) as i32; // f < 2^16
    ((f2 as i64 * (3 * ONE - 2 * f) as i64) >> 17) as i32
}

#[inline(always)]
fn lerp15(a: i32, b: i32, t: i32) -> i32 {
    a + (((b - a) * t) >> 15)
}

const P1: u32 = 0x8da6_b343;
const P2: u32 = 0xd816_3841;
const P3: u32 = 0xcb1a_b31f;

/// 3D value noise, 0..1: a hash at every integer lattice point, blended
/// with the smooth weight. One lattice cell per unit, so `noise(x*8, ...)`
/// has eight cells across the panel.
#[inline(always)]
pub fn noise(x: i32, y: i32, z: i32) -> i32 {
    let (x0, y0, z0) = (
        ((x >> 16) as u32).wrapping_mul(P1),
        ((y >> 16) as u32).wrapping_mul(P2),
        ((z >> 16) as u32).wrapping_mul(P3),
    );
    let (x1, y1, z1) = (
        x0.wrapping_add(P1),
        y0.wrapping_add(P2),
        z0.wrapping_add(P3),
    );
    let (fx, fy, fz) = (fade15(x & 0xffff), fade15(y & 0xffff), fade15(z & 0xffff));
    let a = lerp15(corner(x0, y0, z0), corner(x1, y0, z0), fx);
    let b = lerp15(corner(x0, y1, z0), corner(x1, y1, z0), fx);
    let c = lerp15(corner(x0, y0, z1), corner(x1, y0, z1), fx);
    let d = lerp15(corner(x0, y1, z1), corner(x1, y1, z1), fx);
    lerp15(lerp15(a, b, fy), lerp15(c, d, fy), fz)
}

#[inline(always)]
pub fn select(c: i32, a: i32, b: i32) -> i32 {
    if c != 0 {
        a
    } else {
        b
    }
}

#[inline(always)]
pub fn clamp(x: i32, lo: i32, hi: i32) -> i32 {
    x.max(lo).min(hi)
}

/// `a + (b − a)·t`.
#[inline(always)]
pub fn mix(a: i32, b: i32, t: i32) -> i32 {
    add(a, mul(sub(b, a), t))
}

/// `smoothstep(e0, e1, x)` with `inv` = 1 / (e1 − e0) worked out once:
/// t = clamp((x − e0)·inv, 0, 1), t²(3 − 2t).
#[inline(always)]
pub fn smooth(x: i32, e0: i32, inv: i32) -> i32 {
    let t = clamp(mul(sub(x, e0), inv), 0, ONE);
    let t2 = ((t as i64 * t as i64) >> 16) as i32;
    ((t2 as i64 * (3 * ONE - 2 * t) as i64) >> 16) as i32
}

/// A Q16 colour channel 0..1 as 0..255, rounded.
#[inline(always)]
pub fn chan(v: i32) -> u8 {
    let c = clamp(v, 0, ONE);
    ((c * 255 + 0x8000) >> 16) as u8
}

/// HSV (hue in turns, the fraction only; saturation and value 0..1) to
/// 8-bit RGB. Saturation and value 1.0 are taken as 65,535/65,536 so
/// every product fits a u32 (the 8-bit result is the same).
#[inline(always)]
pub fn hsv(h: i32, s: i32, v: i32) -> [u8; 3] {
    let s = clamp(s, 0, 0xffff) as u32;
    let v = clamp(v, 0, 0xffff) as u32;
    let h6 = (h & 0xffff) as u32 * 6; // 0..393,210
    let sector = h6 >> 16;
    let f = h6 & 0xffff;
    let p = (v * (0x10000 - s)) >> 16;
    let q = (v * (0x10000 - ((s * f) >> 16))) >> 16;
    let t = (v * (0x10000 - ((s * (0x10000 - f)) >> 16))) >> 16;
    let c8 = |x: u32| ((x * 255 + 0x8000) >> 16) as u8;
    let (r, g, b) = match sector {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [c8(r), c8(g), c8(b)]
}

/// A palette entry for k in 0..1 (clamped): 32 stepped entries.
#[inline(always)]
pub fn pal(p: &[[u8; 3]; 32], k: i32) -> [u8; 3] {
    let i = (clamp(k, 0, ONE) >> 11).min(31) as usize;
    p[i]
}

// --- Layers: what is under, and blending ------------------------------------

/// A byte 0..255 as Q16 0..1, exactly: 255 is 1.0 and `chan` of the result
/// is the byte again (`b·257 + b/128`: 0, 257, …, 65,536).
#[inline(always)]
pub fn byte(b: u8) -> i32 {
    (b as i32) * 257 + (b as i32 >> 7)
}

/// Rec. 601 luminance of an RGB888 pixel, as the panel's tint works it out
/// (`opendmd_core::look::luma`: `(77 R + 150 G + 29 B + 128) / 256`).
#[inline(always)]
pub fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((77 * r as u32 + 150 * g as u32 + 29 * b as u32 + 128) >> 8) as u8
}

/// `a · b / 255`, rounded (exact for every pair of bytes).
#[inline(always)]
pub fn mul255(a: u8, b: u8) -> u8 {
    let p = a as u32 * b as u32 + 128;
    ((p + (p >> 8)) >> 8) as u8
}

/// Blend modes, in the order of `layer`'s names (`over add multiply screen
/// max min`).
pub const BLEND_OVER: u8 = 0;
pub const BLEND_ADD: u8 = 1;
pub const BLEND_MULTIPLY: u8 = 2;
pub const BLEND_SCREEN: u8 = 3;
pub const BLEND_MAX: u8 = 4;
pub const BLEND_MIN: u8 = 5;
pub const BLENDS: usize = 6;

/// One channel of a layer `s` blended onto what is below it, `d`.
#[inline(always)]
pub fn blend(mode: u8, d: u8, s: u8) -> u8 {
    match mode {
        BLEND_ADD => d.saturating_add(s),
        BLEND_MULTIPLY => mul255(d, s),
        BLEND_SCREEN => d.saturating_add(s - mul255(d, s)),
        BLEND_MAX => d.max(s),
        BLEND_MIN => d.min(s),
        _ => s,
    }
}

/// `d` moved towards `r` by the coverage `a` (Q16, clamped to 0..1):
/// `d + (r − d)·a`, rounded; `a` = 1 is exactly `r`, 0 exactly `d`.
#[inline(always)]
pub fn cover(d: u8, r: u8, a: i32) -> u8 {
    let a = clamp(a, 0, ONE);
    (d as i32 + (((r as i32 - d as i32) * a + 0x8000) >> 16)) as u8
}
