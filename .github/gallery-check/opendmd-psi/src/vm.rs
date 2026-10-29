//! The row-vectorised VM.
//!
//! A frame runs the per-frame steps once (scalars), then for each of the 32
//! rows the per-row steps (scalars: anything that depends on `y` but not
//! `x`), then the per-pixel steps over a *lane* of `N` pixels at once (`N`
//! = 128, one row, or 64, half of one): each step is one tight loop over
//! the lane, so the interpreter's dispatch (a `match` per step) is paid
//! once per `N` pixels, and an operand that is the same across the lane (a
//! constant, a per-frame or per-row value) is a register in that loop, not
//! a broadcast array. The colour step writes the lane's RGB888 bytes
//! straight into the frame.
//!
//! Every loop is monomorphised over which operands are lanes and which are
//! scalars ([`Src`]); the arithmetic is `fixed`'s, the same functions the
//! constant folder and the scalar steps use, so the output is bit-identical
//! wherever it runs. Nothing here allocates; the working memory is the
//! caller's [`Scratch`].

use crate::draw::Ink;
use crate::fixed::{self, ONE};
use crate::program::*;
use crate::still::Still;
use crate::tables::Tables;

/// What a frame depends on besides the program.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    /// Seconds into the loop, Q16.16 (see [`Inputs::at_ms`]).
    pub t: i32,
    /// The wall clock: 0..23, 0..59, 0..59 (whole numbers).
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// The sliders `a b c`, Q16.16 (0..1 by convention).
    pub a: i32,
    pub b: i32,
    pub c: i32,
    /// A `loop forever` formula's `t` ([`Program::forever`]): the time
    /// since it started in 1/65,536 s (Q48.16; see [`Inputs::wide_ms`]),
    /// growing without end. Other formulas read `t` instead.
    pub tw: i64,
    /// A `loop forever` formula's `seed`, Q16.16 in 0..1: the same for a
    /// whole play, a new one each time it plays.
    pub seed: i32,
}

impl Inputs {
    /// `t` for `ms` into the loop: exactly `ms · 65536 / 1000`, rounded
    /// down.
    pub fn at_ms(ms: u32) -> i32 {
        ((ms as u64 * 65536) / 1000).min(i32::MAX as u64) as i32
    }

    /// [`tw`](Self::tw) for `ms` since the formula started: `ms · 65536 /
    /// 1000` rounded down, as [`at_ms`](Self::at_ms) (the same value
    /// wherever both fit).
    pub fn wide_ms(ms: u64) -> i64 {
        // (No u128: its division is ~1 KB of the web module.) Exact to
        // 2^47 ms, 4,000 years; held there.
        let ms = ms.min(1 << 47);
        (ms / 1000 * 65536 + ms % 1000 * 65536 / 1000) as i64
    }

    /// Both clocks for `ms` since `prog` started: `t` at `ms` into its loop
    /// and, for a `loop forever` formula, `tw` at `ms`.
    pub fn set_ms(&mut self, prog: &Program, ms: u64) {
        let lp = u64::from(prog.loop_ms().max(1));
        self.t = Self::at_ms((ms % lp) as u32);
        self.tw = Self::wide_ms(ms);
    }
}

/// A Q16.16 from a decimal fraction `num / den` (sliders, tests).
pub fn q16(num: i32, den: i32) -> i32 {
    fixed::div(num << 16, den << 16)
}

/// The VM's working memory for a lane width `N` (64 or 128): the lane
/// registers, the `i` lane, the scalar slots, and the `x`/`px` columns
/// (worked out once, by [`Scratch::new`] or [`Scratch::reset`]): 8,192 B
/// at 128 lanes, 4,864 B at 64. Plain integers, so a zeroed allocation plus `reset` is a
/// valid one.
pub struct Scratch<const N: usize> {
    /// `[0]` is the `i` lane, then the [`REGS`] registers.
    pub(crate) lanes: [[i32; N]; REGS + 1],
    pub(crate) sc: [i32; MAX_SCALARS],
    pub(crate) xs: [i32; W],
    pub(crate) pxs: [i32; W],
}

impl<const N: usize> Scratch<N> {
    // (A lane holds the wide values while the frame's steps run: `WIDE`
    // i64s.)
    const _OK: () = assert!(N >= 2 * WIDE && W.is_multiple_of(N));

    pub fn new() -> Self {
        let mut s = Scratch {
            lanes: [[0; N]; REGS + 1],
            sc: [0; MAX_SCALARS],
            xs: [0; W],
            pxs: [0; W],
        };
        s.reset();
        s
    }

    /// Fill in the constant columns (for a zeroed allocation).
    pub fn reset(&mut self) {
        #[allow(clippy::let_unit_value)]
        let () = Self::_OK;
        for k in 0..W {
            self.pxs[k] = (k as i32) << 16;
            self.xs[k] = fixed::div(self.pxs[k], ((W - 1) as i32) << 16);
        }
    }
}

impl<const N: usize> Scratch<N> {
    /// (The firmware's probe.) Lane register `k` (0 = `L0`) as the VM left it.
    #[doc(hidden)]
    pub fn lane_reg(&self, k: usize) -> &[i32; N] {
        &self.lanes[k + 1]
    }

    /// (The firmware's probe.) Scalar slot `k` as the VM left it.
    #[doc(hidden)]
    pub fn scalar_slot(&self, k: usize) -> i32 {
        self.sc[k]
    }
}

impl<const N: usize> Default for Scratch<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// An operand inside a lane loop.
trait Src: Copy {
    /// The value `k` ahead of the cursor.
    fn at(self, k: usize) -> i32;
    /// Move the cursor `n` ahead (a lane's pointer; a scalar stays).
    fn step(&mut self, n: usize);
}

/// A lane: `N` values.
#[derive(Clone, Copy)]
struct L(*const i32);

impl Src for L {
    #[inline(always)]
    fn at(self, k: usize) -> i32 {
        // SAFETY: `L` is only made from a lane of `N` values that outlives
        // the step, and `k < N`.
        unsafe { *self.0.add(k) }
    }

    #[inline(always)]
    fn step(&mut self, n: usize) {
        // SAFETY: stays within (or one past) the lane.
        self.0 = unsafe { self.0.add(n) };
    }
}

/// A scalar, the same in every pixel of the lane.
#[derive(Clone, Copy)]
struct S(i32);

impl Src for S {
    #[inline(always)]
    fn at(self, _: usize) -> i32 {
        self.0
    }

    #[inline(always)]
    fn step(&mut self, _: usize) {}
}

#[derive(Clone, Copy)]
enum Arg {
    Lane(*const i32),
    Sc(i32),
}

// On the ESP32 the hottest loops (`fast1`, `fast2`, the colour write
// `out1` and `pal(P, v) * k`'s) run from IRAM, ~8 KB: not for speed (speed-a
// measured IRAM against flash at 0-2.5 %) but so their timing does not
// swing with the flash cache's layout from build to build (speed-b to
// speed-c: Tunnel as an effect 4,043 -> 4,319 µs, Rotozoom's checked steps
// 5,242 -> 6,983, the same code).
//
// The loops. `run*` are one element a turn, for the steps whose work
// dwarfs the loop's (`len`, `noise`, ...); `fast*` four a turn, for the
// cheap ones, where the ESP32 would otherwise spend as long on the loop
// (counter, branch, pointers: it has no post-increment loads) as on the
// add. Each is its own function, not inlined into the dispatch: with the
// whole VM in one function the ESP32's compiler ran out of registers and
// reloaded constants from flash and spilled to the stack every element.

#[inline(never)]
fn run1<const N: usize, A: Src>(d: &mut [i32; N], mut a: A, f: impl Fn(i32) -> i32) {
    for o in d.iter_mut() {
        *o = f(a.at(0));
        a.step(1);
    }
}

#[inline(never)]
fn run2<const N: usize, A: Src, B: Src>(
    d: &mut [i32; N],
    mut a: A,
    mut b: B,
    f: impl Fn(i32, i32) -> i32,
) {
    for o in d.iter_mut() {
        *o = f(a.at(0), b.at(0));
        a.step(1);
        b.step(1);
    }
}

#[inline(never)]
fn run3<const N: usize, A: Src, B: Src, C: Src>(
    d: &mut [i32; N],
    mut a: A,
    mut b: B,
    mut c: C,
    f: impl Fn(i32, i32, i32) -> i32,
) {
    for o in d.iter_mut() {
        *o = f(a.at(0), b.at(0), c.at(0));
        a.step(1);
        b.step(1);
        c.step(1);
    }
}

/// `noise(x, y, z)` at a whole `z` along the lane ([`fixed::noise_flat`]),
/// `x` and `y` read with a stride (0: a value for the lane): one loop for
/// every kind of operand.
#[inline(never)]
fn noise_flat_lane<const N: usize>(d: &mut [i32; N], args: [Arg; 2], z: i32) {
    let mut one = [0i32; 2];
    let mut ptr = [core::ptr::null::<i32>(); 2];
    let mut stride = [0usize; 2];
    for k in 0..2 {
        match args[k] {
            Arg::Lane(q) => (ptr[k], stride[k]) = (q, 1),
            Arg::Sc(v) => one[k] = v,
        }
    }
    for k in 0..2 {
        if stride[k] == 0 {
            ptr[k] = &one[k];
        }
    }
    for o in d.iter_mut() {
        // SAFETY: as `pal_lit_lane`'s.
        let (x, y) = unsafe { (*ptr[0], *ptr[1]) };
        *o = fixed::noise_flat(x, y, z);
        for j in 0..2 {
            ptr[j] = ptr[j].wrapping_add(stride[j]);
        }
    }
}

