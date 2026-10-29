//! The ESP32's 32-bit forms of `div`, `recip`, `sqrt`, `len` and `atan2`
//! (branch fast-div) against their definitions, the 64-bit code they
//! replaced: every edge, sign, zero divisor and saturation, dense windows
//! and random sampling. The `#[ignore]`d tests are exhaustive (run with
//! `cargo test --release -- --ignored exhaustive`).

use crate::fixed::{self, ONE};
use crate::tables::{ATAN, SQRT};

// --- The definitions (the code as it was before fast-div) -------------------

fn div_ref(a: i32, b: i32) -> i32 {
    if b == 0 {
        return match a {
            0 => 0,
            a if a > 0 => i32::MAX,
            _ => i32::MIN,
        };
    }
    fixed::sat(((a as i64) << 16) / b as i64)
}

fn sqrt_q32_ref(v: u64) -> i32 {
    if v == 0 {
        return 0;
    }
    let lz = v.leading_zeros() as i32;
    let s = lz & !1;
    let w = ((v << s) >> 34) as u32;
    let r = fixed::sqrt30(w) as u64;
    let sh = 17 - s / 2;
    let out = if sh >= 0 { r << sh } else { r >> -sh };
    fixed::sat(out as i64)
}

fn sqrt_ref(a: i32) -> i32 {
    if a <= 0 {
        return 0;
    }
    sqrt_q32_ref((a as u64) << 16)
}

fn len_ref(a: i32, b: i32) -> i32 {
    let (a, b) = (a.unsigned_abs() as u64, b.unsigned_abs() as u64);
    sqrt_q32_ref((a * a).saturating_add(b * b))
}

