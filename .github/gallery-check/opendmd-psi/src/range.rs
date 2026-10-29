//! What each value of a laid-out program can be (an interval), and the
//! pixel steps that can therefore skip work and still give the same bits.
//!
//! [`narrow`] walks the steps in the order the VM runs them (per frame,
//! the draw stage, per row over every row, per pixel) with an interval
//! for every scalar slot and lane register, the inputs within the ranges
//! [`inputs_in_range`] checks (`t` within the loop, the sliders 0..1, the
//! clock's whole hours, minutes and seconds), and rewrites:
//!
//! (A slot a `control` line declares is read only through the clamp to its
//! range that the line made: its raw value is taken as anything, so the
//! clamp's interval is the range whatever the path says, and the frame
//! needs no check of it.)
//!
//! - `mul(v, 2^-n)` (either order, the power of two a scalar whose range
//!   is that one value, n 0..=16) as `shr(v, n)`: exactly `v >> n`, the
//!   scalar kept in `b` so out of range it is `mul(v, b)` again;
//! - `add`/`sub`/`mul` whose exact result always fits an i32 as `add_w`,
//!   `sub_w`, `mul_w` (no saturation test), and a `mul` whose 64-bit
//!   product always fits an i32 as `mul_n` (one 32-bit multiply).
//!
//! In range those give exactly what the checked steps give (saturation
//! only ever acts outside i32). A frame whose inputs are outside the
//! ranges runs the checked steps instead (the VM asks
//! [`inputs_in_range`]), so the bytes are the same for any inputs.

use crate::fixed::ONE;
use crate::program::*;
use crate::vm::Inputs;

/// An interval of i32 values, held as i64 so sums and products of two
/// fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Iv(pub i64, pub i64);

const LO: i64 = i32::MIN as i64;
const HI: i64 = i32::MAX as i64;
const FULL: Iv = Iv(LO, HI);
const UNIT: Iv = Iv(0, ONE as i64);
const SIGNED: Iv = Iv(-(ONE as i64), ONE as i64);

impl Iv {
    fn point(v: i32) -> Iv {
        Iv(v as i64, v as i64)
    }

    /// Clamped into i32, as saturation does.
    fn sat(self) -> Iv {
        Iv(self.0.clamp(LO, HI), self.1.clamp(LO, HI))
    }

    fn fits(self) -> bool {
        self.0 >= LO && self.1 <= HI
    }

    fn union(self, o: Iv) -> Iv {
        Iv(self.0.min(o.0), self.1.max(o.1))
    }

    fn konst(self) -> Option<i32> {
        (self.0 == self.1).then_some(self.0 as i32)
    }
}

/// The highest `t` a frame of a loop of `loop_ms` is drawn at: every player
/// draws `t` = `Inputs::at_ms(ms)` for an `ms` under the loop.
fn t_max(loop_ms: u32) -> i32 {
    Inputs::at_ms(loop_ms)
}

/// Whether a frame's inputs are within the ranges [`narrow`] assumed for
/// `prog` (else the VM runs the checked steps).
#[inline]
pub(crate) fn inputs_in_range(prog: &Program, i: &Inputs) -> bool {
    let unit = |v: i32| (0..=ONE).contains(&v);
    // (A `loop forever` formula reads `tw`, never `t`; its wide values
    // come back to Q16.16 only through steps whose ranges hold for any
    // `tw`, and `seed` is 0..1.)
    let free = prog.meta.declared();
    (prog.forever() || (0..=t_max(prog.loop_ms)).contains(&i.t))
        && (prog.uses & SEEDED == 0 || unit(i.seed))
        && (free & 1 != 0 || unit(i.a))
        && (free & 2 != 0 || unit(i.b))
        && (free & 4 != 0 || unit(i.c))
        && i.hour < 24
        && i.minute < 60
        && i.second < 60
}

