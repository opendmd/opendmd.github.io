//! `layer rate N { }`: slow layers (console-lessons Phase B, 2.2).
//!
//! The bottom layers of a formula may say `rate N`: they are redrawn every
//! Nth frame of the formula's grid, a band of rows a frame, and every frame
//! the layers above are drawn over what they last drew. The compiler lays
//! the slow layers' steps out first, after a `code::SLOW` marker (their own
//! values: nothing shared with the layers above); the VM runs the slow part
//! alone, or the fast part alone over a picture ([`crate::vm::frame`]'s
//! `part`). Never a `rate` formula's steps in one go: the render functions
//! send one to [`render_scratch`].
//!
//! **The definition** (a function of the frame's inputs alone, so the
//! browser, the simulator and the device draw the same bytes): the
//! formula's grid is its `fps` line's frame time F (20 ms without one); the
//! frame of time `ms` is k = floor(ms / F); row r (from the top) is in band
//! b = floor(r·N/32); and in frame k row r of the slow layers is drawn as
//! at frame k − ((k − b) mod N), the last frame of its band — its time
//! taken back by whole frames (into the loop before, for a formula that
//! loops — for a loop that isn't a whole number of frames that may be an
//! instant the grid never shows; never before the start, for one that goes
//! on forever), the clock,
//! the sliders and `seed` as they are now. When the slow layers read no
//! `t`, every row is simply drawn now.
//!
//! [`render_scratch`] draws a frame from nothing (each band's rows at their
//! time, then the fast part over them): the render functions' way for a
//! `rate` formula. [`Held`] keeps the slow layers' picture between frames
//! and redraws only the rows whose time (or clock, sliders, `seed`, as far
//! as the slow layers read them) moved since: in steady play one band a
//! frame, all of them when the clock they read ticks. Both give the bytes
//! of the definition.

use crate::draw::Ink;
use crate::program::*;
use crate::still::Still;
use crate::tables::Tables;
use crate::vm::{frame, Inputs, Scratch};
use alloc::boxed::Box;

/// A frame drawn whole (every program but a `rate` one's way).
pub(crate) const WHOLE: u8 = 0;
/// Only the slow layers' steps (after the marker).
pub(crate) const SLOW: u8 = 1;
/// The steps but the slow layers', over a picture.
pub(crate) const FAST: u8 = 2;

/// The pixel steps a `part` runs (`vm::frame`'s), from and to: the slow
/// layers' are the first, after their marker (`compile`'s layout). Out of
/// line, in flash: once a frame. (A branch for the marker in the frame
/// loop cost 266 B of IRAM.)
#[inline(never)]
pub(crate) fn steps(prog: &Prog, part: u8) -> (usize, usize) {
    let (re, pe) = (prog.row_end as usize, prog.len().min(MAX_OPS));
    match prog.slow_marker() {
        Some(m) if part == SLOW => (m + 1, (m + 1 + prog.ops[m].a as usize).min(pe)),
        Some(m) => {
            debug_assert!(part == FAST, "a `rate` formula drawn whole");
            ((m + 1 + prog.ops[m].a as usize).min(pe), pe)
        }
        None => (re, pe),
    }
}

/// What the slow layers read (the `SLOW` marker's `c`), transitively:
/// `t`, the clock, a slider, `seed`.
pub(crate) const READS_T: u8 = 1;
pub(crate) const READS_CLOCK: u8 = 2;
pub(crate) const READS_SLIDERS: u8 = 4;
pub(crate) const READS_SEED: u8 = 8;
/// A built-in input (`crate::world`): the picture is drawn again when one
/// the program reads moves.
pub(crate) const READS_WORLD: u8 = 16;

/// The largest `rate`.
pub const MAX_RATE: usize = 200;

/// The time (ms) each row of the slow layers is drawn at in the frame of
/// `inputs` (see the module), and whether the slow layers read `t` at all
/// (else every row is drawn at `inputs` as they are).
#[cfg_attr(target_arch = "xtensa", optimize(size))]
fn plan(prog: &Prog, m: usize, inputs: &Inputs) -> ([i64; H], bool) {
    let mut at = [0i64; H];
    let op = &prog.ops[m];
    if op.c & READS_T == 0 {
        return (at, false);
    }
    let n = i64::from(op.d.max(1));
    let f = i64::from(prog.frame_ms().unwrap_or(20));
    // The time of the frame: the whole ms whose `t` this is (`at_ms` is
    // one to one on whole ms; any other `t` the ms just after it).
    let ms = if prog.forever() {
        (inputs.tw.max(0) * 1000 + 65535) / 65536
    } else {
        (i64::from(inputs.t.max(0)) * 1000 + 65535) / 65536
    };
    let k = ms / f;
    let lp = i64::from(prog.loop_ms().max(1));
    for (r, a) in at.iter_mut().enumerate() {
        let b = r as i64 * n / H as i64;
        let back = (k - b).rem_euclid(n);
        let t = (k - back) * f;
        *a = if prog.forever() {
            t.max(0)
        } else {
            t.rem_euclid(lp)
        };
    }
    (at, true)
}

