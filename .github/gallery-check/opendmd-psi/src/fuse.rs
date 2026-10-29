//! Fused pixel steps: short runs of steps that the VM works out in one
//! loop over the lane instead of one loop each (the per-element load,
//! store and loop of each step are most of a cheap step's cost on the
//! ESP32).
//!
//! [`fuse`] runs after `range::narrow` on a laid-out program and puts a
//! marker (`code::FUSE d, a`: kind `d`, the `a` steps after it) before each
//! run it recognises. **The steps stay**: the VM runs the run's kernel in
//! their place only on a frame that runs the narrowed steps (and never in
//! wasm); a frame that runs the checked steps, or a kernel that does not
//! take its operands' kinds, runs the steps as ever. Each kernel is the
//! composition of the same `fixed` functions the steps call, in the same
//! order, so its value is the steps' value bit for bit; what [`fuse`]
//! checks is that skipping the steps' other writes changes nothing: every
//! value a run works out on the way is read by nothing after it (it is
//! written again first), the run reads nothing it wrote except along its
//! own pattern, and a kernel's destination is none of its sources.
//!
//! Only in the steps after the column run and the still cache's run (the
//! markers' counts are left as they are), and only where the step limit
//! leaves room for the markers; they are not counted as steps
//! ([`Program::steps`]).

use crate::program::*;
use alloc::vec::Vec;

/// `mask(add_w(floor(x), floor(y)), m)`: a checker's `(floor(u) +
/// floor(v)) % 2^k`, x and y lanes, m a value for the lane.
pub(crate) const F_CHECKER: u8 = 1;
/// A checker's last three steps, `mask(add_w(r, floor(y)), m)`, where the
/// first floor (`r`, a register) was worked out earlier (not next to the
/// second: Psi lays steps out as their values are needed). [`fuse`] checks
/// that the step that last wrote `r` is a floor, so the kernel is
/// [`F_CHECKER`]'s (`floor(r) == r`): one loop for both.
pub(crate) const F_CHECKER3: u8 = 2;
/// The bottom layer's `pal(P, select(c, A, B))` (`* k`, k a value for the
/// lane), A and B values for the lane: two colours once a lane.
pub(crate) const F_SELPAL: u8 = 3;
/// [`F_SELPAL`] with k a lane: each choice's last scale and colour kept.
pub(crate) const F_SELPAL_K: u8 = 6;
/// The bottom layer's `pal(P, ink)` or `gray(ink)`: the canvas's bytes to
/// colours in one pass (no lane of `ink` written and read back).
pub(crate) const F_INK1: u8 = 7;
/// `add_w(mul_w(a, b), c)` (or `mul_n`, run as `mul_w`: the same bits on
/// a narrowed frame), a a lane, b a value for the lane, c either (a value
/// for the lane when the result goes into a's register). (Dropped after
/// s2probe-a's 18 ns a pixel, back after s2probe-b: Rotozoom floor 1,766
/// -> 1,965 µs and Tunnel 3,399 -> 3,479 without it, ~24 ns a pixel each.)
pub(crate) const F_MADD: u8 = 8;
/// The bottom layer's `pal(P, pick(m, …))` (`* k`, k a value for the
/// lane): the choices' colours worked out once a lane, then looked up.
pub(crate) const F_PICKPAL: u8 = 4;
/// A pick fused with its colour has at most this many choices.
pub(crate) const PICK_MAX: usize = 16;

/// What a fused colour costs in the JIT's kernel, ns a pixel, when the
/// kernel draws it in one piece (`pal_select`, `pal_pick`: a choice of
/// colours worked out a row); `None`: its steps, one at a time.
pub(crate) fn jit_ns(kind: u8) -> Option<u32> {
    match kind {
        F_SELPAL => Some(35),
        F_SELPAL_K => Some(80),
        F_PICKPAL => Some(45),
        _ => None,
    }
}

/// A kind's name, for listings.
pub(crate) fn name(kind: u8) -> &'static str {
    match kind {
        F_CHECKER => "checker",
        F_CHECKER3 => "checker_3",
        F_SELPAL => "pal_select",
        F_SELPAL_K => "pal_select_k",
        F_INK1 => "colour_of_ink_1",
        F_MADD => "mul_add",
        F_PICKPAL => "pal_pick",
        _ => "?",
    }
}

