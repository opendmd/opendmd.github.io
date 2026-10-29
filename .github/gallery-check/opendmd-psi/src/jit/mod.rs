//! A narrow JIT for the ESP32 (phase 1 of `docs/research/psi-jit-esp32.md`):
//! a formula's pixel stage compiled to Xtensa code in executable heap, the
//! VM for everything else.
//!
//! - [`enc`]: the instruction encoders (tested against the GNU assembler).
//! - [`lower`]: the pixel stage after the column and still runs → one
//!   function a row, or a [`Refusal`] (the formula plays on the VM).
//! - here: the frame driver (`frame`, the VM's frame with the kernel in
//!   place of the pixel steps), the helpers a kernel calls, and on the
//!   device the `Kernel` (its block of `MALLOC_CAP_EXEC` heap).
//!
//! **The VM is the definition.** A kernel draws only frames whose inputs are
//! in the ranges the program was narrowed for (else the VM's checked steps
//! draw it, as they would); the player checks a kernel against the VM when
//! it opens one (`Kernel::check`) and plays the formula on the VM if the
//! bytes differ. The host tests run kernels through an emulator of the
//! instructions they use (`emu`) against the VM over the goldens and the
//! bundled formulas.

#[cfg(test)]
mod emu;
pub mod enc;
#[cfg(test)]
mod enc_tests;
pub mod lower;
#[cfg(test)]
mod tests;

pub use lower::Refusal;
use lower::{hid, Lowered, Word, NONE};

use crate::fixed;
use crate::fuse::PICK_MAX;
use crate::program::*;
use crate::draw::Ink;
use crate::still::Still;
use crate::tables::{Glyphs, Tables};
use crate::vm::{self, Alias, Inputs, Scratch};

/// Where a kernel runs: the device (real addresses, a call), or the host
/// tests' emulator (its own address space).
pub(crate) trait Target {
    /// Before a frame's words are filled.
    fn begin(&mut self) {}
    /// The address the kernel reads `len` bytes at `p` through.
    fn addr(&mut self, p: *const u8, len: usize) -> u32;
    /// Helper `id`'s address.
    fn helper(&mut self, id: u8) -> u32;
    /// Run the kernel for a row.
    fn run(&mut self, ctx: &mut [u32], out: *mut u8);
    /// Run the row stage's function (at `at` in the code) on `sc`.
    fn run_rows(&mut self, at: usize, ctx: &mut [u32], sc: *mut i32);
}

/// A colour as a word: `r | g << 8 | b << 16`.
#[inline]
fn pack(c: [u8; 3]) -> u32 {
    u32::from(c[0]) | u32::from(c[1]) << 8 | u32::from(c[2]) << 16
}