#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
#[inline(never)]
fn fast1<const N: usize, A: Src>(d: &mut [i32; N], mut a: A, f: impl Fn(i32) -> i32) {
    for o in d.as_chunks_mut::<4>().0 {
        // Every load before any store: `d` may be an operand's lane as far
        // as the compiler knows, so loads written after a store stay after
        // it, each stalling on its own (the ESP32's load-use delay).
        let x = [a.at(0), a.at(1), a.at(2), a.at(3)];
        *o = [f(x[0]), f(x[1]), f(x[2]), f(x[3])];
        a.step(4);
    }
}

#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
#[inline(never)]
fn fast2<const N: usize, A: Src, B: Src>(
    d: &mut [i32; N],
    mut a: A,
    mut b: B,
    f: impl Fn(i32, i32) -> i32,
) {
    for o in d.as_chunks_mut::<4>().0 {
        let x = [a.at(0), a.at(1), a.at(2), a.at(3)];
        let y = [b.at(0), b.at(1), b.at(2), b.at(3)];
        *o = [f(x[0], y[0]), f(x[1], y[1]), f(x[2], y[2]), f(x[3], y[3])];
        a.step(4);
        b.step(4);
    }
}

#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
fn fast3<const N: usize, A: Src, B: Src, C: Src>(
    d: &mut [i32; N],
    mut a: A,
    mut b: B,
    mut c: C,
    f: impl Fn(i32, i32, i32) -> i32,
) {
    for o in d.as_chunks_mut::<4>().0 {
        let x = [a.at(0), a.at(1), a.at(2), a.at(3)];
        let y = [b.at(0), b.at(1), b.at(2), b.at(3)];
        let z = [c.at(0), c.at(1), c.at(2), c.at(3)];
        *o = [
            f(x[0], y[0], z[0]),
            f(x[1], y[1], z[1]),
            f(x[2], y[2], z[2]),
            f(x[3], y[3], z[3]),
        ];
        a.step(4);
        b.step(4);
        c.step(4);
    }
}

#[inline(never)]
fn out3<A: Src, B: Src, C: Src>(
    px: &mut [[u8; 3]],
    mut a: A,
    mut b: B,
    mut c: C,
    f: impl Fn(i32, i32, i32) -> [u8; 3],
) {
    for o in px.iter_mut() {
        *o = f(a.at(0), b.at(0), c.at(0));
        a.step(1);
        b.step(1);
        c.step(1);
    }
}

/// `pal(P, k) * s` along the lane, its two operands read with a stride (0:
/// a value for the lane): one loop for the four kinds of operand.
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
#[inline(never)]
fn pal_lit_lane(px: &mut [[u8; 3]], args: [Arg; 2], p: &[[u8; 3]; 32], shift: u8) {
    // Both lanes (the usual: Tunnel's, Rotozoom floor's): plain
    // pointers (not in wasm: the same bytes, a smaller download).
    #[cfg(not(target_arch = "wasm32"))]
    if let [Arg::Lane(k), Arg::Lane(s)] = args {
        return pal_lit_lanes(px, k, s, p, shift);
    }
    let mut one = [0i32; 2];
    let mut ptr = [core::ptr::null::<i32>(); 2];
    let mut stride = [0usize; 2];
    for k in 0..2 {
        match args[k] {
            Arg::Lane(q) => (ptr[k], stride[k]) = (q, 1),
            Arg::Sc(v) => one[k] = v,
        }
    }
    for k in 0..2 {
        if stride[k] == 0 {
            ptr[k] = &one[k];
        }
    }
    // The last entry and scale and their colour: neighbours mostly share
    // both (Tunnel ~80 %, Rotozoom floor ~90 % of pixels), and the scaling
    // is three multiplies a channel.
    let (mut last_i, mut last_s) = (usize::MAX, 0);
    let mut rgb = [0u8; 3];
    for o in px.iter_mut() {
        // SAFETY: each pointer is a lane of at least `px.len()` values read
        // once per element (stride 1), or one value (stride 0), alive for
        // the call.
        let (k, s) = unsafe { (*ptr[0], *ptr[1]) };
        // (`>> shift` is exactly `fixed::mul(s, 2^-shift)`.)
        let (i, s) = (fixed::pal_index(k), s >> shift);
        if i != last_i || s != last_s {
            (last_i, last_s) = (i, s);
            rgb = fixed::scale_rgb(p[i], s);
        }
        *o = rgb;
        for j in 0..2 {
            ptr[j] = ptr[j].wrapping_add(stride[j]);
        }
    }
}

/// [`pal_lit_lane`] of two lanes.
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
#[inline(never)]
fn pal_lit_lanes(px: &mut [[u8; 3]], k: *const i32, s: *const i32, p: &[[u8; 3]; 32], shift: u8) {
    let (mut last_i, mut last_s) = (usize::MAX, 0);
    let mut rgb = [0u8; 3];
    for (j, o) in px.iter_mut().enumerate() {
        // SAFETY: `k` and `s` are lanes of at least `px.len()` values,
        // alive for the call.
        let (k, s) = unsafe { (*k.add(j), *s.add(j)) };
        let (i, s) = (fixed::pal_index(k), s >> shift);
        if i != last_i || s != last_s {
            (last_i, last_s) = (i, s);
            rgb = fixed::scale_rgb(p[i], s);
        }
        *o = rgb;
    }
}

#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
#[inline(never)]
fn out1<A: Src>(px: &mut [[u8; 3]], mut a: A, f: impl Fn(i32) -> [u8; 3]) {
    for o in px.iter_mut() {
        *o = f(a.at(0));
        a.step(1);
    }
}

macro_rules! un {
    ($run:ident, $d:expr, $a:expr, $f:expr) => {
        match $a {
            Arg::Lane(p) => $run($d, L(p), $f),
            Arg::Sc(v) => $d.fill(($f)(v)),
        }
    };
}

macro_rules! bin {
    ($run:ident, $d:expr, $a:expr, $b:expr, $f:expr) => {
        match ($a, $b) {
            (Arg::Lane(p), Arg::Lane(q)) => $run($d, L(p), L(q), $f),
            (Arg::Lane(p), Arg::Sc(v)) => $run($d, L(p), S(v), $f),
            (Arg::Sc(u), Arg::Lane(q)) => $run($d, S(u), L(q), $f),
            (Arg::Sc(u), Arg::Sc(v)) => $d.fill(($f)(u, v)),
        }
    };
}

macro_rules! tern {
    ($run:ident, $d:expr, $a:expr, $b:expr, $c:expr, $f:expr) => {
        match ($a, $b, $c) {
            (Arg::Lane(p), Arg::Lane(q), Arg::Lane(r)) => $run($d, L(p), L(q), L(r), $f),
            (Arg::Lane(p), Arg::Lane(q), Arg::Sc(w)) => $run($d, L(p), L(q), S(w), $f),
            (Arg::Lane(p), Arg::Sc(v), Arg::Lane(r)) => $run($d, L(p), S(v), L(r), $f),
            (Arg::Lane(p), Arg::Sc(v), Arg::Sc(w)) => $run($d, L(p), S(v), S(w), $f),
            (Arg::Sc(u), Arg::Lane(q), Arg::Lane(r)) => $run($d, S(u), L(q), L(r), $f),
            (Arg::Sc(u), Arg::Lane(q), Arg::Sc(w)) => $run($d, S(u), L(q), S(w), $f),
            (Arg::Sc(u), Arg::Sc(v), Arg::Lane(r)) => $run($d, S(u), S(v), L(r), $f),
            (Arg::Sc(u), Arg::Sc(v), Arg::Sc(w)) => $d.fill(($f)(u, v, w)),
        }
    };
}

macro_rules! outt {
    ($px:expr, $a:expr, $b:expr, $c:expr, $f:expr) => {
        match ($a, $b, $c) {
            (Arg::Lane(p), Arg::Lane(q), Arg::Lane(r)) => out3($px, L(p), L(q), L(r), $f),
            (Arg::Lane(p), Arg::Lane(q), Arg::Sc(w)) => out3($px, L(p), L(q), S(w), $f),
            (Arg::Lane(p), Arg::Sc(v), Arg::Lane(r)) => out3($px, L(p), S(v), L(r), $f),
            (Arg::Lane(p), Arg::Sc(v), Arg::Sc(w)) => out3($px, L(p), S(v), S(w), $f),
            (Arg::Sc(u), Arg::Lane(q), Arg::Lane(r)) => out3($px, S(u), L(q), L(r), $f),
            (Arg::Sc(u), Arg::Lane(q), Arg::Sc(w)) => out3($px, S(u), L(q), S(w), $f),
            (Arg::Sc(u), Arg::Sc(v), Arg::Lane(r)) => out3($px, S(u), S(v), L(r), $f),
            (Arg::Sc(u), Arg::Sc(v), Arg::Sc(w)) => $px.fill(($f)(u, v, w)),
        }
    };
}

/// Render one frame of `prog` into `out` (128 × 32 RGB888, row-major from
/// the top), with lanes of `N` pixels, with nothing under it (`ur ug ub ul`
/// read 0; a layer that lets something show through shows black). Writes
/// every byte.
pub fn render_with<const N: usize>(
    prog: &Program,
    scratch: &mut Scratch<N>,
    inputs: &Inputs,
    tables: &Tables,
    out: &mut [u8],
) {
    render_rows::<N>(prog, scratch, inputs, tables, None, u32::MAX, out)
}

