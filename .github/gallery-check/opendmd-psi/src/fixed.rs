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

use crate::tables::{Glyphs, Tables, ATAN, SQRT};

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
    // (As `hi >> 15 == hi >> 31`, hi in −32,768..32,767: two shifts, where
    // the ESP32's compiler loaded the constants of an add-and-compare from
    // flash for every element of a lane.)
    let p = a as i64 * b as i64;
    let hi = (p >> 32) as i32;
    if hi >> 15 != hi >> 31 {
        ends(hi)
    } else {
        (p >> 16) as i32
    }
}

// The narrowed steps (`crate::range`): what `add`, `sub` and `mul` give
// when their result is known to fit — no saturation test.

/// `add` of values whose sum fits an i32.
#[inline(always)]
pub fn add_w(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

/// `sub` of values whose difference fits an i32.
#[inline(always)]
pub fn sub_w(a: i32, b: i32) -> i32 {
    a.wrapping_sub(b)
}

/// `mul` of values whose product, shifted, fits an i32.
#[inline(always)]
pub fn mul_w(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64) >> 16) as i32
}

/// `mul` of values whose product fits an i32 before the shift: one 32-bit
/// multiply.
#[inline(always)]
pub fn mul_n(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b) >> 16
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
    #[cfg(target_arch = "xtensa")]
    {
        div_u32(a, b)
    }
    #[cfg(not(target_arch = "xtensa"))]
    {
        div_i64(a, b)
    }
}

/// [`div`] by a non-zero `b` as a 64-bit division: `sat((a << 16) / b)`,
/// truncated towards zero (the definition; the host and wasm run it, where
/// a 64-bit division is one instruction).
#[cfg_attr(target_arch = "xtensa", allow(dead_code))]
#[inline(always)]
pub(crate) fn div_i64(a: i32, b: i32) -> i32 {
    sat(((a as i64) << 16) / b as i64)
}

/// [`div_i64`] with 32-bit divisions only (the ESP32's `quou`; its 64-bit
/// division is a ROM call, ~900 ns a pixel): the same bits for every
/// non-zero `b`. On magnitudes `n = |a|`, `d = |b|` (the quotient
/// truncates towards zero, so it is the magnitudes' floor with the sign
/// put back): `n << 16` fits 32 bits when `n < 2^15`, one division; else
/// `n / d` is the whole part (2^15 or more saturates: the result is at
/// least 2^31 either way) and the 16 bits below the point divide the
/// remainder, `(r << 16) / d`, which fits 32 bits when `d < 2^16` and is
/// one Knuth digit ([`digit16`]) when it doesn't.
#[cfg_attr(not(target_arch = "xtensa"), allow(dead_code))]
#[inline(always)]
pub(crate) fn div_u32(a: i32, b: i32) -> i32 {
    // (`| (b == 0)`: no zero test before the divisions; b is never 0.)
    let (n, d) = (a.unsigned_abs(), b.unsigned_abs() | (b == 0) as u32);
    let m = if n >> 15 == 0 {
        (n << 16) / d
    } else {
        let q = n / d;
        if q >> 15 != 0 {
            return ends(a ^ b);
        }
        let r = n - q * d;
        let f = if d >> 16 == 0 {
            (r << 16) / d
        } else {
            digit16(r, d)
        };
        (q << 16) | f
    };
    // m < 2^31: negating it cannot overflow.
    if (a ^ b) < 0 {
        (m as i32).wrapping_neg()
    } else {
        m as i32
    }
}

/// `floor(r · 2^16 / d)` for `r < d`, `d ≥ 2^16` (a 16-bit quotient) with
/// one 32-bit division: Knuth's algorithm D for a single digit in base
/// 2^16 (Hacker's Delight's `divlu`): `d` normalised so its top bit is
/// set, the digit estimated from `d`'s top half, then corrected (at most
/// twice) against its low half. With a two-digit divisor that test is
/// exact, so no multiply-and-subtract follows.
#[inline(always)]
fn digit16(r: u32, d: u32) -> u32 {
    // d ≥ 2^16: s ≤ 15; r < d, so r << s fits too. (`| 0x8000` changes
    // nothing, dn's top bit is set: it spares a zero test.)
    let s = lz32(d);
    let (dn, rn) = (d << s, r << s);
    let (dh, dl) = ((dn >> 16) | 0x8000, dn & 0xffff);
    let mut q = rn / dh;
    let mut rh = rn - q * dh;
    while q > 0xffff || q * dl > (rh << 16) {
        q -= 1;
        rh += dh;
        if rh > 0xffff {
            break;
        }
    }
    q
}

