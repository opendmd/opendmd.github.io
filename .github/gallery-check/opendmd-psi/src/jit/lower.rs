//! The lowering: a [`Program`]'s pixel stage, after its column and still
//! runs, as one Xtensa function a row
//! (`extern "C" fn(ctx: *const u32, out: *mut u8)`): a loop over the 128
//! pixels with every Psi register in an address register, every scalar,
//! lane address, table and helper read from `ctx` (words the driver fills
//! for each row, [`Word`]), and the colour's three bytes stored at `out`.
//!
//! Each step is a template derived from the `fixed` function the VM calls
//! for it, the same operations in the same order (so the same bits); the
//! heavy steps (`len`, `atan2`, `div`, `noise`, …) call that function
//! through a shim (`callx8`, [`hid`]). Anything else — a step without a
//! template, layers, `ur ug ub ul`, `ink`, more registers than the map
//! holds — is a [`Refusal`]: the whole formula plays on the VM.
//!
//! **Registers.** a0/a1 the windowed ABI's (return, stack); a2 `ctx`, a3
//! the pixel (0..127), a4 the output; a5..a12 the homes of the Psi
//! registers the stage uses (one each, ≤ 8) and, in what is left, values
//! hoisted out of the loop (the most used scalars and lane addresses);
//! a13..a15 a step's temporaries. A `callx8` keeps a2..a7 only: the homes
//! in a8..a12 that are live across one are saved to the frame and loaded
//! back, and a kernel with calls hoists into a5..a7 only. SAR (`src`'s
//! shift, 16 for `mul_w`) is set again after a call.

use super::enc::*;
use crate::fixed;
use crate::fuse::PICK_MAX;
use crate::program::*;
use alloc::vec::Vec;

/// 1.0 in Q16.16.
const ONE_Q: i32 = 1 << 16;

/// "No slot" in a [`Word`] (a scale of 1).
pub const NONE: u8 = 0xff;

/// [`Word::Div`]'s parts: the divisor's magnitude, the multiplier, `E`,
/// the magnitude from which the quotient is held, the divisor's sign.
pub const DIV_D: u8 = 0;
pub const DIV_M: u8 = 1;
pub const DIV_E: u8 = 2;
pub const DIV_T: u8 = 3;
pub const DIV_BS: u8 = 4;

/// [`Word::Div`]'s value for a divisor `by`.
pub fn div_word(by: &fixed::DivBy, part: u8) -> u32 {
    match part {
        DIV_D => by.d,
        DIV_M => by.m,
        DIV_E => by.e(),
        DIV_T => by.t,
        _ => by.bs as u32,
    }
}

/// How the kernel divides a lane by a scalar (`div_s`).
#[derive(Clone, Copy, Debug)]
enum DivPlan {
    /// By a constant ±2^j of a dividend never below 0: a shift.
    Shift { j: u32, neg: bool },
    /// By a constant with a magic number (`fixed::DivBy::magic`: every
    /// whole number, ±2^j among them, and most fractions); `nonneg`: of a
    /// dividend never below 0 (no magnitude, no sign).
    Magic { m: u32, s: u32, neg: bool, nonneg: bool },
    /// `fixed::DivBy`'s steps, its parts in words (`Div`, or `Const` for a
    /// constant); `big`, `pos` what is known of the divisor.
    General { w: [Word; 5], big: bool, pos: bool },
}

impl DivPlan {
    /// The words its template reads, once for each read (the hoists'
    /// counts).
    fn words(&self) -> Vec<Word> {
        match *self {
            DivPlan::Shift { .. } => Vec::new(),
            DivPlan::Magic { m, .. } => alloc::vec![Word::Const(m as i32)],
            DivPlan::General { w, big, pos } => {
                let mut v = alloc::vec![w[0], w[0], w[1], w[2]];
                if !big {
                    v.push(w[3]);
                }
                if !pos {
                    v.push(w[4]);
                }
                v
            }
        }
    }
}

/// What a word of `ctx` holds for a row: the driver works each out from
/// the VM's state after the row's steps (`super::fill`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    /// A lane's address at the row's first pixel: `L_X`, `L_I`, or a
    /// register's (`L_REG0 + k`: its lane, or its still cache's alias).
    Lane(u8),
    /// Scalar slot `k`'s value.
    Scalar(u8),
    /// Palette `k`'s address (a built-in, or a formula's own unpacked).
    Pal(u8),
    /// The sine table's address.
    Sin,
    /// A helper's address ([`hid`]).
    Helper(u8),
    /// The scalar slots' address.
    Sc,
    /// The program's tables' address.
    Tabs,
    /// The address of the font a glyph step of this code reads.
    Glyphs(u8),
    /// `pal(P, sc[v]) * k` for the row, packed `r | g << 8 | b << 16`
    /// (`k` a slot, shifted right by `shift`; [`NONE`]: 1).
    Colour { pal: u8, v: u8, k: u8, shift: u8 },
    /// The address of palette `pal`'s entry for `sc[v]`.
    Entry { pal: u8, v: u8 },
    /// Part `part` of `fixed::DivBy::new(sc[v])` for a `div_s` (a lane
    /// divided by scalar `v`): [`DIV_D`], [`DIV_M`], [`DIV_E`], [`DIV_T`],
    /// [`DIV_BS`].
    Div { v: u8, part: u8 },
    /// A constant (a colour memo's start: the kernel writes it).
    Init(i32),
    /// A constant the kernel only reads (hoisted: `chan`'s).
    Const(i32),
    /// The draw stage's canvas at the row's first pixel (a row of zeros
    /// without one: `ink` reads 0).
    Ink,
    /// The picture under the formula at the row's first pixel (a row of
    /// zeros without one: `ur ug ub ul` read 0).
    Under,
    /// The whole picture under the formula, at its first pixel (`UAT`:
    /// `under(x, y)` reads anywhere in it); a row of zeros without one.
    Picture,
    /// What a `UAT` index is masked with: all ones over a picture, 0
    /// without one (then every read is the zeros' first bytes: 0, as the
    /// VM's `under_at` gives).
    PictureMask,
    /// A layer's coverage, scalar slot `k`'s value clamped to 0..1 (as
    /// `fixed::cover` clamps it).
    Cover(u8),
    /// Choice `i` of a fused `pal(P, pick(m, …)) * k`'s colours for the
    /// row (`b`, `n` the pick's), packed; `i` = [`PICK_MAX`] the colour of
    /// a pick outside its choices.
    Pick {
        pal: u8,
        b: u8,
        n: u8,
        i: u8,
        k: u8,
        shift: u8,
    },
}

/// The helpers a kernel calls (`super::helper_addr`, `super::eval_helper`).
pub mod hid {
    pub const DIV: u8 = 0;
    pub const MOD: u8 = 1;
    pub const ATAN2: u8 = 2;
    pub const HASH: u8 = 3;
    pub const LEN: u8 = 4;
    pub const SQRT: u8 = 5;
    pub const RECIP: u8 = 6;
    pub const SMOOTH: u8 = 7;
    pub const NOISE: u8 = 8;
    pub const NOISE_O: u8 = 9;
    pub const MULR: u8 = 10;
    pub const MIX: u8 = 11;
    pub const BIT: u8 = 12;
    pub const TAB: u8 = 13;
    pub const PICK: u8 = 14;
    pub const SPR: u8 = 15;
    pub const GLYPH: u8 = 16;
    pub const HSV: u8 = 17;
    /// `scale_rgb(entry, s)`, packed.
    pub const SCALE: u8 = 18;
    pub const COUNT: u8 = 19;
}

pub use crate::jitplan::Refusal;
use crate::jitplan::*;

/// A lowered pixel stage.
pub struct Lowered {
    pub code: Vec<u8>,
    pub words: Vec<Word>,
    /// Which words change from row to row (the rest are filled once a frame).
    pub per_row: Vec<bool>,
    /// The first step the kernel draws (after the column and still runs,
    /// which the driver runs as lanes).
    pub main_start: usize,
    /// It calls helpers.
    pub calls: bool,
    /// Where the row stage's function starts in `code` (4-byte aligned),
    /// if the row steps lowered ([`lower_rows`]): it shares `words`.
    pub rows_at: Option<usize>,
    /// A `rate` formula's (`layer rate N`): the kernel is its fast part (the
    /// slow layers' steps are the VM's), each row drawn over the picture
    /// under (the held slow layers), or over what the row holds without one.
    pub rate: bool,
    /// The scenes' guards, each patched on or off a row by the driver (the
    /// code as emitted has them all on).
    pub guards: Vec<GuardSite>,
}

// The register map.
const CTX: Reg = 2;
const PIX: Reg = 3;
const OUT: Reg = 4;
const POOL: [Reg; 8] = [5, 6, 7, 8, 9, 10, 11, 12];
const TEMPS: [Reg; 3] = [13, 14, 15];
/// A step wanted more temporaries than it had (the lowering tries again
/// with more).
const TEMPS_OUT: Refusal = "the kernel ran out of temporaries";
/// The helper's address goes here for `callx8` (and comes back as the
/// return address: it is the callee's a0).
const CALLEE: Reg = 8;
/// Frame slots: a8..a12's homes across a call (0..19), (20 unused),
/// a layer's `hsv` bytes (24..27), a coverage kept across a call (28),
/// and a5..a15 kept across a colour memo's cold call ([`cold_slot`]).
const SLOT_HSV: i32 = 24;
const SLOT_COVER: i32 = 28;
/// A multiple of 16 (the windowed ABI keeps a1 16-byte aligned for the
/// helpers and interrupts that run on it), and nothing of ours in its top
/// 32 B: a window overflow keeps this frame's a4..a7 at [FRAME-32,
/// FRAME-16) (it makes `call8`s) and its caller's caller's a0..a3 at
/// [FRAME-16, FRAME), possibly before the kernel even starts. (jit2-a, on
/// the emulator: an 80 B frame with a slot at 64 overwrote
/// `Kernel::check`'s saved return address.) The slots end at 76.
const FRAME: u32 = 112;
const _: () = assert!(cold_slot_end() <= FRAME as i32 - 32);

/// Where the last cold slot ends.
const fn cold_slot_end() -> i32 {
    32 + (15 - 5 + 1) * 4
}

/// Where a5..a15's value is kept across a memo's cold call.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
fn cold_slot(r: Reg) -> i32 {
    32 + (i32::from(r) - 5) * 4
}

/// Where the kernel starts: after the column run and the still cache's run
/// (with the values laid out before the still marker): the driver runs
/// those as lanes, the VM's way.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub fn main_start(prog: &Prog) -> usize {
    prog.kernel_start()
}