/// Render the rows of `prog`'s frame that `rows` has a bit for (bit `k`:
/// row `k` from the top) into `out` (128 × 32 RGB888, row-major from the
/// top), over `under` (the picture under the formula, the same layout: what
/// `ur ug ub ul` read and what shows through its layers; `None`: black).
/// The rest of `out` is left as it was. A row's bytes are the same whichever
/// other rows are drawn with it. A program that draws
/// ([`Program::draws`]) draws nothing here (`ink` reads 0): give it a
/// canvas with [`render_rows_ink`].
pub fn render_rows<const N: usize>(
    prog: &Program,
    scratch: &mut Scratch<N>,
    inputs: &Inputs,
    tables: &Tables,
    under: Option<&[u8]>,
    rows: u32,
    out: &mut [u8],
) {
    render_rows_ink::<N>(prog, scratch, None, inputs, tables, under, rows, out)
}

/// [`render_rows`] with the draw stage's canvas: a program that draws
/// ([`Program::draws`]) draws its frame's lines into `ink` first (cleared)
/// on a **full call** (`rows == u32::MAX`), and its `ink` reads them back.
/// A call for some rows only does not draw: it reads the canvas the last
/// full call left — the caller's job to make that the same program and
/// inputs (a look redraws only the rows its picture changed, with the same
/// `t`; anything new for it is drawn with every row first). `None`, or a
/// program that does not draw: `ink` reads 0.
#[allow(clippy::too_many_arguments)]
pub fn render_rows_ink<const N: usize>(
    prog: &Program,
    scratch: &mut Scratch<N>,
    ink: Option<&mut Ink>,
    inputs: &Inputs,
    tables: &Tables,
    under: Option<&[u8]>,
    rows: u32,
    out: &mut [u8],
) {
    render_rows_still::<N>(prog, scratch, ink, None, inputs, tables, under, rows, out)
}

/// What the still cache does this frame.
enum Cache<'a> {
    Off,
    /// Skip the cached run (at the marker `at`), copy the kept lanes in.
    Use(&'a Still),
    /// Run everything and keep the cached values after the run.
    Keep(&'a mut Still, usize),
}

/// [`render_rows_ink`] with the still cache ([`Still`]): a program with
/// cached values ([`Program::still_lanes`]) given a `still` that holds
/// lanes for them keeps them on a full call (`rows == u32::MAX`) and, once
/// kept, skips the steps that work them out on every call after. The bytes
/// are the same with or without it, kept or not; `None`, too few lanes, or
/// a program without cached values: every step runs.
#[allow(clippy::too_many_arguments)]
pub fn render_rows_still<const N: usize>(
    prog: &Program,
    scratch: &mut Scratch<N>,
    ink: Option<&mut Ink>,
    still: Option<&mut Still>,
    inputs: &Inputs,
    tables: &Tables,
    under: Option<&[u8]>,
    rows: u32,
    out: &mut [u8],
) {
    frame::<N>(
        prog,
        scratch,
        ink,
        still,
        inputs,
        tables,
        under,
        rows,
        out,
        true,
        // (Opaque: a constant here had LLVM make a second, specialised
        // copy of the frame loop, 9 KB of IRAM.)
        core::hint::black_box(None),
    )
}

/// What changed: [`render_rows_still`], and `changed[row]` (row from the
/// top) has bit `g` set when any byte of pixels `4g..4g + 4` of that row
/// differs from what `out` held before the call (all bits when `out` is
/// not word-aligned; a row not drawn is left as it was: give zeros).
/// The bytes written are the same as without it; the player marks only
/// what changed, so the blit pushes nothing else (console-lessons 2.5).
#[allow(clippy::too_many_arguments)]
pub fn render_rows_changed<const N: usize>(
    prog: &Program,
    scratch: &mut Scratch<N>,
    ink: Option<&mut Ink>,
    still: Option<&mut Still>,
    inputs: &Inputs,
    tables: &Tables,
    under: Option<&[u8]>,
    rows: u32,
    out: &mut [u8],
    changed: &mut [u32; H],
) {
    frame::<N>(
        prog,
        scratch,
        ink,
        still,
        inputs,
        tables,
        under,
        rows,
        out,
        true,
        core::hint::black_box(Some(changed)),
    )
}

/// [`render_rows_ink`] with every draw loop run a turn at a time (as before
/// draw loops ran as lanes, `draw::run`): the same bytes, for the tests
/// that compare the two.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn render_rows_ink_turns<const N: usize>(
    prog: &Program,
    scratch: &mut Scratch<N>,
    ink: Option<&mut Ink>,
    inputs: &Inputs,
    tables: &Tables,
    under: Option<&[u8]>,
    rows: u32,
    out: &mut [u8],
) {
    frame::<N>(
        prog,
        scratch,
        ink,
        None,
        inputs,
        tables,
        under,
        rows,
        out,
        core::hint::black_box(false),
        core::hint::black_box(None),
    )
}

/// [`render_rows_still`]'s body: one copy whoever calls it, and on the
/// ESP32 in IRAM with the cheap loops, so the frame loop and its dispatch
/// are never evicted from the flash cache between steps (speed-a..f: the
/// same rows 3.65 or 4.3-4.9 ms by which copy of this loop ran and where
/// the build put it).
#[allow(clippy::too_many_arguments)]
#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
fn frame<const N: usize>(
    prog: &Program,
    scratch: &mut Scratch<N>,
    ink: Option<&mut Ink>,
    still: Option<&mut Still>,
    inputs: &Inputs,
    tables: &Tables,
    under: Option<&[u8]>,
    rows: u32,
    out: &mut [u8],
    lanes: bool,
    changed: Option<&mut [u32; H]>,
) {
    #[cfg(feature = "stats")]
    stats::stamp(1);
    assert_eq!(out.len(), W * H * 3);
    if let Some(u) = under {
        assert_eq!(u.len(), W * H * 3);
    }
    let ns = prog.n_scalars as usize;
    scratch.sc[..ns].copy_from_slice(&prog.init[..ns]);
    let sc = &mut scratch.sc;
    sc[S_T as usize] = inputs.t;
    sc[S_HOUR as usize] = (inputs.hour as i32) << 16;
    sc[S_MINUTE as usize] = (inputs.minute as i32) << 16;
    sc[S_SECOND as usize] = (inputs.second as i32) << 16;
    sc[S_A as usize] = inputs.a;
    sc[S_B as usize] = inputs.b;
    sc[S_C as usize] = inputs.c;
    if prog.uses & SEEDED != 0 {
        sc[S_SEED as usize] = inputs.seed;
    }
    let (fe, re, pe) = (
        prog.frame_end as usize,
        prog.row_end as usize,
        prog.pixel_end as usize,
    );
    // A `loop forever` formula's wide values (0: `t`) live in the last
    // lane while the frame's steps run (no lane is in use before the rows).
    if prog.forever() {
        wset(&mut scratch.lanes[REGS], 0, inputs.tw);
    }
    for op in &prog.ops[..fe] {
        if code::is_wide(op.code) {
            wide_step(&mut scratch.sc, &mut scratch.lanes[REGS], op);
        } else {
            scalar(&mut scratch.sc, op, tables, &prog.tables);
        }
    }
    let ink: Option<&Ink> = match ink {
        Some(k) if prog.draws() => {
            if rows == u32::MAX {
                crate::draw::run(prog, scratch, tables, k, lanes);
            }
            Some(k)
        }
        _ => None,
    };
    let mut cache = match (still, prog.still_marker()) {
        (Some(st), Some(at)) if st.lanes_for(prog) => {
            if st.holds(prog) {
                Cache::Use(st)
            } else if rows == u32::MAX {
                st.kept = 0;
                Cache::Keep(st, at)
            } else {
                Cache::Off
            }
        }
        _ => Cache::Off,
    };
    #[cfg(feature = "stats")]
    stats::note(prog, scratch, &cache, out, inputs);
    // The narrowed steps as they are only for inputs in the ranges they
    // were narrowed for (else their checked forms: the same bytes). In wasm
    // (the web app's previews) always their checked forms: the same bytes
    // without their loops in the download.
    let narrow = !cfg!(target_arch = "wasm32") && crate::range::inputs_in_range(prog, inputs);
    let uses_i = prog.uses & USES_I != 0;
    // A formula's own palettes, unpacked once for the frame.
    let mut own: Pals = [[[0; 3]; 32]; USER_PALETTES];
    if prog.uses & OWNPAL != 0 {
        unpack_palettes(&prog.tables, &mut own);
    }
    let backdrop = prog.uses & BACKDROP != 0;
    // Each row's values and what it changed (`Rows`): the row as it was
    // kept when `changed` asks.
    let mut per_row = Rows::new(changed, out);
    // The column run: on the first row drawn, then its values are kept in
    // their registers (one lane is the whole row only at N = W).
    let mut columns_kept = false;
    #[cfg(feature = "stats")]
    stats::stamp(2);
    for row in 0..H {
        if rows & (1 << row) == 0 {
            continue;
        }
        per_row.begin(prog, scratch, tables, out, row);
        let span = row * W * 3..(row + 1) * W * 3;
        let row_bytes = &mut out[span.clone()];
        let under_row = under.map(|u| &u[span.clone()]);
        for base in (0..W).step_by(N) {
            if uses_i {
                for (k, v) in scratch.lanes[0].iter_mut().enumerate() {
                    *v = ((row * W + base + k) as i32) << 16;
                }
            }
            let lane = base * 3..(base + N) * 3;
            let ul = under_row.map(|u| &u[lane.clone()]);
            let il = ink.map(|k| &k.0[row * W + base..row * W + base + N]);
            let px = row_bytes[lane].as_chunks_mut::<3>().0;
            if backdrop {
                match ul {
                    Some(u) => px.as_flattened_mut().copy_from_slice(u),
                    None => px.fill([0; 3]),
                }
            }
            // (A still cache's lanes are pointed at again by its marker
            // below, at this lane's pixels.)
            let mut alias: Alias = [core::ptr::null(); REGS + 1];
            let mut k = re;
            let mut keep_at = usize::MAX;
            while k < pe {
                let op = &prog.ops[k];
                if k == keep_at {
                    if let Cache::Keep(st, at) = &mut cache {
                        let m = &prog.ops[*at];
                        keep(scratch, st, [m.b, m.c], m.d, row * W + base);
                    }
                }
                if op.code == code::COLUMN {
                    k += 1 + if columns_kept { op.a as usize } else { 0 };
                    continue;
                }
                if op.code == code::STILL {
                    match &cache {
                        Cache::Use(st) => {
                            fetch::<N>(&mut alias, st, [op.b, op.c], op.d, row * W + base);
                            k += 1 + op.a as usize;
                        }
                        Cache::Keep(..) => {
                            keep_at = k + 1 + op.a as usize;
                            k += 1;
                        }
                        Cache::Off => k += 1,
                    }
                    continue;
                }
                if op.code == code::FUSE {
                    // A fused run (`crate::fuse`): its kernel on a frame
                    // that runs the narrowed steps, else its steps.
                    let n = op.a as usize;
                    #[cfg(not(target_arch = "wasm32"))]
                    // (Read where it is used: a flag the loop could test
                    // once had LLVM copy the whole lane loop for each way
                    // it goes, ~0.5-0.7 KB of IRAM, as soon as the loop
                    // was small enough.)
                    if core::hint::black_box(narrow)
                        && fused(
                            scratch,
                            &mut alias,
                            op.d,
                            &prog.ops[(k + 1).min(pe)..(k + 1 + n).min(pe)],
                            base,
                            tables,
                            &own,
                            px,
                            il,
                        )
                    {
                        k += 1 + n;
                        continue;
                    }
                    let _ = n;
                    k += 1;
                    continue;
                }
                if code::is_colour(op.code) {
                    colour_step(scratch, &alias, op, base, tables, &own, px);
                } else {
                    lane_op(
                        scratch,
                        &mut alias,
                        op,
                        core::hint::black_box(narrow),
                        base,
                        tables,
                        &prog.tables,
                        ul,
                        il,
                    );
                }
                k += 1;
            }
        }
        columns_kept = N == W;
    }
    per_row.finish(out);
    #[cfg(feature = "stats")]
    stats::stamp(3);
    if let Cache::Keep(st, _) = cache {
        st.kept = crate::still::fingerprint(prog);
    }
}