#[inline]
fn is_lane(v: u8) -> bool {
    v & LANE != 0
}

#[inline]
fn is_reg(v: u8) -> bool {
    v & LANE != 0 && v & !LANE >= L_REG0
}

/// The register a pixel step writes, if it writes one.
#[inline]
fn writes(op: &Op) -> Option<u8> {
    (code::writes_scalar(op.code) && is_reg(op.d)).then_some(op.d)
}

/// The bottom layer's palette colour reading `r` as its entry (and, scaled,
/// a scale other than `r`; `scalar_k`: a scale that is a value for the
/// lane).
fn pal_of(op: &Op, r: u8, scalar_k: bool) -> bool {
    if op.code != code::OUT_PAL || op.a != r {
        return false;
    }
    if op.d & PAL_SCALED == 0 {
        return true;
    }
    op.c != r && !(scalar_k && is_lane(op.c))
}

/// The pattern at `ops[0..]`: its kind, length, the values it works out on
/// the way (to be dead after it) and its kernel's destination (a register,
/// or 0 for one that writes the pixels).
fn pattern(ops: &[Op]) -> Option<(u8, usize, [u8; 3], u8)> {
    use code::*;
    let op = |k: usize| ops.get(k);
    let a = op(0)?;
    match a.code {
        FLOOR => {
            let (b, c) = (op(1)?, op(2)?);
            // `floor(x), floor(y), add_w, mask`: the four steps.
            if let Some(d) = op(3) {
                let (x, r1, y, r2) = (a.a, a.d, b.a, b.d);
                let sum = c.code == ADD_W && ((c.a == r1 && c.b == r2) || (c.a == r2 && c.b == r1));
                let ok = b.code == FLOOR
                    && is_lane(x)
                    && is_lane(y)
                    && y != r1
                    && r1 != r2
                    && sum
                    && d.code == MASK
                    && d.a == c.d
                    && !is_lane(d.b)
                    && d.d != x
                    && d.d != y;
                if ok {
                    return Some((F_CHECKER, 4, [r1, r2, c.d], d.d));
                }
            }
            // `floor(y), add_w(r, ·), mask`: the second floor alone, the
            // first's value `r` an operand (`fuse` checks that a floor
            // wrote it: the kernel floors it again).
            let (fy, add, m) = (a, b, c);
            let (y, ry) = (fy.a, fy.d);
            let r = if add.a == ry { add.b } else { add.a };
            let ok = is_lane(y)
                && add.code == ADD_W
                && (add.a == ry || add.b == ry)
                && r != ry
                && is_reg(r)
                && m.code == MASK
                && m.a == add.d
                && !is_lane(m.b)
                && m.d != y
                && m.d != r;
            ok.then_some((F_CHECKER3, 3, [ry, add.d, add.d], m.d))
        }
        MUL_W | MUL_N => {
            let b = op(1)?;
            let l = match (is_lane(a.a), is_lane(a.b)) {
                (true, false) => a.a,
                (false, true) => a.b,
                _ => return None,
            };
            let r = a.d;
            let c = if b.a == r { b.b } else { b.a };
            // (The destination may be the lane multiplied when the added
            // value is one for the lane: a kernel that reads and writes
            // its own lane.)
            // (`mul_n` runs as `mul_w`, the same bits on the frames a
            // kernel runs on: see `vm::fused`.)
            let ok = b.code == ADD_W
                && (b.a == r || b.b == r)
                && c != r
                && (b.d != l || !is_lane(c))
                && b.d != c;
            ok.then_some((F_MADD, 2, [r, r, r], b.d))
        }
        SELECT => {
            let b = op(1)?;
            let ok = is_lane(a.a) && !is_lane(a.b) && !is_lane(a.c) && pal_of(b, a.d, false);
            let lane_k = b.d & PAL_SCALED != 0 && is_lane(b.c);
            ok.then_some((if lane_k { F_SELPAL_K } else { F_SELPAL }, 2, [a.d; 3], 0))
        }
        PICK => {
            let b = op(1)?;
            let ok = is_lane(a.a) && a.c as usize <= PICK_MAX && pal_of(b, a.d, true);
            ok.then_some((F_PICKPAL, 2, [a.d; 3], 0))
        }
        INK => {
            // `pal(P, ink)` or `gray(ink)`, the bottom layer's plain colour.
            // (A longer colour of the canvas kept per byte lost on the
            // spare: s2probe-c's rgb rows −68 and −51 ns a pixel, the cube
            // 2,353 against 2,183 µs for its steps.)
            let b = op(1)?;
            let ok =
                b.a == a.d && (b.code == OUT_GRAY || (b.code == OUT_PAL && b.d & PAL_SCALED == 0));
            ok.then_some((F_INK1, 2, [a.d; 3], 0))
        }
        _ => None,
    }
}

