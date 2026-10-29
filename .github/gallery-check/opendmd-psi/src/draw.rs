//! The draw stage: `draw { … }` run once a frame, before the rows — its
//! loops and values (scalar steps), and the lines and dots it puts into the
//! canvas ([`Ink`]) that the pixel steps read back as `ink`.
//!
//! Integer arithmetic throughout (the scalar steps are `eval`'s, the
//! rasteriser is integer Bresenham as the built-in effects' was: `opendmd-core`'s
//! `fx_psi` tests keep it as `reference_line`), so a
//! frame's canvas is the same bytes on the ESP32, the host and in wasm. A
//! frame is still a pure function of its inputs: the canvas is cleared
//! first. Bounded whatever the program: [`TURNS`] loop turns, [`PRIMITIVES`]
//! lines and dots, [`LINE_STEPS`] steps a line and [`FRAME_STEPS`] a frame,
//! counted the same everywhere, so a program that reaches one draws the same
//! partial picture on every target.

use crate::fixed::{self, ONE};
use crate::program::*;
use crate::tables::Tables;
use crate::vm::Scratch;
use alloc::boxed::Box;

/// Loop turns a frame (each `for` body run counts one, nested ones too):
/// the compiler refuses more, and the VM stops at it anyway.
pub const TURNS: u32 = 2048;
/// Lines and dots a frame; past it the rest are skipped.
pub const PRIMITIVES: u32 = 2048;
/// Steps one line takes at most (as the built-in effects' rasteriser).
pub const LINE_STEPS: u32 = 1024;
/// Line steps a frame (the canvas four times over); past it the rest of
/// the frame's primitives are skipped.
pub const FRAME_STEPS: u32 = 16_384;

/// A line's ends are pixels in −512..639 across and −512..543 up (four
/// panels round it): enough for anything that crosses the panel.
const X_MIN: i32 = -512;
const X_MAX: i32 = W as i32 + 511;
const Y_MIN: i32 = -512;
const Y_MAX: i32 = H as i32 + 511;

/// The draw stage's canvas: 128 × 32 bytes, row-major from the top as the
/// frame is; 0 where nothing was drawn, 255 at full ink. Plain bytes, so a
/// zeroed allocation is a valid one. Only a program that
/// [`draws`](Program::draws) needs one.
#[repr(C)]
pub struct Ink(pub [u8; W * H]);

impl Ink {
    pub const fn new() -> Ink {
        Ink([0; W * H])
    }

    /// A canvas on the heap, made zeroed in place (4 KB never passes
    /// through the stack).
    pub fn boxed() -> Box<Ink> {
        Self::try_boxed().unwrap_or_else(|| alloc::alloc::handle_alloc_error(Self::LAYOUT))
    }

    /// [`boxed`](Self::boxed), or `None` when the heap hasn't 4 KB.
    pub fn try_boxed() -> Option<Box<Ink>> {
        // SAFETY: a non-zero-sized layout; `Ink` is bytes, so the zeroed
        // memory is a valid value; the box frees it with the same layout.
        unsafe {
            let p = alloc::alloc::alloc_zeroed(Self::LAYOUT).cast::<Ink>();
            (!p.is_null()).then(|| Box::from_raw(p))
        }
    }

    const LAYOUT: core::alloc::Layout = core::alloc::Layout::new::<Ink>();

    /// The byte at column `x`, row `y` from the top.
    pub fn at(&self, x: usize, y: usize) -> u8 {
        self.0[(y * W + x) & (W * H - 1)]
    }
}

impl Default for Ink {
    fn default() -> Ink {
        Ink::new()
    }
}