/// Helper `id` of the arguments a kernel passed (a2..a6 of the callee),
/// pointers read through `ptr` (the device's are real; the emulator's its
/// own). The one definition, for the device's shims and the emulator.
///
/// # Safety
/// The pointer arguments must be what the kernel passes: a `Word`'s
/// address (the scalar slots, the tables, a palette entry, a font) or the
/// pixel's three bytes.
#[inline(always)]
pub(crate) unsafe fn eval_helper(id: u8, a: [u32; 5], ptr: &dyn Fn(u32) -> *mut u8) -> u32 {
    let i = |k: usize| a[k] as i32;
    (match id {
        hid::DIV => fixed::div(i(0), i(1)),
        hid::MOD => fixed::modulo(i(0), i(1)),
        hid::ATAN2 => fixed::atan2(i(0), i(1)),
        hid::HASH => fixed::hash(i(0), i(1)),
        hid::LEN => fixed::len(i(0), i(1)),
        hid::BIT => fixed::bit(i(0), i(1)),
        hid::SQRT => fixed::sqrt(i(0)),
        hid::RECIP => fixed::recip(i(0)),
        hid::SMOOTH => fixed::smooth(i(0), i(1), i(2)),
        hid::MIX => fixed::mix(i(0), i(1), i(2)),
        hid::MULR => fixed::mulr(i(0), i(1), i(2)),
        hid::NOISE => {
            // The VM takes a whole `z` from the plane's four corners: the
            // same value (`fixed::noise_flat`'s tests).
            if i(2) & 0xffff == 0 {
                fixed::noise_flat(i(0), i(1), i(2))
            } else {
                fixed::noise(i(0), i(1), i(2))
            }
        }
        hid::NOISE_O => {
            let sc = &*(ptr(a[3]) as *const [i32; MAX_SCALARS]);
            let k = noise_slot(
                (a[4] as u8).wrapping_sub(code::NOISE_O) as usize % NOISE_CTX,
                0,
            );
            fixed::noise_off(
                i(0),
                i(1),
                i(2),
                [sc[k] as u32, sc[k + 1] as u32, sc[k + 2] as u32],
            )
        }
        hid::TAB => {
            let tabs = &*(ptr(a[1]) as *const [i32; MAX_TABLE]);
            tab(tabs, a[2] as u8, a[3] as u8, i(0))
        }
        hid::PICK => {
            let sc = &*(ptr(a[1]) as *const [i32; MAX_SCALARS]);
            pick(sc, a[2] as u8, a[3] as u8, i(0))
        }
        hid::SPR => {
            let tabs = &*(ptr(a[2]) as *const [i32; MAX_TABLE]);
            SprHead::new(tabs, a[3] as u8).read(tabs, i(0), i(1))
        }
        hid::GLYPH => fixed::glyph(&*(ptr(a[3]) as *const Glyphs), i(0), i(1), i(2)),
        hid::HSV => {
            let px = ptr(a[3]);
            let c = fixed::hsv(i(0), i(1), i(2));
            for (k, b) in c.into_iter().enumerate() {
                *px.add(k) = b;
            }
            0
        }
        hid::SCALE => {
            let e = ptr(a[0]);
            return pack(fixed::scale_rgb([*e, *e.add(1), *e.add(2)], i(1)));
        }
        _ => 0,
    }) as u32
}

/// A frame's state the words are filled from.
struct Rows<'a> {
    row: usize,
    ink: Option<&'a Ink>,
    under: Option<&'a [u8]>,
    prog: &'a Program,
    scratch: &'a Scratch<128>,
    alias: &'a Alias,
    tables: &'a Tables<'a>,
    own: &'a Pals,
    glyphs: &'a [Glyphs<'a>; 5],
}

/// `pal(P, v) * sc[k] >> shift` (k [`NONE`]: 1), as the VM's fused
/// colours work it out (`colours`, `pick_colours`).
fn colour_of(st: &Rows<'_>, pal: u8, v: i32, k: u8, shift: u8) -> u32 {
    let p = palette(st.tables, st.own, pal);
    let k = if k == NONE {
        fixed::ONE
    } else {
        st.scratch.sc[(k & 0x7f) as usize] >> shift
    };
    pack(fixed::scale_rgb(p[fixed::pal_index(v)], k))
}

/// What a kernel reads for a canvas or a picture it has not got.
static ZEROS: [u8; W * 3] = [0; W * 3];