/// `v.leading_zeros()` (32 for 0). On the ESP32 its `nsau` instruction:
/// the Xtensa backend never emits it, and expands `leading_zeros` to a
/// ~30-instruction bit count with four constants loaded from flash.
#[inline(always)]
pub(crate) fn lz32(v: u32) -> u32 {
    #[cfg(target_arch = "xtensa")]
    {
        let r: u32;
        // SAFETY: one register-to-register instruction (NSAU: the number of
        // leading zeros, 32 for 0); no memory, no stack, no flags.
        unsafe {
            core::arch::asm!("nsau {0}, {1}", out(reg) r, in(reg) v, options(pure, nomem, nostack));
        }
        r
    }
    #[cfg(not(target_arch = "xtensa"))]
    {
        v.leading_zeros()
    }
}

/// `1 / a`, what a division by a per-frame or per-row value becomes (one
/// division per frame or row, then a multiply per pixel).
#[inline(always)]
pub fn recip(a: i32) -> i32 {
    #[cfg(target_arch = "xtensa")]
    {
        recip_u32(a)
    }
    #[cfg(not(target_arch = "xtensa"))]
    {
        div(ONE, a)
    }
}

/// [`recip`] with one 32-bit division: `div(ONE, a)` is `2^32 / |a|`,
/// floored, signed and saturated; for `|a| ≥ 2` that is `(2^32 − 1) / |a|`,
/// plus 1 when `|a|` divides 2^32 (a power of two).
#[cfg_attr(not(target_arch = "xtensa"), allow(dead_code))]
#[inline(always)]
pub(crate) fn recip_u32(a: i32) -> i32 {
    let d = a.unsigned_abs();
    if d <= 1 {
        // 1 / 0 and 1 / ±2^-16 saturate (a = 0 to the top end).
        return ends(a | (a == 0) as i32);
    }
    let q = u32::MAX / d + (d & (d - 1) == 0) as u32;
    if q >> 31 != 0 {
        return ends(a);
    }
    if a < 0 {
        -(q as i32)
    } else {
        q as i32
    }
}