/// Where each register's lane is read from instead of its own, null for
/// its own: a still cache's lane at the pixels (`fetch`), until the
/// register is written. One lane pass long.
pub(crate) type Alias = [*const i32; REGS + 1];

/// The lane register a register operand names (as `lane_op`'s
/// destination).
#[inline(always)]
pub(crate) fn reg_index(v: u8) -> usize {
    ((v & !LANE).saturating_sub(L_I) as usize).clamp(1, REGS)
}

/// Copy the cached registers' lanes (`n` of `regs`) into the kept lanes at
/// pixel `at`.
#[inline(never)]
pub(crate) fn keep<const N: usize>(s: &Scratch<N>, st: &mut Still, regs: [u8; 2], n: u8, at: usize) {
    for (j, &r) in regs.iter().enumerate().take(n as usize) {
        if let Some(l) = st.lanes[j].as_deref_mut() {
            l.0[at..at + N].copy_from_slice(&s.lanes[reg_index(r)]);
        }
    }
}

/// Point the cached registers at the kept lanes at pixel `at` (read there
/// in place until written: no copy).
#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
pub(crate) fn fetch<const N: usize>(al: &mut Alias, st: &Still, regs: [u8; 2], n: u8, at: usize) {
    for (j, &r) in regs.iter().enumerate().take(n as usize) {
        if let Some(l) = st.lanes[j].as_deref() {
            al[reg_index(r)] = l.0[at..at + N].as_ptr();
        }
    }
}

/// The row steps for one row. In flash (consolea-a: as quick as in IRAM,
/// the road 3,347 µs against main's 3,393; `noise_o` and the other row
/// helpers they call stay in IRAM), and IRAM is short.
#[inline(never)]
fn row_steps<const N: usize>(prog: &Program, scratch: &mut Scratch<N>, tables: &Tables) {
    for op in &prog.ops[prog.draw_end as usize..prog.row_end as usize] {
        scalar(&mut scratch.sc, op, tables, &prog.tables);
    }
}

/// What the frame loop does for each row besides its pixels: its values
/// in the slots (the row steps run) and, when the caller asked what
/// changed, the row as it was kept before and compared after. One small
/// state and one call a row (in IRAM beside the frame loop), so the loop
/// itself carries little more than before.
struct Rows<'a> {
    changed: Option<&'a mut [u32; H]>,
    /// The row `before` holds, until it is compared (`H`: none).
    kept: usize,
    /// Kept only when the picture is word-aligned (a heap canvas always
    /// is), else every row drawn counts as changed.
    track: bool,
    before: [u32; W * 3 / 4],
}

impl<'a> Rows<'a> {
    #[inline(never)]
    fn new(changed: Option<&'a mut [u32; H]>, out: &[u8]) -> Self {
        Rows {
            track: changed.is_some() && (out.as_ptr() as usize).is_multiple_of(4),
            changed,
            kept: H,
            before: [0; W * 3 / 4],
        }
    }

    /// Row `row`'s values in the slots the pixel steps read (`py` and `y`,
    /// then the row steps), and its bytes kept (the row drawn before it
    /// compared first).
    /// One call a row: a second in the frame loop had LLVM copy the loop's
    /// whole lane dispatch, 1 KB of IRAM.
    #[inline(never)]
    #[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
    fn begin<const N: usize>(
        &mut self,
        prog: &Program,
        scratch: &mut Scratch<N>,
        tables: &Tables,
        out: &[u8],
        row: usize,
    ) {
        self.finish(out);
        // y and py count from the bottom (y = 0 is the bottom row).
        let py = ((H - 1 - row) as i32) << 16;
        scratch.sc[S_PY as usize] = py;
        scratch.sc[S_Y as usize] = fixed::div(py, ((H - 1) as i32) << 16);
        row_steps(prog, scratch, tables);
        if self.track && row < H {
            self.kept = row;
            // SAFETY: row `row`'s W × 3 bytes of the W × H × 3 picture,
            // word-aligned (`track`): `before`'s W × 3 / 4 words.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    out.as_ptr().add(row * W * 3).cast::<u32>(),
                    self.before.as_mut_ptr(),
                    W * 3 / 4,
                )
            };
        } else if let Some(c) = self.changed.as_deref_mut().and_then(|ch| ch.get_mut(row)) {
            *c = u32::MAX;
        }
    }

    /// Which 4-pixel groups of the row `begin` kept differ from its bytes
    /// now (bit `g`: pixels `4g..4g + 4`, three words), into `changed`: a
    /// word at a time, ~1-2 µs a row. (Without the copy, every row drawn
    /// counts as changed: `begin` marks it so.)
    #[inline(never)]
    #[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
    fn finish(&mut self, out: &[u8]) {
        let Some(ch) = self.changed.as_deref_mut() else {
            return;
        };
        let row = core::mem::replace(&mut self.kept, H);
        if row < H {
            let mut m = 0;
            // SAFETY: as `begin`'s: the row's W × 3 / 4 words, and
            // `before`'s as many; a group is three of each, read in step
            // (consolea-a: ~6 µs a row with a slice of `before` a group).
            unsafe {
                let mut now = out.as_ptr().add(row * W * 3).cast::<u32>();
                let mut was = self.before.as_ptr();
                for g in 0..W / 4 {
                    let d = (*was ^ *now) | (*was.add(1) ^ *now.add(1)) | (*was.add(2) ^ *now.add(2));
                    m |= u32::from(d != 0) << g;
                    now = now.add(3);
                    was = was.add(3);
                }
            }
            ch[row] = m;
        }
    }
}

#[inline(always)]
pub(crate) fn scalar(sc: &mut [i32; MAX_SCALARS], op: &Op, t: &Tables, tabs: &[i32; MAX_TABLE]) {
    let v = scalar_value(sc, op, t, tabs);
    sc[(op.d & 0x7f) as usize] = v;
}

/// What a scalar step works out (`scalar` stores it in its `d`).
#[inline(always)]
pub(crate) fn scalar_value(
    sc: &[i32; MAX_SCALARS],
    op: &Op,
    t: &Tables,
    tabs: &[i32; MAX_TABLE],
) -> i32 {
    // Operands of scalar steps are always scalar slots (< 128).
    let g = |v: u8| sc[(v & 0x7f) as usize];
    match op.code {
        code::TAB => tab(tabs, op.b, op.c, g(op.a)),
        code::SPR => spr(tabs, op.c, g(op.a), g(op.b)),
        code::NOISE_O..=code::NOISE_O_LAST => noise_o(g(op.a), g(op.b), g(op.c), sc, op.code),
        _ => eval(op.code, g(op.a), g(op.b), g(op.c), Some(t)),
    }
}

/// A wide noise of the small parts `x y z` with noise context `code -
/// NOISE_O`'s lattice offsets (out of line, and in IRAM beside the frame
/// loop: a row step, run between the row's lanes, whose flash copy cost
/// Endless road ~1.1 ms a frame on the spare — roadprobe-b, two a row).
#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
fn noise_o(x: i32, y: i32, z: i32, sc: &[i32; MAX_SCALARS], c: u8) -> i32 {
    fixed::noise_off(x, y, z, ctx(sc, c))
}