/// Fill the words that change from row to row (`per_row`), or those that
/// do not (once a frame, before the rows).
fn fill<T: Target>(t: &mut T, low: &Lowered, ctx: &mut [u32], st: &Rows<'_>, per_row: bool) {
    let s = st.scratch;
    for ((w, out), &row) in low.words.iter().zip(ctx.iter_mut()).zip(&low.per_row) {
        if row != per_row {
            continue;
        }
        *out = match *w {
            Word::Lane(l) => {
                let p = match l {
                    L_X => s.xs.as_ptr(),
                    L_I => s.lanes[0].as_ptr(),
                    l => {
                        let r = ((l - L_I) as usize).min(REGS);
                        if st.alias[r].is_null() {
                            s.lanes[r].as_ptr()
                        } else {
                            st.alias[r]
                        }
                    }
                };
                t.addr(p as *const u8, W * 4)
            }
            Word::Scalar(k) => s.sc[(k & 0x7f) as usize] as u32,
            Word::Pal(k) => t.addr(palette(st.tables, st.own, k).as_ptr() as *const u8, 96),
            Word::Sin => t.addr(st.tables.sin.as_ptr() as *const u8, 512),
            Word::Helper(id) => t.helper(id),
            Word::Sc => t.addr(s.sc.as_ptr() as *const u8, MAX_SCALARS * 4),
            Word::Tabs => t.addr(st.prog.tables.as_ptr() as *const u8, MAX_TABLE * 4),
            Word::Glyphs(c) => {
                let g = &st.glyphs[(c.saturating_sub(code::DIGIT) as usize).min(4)];
                t.addr(
                    g as *const Glyphs as *const u8,
                    core::mem::size_of::<Glyphs>(),
                )
            }
            Word::Colour { pal, v, k, shift } => {
                colour_of(st, pal, s.sc[(v & 0x7f) as usize], k, shift)
            }
            Word::Entry { pal, v } => {
                let p = palette(st.tables, st.own, pal);
                t.addr(p[fixed::pal_index(s.sc[(v & 0x7f) as usize])].as_ptr(), 3)
            }
            Word::Init(c) | Word::Const(c) => c as u32,
            Word::Ink => match st.ink {
                Some(k) => t.addr(k.0[st.row * W..].as_ptr(), W),
                None => t.addr(ZEROS.as_ptr(), W),
            },
            Word::Under => match st.under {
                Some(u) => t.addr(u[st.row * W * 3..].as_ptr(), W * 3),
                None => t.addr(ZEROS.as_ptr(), W * 3),
            },
            Word::Cover(k) => s.sc[(k & 0x7f) as usize].clamp(0, fixed::ONE) as u32,
            Word::Pick {
                pal,
                b,
                n,
                i,
                k,
                shift,
            } => {
                let m = (n as usize).min(PICK_MAX);
                let v = if (i as usize) < m {
                    pick(&s.sc, b, n, i32::from(i) << 16)
                } else {
                    0
                };
                colour_of(st, pal, v, k, shift)
            }
        };
    }
}

