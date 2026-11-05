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
use crate::program::glyphs;
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

/// One plane of the draw stage's canvas: 128 × 32 bytes, row-major from
/// the top as the frame is.
pub type Plane = [u8; W * H];

/// The draw stage's canvas: **ink** (`.0`: 0 where nothing was drawn, 255
/// at full ink, the brighter wins) and, for a program that paints, the
/// **paint** plane (`.1`: a colour of the program's paint palette, 0 none,
/// the last drawn wins), laid over every layer. A formula pays for the
/// planes it uses: 4 KB for one that only draws lines and dots, 8 KB for
/// one that paints ([`try_fit`](Self::try_fit)). All zero is a valid value
/// (`.1` `None`), so a zeroed allocation is one. Only a program that
/// [`draws`](Program::draws) needs one.
#[repr(C)]
pub struct Ink(pub Plane, pub Option<Box<Plane>>);

impl Ink {
    /// The ink plane alone.
    pub const fn new() -> Ink {
        Ink([0; W * H], None)
    }

    /// A canvas on the heap with both planes (the hosts': tests, tools, the
    /// simulator), each made zeroed in place (never through the stack).
    pub fn boxed() -> Box<Ink> {
        let mut k =
            Self::try_boxed().unwrap_or_else(|| alloc::alloc::handle_alloc_error(Self::LAYOUT));
        k.1 = Some(plane().unwrap_or_else(|| alloc::alloc::handle_alloc_error(PLANE)));
        k
    }

    /// The ink plane alone on the heap (4 KB), or `None` when the heap
    /// hasn't it.
    pub fn try_boxed() -> Option<Box<Ink>> {
        // SAFETY: a non-zero-sized layout; `Ink` is all-zero valid (bytes,
        // and `None`), so the zeroed memory is a value; the box frees it
        // with the same layout.
        unsafe {
            let p = alloc::alloc::alloc_zeroed(Self::LAYOUT).cast::<Ink>();
            (!p.is_null()).then(|| Box::from_raw(p))
        }
    }

    /// The planes `prog` uses, or `None` when the heap hasn't them.
    pub fn try_boxed_for(prog: &Prog) -> Option<Box<Ink>> {
        let mut k = Self::try_boxed()?;
        k.try_fit(prog).then_some(k)
    }

    /// Hold the planes `prog` uses: the paint plane taken (zeroed) when it
    /// paints, let go when it doesn't. `false` when the heap hasn't the
    /// paint plane (the canvas is then ink only).
    pub fn try_fit(&mut self, prog: &Prog) -> bool {
        if !prog.paints() {
            self.1 = None;
        } else if self.1.is_none() {
            self.1 = plane();
        }
        self.1.is_some() || !prog.paints()
    }

    const LAYOUT: core::alloc::Layout = core::alloc::Layout::new::<Ink>();

    /// The byte at column `x`, row `y` from the top.
    pub fn at(&self, x: usize, y: usize) -> u8 {
        self.0[(y * W + x) & (W * H - 1)]
    }
}

const PLANE: core::alloc::Layout = core::alloc::Layout::new::<Plane>();

/// A zeroed plane on the heap, or `None` when the heap hasn't 4 KB.
fn plane() -> Option<Box<Plane>> {
    // SAFETY: as `Ink::try_boxed`: bytes, zeroed, freed with its layout.
    unsafe {
        let p = alloc::alloc::alloc_zeroed(PLANE).cast::<Plane>();
        (!p.is_null()).then(|| Box::from_raw(p))
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
///
/// In IRAM (the dispatch: per step of the draw stage; Endless road's
/// draw stage ~8.5 % of its frame's instructions from flash,
/// display-isolation 2026-11-02), its rare steps out of line in flash:
/// a lane loop's start ([`as_lanes`]), the paint steps ([`args_paint`]).
#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi_draw")]
pub(crate) fn run<const N: usize>(
    prog: &Prog,
    s: &mut Scratch<N>,
    t: &Tables,
    ink: &mut Ink,
    lanes: bool,
) {
    ink.0.fill(0);
    if let Some(p) = ink.1.as_deref_mut() {
        p.fill(0);
    }
    let mut args = [0i32; 9];
    let mut na = 0usize;
    let start = prog.frame_end as usize;
    let end = (prog.draw_end as usize).min(MAX_OPS);
    let m = |v: u8| v as usize;
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
                    pc = (op.target() + 1).max(pc);
                } else if lanes {
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some((n, next)) = as_lanes(prog, s, t, ink, &mut pen, pc - 1, turns) {
                        turns += n;
                        pc = next;
                    }
                }
            }
            code::END => {
                turns += 1;
                let k = fixed::add(sc[m(op.a)], ONE);
                sc[m(op.a)] = k;
                let back = op.target();
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
            code::ARGS | code::PAINT => args_paint(prog, t, ink, &mut pen, &op, sc, &mut args, &mut na),
            // Forward only (the verifier holds it): no loop can come of it.
            code::IF => {
                if sc[m(op.a)] == 0 {
                    pc = op.target().max(pc);
                }
            }
            code::JUMP => pc = op.target().max(pc),
            _ => super::vm::scalar(sc, &op, t, &prog.tables),
        }
    }
}

