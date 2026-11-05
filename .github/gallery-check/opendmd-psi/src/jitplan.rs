//! What the JIT's lowering (`jit::lower`) takes as given before it emits a
//! byte: the kernel's items, the steps it has templates or helpers for,
//! its values' intervals and the most alive at once. Compiled everywhere
//! (no encoder): [`jit_takes`] is the editor's "does the panel's JIT take
//! it" (P0b-2), the same rules the lowering refuses by.

// (Without the lowering — the player off the ESP32, the web app — only
// what `jit_takes` reads is used.)
#![cfg_attr(
    not(any(
        test,
        feature = "docs",
        all(feature = "jit", target_arch = "xtensa", target_os = "espidf")
    )),
    allow(dead_code)
)]

use crate::fuse::{F_PICKPAL, F_SELPAL, F_SELPAL_K};
use crate::program::*;
use alloc::vec::Vec;

/// Why a formula plays on the VM: shown in `/api/status` (`formula.jit`).
pub type Refusal = &'static str;

/// One unit of the kernel: a step, or a fused colour the kernel draws in
/// one piece (as the VM's fused runs, `crate::fuse`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Item {
    Step(Op),
    /// `pal(P, select(c, A, B)) [* k]`: c a lane, A and B scalars.
    SelPal {
        c: u8,
        a: u8,
        b: u8,
        col: Op,
    },
    /// `pal(P, pick(m, …)) [* k]`: m a lane, k a scalar or none.
    PickPal {
        m: u8,
        pk: Op,
        col: Op,
    },
    /// A scene's guard (`code::GUARD`): the items up to its `GuardEnd` run
    /// while scalar `cond` is not 0; else the values they write that are
    /// read after are 0.
    Guard {
        cond: u8,
    },
    GuardEnd,
}

/// The Psi register (0-based) an operand names, if it is one.
#[inline]
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn reg_of(v: u8) -> Option<usize> {
    let l = lane_no(v);
    (is_lane(v) && l >= L_REG0).then(|| (l - L_REG0) as usize)
}

#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn reads(it: &Item) -> ([u8; 4], usize) {
    match *it {
        Item::Step(op) => code::operands(&op),
        Item::SelPal { c, col, .. } => {
            if col.d & PAL_SCALED != 0 {
                ([c, col.c, 0, 0], 2)
            } else {
                ([c, 0, 0, 0], 1)
            }
        }
        Item::PickPal { m, .. } => ([m, 0, 0, 0], 1),
        Item::Guard { cond } => ([cond, 0, 0, 0], 1),
        Item::GuardEnd => ([0; 4], 0),
    }
}

#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn writes(it: &Item) -> Option<usize> {
    match *it {
        Item::Step(op) if !code::is_colour(op.code) => reg_of(op.d),
        _ => None,
    }
}

/// Is `code` a step the kernel has (a template or a helper)?
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn supported(c: u8) -> Result<(), Refusal> {
    use code::*;
    match c {
        ADD | SUB | MUL | ADD_W | SUB_W | MUL_W | MUL_N | MASK | LT | LE | GT | GE | EQ | NE
        | AND | OR | MIN | MAX | STEP | NEG | NOT | ABS | FLOOR | FRAC | SIN | COS | TRI | SAW
        | SQR | SHR | SELECT | CLAMP | SEL_LT..=CNT_GE | DIV_S => Ok(()),
        DIV | MOD | ATAN2 | HASH | LEN | BIT | SQRT | RECIP | PICK | TAB | SPR | MIX | SMOOTH
        | NOISE | MULR => Ok(()),
        DIGIT..=DIGIT_B | NOISE_O..=NOISE_O_LAST => Ok(()),
        OUT_RGB | OUT_HSV | OUT_PAL | OUT_GRAY => Ok(()),
        U_R..=U_L | INK | INKN | UAT => Ok(()),
        c if (OUT_LAYER..OUT_LAYER + 4 * crate::fixed::BLENDS as u8).contains(&c) => Ok(()),
        STILL | COLUMN => Err("an unexpected layout"),
        _ => Err("a step the kernel has no template for"),
    }
}