/// Whether register `r` is read by nothing in `ops` before something
/// writes it (registers carry nothing from one lane of pixels to the next
/// but the column run's, which the compiler never writes again).
fn dead(ops: &[Op], r: u8) -> bool {
    for op in ops {
        let (v, n) = code::operands(op);
        if v[..n].contains(&r) {
            return false;
        }
        if writes(op) == Some(r) {
            return true;
        }
    }
    true
}

/// Whether the first floor's value that the [`F_CHECKER3`] run at `k`
/// adds (`r`) is a floor's: the last step before the run that writes `r`
/// (from the pixel steps' start, `re`: the column run's and the still
/// run's steps included, whose values are the same when kept or cached) is
/// a `floor`. A fused run before it writes only its last step's register
/// (its others are dead after it, so `r` is none of them).
fn floored(prog: &Program, re: usize, k: usize) -> bool {
    let (fy, add) = (&prog.ops[k], &prog.ops[k + 1]);
    let r = if add.a == fy.d { add.b } else { add.a };
    prog.ops[re..k]
        .iter()
        .rev()
        .find(|op| writes(op) == Some(r))
        .is_some_and(|op| op.code == code::FLOOR)
}

/// Put a marker before each run the VM may fuse (see the module).
#[inline(never)]
pub(crate) fn fuse(prog: &mut Program) {
    let (re, pe) = (prog.row_end as usize, prog.len().min(MAX_OPS));
    let mut start = re.max(prog.column_run().end);
    if let Some(k) = prog.still_marker() {
        start = start.max(k + 1 + prog.ops[k].a as usize);
    }
    let room = MAX_OPS - pe;
    let mut runs: Vec<(usize, u8, usize)> = Vec::new();
    let mut k = start;
    while k < pe && runs.len() < room {
        let ops = &prog.ops[k..pe];
        if matches!(ops[0].code, code::STILL | code::COLUMN | code::FUSE) {
            k += 1;
            continue;
        }
        let ok =
            |n: usize, mid: [u8; 3], dst: u8| mid.iter().all(|&r| r == dst || dead(&ops[n..], r));
        match pattern(ops) {
            Some((kind, n, mid, dst))
                if ok(n, mid, dst) && (kind != F_CHECKER3 || floored(prog, re, k)) =>
            {
                runs.push((k, kind, n));
                k += n;
            }
            _ => k += 1,
        }
    }
    if runs.is_empty() {
        return;
    }
    // Insert the markers, from the end.
    let mut end = pe + runs.len();
    let mut src = pe;
    for &(at, kind, n) in runs.iter().rev() {
        while src > at {
            src -= 1;
            end -= 1;
            prog.ops[end] = prog.ops[src];
        }
        end -= 1;
        prog.ops[end] = Op {
            code: code::FUSE,
            d: kind,
            a: n as u8,
            b: 0,
            c: 0,
        };
    }
    debug_assert_eq!(end, src);
    prog.pixel_end = (pe + runs.len()) as u16;
}

/// What a fused run of kind `kind` saves, ns a pixel, against its steps'
/// costs: s2probe-b's `fuse` rows on the spare (2026-10-28), each pattern
/// alone (the multiply-add from s2probe-a and the two Rotozooms' change
/// between the probes).
pub(crate) fn saved(kind: u8) -> u32 {
    match kind {
        F_CHECKER => 74,
        F_CHECKER3 => 50,
        F_SELPAL => 188,
        F_SELPAL_K => 134,
        F_PICKPAL => 50,
        // The ink lane's write and read (`ink` 70 a pixel) and the colour's
        // own load: s2probe-c's rows 64 (lines) and 81 (blank), the lower.
        F_INK1 => 64,
        F_MADD => 22,
        _ => 0,
    }
}