/// The checked step a narrowed one stands for (itself for the rest:
/// `shr` is exact for any value).
#[inline(always)]
pub(crate) fn checked(c: u8) -> u8 {
    match c {
        code::ADD_W => code::ADD,
        code::SUB_W => code::SUB,
        // (A shift keeps the scalar it stands for in `b`: its constant may
        // be one only for inputs in range.)
        code::MUL_W | code::MUL_N | code::SHR => code::MUL,
        c => c,
    }
}

fn mul_corners(a: Iv, b: Iv) -> Iv {
    let c = [a.0 * b.0, a.0 * b.1, a.1 * b.0, a.1 * b.1];
    Iv(*c.iter().min().unwrap_or(&0), *c.iter().max().unwrap_or(&0))
}

fn add_iv(a: Iv, b: Iv) -> Iv {
    Iv(a.0 + b.0, a.1 + b.1)
}

fn sub_iv(a: Iv, b: Iv) -> Iv {
    Iv(a.0 - b.1, a.1 - b.0)
}

/// `mul`'s exact result (before saturation): the corners' products, each
/// shifted down (floor, monotonic).
fn mul_iv(a: Iv, b: Iv) -> Iv {
    let p = mul_corners(a, b);
    Iv(p.0 >> 16, p.1 >> 16)
}

/// What a step's value can be, from its operands'. `tabs` for `tab`.
#[inline(never)]
fn value(op: &Op, a: Iv, b: Iv, c: Iv, tabs: &[i32; MAX_TABLE]) -> Iv {
    use code::*;
    match op.code {
        ADD | ADD_W => add_iv(a, b).sat(),
        SUB | SUB_W => sub_iv(a, b).sat(),
        MUL | MUL_W | MUL_N => mul_iv(a, b).sat(),
        SHR => Iv(a.0 >> (op.c & 31), a.1 >> (op.c & 31)),
        DIV => div_iv(a, b),
        RECIP => div_iv(Iv::point(ONE), a),
        MOD => mod_iv(b),
        MASK => {
            if b.0 >= 0 {
                Iv(0, b.1)
            } else if a.0 >= 0 {
                Iv(0, a.1)
            } else {
                FULL
            }
        }
        LT | LE | GT | GE | EQ | NE | AND | OR | STEP | NOT | BIT => UNIT,
        DIGIT..=DIGIT_B | U_R..=INK | SMOOTH => UNIT,
        MIN => Iv(a.0.min(b.0), a.1.min(b.1)),
        MAX => Iv(a.0.max(b.0), a.1.max(b.1)),
        ATAN2 => Iv(-(ONE as i64) / 2, ONE as i64 / 2),
        HASH | NOISE | NOISE_O..=NOISE_O_LAST => Iv(0, 0xffff),
        // Wide steps back to a Q16.16 (their `a` is a wide slot, whatever
        // its value): a fraction, a remainder within `b`, 1 or 0, at most
        // or at least `b`, any 32 bits.
        W_FRAC | W_NOISEX | W_REM => Iv(0, 0xffff),
        MULR => {
            let p = mul_corners(a, b);
            Iv((p.0 + c.0) >> 16, (p.1 + c.1) >> 16).sat()
        }
        W_MOD => mod_iv(b),
        W_CMP => UNIT,
        W_MINMAX if op.c == MIN => Iv(LO, b.1),
        W_MINMAX => Iv(b.0, HI),
        LEN | SQRT => Iv(0, HI),
        NEG => Iv(-a.1, -a.0).sat(),
        ABS => {
            if a.0 >= 0 {
                a
            } else if a.1 <= 0 {
                Iv(-a.1, -a.0).sat()
            } else {
                Iv(0, (-a.0).max(a.1)).sat()
            }
        }
        FLOOR => Iv(a.0 & !0xffff, a.1 & !0xffff),
        FRAC => {
            if a.0 >> 16 == a.1 >> 16 {
                Iv(a.0 & 0xffff, a.1 & 0xffff)
            } else {
                Iv(0, 0xffff)
            }
        }
        // The sine table's entries are within ±16,384 (a test checks):
        // interpolated and shifted, ±1.
        SIN | COS | TRI | SAW | SQR => SIGNED,
        SPR => {
            // 0 outside, else one of its keys' numbers.
            let at = op.c as usize;
            let keys = (tabs[at] as u32 >> 20) & 31;
            let mut r = Iv::point(0);
            for k in 0..keys as usize {
                r = r.union(Iv::point(tabs[(at + 1 + k) & (MAX_TABLE - 1)]));
            }
            r
        }
        TAB => {
            let mut r = Iv::point(0);
            for k in 0..=op.c as usize {
                r = r.union(Iv::point(tabs[(op.b as usize + k) & (MAX_TABLE - 1)]));
            }
            r
        }
        SELECT => b.union(c),
        CLAMP => {
            let m = Iv(a.0.max(b.0), a.1.max(b.1));
            Iv(m.0.min(c.0), m.1.min(c.1))
        }
        MIX => add_iv(a, mul_iv(sub_iv(b, a).sat(), c).sat()).sat(),
        _ => FULL,
    }
}