/// A sprite's pixel once a frame or a row (out of line: the frame loop is
/// in IRAM).
#[inline(never)]
fn spr(tabs: &[i32; MAX_TABLE], at: u8, u: i32, v: i32) -> i32 {
    SprHead::new(tabs, at).read(tabs, u, v)
}

/// Noise context `c - NOISE_O`'s three lattice offsets: its slots
/// ([`noise_slot`]; 0 for an axis without one).
#[inline(always)]
fn ctx(sc: &[i32; MAX_SCALARS], c: u8) -> [u32; 3] {
    let k = noise_slot(c.wrapping_sub(code::NOISE_O) as usize % NOISE_CTX, 0);
    [sc[k] as u32, sc[k + 1] as u32, sc[k + 2] as u32]
}

/// Wide value `k` of the frame (`WIDE` of them in a lane, two halves each).
#[inline(always)]
fn wget(l: &[i32], k: usize) -> i64 {
    let k = (k % WIDE) * 2;
    (l[k] as u32 as i64) | ((l[k + 1] as i64) << 32)
}

#[inline(always)]
pub(crate) fn wset(l: &mut [i32], k: usize, v: i64) {
    let k = (k % WIDE) * 2;
    l[k] = v as i32;
    l[k + 1] = (v >> 32) as i32;
}

/// One wide step (`code::W_*`, once a frame: a `loop forever` formula's
/// `t` and what is made from it; `w` the lane holding the wide values),
/// out of line and in flash: the frame loop that calls it is in IRAM.
#[inline(never)]
pub(crate) fn wide_step(sc: &mut [i32; MAX_SCALARS], w: &mut [i32], op: &Op) {
    use code::*;
    let s = |v: u8| sc[(v & 0x7f) as usize];
    let (a, d) = (wget(w, op.a as usize), op.d);
    let out = match op.code {
        W_ADD => a.wrapping_add(wget(w, op.b as usize)),
        W_ADDS => a.wrapping_add(s(op.b) as i64),
        W_SUB => a.wrapping_sub(wget(w, op.b as usize)),
        W_SUBS => a.wrapping_sub(s(op.b) as i64),
        W_NEG => a.wrapping_neg(),
        W_MULS => fixed::w_mul(a, s(op.b)),
        W_DIVS => fixed::w_div(a, s(op.b)),
        W_FLOOR => fixed::w_floor(a),
        c => {
            let v = match c {
                W_FRAC => fixed::w_frac(a),
                W_MOD => fixed::w_mod(a, s(op.b)),
                W_CMP => fixed::w_cmp(op.c, a, s(op.b)),
                W_MINMAX => fixed::w_minmax(op.c, a, s(op.b)),
                W_REM => fixed::w_rem(a, s(op.b)),
                // (`d` one of a noise context's slots: its axis.)
                W_LAT => fixed::w_lat(a, (d as usize).wrapping_sub(noise_slot(0, 0)) % 3) as i32,
                W_NOISEX => fixed::noise_off(
                    fixed::w_frac(a),
                    s(op.b),
                    s(op.c),
                    [fixed::w_lat(a, 0), 0, 0],
                ),
                _ => fixed::w_reduce(a),
            };
            sc[(d & 0x7f) as usize] = v;
            return;
        }
    };
    wset(w, d as usize, out);
}

impl<const N: usize> Scratch<N> {
    /// Register operand `v`'s own lane, to be written (no longer read from
    /// a still cache's lane).
    #[cfg(not(target_arch = "wasm32"))]
    #[inline(always)]
    fn dest(&mut self, al: &mut Alias, v: u8) -> &mut [i32; N] {
        let di = reg_index(v);
        al[di] = core::ptr::null();
        &mut self.lanes[di]
    }

    /// The operand `v` for the lane at column `base`.
    #[inline(always)]
    fn arg(&self, al: &Alias, v: u8, base: usize) -> Arg {
        if v & LANE == 0 {
            return Arg::Sc(self.sc[(v & 0x7f) as usize]);
        }
        Arg::Lane(match v & !LANE {
            L_X => self.xs[base..base + N].as_ptr(),
            L_PX => self.pxs[base..base + N].as_ptr(),
            l => {
                let r = ((l - L_I) as usize).min(REGS);
                if al[r].is_null() {
                    self.lanes[r].as_ptr()
                } else {
                    al[r]
                }
            }
        })
    }
}

/// One step of a draw loop's body run as lanes (`draw::run_lanes`: a lane
/// a turn), in flash. The steps a drawing's values mostly are
/// (`program::lane_loop`'s list, which must name these arms) go through
/// the lane loops the frame loop already has (the same instantiations: no
/// code added to IRAM); the rest are worked out turn by turn with the
/// scalar steps' own functions ([`value_of`]: what a turn at a time runs).
#[cfg(not(target_arch = "wasm32"))]
#[inline(never)]
pub(crate) fn lane_draw<const N: usize>(
    s: &mut Scratch<N>,
    op: &Op,
    t: &Tables,
    tabs: &[i32; MAX_TABLE],
) {
    use code::*;
    let al: Alias = [core::ptr::null(); REGS + 1];
    let (a, b, c) = (
        s.arg(&al, op.a, 0),
        s.arg(&al, op.b, 0),
        s.arg(&al, op.c, 0),
    );
    let Scratch { lanes, sc, .. } = s;
    let d: &mut [i32; N] = &mut lanes[reg_index(op.d)];
    match op.code {
        ADD => bin!(fast2, d, a, b, fixed::add),
        SUB => bin!(fast2, d, a, b, fixed::sub),
        MUL => bin!(fast2, d, a, b, fixed::mul),
        DIV => bin!(run2, d, a, b, fixed::div),
        RECIP => un!(run1, d, a, fixed::recip),
        LT => bin!(fast2, d, a, b, fixed::lt),
        GT => bin!(fast2, d, a, b, fixed::gt),
        AND => bin!(fast2, d, a, b, fixed::and),
        OR => bin!(fast2, d, a, b, fixed::or),
        MIN => bin!(fast2, d, a, b, fixed::min),
        MAX => bin!(fast2, d, a, b, fixed::max),
        HASH => bin!(run2, d, a, b, fixed::hash),
        MOD => bin!(run2, d, a, b, fixed::modulo),
        LE => bin!(fast2, d, a, b, fixed::le),
        GE => bin!(fast2, d, a, b, fixed::ge),
        EQ => bin!(fast2, d, a, b, fixed::eq),
        NE => bin!(fast2, d, a, b, fixed::ne),
        NEG => un!(fast1, d, a, fixed::neg),
        NOT => un!(fast1, d, a, fixed::not),
        ABS => un!(fast1, d, a, fixed::abs),
        FLOOR => un!(fast1, d, a, fixed::floor),
        FRAC => un!(fast1, d, a, fixed::frac),
        SIN => sin_lane(d, a, t),
        COS => cos_lane(d, a, t),
        TAB => tab_lane(d, a, tabs, op),
        SELECT => tern!(fast3, d, a, b, c, fixed::select),
        CLAMP => tern!(fast3, d, a, b, c, fixed::clamp),
        _ => {
            let mut one = [0i32; 3];
            let mut p = [core::ptr::null::<i32>(); 3];
            let mut stride = [0usize; 3];
            for (k, x) in [a, b, c].into_iter().enumerate() {
                match x {
                    Arg::Lane(q) => (p[k], stride[k]) = (q, 1),
                    Arg::Sc(v) => one[k] = v,
                }
            }
            for k in 0..3 {
                if stride[k] == 0 {
                    p[k] = &one[k];
                }
            }
            for o in d.iter_mut() {
                // SAFETY: as `glyph_lane`'s (the destination is never one
                // of the lanes read: `run_lanes` sees to it).
                let (x, y, z) = unsafe { (*p[0], *p[1], *p[2]) };
                *o = value_of(op, x, y, z, sc, t, tabs);
                for k in 0..3 {
                    p[k] = p[k].wrapping_add(stride[k]);
                }
            }
        }
    }
}

/// A step's value of the operand values `x y z` as the scalar steps work
/// it out (`scalar_value`'s, the slots read already): a draw loop's step
/// turn by turn ([`lane_draw`]).
#[cfg(not(target_arch = "wasm32"))]
#[inline(never)]
fn value_of(
    op: &Op,
    x: i32,
    y: i32,
    z: i32,
    sc: &[i32; MAX_SCALARS],
    t: &Tables,
    tabs: &[i32; MAX_TABLE],
) -> i32 {
    use code::*;
    match op.code {
        SHR => x >> (op.c & 31),
        PICK => pick(sc, op.b, op.c, x),
        TAB => tab(tabs, op.b, op.c, x),
        SPR => spr(tabs, op.c, x, y),
        NOISE_O..=NOISE_O_LAST => noise_o(x, y, z, sc, op.code),
        c => eval(c, x, y, z, Some(t)),
    }
}

// Steps whose lane loop is a closure, shared by the frame loop
// (`lane_op`) and the draw lanes (`lane_draw`): one closure type, so one
// instantiation of each loop for both (a closure written in each would be
// a second copy, `fast1`'s in IRAM).

/// `sin` along the lane (or of one value, filled).
#[inline(always)]
fn sin_lane<const N: usize>(d: &mut [i32; N], a: Arg, t: &Tables) {
    un!(fast1, d, a, |v| fixed::sin(t, v))
}

/// `cos` along the lane (or of one value, filled).
#[inline(always)]
fn cos_lane<const N: usize>(d: &mut [i32; N], a: Arg, t: &Tables) {
    un!(fast1, d, a, |v| fixed::cos(t, v))
}