/// Render the rows of `prog`'s frame that `rows` has a bit for, each into
/// `dest(row)` (384 bytes), with the kernel in place of the pixel steps
/// after the column and still runs: the VM's frame otherwise
/// (`vm::frame`'s order: the frame steps, the still cache's choice, then
/// per row the row steps, the column run on the first row drawn, the still
/// run or its cached lanes). The inputs must be in range (the caller draws
/// a frame that is not with the VM). The draw stage draws into `ink` on a
/// full call (`rows` all), as the VM's; `under` is what `ur ug ub ul` read
/// and a backdrop shows (`None`: black); `changed`, when given, says what
/// each row drawn changed, as `render_rows_changed` does. Lanes of a whole
/// row.
#[allow(clippy::too_many_arguments)]
pub(crate) fn frame<T: Target>(
    t: &mut T,
    low: &Lowered,
    ctx: &mut [u32],
    prog: &Program,
    scratch: &mut Scratch<128>,
    ink: Option<&mut Ink>,
    still: Option<&mut Still>,
    inputs: &Inputs,
    tables: &Tables,
    under: Option<&[u8]>,
    rows: u32,
    dest: &dyn Fn(usize) -> *mut u8,
    mut changed: Option<&mut [u32; H]>,
) {
    #[cfg(feature = "stats")]
    let mut clk = stats::Clock::start();
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
    let (fe, de, re) = (
        prog.frame_end as usize,
        prog.draw_end as usize,
        prog.row_end as usize,
    );
    if prog.forever() {
        vm::wset(&mut scratch.lanes[REGS], 0, inputs.tw);
    }
    for op in &prog.ops[..fe] {
        if code::is_wide(op.code) {
            vm::wide_step(&mut scratch.sc, &mut scratch.lanes[REGS], op);
        } else {
            vm::scalar(&mut scratch.sc, op, tables, &prog.tables);
        }
    }
    #[cfg(feature = "stats")]
    clk.lap(stats::FRAME);
    // The draw stage, as the VM runs it: on a full call only (a call for
    // some rows reads the canvas the last full one left).
    let ink: Option<&Ink> = match ink {
        Some(k) if prog.draws() => {
            if rows == u32::MAX {
                crate::draw::run(prog, scratch, tables, k, true);
            }
            Some(k)
        }
        _ => None,
    };
    if let Some(u) = under {
        assert_eq!(u.len(), W * H * 3);
    }
    let backdrop = prog.uses & BACKDROP != 0;
    enum Cache<'s> {
        Off,
        Use(&'s Still),
        Keep(&'s mut Still, usize),
    }
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
    let mut own: Pals = [[[0; 3]; 32]; USER_PALETTES];
    if prog.uses & OWNPAL != 0 {
        unpack_palettes(&prog.tables, &mut own);
    }
    let glyphs = [
        glyphs(tables, code::DIGIT),
        glyphs(tables, code::GLYPH),
        glyphs(tables, code::DIGIT_S),
        glyphs(tables, code::GLYPH_S),
        glyphs(tables, code::DIGIT_B),
    ];
    let uses_i = prog.uses & USES_I != 0;
    let main = low.main_start;
    let mut columns_kept = false;
    t.begin();
    {
        // The words the same all frame (the row stage's among them), once.
        let alias: Alias = [core::ptr::null(); REGS + 1];
        let st = Rows {
            row: 0,
            ink,
            under,
            prog,
            scratch,
            alias: &alias,
            tables,
            own: &own,
            glyphs: &glyphs,
        };
        fill(t, low, ctx, &st, false);
    }
    #[cfg(feature = "stats")]
    clk.lap(stats::DRAW);
    for row in 0..H {
        if rows & (1 << row) == 0 {
            continue;
        }
        let py = ((H - 1 - row) as i32) << 16;
        scratch.sc[S_PY as usize] = py;
        scratch.sc[S_Y as usize] = fixed::div(py, ((H - 1) as i32) << 16);
        match low.rows_at {
            Some(at) => t.run_rows(at, ctx, scratch.sc.as_mut_ptr()),
            None => {
                for op in &prog.ops[de..re] {
                    vm::scalar(&mut scratch.sc, op, tables, &prog.tables);
                }
            }
        }
        #[cfg(feature = "stats")]
        clk.lap(stats::ROWS);
        if uses_i {
            for (k, v) in scratch.lanes[0].iter_mut().enumerate() {
                *v = ((row * W + k) as i32) << 16;
            }
        }
        // The column and still runs (and what the still run reads, laid
        // out before its marker): lanes, the VM's own steps.
        let mut alias: Alias = [core::ptr::null(); REGS + 1];
        let mut k = re;
        let mut keep_at = usize::MAX;
        while k < main {
            let op = &prog.ops[k];
            match op.code {
                code::COLUMN => {
                    k += 1 + if columns_kept { op.a as usize } else { 0 };
                }
                code::STILL => match &cache {
                    Cache::Use(st) => {
                        vm::fetch::<128>(&mut alias, st, [op.b, op.c], op.d, row * W);
                        k += 1 + op.a as usize;
                    }
                    Cache::Keep(..) => {
                        keep_at = k + 1 + op.a as usize;
                        k += 1;
                    }
                    Cache::Off => k += 1,
                },
                code::FUSE => k += 1,
                _ => {
                    vm::lane_op::<128>(
                        scratch,
                        &mut alias,
                        op,
                        true,
                        0,
                        tables,
                        &prog.tables,
                        None,
                        None,
                    );
                    k += 1;
                }
            }
        }
        if keep_at == main {
            if let Cache::Keep(st, at) = &mut cache {
                let m = &prog.ops[*at];
                vm::keep::<128>(scratch, st, [m.b, m.c], m.d, row * W);
            }
        }
        #[cfg(feature = "stats")]
        clk.lap(stats::PRE);
        let st = Rows {
            row,
            ink,
            under,
            prog,
            scratch,
            alias: &alias,
            tables,
            own: &own,
            glyphs: &glyphs,
        };
        fill(t, low, ctx, &st, true);
        #[cfg(feature = "stats")]
        clk.lap(stats::FILL);
        let d = dest(row);
        // What changed (the VM's `Rows::begin`/`finish`): the row's bytes
        // kept before, compared after (every group when not word-aligned).
        let mut before = [0u32; W * 3 / 4];
        let track = changed.is_some() && (d as usize).is_multiple_of(4);
        if track {
            // SAFETY: `dest` gives W × 3 bytes, word-aligned (`track`).
            unsafe { core::ptr::copy_nonoverlapping(d.cast::<u32>(), before.as_mut_ptr(), W * 3 / 4) };
        }
        if backdrop {
            let src = under.map_or(&ZEROS[..], |u| &u[row * W * 3..(row + 1) * W * 3]);
            // SAFETY: as above: W × 3 bytes at `d`, not `src`'s.
            unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), d, W * 3) };
        }
        t.run(ctx, d);
        if let Some(ch) = changed.as_deref_mut() {
            ch[row] = if track {
                // SAFETY: the row's W × 3 bytes at `d`, word-aligned.
                unsafe { changed_groups(d.cast::<u32>(), &before) }
            } else {
                u32::MAX
            };
        }
        #[cfg(feature = "stats")]
        clk.lap(stats::KERNEL);
        columns_kept = true;
    }
    if let Cache::Keep(st, _) = cache {
        st.kept = crate::still::fingerprint(prog);
    }
}