/// `inputs` at time `ms` (the slow rows' time; the rest as they are).
fn inputs_at(prog: &Prog, inputs: &Inputs, ms: i64) -> Inputs {
    let mut i = *inputs;
    i.set_ms(prog, ms.max(0) as u64);
    i
}

/// Draw the slow layers' rows of `rows` into `out`, each band at its time
/// (`dirty`: only the rows whose time is not `at[r]` already, then noted
/// there). Returns the rows drawn.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(target_arch = "xtensa", optimize(size))]
fn slow_rows<const N: usize>(
    prog: &Prog,
    m: usize,
    scratch: &mut Scratch<N>,
    inputs: &Inputs,
    tables: &Tables,
    rows: u32,
    out: &mut [u8],
    lanes: bool,
    mut held: Option<(&mut [i64; H], u32)>,
) -> u32 {
    let (times, reads_t) = plan(prog, m, inputs);
    let mut due = rows;
    if let Some((at, valid)) = held.as_mut() {
        due = 0;
        for r in 0..H {
            if rows & (1 << r) != 0 && (*valid & (1 << r) == 0 || at[r] != times[r]) {
                due |= 1 << r;
            }
        }
    }
    let mut left = due;
    while left != 0 {
        let r = left.trailing_zeros() as usize;
        let tm = times[r];
        let mut mask = 0u32;
        for (j, &t) in times.iter().enumerate() {
            if left & (1 << j) != 0 && t == tm {
                mask |= 1 << j;
            }
        }
        let i = if reads_t {
            inputs_at(prog, inputs, tm)
        } else {
            *inputs
        };
        frame::<N>(
            prog,
            scratch,
            None,
            None,
            &i,
            tables,
            None,
            mask,
            out,
            lanes,
            core::hint::black_box(None),
            core::hint::black_box(SLOW),
        );
        left &= !mask;
    }
    if let Some((at, _)) = held {
        for r in 0..H {
            if due & (1 << r) != 0 {
                at[r] = times[r];
            }
        }
    }
    due
}

/// The slow layers' rows of `rows` from nothing, into `out` (a kernel's
/// frame without a held picture draws them so first, then its fast part
/// over them). Nothing for a formula without `rate`.
#[doc(hidden)]
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub fn draw_slow<const N: usize>(
    prog: &Prog,
    scratch: &mut Scratch<N>,
    inputs: &Inputs,
    tables: &Tables,
    rows: u32,
    out: &mut [u8],
) {
    if let Some(m) = prog.slow_marker() {
        slow_rows::<N>(prog, m, scratch, inputs, tables, rows, out, true, None);
    }
}

/// A `rate` formula's frame from nothing (the render functions' way for
/// one): the slow layers' rows of `rows`, each band at its time, then the
/// fast part over them, into `out`; `changed` (when asked) says every row
/// drawn changed. The still cache, the canvas and `under` as the render
/// functions take them (a `rate` formula reads no picture under).
#[allow(clippy::too_many_arguments)]
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn render_scratch<const N: usize>(
    prog: &Prog,
    scratch: &mut Scratch<N>,
    ink: Option<&mut Ink>,
    still: Option<&mut Still>,
    inputs: &Inputs,
    tables: &Tables,
    rows: u32,
    out: &mut [u8],
    lanes: bool,
    changed: Option<&mut [u32; H]>,
) {
    let Some(m) = prog.slow_marker() else { return };
    slow_rows::<N>(prog, m, scratch, inputs, tables, rows, out, lanes, None);
    frame::<N>(
        prog,
        scratch,
        ink,
        still,
        inputs,
        tables,
        None,
        rows,
        out,
        lanes,
        core::hint::black_box(None),
        core::hint::black_box(FAST),
    );
    if let Some(ch) = changed {
        for (r, c) in ch.iter_mut().enumerate() {
            if rows & (1 << r) != 0 {
                *c = u32::MAX;
            }
        }
    }
}

/// The built-in inputs `prog` reads as they are in `inputs`, hashed (the
/// slow layers' picture is drawn again when one moves).
fn world_key(prog: &Prog, inputs: &Inputs) -> i32 {
    prog.ins.iter().fold(0x811c_9dc5u32, |h, (_, b)| {
        (h ^ inputs.world.read(b) as u32).wrapping_mul(0x0100_0193)
    }) as i32
}

/// The slow layers' picture between frames (12 KB and a little): a
/// player's, for a `rate` formula ([`Program::rate`]). All-zero is a valid
/// empty one (nothing held). Give it back to [`Held::forget`] whenever the
/// program changes (a live edit): it also notices a program of another
/// shape by itself.
pub struct Held {
    pic: [u8; W * H * 3],
    /// Each row's time as drawn, and which rows hold a picture.
    at: [i64; H],
    valid: u32,
    /// The inputs besides `t` the slow layers read, as drawn, and the
    /// program's shape.
    key: [i32; 8],
    shape: u32,
}

impl Held {
    /// Its size, bytes.
    pub const BYTES: usize = core::mem::size_of::<Held>();

    const LAYOUT: core::alloc::Layout = core::alloc::Layout::new::<Held>();