/// `T[k]` along a lane of `k` (a table read of one value is a scalar step,
/// worked out before: nothing here).
#[inline(always)]
fn tab_lane<const N: usize>(d: &mut [i32; N], a: Arg, tabs: &[i32; MAX_TABLE], op: &Op) {
    if let Arg::Lane(p) = a {
        run1(d, L(p), |k| tab(tabs, op.b, op.c, k))
    }
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) fn lane_op<const N: usize>(
    s: &mut Scratch<N>,
    al: &mut Alias,
    op: &Op,
    narrow: bool,
    base: usize,
    t: &Tables,
    tabs: &[i32; MAX_TABLE],
    under: Option<&[u8]>,
    ink: Option<&[u8]>,
) {
    use code::*;
    let (a, b, c) = (
        s.arg(al, op.a, base),
        s.arg(al, op.b, base),
        s.arg(al, op.c, base),
    );
    // The destination is a register the compiler never also reads in the
    // same step (`compile` allocates it that way), so the `&mut` lane does
    // not alias the source pointers.
    let di = reg_index(op.d);
    debug_assert!(op.d & LANE != 0 && op.d & !LANE >= L_REG0);
    al[di] = core::ptr::null();
    let d: &mut [i32; N] = &mut s.lanes[di];
    let code = if narrow {
        op.code
    } else {
        crate::range::checked(op.code)
    };
    match code {
        ADD => bin!(fast2, d, a, b, fixed::add),
        #[cfg(not(target_arch = "wasm32"))]
        ADD_W => bin!(fast2, d, a, b, fixed::add_w),
        #[cfg(not(target_arch = "wasm32"))]
        SUB_W => bin!(fast2, d, a, b, fixed::sub_w),
        #[cfg(not(target_arch = "wasm32"))]
        MUL_W => bin!(fast2, d, a, b, fixed::mul_w),
        #[cfg(not(target_arch = "wasm32"))]
        MUL_N => bin!(fast2, d, a, b, fixed::mul_n),
        SHR => {
            let n = op.c & 31;
            un!(fast1, d, a, |v| v >> n)
        }
        SUB => bin!(fast2, d, a, b, fixed::sub),
        MUL => bin!(fast2, d, a, b, fixed::mul),
        DIV => bin!(run2, d, a, b, fixed::div),
        MOD => bin!(run2, d, a, b, fixed::modulo),
        MASK => bin!(fast2, d, a, b, fixed::mask),
        LT => bin!(fast2, d, a, b, fixed::lt),
        LE => bin!(fast2, d, a, b, fixed::le),
        GT => bin!(fast2, d, a, b, fixed::gt),
        GE => bin!(fast2, d, a, b, fixed::ge),
        EQ => bin!(fast2, d, a, b, fixed::eq),
        NE => bin!(fast2, d, a, b, fixed::ne),
        AND => bin!(fast2, d, a, b, fixed::and),
        OR => bin!(fast2, d, a, b, fixed::or),
        MIN => bin!(fast2, d, a, b, fixed::min),
        MAX => bin!(fast2, d, a, b, fixed::max),
        STEP => bin!(fast2, d, a, b, fixed::step),
        ATAN2 => bin!(run2, d, a, b, fixed::atan2),
        HASH => bin!(run2, d, a, b, fixed::hash),
        LEN => bin!(run2, d, a, b, fixed::len),
        BIT => bin!(fast2, d, a, b, fixed::bit),
        NEG => un!(fast1, d, a, fixed::neg),
        NOT => un!(fast1, d, a, fixed::not),
        ABS => un!(fast1, d, a, fixed::abs),
        FLOOR => un!(fast1, d, a, fixed::floor),
        FRAC => un!(fast1, d, a, fixed::frac),
        SIN => sin_lane(d, a, t),
        COS => cos_lane(d, a, t),
        TRI => un!(fast1, d, a, fixed::tri),
        SAW => un!(fast1, d, a, fixed::saw),
        SQR => un!(fast1, d, a, fixed::sqr),
        SQRT => un!(run1, d, a, fixed::sqrt),
        RECIP => un!(run1, d, a, fixed::recip),
        // (k is always a lane: a pick with a scalar k is compares and
        // selects.)
        PICK => {
            if let Arg::Lane(p) = a {
                let sc = &s.sc;
                run1(d, L(p), |k| pick(sc, op.b, op.c, k))
            }
        }
        SPR => spr_lane(d, [a, b], tabs, op.c),
        // (k is always a lane here: a table read of a scalar is a scalar
        // step.)
        TAB => tab_lane(d, a, tabs, op),
        SELECT => tern!(fast3, d, a, b, c, fixed::select),
        CLAMP => tern!(fast3, d, a, b, c, fixed::clamp),
        MIX => tern!(fast3, d, a, b, c, fixed::mix),
        SMOOTH => tern!(run3, d, a, b, c, fixed::smooth),
        // A whole `z` (`noise(x, y, 0)`): the plane's four corners only —
        // the other four are weighed 0 there, so the bytes are the same.
        NOISE => match c {
            Arg::Sc(z) if z & 0xffff == 0 => noise_flat_lane(d, [a, b], z),
            _ => tern!(run3, d, a, b, c, fixed::noise),
        },
        NOISE_O..=NOISE_O_LAST => noise_o_lane(d, [a, b, c], ctx(&s.sc, op.code)),
        MULR => mulr_lane(d, [a, b, c]),
        // One loop for the four, its operands read with a stride (0: a
        // value for the whole lane): the lookup dwarfs the loop, and seven
        // monomorphised copies would be ~1 KB more of wasm.
        DIGIT..=DIGIT_B => glyph_lane(d, [a, b, c], &glyphs(t, op.code)),
        U_R..=U_L => load_under(d, under, op.code - U_R),
        INK => load_ink(d, ink),
        _ => {}
    }
}

/// A fused run's kernel (`crate::fuse`) for the lane at `base`: the value
/// its last step writes (or the pixels its colour writes) as its steps
/// would leave it — the same `fixed` functions in the same order, in one
/// loop. `false` when its operands are not of the kinds the kernel takes
/// (then its steps run).
///
/// In IRAM, like the frame loop that calls it a lane per run. psi-speed3
/// put it in flash (it was 2,344 B of IRAM) and s3probe-a measured
/// Rotozoom floor's VM frame at 3,843 µs against 1,618 (its steps 2,542,
/// the same runs fused): the flash cache's layout, not the work, so back
/// here. Smaller instead: its operands are decoded by one out-of-line
/// copy of `Scratch::arg` ([`arg_at`]) rather than a dozen inlined ones.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
fn fused<const N: usize>(
    s: &mut Scratch<N>,
    al: &mut Alias,
    kind: u8,
    ops: &[Op],
    base: usize,
    t: &Tables,
    own: &Pals,
    px: &mut [[u8; 3]],
    ink: Option<&[u8]>,
) -> bool {
    use crate::fuse::*;
    match (kind, ops) {
        // The four-step checker and its last three: one kernel. For the
        // three, `r` is the first floor's value, worked out earlier
        // (`fuse` checks that a floor wrote it last), and floor is
        // idempotent (`floor(r) == r` for such an `r`), so
        // `mask(add_w(floor(r), floor(y)), m)` is the steps' value.
        (F_CHECKER | F_CHECKER3, _) => {
            let (x, y, m, dst) = match (kind, ops) {
                (F_CHECKER, [fx, fy, _, m]) => (fx.a, fy.a, m.b, m.d),
                (F_CHECKER3, [fy, add, m]) => {
                    let r = if add.a == fy.d { add.b } else { add.a };
                    (r, fy.a, m.b, m.d)
                }
                _ => return false,
            };
            let (Arg::Lane(x), Arg::Lane(y), Arg::Sc(m)) = (
                arg_at(s, al, x, base),
                arg_at(s, al, y, base),
                arg_at(s, al, m, base),
            ) else {
                return false;
            };
            let d = s.dest(al, dst);
            fast2(d, L(x), L(y), move |x, y| {
                fixed::mask(fixed::add_w(fixed::floor(x), fixed::floor(y)), m)
            });
        }
        (F_SELPAL | F_SELPAL_K, [sel, col]) => {
            let (Arg::Lane(c), Arg::Sc(va), Arg::Sc(vb)) = (
                arg_at(s, al, sel.a, base),
                arg_at(s, al, sel.b, base),
                arg_at(s, al, sel.c, base),
            ) else {
                return false;
            };
            let p = palette(t, own, col.d & code::PAL_MASK);
            let shift = (col.d & !code::PAL_SCALED) >> code::PAL_SHIFT;
            let k = match (col.d & code::PAL_SCALED != 0, arg_at(s, al, col.c, base)) {
                (false, _) => ONE,
                (true, Arg::Sc(k)) => k >> shift,
                (true, Arg::Lane(k)) => {
                    let idx = [fixed::pal_index(va), fixed::pal_index(vb)];
                    pal_select_lanes(px, c, k, p, idx, shift);
                    return true;
                }
            };
            let [ca, cb] = colours(p, [va, vb], k);
            out1(px, L(c), move |c| if c != 0 { ca } else { cb });
        }
        (F_PICKPAL, [pk, col]) => {
            let Arg::Lane(m) = arg_at(s, al, pk.a, base) else {
                return false;
            };
            let k = match (col.d & code::PAL_SCALED != 0, arg_at(s, al, col.c, base)) {
                (false, _) => ONE,
                (true, Arg::Sc(k)) => k >> ((col.d & !code::PAL_SCALED) >> code::PAL_SHIFT),
                (true, Arg::Lane(_)) => return false,
            };
            let n = (pk.c as usize).min(PICK_MAX);
            let tbl = pick_colours(&s.sc, pk, palette(t, own, col.d & code::PAL_MASK), k);
            out1(px, L(m), move |m| {
                let i = m >> 16;
                // (Outside the choices a pick is 0: its colour last.)
                tbl[if (i as u32) < n as u32 {
                    i as usize
                } else {
                    PICK_MAX
                }]
            });
        }
        (F_INK1, [_, col]) => match (col.code, ink) {
            (code::OUT_PAL, Some(k)) => {
                let p = palette(t, own, col.d & code::PAL_MASK);
                ink_direct(px, k, |b| fixed::pal(p, fixed::byte(b)))
            }
            (_, Some(k)) => ink_direct(px, k, |b| {
                let g = fixed::chan(fixed::byte(b));
                [g, g, g]
            }),
            _ => return false,
        },
        // `mul_w` for `mul_n` too: on a frame that runs the narrowed steps
        // (the only frames a kernel runs on) `range::narrow` chose `mul_n`
        // only where the product fits an i32 before the shift, and there
        // the two are the same bits (`a.wrapping_mul(b) >> 16` is
        // `(a*b as i64 >> 16) as i32` when a*b fits). One kernel each
        // instead of two (psi-speed3: −134 B of IRAM, ~1 instruction a
        // pixel more for a `mul_n`).
        (F_MADD, [mul, add]) => {
            let (a, b) = match (arg_at(s, al, mul.a, base), arg_at(s, al, mul.b, base)) {
                (Arg::Lane(a), Arg::Sc(b)) | (Arg::Sc(b), Arg::Lane(a)) => (a, b),
                _ => return false,
            };
            let c = arg_at(s, al, if add.a == mul.d { add.b } else { add.a }, base);
            let own = core::ptr::eq(a, s.lanes[reg_index(add.d)].as_ptr());
            let d = s.dest(al, add.d);
            match c {
                // (Into the lane multiplied: read and written in place.)
                Arg::Sc(c) if own => fast1_self(d, move |a| fixed::add_w(fixed::mul_w(a, b), c)),
                _ if own => return false,
                Arg::Sc(c) => fast1(d, L(a), move |a| fixed::add_w(fixed::mul_w(a, b), c)),
                Arg::Lane(c) => fast2(d, L(a), L(c), move |a, c| {
                    fixed::add_w(fixed::mul_w(a, b), c)
                }),
            }
        }
        _ => return false,
    }
    true
}