/// Lower `prog`'s pixel stage, or say why the VM must draw it.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub fn lower(prog: &Prog) -> Result<Lowered, Refusal> {
    let (pe, start) = (prog.len(), main_start(prog));
    if start >= pe {
        return Err("no pixel steps");
    }
    // The driver runs the steps before the kernel as lanes with nothing
    // under and no canvas: none of them may read either (they are values
    // of the position and the frame, so none does).
    if prog.ops[prog.row_end as usize..start]
        .iter()
        .any(|op| (code::U_R..=code::INKN).contains(&op.code))
    {
        return Err("an unexpected layout");
    }
    let items = items(prog, start, pe)?;
    // The fewest temporaries the steps get by with (a kernel without calls
    // hoists into the rest).
    // (Up to 7: a layer's blend wants five or six at once, an inline
    // `hsv` eight; short of them, `hsv` through its helper.)
    let mut e = Emitter::new(prog, &items, 0, false)?;
    'ways: for hsv_call in [false, true] {
        for t in 0..=7 {
            e = match Emitter::new(prog, &items, t, hsv_call) {
                Ok(e) => e,
                // A short heap is not a way that doesn't fit: said as
                // such, so the build is asked again (not a fixed refusal).
                Err(NO_MEMORY) => return Err(NO_MEMORY),
                // (An inline `hsv` keeps its values in more registers
                // than the call does: the call's way may fit.)
                Err(_) if !hsv_call => continue 'ways,
                Err(w) => return Err(w),
            };
            match e.emit() {
                Ok(()) => break 'ways,
                Err(TEMPS_OUT) if t < 7 => continue,
                Err(TEMPS_OUT) if !hsv_call => continue 'ways,
                Err(w) => return Err(w),
            }
        }
    }
    if e.c.oom {
        return Err(NO_MEMORY);
    }
    if e.c.bad {
        return Err("an operand out of the kernel's range");
    }
    if e.words.len() > 255 {
        return Err("too many values for the kernel");
    }
    let mut code = e.c.bytes;
    let mut words = e.words;
    let mut guards = e.sites;
    for g in &mut guards {
        let on = &code[g.at..g.at + g.off.len()];
        g.on = try_filled(0, on.len())?;
        g.on.copy_from_slice(on);
    }
    let mut shared = Vec::new();
    shared.try_reserve_exact(words.len()).map_err(|_| NO_MEMORY)?;
    shared.extend_from_slice(&words);
    let rows_at = match lower_rows_from(prog, shared) {
        Ok(r) => {
            let at = code.len().next_multiple_of(4);
            code.try_reserve_exact(at - code.len() + r.code.len())
                .map_err(|_| NO_MEMORY)?;
            code.resize(at, 0);
            code.extend_from_slice(&r.code);
            words = r.words;
            Some(at)
        }
        Err(NO_MEMORY) => return Err(NO_MEMORY),
        Err(_) => None,
    };
    // Slots a row writes: y, py and the row steps' values.
    let mut row_slot = [false; crate::program::SC];
    row_slot[S_Y as usize] = true;
    row_slot[S_PY as usize] = true;
    for op in &prog.ops[prog.draw_end as usize..prog.row_end as usize] {
        if code::writes_scalar(op.code) {
            row_slot[op.d as usize] = true;
        }
    }
    let rs = |k: u8| k != NONE && row_slot[k as usize];
    // The palettes `cycle` turns: turned each row (`TURN`), so a colour of
    // one is a row's.
    let turned = |pal: u8| {
        prog.ops[prog.draw_end as usize..prog.row_end as usize]
            .iter()
            .any(|o| o.code == code::TURN && 5 + o.d == pal & code::PAL_MASK)
    };
    let mut per_row = Vec::new();
    per_row.try_reserve_exact(words.len()).map_err(|_| NO_MEMORY)?;
    per_row.extend(words
        .iter()
        .map(|w| match *w {
            // (A register's lane: its still cache's alias moves each row.)
            Word::Lane(l) => l >= L_REG0,
            Word::Ink | Word::Under => true,
            Word::Scalar(k) | Word::Cover(k) | Word::Div { v: k, .. } => rs(k),
            Word::Colour { pal, v, k, .. } => rs(v) || rs(k) || turned(pal),
            Word::Entry { pal, v } => rs(v) || turned(pal),
            // (A memo starts again each row, as the VM's each lane.)
            Word::Init(_) => true,
            // (Its choice's slot, from the pick's table of slots: a row's
            // only if a row step writes it; outside the choices, 0.)
            Word::Pick {
                pal, b, n, i, k, ..
            } => {
                let m = (n as usize).min(PICK_MAX);
                let choice =
                    (i as usize) < m && rs(prog.init_at((b as usize + i as usize) as u8) as u8);
                choice || rs(k) || turned(pal)
            }
            _ => false,
        }));
    Ok(Lowered {
        code,
        words,
        per_row,
        main_start: start,
        calls: e.calls,
        rows_at,
        rate: prog.slow_marker().is_some(),
        guards,
    })
}

/// A lowered row stage: one function (`extern "C" fn(ctx: *const u32,
/// sc: *mut i32)`) running the row steps once, straight through, each
/// reading its scalar slots from `sc` and writing its own there, as
/// `vm::scalar` does (the narrowed steps as their checked forms, as
/// `program::eval` gives them); its words (helpers, tables) once a frame.
pub struct LoweredRows {
    pub code: Vec<u8>,
    pub words: Vec<Word>,
}

/// The row kernel's `sc` (a2 is `ctx`, as the pixel kernel's).
const SC: Reg = 3;

/// Lower `prog`'s row steps, or say why the VM runs them.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub fn lower_rows(prog: &Prog) -> Result<LoweredRows, Refusal> {
    lower_rows_from(prog, Vec::new())
}

/// [`lower_rows`] with `words` (the pixel kernel's) first: the two share
/// one `ctx`.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
fn lower_rows_from(prog: &Prog, words: Vec<Word>) -> Result<LoweredRows, Refusal> {
    let (de, re) = (prog.draw_end as usize, prog.row_end as usize);
    if re <= de {
        return Err("no row steps");
    }
    let mut e = Emitter::bare(prog);
    e.words = words;
    e.c.entry(FRAME);
    let mut k = de;
    while k < re {
        let op = prog.ops[k];
        k += 1;
        if op.code == code::GUARD {
            // A scene's row steps: while its `when` is 0, their slots 0.
            let end = (k + op.b as usize).min(re);
            let slot = |v: u8| i32::from(v) * 4;
            e.c.l32i(10, SC, slot(op.a));
            let on = e.c.bz_fwd(Zero::Nez, 10);
            let off = e.c.j_fwd();
            e.c.bind(on);
            for o in &prog.ops[k..end] {
                e.row_step(*o)?;
            }
            let join = e.c.j_fwd();
            e.c.bind(off);
            e.c.movi(10, 0);
            for o in &prog.ops[k..end] {
                e.c.s32i(10, SC, slot(o.d));
            }
            e.c.bind(join);
            e.sar16 = false;
            k = end;
            continue;
        }
        // (`cycle`'s turns, the section's last steps: the driver's, after
        // the function: `program::turn_rows`.)
        if op.code != code::TURN {
            e.row_step(op)?;
        }
    }
    e.c.retw_n();
    if e.c.oom {
        return Err(NO_MEMORY);
    }
    if e.c.bad {
        return Err("an operand out of the kernel's range");
    }
    if e.words.len() > 255 {
        return Err("too many values for the kernel");
    }
    Ok(LoweredRows {
        code: e.c.bytes,
        words: e.words,
    })
}

/// An operand argument of a helper call.
#[derive(Clone, Copy)]
enum Arg {
    Val(u8),
    Word(Word),
    Imm(i32),
    Out,
    /// The address of a frame slot (a1 + the offset).
    Stack(i32),
}

struct Emitter<'a> {
    prog: &'a Prog,
    items: &'a [Item],
    c: Code,
    words: Vec<Word>,
    vals: Vec<Val>,
    /// The value each Psi register holds now (during emission).
    cur: [Option<usize>; REGS],
    /// Each register's value from before the kernel, if it is read so.
    livein: [Option<usize>; REGS],
    /// The value each item writes.
    wv: Vec<Option<usize>>,
    /// Whether a value is in its register this pixel (a live-in one is
    /// loaded at its first read; one saved across a call may be dropped).
    valid: Vec<bool>,
    /// Words held in registers for the whole row.
    hoist: Vec<(Word, Reg)>,
    temps: Vec<Reg>,
    /// The values' registers (each a temporary where its values are not
    /// alive).
    pool: Vec<Reg>,
    free: Vec<Reg>,
    /// The temporaries `tmp` gave this item (only these are released).
    taken: Vec<Reg>,
    sar16: bool,
    /// Any helper call (the frame's save areas).
    calls: bool,
    /// Calls on every pixel (registers: values across them in a5..a7).
    hot: bool,
    /// `src` with SAR 16 (`mul_w`, `mul`).
    uses_src: bool,
    /// `hsv` through its helper (else inline, when the registers allow).
    hsv_call: bool,
    /// The open guard: where its run starts, its condition, and SAR's
    /// state there.
    guards: Vec<(usize, u8, bool)>,
    /// Each guard's site, as [`Lowered::guards`] has them.
    sites: Vec<GuardSite>,
}

/// A scene's guard in the kernel (P0b, the guards tested once a row): its
/// run's first bytes are its "on" form (the steps, as emitted); its "off"
/// form, written over them by the driver for a row whose `cond` is 0, sets
/// the values the run gives the steps after it to 0 and jumps past the
/// run. No test or branch a pixel either way: the driver patches a site
/// only when its condition changes from one row to the next
/// ([`super::patch_guards`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardSite {
    /// The run's first byte in the code.
    pub at: usize,
    /// The scalar slot the guard tests (a frame's or a row's value).
    pub cond: u8,
    /// The bytes there when it is on (the run's own, filled in once the
    /// code is whole) and when it is off (the zeros and the jump), the same
    /// length.
    pub on: Vec<u8>,
    pub off: Vec<u8>,
}

/// Give each value a register from `pool` (a linear scan over the items:
/// a value's register is free again after the item that reads it last),
/// those alive across a call preferring a5..a7 and the rest the others.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
fn allocate(vals: &mut [Val], n: usize, pool: &[Reg]) -> Result<(), Refusal> {
    let mut free: Vec<Reg> = pool.to_vec();
    for k in 0..n {
        for v in vals.iter_mut().filter(|v| v.start == k) {
            let pick = if v.crosses {
                free.iter()
                    .position(|&r| r <= 7)
                    .or(if free.is_empty() { None } else { Some(0) })
            } else {
                free.iter().rposition(|&r| r >= 8).or(if free.is_empty() {
                    None
                } else {
                    Some(free.len() - 1)
                })
            };
            let j = pick.ok_or("more values at once than the kernel's registers")?;
            v.gpr = free.remove(j);
        }
        for v in vals.iter().filter(|v| v.end == k) {
            free.push(v.gpr);
        }
        free.sort_unstable();
    }
    Ok(())
}