/// Run `prog`'s draw stage into `ink` (cleared first), the scalar slots
/// (`s.sc`) as the per-frame steps left them. With `lanes`, a loop that
/// [`Program::vector_loop`] allows runs as lanes ([`run_lanes`]; the lane
/// registers are free before the rows); without, every loop a turn at a
/// time (the same canvas and slots: the tests compare).
#[inline(never)]
pub(crate) fn run<const N: usize>(
    prog: &Program,
    s: &mut Scratch<N>,
    t: &Tables,
    ink: &mut Ink,
    lanes: bool,
) {
    ink.0.fill(0);
    let start = prog.frame_end as usize;
    let end = (prog.draw_end as usize).min(MAX_OPS);
    let m = |v: u8| (v & 0x7f) as usize;
    let mut pen = Pen {
        prims: 0,
        steps: 0,
        from: (0, 0, 0),
    };
    let mut turns = 0u32;
    let mut pc = start;
    while pc < end {
        let op = prog.ops[pc];
        pc += 1;
        let sc = &mut s.sc;
        match op.code {
            code::FOR => {
                sc[m(op.d)] = 0;
                // Out of turns: past the loop (its END), not into it.
                if turns >= TURNS {
                    pc = (op.b as usize + 1).max(pc);
                } else if lanes {
                    // Its turns (as many as a turn at a time would run:
                    // each END counts one, and the last is the one that
                    // reaches the count or the frame's turns).
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(vl) = prog.vector_loop(pc - 1) {
                        let n = vl.count.min(TURNS - turns);
                        run_lanes(prog, s, t, ink, &mut pen, pc - 1, &vl, n);
                        turns += n;
                        pc = vl.end + 1;
                    }
                }
            }
            code::END => {
                turns += 1;
                let k = fixed::add(sc[m(op.a)], ONE);
                sc[m(op.a)] = k;
                let back = op.c as usize;
                if k < sc[m(op.b)] && turns < TURNS && back > start && back < pc {
                    pc = back;
                }
            }
            code::LINE_FROM => pen.from = (sc[m(op.a)], sc[m(op.b)], sc[m(op.c)]),
            code::LINE_TO => {
                let to = (sc[m(op.a)], sc[m(op.b)], sc[m(op.c)]);
                pen.line(ink, to);
            }
            code::DOT => pen.dot(ink, sc[m(op.a)], sc[m(op.b)], sc[m(op.c)]),
            _ => super::vm::scalar(sc, &op, t, &prog.tables),
        }
    }
}

/// Lines and dots of a lane loop's body whose operands are looked up once
/// a lane of turns (more are looked up each turn).
#[cfg(not(target_arch = "wasm32"))]
const PRIMS: usize = 6;

/// `n` turns of the loop whose `FOR` is at `at` ([`Program::vector_loop`]
/// says it may), `N` turns at a time: each body step once over a lane of
/// turns (lane `j` turn `j`: the counter's lane holds j), then each turn's
/// lines and dots in order. Then the slots as a turn at a time leaves
/// them: the last turn's values, the counter at `n`.
///
/// Not in wasm: the web app's previews draw the same bytes a turn at a
/// time, without a second copy of the lane steps' dispatch in the
/// download.
#[cfg(not(target_arch = "wasm32"))]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn run_lanes<const N: usize>(
    prog: &Program,
    s: &mut Scratch<N>,
    t: &Tables,
    ink: &mut Ink,
    pen: &mut Pen,
    at: usize,
    vl: &VecLoop,
    n: u32,
) {
    use crate::program::LANE;
    let body = at + 1..vl.end;
    // A register's operand: `L_I + r` with the lane bit (register `r`
    // is `s.lanes[r]`).
    let lane = |r: usize| LANE | (2 + r as u8);
    let spare = vl.regs as usize + 1;
    let mut j0 = 0u32;
    while j0 < n {
        let m = (n - j0).min(N as u32) as usize;
        for (j, v) in s.lanes[1].iter_mut().enumerate() {
            *v = ((j0 as usize + j) as i32) << 16;
        }
        for op in &prog.ops[body.clone()] {
            if !code::writes_scalar(op.code) {
                continue;
            }
            let ar = code::arity(op.code);
            let mut o = *op;
            let mut any = false;
            for f in [&mut o.a, &mut o.b, &mut o.c].into_iter().take(ar) {
                if let Some(r) = vl.reg(*f) {
                    *f = lane(r);
                    any = true;
                }
            }
            let d = vl.reg(op.d).unwrap_or(spare).min(REGS);
            if !any {
                // Of values the same every turn: worked out once.
                let v = super::vm::scalar_value(&s.sc, op, t, &prog.tables);
                s.lanes[d].fill(v);
                continue;
            }
            // A step that writes a register it reads: into the spare one,
            // then copied (the lane loops write one register while
            // reading others).
            let reads_d = [o.a, o.b, o.c][..ar].contains(&lane(d));
            let w = if reads_d { spare.min(REGS) } else { d };
            o.d = lane(w);
            super::vm::lane_draw(s, &o, t, &prog.tables);
            if w != d {
                let (lo, hi) = s.lanes.split_at_mut(w);
                lo[d] = hi[0];
            }
        }
        // The lines and dots, turn by turn: each operand a register's
        // lane at the turn (`0x80 | r`) or a value the same every turn (its
        // slot), looked up once a lane of turns.
        let (sc, lanes) = (&s.sc, &s.lanes);
        let src = |v: u8| vl.reg(v).map_or(v & 0x7f, |r| 0x80 | r as u8);
        let at = |o: u8, j: usize| {
            if o & 0x80 != 0 {
                lanes[(o & 0x7f) as usize % (REGS + 1)][j]
            } else {
                sc[o as usize]
            }
        };
        let prim = |op: &Op| matches!(op.code, code::LINE_FROM | code::LINE_TO | code::DOT);
        let mut ps = [[0u8; 4]; PRIMS];
        let mut np = 0;
        for op in prog.ops[body.clone()].iter().filter(|op| prim(op)) {
            if np < PRIMS {
                ps[np] = [op.code, src(op.a), src(op.b), src(op.c)];
            }
            np += 1;
        }
        for j in 0..m {
            let mut one = |p: [u8; 4]| {
                let (a, b, c) = (at(p[1], j), at(p[2], j), at(p[3], j));
                match p[0] {
                    code::LINE_FROM => pen.from = (a, b, c),
                    code::LINE_TO => pen.line(ink, (a, b, c)),
                    _ => pen.dot(ink, a, b, c),
                }
            };
            if np <= PRIMS {
                for &p in &ps[..np] {
                    one(p);
                }
            } else {
                for op in prog.ops[body.clone()].iter().filter(|op| prim(op)) {
                    one([op.code, src(op.a), src(op.b), src(op.c)]);
                }
            }
        }
        j0 += m as u32;
    }
    // The slots as the last turn left them, the counter at n.
    let last = ((n - 1) % N as u32) as usize;
    for r in 2..=vl.regs as usize {
        s.sc[vl.slot[r] as usize] = s.lanes[r][last];
    }
    s.sc[vl.slot[1] as usize] = (n as i32) << 16;
}