/// `1 / k` for the compiler's `x / k` (k a positive constant, x per pixel),
/// rounded up instead of down: then a whole number divided by a whole
/// number that divides it comes out exactly that or a hair above, never
/// below, so `floor(px/10)` is right at 20, 30, … (rounded down, 20/10 was
/// 1.99998). A negative k keeps `recip`.
pub fn recip_up(k: i32) -> i32 {
    if k <= 0 {
        return recip(k);
    }
    sat(((1i64 << 32) + k as i64 - 1) / k as i64)
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
pub(crate) fn sqrt30(v: u32) -> u32 {
    let i = (v >> 22) as usize - 64; // 0..192
    let f = (v >> 6) & 0xffff;
    let (a, b) = (SQRT[i] as u32, SQRT[(i + 1).min(192)] as u32);
    a + (((b - a) * f) >> 16)
}

/// √ of a Q32 value (u64) as Q16, ~15 significant bits: normalised to
/// 2^28..2^30 by an even shift, the table root, shifted back. (The same
/// bits as first written, with 64-bit shifts and `leading_zeros`, in
/// 32-bit steps: the ESP32 has neither.)
#[inline(always)]
fn sqrt_q32(v: u64) -> i32 {
    // The top non-zero word, the word under it, and what the even shift
    // of the whole is past the top word's own.
    let (hi, lo) = ((v >> 32) as u32, v as u32);
    let (top, next, base) = if hi == 0 { (lo, 0, 32) } else { (hi, lo, 0) };
    if top == 0 {
        return 0;
    }
    // (v << (base + s)) >> 34: the top word shifted by its own even shift
    // (s ≤ 30) with the next word's top s bits under it, then >> 2.
    let s = lz32(top) & !1;
    let w = ((top << s) | ((next >> 1) >> (31 - s))) >> 2;
    let r = sqrt30(w);
    // √v = r · 2^(17 − (base + s)/2), −14..=17; past i32::MAX saturates
    // (r ≤ 2^15).
    let sh = 17 - ((base + s) / 2) as i32;
    if sh < 0 {
        (r >> -sh) as i32
    } else if r >> (31 - sh) != 0 {
        i32::MAX
    } else {
        (r << sh) as i32
    }
}

#[inline(always)]
pub fn sqrt(a: i32) -> i32 {
    if a <= 0 {
        return 0;
    }
    // a is Q16; a << 16 is Q32, whose even shift is 16 + s for a's own s
    // (a < 2^31): w = (a << s) >> 2, the root shifted by 9 − s/2.
    let s = lz32(a as u32) & !1;
    let r = sqrt30((a as u32) << s >> 2);
    if s <= 18 {
        (r << (9 - s / 2)) as i32
    } else {
        (r >> (s / 2 - 9)) as i32
    }
}

/// √(a² + b²).
#[inline(always)]
pub fn len(a: i32, b: i32) -> i32 {
    let (a, b) = (a.unsigned_abs() as u64, b.unsigned_abs() as u64);
    // (Each square is at most 2^62: the sum fits.)
    sqrt_q32(a * a + b * b)
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
    let bits = 32 - lz32(hi);
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
    noise_cell(x0, y0, z0, x, y, z)
}

/// [`noise`] with each axis's lattice term moved by `off` (the whole part
/// of a value that grows without end, times the axis's prime: [`w_lat`]):
/// `noise_off(frac(w) + s, …, [w_lat(w, 0), …])` is `noise(w + s, …)`
/// exactly, wherever `w + s` fits a Q16.16, and goes on past it without
/// repeating (the hash sees only the low 32 bits of the lattice index, as
/// `noise`'s does).
#[inline(always)]
pub fn noise_off(x: i32, y: i32, z: i32, off: [u32; 3]) -> i32 {
    let (x0, y0, z0) = (
        ((x >> 16) as u32).wrapping_mul(P1).wrapping_add(off[0]),
        ((y >> 16) as u32).wrapping_mul(P2).wrapping_add(off[1]),
        ((z >> 16) as u32).wrapping_mul(P3).wrapping_add(off[2]),
    );
    noise_cell(x0, y0, z0, x, y, z)
}

/// The noise of the cell whose low corner's lattice terms are `x0 y0 z0`,
/// at the fractions of `x y z`.
#[inline(always)]
fn noise_cell(x0: u32, y0: u32, z0: u32, x: i32, y: i32, z: i32) -> i32 {
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

/// [`noise`] at a whole `z` (its fraction 0): the same value from the
/// four corners of that plane (the other four are weighed by
/// `fade15(0)` = 0, and `lerp15(a, b, 0)` is `a`).
#[inline(always)]
pub fn noise_flat(x: i32, y: i32, z: i32) -> i32 {
    debug_assert!(z & 0xffff == 0);
    let (x0, y0, z0) = (
        ((x >> 16) as u32).wrapping_mul(P1),
        ((y >> 16) as u32).wrapping_mul(P2),
        ((z >> 16) as u32).wrapping_mul(P3),
    );
    let (x1, y1) = (x0.wrapping_add(P1), y0.wrapping_add(P2));
    let (fx, fy) = (fade15(x & 0xffff), fade15(y & 0xffff));
    let a = lerp15(corner(x0, y0, z0), corner(x1, y0, z0), fx);
    let b = lerp15(corner(x0, y1, z0), corner(x1, y1, z0), fx);
    lerp15(a, b, fy)
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

/// Bit `i` of `floor(n)` (two's complement), 1 or 0: bits 0..14 of a
/// whole number up to 32,767; a negative `n`'s bits from 15 up are 1. `i`
/// is floored; below 0 it is 0.
#[inline(always)]
pub fn bit(n: i32, i: i32) -> i32 {
    let k = i >> 16;
    if k < 0 {
        0
    } else {
        (((n >> 16) >> k.min(31)) & 1) << 16
    }
}

/// Pixel (`x`, `y`) of character `c` in the font `g` (each floored; `x`
/// a column from the cell's left, `y` a row from its top): 1 when lit, else
/// 0, and 0 outside the cell or for a character the font hasn't. One
/// lookup.
#[inline(always)]
pub fn glyph(g: &Glyphs, c: i32, x: i32, y: i32) -> i32 {
    let (c, x, y) = (c >> 16, (x >> 16) as u32, (y >> 16) as u32);
    let k = c.wrapping_sub(g.lo) as u32;
    if k >= g.n || x >= g.w || y >= g.h {
        return 0;
    }
    let at = (c + g.off) as u32 * g.h + y;
    match g.rows.get(at as usize) {
        Some(&r) => (((r << x) >> 7) as i32 & 1) << 16,
        None => 0,
    }
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
    p[pal_index(k)]
}

/// The entry `pal` reads for `k`: 0..=31.
#[inline(always)]
pub fn pal_index(k: i32) -> usize {
    // (`clamp(k, 0, ONE) >> 11`, at most 31: the same without a constant
    // the ESP32 loads from flash.)
    ((k.max(0) >> 11) as u32).min(31) as usize
}

/// `pal(p, k) * s`: the palette's colour with each channel times `s`, as
/// `rgb(r·s, g·s, b·s)` of its channels read as 0..1 would be.
#[inline(always)]
pub fn pal_lit(p: &[[u8; 3]; 32], k: i32, s: i32) -> [u8; 3] {
    scale_rgb(pal(p, k), s)
}

/// A colour's channels each times `s` (`pal_lit` of its entry).
#[inline(always)]
pub fn scale_rgb(c: [u8; 3], s: i32) -> [u8; 3] {
    // Exactly `chan(mul(byte(b), s))` for each channel b, without the
    // 64-bit multiply where it isn't needed (the ESP32 has no 64-bit one):
    // s ≤ 0 is 0 (the product is never positive); s = 1 is the byte again
    // (`chan` round-trips `byte`); 0 < s < 1 fits a u32 (byte ≤ 65,536,
    // s < 65,536), and the product is under 1, so `chan`'s clamp is idle.
    if s <= 0 {
        return [0; 3];
    }
    if s == ONE {
        return c;
    }
    if s < ONE {
        let f = |b: u8| {
            let v = (byte(b) as u32 * s as u32) >> 16;
            ((v * 255 + 0x8000) >> 16) as u8
        };
        return [f(c[0]), f(c[1]), f(c[2])];
    }
    [
        chan(mul(byte(c[0]), s)),
        chan(mul(byte(c[1]), s)),
        chan(mul(byte(c[2]), s)),
    ]
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

// --- Wide values: `t` in a `loop forever` formula -----------------------------
//
// Q48.16 in an i64: the same scale as every other value, so wherever a wide
// value fits a Q16.16 each `w_*` gives exactly the bits its Q16.16 operation
// gives (`w_mul` is `mul`, `w_div` is `div`, `w_mod` is `modulo`, …), and
// past it carries on instead of saturating. Sums and products wrap at 2^64
// (the whole part at 2^47 s, 4 million years of `t`); the ways back to a
// Q16.16 (`w_frac`, `w_mod`, compares, `w_minmax`, `w_reduce`, `w_lat`) are
// exact for any value.

/// The axes' primes of [`noise`]'s lattice, for [`w_lat`].
pub const PRIMES: [u32; 3] = [P1, P2, P3];

/// `a · k` (k a Q16.16), floored, wrapping: `mul` without the saturation.
#[inline]
pub fn w_mul(a: i64, k: i32) -> i64 {
    // a = hi·2^32 + lo: (hi·k)·2^16 + floor(lo·k / 2^16), each part within
    // an i64 (|hi·k| < 2^62, |lo·k| < 2^63); the first is whole, so the
    // floor is the whole product's.
    let (hi, lo) = (a >> 32, a as u32 as i64);
    (hi * k as i64)
        .wrapping_shl(16)
        .wrapping_add((lo * k as i64) >> 16)
}

/// `a / k` (k a Q16.16), truncated as `div` is; by 0 the end of `a`'s sign
/// (0 for 0 / 0).
#[inline]
pub fn w_div(a: i64, k: i32) -> i64 {
    if k == 0 {
        return match a {
            0 => 0,
            a if a > 0 => i64::MAX,
            _ => i64::MIN,
        };
    }
    // (a·2^16) / k without the 2^16 overflowing: a = q·k + r, r with a's
    // sign and |r| < |k|, so both parts truncate the same way.
    let k = k as i64;
    let (q, r) = (a.wrapping_div(k), a.wrapping_rem(k));
    q.wrapping_shl(16).wrapping_add((r << 16) / k)
}

/// Floored `a % b` (`modulo`'s rule: the divisor's sign; by 0, 0): within
/// b, so a Q16.16.
#[inline]
pub fn w_mod(a: i64, b: i32) -> i32 {
    if b == 0 {
        return 0;
    }
    let r = a.wrapping_rem(b as i64);
    (if r != 0 && ((r < 0) != (b < 0)) {
        r + b as i64
    } else {
        r
    }) as i32
}

/// The fraction, as `frac`.
#[inline]
pub fn w_frac(a: i64) -> i32 {
    (a & 0xffff) as i32
}

/// `floor`.
#[inline]
pub fn w_floor(a: i64) -> i64 {
    a & !0xffff
}

/// A compare (`c` one of the compare steps, `LT`..`NE` of
/// `program::code`) of `a` with the Q16.16 `b`: 1 or 0.
#[inline]
pub fn w_cmp(c: u8, a: i64, b: i32) -> i32 {
    use crate::program::code::*;
    let b = b as i64;
    b2f(match c {
        LT => a < b,
        LE => a <= b,
        GT => a > b,
        GE => a >= b,
        EQ => a == b,
        _ => a != b,
    })
}

/// `min` (`c` = `MIN`) or `max` of `a` and the Q16.16 `b`, saturated to a
/// Q16.16 (exactly `min`/`max` wherever `a` fits).
#[inline]
pub fn w_minmax(c: u8, a: i64, b: i32) -> i32 {
    let b = b as i64;
    sat(if c == crate::program::code::MIN {
        a.min(b)
    } else {
        a.max(b)
    })
}

/// `a` as 32 bits for `hash`: its own Q16.16 bits wherever it fits one (so
/// `hash` of it is what it always was), and past that its high bits mixed
/// in, so hashes don't repeat every 65,536 s.
#[inline]
pub fn w_reduce(a: i64) -> i32 {
    // 0 exactly when `a` fits an i32 (bits 31-63 all its sign).
    let e = (a >> 31) ^ (a >> 63);
    (a as u32 ^ mix32(e as u32 ^ (e >> 32) as u32)) as i32
}

/// The lattice offset of `a`'s whole part on `axis` (0 x, 1 y, 2 z): the
/// low 32 bits of floor(a), times the axis's prime ([`noise_off`]).
#[inline]
pub fn w_lat(a: i64, axis: usize) -> u32 {
    ((a >> 16) as u32).wrapping_mul(PRIMES[axis % 3])
}

/// `(a·b + c) >> 16`, saturated, `c` raw: a split's part times k with the
/// wide part's remainder below the point carried in (`w_rem`), so the two
/// parts' floors add up to the whole product's.
#[inline(always)]
pub fn mulr(a: i32, b: i32, c: i32) -> i32 {
    sat((a as i64 * b as i64 + c as i64) >> 16)
}

/// The 16 bits below the point of `a · k` (what `w_mul` floors away).
#[inline]
pub fn w_rem(a: i64, k: i32) -> i32 {
    ((a as u32 as i64 * k as i64) & 0xffff) as i32
}