impl<'a> Emitter<'a> {
    /// The emitter with `t` temporaries (a kernel without calls: values,
    /// hoists and temporaries share a5..a15; with calls: values in a5..a12,
    /// hoists in what a5..a7 the values leave, temporaries a13..a15).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn new(
        prog: &'a Prog,
        items: &'a [Item],
        t: usize,
        hsv_call: bool,
    ) -> Result<Emitter<'a>, Refusal> {
        let calls = items.iter().any(|it| needs_call(hsv_call, it));
        let hot = items.iter().any(|it| hot_call(hsv_call, it));
        let uses_src = items
            .iter()
            .any(|it| matches!(it, Item::Step(op) if matches!(op.code, code::MUL_W | code::MUL)));
        let (mut vals, livein, wv) = values(hsv_call, items)?;
        let p = pressure(&vals, items.len());
        let all: [Reg; 11] = [5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let (temps, spare, pool): (Vec<Reg>, Vec<Reg>, Vec<Reg>) = if hot {
            if p > POOL.len() {
                return Err("more values at once than the kernel's registers");
            }
            allocate(&mut vals, items.len(), &POOL)?;
            let spare = [5, 6, 7]
                .into_iter()
                .filter(|&r| vals.iter().all(|v| v.gpr != r))
                .collect();
            // (No borrowing here: a call item writes a10.. directly.)
            (TEMPS.to_vec(), spare, Vec::new())
        } else {
            if p + t > all.len() {
                return Err("more values at once than the kernel's registers");
            }
            allocate(&mut vals, items.len(), &all[..p])?;
            (
                all[all.len() - t..].to_vec(),
                all[p..all.len() - t].to_vec(),
                all[..p].to_vec(),
            )
        };
        let valid = try_filled(false, vals.len())?;
        let mut e = Emitter {
            prog,
            items,
            c: Code::new(),
            words: Vec::new(),
            valid,
            vals,
            cur: livein,
            livein,
            wv,
            hoist: Vec::new(),
            temps,
            pool,
            free: Vec::new(),
            taken: Vec::new(),
            sar16: false,
            calls,
            hot,
            uses_src,
            hsv_call,
            guards: Vec::new(),
            sites: Vec::new(),
        };
        e.choose_hoists(&spare);
        if hot {
            // The values' registers where they are not alive are a step's
            // temporaries too (a call item takes none: it writes a10..
            // itself), but never a hoist's.
            let hoisted: Vec<Reg> = e.hoist.iter().map(|&(_, r)| r).collect();
            e.pool = POOL
                .iter()
                .copied()
                .filter(|r| !hoisted.contains(r))
                .collect();
        }
        Ok(e)
    }

    /// An emitter with no items, values or hoists (the row stage's).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn bare(prog: &'a Prog) -> Emitter<'a> {
        Emitter {
            prog,
            items: &[],
            c: Code::new(),
            words: Vec::new(),
            valid: Vec::new(),
            vals: Vec::new(),
            cur: [None; REGS],
            livein: [None; REGS],
            wv: Vec::new(),
            hoist: Vec::new(),
            temps: Vec::new(),
            pool: Vec::new(),
            free: Vec::new(),
            taken: Vec::new(),
            sar16: false,
            calls: true,
            hot: true,
            uses_src: false,
            hsv_call: true,
            guards: Vec::new(),
            sites: Vec::new(),
        }
    }

    /// A row step: its operands loaded from their slots, its template or
    /// its helper, its value stored in its slot.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn row_step(&mut self, op: Op) -> Result<(), Refusal> {
        use code::*;
        let slot = |v: u8| i32::from(v) * 4;
        // (`eval`: a narrowed step is its checked form here.)
        let code = match op.code {
            ADD_W => ADD,
            SUB_W => SUB,
            MUL_W | MUL_N => MUL,
            c => c,
        };
        let h = match code {
            DIV => Some(hid::DIV),
            MOD => Some(hid::MOD),
            ATAN2 => Some(hid::ATAN2),
            HASH => Some(hid::HASH),
            LEN => Some(hid::LEN),
            BIT => Some(hid::BIT),
            SQRT => Some(hid::SQRT),
            RECIP => Some(hid::RECIP),
            SMOOTH => Some(hid::SMOOTH),
            MIX => Some(hid::MIX),
            MULR => Some(hid::MULR),
            NOISE => Some(hid::NOISE),
            TAB => Some(hid::TAB),
            SPR => Some(hid::SPR),
            NOISE_O..=NOISE_O_LAST => Some(hid::NOISE_O),
            DIGIT..=DIGIT_B => Some(hid::GLYPH),
            _ => None,
        };
        if let Some(h) = h {
            let n = arity(code);
            let args: Vec<Arg> = match code {
                TAB => alloc::vec![
                    Arg::Val(op.a),
                    Arg::Word(Word::Tabs),
                    Arg::Imm(i32::from(op.b)),
                    Arg::Imm(i32::from(op.c)),
                    Arg::Imm(self.prog.tables.len() as i32),
                ],
                SPR => alloc::vec![
                    Arg::Val(op.a),
                    Arg::Val(op.b),
                    Arg::Word(Word::Tabs),
                    Arg::Imm(i32::from(op.c)),
                    Arg::Imm(self.prog.tables.len() as i32),
                ],
                NOISE_O..=NOISE_O_LAST => alloc::vec![
                    Arg::Val(op.a),
                    Arg::Val(op.b),
                    Arg::Val(op.c),
                    Arg::Word(Word::Sc),
                    Arg::Imm(i32::from(code)),
                ],
                DIGIT..=DIGIT_B => alloc::vec![
                    Arg::Val(op.a),
                    Arg::Val(op.b),
                    Arg::Val(op.c),
                    Arg::Word(Word::Glyphs(code)),
                ],
                _ => [op.a, op.b, op.c][..n]
                    .iter()
                    .map(|&v| Arg::Val(v))
                    .collect(),
            };
            for (j, a) in args.into_iter().enumerate() {
                let t = 10 + j as Reg;
                match a {
                    Arg::Val(v) => self.c.l32i(t, SC, slot(v)),
                    Arg::Word(w) => self.load_word(t, w),
                    Arg::Imm(v) => self.c.movi(t, v),
                    _ => return Err("an unexpected helper argument"),
                }
            }
            self.load_word(CALLEE, Word::Helper(h));
            self.c.callx8(CALLEE);
            self.sar16 = false;
            self.c.s32i(10, SC, slot(op.d));
            return Ok(());
        }
        match code {
            ADD
            | SUB
            | MUL
            | MASK
            | LT
            | LE
            | GT
            | GE
            | EQ
            | NE
            | AND
            | OR
            | MIN
            | MAX
            | STEP
            | NEG
            | NOT
            | ABS
            | FLOOR
            | FRAC
            | SIN
            | COS
            | TRI
            | SAW
            | SQR
            | SELECT
            | CLAMP
            | SEL_LT..=CNT_GE => {}
            _ => return Err("a row step the kernel has no template for"),
        }
        self.free = (4..=15).rev().filter(|&r| r != CALLEE).collect();
        self.taken.clear();
        let n = arity(code);
        let mut r = [0 as Reg; 3];
        for (j, &v) in [op.a, op.b, op.c][..n].iter().enumerate() {
            r[j] = self.tmp()?;
            self.c.l32i(r[j], SC, slot(v));
        }
        let d = self.tmp()?;
        self.template(Op { code, ..op }, d, r[0], r[1], r[2])?;
        self.c.s32i(d, SC, slot(op.d));
        Ok(())
    }

    /// Hoist the most used loop-invariant words into `spare`.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn choose_hoists(&mut self, spare: &[Reg]) {
        let mut counts: Vec<(Word, u32)> = Vec::new();
        let mut note = |w: Word| match counts.iter_mut().find(|(x, _)| *x == w) {
            Some((_, n)) => *n += 1,
            None => counts.push((w, 1)),
        };
        for it in self.items {
            if let Item::Step(op) = it {
                let (v, n) = reads(it);
                let cover = code::is_colour(op.code) && code::colour_parts(op.code).1.is_some();
                if op.code == code::DIV_S {
                    for w in self.div_plan(op).words() {
                        note(w);
                    }
                }
                for (j, &x) in v[..n].iter().enumerate() {
                    match lane_no(x) {
                        // (`div_s`'s divisor is read as its plan's words.)
                        _ if op.code == code::DIV_S && j == 1 => {}
                        // (A layer's scalar coverage is read as `Cover`.)
                        _ if !is_lane(x) && cover && j + 1 == n => note(Word::Cover(x)),
                        _ if !is_lane(x) => note(Word::Scalar(x)),
                        L_X | L_I => note(Word::Lane(lane_no(x))),
                        _ => {}
                    }
                }
                let (kind, blend) = code::colour_parts(op.code);
                let chans = match kind {
                    _ if !code::is_colour(op.code) => 0,
                    code::OUT_GRAY => 1,
                    code::OUT_RGB => 3,
                    _ => 0,
                };
                if matches!(blend, Some(fixed::BLEND_ADD | fixed::BLEND_SCREEN)) {
                    for _ in 0..3 {
                        note(Word::Const(255));
                    }
                }
                match op.code {
                    code::INK | code::INKN => note(Word::Ink),
                    code::U_R..=code::U_L => note(Word::Under),
                    code::UAT => {
                        note(Word::Picture);
                        note(Word::PictureMask);
                    }
                    _ => {}
                }
                for _ in 0..chans {
                    for c in [0, ONE_Q, 0x8000] {
                        note(Word::Const(c));
                    }
                }
                match op.code {
                    code::SIN | code::COS => {
                        note(Word::Sin);
                        note(Word::Sin);
                    }
                    c if code::is_colour(c) && kind == code::OUT_PAL && op.d & PAL_SCALED == 0 => {
                        note(Word::Pal(op.d & code::PAL_MASK));
                        note(Word::Pal(op.d & code::PAL_MASK));
                    }
                    _ => {}
                }
            }
        }
        // Live-in registers' lanes: read once a pixel each.
        for (r, id) in self.livein.iter().enumerate() {
            if id.is_some() {
                note(Word::Lane(L_REG0 + r as u8));
            }
        }
        // (An insertion sort: the list is short, and `sort_unstable`'s is
        // ~3.7 KB of flash for this one type.)
        let key = |&(w, n): &(Word, u32)| (core::cmp::Reverse(n), word_rank(w));
        for i in 1..counts.len() {
            let mut j = i;
            while j > 0 && key(&counts[j]) < key(&counts[j - 1]) {
                counts.swap(j, j - 1);
                j -= 1;
            }
        }
        for (&(w, _), &r) in counts.iter().zip(spare.iter()) {
            self.hoist.push((w, r));
        }
    }

    /// The `ctx` offset of `w` (its word added the first time).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn word(&mut self, w: Word) -> i32 {
        let k = match self.words.iter().position(|&x| x == w) {
            Some(k) => k,
            None => {
                // (Fallibly: no room is the whole kernel refused, `oom`.)
                if self.words.try_reserve(1).is_err() {
                    self.c.oom = true;
                    return 0;
                }
                self.words.push(w);
                self.words.len() - 1
            }
        };
        (k * 4) as i32
    }

    /// `n` words in a row, each new: the offset of the first.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn words_run(&mut self, ws: &[Word]) -> i32 {
        let k = self.words.len();
        if self.words.try_reserve(ws.len()).is_err() {
            self.c.oom = true;
            return 0;
        }
        self.words.extend_from_slice(ws);
        (k * 4) as i32
    }

    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn hoisted(&self, w: Word) -> Option<Reg> {
        self.hoist.iter().find(|(x, _)| *x == w).map(|&(_, r)| r)
    }

    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn tmp(&mut self) -> Result<Reg, Refusal> {
        let r = self.free.pop().ok_or(TEMPS_OUT)?;
        self.taken.push(r);
        Ok(r)
    }

    /// A memo miss writes a8 and a10..a12 itself: none of them a
    /// temporary of this item.
    /// The registers taken out, to give back once the miss is past.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn keep_call_regs(&mut self) -> Vec<Reg> {
        // (This item's only: `reset_temps` starts the next from the pool.)
        let out = self
            .free
            .iter()
            .copied()
            .filter(|&r| r == 8 || (10..=12).contains(&r))
            .collect();
        self.free.retain(|&r| r != 8 && !(10..=12).contains(&r));
        out
    }

    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn release(&mut self, r: Reg) {
        // Only what `tmp` gave this item: an operand's register may be a
        // live value's (`release` once freed one).
        if self.taken.contains(&r) && !self.free.contains(&r) {
            self.free.push(r);
        }
    }

    /// Item `k`'s temporaries: the fixed ones, and the registers of the
    /// values not alive at `k` (ended, or not begun).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn reset_temps(&mut self, k: usize) {
        let mut free: Vec<Reg> = self.temps.iter().rev().copied().collect();
        for g in self.pool.clone() {
            if !free.contains(&g)
                && self
                    .vals
                    .iter()
                    .all(|v| v.gpr != g || v.start > k || v.end < k)
            {
                free.insert(0, g);
            }
        }
        self.free = free;
        self.taken.clear();
    }

    /// `w` (a word) into `r`.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn load_word(&mut self, r: Reg, w: Word) {
        match self.hoisted(w) {
            Some(h) => self.c.mov(r, h),
            None => {
                let off = self.word(w);
                self.c.l32i(r, CTX, off);
            }
        }
    }

    /// A register holding `w` (its hoist, or `r` loaded).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn word_in(&mut self, r: Reg, w: Word) -> Reg {
        match self.hoisted(w) {
            Some(h) => h,
            None => {
                let off = self.word(w);
                self.c.l32i(r, CTX, off);
                r
            }
        }
    }

    /// The pixel's value of lane `l` (`L_X`, `L_I`, a register's) into `r`.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn lane_into(&mut self, r: Reg, l: u8) {
        let w = Word::Lane(l);
        let p = self.word_in(r, w);
        self.c.addx4(r, PIX, p);
        self.c.l32i(r, r, 0);
    }

    /// A register holding operand `v` for this pixel.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn val(&mut self, v: u8) -> Result<Reg, Refusal> {
        if !is_lane(v) {
            let w = Word::Scalar(v);
            if let Some(h) = self.hoisted(w) {
                return Ok(h);
            }
            let t = self.tmp()?;
            let off = self.word(w);
            self.c.l32i(t, CTX, off);
            return Ok(t);
        }
        match lane_no(v) {
            L_PX => {
                // px = the pixel << 16 (exactly the VM's column: N = W, base 0).
                let t = self.tmp()?;
                self.c.slli(t, PIX, 16);
                Ok(t)
            }
            L_X | L_I => {
                let t = self.tmp()?;
                self.lane_into(t, lane_no(v));
                Ok(t)
            }
            l => {
                let id = self.cur[(l - L_REG0) as usize]
                    .ok_or("a register read before it is written")?;
                let h = self.vals[id].gpr;
                if !self.valid[id] {
                    // A value from before the kernel (the column or still
                    // run, or the still cache): its lane, at this pixel.
                    self.lane_into(h, l);
                    self.valid[id] = true;
                }
                Ok(h)
            }
        }
    }

    /// Item `k`'s result: its value and register.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn dst(&self, k: usize) -> Result<(usize, Reg), Refusal> {
        let id = self.wv[k].ok_or("a step without a register")?;
        Ok((id, self.vals[id].gpr))
    }

    /// Item `k`'s result `id` is in its register now.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn wrote(&mut self, k: usize, id: usize) {
        self.valid[id] = true;
        if let Item::Step(op) = self.items[k] {
            if let Some(r) = reg_of(op.d) {
                self.cur[r] = Some(id);
            }
        }
    }

    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn sar(&mut self) {
        if !self.sar16 {
            self.c.ssai(16);
            self.sar16 = true;
        }
    }

    /// `d = (1 << 16)`.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn one(&mut self, d: Reg) {
        self.c.movi(d, 1);
        self.c.slli(d, d, 16);
    }

    /// `d = i32::MAX`.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn max_i32(&mut self, d: Reg) {
        self.c.movi(d, -1);
        self.c.srli(d, d, 1);
    }

    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn emit(&mut self) -> Result<(), Refusal> {
        let uses_src = self.uses_src;
        self.c.entry(if self.calls { FRAME } else { 32 });
        self.c.mov(OUT, PIX);
        for (w, r) in self.hoist.clone() {
            let off = self.word(w);
            self.c.l32i(r, CTX, off);
        }
        if uses_src {
            self.c.ssai(16);
        }
        self.c.movi(PIX, 0);
        let top = self.c.here();
        // At the loop's top SAR is 16 unless a call in the body changed it.
        self.sar16 = uses_src && !self.hot;
        self.cur = self.livein;
        for v in self.valid.iter_mut() {
            *v = false;
        }
        for k in 0..self.items.len() {
            self.reset_temps(k);
            let it = self.items[k];
            match it {
                Item::Step(op) if code::is_colour(op.code) => self.colour(k, op)?,
                Item::Step(op) => self.step(k, op)?,
                Item::SelPal { c, a, b, col } => self.sel_pal(k, c, a, b, col)?,
                Item::PickPal { m, pk, col } => self.pick_pal(m, pk, col)?,
                Item::Guard { cond } => self.guard(k, cond)?,
                Item::GuardEnd => self.guard_end(k),
            }
        }
        self.c.addi_n(PIX, PIX, 1);
        self.c.addi_n(OUT, OUT, 3);
        let off = top as i32 - self.c.here() as i32 - 4;
        if (-128..128).contains(&off) {
            self.c.bi(Imm::Nei, PIX, 128, top);
        } else {
            let f = self.c.bi_fwd(Imm::Eqi, PIX, 128);
            self.c.j(top);
            self.c.bind(f);
        }
        self.c.retw_n();
        Ok(())
    }

    // --- Scenes' guards ----------------------------------------------------

    /// A guard at item `k`: the values from before the kernel its run reads
    /// first and the steps after read too, loaded; then its run, which the
    /// driver turns into [`guard_end`](Self::guard_end)'s zeros and jump
    /// for the rows `cond` is 0 on (no test in the kernel).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn guard(&mut self, k: usize, cond: u8) -> Result<(), Refusal> {
        if is_lane(cond) {
            return Err("an unexpected layout");
        }
        for r in 0..REGS {
            if let Some(id) = self.livein[r] {
                if self.vals[id].start == k && !self.valid[id] {
                    let h = self.vals[id].gpr;
                    self.lane_into(h, L_REG0 + r as u8);
                    self.valid[id] = true;
                }
            }
        }
        self.guards.push((self.c.here(), cond, self.sar16));
        Ok(())
    }

    /// A guard's run ends at item `k`: its "off" form (the values it wrote
    /// that are read after it 0, the VM's mask, then a jump to here), the
    /// run padded to its length if it is shorter; SAR known only if both
    /// ways set it.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn guard_end(&mut self, k: usize) {
        let Some((at, cond, sar)) = self.guards.pop() else {
            return;
        };
        let kg = (0..k)
            .rev()
            .find(|&j| matches!(self.items[j], Item::Guard { .. }))
            .unwrap_or(0);
        let outs: Vec<Reg> = self
            .vals
            .iter()
            .filter(|v| !v.livein && v.start > kg && v.start < k && v.end > k)
            .map(|v| v.gpr)
            .collect();
        // The zeros' length, then the run padded (with moves of the pixel
        // counter to itself: a2..a15 hold nothing else to keep) to hold
        // them and the jump.
        let mut z = Code::new();
        for &g in &outs {
            z.movi(g, 0);
        }
        // (No room for the padding: the code is refused whole, `oom`.)
        while self.c.here() - at < z.here() + 3 && !self.c.oom {
            if self.c.here() - at + 2 == z.here() + 3 {
                self.c.mov_n(PIX, PIX);
            } else {
                self.c.or(PIX, PIX, PIX);
            }
        }
        let end = self.c.here();
        // The jump after the zeros lands on the run's end (its offset as
        // from where the stub sits: its first byte is the run's).
        let mut off = z;
        off.j(end - at);
        self.c.oom |= off.oom;
        // (The rest of the run's bytes stay: never reached while off.)
        let bytes = off.bytes;
        if self.sites.try_reserve(1).is_err() {
            self.c.oom = true;
            return;
        }
        self.sites.push(GuardSite {
            at,
            cond,
            on: Vec::new(),
            off: bytes,
        });
        self.sar16 &= sar;
    }

    // --- Steps -----------------------------------------------------------

    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn step(&mut self, k: usize, op: Op) -> Result<(), Refusal> {
        use code::*;
        match op.code {
            DIV => return self.call(k, hid::DIV, &[Arg::Val(op.a), Arg::Val(op.b)], Some(op.d)),
            MOD => return self.call(k, hid::MOD, &[Arg::Val(op.a), Arg::Val(op.b)], Some(op.d)),
            ATAN2 => {
                return self.call(k, hid::ATAN2, &[Arg::Val(op.a), Arg::Val(op.b)], Some(op.d))
            }
            HASH => return self.call(k, hid::HASH, &[Arg::Val(op.a), Arg::Val(op.b)], Some(op.d)),
            LEN => return self.call(k, hid::LEN, &[Arg::Val(op.a), Arg::Val(op.b)], Some(op.d)),
            BIT => return self.call(k, hid::BIT, &[Arg::Val(op.a), Arg::Val(op.b)], Some(op.d)),
            SQRT => return self.call(k, hid::SQRT, &[Arg::Val(op.a)], Some(op.d)),
            RECIP => return self.call(k, hid::RECIP, &[Arg::Val(op.a)], Some(op.d)),
            SMOOTH => {
                return self.call(
                    k,
                    hid::SMOOTH,
                    &[Arg::Val(op.a), Arg::Val(op.b), Arg::Val(op.c)],
                    Some(op.d),
                )
            }
            MIX => {
                return self.call(
                    k,
                    hid::MIX,
                    &[Arg::Val(op.a), Arg::Val(op.b), Arg::Val(op.c)],
                    Some(op.d),
                )
            }
            MULR => {
                return self.call(
                    k,
                    hid::MULR,
                    &[Arg::Val(op.a), Arg::Val(op.b), Arg::Val(op.c)],
                    Some(op.d),
                )
            }
            NOISE => {
                return self.call(
                    k,
                    hid::NOISE,
                    &[Arg::Val(op.a), Arg::Val(op.b), Arg::Val(op.c)],
                    Some(op.d),
                )
            }
            NOISE_O..=NOISE_O_LAST => {
                let args = [
                    Arg::Val(op.a),
                    Arg::Val(op.b),
                    Arg::Val(op.c),
                    Arg::Word(Word::Sc),
                    Arg::Imm(i32::from(op.code)),
                ];
                return self.call(k, hid::NOISE_O, &args, Some(op.d));
            }
            TAB => return self.table_read(k, op),
            PICK => {
                let args = [
                    Arg::Val(op.a),
                    Arg::Word(Word::Sc),
                    Arg::Imm(i32::from(op.b)),
                    Arg::Imm(i32::from(op.c)),
                ];
                return self.call(k, hid::PICK, &args, Some(op.d));
            }
            SPR => return self.sprite_read(k, op),
            DIV_S => return self.div_s(k, op),
            INK | U_R..=U_L => return self.read_pixel(k, op.code),
            UAT => return self.under_at(k, op),
            INKN => return self.ink_levels(k, op.a),
            DIGIT..=DIGIT_B => {
                let args = [
                    Arg::Val(op.a),
                    Arg::Val(op.b),
                    Arg::Val(op.c),
                    Arg::Word(Word::Glyphs(op.code)),
                ];
                return self.call(k, hid::GLYPH, &args, Some(op.d));
            }
            _ => {}
        }
        let (id, d) = self.dst(k)?;
        let n = code::arity(op.code);
        let a = self.val(op.a)?;
        let b = if n >= 2 { self.val(op.b)? } else { 0 };
        let cc = if n >= 3 { self.val(op.c)? } else { 0 };
        if [a, b, cc][..n].contains(&d) {
            return Err("a step reads its own register");
        }
        self.template(op, d, a, b, cc)?;
        self.wrote(k, id);
        Ok(())
    }

    /// `d = op(a, b, cc)` for a step with an inline template (`fixed`'s
    /// operation in its order), its operands in registers (none `d`).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn template(&mut self, op: Op, d: Reg, a: Reg, b: Reg, cc: Reg) -> Result<(), Refusal> {
        use code::*;
        let c = &mut self.c;
        match op.code {
            ADD_W => c.add_n(d, a, b),
            SUB_W => c.sub(d, a, b),
            MASK => c.and(d, a, b),
            MIN => c.min(d, a, b),
            MAX => c.max(d, a, b),
            MUL_N => {
                c.mull(d, a, b);
                c.srai(d, d, 16);
            }
            MUL_W => {
                let t = self.tmp()?;
                self.c.mull(d, a, b);
                self.c.mulsh(t, a, b);
                self.sar();
                self.c.src(d, t, d);
            }
            ADD | SUB => {
                // As `fixed::add`/`sub`: the wrapped result, unless it
                // overflowed: then the end of `a`'s sign.
                let t = self.tmp()?;
                let c = &mut self.c;
                if op.code == ADD {
                    c.add(d, a, b);
                    c.xor(t, a, b);
                    let f1 = c.bz_fwd(Zero::Ltz, t);
                    c.xor(t, a, d);
                    let f2 = c.bz_fwd(Zero::Gez, t);
                    c.srai(d, a, 31);
                    self.max_i32(t);
                    self.c.xor(d, d, t);
                    self.c.bind(f1);
                    self.c.bind(f2);
                } else {
                    c.sub(d, a, b);
                    c.xor(t, a, b);
                    let f1 = c.bz_fwd(Zero::Gez, t);
                    c.xor(t, a, d);
                    let f2 = c.bz_fwd(Zero::Gez, t);
                    c.srai(d, a, 31);
                    self.max_i32(t);
                    self.c.xor(d, d, t);
                    self.c.bind(f1);
                    self.c.bind(f2);
                }
            }
            MUL => {
                // As `fixed::mul`: the high word says whether the product
                // fits (hi in −32,768..32,767): then (p >> 16), else the end
                // of hi's sign.
                let t = self.tmp()?;
                self.c.mull(d, a, b);
                self.c.mulsh(t, a, b);
                self.release(a);
                self.release(b);
                let u = self.tmp()?;
                self.sar();
                let c = &mut self.c;
                c.srai(u, t, 15);
                c.addi(u, u, 1);
                let fit = c.bui_fwd(false, u, 2);
                c.srai(d, t, 31);
                self.max_i32(u);
                self.c.xor(d, d, u);
                let done = self.c.j_fwd();
                self.c.bind(fit);
                self.c.src(d, t, d);
                self.c.bind(done);
            }
            LT | LE | GT | GE | EQ | NE | STEP => {
                // 1 where the compare holds (a branch over the 0), then << 16.
                let (cond, x, y) = match op.code {
                    LT => (Cond::Lt, a, b),
                    LE => (Cond::Ge, b, a),
                    GT => (Cond::Lt, b, a),
                    GE => (Cond::Ge, a, b),
                    EQ => (Cond::Eq, a, b),
                    NE => (Cond::Ne, a, b),
                    _ => (Cond::Ge, b, a), // step(e, x): x >= e
                };
                c.movi(d, 1);
                let f = c.b_fwd(cond, x, y);
                c.movi(d, 0);
                c.bind(f);
                c.slli(d, d, 16);
            }
            AND => {
                c.movi(d, 0);
                let f1 = c.bz_fwd(Zero::Eqz, a);
                let f2 = c.bz_fwd(Zero::Eqz, b);
                c.movi(d, 1);
                c.bind(f1);
                c.bind(f2);
                c.slli(d, d, 16);
            }
            OR => {
                c.movi(d, 1);
                let f1 = c.bz_fwd(Zero::Nez, a);
                let f2 = c.bz_fwd(Zero::Nez, b);
                c.movi(d, 0);
                c.bind(f1);
                c.bind(f2);
                c.slli(d, d, 16);
            }
            NOT => {
                c.movi(d, 1);
                let f = c.bz_fwd(Zero::Eqz, a);
                c.movi(d, 0);
                c.bind(f);
                c.slli(d, d, 16);
            }
            NEG => {
                // `saturating_neg`: only i32::MIN overflows (and only then
                // are a and -a both negative).
                let t = self.tmp()?;
                let c = &mut self.c;
                c.neg(d, a);
                c.and(t, d, a);
                let f = c.bz_fwd(Zero::Gez, t);
                self.max_i32(d);
                self.c.bind(f);
            }
            ABS => {
                c.abs(d, a);
                let f = c.bz_fwd(Zero::Gez, d);
                self.max_i32(d);
                self.c.bind(f);
            }
            FLOOR => {
                c.srai(d, a, 16);
                c.slli(d, d, 16);
            }
            FRAC => c.extui(d, a, 0, 16),
            SHR => c.srai(d, a, u32::from(op.c & 31)),
            SIN => self.sin(d, a)?,
            COS => {
                // cos(a) = sin(a + 1/4), wrapping (only the turn's fraction is read).
                let t = self.tmp()?;
                self.c.addmi(t, a, 0x4000);
                self.release(a);
                self.sin(d, t)?;
            }
            TRI => {
                let t = self.tmp()?;
                let c = &mut self.c;
                c.extui(d, a, 0, 16);
                c.slli(d, d, 2);
                // The quarter of the turn: p < 0x4000 is 0, < 0xc000 is 1 or 2.
                c.extui(t, a, 14, 2);
                let q0 = c.bz_fwd(Zero::Eqz, t);
                let q3 = c.bi_fwd(Imm::Eqi, t, 3);
                c.movi(t, 2);
                c.slli(t, t, 16);
                c.sub(d, t, d);
                let done = c.j_fwd();
                c.bind(q3);
                c.movi(t, 1);
                c.slli(t, t, 18);
                c.sub(d, d, t);
                c.bind(q0);
                c.bind(done);
            }
            SAW => {
                c.extui(d, a, 0, 16);
                c.slli(d, d, 1);
                c.addmi(d, d, -32768);
                c.addmi(d, d, -32768);
            }
            SQR => {
                c.movi(d, 1);
                c.slli(d, d, 16);
                let f = c.bbci_fwd(a, 15);
                c.neg(d, d);
                c.bind(f);
            }
            SELECT => {
                c.mov(d, cc);
                c.movnez(d, b, a);
            }
            SEL_LT | SEL_LE => {
                // `cc` where a < b (a <= b: b >= a), else 0.
                c.mov(d, cc);
                let f = if op.code == SEL_LT {
                    c.b_fwd(Cond::Lt, a, b)
                } else {
                    c.b_fwd(Cond::Ge, b, a)
                };
                c.movi(d, 0);
                c.bind(f);
            }
            CNT_LT..=CNT_GE => {
                // 1 for each of `a op b`, `a op cc` that holds (a branch
                // over its add when it does not), then << 16.
                c.movi(d, 0);
                for y in [b, cc] {
                    let f = match op.code {
                        CNT_LT => c.b_fwd(Cond::Ge, a, y),
                        CNT_LE => c.b_fwd(Cond::Lt, y, a),
                        CNT_GT => c.b_fwd(Cond::Ge, y, a),
                        _ => c.b_fwd(Cond::Lt, a, y),
                    };
                    c.addi(d, d, 1);
                    c.bind(f);
                }
                c.slli(d, d, 16);
            }
            CLAMP => {
                c.max(d, a, b);
                c.min(d, d, cc);
            }
            _ => return Err("a step the kernel has no template for"),
        }
        Ok(())
    }

    /// `ink` (the canvas's byte at the pixel) or `ur ug ub ul` (the
    /// picture's), as 0..1: `fixed::byte` (`b·257 + b/128`), the luminance
    /// as `fixed::luma`.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn read_pixel(&mut self, k: usize, code: u8) -> Result<(), Refusal> {
        let (id, d) = self.dst(k)?;
        let t = self.tmp()?;
        if code == code::INK {
            let p = self.word_in(t, Word::Ink);
            self.c.add(t, p, PIX);
            self.c.l8ui(t, t, 0);
        } else {
            let p = self.word_in(t, Word::Under);
            let u = self.tmp()?;
            self.c.addx2(u, PIX, PIX);
            self.c.add(t, p, u);
            if code == code::U_L {
                // (77 r + 150 g + 29 b + 128) >> 8.
                let m = self.tmp()?;
                let n = self.tmp()?;
                let c = &mut self.c;
                c.l8ui(u, t, 0);
                c.movi(n, 77);
                c.mull(u, u, n);
                c.l8ui(m, t, 1);
                c.movi(n, 150);
                c.mull(m, m, n);
                c.add(u, u, m);
                c.l8ui(m, t, 2);
                c.movi(n, 29);
                c.mull(m, m, n);
                c.add(u, u, m);
                c.movi(n, 128);
                c.add(u, u, n);
                c.srli(t, u, 8);
            } else {
                self.c.l8ui(t, t, i32::from(code - code::U_R));
            }
        }
        let c = &mut self.c;
        c.slli(d, t, 8);
        c.add(d, d, t);
        c.srli(t, t, 7);
        c.add(d, d, t);
        self.wrote(k, id);
        Ok(())
    }

    /// `under(x, y)`'s channel `op.c`: the picture's byte at the pixel
    /// nearest (x, y) held to the panel, as 0..1 — `vm::under_at`'s
    /// `(fixed::add(v, ½) >> 16).clamp(..)` for each (the saturating add as
    /// `max(v + ½, v)`: where `v + ½` wraps, `v` is within ½ of the top and
    /// clamps the same), the index masked by [`Word::PictureMask`] (no
    /// picture: 0), then `fixed::byte`.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn under_at(&mut self, k: usize, op: Op) -> Result<(), Refusal> {
        let a = self.val(op.a)?;
        let b = self.val(op.b)?;
        let (id, d) = self.dst(k)?;
        if a == d || b == d {
            return Err("a step reads its own register");
        }
        let t = self.tmp()?;
        let u = self.tmp()?;
        let c = &mut self.c;
        for (r, v, top) in [(t, a, W as i32 - 1), (d, b, H as i32 - 1)] {
            c.addmi(r, v, 0x4000);
            c.addmi(r, r, 0x4000);
            c.max(r, r, v);
            c.srai(r, r, 16);
            c.movi(u, 0);
            c.max(r, r, u);
            c.movi(u, top);
            c.min(r, r, u);
        }
        self.release(a);
        self.release(b);
        let c = &mut self.c;
        c.slli(d, d, 7);
        c.add(t, t, d);
        c.addx2(t, t, t);
        let m = self.word_in(u, Word::PictureMask);
        self.c.and(t, t, m);
        let p = self.word_in(u, Word::Picture);
        let c = &mut self.c;
        c.add(t, t, p);
        c.l8ui(t, t, i32::from(op.c.min(2)));
        c.slli(d, t, 8);
        c.add(d, d, t);
        c.srli(t, t, 7);
        c.add(d, d, t);
        self.wrote(k, id);
        Ok(())
    }

    /// `T[k]` (`program::tab`), inline: floor(k) against the table's
    /// length (an unsigned compare: below 0 is huge), then the word at the
    /// store's base + 4·(4·start + i), else 0. The verifier holds the table
    /// inside the store, so the index needs no mask here. (A helper call
    /// was slower than the VM's lane loop: paint80-3e6de.)
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn table_read(&mut self, k: usize, op: Op) -> Result<(), Refusal> {
        let a = self.val(op.a)?;
        let (id, d) = self.dst(k)?;
        let i = self.tmp()?;
        self.c.srai(i, a, 16);
        self.release(a);
        let u = self.tmp()?;
        let v = self.tmp()?;
        self.c.movi(u, i32::from(op.c) + 1);
        self.c.movi(v, 0);
        let out = self.c.b_fwd(Cond::Geu, i, u);
        let base = self.word_in(u, Word::Tabs);
        self.c.addx4(i, i, base);
        let off = (crate::program::store_at(op.b) * 4) as i32;
        if off & !255 != 0 {
            self.c.addmi(i, i, off & !255);
        }
        self.c.l32i_wide(v, i, off & 255);
        self.c.bind(out);
        self.c.mov(d, v);
        self.wrote(k, id);
        Ok(())
    }

    /// `S[u, v]` (`SprHead::read`), inline, the sprite's shape known here
    /// (the store is the program's): floor(u), floor(v) against its width
    /// and height (unsigned: below 0 is outside), the row's word, the
    /// pixel's bits out of it (`ssr` and `src` by its place in the word),
    /// then its key's number; 0 outside.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn div_plan(&self, op: &Op) -> DivPlan {
        let Some(b) = self.prog.const_value(op.b, self.prog.len()) else {
            let w = |part| Word::Div { v: op.b, part };
            return DivPlan::General {
                w: [w(DIV_D), w(DIV_M), w(DIV_E), w(DIV_T), w(DIV_BS)],
                big: op.c & code::DIV_BIG != 0,
                pos: op.c & code::DIV_POS != 0,
            };
        };
        let nonneg = op.c & code::DIV_NONNEG != 0;
        if let Some((j, neg)) = fixed::DivBy::shift(b).filter(|_| nonneg) {
            return DivPlan::Shift { j, neg };
        }
        if let Some((m, s)) = fixed::DivBy::magic(b) {
            return DivPlan::Magic { m, s, neg: b < 0, nonneg };
        }
        let by = fixed::DivBy::new(b);
        let w = |part| Word::Const(div_word(&by, part) as i32);
        DivPlan::General {
            w: [w(DIV_D), w(DIV_M), w(DIV_E), w(DIV_T), w(DIV_BS)],
            big: by.t == u32::MAX,
            pos: b > 0,
        }
    }

    /// `div_s`: a lane divided by a scalar, exactly `fixed::div` (the VM's
    /// `fixed::DivBy`, or for a constant divisor a magic number's
    /// multiply).
    #[inline(never)]
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn div_s(&mut self, k: usize, op: Op) -> Result<(), Refusal> {
        let plan = self.div_plan(&op);
        let (id, d) = self.dst(k)?;
        let a = self.val(op.a)?;
        if a == d {
            return Err("a step reads its own register");
        }
        match plan {
            DivPlan::Shift { j, neg } => {
                self.c.srai(d, a, j);
                if neg {
                    self.c.neg(d, d);
                }
            }
            DivPlan::Magic { m, s, neg, nonneg: true } => {
                let w = self.tmp()?;
                let mr = self.word_in(w, Word::Const(m as i32));
                let c = &mut self.c;
                c.muluh(d, a, mr);
                match s {
                    0 => {}
                    1..=15 => c.srli(d, d, s),
                    _ => c.extui(d, d, s, 32 - s),
                }
                if neg {
                    c.neg(d, d);
                }
            }
            DivPlan::Magic { m, s, neg, .. } => {
                let sg = self.tmp()?;
                let w = self.tmp()?;
                self.c.srai(sg, a, 31);
                self.c.xor(d, a, sg);
                self.c.sub(d, d, sg);
                let mr = self.word_in(w, Word::Const(m as i32));
                let c = &mut self.c;
                c.muluh(d, d, mr);
                match s {
                    0 => {}
                    1..=15 => c.srli(d, d, s),
                    _ => c.extui(d, d, s, 32 - s),
                }
                if neg {
                    // The result's sign: a's flipped (~sg = −sg − 1).
                    c.neg(sg, sg);
                    c.addi(sg, sg, -1);
                }
                c.xor(d, d, sg);
                c.sub(d, d, sg);
            }
            DivPlan::General { w, big, pos } => {
                let [wd, wm, we, wt, wb] = w;
                let sg = self.tmp()?;
                let t = self.tmp()?;
                let q = self.tmp()?;
                let x = self.tmp()?;
                self.c.srai(sg, a, 31);
                self.c.xor(d, a, sg);
                self.c.sub(d, d, sg);
                // n = |a| in d; held to the ends from t on.
                let sat = if big {
                    None
                } else {
                    let r = self.word_in(t, wt);
                    Some(self.c.b_fwd(Cond::Geu, d, r))
                };
                let mr = self.word_in(t, wm);
                if big {
                    // E ≥ 32: the product's high word >> (E − 32).
                    self.c.muluh(q, d, mr);
                    let er = self.word_in(t, we);
                    self.c.ssr(er);
                    self.c.movi(x, 0);
                    self.c.src(q, x, q);
                } else {
                    self.c.mull(x, d, mr);
                    self.c.muluh(q, d, mr);
                    let er = self.word_in(t, we);
                    self.c.ssr(er);
                    // E < 32: (hi:lo) >> E; else hi >> (E − 32).
                    let small = self.c.bbci_fwd(er, 5);
                    self.c.movi(x, 0);
                    self.c.src(q, x, q);
                    let join = self.c.j_fwd();
                    self.c.bind(small);
                    self.c.src(q, q, x);
                    self.c.bind(join);
                }
                // The remainder n·2^16 − q·d (below 2d): one more past d.
                self.c.slli(x, d, 16);
                let dr = self.word_in(t, wd);
                self.c.mull(d, q, dr);
                self.c.sub(x, x, d);
                let near = self.c.b_fwd(Cond::Ltu, x, dr);
                self.c.addi(q, q, 1);
                self.c.bind(near);
                if !pos {
                    let br = self.word_in(t, wb);
                    self.c.xor(sg, sg, br);
                }
                self.c.xor(d, q, sg);
                self.c.sub(d, d, sg);
                if let Some(f) = sat {
                    let done = self.c.j_fwd();
                    self.c.bind(f);
                    if !pos {
                        let br = self.word_in(t, wb);
                        self.c.xor(sg, sg, br);
                    }
                    self.max_i32(d);
                    self.c.xor(d, d, sg);
                    self.c.bind(done);
                }
                // SAR back to the multiplies' 16 (hoisted out of the loop).
                self.c.ssai(16);
                self.sar16 = true;
            }
        }
        self.wrote(k, id);
        Ok(())
    }

    fn sprite_read(&mut self, k: usize, op: Op) -> Result<(), Refusal> {
        let hd = crate::program::SprHead::new(&self.prog.tables, op.c);
        let ua = self.val(op.a)?;
        let x = self.tmp()?;
        self.c.srai(x, ua, 16);
        self.release(ua);
        let va = self.val(op.b)?;
        let y = self.tmp()?;
        self.c.srai(y, va, 16);
        self.release(va);
        let (id, d) = self.dst(k)?;
        let t = self.tmp()?;
        let z = self.tmp()?;
        let c = &mut self.c;
        c.movi(z, 0);
        c.movi(t, hd.w as i32);
        let f1 = c.b_fwd(Cond::Geu, x, t);
        c.movi(t, hd.h as i32);
        let f2 = c.b_fwd(Cond::Geu, y, t);
        // The word: data + y·wpr + (x >> lg).
        if hd.wpr == 1 {
            c.mov(t, y);
        } else if hd.wpr.is_power_of_two() {
            c.slli(t, y, hd.wpr.trailing_zeros());
        } else {
            c.movi(t, hd.wpr as i32);
            c.mull(t, y, t);
        }
        c.srli(y, x, hd.lg);
        c.add(t, t, y);
        let base = self.word_in(y, Word::Tabs);
        let c = &mut self.c;
        c.addx4(t, t, base);
        let off = (hd.data * 4) as i32;
        if off & !255 != 0 {
            c.addmi(t, t, off & !255);
        }
        c.l32i_wide(t, t, off & 255);
        // The pixel's bits: (x mod the pixels a word) << lb, then its
        // key's 1 << lb bits.
        c.extui(x, x, 0, hd.lg);
        if hd.lb > 0 {
            c.slli(x, x, hd.lb);
        }
        c.ssr(x);
        c.src(t, z, t);
        // SAR back to the multiplies' 16: they hoist it out of the pixel
        // loop, so the next pixel's would read this one's.
        c.ssai(16);
        c.extui(t, t, 0, 1 << hd.lb);
        c.addx4(t, t, base);
        let off = ((hd.at + 1) * 4) as i32;
        if off & !255 != 0 {
            c.addmi(t, t, off & !255);
        }
        c.l32i_wide(z, t, off & 255);
        c.bind(f1);
        c.bind(f2);
        c.mov(d, z);
        self.wrote(k, id);
        Ok(())
    }

    /// `ink(n)`: the canvas's byte `b` at the pixel in `n` levels, as
    /// `fixed::ink_levels` (`n` the scalar operand `nv`, n << 16):
    /// u = b·n + 127, ((u + 1 + (u >> 8)) >> 8) << 16.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn ink_levels(&mut self, k: usize, nv: u8) -> Result<(), Refusal> {
        let n = self.val(nv)?;
        let (id, d) = self.dst(k)?;
        let t = self.tmp()?;
        let u = self.tmp()?;
        let p = self.word_in(t, Word::Ink);
        self.c.add(t, p, PIX);
        self.c.l8ui(t, t, 0);
        self.c.extui(u, n, 16, 8);
        self.release(n);
        let c = &mut self.c;
        c.mull(t, t, u);
        c.addi(t, t, 127);
        c.srli(u, t, 8);
        c.add(t, t, u);
        c.addi(t, t, 1);
        c.srli(t, t, 8);
        c.slli(d, t, 16);
        self.wrote(k, id);
        Ok(())
    }

    /// `d = sin(a)` as `fixed::sin`: the table at the turn's top byte,
    /// interpolated by the next.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn sin(&mut self, d: Reg, a: Reg) -> Result<(), Refusal> {
        let i = self.tmp()?;
        self.c.extui(d, a, 0, 8); // f
        self.c.extui(i, a, 8, 8); // i
        self.release(a);
        let s = self.tmp()?;
        let s = self.word_in(s, Word::Sin);
        let p = self.tmp()?;
        let c = &mut self.c;
        c.addx2(p, i, s);
        c.l16si(p, p, 0); // s0
        c.addi(i, i, 1);
        c.extui(i, i, 0, 8);
        c.addx2(i, i, s);
        c.l16si(i, i, 0); // s1
        c.sub(i, i, p);
        c.mull(i, i, d);
        c.srai(i, i, 8);
        c.add_n(d, p, i);
        c.slli(d, d, 2);
        Ok(())
    }

    // --- Helper calls --------------------------------------------------------

    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn call(&mut self, k: usize, h: u8, args: &[Arg], dst: Option<u8>) -> Result<(), Refusal> {
        if args.len() > 6 {
            return Err("a helper with too many arguments");
        }
        let dv = match dst {
            Some(_) => Some(self.dst(k)?.0),
            None => None,
        };
        // Save what a8..a12 hold that is still wanted (read here or later):
        // the values alive at this item, in their registers.
        let mut saved: Vec<usize> = Vec::new();
        for id in 0..self.vals.len() {
            let v = self.vals[id];
            if v.start > k || v.end < k || Some(id) == dv || v.gpr < 8 || !self.valid[id] {
                continue;
            }
            self.c.s32i(v.gpr, 1, slot(v.gpr));
            saved.push(id);
        }
        for (j, &a) in args.iter().enumerate() {
            let t = 10 + j as Reg;
            match a {
                Arg::Imm(v) => self.c.movi(t, v),
                Arg::Word(w) => self.load_word(t, w),
                Arg::Out => self.c.mov(t, OUT),
                Arg::Stack(off) => self.c.addi(t, 1, off),
                Arg::Val(v) if !is_lane(v) => self.load_word(t, Word::Scalar(v)),
                Arg::Val(v) => match lane_no(v) {
                    L_PX => self.c.slli(t, PIX, 16),
                    L_X | L_I => self.lane_into(t, lane_no(v)),
                    l => {
                        let id = self.cur[(l - L_REG0) as usize]
                            .ok_or("a register read before it is written")?;
                        let g = self.vals[id].gpr;
                        if !self.valid[id] {
                            self.lane_into(t, l);
                        } else if g < 8 {
                            self.c.mov(t, g);
                        } else {
                            // (Saved above: a10.. may hold an earlier argument.)
                            self.c.l32i(t, 1, slot(g));
                        }
                    }
                },
            }
        }
        self.load_word(CALLEE, Word::Helper(h));
        self.c.callx8(CALLEE);
        self.sar16 = false;
        if let Some(id) = dv {
            let g = self.vals[id].gpr;
            self.c.mov(g, 10);
            self.wrote(k, id);
        }
        for id in saved {
            let v = self.vals[id];
            if v.end > k {
                self.c.l32i(v.gpr, 1, slot(v.gpr));
            } else {
                self.valid[id] = false;
            }
        }
        // Whatever else a8..a15 held is gone.
        for id in 0..self.vals.len() {
            let v = self.vals[id];
            if v.gpr >= 8 && Some(id) != dv && !(v.start <= k && v.end > k) {
                self.valid[id] = false;
            }
        }
        Ok(())
    }

    /// A memo miss: `x` = `fixed::scale_rgb(*a10, s)` packed (a10 the
    /// palette entry's address), as `scale_rgb` takes its cases: s ≤ 0
    /// black, s = 1 the bytes, 0 < s < 1 inline, s > 1 the helper (a call:
    /// `keep`, saved by [`keep_save`](Self::keep_save) before the miss
    /// touched a10, loaded back at the end). a11, a12 scratch.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn scale_miss(&mut self, s: Reg, x: Reg, keep: &[Reg]) -> Result<(), Refusal> {
        let (e, y, b) = (10, 11, 12);
        let c = &mut self.c;
        c.movi(x, 0);
        let black1 = c.bz_fwd(Zero::Ltz, s);
        let black2 = c.bz_fwd(Zero::Eqz, s);
        c.movi(y, 1);
        c.slli(y, y, 16);
        // (Through jumps: the inline block is past a branch's reach.)
        let to_bytes = c.b_fwd(Cond::Eq, s, y);
        let to_big = c.b_fwd(Cond::Lt, y, s);
        let inline = c.j_fwd();
        c.bind(to_bytes);
        let bytes = c.j_fwd();
        c.bind(to_big);
        let big = c.j_fwd();
        c.bind(inline);
        // 0 < s < 1: chan(mul(byte(b), s)) as one 32-bit multiply a channel
        // (`scale_rgb`'s), blue first into the low byte, shifted up.
        for ch in [2, 1, 0] {
            c.l8ui(b, e, ch);
            c.slli(y, b, 8);
            c.add(y, y, b);
            c.srli(b, b, 7);
            c.add(y, y, b); // byte(b) = b·257 + b/128
            c.mull(y, y, s);
            c.extui(y, y, 16, 16);
            c.slli(b, y, 8);
            c.sub(y, b, y); // · 255
            c.addmi(y, y, 0x4000);
            c.addmi(y, y, 0x4000); // + 0x8000
            c.extui(y, y, 16, 8);
            if ch != 2 {
                c.slli(x, x, 8);
            }
            c.or(x, x, y);
        }
        let done1 = c.j_fwd();
        c.bind(bytes);
        c.l8ui(x, e, 2);
        c.slli(x, x, 8);
        c.l8ui(y, e, 1);
        c.or(x, x, y);
        c.slli(x, x, 8);
        c.l8ui(y, e, 0);
        c.or(x, x, y);
        let done2 = c.j_fwd();
        c.bind(big);
        c.mov(11, s);
        self.load_word(CALLEE, Word::Helper(hid::SCALE));
        self.c.callx8(CALLEE);
        self.sar16 = false;
        self.c.mov(x, 10);
        self.after_cold_call();
        let sar = self.sar16;
        self.c.bind(black1);
        self.c.bind(black2);
        self.c.bind(done1);
        self.c.bind(done2);
        for &r in keep {
            self.c.l32i(r, 1, cold_slot(r));
        }
        // a10..a12 were scratch on every path here: the hoists there again.
        if !self.hot {
            for (w, r) in self.hoist.clone() {
                if (10..=12).contains(&r) {
                    let off = self.word(w);
                    self.c.l32i(r, CTX, off);
                }
            }
        }
        // (SAR is 16 on the other paths when it was before; the call's
        // path set it again only for a kernel without hot calls.)
        self.sar16 = sar;
        Ok(())
    }

    /// What a memo miss must keep: a10..a12 are scratch on every path of
    /// [`scale_miss`](Self::scale_miss), a8..a15 on its call's, so the
    /// values alive at item `k` held there (and `extra`) are kept in the
    /// frame meanwhile (`scale_miss` loads them back). Before the miss
    /// writes a10.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn keep_save(&mut self, k: usize, extra: &[Reg]) -> Vec<Reg> {
        let mut keep: Vec<Reg> = extra.to_vec();
        for (id, v) in self.vals.iter().enumerate() {
            if v.start <= k && k <= v.end && v.gpr >= 8 && self.valid[id] && !keep.contains(&v.gpr)
            {
                keep.push(v.gpr);
            }
        }
        for &r in &keep {
            self.c.s32i(r, 1, cold_slot(r));
        }
        keep
    }

    /// After a memo miss's call: the hoists it clobbered (a8..a15) and SAR,
    /// as the loop's top expects them.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn after_cold_call(&mut self) {
        if self.hot {
            return;
        }
        for (w, r) in self.hoist.clone() {
            if r >= 8 {
                let off = self.word(w);
                self.c.l32i(r, CTX, off);
            }
        }
        if self.uses_src {
            self.c.ssai(16);
        }
        self.sar16 = self.uses_src;
    }

    // --- Colours ---------------------------------------------------------------

    /// Store a packed colour (`r | g << 8 | b << 16`) at the pixel.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn store_packed(&mut self, x: Reg) -> Result<(), Refusal> {
        let y = self.tmp()?;
        let c = &mut self.c;
        c.s8i(x, OUT, 0);
        c.srli(y, x, 8);
        c.s8i(y, OUT, 1);
        c.extui(y, x, 16, 8);
        c.s8i(y, OUT, 2);
        Ok(())
    }

    /// A register holding the constant `c`: its hoist, or `r` made.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn konst(&mut self, r: Reg, c: i32) -> Reg {
        if let Some(h) = self.hoisted(Word::Const(c)) {
            return h;
        }
        match c {
            ONE_Q => self.one(r),
            0x8000 => {
                self.c.movi(r, 1);
                self.c.slli(r, r, 15);
            }
            c => self.c.movi(r, c),
        }
        r
    }

    /// `fixed::chan(v)` into a temporary.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn chan(&mut self, v: Reg) -> Result<Reg, Refusal> {
        let t = self.tmp()?;
        let u = self.tmp()?;
        let z = self.konst(t, 0);
        self.c.max(t, v, z);
        self.release(v);
        let one = self.konst(u, ONE_Q);
        self.c.min(t, t, one);
        self.c.slli(u, t, 8);
        self.c.sub(t, u, t); // · 255
        let h = self.konst(u, 0x8000);
        self.c.add_n(t, t, h); // + 0x8000
        self.c.extui(t, t, 16, 8);
        self.release(u);
        Ok(t)
    }

    /// `fixed::pal_index(k)` into a temporary.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn pal_index(&mut self, k: Reg) -> Result<Reg, Refusal> {
        let t = self.tmp()?;
        self.c.movi(t, 0);
        self.c.max(t, k, t);
        self.release(k);
        let u = self.tmp()?;
        let c = &mut self.c;
        c.srai(t, t, 11);
        c.movi(u, 31);
        c.minu(t, t, u);
        self.release(u);
        Ok(t)
    }

    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn colour(&mut self, k: usize, op: Op) -> Result<(), Refusal> {
        use code::*;
        match op.code {
            OUT_GRAY => {
                let v = self.val(op.a)?;
                let g = self.chan(v)?;
                for o in 0..3 {
                    self.c.s8i(g, OUT, o);
                }
            }
            OUT_RGB => {
                for (o, x) in [op.a, op.b, op.c].into_iter().enumerate() {
                    let v = self.val(x)?;
                    let g = self.chan(v)?;
                    self.c.s8i(g, OUT, o as i32);
                    self.release(g);
                }
            }
            OUT_HSV if !self.hsv_call => {
                let rgb = self.hsv(op.a, op.b, op.c)?;
                for (o, g) in rgb.into_iter().enumerate() {
                    self.c.s8i(g, OUT, o as i32);
                }
            }
            OUT_HSV => {
                self.call(
                    k,
                    hid::HSV,
                    &[Arg::Val(op.a), Arg::Val(op.b), Arg::Val(op.c), Arg::Out],
                    None,
                )?;
            }
            OUT_PAL if op.d & PAL_SCALED == 0 => {
                let v = self.val(op.a)?;
                let i = self.pal_index(v)?;
                let p = self.tmp()?;
                let p = self.word_in(p, Word::Pal(op.d & code::PAL_MASK));
                let e = self.tmp()?;
                let c = &mut self.c;
                c.addx2(e, i, i);
                c.add(e, e, p);
                for o in 0..3 {
                    c.l8ui(i, e, o);
                    c.s8i(i, OUT, o);
                }
            }
            OUT_PAL => self.pal_lit(k, op)?,
            _ => self.layer(k, op)?,
        }
        Ok(())
    }

    /// `fixed::hsv(h, s, v)` inline: its three bytes in temporaries
    /// (red, green, blue). The same operations in its order: s and v
    /// clamped to 0..65,535, the hue's sixth, p q t as u32 products >> 16,
    /// each channel `(x·255 + 0x8000) >> 16`, chosen by the sector.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn hsv(&mut self, h: u8, s: u8, v: u8) -> Result<[Reg; 3], Refusal> {
        // 0..65,535 of a value (clamp(x, 0, 0xffff)).
        fn clamp16(e: &mut Emitter, x: Reg) -> Result<Reg, Refusal> {
            let t = e.tmp()?;
            let m = e.tmp()?;
            e.c.movi(t, 0);
            e.c.max(t, x, t);
            e.release(x);
            e.c.movi(m, -1);
            e.c.extui(m, m, 0, 16);
            e.c.min(t, t, m);
            e.release(m);
            Ok(t)
        }
        let hv = self.val(h)?;
        let x = self.tmp()?;
        self.c.extui(x, hv, 0, 16);
        self.release(hv);
        self.c.addx2(x, x, x);
        self.c.slli(x, x, 1); // h6 = hue · 6
        let sec = self.tmp()?;
        self.c.extui(sec, x, 16, 3);
        self.c.extui(x, x, 0, 16); // f
        let f = x;
        let sv = self.val(s)?;
        let s = clamp16(self, sv)?;
        let vv = self.val(v)?;
        let v = clamp16(self, vv)?;
        let w = self.tmp()?;
        // v · (0x10000 − y) >> 16 into d (y consumed into w).
        let prod = |e: &mut Emitter, d: Reg, y: Reg, w: Reg| {
            e.c.movi(w, 1);
            e.c.slli(w, w, 16);
            e.c.sub(w, w, y);
            e.c.mull(d, v, w);
            e.c.extui(d, d, 16, 16);
        };
        let p = self.tmp()?;
        prod(self, p, s, w);
        let q = self.tmp()?;
        self.c.mull(q, s, f);
        self.c.extui(q, q, 16, 16);
        prod(self, q, q, w);
        // t: s · (0x10000 − f) >> 16 first.
        self.c.movi(w, 1);
        self.c.slli(w, w, 16);
        self.c.sub(w, w, f);
        self.c.mull(w, s, w);
        self.c.extui(f, w, 16, 16);
        let t = f;
        prod(self, t, t, w);
        // Each as a byte: (x·255 + 0x8000) >> 16.
        for r in [v, p, q, t] {
            self.c.slli(w, r, 8);
            self.c.sub(r, w, r);
            self.c.addmi(r, r, 0x4000);
            self.c.addmi(r, r, 0x4000);
            self.c.extui(r, r, 16, 8);
        }
        // (r, g, b) by sector: 0 (v t p), 1 (q v p), 2 (p v t), 3 (p q v),
        // 4 (t p v), 5 (v p q).
        let (r, g, b) = (s, w, self.tmp()?);
        let ways = [
            [v, t, p],
            [q, v, p],
            [p, v, t],
            [p, q, v],
            [t, p, v],
            [v, p, q],
        ];
        let mut done = Vec::new();
        for (k, way) in ways.iter().enumerate() {
            let next = match k {
                0 => Some(self.c.bz_fwd(Zero::Nez, sec)),
                5 => None,
                k => Some(self.c.bi_fwd(Imm::Nei, sec, k as i32)),
            };
            self.c.mov(r, way[0]);
            self.c.mov(g, way[1]);
            self.c.mov(b, way[2]);
            if let Some(n) = next {
                done.push(self.c.j_fwd());
                self.c.bind(n);
            }
        }
        for d in done {
            self.c.bind(d);
        }
        for x in [sec, v, p, q, t] {
            self.release(x);
        }
        Ok([r, g, b])
    }

    /// A layer's coverage operand `v` clamped to 0..1 in a register, or
    /// `None` when it is a constant ≥ 1 (then `fixed::cover` is the blend
    /// itself).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn coverage(&mut self, v: u8) -> Result<Option<Reg>, Refusal> {
        if !is_lane(v) {
            let at = self.prog.len();
            if self.prog.const_value(v, at).is_some_and(|c| c >= ONE_Q) {
                return Ok(None);
            }
            let t = self.tmp()?;
            return Ok(Some(self.word_in(t, Word::Cover(v))));
        }
        let a = self.val(v)?;
        let t = self.tmp()?;
        self.c.movi(t, 0);
        self.c.max(t, a, t);
        self.release(a);
        let u = self.tmp()?;
        let one = self.konst(u, ONE_Q);
        self.c.min(t, t, one);
        self.release(u);
        Ok(Some(t))
    }

    /// Channel `o` of a layer's colour (`s`, a byte, left as it is)
    /// blended onto the pixel's byte by `mode` (`fixed::blend`) and moved
    /// towards it by the coverage `cov` (`fixed::cover`), stored.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn put(&mut self, o: i32, s: Reg, mode: u8, cov: Option<Reg>) -> Result<(), Refusal> {
        if mode == fixed::BLEND_OVER && cov.is_none() {
            self.c.s8i(s, OUT, o);
            return Ok(());
        }
        let d = self.tmp()?;
        self.c.l8ui(d, OUT, o);
        let r = self.tmp()?;
        match mode {
            fixed::BLEND_ADD => {
                self.c.add(r, d, s);
                let u = self.tmp()?;
                let m = self.konst(u, 255);
                self.c.minu(r, r, m);
                self.release(u);
            }
            fixed::BLEND_MULTIPLY | fixed::BLEND_SCREEN => {
                // mul255(d, s): p = d·s + 128, (p + p/256) / 256.
                let u = self.tmp()?;
                let c = &mut self.c;
                c.mull(r, d, s);
                c.addi(r, r, 127);
                c.addi(r, r, 1);
                c.srli(u, r, 8);
                c.add(r, r, u);
                c.srli(r, r, 8);
                if mode == fixed::BLEND_SCREEN {
                    // d + (s − mul255), at most 255.
                    c.sub(r, s, r);
                    c.add(r, r, d);
                    let m = self.konst(u, 255);
                    self.c.minu(r, r, m);
                }
                self.release(u);
            }
            fixed::BLEND_MAX => self.c.maxu(r, d, s),
            fixed::BLEND_MIN => self.c.minu(r, d, s),
            _ => self.c.mov(r, s),
        }
        if let Some(a) = cov {
            // d + ((r − d)·a + 0x8000) >> 16.
            let c = &mut self.c;
            c.sub(r, r, d);
            c.mull(r, r, a);
            c.addmi(r, r, 0x4000);
            c.addmi(r, r, 0x4000);
            c.srai(r, r, 16);
            c.add(r, r, d);
        }
        self.c.s8i(r, OUT, o);
        self.release(r);
        self.release(d);
        Ok(())
    }

    /// A layer: its colour worked out as the bottom layer's is, then each
    /// channel blended onto the pixel (`vm::colour_step`).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn layer(&mut self, k: usize, op: Op) -> Result<(), Refusal> {
        use code::*;
        let (kind, Some(mode)) = colour_parts(op.code) else {
            return Err("a colour the kernel has no template for");
        };
        let cv = if kind == OUT_RGB || kind == OUT_HSV {
            op.d
        } else {
            op.b
        };
        match kind {
            OUT_GRAY => {
                let v = self.val(op.a)?;
                let g = self.chan(v)?;
                let cov = self.coverage(cv)?;
                for o in 0..3 {
                    self.put(o, g, mode, cov)?;
                }
            }
            OUT_RGB => {
                let cov = self.coverage(cv)?;
                for (o, x) in [op.a, op.b, op.c].into_iter().enumerate() {
                    let v = self.val(x)?;
                    let g = self.chan(v)?;
                    self.put(o as i32, g, mode, cov)?;
                    self.release(g);
                }
            }
            OUT_HSV if !self.hsv_call => {
                let rgb = self.hsv(op.a, op.b, op.c)?;
                let cov = self.coverage(cv)?;
                for (o, g) in rgb.into_iter().enumerate() {
                    self.put(o as i32, g, mode, cov)?;
                }
            }
            OUT_HSV => {
                // The coverage first, kept in the frame across the call.
                let cov = self.coverage(cv)?;
                if let Some(a) = cov {
                    self.c.s32i(a, 1, SLOT_COVER);
                }
                self.call(
                    k,
                    hid::HSV,
                    &[
                        Arg::Val(op.a),
                        Arg::Val(op.b),
                        Arg::Val(op.c),
                        Arg::Stack(SLOT_HSV),
                    ],
                    None,
                )?;
                let cov = match cov {
                    Some(_) => {
                        let a = self.tmp()?;
                        self.c.l32i(a, 1, SLOT_COVER);
                        Some(a)
                    }
                    None => None,
                };
                for o in 0..3 {
                    let t = self.tmp()?;
                    self.c.l8ui(t, 1, SLOT_HSV + o);
                    self.put(o, t, mode, cov)?;
                    self.release(t);
                }
            }
            OUT_PAL if op.d & PAL_SCALED == 0 => {
                let v = self.val(op.a)?;
                let i = self.pal_index(v)?;
                let p = self.tmp()?;
                let p = self.word_in(p, Word::Pal(op.d & code::PAL_MASK));
                let e = self.tmp()?;
                self.c.addx2(e, i, i);
                self.c.add(e, e, p);
                self.release(i);
                self.release(p);
                let cov = self.coverage(cv)?;
                for o in 0..3 {
                    let t = self.tmp()?;
                    self.c.l8ui(t, e, o);
                    self.put(o, t, mode, cov)?;
                    self.release(t);
                }
            }
            OUT_PAL => {
                let x = self.pal_lit_x(k, op)?;
                let cov = self.coverage(cv)?;
                for o in 0..3 {
                    let t = self.tmp()?;
                    self.c.extui(t, x, 8 * o as u32, 8);
                    self.put(o, t, mode, cov)?;
                    self.release(t);
                }
            }
            _ => return Err("a colour the kernel has no template for"),
        }
        Ok(())
    }

    /// `pal(P, k) * s` with the last index and scale's colour kept in `ctx`
    /// (the VM's `pal_lit_lane`): a new one from `scale_rgb` (a call).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn pal_lit(&mut self, k: usize, op: Op) -> Result<(), Refusal> {
        let x = self.pal_lit_x(k, op)?;
        self.store_packed(x)
    }

    /// [`pal_lit`](Self::pal_lit)'s colour, packed, in a temporary.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn pal_lit_x(&mut self, k: usize, op: Op) -> Result<Reg, Refusal> {
        let shift = u32::from((op.d & !PAL_SCALED) >> PAL_SHIFT);
        let memo = self.words_run(&[Word::Init(-1), Word::Init(0), Word::Init(0)]);
        let pal = Word::Pal(op.d & code::PAL_MASK);
        let back = self.keep_call_regs();
        let v = self.val(op.a)?;
        let i = self.pal_index(v)?;
        let sv = self.val(op.c)?;
        let s = self.tmp()?;
        self.c.srai(s, sv, shift);
        self.release(sv);
        let x = self.tmp()?;
        let c = &mut self.c;
        c.l32i(x, CTX, memo);
        let m1 = c.b_fwd(Cond::Ne, i, x);
        c.l32i(x, CTX, memo + 4);
        let m2 = c.b_fwd(Cond::Ne, s, x);
        c.l32i(x, CTX, memo + 8);
        let hit = c.j_fwd();
        c.bind(m1);
        c.bind(m2);
        c.s32i(i, CTX, memo);
        c.s32i(s, CTX, memo + 4);
        let keep = self.keep_save(k, &[]);
        // (The palette into a11 first: its hoist may be a10 itself: a hoist
        // read after a10 was written made 6·i the entry.)
        self.load_word(11, pal);
        self.c.addx2(10, i, i);
        self.c.add(10, 10, 11);
        self.scale_miss(s, x, &keep)?;
        self.c.s32i(x, CTX, memo + 8);
        self.c.bind(hit);
        self.release(i);
        self.release(s);
        // (Past the miss: a8, a10..a12 are this item's again.)
        self.free.extend(back);
        Ok(x)
    }

    /// `pal(P, select(c, A, B)) [* k]`.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn sel_pal(&mut self, k: usize, cv: u8, a: u8, b: u8, col: Op) -> Result<(), Refusal> {
        let pal = col.d & code::PAL_MASK;
        let shift = (col.d & !PAL_SCALED) >> PAL_SHIFT;
        let scaled = col.d & PAL_SCALED != 0;
        if scaled && is_lane(col.c) {
            let _ = self.keep_call_regs();
        }
        if !scaled || !is_lane(col.c) {
            // Two colours a row (the VM's: two a lane).
            let k = if scaled { col.c } else { NONE };
            let ca = self.word(Word::Colour {
                pal,
                v: a,
                k,
                shift,
            });
            let cb = self.word(Word::Colour {
                pal,
                v: b,
                k,
                shift,
            });
            let c = self.val(cv)?;
            let x = self.tmp()?;
            let y = self.tmp()?;
            self.c.l32i(x, CTX, cb);
            self.c.l32i(y, CTX, ca);
            self.c.movnez(x, y, c);
            self.release(y);
            self.release(c);
            return self.store_packed(x);
        }
        // k a lane: each choice's last scale and colour kept (the VM's
        // `pal_select_lanes`). Slot w (0: c ≠ 0, A; 1: c = 0, B):
        // [last scale, colour, entry, -].
        let base = self.words_run(&[
            Word::Init(i32::MIN),
            Word::Init(0),
            Word::Entry { pal, v: a },
            Word::Init(0),
            Word::Init(i32::MIN),
            Word::Init(0),
            Word::Entry { pal, v: b },
            Word::Init(0),
        ]);
        let c = self.val(cv)?;
        let o = self.tmp()?;
        let z = self.tmp()?;
        self.c.movi(o, 0);
        self.c.movi(z, 16);
        self.c.moveqz(o, z, c);
        self.release(z);
        self.release(c);
        self.c.add(o, o, CTX);
        let kv = self.val(col.c)?;
        let s = self.tmp()?;
        self.c.srai(s, kv, u32::from(shift));
        self.release(kv);
        let x = self.tmp()?;
        let cc = &mut self.c;
        cc.l32i(x, o, base);
        let miss = cc.b_fwd(Cond::Ne, s, x);
        cc.l32i(x, o, base + 4);
        let hit = cc.j_fwd();
        cc.bind(miss);
        cc.s32i(s, o, base);
        let keep = self.keep_save(k, &[o]);
        self.c.l32i(10, o, base + 8);
        self.scale_miss(s, x, &keep)?;
        self.c.s32i(x, o, base + 4);
        self.c.bind(hit);
        self.release(o);
        self.release(s);
        self.store_packed(x)
    }

    /// `pal(P, pick(m, …)) [* k]`: the choices' colours a row.
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn pick_pal(&mut self, m: u8, pk: Op, col: Op) -> Result<(), Refusal> {
        let pal = col.d & code::PAL_MASK;
        let shift = (col.d & !PAL_SCALED) >> PAL_SHIFT;
        let k = if col.d & PAL_SCALED != 0 { col.c } else { NONE };
        let n = (pk.c as usize).min(PICK_MAX);
        let ws: Vec<Word> = (0..=PICK_MAX as u8)
            .map(|i| Word::Pick {
                pal,
                b: pk.b,
                n: pk.c,
                i,
                k,
                shift,
            })
            .collect();
        let tbl = self.words_run(&ws);
        let mv = self.val(m)?;
        let i = self.tmp()?;
        self.c.srai(i, mv, 16);
        self.release(mv);
        let u = self.tmp()?;
        self.c.movi(u, n as i32);
        let ok = self.c.b_fwd(Cond::Ltu, i, u);
        self.c.movi(i, PICK_MAX as i32);
        self.c.bind(ok);
        self.release(u);
        let x = self.tmp()?;
        self.c.addx4(x, i, CTX);
        self.c.l32i(x, x, tbl);
        self.release(i);
        self.store_packed(x)
    }
}

/// The frame slot of a8..a12's home `h`.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
fn slot(h: Reg) -> i32 {
    (i32::from(h) - 8) * 4
}

/// A fixed order among hoist candidates used as often, so the unstable
/// sort gives a formula the same kernel every time.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
fn word_rank(w: Word) -> u32 {
    match w {
        Word::Lane(l) => 0x100 | u32::from(l),
        Word::Scalar(k) => 0x200 | u32::from(k),
        Word::Sin => 0x300,
        Word::Pal(k) => 0x400 | u32::from(k),
        Word::Const(c) => 0x500 | (c as u32 >> 12 & 0xff) | (c as u32 & 0xfff) << 8,
        Word::Ink => 0x600,
        Word::Under => 0x601,
        Word::Picture => 0x602,
        Word::PictureMask => 0x603,
        Word::Cover(k) => 0x700 | u32::from(k),
        Word::Div { v, part } => 0x1_0000 | u32::from(part) << 8 | u32::from(v),
        _ => 0xfff,
    }
}