    /// An empty one on the heap, or `None` when the heap has no block for
    /// it.
    pub fn try_boxed() -> Option<Box<Held>> {
        // SAFETY: a non-zero-sized layout; `Held` is integers, so the
        // zeroed memory is a valid value; the box frees it with the same
        // layout.
        unsafe {
            let p = alloc::alloc::alloc_zeroed(Self::LAYOUT).cast::<Held>();
            (!p.is_null()).then(|| Box::from_raw(p))
        }
    }

    /// Hold nothing (the next frame draws every slow row).
    pub fn forget(&mut self) {
        self.valid = 0;
    }

    /// The slow layers' picture (128 × 32 RGB888).
    pub fn picture(&self) -> &[u8] {
        &self.pic
    }

    /// Bring the slow layers' picture to the frame of `inputs`: the rows
    /// whose time moved (or all, when what else they read moved or the
    /// program is another); how many rows were drawn.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    pub fn update<const N: usize>(
        &mut self,
        prog: &Prog,
        scratch: &mut Scratch<N>,
        inputs: &Inputs,
        tables: &Tables,
    ) -> u32 {
        let Some(m) = prog.slow_marker() else {
            return 0;
        };
        let reads = prog.ops[m].c;
        let pick = |on: u8, v: i32| if reads & on != 0 { v } else { 0 };
        let key = [
            pick(READS_CLOCK, i32::from(inputs.hour)),
            pick(READS_CLOCK, i32::from(inputs.minute)),
            pick(READS_CLOCK, i32::from(inputs.second)),
            pick(READS_SLIDERS, inputs.a),
            pick(READS_SLIDERS, inputs.b),
            pick(READS_SLIDERS, inputs.c),
            pick(READS_SEED, inputs.seed),
            pick(READS_WORLD, world_key(prog, inputs)),
        ];
        let shape = shape(prog, m);
        if key != self.key || shape != self.shape {
            self.valid = 0;
            self.key = key;
            self.shape = shape;
        }
        let drawn = slow_rows::<N>(
            prog,
            m,
            scratch,
            inputs,
            tables,
            u32::MAX,
            &mut self.pic,
            true,
            Some((&mut self.at, self.valid)),
        );
        self.valid |= drawn;
        drawn.count_ones()
    }
}

/// A program's shape, cheaply: enough that a held picture is not used for
/// another formula's by mistake (the player forgets it at a live edit).
#[cfg_attr(target_arch = "xtensa", optimize(size))]
fn shape(prog: &Prog, m: usize) -> u32 {
    let op = &prog.ops[m];
    let mut h: u32 = 0x811c_9dc5;
    for v in [
        prog.frame_end as u32,
        prog.draw_end as u32,
        prog.row_end as u32,
        prog.pixel_end as u32,
        prog.n_scalars as u32,
        prog.loop_ms,
        prog.frame_ms().unwrap_or(0),
        u32::from_le_bytes([op.d, op.a, op.b, op.c]),
        m as u32,
    ] {
        h = (h ^ v).wrapping_mul(0x0100_0193);
    }
    h | 1
}

/// A `rate` formula's frame with its slow layers held ([`Held`]): the slow
/// rows due brought up to date, then the fast part over the held picture
/// into `out` (every row; `changed` as [`crate::render_rows_changed`] says
/// it). The bytes of [`crate::render_rows_still`]'s frame. A program
/// without `rate`: its whole frame, `held` untouched.
#[allow(clippy::too_many_arguments)]
pub fn render_held<const N: usize>(
    prog: &Prog,
    scratch: &mut Scratch<N>,
    ink: Option<&mut Ink>,
    still: Option<&mut Still>,
    held: &mut Held,
    inputs: &Inputs,
    tables: &Tables,
    out: &mut [u8],
    changed: Option<&mut [u32; H]>,
) {
    if prog.slow_marker().is_none() {
        match changed {
            Some(ch) => crate::render_rows_changed::<N>(
                prog,
                scratch,
                ink,
                still,
                inputs,
                tables,
                None,
                u32::MAX,
                out,
                ch,
            ),
            None => crate::render_rows_still::<N>(
                prog,
                scratch,
                ink,
                still,
                inputs,
                tables,
                None,
                u32::MAX,
                out,
            ),
        }
        return;
    }
    held.update::<N>(prog, scratch, inputs, tables);
    render_fast::<N>(
        prog,
        scratch,
        ink,
        still,
        inputs,
        tables,
        Some(&held.pic),
        u32::MAX,
        out,
        changed,
    );
}

/// The fast part of a `rate` formula's frame over `under` (the slow
/// layers' picture; `None`: what `out` holds): the JIT's check and trial
/// against the VM.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn render_fast<const N: usize>(
    prog: &Prog,
    scratch: &mut Scratch<N>,
    ink: Option<&mut Ink>,
    still: Option<&mut Still>,
    inputs: &Inputs,
    tables: &Tables,
    under: Option<&[u8]>,
    rows: u32,
    out: &mut [u8],
    changed: Option<&mut [u32; H]>,
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
        core::hint::black_box(changed),
        core::hint::black_box(FAST),
    );
}