/// A `FOR` at `at` with turns left, as lanes if [`Program::vector_loop`]
/// allows: its turns (as many as a turn at a time would run: each END
/// counts one, and the last is the one that reaches the count or the
/// frame's turns) and the step after its END. Once a loop a frame: out of
/// [`run`]'s IRAM.
#[cfg(not(target_arch = "wasm32"))]
#[inline(never)]
fn as_lanes<const N: usize>(
    prog: &Prog,
    s: &mut Scratch<N>,
    t: &Tables,
    ink: &mut Ink,
    pen: &mut Pen,
    at: usize,
    turns: u32,
) -> Option<(u32, usize)> {
    let vl = prog.vector_loop(at)?;
    let n = vl.count.min(TURNS - turns);
    run_lanes(prog, s, t, ink, pen, at, &vl, n);
    Some((n, vl.end + 1))
}

/// An `ARGS` or `PAINT` step: its values into `args`; a `PAINT` draws the
/// primitive (into the paint plane, if the heap had it) and starts the
/// next one's. Out of [`run`]'s IRAM: few formulas paint.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn args_paint(
    prog: &Prog,
    t: &Tables,
    ink: &mut Ink,
    pen: &mut Pen,
    op: &Op,
    sc: &Sc,
    args: &mut [i32; 9],
    na: &mut usize,
) {
    for v in [op.a, op.b, op.c] {
        if *na < args.len() {
            args[*na] = sc[v as usize];
            *na += 1;
        }
    }
    if op.code == code::PAINT {
        // (No paint plane: the heap hadn't it. Nothing painted.)
        if let Some(p) = ink.1.as_deref_mut() {
            paint_prim(prog, t, p, pen, op.d, args);
        }
        *na = 0;
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
#[cfg_attr(all(target_os = "espidf", feature = "sram1-iram"), link_section = ".iram1.s1")]
fn run_lanes<const N: usize>(
    prog: &Prog,
    s: &mut Scratch<N>,
    t: &Tables,
    ink: &mut Ink,
    pen: &mut Pen,
    at: usize,
    vl: &VecLoop,
    n: u32,
) {
    let body = at + 1..vl.end;
    // A register's operand: lane `L_I + r` (register `r` is
    // `s.lanes[r]`).
    let reg_op = |r: usize| lane(L_I + r as u8);
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
                    *f = reg_op(r);
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
            let reads_d = [o.a, o.b, o.c][..ar].contains(&reg_op(d));
            let w = if reads_d { spare.min(REGS) } else { d };
            o.d = reg_op(w);
            super::vm::lane_draw(s, &o, t, &prog.tables);
            if w != d {
                let (lo, hi) = s.lanes.split_at_mut(w);
                lo[d] = hi[0];
            }
        }
        // The lines and dots, turn by turn: each operand a register's
        // lane at the turn (`lane(r)`) or a value the same every turn (its
        // slot), looked up once a lane of turns.
        let (sc, lanes) = (&s.sc, &s.lanes);
        let src = |v: u8| vl.reg(v).map_or(v, |r| lane(r as u8));
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
            if np <= PRIMS {
                for &p in &ps[..np] {
                    lane_prim(pen, ink, p, sc, lanes, j);
                }
            } else {
                for op in prog.ops[body.clone()].iter().filter(|op| prim(op)) {
                    lane_prim(pen, ink, [op.code, src(op.a), src(op.b), src(op.c)], sc, lanes, j);
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

/// One line or dot of a lane loop's turn `j`: each operand a register's
/// lane at the turn (`lane(r)`) or a slot. In IRAM with [`Pen::line`]:
/// the two were ~12 % of Starfield's frame instructions from flash
/// (display-isolation 2026-11-02, esp32sim's block profile).
#[cfg(not(target_arch = "wasm32"))]
#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi_draw")]
fn lane_prim<const N: usize>(
    pen: &mut Pen,
    ink: &mut Ink,
    p: [u8; 4],
    sc: &Sc,
    lanes: &[[i32; N]; REGS + 1],
    j: usize,
) {
    let at = |o: u8| {
        if is_lane(o) {
            lanes[lane_no(o) as usize % (REGS + 1)][j % N]
        } else {
            sc[o as usize]
        }
    };
    let (a, b, c) = (at(p[1]), at(p[2]), at(p[3]));
    match p[0] {
        code::LINE_FROM => pen.from = (a, b, c),
        code::LINE_TO => pen.line(ink, (a, b, c)),
        _ => pen.dot(ink, a, b, c),
    }
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
        if !self.may() || fixed::chan(v) == 0 {
            return;
        }
        let (x, y) = (pixel(x, X_MIN, X_MAX), pixel(y, Y_MIN, Y_MAX));
        plot(ink, x, y, fixed::chan(v));
    }

    /// The line from `self.from` to `to`: integer Bresenham as
    /// `opendmd-core`'s `reference_line` (its two tests in its order), the ink from
    /// the start's byte to the end's in equal integer steps.
    #[inline(never)]
    #[cfg_attr(target_os = "espidf", link_section = ".iram1.psi_draw")]
    fn line(&mut self, ink: &mut Ink, to: (i32, i32, i32)) {
        if !self.may() {
            return;
        }
        let from = self.from;
        // Ink 0 at both ends draws nothing (0 under the canvas's `max`):
        // walked, it would only use the frame's steps. A line hidden this
        // way (`line(…, shown)`) costs what one off the panel does.
        if fixed::chan(from.2) == 0 && fixed::chan(to.2) == 0 {
            return;
        }
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

/// Lighten the pixel at `x`, `y` (down from the top) to `v`, if it is on
/// the panel.
#[inline(always)]
fn plot(ink: &mut Ink, x: i32, y: i32, v: u8) {
    if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
        let i = y as usize * W + x as usize;
        let p = &mut ink.0[i];
        *p = (*p).max(v);
    }
}

/// A paint colour value (a whole number, Q16.16) as its canvas byte: an
/// index of the paint palette, 0 none.
#[inline]
fn colour(v: i32) -> u8 {
    ((v >> 16) as u32 & (MAX_PAINT as u32 - 1)) as u8
}

/// One paint primitive `kind` (`program::paint`) with its values `a` (its
/// colour last), into the paint canvas `p`: the last drawn wins. Bounded
/// as the lines and dots are: a primitive counts one of the frame's, and
/// every pixel it covers one of its line steps.
fn paint_prim(
    prog: &Prog,
    t: &Tables,
    p: &mut [u8; W * H],
    pen: &mut Pen,
    kind: u8,
    a: &[i32; 9],
) {
    use crate::program::paint::*;
    if kind > LAST || !pen.may() {
        return;
    }
    let n = ARGS[kind as usize];
    let c = colour(a[n - 1]);
    let px = |v: i32| pixel(v, X_MIN, X_MAX);
    let py = |v: i32| pixel(v, Y_MIN, Y_MAX);
    match kind {
        CLS => {
            p.fill(c);
            pen.steps = pen.steps.saturating_add(256);
        }
        PSET => put(p, px(a[0]), py(a[1]), c),
        LINE => {
            let mut x0 = px(a[0]);
            let mut y0 = py(a[1]);
            let (x1, y1) = (px(a[2]), py(a[3]));
            let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
            let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
            let mut err = dx + dy;
            for _ in 0..LINE_STEPS {
                if pen.steps >= FRAME_STEPS {
                    return;
                }
                pen.steps += 1;
                put(p, x0, y0, c);
                if x0 == x1 && y0 == y1 {
                    break;
                }
                let e2 = 2 * err;
                if e2 >= dy {
                    err += dy;
                    x0 += sx;
                }
                if e2 <= dx {
                    err += dx;
                    y0 += sy;
                }
            }
        }
        RECT | RECTFILL => {
            let (x0, x1) = (px(a[0]).min(px(a[2])), px(a[0]).max(px(a[2])));
            let (y0, y1) = (py(a[1]).min(py(a[3])), py(a[1]).max(py(a[3])));
            for y in y0.max(0)..=y1.min(H as i32 - 1) {
                for x in x0.max(0)..=x1.min(W as i32 - 1) {
                    if kind == RECTFILL || y == y0 || y == y1 || x == x0 || x == x1 {
                        put(p, x, y, c);
                    }
                }
                pen.steps = pen.steps.saturating_add(1);
            }
        }
        CIRC | CIRCFILL => {
            // Pixels whose centre is within r + ½ of (x, y) (the ring: the
            // outermost of them on each row and column).
            let (cx, cy) = (a[0], a[1]);
            let r = a[2].max(0) as i64 + (ONE as i64) / 2;
            let r2 = r * r;
            let inside = |x: i32, y: i32| {
                let (dx, dy) = (
                    ((x as i64) << 16) - cx as i64,
                    ((y as i64) << 16) - cy as i64,
                );
                dx * dx + dy * dy <= r2
            };
            let rr = (r >> 16) as i32 + 1;
            let (x0, y0) = ((cx >> 16) - rr, (cy >> 16) - rr);
            for y in (y0).max(0)..=(y0 + 2 * rr + 1).min(H as i32 - 1) {
                for x in (x0).max(0)..=(x0 + 2 * rr + 1).min(W as i32 - 1) {
                    if inside(x, y)
                        && (kind == CIRCFILL
                            || !(inside(x - 1, y)
                                && inside(x + 1, y)
                                && inside(x, y - 1)
                                && inside(x, y + 1)))
                    {
                        put(p, x, y, c);
                    }
                }
                pen.steps = pen.steps.saturating_add(1);
            }
        }
        TRIFILL => {
            // Pixels whose centre is inside the triangle (edges included),
            // in whole-pixel steps of its bounding box on the panel.
            let v = [(a[0], a[1]), (a[2], a[3]), (a[4], a[5])];
            let edge = |(ax, ay): (i32, i32), (bx, by): (i32, i32), x: i64, y: i64| -> i64 {
                ((bx as i64 - ax as i64) * (y - ay as i64)
                    - (by as i64 - ay as i64) * (x - ax as i64))
                    >> 16
            };
            let area = edge(v[0], v[1], v[2].0 as i64, v[2].1 as i64);
            if area == 0 {
                return;
            }
            let (xs, ys) = (v.map(|q| q.0 >> 16), v.map(|q| q.1 >> 16));
            let (xa, xb) = (xs.iter().min().unwrap() - 1, xs.iter().max().unwrap() + 1);
            let (ya, yb) = (ys.iter().min().unwrap() - 1, ys.iter().max().unwrap() + 1);
            for y in ya.max(0)..=yb.min(H as i32 - 1) {
                for x in xa.max(0)..=xb.min(W as i32 - 1) {
                    let (fx, fy) = ((x as i64) << 16, (y as i64) << 16);
                    let e = [
                        edge(v[0], v[1], fx, fy),
                        edge(v[1], v[2], fx, fy),
                        edge(v[2], v[0], fx, fy),
                    ];
                    let inside = if area > 0 {
                        e.iter().all(|&k| k >= 0)
                    } else {
                        e.iter().all(|&k| k <= 0)
                    };
                    if inside {
                        put(p, x, y, c);
                    }
                }
                pen.steps = pen.steps.saturating_add(1);
            }
        }
        SPR => {
            // A sprite whose key gives colours (its keys' numbers paint
            // indices): a[0] its header's place in the tables (in fours:
            // `store_at`), a[3] flip (1 across, 2 down, 3 both).
            let at = (a[0] >> 16) as u8;
            let h = SprHead::new(&prog.tables, at);
            let (x0, y0) = (px(a[1]), py(a[2]));
            let flip = (a[3] >> 16) & 3;
            for v in 0..h.h as i32 {
                for u in 0..h.w as i32 {
                    let su = if flip & 1 != 0 { h.w as i32 - 1 - u } else { u };
                    let sv = if flip & 2 != 0 { h.h as i32 - 1 - v } else { v };
                    let k = colour(h.read(&prog.tables, su << 16, sv << 16));
                    if k != 0 {
                        put(p, x0 + u, y0 + v, k);
                    }
                }
                pen.steps = pen.steps.saturating_add(1);
            }
        }
        PRINT | PRINTNUM => {
            // Text from the tables (a[0] where, a[1] how many characters)
            // or a whole number (a[0], at least a[1] digits); a[2] the font
            // (0 normal 5 × 7, 1 small 4 × 6, 2 bold digits).
            let font = (a[2] >> 16).clamp(0, 2) as u8;
            let gc = match font {
                1 => code::GLYPH_S,
                2 => code::DIGIT_B,
                _ => code::GLYPH,
            };
            let (cw, ch) = if font == 0 { (5, 7) } else { (4, 6) };
            let g = glyphs(t, gc);
            let mut chars = [0i32; 32];
            let n = if kind == PRINT {
                let (at, len) = (
                    (a[0] >> 16).max(0) as usize,
                    (a[1] >> 16).clamp(0, 32) as usize,
                );
                for (k, c) in chars.iter_mut().take(len).enumerate() {
                    *c = prog.tables[(at + k) & (prog.tables.len() - 1)];
                }
                len
            } else {
                let neg = a[0] < 0;
                let mut v = ((a[0] >> 16) as i64).unsigned_abs();
                let mut digits = [0i32; 12];
                let mut nd = 0;
                loop {
                    digits[nd] = (v % 10) as i32;
                    v /= 10;
                    nd += 1;
                    if v == 0 && nd as i32 >= (a[1] >> 16).clamp(1, 10) {
                        break;
                    }
                }
                let mut n = 0;
                if neg {
                    chars[0] = ('-' as i32) << 16;
                    n = 1;
                }
                for k in (0..nd).rev() {
                    chars[n] = if font == 2 {
                        digits[k] << 16
                    } else {
                        (('0' as i32) + digits[k]) << 16
                    };
                    n += 1;
                }
                n
            };
            let (x0, y0) = (px(a[3]), py(a[4]));
            for (k, &chv) in chars.iter().take(n).enumerate() {
                for row in 0..ch {
                    for col in 0..cw {
                        if fixed::glyph(&g, chv, col << 16, row << 16) != 0 {
                            put(p, x0 + k as i32 * cw + col, y0 + row, c);
                        }
                    }
                }
                pen.steps = pen.steps.saturating_add(1);
            }
        }
        _ => {}
    }
}

/// Paint index `c` at `x`, `y` if it is on the panel.
#[inline(always)]
fn put(p: &mut [u8; W * H], x: i32, y: i32, c: u8) {
    if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
        p[y as usize * W + x as usize] = c;
    }
}

/// The paint plane of a frame's canvas when `prog` paints and the canvas
/// has one: what [`paint_row`] lays over each row (`None`: nothing, and no
/// call a row).
#[inline]
pub(crate) fn paint_of<'a>(prog: &Prog, ink: Option<&'a Ink>) -> Option<&'a Plane> {
    ink.filter(|_| prog.paints()).and_then(|k| k.1.as_deref())
}

/// The paint plane's colours over row `row` of the picture (`out`, its
/// W × 3 bytes). Out of line, in flash; called only for a program that
/// paints ([`paint_of`]).
#[inline(never)]
pub(crate) fn paint_row(prog: &Prog, paint: &Plane, row: usize, out: &mut [u8]) {
    let px = out.as_chunks_mut::<3>().0;
    overlay(prog, &paint[row * W..(row + 1) * W], px);
}

/// The paint canvas's colours over a lane of the picture (`row` from
/// `paint`, `px` its pixels): wherever something was painted. (Out of the
/// frame loop: in flash.)
#[inline(never)]
pub(crate) fn overlay(prog: &Prog, paint: &[u8], px: &mut [[u8; 3]]) {
    for (o, &k) in px.iter_mut().zip(paint) {
        if k != 0 {
            let v = prog.paint[k as usize & (MAX_PAINT - 1)];
            *o = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
        }
    }
}