fn atan2_ref(y: i32, x: i32) -> i32 {
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
    let r = (lo << 16) / hi;
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

// --- Inputs ----------------------------------------------------------------

/// Every power of two and its neighbours, both signs, and the ends.
fn edges() -> Vec<i32> {
    let mut v = vec![0, 1, -1, 2, -2, 3, -3, ONE, -ONE, ONE / 2, -ONE / 2];
    for k in 0..32 {
        let p = 1i64 << k;
        for d in -3i64..=3 {
            for s in [1i64, -1] {
                let x = s * (p + d);
                if x >= i32::MIN as i64 && x <= i32::MAX as i64 {
                    v.push(x as i32);
                }
            }
        }
    }
    v.extend([
        i32::MIN,
        i32::MIN + 1,
        i32::MIN + 2,
        i32::MAX,
        i32::MAX - 1,
        i32::MAX - 2,
    ]);
    // Divisors whose Knuth digit needs both corrections: a top half just
    // over 2^15 and a big low half, at every normalising shift.
    for dh in [0x8000u32, 0x8001, 0x8002, 0xaaaa, 0xfffe, 0xffff] {
        for dl in [0u32, 1, 0x7fff, 0x8000, 0xfffe, 0xffff] {
            let dn = (dh << 16) | dl;
            for s in 0..16 {
                let d = dn >> s;
                v.push(d as i32);
                v.push((d as i32).wrapping_neg());
            }
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// xorshift32.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    /// A value of any magnitude (a random number of bits), either sign.
    fn any(&mut self) -> i32 {
        let v = self.next();
        let bits = self.next() % 33;
        let m = if bits == 32 {
            v
        } else {
            v & ((1u32 << bits) - 1)
        };
        if self.next() & 1 == 0 {
            m as i32
        } else {
            (m as i32).wrapping_neg()
        }
    }
}

// --- div --------------------------------------------------------------------

#[test]
fn div_u32_is_the_64_bit_division_at_the_edges() {
    let e = edges();
    for &a in &e {
        for &b in &e {
            if b != 0 {
                assert_eq!(fixed::div_u32(a, b), div_ref(a, b), "div({a}, {b})");
            }
            assert_eq!(fixed::div(a, b), div_ref(a, b), "div({a}, {b})");
        }
    }
    // Zero divisors: the numerator's end (0 for 0 / 0).
    assert_eq!(fixed::div(0, 0), 0);
    assert_eq!(fixed::div(1, 0), i32::MAX);
    assert_eq!(fixed::div(-1, 0), i32::MIN);
    assert_eq!(fixed::div(i32::MIN, 0), i32::MIN);
    // Saturation both ways, and exactly i32::MIN.
    assert_eq!(fixed::div_u32(i32::MIN, 1), i32::MIN);
    assert_eq!(fixed::div_u32(i32::MAX, 1), i32::MAX);
    assert_eq!(fixed::div_u32(i32::MIN, -1), i32::MAX);
    assert_eq!(fixed::div_u32(-(1 << 15), 1), i32::MIN);
    assert_eq!(fixed::div_u32(1 << 15, 1), i32::MAX);
    assert_eq!(fixed::div_u32(i32::MIN, ONE), i32::MIN);
    assert_eq!(fixed::div_u32(i32::MIN, i32::MIN), ONE);
    // Truncation towards zero, as the 64-bit division.
    assert_eq!(fixed::div_u32(-1, 3), -21845);
    assert_eq!(fixed::div_u32(-1, 3 * ONE), 0);
    assert_eq!(fixed::div_u32(-ONE, 3 * ONE), -21845);
    assert_eq!(fixed::div_u32(ONE, -3 * ONE), -21845);
}

#[test]
fn div_u32_is_the_64_bit_division_densely() {
    // Every numerator and divisor in a window round each scale.
    for c in [0i32, 1 << 15, ONE, 1 << 20, 1 << 24, 1 << 30] {
        for d in [1i32, 1 << 8, 1 << 15, ONE, 1 << 20, 1 << 28] {
            for a in c.saturating_sub(40)..c.saturating_add(40) {
                for b in d - 40..d + 40 {
                    for (a, b) in [(a, b), (-a, b), (a, -b), (-a, -b)] {
                        if b != 0 {
                            assert_eq!(fixed::div_u32(a, b), div_ref(a, b), "div({a}, {b})");
                        }
                    }
                }
            }
        }
    }
    // Small values, all pairs.
    for a in -300..300 {
        for b in -300..300 {
            if b != 0 {
                assert_eq!(fixed::div_u32(a, b), div_ref(a, b), "div({a}, {b})");
            }
        }
    }
}

#[test]
fn div_u32_is_the_64_bit_division_at_random() {
    let mut r = Rng(0x2545_f491);
    for _ in 0..2_000_000 {
        let (a, b) = (r.any(), r.any());
        if b != 0 {
            assert_eq!(fixed::div_u32(a, b), div_ref(a, b), "div({a}, {b})");
        }
    }
    // Psi's own magnitudes: pixels and distances (0..~200 in Q16.16).
    for _ in 0..1_000_000 {
        let a = (r.next() % (300 << 16)) as i32 - (150 << 16);
        let b = (r.next() % (300 << 16)) as i32 - (150 << 16);
        if b != 0 {
            assert_eq!(fixed::div_u32(a, b), div_ref(a, b), "div({a}, {b})");
        }
    }
}

#[test]
fn the_knuth_digit_needs_both_corrections_and_gets_them_right() {
    // r just under d, for divisors with a 0x8000 top half and a big low
    // half: the first estimate is 2^16 + 1, two too big.
    let mut r = Rng(0x9e37_79b9);
    for dh in 0x8000u32..0x8040 {
        for dl in (0xff00u32..=0xffff).chain(0..0x40) {
            let d = (dh << 16) | dl;
            // (Shifted at least once: the divisor must fit an i32.)
            for s in [1, 7, 15] {
                let d = d >> s;
                for n in [d - 1, d - 2, d / 2, d - 1 - (r.next() % 0x1_0000)] {
                    let want = ((n as u64) << 16) / d as u64;
                    assert_eq!(fixed::div_u32(n as i32, d as i32) as u64, want, "{n} / {d}");
                }
            }
        }
    }
}

#[test]
#[ignore]
fn exhaustive_div_u32_over_every_numerator() {
    // Every a against a spread of divisors (a few minutes in release).
    let e: Vec<i32> = edges().into_iter().filter(|&b| b != 0).step_by(7).collect();
    for a in i32::MIN..=i32::MAX {
        for &b in &e {
            assert_eq!(fixed::div_u32(a, b), div_ref(a, b), "div({a}, {b})");
        }
    }
}

// --- recip ------------------------------------------------------------------

#[test]
fn recip_u32_is_one_over() {
    for &a in &edges() {
        assert_eq!(fixed::recip_u32(a), div_ref(ONE, a), "recip({a})");
        assert_eq!(fixed::recip(a), div_ref(ONE, a), "recip({a})");
    }
    for a in -(1 << 18)..(1 << 18) {
        assert_eq!(fixed::recip_u32(a), div_ref(ONE, a), "recip({a})");
    }
    let mut r = Rng(0x1234_5678);
    for _ in 0..2_000_000 {
        let a = r.any();
        assert_eq!(fixed::recip_u32(a), div_ref(ONE, a), "recip({a})");
    }
}

#[test]
#[ignore]
fn exhaustive_recip_u32() {
    for a in i32::MIN..=i32::MAX {
        assert_eq!(fixed::recip_u32(a), div_ref(ONE, a), "recip({a})");
    }
}

// --- The leading zeros -----------------------------------------------------

#[test]
fn lz32_is_leading_zeros() {
    // (On the host it *is* `leading_zeros`; on the ESP32 `nsau`, which the
    // ISA defines the same, 32 for 0; psi-probe checks it there.)
    for k in 0..32 {
        for v in [1u32 << k, (1u32 << k) - 1, (1u32 << k) + 1] {
            assert_eq!(fixed::lz32(v), v.leading_zeros());
        }
    }
    assert_eq!(fixed::lz32(0), 32);
}

// --- sqrt, len, atan2 -------------------------------------------------------

#[test]
fn sqrt_is_the_64_bit_root() {
    for &a in &edges() {
        assert_eq!(fixed::sqrt(a), sqrt_ref(a), "sqrt({a})");
    }
    for a in 0..(1 << 20) {
        assert_eq!(fixed::sqrt(a), sqrt_ref(a), "sqrt({a})");
    }
    let mut r = Rng(0x0bad_cafe);
    for _ in 0..2_000_000 {
        let a = r.any();
        assert_eq!(fixed::sqrt(a), sqrt_ref(a), "sqrt({a})");
    }
}

#[test]
#[ignore]
fn exhaustive_sqrt() {
    for a in 0..=i32::MAX {
        assert_eq!(fixed::sqrt(a), sqrt_ref(a), "sqrt({a})");
    }
}

#[test]
fn len_is_the_64_bit_length() {
    let e = edges();
    for &a in &e {
        for &b in &e {
            assert_eq!(fixed::len(a, b), len_ref(a, b), "len({a}, {b})");
        }
    }
    assert_eq!(fixed::len(i32::MIN, i32::MIN), i32::MAX);
    for a in -300..300 {
        for b in -300..300 {
            assert_eq!(fixed::len(a, b), len_ref(a, b), "len({a}, {b})");
        }
    }
    let mut r = Rng(0x5eed_1e55);
    for _ in 0..2_000_000 {
        let (a, b) = (r.any(), r.any());
        assert_eq!(fixed::len(a, b), len_ref(a, b), "len({a}, {b})");
    }
    // The panel's distances, every pixel at quarter-pixel steps.
    for x in -1000..1000 {
        for y in -200..200 {
            let (a, b) = (x << 14, y << 14);
            assert_eq!(fixed::len(a, b), len_ref(a, b), "len({a}, {b})");
        }
    }
}

#[test]
fn atan2_is_the_old_angle() {
    let e = edges();
    for &y in &e {
        for &x in &e {
            assert_eq!(fixed::atan2(y, x), atan2_ref(y, x), "atan2({y}, {x})");
        }
    }
    for y in -300..300 {
        for x in -300..300 {
            assert_eq!(fixed::atan2(y, x), atan2_ref(y, x), "atan2({y}, {x})");
        }
    }
    let mut r = Rng(0x7357_a7a2);
    for _ in 0..2_000_000 {
        let (y, x) = (r.any(), r.any());
        assert_eq!(fixed::atan2(y, x), atan2_ref(y, x), "atan2({y}, {x})");
    }
    for x in -1000..1000 {
        for y in -200..200 {
            let (a, b) = (y << 14, x << 14);
            assert_eq!(fixed::atan2(a, b), atan2_ref(a, b), "atan2({a}, {b})");
        }
    }
}

#[test]
fn the_tables_are_the_ones_measured() {
    // (So a table change can't hide behind both sides of these tests.)
    assert_eq!(SQRT.len(), 193);
    assert_eq!(ATAN.len(), 257);
}