/// Which 4-pixel groups of a row differ from `before` (bit g: pixels
/// 4g..4g + 4, three words each), a word at a time as `vm::Rows::finish`
/// does. (jit2-b: a slice compare a group cost the player's kernel frames
/// ~0.5-0.8 ms, the floor's kernel lost its trial to the VM by it.)
///
/// # Safety
/// `now` points at W × 3 / 4 readable words.
#[inline(never)]
unsafe fn changed_groups(now: *const u32, before: &[u32; W * 3 / 4]) -> u32 {
    let mut m = 0;
    let mut now = now;
    let mut was = before.as_ptr();
    for g in 0..W / 4 {
        let d = (*was ^ *now) | (*was.add(1) ^ *now.add(1)) | (*was.add(2) ^ *now.add(2));
        m |= u32::from(d != 0) << g;
        now = now.add(3);
        was = was.add(3);
    }
    m
}

/// psi-probe's split of the JIT's frames (the `stats` feature): cycles in
/// the frame steps, the row steps, the column and still runs, filling the
/// words, and the kernel, summed over the frames since the last `take`.
#[cfg(feature = "stats")]
#[doc(hidden)]
pub mod stats {
    use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
    pub const FRAME: usize = 0;
    pub const ROWS: usize = 1;
    pub const PRE: usize = 2;
    pub const FILL: usize = 3;
    pub const KERNEL: usize = 4;
    /// The draw stage, and the frame's words and the still cache's choice.
    pub const DRAW: usize = 5;
    pub const NAMES: [&str; 6] = [
        "frame steps",
        "row steps",
        "column/still runs",
        "words",
        "kernel",
        "draw stage and frame words",
    ];
    pub static CYCLES: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
    pub static FRAMES: AtomicU32 = AtomicU32::new(0);

    pub(crate) struct Clock(u32);

    fn now() -> u32 {
        let f = crate::vm::stats::CLOCK.load(Relaxed);
        if f == 0 {
            return 0;
        }
        // SAFETY: stored by the firmware from a `fn() -> u32`.
        unsafe { core::mem::transmute::<usize, fn() -> u32>(f)() }
    }

    impl Clock {
        pub(crate) fn start() -> Clock {
            FRAMES.fetch_add(1, Relaxed);
            Clock(now())
        }
        pub(crate) fn lap(&mut self, k: usize) {
            let t = now();
            CYCLES[k].fetch_add(t.wrapping_sub(self.0), Relaxed);
            self.0 = t;
        }
    }

    /// The sums so far (cycles), and the frames; reset.
    pub fn take() -> ([u32; 6], u32) {
        (
            core::array::from_fn(|k| CYCLES[k].swap(0, Relaxed)),
            FRAMES.swap(0, Relaxed),
        )
    }
}

/// The rows a kernel is checked on when it opens: the top, the middle and
/// the bottom.
pub const CHECK_ROWS: [usize; 3] = [0, 15, 31];

/// The rows' mask and each row's place in a buffer of [`CHECK_ROWS`] rows.
pub(crate) fn check_rows() -> u32 {
    CHECK_ROWS.iter().fold(0u32, |m, &r| m | 1 << r)
}

// --- The device ---------------------------------------------------------------

#[cfg(all(target_arch = "xtensa", target_os = "espidf"))]
mod device {
    use super::*;
    use alloc::vec::Vec;

    extern "C" {
        fn heap_caps_malloc(size: usize, caps: u32) -> *mut core::ffi::c_void;
        fn heap_caps_free(p: *mut core::ffi::c_void);
    }
    /// ESP-IDF's `MALLOC_CAP_EXEC`.
    const MALLOC_CAP_EXEC: u32 = 1;

    type Shim = extern "C" fn(u32, u32, u32, u32, u32) -> u32;