/// `Scratch::arg` out of line, in IRAM: the fused runs' operands (a few a
/// lane each), one copy for all of [`fused`]'s arms.
#[cfg(not(target_arch = "wasm32"))]
#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
fn arg_at<const N: usize>(s: &Scratch<N>, al: &Alias, v: u8, base: usize) -> Arg {
    s.arg(al, v, base)
}

/// The colours of palette entries `v` scaled by `k` (`pal_lit` of each;
/// `k` = 1: the entries' bytes, as `pal`), once a lane for a fused colour:
/// in flash, out of the frame loop's IRAM.
#[cfg(not(target_arch = "wasm32"))]
#[inline(never)]
fn colours(p: &[[u8; 3]; 32], v: [i32; 2], k: i32) -> [[u8; 3]; 2] {
    v.map(|v| fixed::scale_rgb(p[fixed::pal_index(v)], k))
}

/// A fused `pal(P, pick(m, …)) * k`'s colours (`colours`): each choice's,
/// then (at `PICK_MAX`) the colour of 0, a pick's value outside them.
#[cfg(not(target_arch = "wasm32"))]
#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
fn pick_colours(
    sc: &[i32; MAX_SCALARS],
    pk: &Op,
    p: &[[u8; 3]; 32],
    k: i32,
) -> [[u8; 3]; crate::fuse::PICK_MAX + 1] {
    let n = (pk.c as usize).min(crate::fuse::PICK_MAX);
    let mut tbl = [[0u8; 3]; crate::fuse::PICK_MAX + 1];
    for (i, c) in tbl.iter_mut().enumerate() {
        let v = if i < n {
            pick(sc, pk.b, pk.c, (i as i32) << 16)
        } else {
            0
        };
        *c = fixed::scale_rgb(p[fixed::pal_index(v)], k);
    }
    tbl
}

/// `pal(P, select(c, A, B)) * k` along the lane (`crate::fuse`'s
/// `F_SELPAL`, k a lane): entry `idx[0]` where c is not 0, else `idx[1]`,
/// each choice's last scale and colour kept (`pal_lit_lanes`' reuse, one
/// for each).
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
#[inline(never)]
fn pal_select_lanes(
    px: &mut [[u8; 3]],
    c: *const i32,
    k: *const i32,
    p: &[[u8; 3]; 32],
    idx: [usize; 2],
    shift: u8,
) {
    // (A scale of i32::MIN is ≤ 0: black, the colour kept to start with.)
    let mut last = [i32::MIN; 2];
    let mut rgb = [[0u8; 3]; 2];
    for (j, o) in px.iter_mut().enumerate() {
        // SAFETY: `c` and `k` are lanes of at least `px.len()` values,
        // alive for the call.
        let (c, k) = unsafe { (*c.add(j), *k.add(j)) };
        let w = usize::from(c == 0);
        let s = k >> shift;
        if s != last[w] {
            last[w] = s;
            rgb[w] = fixed::scale_rgb(p[idx[w]], s);
        }
        *o = rgb[w];
    }
}

/// [`fast1`] of a lane into itself (a fused run whose result goes into the
/// register it reads, `crate::fuse`).
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
#[inline(never)]
fn fast1_self<const N: usize>(d: &mut [i32; N], f: impl Fn(i32) -> i32) {
    for o in d.as_chunks_mut::<4>().0 {
        let x = *o;
        *o = [f(x[0]), f(x[1]), f(x[2]), f(x[3])];
    }
}

/// `f` of each byte of the canvas along the lane (`crate::fuse`'s
/// `F_INK1`: `pal(P, ink)`, `gray(ink)`).
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
#[inline(never)]
fn ink_direct(px: &mut [[u8; 3]], ink: &[u8], f: impl Fn(u8) -> [u8; 3]) {
    for (o, &b) in px.iter_mut().zip(ink) {
        *o = f(b);
    }
}

/// `mulr` along the lane, its operands read with a stride (0: a value for
/// the lane): one loop in flash for every kind.
#[inline(never)]
fn mulr_lane<const N: usize>(d: &mut [i32; N], args: [Arg; 3]) {
    let mut one = [0i32; 3];
    let mut p = [core::ptr::null::<i32>(); 3];
    let mut stride = [0usize; 3];
    for k in 0..3 {
        match args[k] {
            Arg::Lane(q) => (p[k], stride[k]) = (q, 1),
            Arg::Sc(v) => one[k] = v,
        }
    }
    for k in 0..3 {
        if stride[k] == 0 {
            p[k] = &one[k];
        }
    }
    for o in d.iter_mut() {
        // SAFETY: as `glyph_lane`'s.
        let (a, b, c) = unsafe { (*p[0], *p[1], *p[2]) };
        *o = fixed::mulr(a, b, c);
        for k in 0..3 {
            p[k] = p[k].wrapping_add(stride[k]);
        }
    }
}

/// A sprite's pixels along the lane (its header read once), the column
/// and row read with a stride (0: a value for the lane); a row value
/// outside its rows fills the lane with 0 at once. In IRAM (road-cost).
#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
fn spr_lane<const N: usize>(d: &mut [i32; N], args: [Arg; 2], tabs: &[i32; MAX_TABLE], at: u8) {
    let hd = SprHead::new(tabs, at);
    // A row value outside the sprite's rows: 0 along the whole lane (as
    // `read` gives each pixel).
    if let Arg::Sc(v) = args[1] {
        if (v >> 16) as u32 >= hd.h {
            d.fill(0);
            return;
        }
    }
    let mut one = [0i32; 2];
    let mut p = [core::ptr::null::<i32>(); 2];
    let mut stride = [0usize; 2];
    for k in 0..2 {
        match args[k] {
            Arg::Lane(q) => (p[k], stride[k]) = (q, 1),
            Arg::Sc(v) => one[k] = v,
        }
    }
    for k in 0..2 {
        if stride[k] == 0 {
            p[k] = &one[k];
        }
    }
    for o in d.iter_mut() {
        // SAFETY: as `glyph_lane`'s.
        let (u, v) = unsafe { (*p[0], *p[1]) };
        *o = hd.read(tabs, u, v);
        for k in 0..2 {
            p[k] = p[k].wrapping_add(stride[k]);
        }
    }
}

/// A wide noise along the lane ([`fixed::noise_off`]), its operands read
/// with a stride (0: a value for the lane): one loop for every kind.
#[inline(never)]
fn noise_o_lane<const N: usize>(d: &mut [i32; N], args: [Arg; 3], off: [u32; 3]) {
    let mut one = [0i32; 3];
    let mut p = [core::ptr::null::<i32>(); 3];
    let mut stride = [0usize; 3];
    for k in 0..3 {
        match args[k] {
            Arg::Lane(q) => (p[k], stride[k]) = (q, 1),
            Arg::Sc(v) => one[k] = v,
        }
    }
    for k in 0..3 {
        if stride[k] == 0 {
            p[k] = &one[k];
        }
    }
    for o in d.iter_mut() {
        // SAFETY: as `glyph_lane`'s.
        let (x, y, z) = unsafe { (*p[0], *p[1], *p[2]) };
        *o = fixed::noise_off(x, y, z, off);
        for k in 0..3 {
            p[k] = p[k].wrapping_add(stride[k]);
        }
    }
}