/// The kernel's units, from `ops[start..pe]`.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn items(prog: &Prog, start: usize, pe: usize) -> Result<Vec<Item>, Refusal> {
    let ops = &prog.ops[..pe];
    let mut out = Vec::new();
    let mut k = start;
    // Where the open guard's run ends.
    let mut guard_end = usize::MAX;
    while k < pe {
        if k == guard_end {
            out.push(Item::GuardEnd);
            guard_end = usize::MAX;
        }
        let op = ops[k];
        if op.code == code::GUARD {
            if guard_end != usize::MAX {
                return Err("an unexpected layout");
            }
            guard_end = k + 1 + op.b as usize;
            out.push(Item::Guard { cond: op.a });
            k += 1;
            continue;
        }
        if op.code == code::SLOW {
            // The slow layers: the VM's (`crate::rate`).
            k += 1 + op.a as usize;
            continue;
        }
        if op.code == code::FUSE {
            let n = op.a as usize;
            let run = &ops[(k + 1).min(pe)..(k + 1 + n).min(pe)];
            if let Some(it) = fused(op.d, run) {
                out.push(it);
                k += 1 + n;
            } else {
                // The steps one at a time: the same values.
                k += 1;
            }
            continue;
        }
        supported(op.code)?;
        out.push(Item::Step(op));
        k += 1;
    }
    if guard_end != usize::MAX {
        return Err("an unexpected layout");
    }
    match out.last() {
        Some(Item::Step(op)) if code::is_colour(op.code) => {}
        Some(Item::SelPal { .. } | Item::PickPal { .. }) => {}
        _ => return Err("an unexpected layout"),
    }
    Ok(out)
}

/// A fused colour the kernel draws in one piece, when its operands are of
/// the kinds the VM's kernel takes too (else `None`: the steps).
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn fused(kind: u8, run: &[Op]) -> Option<Item> {
    match (kind, run) {
        (F_SELPAL | F_SELPAL_K, [sel, col])
            if sel.code == code::SELECT
                && col.code == code::OUT_PAL
                && is_lane(sel.a)
                && !is_lane(sel.b)
                && !is_lane(sel.c)
                && col.a == sel.d =>
        {
            Some(Item::SelPal {
                c: sel.a,
                a: sel.b,
                b: sel.c,
                col: *col,
            })
        }
        (F_PICKPAL, [pk, col])
            if pk.code == code::PICK
                && col.code == code::OUT_PAL
                && is_lane(pk.a)
                && col.a == pk.d
                && (col.d & PAL_SCALED == 0 || !is_lane(col.c)) =>
        {
            Some(Item::PickPal {
                m: pk.a,
                pk: *pk,
                col: *col,
            })
        }
        _ => None,
    }
}

/// A value a Psi register holds in the kernel: written by an item (or, for
/// a register read before any item writes it, its lane from before the
/// kernel: `livein`), alive until its last read; an address register for
/// that span.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Val {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) livein: bool,
    /// Alive across a helper call (then it wants a5..a7, which a call
    /// keeps).
    pub(crate) crosses: bool,
    pub(crate) gpr: u8,
}