/// A floored remainder by a divisor in `b`: the divisor's sign, within it
/// (0 by 0).
fn mod_iv(b: Iv) -> Iv {
    if b.0 > 0 {
        Iv(0, b.1 - 1)
    } else if b.1 < 0 {
        Iv(b.0 + 1, 0)
    } else {
        Iv((b.0 + 1).min(0), (b.1 - 1).max(0))
    }
}

/// `div`: `sat((a << 16) / b)` truncated; over a `b` of one sign the
/// quotient is monotonic in each operand, so the corners bound it. A `b`
/// that can be 0: anything.
fn div_iv(a: Iv, b: Iv) -> Iv {
    if b.0 <= 0 && b.1 >= 0 {
        // Over a `b` of one sign and 0 (a division by 0 is the end of the
        // numerator's sign, 0/0 is 0), the quotient's sign is still known.
        // Only over a `b` of 0 and up: a division by 0 is the end of
        // the *numerator's* sign, so with `b` 0 and down a positive
        // numerator still gives the top end (review, psi-speed).
        return if a.0 >= 0 && b.0 >= 0 {
            Iv(0, HI)
        } else if a.1 <= 0 && b.0 >= 0 {
            Iv(LO, 0)
        } else {
            FULL
        };
    }
    let q = |x: i64, y: i64| (x << 16) / y;
    let c = [q(a.0, b.0), q(a.0, b.1), q(a.1, b.0), q(a.1, b.1)];
    Iv(*c.iter().min().unwrap_or(&0), *c.iter().max().unwrap_or(&0)).sat()
}