    /// A helper as the kernel calls it: five argument registers whatever
    /// it reads (the windowed ABI passes them in a2..a6; the ones a helper
    /// does not take are not looked at).
    extern "C" fn shim<const ID: u8>(a: u32, b: u32, c: u32, d: u32, e: u32) -> u32 {
        // SAFETY: the kernel passes what `eval_helper` expects (the
        // lowering's arguments for this helper), real addresses here.
        unsafe { eval_helper(ID, [a, b, c, d, e], &|p| p as usize as *mut u8) }
    }

    const SHIMS: [Shim; hid::COUNT as usize] = [
        shim::<0>, shim::<1>, shim::<2>, shim::<3>, shim::<4>, shim::<5>, shim::<6>, shim::<7>,
        shim::<8>, shim::<9>, shim::<10>, shim::<11>, shim::<12>, shim::<13>, shim::<14>,
        shim::<15>, shim::<16>, shim::<17>, shim::<18>,
    ];

    type KernelFn = extern "C" fn(*mut u32, *mut u8);
    type RowsFn = extern "C" fn(*mut u32, *mut i32);

    struct Device(KernelFn);

    impl Target for Device {
        fn addr(&mut self, p: *const u8, _: usize) -> u32 {
            p as usize as u32
        }
        fn helper(&mut self, id: u8) -> u32 {
            SHIMS[(id as usize).min(SHIMS.len() - 1)] as usize as u32
        }
        fn run(&mut self, ctx: &mut [u32], out: *mut u8) {
            (self.0)(ctx.as_mut_ptr(), out)
        }
        fn run_rows(&mut self, at: usize, ctx: &mut [u32], sc: *mut i32) {
            // SAFETY: `at` is the row stage's function in the block
            // (`lower`'s, 4-byte aligned): the windowed ABI, (ctx, sc).
            let f = unsafe {
                core::mem::transmute::<*const u8, RowsFn>((self.0 as *const u8).add(at))
            };
            f(ctx.as_mut_ptr(), sc)
        }
    }

    /// A formula's pixel stage as code in executable heap (the IRAM left
    /// over first, then SRAM1's D/IRAM: whatever `MALLOC_CAP_EXEC` gives),
    /// freed when dropped. For the program it was made from only.
    pub struct Kernel {
        block: *mut u32,
        code_bytes: usize,
        low: Lowered,
        ctx: Vec<u32>,
    }

    // SAFETY: the block is owned memory like a `Box`'s; nothing else
    // points into it.
    unsafe impl Send for Kernel {}

    /// The kernel for `prog`'s pixel stage, or why the VM draws it.
    pub fn emit(prog: &Program) -> Result<Kernel, Refusal> {
        let mut low = lower::lower(prog)?;
        let n = low.code.len().div_ceil(4);
        // SAFETY: a plain allocation, freed by `Drop`.
        let block = unsafe { heap_caps_malloc(n * 4, MALLOC_CAP_EXEC) } as *mut u32;
        if block.is_null() {
            return Err("no executable memory");
        }
        // Word by word: IRAM takes no byte stores.
        for (k, ch) in low.code.chunks(4).enumerate() {
            let mut b = [0u8; 4];
            b[..ch.len()].copy_from_slice(ch);
            // SAFETY: within the block (n words), word-aligned (the heap's).
            unsafe { block.add(k).write_volatile(u32::from_le_bytes(b)) };
        }
        // A data barrier (`memw`). No `isync`: internal SRAM has no
        // instruction cache (the spike ran code so copied from both regions).
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        let code_bytes = low.code.len();
        low.code = Vec::new();
        let ctx = alloc::vec![0u32; low.words.len()];
        Ok(Kernel {
            block,
            code_bytes,
            low,
            ctx,
        })
    }

    impl Drop for Kernel {
        fn drop(&mut self) {
            // SAFETY: from `heap_caps_malloc`, freed once.
            unsafe { heap_caps_free(self.block as *mut _) }
        }
    }

    impl Kernel {
        /// The code's size, bytes.
        pub fn code_bytes(&self) -> usize {
            self.code_bytes
        }

        /// The block's address (the probe).
        pub fn addr(&self) -> usize {
            self.block as usize
        }