/// What a frame has drawn so far, and a line's start.
struct Pen {
    prims: u32,
    steps: u32,
    from: (i32, i32, i32),
}

/// A coordinate (Q16.16 pixels) to the nearest pixel (x + ½ floored: ties
/// go up), held within `lo..=hi`.
#[inline]
fn pixel(v: i32, lo: i32, hi: i32) -> i32 {
    (fixed::add(v, ONE / 2) >> 16).clamp(lo, hi)
}

impl Pen {
    /// Whether the frame may draw another primitive (counting it).
    fn may(&mut self) -> bool {
        if self.prims >= PRIMITIVES || self.steps >= FRAME_STEPS {
            return false;
        }
        self.prims += 1;
        true
    }

    fn dot(&mut self, ink: &mut Ink, x: i32, y: i32, v: i32) {
        if !self.may() {
            return;
        }
        let (x, y) = (pixel(x, X_MIN, X_MAX), pixel(y, Y_MIN, Y_MAX));
        plot(ink, x, y, fixed::chan(v));
    }

    /// The line from `self.from` to `to`: integer Bresenham as
    /// `opendmd-core`'s `reference_line` (its two tests in its order), the ink from
    /// the start's byte to the end's in equal integer steps.
    fn line(&mut self, ink: &mut Ink, to: (i32, i32, i32)) {
        if !self.may() {
            return;
        }
        let from = self.from;
        let (mut x, mut y) = (pixel(from.0, X_MIN, X_MAX), pixel(from.1, Y_MIN, Y_MAX));
        let (x1, y1) = (pixel(to.0, X_MIN, X_MAX), pixel(to.1, Y_MIN, Y_MAX));
        // Wholly off one side of the panel: nothing of it would show (its
        // pixels lie between its ends), so it costs no steps.
        let (w, h) = (W as i32 - 1, H as i32 - 1);
        if (x < 0 && x1 < 0) || (x > w && x1 > w) || (y < 0 && y1 < 0) || (y > h && y1 > h) {
            return;
        }
        let (dx, dy) = ((x1 - x).abs(), -(y1 - y).abs());
        let (sx, sy) = (if x < x1 { 1 } else { -1 }, if y < y1 { 1 } else { -1 });
        let steps = dx.max(-dy).max(1);
        let mut err = dx + dy;
        let mut k = (fixed::chan(from.2) as i32) << 16;
        let dk = (((fixed::chan(to.2) as i32) << 16) - k) / steps;
        for _ in 0..LINE_STEPS {
            self.steps += 1;
            plot(ink, x, y, ((k + 0x8000) >> 16).clamp(0, 255) as u8);
            if x == x1 && y == y1 {
                break;
            }
            k += dk;
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
}

/// Lighten the pixel at `x`, `y` (up from the bottom) to `v`, if it is on
/// the panel.
#[inline(always)]
fn plot(ink: &mut Ink, x: i32, y: i32, v: u8) {
    if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
        let i = (H - 1 - y as usize) * W + x as usize;
        let p = &mut ink.0[i];
        *p = (*p).max(v);
    }
}