/// The values of `items` (each with its interval) and which value each
/// register reads first, and each item writes.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn values(hsv_call: bool, items: &[Item]) -> (Vec<Val>, [Option<usize>; REGS], Vec<Option<usize>>) {
    let mut vals: Vec<Val> = Vec::new();
    let mut cur: [Option<usize>; REGS] = [None; REGS];
    let mut livein: [Option<usize>; REGS] = [None; REGS];
    let mut wv = alloc::vec![None; items.len()];
    for (k, it) in items.iter().enumerate() {
        let (v, n) = reads(it);
        for &x in &v[..n] {
            let Some(r) = reg_of(x) else { continue };
            match cur[r] {
                Some(id) => vals[id].end = k,
                None => {
                    vals.push(Val {
                        start: k,
                        end: k,
                        livein: true,
                        crosses: false,
                        gpr: 0,
                    });
                    let id = vals.len() - 1;
                    cur[r] = Some(id);
                    livein[r] = Some(id);
                }
            }
        }
        if let Some(w) = writes(it) {
            vals.push(Val {
                start: k,
                end: k,
                livein: false,
                crosses: false,
                gpr: 0,
            });
            cur[w] = Some(vals.len() - 1);
            wv[k] = cur[w];
        }
    }
    // A value from before the kernel read first inside a guard's run and
    // again after it is loaded at the guard (both ways on have it).
    let mut g = None;
    for (k, it) in items.iter().enumerate() {
        match it {
            Item::Guard { .. } => g = Some(k),
            Item::GuardEnd => {
                if let Some(kg) = g.take() {
                    for v in vals.iter_mut() {
                        if v.livein && v.start > kg && v.start < k && v.end > k {
                            v.start = kg;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for (k, it) in items.iter().enumerate() {
        if hot_call(hsv_call, it) {
            for v in vals.iter_mut() {
                // (Its own result, and operands read for the last time, need not survive.)
                if v.start <= k && v.end > k && !(v.start == k && !v.livein) {
                    v.crosses = true;
                }
            }
        }
    }
    (vals, livein, wv)
}

/// The most values alive at once.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn pressure(vals: &[Val], n: usize) -> usize {
    (0..n)
        .map(|k| vals.iter().filter(|v| v.start <= k && k <= v.end).count())
        .max()
        .unwrap_or(0)
}

/// Whether an item calls a helper on every pixel (a colour's memo misses
/// call one now and then: "cold", after which the hoists are loaded again;
/// they do not make the kernel one with calls for its registers).
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn hot_call(hsv_call: bool, it: &Item) -> bool {
    needs_call(hsv_call, it)
        && !matches!(*it, Item::Step(op)
            if code::is_colour(op.code) && code::colour_parts(op.code).0 == code::OUT_PAL)
        && !matches!(*it, Item::SelPal { .. })
}

/// Whether an item calls a helper.
#[cfg_attr(target_arch = "xtensa", optimize(size))]
pub(crate) fn needs_call(hsv_call: bool, it: &Item) -> bool {
    use code::*;
    match *it {
        Item::Step(op) => {
            matches!(
                op.code,
                DIV | MOD
                    | ATAN2
                    | HASH
                    | LEN
                    | BIT
                    | SQRT
                    | RECIP
                    | PICK
                    | MIX
                    | SMOOTH
                    | NOISE
                    | MULR
            ) || (DIGIT..=DIGIT_B).contains(&op.code)
                || (NOISE_O..=NOISE_O_LAST).contains(&op.code)
                || (hsv_call && is_colour(op.code) && colour_parts(op.code).0 == OUT_HSV)
                || (is_colour(op.code)
                    && colour_parts(op.code).0 == OUT_PAL
                    && op.d & PAL_SCALED != 0)
        }
        Item::SelPal { col, .. } => col.d & PAL_SCALED != 0 && is_lane(col.c),
        Item::PickPal { .. } | Item::Guard { .. } | Item::GuardEnd => false,
    }
}


/// Whether the panel's JIT takes `prog` (`jit::lower` accepts it), by the
/// rules the lowering refuses by before it emits: a pixel stage after the
/// runs, steps it has templates or helpers for, and its values fitting the
/// kernel's registers (eight across a helper called every pixel, else
/// eleven), with `hsv` inline or through its helper. (What it may still
/// refuse while it emits — temporaries, an operand's range, 255 words —
/// the host tests check never happens for the bundled Formulas, the
/// goldens and the fuzz: `jit_takes_is_lower`.)
pub(crate) fn jit_takes(prog: &Prog) -> bool {
    let (pe, start) = (prog.len(), prog.kernel_start());
    if start >= pe
        || prog.ops[prog.row_end as usize..start]
            .iter()
            .any(|op| (code::U_R..=code::INKN).contains(&op.code))
    {
        return false;
    }
    let Ok(items) = items(prog, start, pe) else {
        return false;
    };
    [false, true].into_iter().any(|hsv_call| {
        let (vals, _, _) = values(hsv_call, &items);
        let p = pressure(&vals, items.len());
        let hot = items.iter().any(|it| hot_call(hsv_call, it));
        p <= if hot { 8 } else { 11 }
    })
}