        fn device(&self) -> Device {
            // SAFETY: the block holds a function of this signature
            // (`lower`'s prologue and epilogue: the windowed ABI).
            Device(unsafe { core::mem::transmute::<*mut u32, KernelFn>(self.block) })
        }

        /// Render the rows `rows` has a bit for of a frame of the program
        /// this kernel was made from, as [`crate::render_rows_changed`]
        /// does (`changed` given) or [`crate::render_rows_still`]: the
        /// kernel draws the pixel stage when the inputs are in range, the
        /// VM the whole frame when they are not (`false`).
        #[allow(clippy::too_many_arguments)]
        pub fn render(
            &mut self,
            prog: &Program,
            scratch: &mut Scratch<128>,
            ink: Option<&mut Ink>,
            still: Option<&mut Still>,
            inputs: &Inputs,
            tables: &Tables,
            under: Option<&[u8]>,
            rows: u32,
            out: &mut [u8],
            changed: Option<&mut [u32; H]>,
        ) -> bool {
            assert_eq!(out.len(), W * H * 3);
            if !crate::range::inputs_in_range(prog, inputs) {
                match changed {
                    Some(ch) => crate::render_rows_changed::<128>(
                        prog, scratch, ink, still, inputs, tables, under, rows, out, ch,
                    ),
                    None => crate::render_rows_still::<128>(
                        prog, scratch, ink, still, inputs, tables, under, rows, out,
                    ),
                }
                return false;
            }
            let mut d = self.device();
            let base = out.as_mut_ptr();
            // SAFETY: row < 32, within `out` (asserted above).
            let dest = |row: usize| unsafe { base.add(row * W * 3) };
            frame(
                &mut d,
                &self.low,
                &mut self.ctx,
                prog,
                scratch,
                ink,
                still,
                inputs,
                tables,
                under,
                rows,
                &dest,
                changed,
            );
            true
        }

        /// The kernel's bytes for [`CHECK_ROWS`] against `frame_bytes`, the
        /// VM's frame at `inputs` (in range: else nothing is checked,
        /// `Ok(false)`). `Err((row, x))`: the first pixel that differs.
        /// `ink` and `under` as the VM's frame had them (the canvas is
        /// read, not drawn again: the VM's full frame just drew it).
        #[allow(clippy::too_many_arguments)]
        pub fn check(
            &mut self,
            prog: &Program,
            scratch: &mut Scratch<128>,
            ink: Option<&mut Ink>,
            inputs: &Inputs,
            tables: &Tables,
            under: Option<&[u8]>,
            frame_bytes: &[u8],
        ) -> Result<bool, (usize, usize)> {
            if !crate::range::inputs_in_range(prog, inputs) {
                return Ok(false);
            }
            // (On the heap, once an open: the main task's stack is short.)
            let mut buf: Vec<u8> = Vec::new();
            if buf.try_reserve_exact(CHECK_ROWS.len() * W * 3).is_err() {
                return Ok(false);
            }
            buf.resize(CHECK_ROWS.len() * W * 3, 0);
            let base = buf.as_mut_ptr();
            let dest = |row: usize| {
                let j = CHECK_ROWS.iter().position(|&r| r == row).unwrap_or(0);
                // SAFETY: j < 3: within `buf`.
                unsafe { base.add(j * W * 3) }
            };
            let mut d = self.device();
            frame(
                &mut d,
                &self.low,
                &mut self.ctx,
                prog,
                scratch,
                ink,
                None,
                inputs,
                tables,
                under,
                check_rows(),
                &dest,
                None,
            );
            for (j, &row) in CHECK_ROWS.iter().enumerate() {
                let want = &frame_bytes[row * W * 3..(row + 1) * W * 3];
                let got = &buf[j * W * 3..(j + 1) * W * 3];
                if let Some(x) = (0..W).find(|&x| want[x * 3..x * 3 + 3] != got[x * 3..x * 3 + 3]) {
                    return Err((row, x));
                }
            }
            Ok(true)
        }
    }
}

#[cfg(all(target_arch = "xtensa", target_os = "espidf"))]
pub use device::{emit, Kernel};

/// Why the VM would draw `prog` (`None`: a kernel can), without making one
/// (the probe, tests).
pub fn refusal(prog: &Program) -> Option<Refusal> {
    lower::lower(prog).err()
}