/// Narrow `prog`'s pixel steps where the ranges allow (see the module).
///
/// Out of line, its intervals on the heap (like the rest of the compiler's
/// working memory): the compiler runs on a 12 KB stack of its own on the
/// ESP32.
#[inline(never)]
pub(crate) fn narrow(prog: &mut Program) {
    use code::*;
    let mut sc = alloc::vec![FULL; MAX_SCALARS];
    for (k, v) in sc.iter_mut().enumerate().skip(S_FIRST_FREE as usize) {
        *v = Iv::point(prog.init[k]);
    }
    sc[S_T as usize] = Iv(0, t_max(prog.loop_ms) as i64);
    sc[S_HOUR as usize] = Iv(0, 23 * ONE as i64);
    sc[S_MINUTE as usize] = Iv(0, 59 * ONE as i64);
    sc[S_SECOND as usize] = Iv(0, 59 * ONE as i64);
    for s in [S_A, S_B, S_C, S_Y] {
        sc[s as usize] = UNIT;
    }
    // A declared slot: anything (only its line's clamp reads it).
    for k in 0..3 {
        if prog.meta.declared() & (1 << k) != 0 {
            sc[(S_A + k as u8) as usize] = FULL;
        }
    }
    sc[S_PY as usize] = Iv(0, ((H - 1) * ONE as usize) as i64);
    if prog.uses & SEEDED != 0 {
        sc[S_SEED as usize] = UNIT;
    }
    let (fe, de, re, pe) = (
        (prog.frame_end as usize).min(MAX_OPS),
        (prog.draw_end as usize).min(MAX_OPS),
        (prog.row_end as usize).min(MAX_OPS),
        prog.len().min(MAX_OPS),
    );
    let (ops, tabs) = (&mut prog.ops, &prog.tables);
    let g = |sc: &[Iv], v: u8| sc[(v & 0x7f) as usize];
    // Per frame, the draw stage, then per row (over every row: y and py
    // are ranges). Every slot is written by one step, before any step
    // reads it (`emit`), so one walk in order is exact.
    for &op in &ops[..fe] {
        // (A wide step's own slots are not scalar ones: only those that come
        // back to a Q16.16 write one.)
        if code::writes_scalar(op.code) {
            sc[(op.d & 0x7f) as usize] = value(&op, g(&sc, op.a), g(&sc, op.b), g(&sc, op.c), tabs);
        }
    }
    // The draw stage's slots (its counters and values): anything.
    for op in &ops[fe.min(de)..de] {
        match op.code {
            // A loop's counter (END counts in `a`); the primitives write
            // nothing (their `d` is 0: `t`'s slot, not theirs).
            END => sc[(op.a & 0x7f) as usize] = FULL,
            LINE_FROM | LINE_TO | DOT => {}
            _ => sc[(op.d & 0x7f) as usize] = FULL,
        }
    }
    for &op in &ops[de.min(re)..re] {
        sc[(op.d & 0x7f) as usize] = value(&op, g(&sc, op.a), g(&sc, op.b), g(&sc, op.c), tabs);
    }
    let mut regs = [FULL; REGS + 1];
    let lane = |regs: &[Iv; REGS + 1], sc: &[Iv], v: u8| -> Iv {
        if v & LANE == 0 {
            return sc[(v & 0x7f) as usize];
        }
        match v & !LANE {
            L_X => UNIT,
            L_PX => Iv(0, ((W - 1) * ONE as usize) as i64),
            L_I => Iv(0, ((W * H - 1) * ONE as usize) as i64),
            l => regs[((l - L_I) as usize).min(REGS)],
        }
    };
    for slot in &mut ops[re.min(pe)..pe] {
        let mut op = *slot;
        if code::is_colour(op.code) || matches!(op.code, STILL | COLUMN | FUSE) {
            continue;
        }
        if op.d & LANE == 0 || op.d & !LANE < L_REG0 {
            continue;
        }
        let (a, b) = (lane(&regs, &sc, op.a), lane(&regs, &sc, op.b));
        let lanes = |v: u8| v & LANE != 0;
        match op.code {
            ADD if add_iv(a, b).fits() => op.code = ADD_W,
            SUB if sub_iv(a, b).fits() => op.code = SUB_W,
            MUL => {
                // A power of two up to 1 as a scalar constant: a shift.
                let pow = |v: Iv| {
                    v.konst()
                        .filter(|&k| k > 0 && k <= ONE && (k as u32).is_power_of_two())
                        .map(|k| 16 - (k as u32).trailing_zeros() as u8)
                };
                let shift = match (pow(b), pow(a)) {
                    (Some(n), _) if !lanes(op.b) => Some((op.a, n)),
                    (_, Some(n)) if !lanes(op.a) => Some((op.b, n)),
                    _ => None,
                };
                if let Some((v, n)) = shift {
                    // The scalar stays in `b`: out of range the step is
                    // `mul(a, b)` again (`checked`).
                    let k = if v == op.a { op.b } else { op.a };
                    op = Op {
                        code: SHR,
                        d: op.d,
                        a: v,
                        b: k,
                        c: n,
                    };
                } else if mul_corners(a, b).fits() {
                    op.code = MUL_N;
                } else if mul_iv(a, b).fits() {
                    op.code = MUL_W;
                }
            }
            _ => {}
        }
        let r = value(
            &op,
            lane(&regs, &sc, op.a),
            lane(&regs, &sc, op.b),
            lane(&regs, &sc, op.c),
            tabs,
        );
        let di = ((op.d & !LANE) - L_I) as usize;
        regs[di.min(REGS)] = r;
        *slot = op;
    }
}