/// A font's pixel for each element of the lane.
#[inline(never)]
fn glyph_lane<const N: usize>(d: &mut [i32; N], args: [Arg; 3], g: &crate::tables::Glyphs) {
    let mut one = [0i32; 3];
    let mut p = [core::ptr::null::<i32>(); 3];
    let mut stride = [0usize; 3];
    for k in 0..3 {
        match args[k] {
            Arg::Lane(q) => (p[k], stride[k]) = (q, 1),
            Arg::Sc(v) => one[k] = v,
        }
    }
    for k in 0..3 {
        if stride[k] == 0 {
            p[k] = &one[k];
        }
    }
    for o in d.iter_mut() {
        // SAFETY: each pointer is a lane of `N` values read once per
        // element (stride 1), or one value (stride 0), alive for the call.
        let (c, x, y) = unsafe { (*p[0], *p[1], *p[2]) };
        *o = fixed::glyph(g, c, x, y);
        for k in 0..3 {
            p[k] = p[k].wrapping_add(stride[k]);
        }
    }
}

/// A channel of what is under the lane (0 red, 1 green, 2 blue, 3 the
/// luminance) as 0..1; 0 with nothing under.
#[inline(never)]
fn load_under<const N: usize>(d: &mut [i32; N], under: Option<&[u8]>, ch: u8) {
    let Some(u) = under else {
        d.fill(0);
        return;
    };
    let px = u.as_chunks::<3>().0;
    match ch {
        0..=2 => {
            let ch = ch as usize;
            for (o, p) in d.iter_mut().zip(px) {
                *o = fixed::byte(p[ch]);
            }
        }
        _ => {
            for (o, p) in d.iter_mut().zip(px) {
                *o = fixed::byte(fixed::luma(p[0], p[1], p[2]));
            }
        }
    }
}

/// The draw stage's canvas under the lane, 0..1; 0 without one.
#[inline(never)]
#[cfg_attr(target_os = "espidf", link_section = ".iram1.psi")]
fn load_ink<const N: usize>(d: &mut [i32; N], ink: Option<&[u8]>) {
    match ink {
        Some(k) => {
            for (o, &b) in d.iter_mut().zip(k) {
                *o = fixed::byte(b);
            }
        }
        None => d.fill(0),
    }
}

/// A colour step: the bottom layer's colour written over the lane, or a
/// layer's colour (into a row on the stack) blended onto it.
fn colour_step<const N: usize>(
    s: &Scratch<N>,
    al: &Alias,
    op: &Op,
    base: usize,
    t: &Tables,
    own: &Pals,
    px: &mut [[u8; 3]],
) {
    let (kind, blend) = code::colour_parts(op.code);
    let Some(mode) = blend else {
        colour(s, al, op, base, t, own, px);
        return;
    };
    let (v, n) = code::operands(op);
    let plain = Op {
        code: kind,
        d: if kind == code::OUT_PAL { op.d } else { 0 },
        ..*op
    };
    let mut row = [[0u8; 3]; W];
    let src = &mut row[..px.len()];
    colour(s, al, &plain, base, t, own, src);
    // A layer that covers everything (the usual: no coverage given) is one
    // loop per blend, so the blend is not a branch per channel (an indirect
    // jump on the ESP32); one with a coverage, a lane or a value read with
    // a stride of 0, shares one loop.
    let one;
    let (k, stride) = match s.arg(al, v[n - 1], base) {
        Arg::Sc(v) if v >= ONE => {
            match mode {
                fixed::BLEND_ADD => blend_all::<{ fixed::BLEND_ADD }>(px, src),
                fixed::BLEND_MULTIPLY => blend_all::<{ fixed::BLEND_MULTIPLY }>(px, src),
                fixed::BLEND_SCREEN => blend_all::<{ fixed::BLEND_SCREEN }>(px, src),
                fixed::BLEND_MAX => blend_all::<{ fixed::BLEND_MAX }>(px, src),
                fixed::BLEND_MIN => blend_all::<{ fixed::BLEND_MIN }>(px, src),
                _ => px.copy_from_slice(src),
            }
            return;
        }
        Arg::Lane(p) => (p, 1),
        Arg::Sc(v) => {
            one = v;
            (&one as *const i32, 0)
        }
    };
    blend_cover(px, src, k, stride, mode);
}

#[inline(never)]
fn blend_all<const M: u8>(px: &mut [[u8; 3]], src: &[[u8; 3]]) {
    for (d, s) in px.iter_mut().zip(src) {
        for c in 0..3 {
            d[c] = fixed::blend(M, d[c], s[c]);
        }
    }
}

/// `px` (what is below) with `src` blended on by `mode`, covered by the
/// values at `k`, `stride` apart (0: one value for the lane).
#[inline(never)]
fn blend_cover(px: &mut [[u8; 3]], src: &[[u8; 3]], mut k: *const i32, stride: usize, mode: u8) {
    for (d, s) in px.iter_mut().zip(src) {
        // SAFETY: `k` is a lane of at least `px.len()` values (stride 1) or
        // one value (stride 0), alive for the call.
        let a = unsafe { *k };
        for c in 0..3 {
            d[c] = fixed::cover(d[c], fixed::blend(mode, d[c], s[c]), a);
        }
        k = k.wrapping_add(stride);
    }
}

fn colour<const N: usize>(
    s: &Scratch<N>,
    al: &Alias,
    op: &Op,
    base: usize,
    t: &Tables,
    own: &Pals,
    px: &mut [[u8; 3]],
) {
    use code::*;
    let (a, b, c) = (
        s.arg(al, op.a, base),
        s.arg(al, op.b, base),
        s.arg(al, op.c, base),
    );
    match op.code {
        OUT_RGB => outt!(px, a, b, c, |r, g, b| [
            fixed::chan(r),
            fixed::chan(g),
            fixed::chan(b)
        ]),
        OUT_HSV => outt!(px, a, b, c, fixed::hsv),
        OUT_PAL if op.d & PAL_SCALED != 0 => pal_lit_lane(
            px,
            [a, c],
            palette(t, own, op.d & PAL_MASK),
            (op.d & !PAL_SCALED) >> PAL_SHIFT,
        ),
        OUT_PAL => {
            let p = palette(t, own, op.d & PAL_MASK);
            match a {
                Arg::Lane(q) => out1(px, L(q), |k| fixed::pal(p, k)),
                Arg::Sc(v) => px.fill(fixed::pal(p, v)),
            }
        }
        _ => match a {
            Arg::Lane(q) => out1(px, L(q), |k| {
                let v = fixed::chan(k);
                [v, v, v]
            }),
            Arg::Sc(v) => {
                let v = fixed::chan(v);
                px.fill([v, v, v])
            }
        },
    }
}

/// `ONE` for callers that build inputs by hand.
pub const Q16_ONE: i32 = ONE;

/// psi-probe's view of the frames drawn (the `stats` feature): how many
/// ran the narrowed steps and how many their checked forms, and where the
/// last one's memory was (the working memory, the picture, a still cache's
/// first lane), to tell placement from code on the device.
#[cfg(feature = "stats")]
#[doc(hidden)]
pub mod stats {
    use super::*;
    use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering::Relaxed};

    /// A cycle counter the firmware installs (`fn() -> u32`, as a usize;
    /// 0: none), and the last frame's stamps on it: 0 the player's frame
    /// begun, 1 the VM begun, 2 its rows begun (the frame's steps, the
    /// draw stage and the still cache's choice done), 3 its rows done, 4
    /// the player's frame done.
    pub static CLOCK: AtomicUsize = AtomicUsize::new(0);
    pub static STAMPS: [AtomicU32; 5] = [const { AtomicU32::new(0) }; 5];

    /// Stamp `k` with the clock now.
    #[inline]
    pub fn stamp(k: usize) {
        let f = CLOCK.load(Relaxed);
        if f != 0 && k < 5 {
            // SAFETY: stored by the firmware from a `fn() -> u32`.
            let f = unsafe { core::mem::transmute::<usize, fn() -> u32>(f) };
            STAMPS[k].store(f(), Relaxed);
        }
    }

    pub static NARROWED: AtomicU32 = AtomicU32::new(0);
    pub static CHECKED: AtomicU32 = AtomicU32::new(0);
    pub static SCRATCH: AtomicUsize = AtomicUsize::new(0);
    pub static OUT: AtomicUsize = AtomicUsize::new(0);
    pub static STILL: AtomicUsize = AtomicUsize::new(0);
    pub static STILL1: AtomicUsize = AtomicUsize::new(0);

    pub(super) fn note<const N: usize>(
        prog: &Program,
        s: &Scratch<N>,
        cache: &Cache,
        out: &[u8],
        inputs: &Inputs,
    ) {
        if crate::range::inputs_in_range(prog, inputs) {
            NARROWED.fetch_add(1, Relaxed);
        } else {
            CHECKED.fetch_add(1, Relaxed);
        }
        SCRATCH.store(s as *const _ as usize, Relaxed);
        OUT.store(out.as_ptr() as usize, Relaxed);
        let still = match cache {
            Cache::Use(st) => st.lanes[0].as_deref().map_or(0, |l| l.0.as_ptr() as usize),
            _ => 0,
        };
        STILL.store(still, Relaxed);
        let still1 = match cache {
            Cache::Use(st) => st.lanes[1].as_deref().map_or(0, |l| l.0.as_ptr() as usize),
            _ => 0,
        };
        STILL1.store(still1, Relaxed);
    }
}
