//! A compiled formula: a fixed-size value (no pointers, no heap), so it can
//! sit inline in an effect, be copied, or be built in place in a zeroed
//! allocation ([`Program::zeroed_in_place`]).

use crate::fixed::{self, ONE};
use crate::tables::{Glyphs, Tables};

/// The font a glyph step (`code::DIGIT..=DIGIT_B`) reads.
#[inline]
pub(crate) fn glyphs<'a>(t: &Tables<'a>, c: u8) -> Glyphs<'a> {
    use crate::tables::{BOLD, NORMAL, SMALL};
    let font = match c {
        code::DIGIT | code::GLYPH => NORMAL,
        code::DIGIT_S | code::GLYPH_S => SMALL,
        _ => BOLD,
    };
    t.glyphs(font, c != code::GLYPH && c != code::GLYPH_S)
}

/// The panel the VM draws: 128 × 32.
pub const W: usize = 128;
pub const H: usize = 32;

/// What one frame of a program runs (`crate::scene_rows`): each scene's
/// rows on (`rows[s]` for scene `s`), and how the draw stage's `if`s
/// outside loops go (bit k of `taken` for the k-th, known where bit k of
/// `known` is set). [`Program::tier_us_frame`] costs the frame by it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneFrame {
    pub rows: [u8; 16],
    pub known: u32,
    pub taken: u32,
}

/// Steps a formula may take in all (per-frame, per-row, per-pixel and the
/// colour): the cap the compiler holds every formula to.
pub const MAX_STEPS: usize = 768;
/// Steps a program may hold: the formula's [`MAX_STEPS`] and the markers
/// the compiler adds for the column run and the still cache, which do not
/// count against the cap (a formula at the cap keeps its column run).
pub const MAX_OPS: usize = MAX_STEPS + 2;
/// Scalar slots: the inputs, the constants, and every per-frame and per-row
/// value. An operand byte below [`LANE`] is one of them; the 15 lane
/// operands (`x`, `px`, `i` and the [`REGS`] registers) take the top.
pub const MAX_SCALARS: usize = 256 - 3 - REGS;
/// Lane registers (working values a pixel step reads and writes). Each is
/// `N` × 4 B of [`crate::Scratch`].
pub const REGS: usize = 12;

// Fixed scalar slots: the inputs.
pub(crate) const S_T: u8 = 0;
pub(crate) const S_HOUR: u8 = 1;
pub(crate) const S_MINUTE: u8 = 2;
pub(crate) const S_SECOND: u8 = 3;
pub(crate) const S_A: u8 = 4;
pub(crate) const S_B: u8 = 5;
pub(crate) const S_C: u8 = 6;
pub(crate) const S_Y: u8 = 7;
pub(crate) const S_PY: u8 = 8;
pub(crate) const S_FIRST_FREE: u8 = 9;
/// `seed` (a `loop forever` formula's, 0..1, new each time it plays): the
/// first free slot, taken only by a formula that reads it ([`SEEDED`]).
pub(crate) const S_SEED: u8 = S_FIRST_FREE;

/// Wide slots (Q48.16 values once a frame: `t` in a `loop forever` formula
/// and what is made from it), in [`crate::Scratch`]; slot 0 is `t`.
pub const WIDE: usize = 16;
/// Noise contexts (`code::NOISE_O`..): the lattice offsets of up to four
/// different noises of values that grow without end, three axes each, in
/// scalar slots of their own ([`noise_slot`]).
pub const NOISE_CTX: usize = 4;

/// The scalar slot of noise context `k`'s offset on `axis`: just after
/// `seed`'s ([`S_SEED`], kept for it in a formula with wide noises), three
/// a context, 0 where an axis has no wide part (its constant 0).
#[inline(always)]
pub(crate) const fn noise_slot(k: usize, axis: usize) -> usize {
    S_SEED as usize + 1 + 3 * k + axis
}

/// An operand is a scalar slot (0..LANE) or a lane, `LANE + k`: k = 0 `x`,
/// 1 `px`, 2 `i`, 3.. the registers ([`lane`], [`lane_no`]).
pub(crate) const LANE: u8 = MAX_SCALARS as u8;
const _: () = assert!(LANE as usize + 3 + REGS == 256);

/// Whether operand `v` is a lane (else a scalar slot).
#[inline(always)]
pub(crate) const fn is_lane(v: u8) -> bool {
    v >= LANE
}

/// Lane operand `v`'s lane: 0 `x`, 1 `px`, 2 `i`, 3.. the registers.
#[inline(always)]
pub(crate) const fn lane_no(v: u8) -> u8 {
    v.wrapping_sub(LANE)
}

/// The operand of lane `k` (`L_X`, `L_PX`, `L_I`, `L_REG0 + r`).
#[inline(always)]
pub(crate) const fn lane(k: u8) -> u8 {
    LANE + k
}

/// The scalar slots as the VM holds them: one for every operand byte, so
/// any byte indexes them without a check (slots from [`MAX_SCALARS`] up
/// are never written).
pub(crate) const SC: usize = 256;
pub(crate) type Sc = [i32; SC];
pub(crate) const L_X: u8 = 0;
pub(crate) const L_PX: u8 = 1;
pub(crate) const L_I: u8 = 2;
pub(crate) const L_REG0: u8 = 3;

/// Opcodes. Binary, unary and ternary arithmetic run per frame or per row
/// on scalars, or per pixel on lanes (each operand a lane or a scalar
/// broadcast); `U_*` load what is under the formula into a lane; the
/// colour steps (`OUT_*`, one per layer) write the pixels.
pub(crate) use code::{PAL_SCALED, PAL_SHIFT};

pub(crate) mod code {
    // binary
    pub const ADD: u8 = 0;
    pub const SUB: u8 = 1;
    pub const MUL: u8 = 2;
    pub const DIV: u8 = 3;
    pub const MOD: u8 = 4;
    pub const MASK: u8 = 5;
    pub const LT: u8 = 6;
    pub const LE: u8 = 7;
    pub const GT: u8 = 8;
    pub const GE: u8 = 9;
    pub const EQ: u8 = 10;
    pub const NE: u8 = 11;
    pub const AND: u8 = 12;
    pub const OR: u8 = 13;
    pub const MIN: u8 = 14;
    pub const MAX: u8 = 15;
    pub const STEP: u8 = 16;
    pub const ATAN2: u8 = 17;
    pub const HASH: u8 = 18;
    pub const LEN: u8 = 19;
    pub const BIT: u8 = 20;
    // Per-pixel only, written by `range::narrow` over a laid-out program:
    // `add`, `sub` and `mul` whose operands' ranges (the inputs within
    // theirs: see `range`) say they never saturate — the same value
    // without the overflow test (`ADD_W`, `SUB_W`, `MUL_W`), and a `mul`
    // whose 64-bit product fits 32 bits with one multiply (`MUL_N`).
    pub const ADD_W: u8 = 21;
    pub const SUB_W: u8 = 22;
    pub const MUL_W: u8 = 23;
    pub const MUL_N: u8 = 24;
    // `NAME[u, v]`, a sprite's pixel (`sprite NAME { … }`): `d = the key's
    // number at column floor(u), row floor(v)` of the sprite whose header
    // is `tables[4c]` (`c` an immediate: `super::store_at`), 0 outside it
    // ([`super::SprHead`]).
    // Any level; never folded (a constant u, v: once a frame).
    pub const SPR: u8 = 25;
    pub const LAST_BIN: u8 = 25;
    // The draw stage's paint (`crate::draw`, scalar slots only): `ARGS a
    // b c` adds three values to the primitive's arguments; `PAINT d, a, b,
    // c` adds three more and draws primitive `d` (`paint::*`) with them;
    // `IF a, b`: on to step `b` (forward, in the draw stage) when slot `a`
    // is 0; `JUMP b`: on to step `b` (forward).
    pub const ARGS: u8 = 26;
    pub const PAINT: u8 = 27;
    pub const IF: u8 = 28;
    pub const JUMP: u8 = 29;
    // `under(x', y')`'s channel `c` (0 red, 1 green, 2 blue, an immediate)
    // of the picture under the formula at pixel (round(a), round(b)),
    // held to the panel; 0..1. Per pixel only.
    pub const UAT: u8 = 30;
    // `palette P = tween(Q, R, w)`: `TWEEN d, a, b, c`, palette `a` mixed
    // towards `b` by slot `c` into `d` (`super::TW_*`: live slots, the
    // store, working palettes; all 32 entries or a half). Once a frame,
    // the frame section's last steps (`super::own_palettes`).
    pub const TWEEN: u8 = 31;
    // unary
    pub const NEG: u8 = 32;
    pub const NOT: u8 = 33;
    pub const ABS: u8 = 34;
    pub const FLOOR: u8 = 35;
    pub const FRAC: u8 = 36;
    pub const SIN: u8 = 37;
    pub const COS: u8 = 38;
    pub const TRI: u8 = 39;
    pub const SAW: u8 = 40;
    pub const SQR: u8 = 41;
    pub const SQRT: u8 = 42;
    pub const RECIP: u8 = 43;
    // `pick(k, …)` per pixel: `d = k`'s choice: `b` is the first of `c`
    // constant slots holding the slots of the choices (all per frame or per
    // row); 0 where floor(k) is outside 0..c ([`super::pick`]). One operand.
    pub const PICK: u8 = 44;
    // `NAME[k]`, a table's entry: `d = tables[4b + floor(k)]` (`b` the
    // table's place in fours, `super::store_at`) where floor(k)
    // is 0..=c (the table's length less one), else 0 ([`super::tab`]). Any
    // level; one operand.
    pub const TAB: u8 = 45;
    // `mul(a, b)` where `b` is 2^-n for inputs in range (n = `c`, 0..=16, an
    // immediate): exactly `a >> n`, an arithmetic shift; its checked form
    // (`range::checked`) is `mul(a, b)` (`range::narrow`, per pixel only).
    pub const SHR: u8 = 46;
    pub const LAST_UN: u8 = 46;
    // `PALIDX d, a, b`: a draw colour `pal(P, v)`'s paint index: `b` (an
    // immediate, where P's 32 entries start in the paint palette) +
    // `fixed::pal_index(a)`, as a whole number. Not per pixel.
    pub const PALIDX: u8 = 47;
    // Wide values: `t` in a `loop forever` formula grows without end, so it
    // and the values made from it once a frame are Q48.16 in an i64
    // (`crate::fixed`'s `w_*`), in [`crate::Scratch`]'s wide slots, never
    // a lane. Once a frame only; their operands are wide slots or scalar
    // slots as each says ([`wide_ops`]). Wide out (`d` a wide slot):
    pub const W_ADD: u8 = 48; // wide a + wide b
    pub const W_ADDS: u8 = 49; // wide a + scalar b
    pub const W_SUB: u8 = 50; // wide a - wide b
    pub const W_SUBS: u8 = 51; // wide a - scalar b
    pub const W_NEG: u8 = 52; // -wide a
    pub const W_MULS: u8 = 53; // wide a * scalar b
    pub const W_DIVS: u8 = 54; // wide a / scalar b
    pub const W_FLOOR: u8 = 55; // floor(wide a)
                                // Back to Q16.16 (`d` a scalar slot):
    pub const W_FRAC: u8 = 56; // frac(wide a)
    pub const W_MOD: u8 = 57; // wide a % scalar b
    pub const W_CMP: u8 = 58; // wide a <op> scalar b, `c` the compare (LT..NE)
    pub const W_MINMAX: u8 = 59; // min/max(wide a, scalar b), `c` MIN or MAX
    pub const W_RED: u8 = 60; // wide a as 32 bits for `hash` (its own bits when it fits)
                              // A noise's lattice offset (`d` its slot, `noise_slot`'s for its
                              // context and axis): the wide a's whole part times the axis's prime,
                              // as 32 bits, for the `NOISE_O*` steps.
    pub const W_LAT: u8 = 61;
    // `noise(w, y, z)` once a frame, w wide (a), y and z scalars (b, c): no
    // context needed (`d` a scalar slot).
    pub const W_NOISEX: u8 = 62;
    // The 16 bits below the point of wide a · scalar b (0..65535, raw): what
    // `MULR` carries into a split's part times b (`d` a scalar slot).
    pub const W_REM: u8 = 63;
    pub const W_FIRST: u8 = W_ADD;
    pub const W_LAST: u8 = W_REM;
    // ternary
    pub const SELECT: u8 = 64;
    pub const CLAMP: u8 = 65;
    pub const MIX: u8 = 66;
    pub const SMOOTH: u8 = 67;
    pub const NOISE: u8 = 68;
    // a font's pixel: `digit(n, x, y)`, `glyph(c, x, y)`, in the normal
    // font (5 × 7) and the small one (4 × 6); a table, like sin, so not
    // folded
    pub const DIGIT: u8 = 69;
    pub const GLYPH: u8 = 70;
    pub const DIGIT_S: u8 = 71;
    pub const GLYPH_S: u8 = 72;
    // the bold digits (`digit(n, x, y, bold)`; no glyph of them)
    pub const DIGIT_B: u8 = 73;
    // `noise(x, y, z)` of values that grow without end (`loop forever`):
    // the operands their small parts, the lattice offsets of noise context
    // k (`NOISE_O + k`, 0..=3) in its slots (`noise_slot`), set by `W_LAT`
    // steps once a frame. Any level; never folded.
    pub const NOISE_O: u8 = 74;
    pub const NOISE_O_LAST: u8 = 77;
    // `sat((a·b + c) >> 16)`, c raw (0..65535): a split's part times a
    // value the same all frame, the wide part's remainder carried in.
    pub const MULR: u8 = 78;
    pub const LAST_TERN: u8 = 78;
    // A scene's guard (`scene NAME when c { … }`), before its row steps or
    // its pixel steps: `GUARD d, a, b, c`: while slot `a` is 0 the `b` steps
    // after it are skipped, their values 0 — in the row section each one's
    // slot, in the pixel section the registers of the mask `d | (c & 15) <<
    // 8` (the run's values read after it); `c >> 4` the scene's number.
    pub const GUARD: u8 = 79;
    // nullary, per pixel: the picture under the formula (its red, green,
    // blue, luminance), 0..1
    pub const U_R: u8 = 80;
    pub const U_G: u8 = 81;
    pub const U_B: u8 = 82;
    pub const U_L: u8 = 83;
    // nullary, per pixel: what the draw stage put at the pixel, 0..1
    pub const INK: u8 = 84;
    // `ink(n)`: the canvas at the pixel in `n` levels, round(byte·n/255)
    // (`fixed::ink_levels`); `a` the constant n. Per pixel, never folded.
    pub const INKN: u8 = 85;
    // `cycle(P, v, s)`'s turn (a row step, the row section's last steps):
    // `TURN d, a, b`: the formula's palette slot `d` (0..=2, palette 5 + d)
    // is palette `b` turned by `a`: entry e is `b`'s entry (e + floor(a·32))
    // mod 32 ([`super::turn`]). Writes no slot; run for each row before its
    // pixels (a turn of a per-frame value the same each row).
    pub const TURN: u8 = 86;
    // `layer rate N { }`'s marker, first of the pixel steps after the
    // column and still runs: `SLOW d, a, b, c`: the `a` steps after it are
    // the slow layers' (their values and `b` colour steps), redrawn every
    // `d`th frame a band of rows at a time (`crate::rate`); `c` says what
    // they read (`rate::READS_*`). Laid out first in the pixel section: the
    // slow part runs only them, the fast part the steps after them, over
    // the held picture; never the two in one go.
    pub const SLOW: u8 = 87;
    // The draw stage (`ops[frame_end..draw_end]`, scalar slots only):
    // `FOR d, a, b`: counter `d` = 0, the count in slot `a`, its END at
    // index `b`; `END a, b, c`: counter `a` += 1 and, while it is under the
    // count in `b`, on at index `c` (the FOR's next); `LINE_FROM a, b, c`:
    // the next line's start (x, y) and ink; `LINE_TO a, b, c`: its end and
    // ink, drawn; `DOT a, b, c`: a point (x, y) of ink c.
    pub const FOR: u8 = 88;
    pub const END: u8 = 89;
    pub const LINE_FROM: u8 = 90;
    pub const LINE_TO: u8 = 91;
    pub const DOT: u8 = 92;
    // The still cache's marker, first of the pixel steps that only work out
    // values the cache holds (position-only values: the same every frame):
    // `STILL d, a, b, c`: `d` values cached (1 or 2), in registers `b` and
    // `c` (lane operands) after the `a` steps that follow it. With a filled
    // [`crate::Still`] the VM skips those `a` steps and copies the cached
    // lanes into `b` and `c` instead; without one it is a no-op.
    pub const STILL: u8 = 93;
    // The column run's marker, first of the pixel steps: `COLUMN a`: the
    // `a` steps after it work out values of `x`, `px` and per-frame values
    // only (the same down a column). With lanes of a whole row the VM runs
    // them on a frame's first row and skips them on the others, their
    // values kept in their registers (the compiler never reuses those).
    pub const COLUMN: u8 = 94;
    // A fused run's marker (`crate::fuse`): `FUSE d, a`: the `a` steps after
    // it are one fused step of kind `d` (`fuse::F_*`). The VM runs them as
    // one kernel when it runs the narrowed steps, the composition of the
    // same functions (else, and in wasm, it is a no-op and they run).
    pub const FUSE: u8 = 95;
    // the colour of the bottom layer when nothing shows through it: written
    // over the pixels, as v0's last line
    pub const OUT_RGB: u8 = 96;
    pub const OUT_HSV: u8 = 97;
    pub const OUT_PAL: u8 = 98;
    pub const OUT_GRAY: u8 = 99;
    /// A layer blended onto what is below it: `OUT_LAYER + kind + 4 ·
    /// blend` (kind: the `OUT_*` above less `OUT_RGB`; blend: `fixed`'s
    /// `BLEND_*`), with a coverage operand (`d` for rgb and hsv, `b` for
    /// pal and gray).
    pub const OUT_LAYER: u8 = 100;
    // Fused compares (P0b-4, psb ABI 2), any level, above the colours
    // (`OUT_LAYER` + 4 × 6 blends ends at 123): the compiler makes them from
    // a comparison only one step reads (`fuse_compares`). `SEL_LT d, a, b,
    // c`: `c` where a < b, else 0 (`(a < b) * c`, `(b > a) * c`); `SEL_LE`:
    // where a <= b (`(a <= b) * c`, `(b >= a) * c`, `step(a, b) * c`).
    // `CNT_* d, a, b, c`: how many of `a op b`, `a op c` hold, 0..2 (`(v >=
    // lo) + (v >= hi)`: a band's index).
    pub const SEL_LT: u8 = 0xf0;
    pub const SEL_LE: u8 = 0xf1;
    pub const CNT_LT: u8 = 0xf2;
    pub const CNT_LE: u8 = 0xf3;
    pub const CNT_GT: u8 = 0xf4;
    pub const CNT_GE: u8 = 0xf5;
    pub const CMP_FIRST: u8 = SEL_LT;
    pub const CMP_LAST: u8 = CNT_GE;
    // `div(a, b)` per pixel where `b` is the same across the lane (a
    // constant, a frame's or a row's value): written by `range::narrow`
    // over a laid-out program (never in a `.psb`), the same value as `DIV`
    // by [`crate::fixed::DivBy`] (multiplies, no division). `c`: what the
    // ranges say of `b` for inputs in range ([`DIV_BIG`], [`DIV_POS`]).
    pub const DIV_S: u8 = 0xf6;
    /// `DIV_S`'s `c`: |b| > 1 (no quotient reaches the ends, `E ≥ 32`).
    pub const DIV_BIG: u8 = 1;
    /// `DIV_S`'s `c`: b > 0.
    pub const DIV_POS: u8 = 2;
    /// `DIV_S`'s `c`: a ≥ 0 (the JIT skips the magnitude and the sign).
    pub const DIV_NONNEG: u8 = 4;
    /// `DIV_S`'s `c`: |b| may be under 1 (the VM's slow case: the costs).
    pub const DIV_SMALL: u8 = 8;

    /// A step that reads a table (the sine, a font): never folded, run once
    /// a frame when its operands are constants.
    pub fn is_table(c: u8) -> bool {
        matches!(c, SIN | COS | SPR | PALIDX | DIGIT..=DIGIT_B | NOISE_O..=NOISE_O_LAST)
    }

    /// A wide step (`W_*`): once a frame, its operands wide or scalar
    /// slots as [`wide_ops`] says.
    #[inline(always)]
    pub fn is_wide(c: u8) -> bool {
        (W_FIRST..=W_LAST).contains(&c)
    }

    /// For a wide step: whether `d` is a wide slot (else a scalar slot, or
    /// for `W_LAT` an offset), and how many of `a`, `b` are value operands
    /// and which of them are wide slots (bit 0 `a`, bit 1 `b`).
    pub fn wide_ops(c: u8) -> (bool, usize, u8) {
        match c {
            W_ADD | W_SUB => (true, 2, 3),
            W_ADDS | W_SUBS | W_MULS | W_DIVS => (true, 2, 1),
            W_NEG | W_FLOOR => (true, 1, 1),
            W_MOD | W_CMP | W_MINMAX | W_REM => (false, 2, 1),
            W_NOISEX => (false, 3, 1),
            _ => (false, 1, 1),
        }
    }

    /// Whether a step writes the scalar slot in its `d` (the per-frame,
    /// per-row and draw stage values; not a wide step that writes a wide
    /// slot or an offset, nor a step that writes pixels or nothing).
    #[cfg_attr(all(target_os = "espidf", feature = "sram1-iram"), link_section = ".iram1.s1")]
    pub fn writes_scalar(c: u8) -> bool {
        match c {
            W_FIRST..=W_LAST => !wide_ops(c).0,
            LINE_FROM | LINE_TO | DOT | STILL | COLUMN | FUSE | END | TURN | SLOW => false,
            ARGS | PAINT | IF | JUMP | TWEEN | GUARD => false,
            c => !is_colour(c),
        }
    }

    /// A step that writes pixels rather than a value.
    pub fn is_colour(c: u8) -> bool {
        (OUT_RGB..CMP_FIRST).contains(&c)
    }

    /// For a colour step: the `OUT_*` kind (`OUT_RGB` .. `OUT_GRAY`) and
    /// the blend, `None` for the bottom layer's plain write.
    pub fn colour_parts(c: u8) -> (u8, Option<u8>) {
        if c < OUT_LAYER {
            (c, None)
        } else {
            let k = c - OUT_LAYER;
            (OUT_RGB + k % 4, Some(k / 4))
        }
    }

    /// Value operands (`a`, `b`, `c` in order) a step reads, the coverage
    /// of a layer not counted.
    #[cfg_attr(all(target_os = "espidf", feature = "sram1-iram"), link_section = ".iram1.s1")]
    pub fn arity(c: u8) -> usize {
        match c {
            ARGS | PAINT => 3,
            UAT => 2,
            TWEEN | GUARD => 1,
            IF => 1,
            JUMP => 0,
            0..=LAST_BIN => 2,
            32..=LAST_UN | PALIDX => 1,
            W_FIRST..=W_LAST => wide_ops(c).1,
            64..=LAST_TERN => 3,
            U_R..=INK => 0,
            INKN => 1,
            TURN => 1,
            SLOW => 0,
            FOR => 1,
            END => 2,
            LINE_FROM..=DOT => 3,
            STILL | COLUMN | FUSE => 0,
            CMP_FIRST..=CMP_LAST => 3,
            DIV_S => 2,
            _ => match colour_parts(c).0 {
                OUT_RGB | OUT_HSV => 3,
                _ => 1,
            },
        }
    }

    /// A palette colour's flag (on its `d`, the palette): times its `c`
    /// (`pal(P, v) * k`).
    pub const PAL_SCALED: u8 = 0x80;
    /// With [`PAL_SCALED`]: bits 3-6 of `d` are a shift, the scale being
    /// `c / 2^shift` (`pal(P, v) * floor(q)/8`: the `/8` folded into the
    /// colour step, exactly `fixed::mul(c, 2^-shift)`, an arithmetic shift).
    pub const PAL_SHIFT: u8 = 3;
    /// The palette's bits of a palette colour's `d`.
    pub const PAL_MASK: u8 = 7;

    /// Value operands a step reads: [`arity`], and for a scaled palette
    /// colour its scale too (the coverage not counted).
    pub fn values(op: &super::Op) -> usize {
        let n = arity(op.code);
        if is_colour(op.code) && colour_parts(op.code).0 == OUT_PAL && op.d & PAL_SCALED != 0 {
            n + 1
        } else {
            n
        }
    }

    /// Every operand a step reads (its value operands, then a layer's
    /// coverage) and how many.
    pub fn operands(op: &super::Op) -> ([u8; 4], usize) {
        if is_wide(op.code) {
            // Only its scalar operand (wide slots are not scalar slots).
            let (_, n, w) = wide_ops(op.code);
            return match n {
                3 => ([op.b, op.c, 0, 0], 2),
                2 if w & 2 == 0 => ([op.b, 0, 0, 0], 1),
                _ => ([0; 4], 0),
            };
        }
        if op.code == TWEEN {
            // Its weight (its palettes are not slots).
            return ([op.c, 0, 0, 0], 1);
        }
        let n = values(op);
        let mut v = [op.a, op.b, op.c, 0];
        if n == 2 && is_colour(op.code) {
            // A scaled palette: its index, then its scale.
            v[1] = op.c;
        }
        match colour_parts(op.code) {
            (k, Some(_)) if is_colour(op.code) => {
                v[n] = if k == OUT_RGB || k == OUT_HSV {
                    op.d
                } else {
                    op.b
                };
                (v, n + 1)
            }
            _ => (v, n),
        }
    }

    /// The step's name, for listings.
    pub fn name(c: u8) -> &'static str {
        const N: [&str; 26] = [
            "add", "sub", "mul", "div", "mod", "mask", "lt", "le", "gt", "ge", "eq", "ne", "and",
            "or", "min", "max", "step", "atan2", "hash", "len", "bit", "add_w", "sub_w", "mul_w",
            "mul_n", "spr",
        ];
        const U: [&str; 15] = [
            "neg", "not", "abs", "floor", "frac", "sin", "cos", "tri", "saw", "sqr", "sqrt",
            "recip", "pick", "tab", "shr",
        ];
        const T: [&str; 15] = [
            "select",
            "clamp",
            "mix",
            "smooth",
            "noise",
            "digit",
            "glyph",
            "digit_small",
            "glyph_small",
            "digit_bold",
            "noise_o0",
            "noise_o1",
            "noise_o2",
            "noise_o3",
            "mulr",
        ];
        const WN: [&str; 16] = [
            "w_add", "w_adds", "w_sub", "w_subs", "w_neg", "w_muls", "w_divs", "w_floor", "w_frac",
            "w_mod", "w_cmp", "w_minmax", "w_red", "w_lat", "w_noisex", "w_rem",
        ];
        match c {
            ARGS => "args",
            PAINT => "paint",
            IF => "if",
            JUMP => "jump",
            UAT => "under_at",
            TWEEN => "tween",
            GUARD => "guard",
            PALIDX => "palidx",
            0..=LAST_BIN => N[c as usize],
            32..=LAST_UN => U[(c - 32) as usize],
            W_FIRST..=W_LAST => WN[(c - W_FIRST) as usize],
            64..=LAST_TERN => T[(c - 64) as usize],
            U_R..=INKN => ["ur", "ug", "ub", "ul", "ink", "inkn"][(c - U_R) as usize],
            TURN => "turn",
            SLOW => "slow",
            CMP_FIRST..=CMP_LAST => ["sel_lt", "sel_le", "cnt_lt", "cnt_le", "cnt_gt", "cnt_ge"]
                [(c - CMP_FIRST) as usize],
            DIV_S => "div_s",
            FOR..=FUSE => [
                "for",
                "end",
                "line_from",
                "line_to",
                "dot",
                "still",
                "column",
                "fuse",
            ][(c - FOR) as usize],
            _ => ["rgb", "hsv", "pal", "gray"][((colour_parts(c).0 - OUT_RGB) & 3) as usize],
        }
    }
}

/// A pixel section guard's registers (`code::GUARD`: bit r, register r).
#[inline(always)]
pub(crate) fn guard_mask(op: &Op) -> u16 {
    op.d as u16 | ((op.c & 15) as u16) << 8
}

/// One step: `d = code(a, b, c)`. For `OUT_PAL`, `d` is the palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Op {
    pub(crate) code: u8,
    pub(crate) d: u8,
    pub(crate) a: u8,
    pub(crate) b: u8,
    pub(crate) c: u8,
}

impl Op {
    /// The step index a draw stage step goes on at (16 bits in two
    /// operand bytes: a program has up to [`MAX_OPS`] steps): `IF`'s and
    /// `JUMP`'s target (`b`, `c` high), a `FOR`'s END (`b`, `c`'s low seven
    /// bits high; its top bit is [`LANES`]), an `END`'s step back (`c`, `d`
    /// high). 0 for any other step.
    #[inline(always)]
    pub(crate) fn target(&self) -> usize {
        match self.code {
            code::IF | code::JUMP => self.b as usize | (self.c as usize) << 8,
            code::FOR => self.b as usize | ((self.c & !LANES) as usize) << 8,
            code::END => self.c as usize | (self.d as usize) << 8,
            _ => 0,
        }
    }

    /// Set [`target`](Self::target) (a FOR's lane mark kept).
    pub(crate) fn set_target(&mut self, at: usize) {
        let (lo, hi) = (at as u8, (at >> 8) as u8);
        match self.code {
            code::IF | code::JUMP => (self.b, self.c) = (lo, hi),
            code::FOR => (self.b, self.c) = (lo, (self.c & LANES) | (hi & !LANES)),
            code::END => (self.c, self.d) = (lo, hi),
            _ => {}
        }
    }
}

/// Inputs bit: the program reads `i` (built per row).
pub(crate) const USES_I: u8 = 1;
/// The bottom layer lets what is under show through: each lane starts as
/// the picture under the formula (black when there is none).
pub(crate) const BACKDROP: u8 = 2;
/// The program reads what is under it (`ur ug ub ul`).
pub(crate) const UNDER: u8 = 4;
/// The program reads the draw stage's canvas (`ink`).
pub(crate) const INKED: u8 = 8;
/// The program reads `seed` (slot [`S_SEED`]).
pub(crate) const SEEDED: u8 = 16;
/// A colour step reads a palette of the formula's own.
pub(crate) const OWNPAL: u8 = 32;

/// `table` entries a formula may hold in all: the store of tables, sprites
/// and texts (1,024 numbers since psb ABI 3; 256 before).
pub const MAX_TABLE: usize = 1024;

/// Numbers one `table` may hold (a `TAB` step's `c` is its last index).
pub const MAX_TABLE_LEN: usize = 256;

/// A table's or a sprite's place in the store as a step holds it (`TAB`'s
/// `b`, `SPR`'s `c`, one byte): in fours, the compiler laying each out at
/// a multiple of [`STORE_ALIGN`] ([`store_at`]).
pub(crate) const STORE_ALIGN: usize = 4;

/// The store index of a step's place byte ([`STORE_ALIGN`]).
#[inline(always)]
pub(crate) const fn store_at(b: u8) -> usize {
    (b as usize) * STORE_ALIGN
}

/// Built-in input reads a formula may have (`crate::world`: a key's
/// value, `since` or `was`, each one), each a scalar slot the player
/// writes once a frame.
pub const MAX_INS: usize = 24;

/// The built-in inputs a program reads ([`crate::world`]): `n` reads, in
/// the scalar slots from `base`, read `keys[k]` (`world::input_byte`: the
/// key's number and what of it). No reads: `n` 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Ins {
    pub(crate) n: u8,
    pub(crate) base: u8,
    pub(crate) keys: [u8; MAX_INS],
}

impl Ins {
    /// The reads: (slot, input byte).
    pub(crate) fn iter(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        let n = (self.n as usize).min(MAX_INS);
        self.keys[..n]
            .iter()
            .enumerate()
            .map(move |(k, &b)| (self.base.wrapping_add(k as u8), b))
    }

    /// Whether scalar slot `v` is one of the reads'.
    #[inline]
    pub(crate) fn holds(&self, v: u8) -> bool {
        self.n != 0 && v >= self.base && ((v - self.base) as usize) < (self.n as usize).min(MAX_INS)
    }
}

/// The program paints (`draw { }` with colours: the paint canvas, laid
/// over every layer).
pub(crate) const PAINTS: u8 = 64;

/// Colours a formula may paint with (the paint palette; entry 0 is none).
pub const MAX_PAINT: usize = 128;

/// The paint primitives (`code::PAINT`'s `d`) and how many values each
/// takes (its colour last).
pub mod paint {
    pub const CLS: u8 = 0;
    pub const PSET: u8 = 1;
    pub const LINE: u8 = 2;
    pub const RECT: u8 = 3;
    pub const RECTFILL: u8 = 4;
    pub const CIRC: u8 = 5;
    pub const CIRCFILL: u8 = 6;
    pub const TRIFILL: u8 = 7;
    pub const SPR: u8 = 8;
    pub const PRINT: u8 = 9;
    pub const PRINTNUM: u8 = 10;
    pub const LAST: u8 = 10;
    /// Values a primitive takes.
    pub const ARGS: [usize; 11] = [1, 3, 5, 5, 5, 4, 4, 7, 4, 6, 6];
}

/// The palettes `pal` reads of a formula's own: three live slots, `pal`'s
/// palettes 5, 6 and 7 after the five built-ins, each 32 entries, worked
/// out once a frame ([`own_palettes`]): a palette of the store as it is
/// (live slot `j` from store slot `j`), a tween of palettes
/// (`code::TWEEN`), or one `cycle` turns each row (`code::TURN`).
pub const USER_PALETTES: usize = 3;

/// The palette store (`Program::pals`): a formula's own palettes of
/// colours, 32 entries (0x00RRGGBB) each. A palette in a tween's half
/// takes the half of a slot (its colours over 16 entries).
pub const MAX_PALETTES: usize = 8;

/// A tween's working palettes besides the live slots (for one inside
/// another, or a palette that only tweens read): `own_palettes`' own.
pub(crate) const TWEEN_TEMPS: usize = 4;

/// A tween's source (`TWEEN`'s `a` and `b`): store slot 0..8, live slot
/// `TW_LIVE + j`, or working palette `TW_TEMP + k`.
pub(crate) const TW_LIVE: u8 = MAX_PALETTES as u8;
pub(crate) const TW_TEMP: u8 = TW_LIVE + USER_PALETTES as u8;
/// A tween's destination (`TWEEN`'s `d`): live slot 0..3 or working
/// palette `3 + k` in the low bits, and which entries: all 32, or with
/// these bits the low or the high 16 (its sources read in the same half).
pub(crate) const TW_LOW: u8 = 0x10;
pub(crate) const TW_HIGH: u8 = 0x20;

/// A formula's own palettes as `pal` reads them, unpacked once a frame.
pub(crate) type Pals = [[[u8; 3]; 32]; USER_PALETTES];

/// Palette `i` as `pal` reads it: a built-in (0..=4) from the tables lent,
/// or a formula's own (5..=7) from those unpacked for the frame.
#[inline(always)]
pub(crate) fn palette<'a>(
    t: &crate::tables::Tables<'a>,
    own: &'a Pals,
    i: u8,
) -> &'a [[u8; 3]; 32] {
    if (i as usize) < t.palettes.len() {
        return t.palettes[i as usize];
    }
    &own[(i as usize).saturating_sub(5) % USER_PALETTES]
}

#[inline(always)]
fn rgb_of(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// The formula's own palettes for a frame: the live slots from the store,
/// then its tweens in order (the frame section's last steps, run after the
/// frame's values: their weights are). Out of line, in flash: once a
/// frame, and only for a formula with some (`OWNPAL`).
#[inline(never)]
pub(crate) fn own_palettes(prog: &Prog, sc: &Sc, own: &mut Pals) {
    for (j, p) in own.iter_mut().enumerate() {
        for (k, c) in p.iter_mut().enumerate() {
            *c = rgb_of(prog.pals[32 * j + k]);
        }
    }
    let tw = prog.tweens();
    if tw.is_empty() {
        return;
    }
    let mut tmp = [[[0u8; 3]; 32]; TWEEN_TEMPS];
    for op in &prog.ops[tw] {
        tween(&prog.pals, own, &mut tmp, sc, op);
    }
}

/// One `TWEEN d, a, b, c`: palette `a` mixed towards `b` by slot `c`'s
/// value (held to 0..1), entry by entry, channel by channel, rounded:
/// `a + ((b − a)·w + ½) >> 16`, exactly `a` at 0 and `b` at 1. Indices
/// are masked, whatever the program.
fn tween(
    store: &[u32; MAX_PALETTES * 32],
    own: &mut Pals,
    tmp: &mut [[[u8; 3]; 32]; TWEEN_TEMPS],
    sc: &Sc,
    op: &Op,
) {
    let w = sc[op.c as usize].clamp(0, ONE);
    let (lo, hi) = match op.d & (TW_LOW | TW_HIGH) {
        TW_LOW => (0, 16),
        TW_HIGH => (16, 32),
        _ => (0, 32),
    };
    for e in lo..hi {
        let src = |s: u8| -> [u8; 3] {
            if s < TW_LIVE {
                rgb_of(store[(32 * s as usize + e) % (MAX_PALETTES * 32)])
            } else if s < TW_TEMP {
                own[(s - TW_LIVE) as usize % USER_PALETTES][e]
            } else {
                tmp[(s - TW_TEMP) as usize % TWEEN_TEMPS][e]
            }
        };
        let (p, q) = (src(op.a), src(op.b));
        let mut c = [0u8; 3];
        for k in 0..3 {
            let (x, y) = (i32::from(p[k]), i32::from(q[k]));
            c[k] = (x + (((y - x) * w + 0x8000) >> 16)) as u8;
        }
        let d = (op.d & 7) as usize;
        if d < USER_PALETTES {
            own[d][e] = c;
        } else {
            tmp[(d - USER_PALETTES) % TWEEN_TEMPS][e] = c;
        }
    }
}

/// `cycle`'s turn (`code::TURN`): the formula's palette slot `op.d` is
/// palette `op.b` turned by the value in slot `op.a`, entry e being `b`'s
/// entry (e + floor(a·32)) mod 32. Out of line, in flash: a row step's
/// (once a row, ~1 µs). Indices are masked, whatever the program.
#[inline(never)]
pub(crate) fn turn(t: &crate::tables::Tables, own: &mut Pals, sc: &Sc, op: &Op) {
    let n = ((sc[op.a as usize] >> 11) & 31) as usize;
    let src = *palette(t, own, op.b);
    let dst = &mut own[(op.d as usize) % USER_PALETTES];
    for (e, c) in dst.iter_mut().enumerate() {
        *c = src[(e + n) & 31];
    }
}

/// Every `TURN` of the row section (the JIT's driver, after its row
/// function: they are the section's last steps, so the same as the VM's).
#[cfg(any(
    test,
    feature = "docs",
    all(feature = "jit", target_arch = "xtensa", target_os = "espidf")
))]
pub(crate) fn turn_rows(prog: &Prog, t: &crate::tables::Tables, own: &mut Pals, sc: &Sc) {
    for op in &prog.ops[prog.turns()] {
        turn(t, own, sc, op);
    }
}

/// A compiled formula with a store of `S` (its tables, sprites and texts:
/// [`Program`]'s holds [`MAX_TABLE`] numbers, the player's [`Prog`] only
/// what its formula needs). Plain data (every field an integer or an array
/// of them), so an all-zero value is valid (an empty program) and
/// [`crate::compile_into`] can fill one in place. `repr(C)`, the store
/// last: a [`Prog`] of `n` numbers is the same fields and `n` of them
/// ([`Prog::try_boxed`]).
#[repr(C)]
pub struct ProgramOf<S: ?Sized> {
    pub(crate) ops: [Op; MAX_OPS],
    /// The scalar slots' values at the start of a frame: the constants.
    pub(crate) init: [i32; MAX_SCALARS],
    /// `ops[..frame_end]` run once a frame, `..draw_end` once a frame
    /// after them (the draw stage: loops and lines into the canvas, the
    /// one section with jumps), `..row_end` once a row, `..pixel_end` per
    /// pixel: values, and a colour step per layer (bottom to top; the last
    /// step is always one).
    pub(crate) frame_end: u16,
    pub(crate) draw_end: u16,
    pub(crate) row_end: u16,
    pub(crate) pixel_end: u16,
    pub(crate) n_scalars: u8,
    pub(crate) regs: u8,
    pub(crate) uses: u8,
    /// The frame time `fps N` asks for, ms (10, 20 or 40); 0: none (the
    /// player's, 20 ms unless the path says).
    pub(crate) frame_ms: u8,
    /// The loop, ms (`loop N`, default 8 s); 0 for `loop forever`.
    pub(crate) loop_ms: u32,
    /// What the formula declares (its controls, whether it is a look).
    pub(crate) meta: crate::meta::Meta,
    /// The paint palette: colour `k` (0x00RRGGBB) of index `k` (0 none).
    pub(crate) paint: [u32; MAX_PAINT],
    /// The palette store: the formula's own palettes of colours, 32
    /// entries (0x00RRGGBB) a slot ([`MAX_PALETTES`]); live slot `j` is
    /// slot `j` unless a tween or a turn writes it.
    pub(crate) pals: [u32; MAX_PALETTES * 32],
    /// The built-in inputs it reads ([`Ins`]).
    pub(crate) ins: Ins,
    /// The store: the `table` constants (Q16.16, each table's entries in a
    /// row), the sprites and the texts. Its length is a power of two (the
    /// steps' reads are masked into it, whatever the program).
    pub(crate) tables: S,
}

/// A whole program: the compiler's, with the whole store
/// ([`MAX_TABLE`]); a value (`Copy`, `Default`).
pub type Program = ProgramOf<[i32; MAX_TABLE]>;

/// A program with the store its formula needs (a power of two of numbers,
/// [`MIN_STORE`] at least): the player's, on the heap
/// ([`Prog::try_boxed`], [`crate::psb::load_boxed`]). Every function that
/// plays a program takes one; a [`Program`] is one too (it derefs).
pub type Prog = ProgramOf<[i32]>;

/// The fewest numbers a [`Prog`]'s store holds.
pub const MIN_STORE: usize = STORE_ALIGN;

/// The store a formula of `used` numbers plays with: the power of two at
/// or above it, [`MIN_STORE`] at least.
pub const fn store_words(used: usize) -> usize {
    let n = if used < MIN_STORE { MIN_STORE } else { used };
    n.next_power_of_two()
}

// (The budget: 6.3 KB at P0's limits, `docs/research/endless-road-v2.md`
// §5; +1.3 KB for 768 steps, P2; the store out of the fixed part, sized
// by the formula (ABI 3), and 26 B for the inputs.)
const _: () = assert!(core::mem::size_of::<ProgramOf<[i32; 0]>>() <= 6_500);

impl Clone for Program {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for Program {}

impl PartialEq for Prog {
    fn eq(&self, o: &Prog) -> bool {
        self.ops == o.ops
            && self.init == o.init
            && self.frame_end == o.frame_end
            && self.draw_end == o.draw_end
            && self.row_end == o.row_end
            && self.pixel_end == o.pixel_end
            && self.n_scalars == o.n_scalars
            && self.regs == o.regs
            && self.uses == o.uses
            && self.frame_ms == o.frame_ms
            && self.loop_ms == o.loop_ms
            && self.meta == o.meta
            && self.paint == o.paint
            && self.pals == o.pals
            && self.ins == o.ins
            && self.used() == o.used()
            && self.tables[..self.used()] == o.tables[..o.used()]
    }
}

impl PartialEq for Program {
    fn eq(&self, o: &Program) -> bool {
        let (a, b): (&Prog, &Prog) = (self, o);
        a == b
    }
}

impl Eq for Program {}

impl core::fmt::Debug for Program {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let p: &Prog = self;
        p.fmt(f)
    }
}

impl core::ops::Deref for Program {
    type Target = Prog;
    #[inline(always)]
    fn deref(&self) -> &Prog {
        self
    }
}

impl core::ops::DerefMut for Program {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Prog {
        self
    }
}

impl Prog {
    /// The memory a [`Prog`] of `words` numbers takes.
    pub fn layout(words: usize) -> core::alloc::Layout {
        let head = core::alloc::Layout::new::<ProgramOf<[i32; 0]>>();
        let (l, _) = head
            .extend(core::alloc::Layout::array::<i32>(words).unwrap_or(head))
            .unwrap_or((head, 0));
        l.pad_to_align()
    }

    /// An empty program with a store of [`store_words`]`(used)` numbers on
    /// the heap, or `None` when the heap has no block for it.
    pub fn try_boxed(used: usize) -> Option<alloc::boxed::Box<Prog>> {
        let words = store_words(used.min(MAX_TABLE));
        let l = Self::layout(words);
        // SAFETY: a non-zero-sized layout (the fixed part is ~6 KB);
        // `ProgramOf` is `repr(C)` with the store last, so the fixed part's
        // fields sit where `ProgramOf<[i32; 0]>`'s do and the `words`
        // numbers follow (the layout above, which is `Layout::for_value`'s
        // for the result: the box frees it with the same one). Integers all
        // the way down: the zeroed memory is a valid value.
        unsafe {
            let p = alloc::alloc::alloc_zeroed(l);
            if p.is_null() {
                return None;
            }
            let fat = core::ptr::slice_from_raw_parts_mut(p.cast::<i32>(), words) as *mut Prog;
            let mut b = alloc::boxed::Box::from_raw(fat);
            b.clear();
            Some(b)
        }
    }

    /// A copy of this program in a store of its own size, or `None` when
    /// the heap has no block for it.
    pub fn try_boxed_copy(&self) -> Option<alloc::boxed::Box<Prog>> {
        let mut b = Self::try_boxed(self.tables.len())?;
        b.copy_from(self).then_some(b)
    }

    /// Become `src` (the store's numbers included); false when this
    /// store is too small for them (nothing changes then).
    pub fn copy_from(&mut self, src: &Prog) -> bool {
        let n = src.used();
        if n > self.tables.len() {
            return false;
        }
        self.ops = src.ops;
        self.init = src.init;
        self.frame_end = src.frame_end;
        self.draw_end = src.draw_end;
        self.row_end = src.row_end;
        self.pixel_end = src.pixel_end;
        self.n_scalars = src.n_scalars;
        self.regs = src.regs;
        self.uses = src.uses;
        self.frame_ms = src.frame_ms;
        self.loop_ms = src.loop_ms;
        self.meta = src.meta;
        self.paint = src.paint;
        self.pals = src.pals;
        self.ins = src.ins;
        self.tables.fill(0);
        self.tables[..n].copy_from_slice(&src.tables[..n]);
        true
    }

    /// The store's numbers in use: up to the last that isn't 0.
    pub fn used(&self) -> usize {
        self.tables.iter().rposition(|&v| v != 0).map_or(0, |k| k + 1)
    }

    /// Numbers the store holds (a power of two).
    pub fn store_len(&self) -> usize {
        self.tables.len()
    }
}

impl core::fmt::Debug for Prog {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (fr, row, px) = self.counts();
        let dr = self.draw_steps();
        write!(
            f,
            "Program {{ steps: {} ({fr}/{dr}/{row}/{px}), regs: {}, loop_ms: {}, frame_ms: {} }}",
            self.len(),
            self.regs,
            self.loop_ms,
            self.frame_ms
        )
    }
}

impl Default for Program {
    fn default() -> Self {
        ProgramOf {
            ops: [Op::default(); MAX_OPS],
            init: [0; MAX_SCALARS],
            frame_end: 0,
            draw_end: 0,
            row_end: 0,
            pixel_end: 0,
            n_scalars: S_FIRST_FREE,
            regs: 0,
            uses: 0,
            frame_ms: 0,
            loop_ms: 8000,
            meta: crate::meta::Meta::default(),
            paint: [0; MAX_PAINT],
            pals: [0; MAX_PALETTES * 32],
            ins: Ins::default(),
            tables: [0; MAX_TABLE],
        }
    }
}

impl Prog {
    /// Steps in all, the colours included (≤ [`MAX_OPS`]).
    pub fn len(&self) -> usize {
        self.pixel_end as usize
    }

    /// Never: a compiled program has at least its colour.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Steps run once a frame, once a row, and per pixel (the colours
    /// counted with the pixel steps); the draw stage's are
    /// [`draw_steps`](Self::draw_steps).
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.frame_end as usize,
            self.row_end.saturating_sub(self.draw_end) as usize,
            self.pixel_end.saturating_sub(self.row_end) as usize - self.fused(),
        )
    }

    /// Steps in all, as the formula's limit counts them ([`MAX_STEPS`]):
    /// the fused runs' markers (`crate::fuse`) and the column run's and the
    /// still cache's not counted.
    pub fn steps(&self) -> usize {
        self.len() - self.fused()
    }

    /// The markers the cap does not count: the fused runs', the column
    /// run's and the still cache's.
    fn fused(&self) -> usize {
        let (re, pe) = (self.row_end as usize, self.len().min(MAX_OPS));
        self.ops[re.min(pe)..pe]
            .iter()
            .filter(|o| matches!(o.code, code::FUSE | code::COLUMN | code::STILL))
            .count()
    }

    /// Steps in the draw stage (`draw { }`: its loops' values, the loops
    /// and the primitives), each written once however often it runs.
    pub fn draw_steps(&self) -> usize {
        self.draw_end.saturating_sub(self.frame_end) as usize
    }

    /// Whether a frame has a draw stage: the canvas ([`crate::Ink`]) it
    /// needs, 4 KB, and draws into before the rows.
    pub fn draws(&self) -> bool {
        self.draw_end > self.frame_end
    }

    /// Whether the draw stage paints (`draw { }` with colours): the canvas's
    /// paint plane, 4 KB more, laid over every layer once a row.
    pub fn paints(&self) -> bool {
        self.uses & PAINTS != 0
    }

    /// The lines and dots the draw stage draws a frame: every primitive
    /// counted as often as its loops run it.
    pub fn primitives(&self) -> u32 {
        let mut n = 0;
        self.draw_walk(|_, op, mult| {
            if matches!(op.code, code::LINE_TO | code::DOT | code::PAINT) {
                n += mult;
            }
        });
        n
    }

    /// Each step of the draw stage with how many times a frame it runs
    /// (the product of its loops' counts).
    fn draw_walk(&self, mut f: impl FnMut(usize, &Op, u32)) {
        let mut stack = [1u32; 4];
        let mut depth = 0;
        let mut mult = 1u32;
        let end = (self.draw_end as usize).min(MAX_OPS);
        let first = (self.frame_end as usize).min(end);
        for (at, op) in self.ops[first..end].iter().enumerate() {
            let at = at + first;
            match op.code {
                code::FOR => {
                    f(at, op, mult);
                    if depth < stack.len() {
                        stack[depth] = mult;
                        depth += 1;
                    }
                    let n = (self.init_at(op.a) >> 16).max(1) as u32;
                    mult = mult.saturating_mul(n);
                }
                code::END => {
                    f(at, op, mult);
                    if depth > 0 {
                        depth -= 1;
                        mult = stack[depth];
                    }
                }
                _ => f(at, op, mult),
            }
        }
    }

    /// Colour steps: the layers drawn (a formula without `layer` blocks
    /// has one; layers hidden under an opaque one are left out).
    pub fn layers(&self) -> usize {
        self.ops[self.row_end as usize..self.len()]
            .iter()
            .filter(|op| code::is_colour(op.code))
            .count()
    }

    /// Whether a frame depends on the picture under the formula (it reads
    /// `ur ug ub ul`, or its bottom layer lets it show through): drawn
    /// over a GIF, it changes the GIF rather than replacing it.
    pub fn over(&self) -> bool {
        self.uses & (BACKDROP | UNDER) != 0
    }

    /// Scalar slots in use (≤ [`MAX_SCALARS`]): the inputs, the constants
    /// and the per-frame and per-row values — the "values" a formula may
    /// have ~119 of besides the inputs.
    pub fn values(&self) -> usize {
        self.n_scalars as usize
    }

    /// Lane registers the pixel steps use (≤ [`REGS`]).
    pub fn regs(&self) -> usize {
        self.regs as usize
    }

    /// The loop's length in ms; **0 for a formula that never repeats**
    /// (`loop forever`: see [`forever`](Self::forever)).
    pub fn loop_ms(&self) -> u32 {
        self.loop_ms
    }

    /// A `loop forever` formula: its `t` is the time since it started,
    /// growing without end ([`crate::Inputs::tw`]), and it may read `seed`
    /// ([`crate::Inputs::seed`]); [`loop_ms`](Self::loop_ms) is 0.
    pub fn forever(&self) -> bool {
        self.loop_ms == 0
    }

    /// The frame time its `fps N` line asks for, ms: 10 (`fps 100`), 20
    /// or 40 (`fps 25`); `None` without one. A path's `&fps=` wins over
    /// it; a look (a formula over the GIF) follows its path only.
    pub fn frame_ms(&self) -> Option<u32> {
        (self.frame_ms != 0).then_some(u32::from(self.frame_ms))
    }

    /// Which inputs the steps read: bits 0-2 the sliders `a`, `b`, `c`
    /// (the web app's editor shows a slider for each), bit 3 `t`, bit 4
    /// the wall clock (`hour`, `minute` or `second`), bit 5 the picture
    /// under the formula ([`Program::over`]), bit 6 `seed`, bit 7 a
    /// built-in input (`crate::world`; a `since` also counts as `t`, bit 3:
    /// it moves with time).
    pub fn inputs(&self) -> u8 {
        let mut m = if self.over() { 32 } else { 0 };
        if self.uses & SEEDED != 0 {
            m |= 64;
        }
        if self.ins.n != 0 {
            m |= 128;
        }
        if self.ins.iter().any(|(_, b)| b >> 6 == crate::world::SINCE) {
            m |= 8;
        }
        for op in &self.ops[..self.len()] {
            // A wide step reads `t` (every wide value is made from it).
            if code::is_wide(op.code) {
                m |= 8;
            }
            let (v, n) = code::operands(op);
            // A pick's choices are read through its table of slots.
            let table = if op.code == code::PICK {
                self.init
                    .get(op.b as usize..op.b as usize + op.c as usize)
                    .unwrap_or(&[])
            } else {
                &[]
            };
            for v in v[..n].iter().copied().chain(table.iter().map(|&s| s as u8)) {
                m |= match v {
                    S_A..=S_C => 1 << (v - S_A),
                    S_T if !self.forever() => 8,
                    S_HOUR..=S_SECOND => 16,
                    _ => 0,
                };
            }
        }
        m
    }

    /// Which sliders the steps read: bit 0 `a`, 1 `b`, 2 `c`.
    pub fn sliders(&self) -> u8 {
        self.inputs() & 7
    }

    /// An estimate of a frame's time on the ESP32 in µs, as an effect or a
    /// look draws it, from the per-step costs `psi-probe` measured on the
    /// device (ns a pixel at 128 lanes; recalibrated 2026-10-26 for layers,
    /// blends and coverage, tests/estimate.rs: within ~12 % of the VM's
    /// frame for layers and looks, then +15 % for the real paths): the
    /// per-pixel steps are nearly all of it; a per-row step is counted at
    /// 32 runs of ~0.3 µs. The web app's editor shows it as light / medium /
    /// heavy.
    pub fn esp32_us(&self) -> u32 {
        self.estimate(None)
    }

    /// [`esp32_us`](Self::esp32_us) as the player draws it with a filled
    /// still cache ([`crate::Still`]): the steps that only work out the
    /// cached values are skipped and each cached value is a copy into its
    /// register instead (~15 ns a pixel). The same as `esp32_us` for a
    /// formula without cached values ([`still_lanes`](Self::still_lanes)
    /// 0). A look never has one (it draws as `esp32_us` says).
    pub fn esp32_us_still(&self) -> u32 {
        self.estimate(self.still_marker())
    }

    /// [`esp32_us`](Self::esp32_us) with every draw loop a turn at a time
    /// (as before they ran as lanes): tests/estimate.rs, whose draw rows
    /// were measured so.
    #[doc(hidden)]
    pub fn esp32_us_turns(&self) -> u32 {
        self.estimate_with(None, false, false, None)
    }

    fn estimate(&self, still: Option<usize>) -> u32 {
        self.estimate_with(still, true, false, None)
    }

    /// [`esp32_us`](Self::esp32_us) as the ESP32's JIT draws it (its
    /// pixel stage and row stage compiled: `docs/research/psi-jit-esp32.md`):
    /// the steps before the kernel (the column and still runs) and the rest
    /// of the frame as the VM's, the kernel's steps at [`jit_value_ns`]'s
    /// costs (the device's rows). Never more than the VM's: the player keeps
    /// whichever draws a frame faster. A look's estimate (no still cache);
    /// the label and the governor's start, never a limit (the limits stay
    /// the VM's: the JIT may be off).
    pub fn esp32_us_jit(&self) -> u32 {
        self.estimate_with(None, true, true, None).min(self.esp32_us())
    }

    /// [`esp32_us_jit`](Self::esp32_us_jit) with a filled still cache, as
    /// [`esp32_us_still`](Self::esp32_us_still).
    pub fn esp32_us_jit_still(&self) -> u32 {
        self.estimate_with(self.still_marker(), true, true, None)
            .min(self.esp32_us_still())
    }

    /// Whether the panel's JIT draws this program's pixel stage (its
    /// kernel takes it): the editor's and the limits' tier
    /// ([`tier_us`](Self::tier_us)).
    pub fn jit_takes(&self) -> bool {
        crate::jitplan::jit_takes(self)
    }

    /// What the panel takes a frame, µs, for the tier and the limits
    /// (P0b-2): [`esp32_us_jit_still`](Self::esp32_us_jit_still) where the
    /// JIT takes it ([`jit_takes`](Self::jit_takes)), else
    /// [`esp32_us_still`](Self::esp32_us_still); as a look (`look`: no still
    /// cache) [`esp32_us_jit`](Self::esp32_us_jit) or
    /// [`esp32_us`](Self::esp32_us). The VM's figure is the "without the
    /// JIT" one shown beside it.
    pub fn tier_us(&self, look: bool) -> u32 {
        match (self.jit_takes(), look) {
            (true, false) => self.esp32_us_jit_still(),
            (true, true) => self.esp32_us_jit(),
            (false, false) => self.esp32_us_still(),
            (false, true) => self.esp32_us(),
        }
    }

    /// [`tier_us`](Self::tier_us) for one frame, its scenes counted as the
    /// panel draws them: each by the rows its `when` is on in that frame
    /// (from [`crate::scene_rows`]), every scene on, none off, and the draw
    /// stage's `if` arms it takes — where `tier_us` counts the two
    /// costliest scenes on all 32 rows and the costlier arm of each `if`.
    /// The JIT's figure where it takes the program, else the VM's.
    pub fn tier_us_frame(&self, look: bool, frame: &SceneFrame) -> u32 {
        let still = if look { None } else { self.still_marker() };
        let vm = self.estimate_with(still, true, false, Some(frame));
        if self.jit_takes() {
            self.estimate_with(still, true, true, Some(frame)).min(vm)
        } else {
            vm
        }
    }

    /// The VM's figure for one frame (the "without the JIT" beside
    /// [`tier_us_frame`](Self::tier_us_frame)).
    pub fn vm_us_frame(&self, look: bool, frame: &SceneFrame) -> u32 {
        let still = if look { None } else { self.still_marker() };
        self.estimate_with(still, true, false, Some(frame))
    }

    /// The first step a kernel draws: after the column run, and after the
    /// still cache's run with what is laid out before its marker (the VM
    /// runs those as lanes).
    pub(crate) fn kernel_start(&self) -> usize {
        let (re, pe) = (self.fast_start(), self.len());
        if let Some(k) = self.still_marker() {
            return (k + 1 + self.ops[k].a as usize).min(pe);
        }
        let col = self.column_run();
        if col.end > col.start {
            col.end
        } else {
            re
        }
    }

    /// `frame`: one frame's scenes and draw arms (`tier_us_frame`); `None`:
    /// the two costliest scenes on every row, the costlier arms.
    fn estimate_with(
        &self,
        still: Option<usize>,
        lanes: bool,
        jit: bool,
        frame: Option<&SceneFrame>,
    ) -> u32 {
        let (skip, cached) = match still {
            Some(k) => {
                let op = &self.ops[k];
                (k + 1..k + 1 + op.a as usize, u32::from(op.d.min(2)))
            }
            None => (0..0, 0),
        };
        // The column run's steps once a frame for each column: a 32nd of a
        // pixel's.
        let col = self.column_run();
        // With the JIT the kernel's steps from here (a fused colour it
        // draws in one piece as one cost).
        let start = if jit { self.kernel_start() } else { usize::MAX };
        // A fused run's steps on the VM: as ever, less what its kernel saves.
        let (mut saved, mut px, mut kp) = (0, 0, JIT_LOOP_NS);
        // `layer rate N`: the slow layers' steps a band of rows a frame (an
        // Nth of the rows), on the VM; the fast part's lanes start from the
        // held picture (a row copy).
        // (Slow layers that read no `t` are redrawn when what they read
        // changes: at most once a second for the clock, counted as every
        // 50th frame, or every Nth if N is more.)
        let (slow, rate) = match self.slow_marker() {
            Some(m) => {
                let op = &self.ops[m];
                let n = u32::from(op.d.max(1));
                let every = if op.c & crate::rate::READS_T != 0 {
                    n
                } else {
                    n.max(50)
                };
                (m + 1..m + 1 + op.a as usize, every)
            }
            None => (0..0, 1),
        };
        let mut slow_ns = 0;
        // The pixel section's guards' scenes (bit s: a guard of scene s),
        // for their cost on the rows each is on and off.
        let pixel_guards: u32 = self.ops[self.row_end as usize..self.len()]
            .iter()
            .filter(|o| o.code == code::GUARD)
            .fold(0, |m, o| m | 1 << ((o.c >> 4) & 15));
        // Scenes (`code::GUARD`): each one's steps apart, then the two
        // costliest counted ("the panel budgets for two scenes at once").
        let guards = self.guard_runs();
        let mut scene = [(0u32, 0u32, 0u32, 0u32); 16];
        let mut k = self.row_end as usize;
        while k < self.len() {
            let op = &self.ops[k];
            let s = guards
                .iter()
                .find(|g| g.0.contains(&k) && g.0.start > self.row_end as usize)
                .map_or(0, |g| g.1 as usize);
            let before = (px, kp, saved);
            if skip.contains(&k) {
            } else if slow.contains(&k) {
                slow_ns += self.step_ns(k, op);
            } else if k >= start {
                // (A marker the kernel does not draw in one piece: its
                // steps follow, one at a time.)
                match (op.code == code::FUSE).then(|| crate::fuse::jit_ns(op.d)) {
                    Some(Some(ns)) => {
                        kp += ns;
                        k += op.a as usize;
                    }
                    Some(None) => {}
                    None => kp += self.jit_step_ns(k, op),
                }
            } else {
                if op.code == code::FUSE {
                    saved += crate::fuse::saved(op.d);
                }
                let ns = self.step_ns(k, op);
                px += if col.contains(&k) {
                    ns.div_ceil(H as u32)
                } else {
                    ns
                };
            }
            if s != 0 {
                let sc = &mut scene[s % 16];
                sc.0 += px - before.0;
                sc.1 += kp - before.1;
                sc.2 += saved - before.2;
                (px, kp, saved) = before;
            }
            k += 1;
        }
        // A scene's row steps (its guard's run in the row section).
        let mut row = self.counts().1 as u32;
        for (r, sn) in &guards {
            if r.start <= self.row_end as usize {
                scene[*sn as usize % 16].3 += r.len() as u32;
                row -= r.len() as u32;
            }
        }
        // (The two largest by a scan: a sort's code is ~3 KB of wasm.) For
        // one frame: every scene, by the rows it is on (a row step's 10 µs
        // a frame is 32 rows' worth).
        let (mut top, mut next, mut framed) = (0u32, 0u32, 0u32);
        let (mut top_s, mut next_s) = (usize::MAX, usize::MAX);
        // A kernel guard's zeros and jump on each row its scene is off,
        // its cost on the rows it is on (the VM's: its lanes zeroed).
        let (on_row, off_row) = if jit {
            (
                GUARD_JIT_ON_NS * JIT_PATH_PCT / 100 * W as u32,
                GUARD_JIT_OFF_NS * JIT_PATH_PCT / 100 * W as u32,
            )
        } else {
            (0, GUARD_VM_OFF_NS * W as u32)
        };
        for (s, &(sp, sk, ss, sr)) in scene.iter().enumerate() {
            let p = if jit { sk * JIT_PATH_PCT / 100 } else { sp }.saturating_sub(ss);
            let c = p * (W * H) as u32 / 1000 + sr * 10;
            if let Some(r) = frame.map(|f| &f.rows) {
                let n = u32::from(r[s].min(H as u8));
                framed += (p * W as u32 * n) / 1000 + sr * 10 * n / H as u32;
                if pixel_guards & (1 << s) != 0 {
                    framed += (on_row * n + off_row * (H as u32 - n)) / 1000;
                }
            }
            if c > top {
                (top, next) = (c, top);
                (top_s, next_s) = (s, top_s);
            } else if c > next {
                next = c;
                next_s = s;
            }
        }
        // The two costliest on, the rest off on every row.
        let off_all: u32 = (0..16)
            .filter(|&s| pixel_guards & (1 << s) != 0)
            .map(|s| {
                let row = if s == top_s || s == next_s { on_row } else { off_row };
                row * H as u32 / 1000
            })
            .sum();
        let scenes = if frame.is_some() {
            framed
        } else {
            top + next + off_all
        };
        // `cycle`'s turns: a palette copied each row, on top of a row
        // step's 10 µs a frame.
        let turns = self.ops[self.draw_end as usize..self.row_end as usize]
            .iter()
            .filter(|o| o.code == code::TURN)
            .count() as u32
            * TURN_US;
        // The tweens: 32 entries' (or 16's) mixes once a frame.
        let tweens = self.tweens().len() as u32 * TWEEN_US;
        // The loop itself (a pixel's colour is counted above: `gray(x)` is
        // 45 + 120, as measured); then what the real paths add to the VM's
        // own frame — the effect's copy into the canvas, the look's rows
        // over the picture, a flash cache shared with the decoder: +13-14 %
        // on the device (plasma as an effect 8,217 against 7,254 µs; Plasma
        // screened at 40 % as a look 8,552 against 7,474). With the JIT: a
        // still lane read in place, the kernel's steps x1.3 (jit1-b as
        // effects against their rows' sum), its words a row.
        let (px, extra) = if jit {
            (
                px + JIT_LANE_NS * cached + kp * JIT_PATH_PCT / 100,
                JIT_ROWS_US,
            )
        } else {
            (45 + px + STILL_COPY_NS * cached, 0)
        };
        let px = px.saturating_sub(saved)
            + if slow.is_empty() {
                0
            } else {
                (45 + slow_ns).div_ceil(rate) + HELD_COPY_NS
            };
        ((px * (W * H) as u32) / 1000
            + row * 10
            + turns
            + tweens
            + scenes
            + self.draw_cost_with(lanes, usize::MAX, frame.map(|f| (f.known, f.taken)))
            + self.overlay_us()
            + extra)
            * 115
            / 100
    }

    /// A kernel step's cost, ns a pixel (see `esp32_us_jit`).
    fn jit_step_ns(&self, at: usize, op: &Op) -> u32 {
        use code::*;
        if op.code == GUARD {
            // (Counted by its scene's rows: `estimate_with`.)
            return 0;
        }
        if op.code == DIV_S {
            // Fitted to paint80-ab8b5's div4 rows on the spare (2026-11-05,
            // each over `div4 mul`'s shifts, four a pixel): a constant's
            // magic number 34-39 ns (±2^j, whole numbers), the general
            // steps 89 for a divisor past ±1, 113 at ±1 or more, 115 where
            // it may be under 1. Of a dividend never below 0 (no magnitude
            // or sign: a shift alone for ±2^j, the multiply and its
            // shift): instruction counts.
            let nonneg = op.c & DIV_NONNEG != 0;
            return match self.const_value(op.b, self.len()) {
                Some(b) if nonneg && crate::fixed::DivBy::shift(b).is_some() => 6,
                Some(b) if crate::fixed::DivBy::magic(b).is_some() => {
                    if nonneg {
                        15
                    } else {
                        37
                    }
                }
                Some(b) if b.unsigned_abs() > ONE as u32 => 89,
                Some(b) if b.unsigned_abs() == ONE as u32 => 113,
                Some(_) => 115,
                _ if op.c & DIV_BIG != 0 => 89,
                _ if op.c & DIV_SMALL == 0 => 113,
                _ => jit_value_ns(op.code),
            };
        }
        if !is_colour(op.code) {
            return jit_value_ns(op.code);
        }
        let (kind, blend) = colour_parts(op.code);
        let (v, n) = operands(op);
        let vals = values(op);
        let base = match kind {
            OUT_GRAY => 27,
            OUT_RGB => 80,
            OUT_HSV => 250,
            OUT_PAL if vals > 1 => {
                if self.const_value(op.c, at) == Some(ONE) {
                    45
                } else if self.stepped(at, op.a) && self.stepped(at, op.c) {
                    60
                } else {
                    400
                }
            }
            _ => 39,
        };
        let k = v[n.saturating_sub(1)];
        let konst = !is_lane(k) && k >= S_FIRST_FREE && self.is_const_slot(k, at);
        let covered = n > vals && !(konst && self.init_at(k) >= ONE);
        let mix = match blend {
            None => 0,
            Some(crate::fixed::BLEND_ADD) => 52,
            Some(crate::fixed::BLEND_MULTIPLY) => 103,
            Some(crate::fixed::BLEND_SCREEN) => 142,
            Some(crate::fixed::BLEND_MAX | crate::fixed::BLEND_MIN) => 39,
            Some(_) => 13,
        };
        let cover = match (blend, covered) {
            (Some(_), true) => 77 + if is_lane(k) { 15 } else { 0 },
            _ => 0,
        };
        base + mix + cover
    }

    /// A pixel step's cost, ns a pixel at 128 lanes (see `esp32_us`).
    fn step_ns(&self, at: usize, op: &Op) -> u32 {
        use code::*;
        match op.code {
            GUARD => GUARD_NS,
            c if is_colour(c) => {
                // Measured on the device (psi-probe, 2026-10-26): a
                // colour's write — a lane's `chan` per pixel, ~5 for one
                // value filled along the row — plus its kind, and for a
                // layer the blend loop: one per blend when it covers all,
                // a shared loop with the coverage (and the blend chosen
                // per channel) when it covers less.
                let (kind, blend) = colour_parts(c);
                let (v, n) = operands(op);
                let vals = values(op);
                let lane = v[..vals.min(n)].iter().any(|&o| is_lane(o));
                let write = if lane { 120 } else { 5 };
                // (Of one value: worked out once, filled along the row.)
                let base = match kind {
                    OUT_HSV if lane => 195,
                    OUT_RGB if lane => 120,
                    // `pal(P, v) * k`: three channels scaled. psi-probe on
                    // the spare (p2probe-b): 346 ns a pixel in all for a
                    // k that varies (226 + the write), 166 for k = 1 (the
                    // palette's bytes: 46 + the write). Since psi-speed a
                    // pixel with its neighbour's entry and scale reuses its
                    // colour: cheap when both take few values along a row
                    // (stepped: a floor, a choice, a comparison, or one
                    // value for the row; Tunnel ~80 %, Rotozoom floor ~90 %
                    // reused: 60, ~21 instructions against ~55), dearer
                    // than before when they vary smoothly (speed-g: 427 ns
                    // in all for `pal(fire, x) * (x*0.9)`: 307 + the write).
                    OUT_PAL if lane && vals > 1 => {
                        if self.const_value(op.c, at) == Some(ONE) {
                            46
                        } else if self.stepped(at, op.a) && self.stepped(at, op.c) {
                            60
                        } else {
                            307
                        }
                    }
                    _ => 0,
                };
                // Covered unless the coverage is a constant slot of 1 or
                // more (a lane or an input never is one).
                let k = v[n.saturating_sub(1)];
                let konst = !is_lane(k) && k >= S_FIRST_FREE && self.is_const_slot(k, at);
                let covered = n > vals && !(konst && self.init_at(k) >= ONE);
                let mix = match (blend, covered) {
                    (None, _) => 0,
                    (Some(crate::fixed::BLEND_ADD), false) => 190,
                    (Some(crate::fixed::BLEND_MULTIPLY), false) => 245,
                    (Some(crate::fixed::BLEND_SCREEN), false) => 300,
                    (Some(crate::fixed::BLEND_MAX | crate::fixed::BLEND_MIN), false) => 180,
                    (Some(_), false) => 20,
                    (Some(crate::fixed::BLEND_ADD), true) => 575,
                    (Some(crate::fixed::BLEND_MULTIPLY), true) => 630,
                    (Some(crate::fixed::BLEND_SCREEN), true) => 600,
                    (Some(crate::fixed::BLEND_MAX | crate::fixed::BLEND_MIN), true) => 450,
                    (Some(_), true) => 390,
                };
                write + base + mix
            }
            // A whole constant `z`: four corners, not eight (a guess until
            // psi-probe measures it).
            NOISE if self.const_value(op.c, at).is_some_and(|z| z & 0xffff == 0) => NOISE_FLAT_NS,
            // By a constant ±2^j: a shift towards zero, `shr`'s and a few
            // more (a guess until psi-probe measures it).
            DIV_S if self.const_value(op.b, self.len()).and_then(crate::fixed::DivBy::shift).is_some() => DIV_SHIFT_NS,
            // (paint80-ab8b5: 136 ns a pixel for a constant or a divisor of
            // ±1 or more, 253 for one that may be under 1.)
            DIV_S if op.c & DIV_SMALL != 0 && self.const_value(op.b, self.len()).is_none() => DIV_SMALL_NS,
            c => value_ns(c),
        }
    }

    /// Whether operand `v` of step `at` takes few values along a row: a
    /// scalar, or a lane last written by a floor, a choice or a comparison.
    fn stepped(&self, at: usize, v: u8) -> bool {
        use code::*;
        if !is_lane(v) {
            return true;
        }
        let re = self.row_end as usize;
        let w = (re.min(at)..at).rev().find(|&k| {
            let o = &self.ops[k];
            !is_colour(o.code) && !matches!(o.code, STILL | COLUMN | FUSE) && o.d == v
        });
        // (A shift of a stepped value, or a division by a scalar,
        // `floor(q)/8`, is stepped.)
        if let Some(k) = w.filter(|&k| matches!(self.ops[k].code, SHR | DIV_S)) {
            return self.stepped(k, self.ops[k].a);
        }
        w.map(|k| &self.ops[k]).is_some_and(|o| {
            matches!(
                o.code,
                FLOOR
                    | SELECT
                    | LT
                    | LE
                    | GT
                    | GE
                    | EQ
                    | NE
                    | AND
                    | OR
                    | NOT
                    | STEP
                    | BIT
                    | SPR
                    | CNT_LT..=CNT_GE
            )
        })
    }

    /// Where the still cache's marker is (`code::STILL`), if the formula
    /// has values for it.
    pub(crate) fn still_marker(&self) -> Option<usize> {
        let (re, pe) = (self.row_end as usize, self.len().min(MAX_OPS));
        (re.min(pe)..pe).find(|&k| self.ops[k].code == code::STILL)
    }

    /// `cycle`'s turns: the row section's last steps (none: an empty
    /// range at its end). A look back from the end, once a row: no scan.
    #[inline]
    pub(crate) fn turns(&self) -> core::ops::Range<usize> {
        let (de, re) = (self.draw_end as usize, (self.row_end as usize).min(MAX_OPS));
        let mut k = re;
        while k > de && self.ops[k - 1].code == code::TURN {
            k -= 1;
        }
        k..re
    }

    /// Each scene's guarded runs (`code::GUARD`, in the row and the pixel
    /// sections): the steps it skips, and the scene's number.
    pub(crate) fn guard_runs(&self) -> alloc::vec::Vec<(core::ops::Range<usize>, u8)> {
        let (de, pe) = (self.draw_end as usize, self.len().min(MAX_OPS));
        (de.min(pe)..pe)
            .filter(|&g| self.ops[g].code == code::GUARD)
            .map(|g| {
                let op = &self.ops[g];
                (g + 1..(g + 1 + op.b as usize).min(pe), op.c >> 4)
            })
            .collect()
    }

    /// The tweens (`code::TWEEN`): the frame section's last steps, run
    /// after its values ([`own_palettes`]).
    pub(crate) fn tweens(&self) -> core::ops::Range<usize> {
        let fe = (self.frame_end as usize).min(MAX_OPS);
        let mut k = fe;
        while k > 0 && self.ops[k - 1].code == code::TWEEN {
            k -= 1;
        }
        k..fe
    }

    /// Where the slow layers' marker is (`code::SLOW`), if the formula has
    /// `layer rate N { }`: always the first pixel step (the compiler lays
    /// the slow layers out first, and fuse never puts a marker before it),
    /// so one look, not a scan — it is asked several times a frame, and
    /// per row by the JIT's driver (consoleb-fix: Endless road missed
    /// deadlines with the scans).
    #[inline]
    pub(crate) fn slow_marker(&self) -> Option<usize> {
        let re = self.row_end as usize;
        (re < self.len().min(MAX_OPS) && self.ops[re].code == code::SLOW).then_some(re)
    }

    /// The rows of the slow layers a frame redraws in steady play at its
    /// grid: a band's (`ceil(32/N)`) when they read `t`, else none (they
    /// are redrawn when what they read changes). `None` without `rate`.
    /// The player's governor counts the slow rows a frame drew beyond this
    /// (a slower grid has more bands due a frame) at this share, so a
    /// slower grid does not look dearer a frame than the faster one.
    pub fn rate_rows(&self) -> Option<u32> {
        self.slow_marker().map(|m| {
            let op = &self.ops[m];
            if op.c & crate::rate::READS_T != 0 {
                (H as u32).div_ceil(u32::from(op.d.max(1)))
            } else {
                0
            }
        })
    }

    /// The first pixel step after the slow layers' (`layer rate N`: they
    /// are laid out first, after their marker; `row_end` without them).
    pub(crate) fn fast_start(&self) -> usize {
        match self.slow_marker() {
            Some(m) => (m + 1 + self.ops[m].a as usize).min(self.len()),
            None => self.row_end as usize,
        }
    }

    /// `layer rate N { }`'s N: its slow layers are redrawn every Nth frame
    /// of its grid, a band of rows a frame (`None`: every layer every
    /// frame). A player keeps their picture in a [`crate::Held`].
    pub fn rate(&self) -> Option<u32> {
        self.slow_marker().map(|m| u32::from(self.ops[m].d))
    }

    /// The column run's steps (`code::COLUMN`), if the formula has one.
    pub(crate) fn column_run(&self) -> core::ops::Range<usize> {
        let (re, pe) = (self.row_end as usize, self.len().min(MAX_OPS));
        match (re.min(pe)..pe).find(|&k| self.ops[k].code == code::COLUMN) {
            Some(k) => k + 1..(k + 1 + self.ops[k].a as usize).min(pe),
            None => 0..0,
        }
    }

    /// Values the still cache holds for this formula (0, 1 or 2): each a
    /// lane of 4,096 values ([`crate::StillLane`], 16 KB) worked out once
    /// instead of every frame. See [`crate::Still`].
    pub fn still_lanes(&self) -> usize {
        self.still_marker().map_or(0, |k| {
            (self.ops[k].d as usize).min(crate::still::STILL_LANES)
        })
    }

    /// The draw stage's part of [`esp32_us`](Self::esp32_us), from
    /// `psi-probe` on the spare (2026-10-27, 1.0.0-beta.11-drawprobe-a,
    /// tests/measured.rs `DRAW`): a step run ~420 ns (a loop's END and its
    /// body's values, each as often as it runs), a dot 400, a line 1,100 +
    /// 320 a pixel walked (its longer side when a side's ends are
    /// constants, at least 12 with a side unknown), and
    /// 10 µs to clear the canvas. (The design guessed 300, 400 and 1,500 +
    /// 200.)
    pub fn draw_us(&self) -> u32 {
        self.draw_cost(true, usize::MAX)
    }

    /// Each draw loop in order: its count, and for one that runs as lanes
    /// the draw stage's cost (µs) as it is and with that loop turned one
    /// turn at a time (the language service's lane note).
    #[cfg(feature = "syntax")]
    pub(crate) fn draw_loops(&self) -> alloc::vec::Vec<(u32, Option<(u32, u32)>)> {
        let (fe, de) = (
            self.frame_end as usize,
            (self.draw_end as usize).min(MAX_OPS),
        );
        let all = self.draw_cost(true, usize::MAX);
        (fe.min(de)..de)
            .filter(|&k| self.ops[k].code == code::FOR)
            .map(|k| {
                let count = (self.init_at(self.ops[k].a) >> 16) as u32;
                (
                    count,
                    self.vector_loop(k).map(|_| (all, self.draw_cost(true, k))),
                )
            })
            .collect()
    }

    /// The paint primitives' part of [`draw_us`](Self::draw_us), µs (each
    /// [`paint_ns`](Self::paint_ns) as often as it runs): psi-probe's paint
    /// rows print it beside the draw stage they measure.
    #[doc(hidden)]
    pub fn paint_us(&self) -> u32 {
        let mut ns = 0u64;
        self.draw_walk(|at, op, mult| {
            if op.code == code::PAINT {
                ns = ns.saturating_add(self.paint_ns(at) * u64::from(mult));
            }
        });
        (ns / 1000).min(u32::MAX as u64) as u32
    }

    /// The paint plane's overlay in [`esp32_us`](Self::esp32_us), µs a
    /// frame before the real paths' 15 %: [`PAINT_FULL_US`] for a formula
    /// that clears the plane, half-way from [`PAINT_ROWS_US`] for one that
    /// paints without, 0 for one that doesn't paint.
    #[doc(hidden)]
    pub fn overlay_us(&self) -> u32 {
        if !self.paints() {
            0
        } else if self.clears_paint() {
            PAINT_FULL_US
        } else {
            (PAINT_ROWS_US + PAINT_FULL_US) / 2
        }
    }

    /// Whether the draw stage clears the paint plane to a colour (`cls`):
    /// then the overlay writes every pixel.
    fn clears_paint(&self) -> bool {
        let end = (self.draw_end as usize).min(MAX_OPS);
        let first = (self.frame_end as usize).min(end);
        self.ops[first..end]
            .iter()
            .any(|o| o.code == code::PAINT && o.d == paint::CLS)
    }

    /// The pixel steps that read the picture elsewhere (`under(x, y)`, a
    /// step a channel): psi-probe's `UAT` rows.
    #[doc(hidden)]
    pub fn uat_steps(&self) -> usize {
        self.ops[self.row_end as usize..self.len()]
            .iter()
            .filter(|op| op.code == code::UAT)
            .count()
    }

    /// [`draw_us`](Self::draw_us), with (`lanes`) or without the loops run
    /// as lanes (and the one whose `FOR` is at `turned` a turn at a time).
    fn draw_cost(&self, lanes: bool, turned: usize) -> u32 {
        self.draw_cost_with(lanes, turned, None)
    }

    /// [`draw_cost`](Self::draw_cost) for one frame when `ifs` says how its
    /// `if`s outside loops went (bit k of the second for the k-th such `if`,
    /// known where bit k of the first is set): the arm taken, else (not
    /// known) the costlier.
    fn draw_cost_with(&self, lanes: bool, turned: usize, ifs: Option<(u32, u32)>) -> u32 {
        if !self.draws() {
            return 0;
        }
        let mut ns: u64 = 10_000;
        let mut from = [0u8; 2];
        // Loops run as lanes (`vector_loop`): their value steps once a
        // lane of turns each (as a pixel step costs a pixel, over 128
        // turns) at the FOR, then their ENDs and values free (up to
        // `lane_end`); lines and dots as ever.
        let mut lane_end = 0;
        let mut lane: Option<VecLoop> = None;
        // `if … { } else { }`: only the costlier arm a frame. Each open one:
        // (the else arm's first step, the end, `ns` at the IF, the then
        // arm's cost once its JUMP is reached).
        let mut arms = [(0usize, 0usize, 0u64, 0u64); 4];
        // For one frame (`ifs`): each open arm's way, 1 taken, 2 not, 0 not
        // known; and the `if`s outside loops counted, and the loops' depth.
        let mut way = [0u8; 4];
        let (mut nth, mut depth) = (0u32, 0usize);
        let mut open = 0usize;
        type Arms = [(usize, usize, u64, u64); 4];
        fn close(arms: &Arms, way: &[u8; 4], open: &mut usize, at: usize, ns: &mut u64) {
            loop {
                if *open == 0 {
                    break;
                }
                let (start, end, ns0, then) = arms[*open - 1];
                let w = way[*open - 1];
                if end > 0 && at >= end {
                    *ns = ns0
                        + match w {
                            1 => then,
                            2 => *ns - ns0,
                            _ => then.max(*ns - ns0),
                        };
                } else if end == 0 && at == start && w != 0 {
                    // (An `if` with no `else`, its way known: its steps
                    // only when taken.)
                    if w == 2 {
                        *ns = ns0;
                    }
                } else {
                    break;
                }
                *open -= 1;
            }
        }
        let end = (self.draw_end as usize).min(MAX_OPS);
        self.draw_walk(|at, op, mult| {
            close(&arms, &way, &mut open, at, &mut ns);
            match op.code {
                code::FOR => depth += 1,
                code::END => depth = depth.saturating_sub(1),
                _ => {}
            }
            match op.code {
                code::IF if open < arms.len() => {
                    arms[open] = (op.target(), 0, ns, 0);
                    way[open] = match ifs {
                        Some((known, taken)) if depth == 0 && nth < 32 && known & (1 << nth) != 0 => {
                            if taken & (1 << nth) != 0 { 1 } else { 2 }
                        }
                        _ => 0,
                    };
                    if depth == 0 {
                        nth += 1;
                    }
                    open += 1;
                    return;
                }
                code::IF => {
                    if depth == 0 {
                        nth += 1;
                    }
                }
                code::JUMP if open > 0 && arms[open - 1].0 == at + 1 && arms[open - 1].1 == 0 => {
                    let a = &mut arms[open - 1];
                    (a.1, a.3) = (op.target(), ns - a.2);
                    ns = a.2;
                    return;
                }
                _ => {}
            }
            if let Some(v) = self.vector_loop(at).filter(|_| lanes && at != turned) {
                lane = Some(v);
                let chunks = u64::from(v.count.div_ceil(W as u32));
                let mut steps = 0u64;
                for o in &self.ops[at + 1..v.end] {
                    if code::writes_scalar(o.code) {
                        let each =
                            value_ns(o.code) + if lane_loop(o.code) { 0 } else { LANE_EVAL_NS };
                        steps += u64::from(each) * W as u64 + LANE_STEP_NS;
                    }
                }
                let turns = u64::from(v.count) * LANE_TURN_NS;
                ns = ns
                    .saturating_add((420 + chunks * (steps + LANE_CHUNK_NS) + turns) * mult as u64);
                lane_end = v.end;
                return;
            }
            if at <= lane_end && !matches!(op.code, code::LINE_FROM | code::LINE_TO | code::DOT) {
                return;
            }
            // A line's or a dot's operands read from the lanes, each turn.
            if let Some(v) = lane.filter(|_| at <= lane_end) {
                let reads = [op.a, op.b, op.c]
                    .iter()
                    .filter(|&&o| v.reg(o).is_some())
                    .count();
                ns = ns.saturating_add(reads as u64 * LANE_READ_NS * mult as u64);
            }
            let cost: u64 = match op.code {
                code::LINE_FROM => {
                    from = [op.a, op.b];
                    0
                }
                code::LINE_TO => {
                    // Its pixels: the longer side + 1 where both ends of a side
                    // are constants; with a side unknown, at least 12.
                    let k = |v: u8| self.const_value(v, at);
                    let d = |a: Option<i32>, b: Option<i32>| {
                        a.zip(b).map(|(a, b)| {
                            ((a >> 16) - (b >> 16)).unsigned_abs().min(1023) as u64 + 1
                        })
                    };
                    let (dx, dy) = (d(k(from[0]), k(op.a)), d(k(from[1]), k(op.b)));
                    let len = match (dx, dy) {
                        (Some(a), Some(b)) => a.max(b),
                        (a, b) => a.or(b).unwrap_or(0).max(6),
                    };
                    1_100 + 270 * len
                }
                code::DOT => 450,
                code::PAINT => self.paint_ns(at),
                _ => 420,
            };
            ns = ns.saturating_add(cost * mult as u64);
        });
        close(&arms, &way, &mut open, end, &mut ns);
        (ns / 1000).min(u32::MAX as u64) as u32
    }

    /// A paint primitive's cost (the `PAINT` at `at`), ns: a call, its
    /// kind's setup, and the rows and pixels it walks (`draw::paint_prim`:
    /// a box on the panel, a line's steps, a sprite's or a text's cells) at
    /// its kind's cost each — counted where its values are constants, else
    /// a typical size. A ring also tests its neighbours where it is inside.
    /// Fitted to the spare (paint-probe, 2026-11-04, 80 MHz flash: each kind
    /// 16 times a frame at three sizes, the draw stage alone; within ~5 %
    /// of every row).
    fn paint_ns(&self, at: usize) -> u64 {
        use crate::program::paint::*;
        let kind = self.ops[at].d.min(LAST) as usize;
        // Its first six values (the ARGS steps before it, then its own),
        // whole pixels where constant.
        let first = (at.saturating_sub(2)..at)
            .rev()
            .take_while(|&k| self.ops[k].code == code::ARGS)
            .last()
            .unwrap_or(at);
        let mut v = [None; 6];
        let mut n = 0;
        for o in &self.ops[first..=at] {
            for s in [o.a, o.b, o.c] {
                if n < 6 {
                    v[n] = self.const_value(s, at).map(|x| x >> 16);
                    n += 1;
                }
            }
        }
        // The pixels on the panel from the least to the most of values
        // `ix`, `m` more each way, held to `lim`.
        let side = |ix: &[usize], m: i32, lim: i32| -> Option<u64> {
            let (mut lo, mut hi) = (i32::MAX, i32::MIN);
            for &i in ix {
                let x = v[i]?;
                (lo, hi) = (lo.min(x), hi.max(x));
            }
            Some(((hi + m).min(lim - 1) - (lo - m).max(0) + 1).max(0) as u64)
        };
        let (w, h) = (W as i32, H as i32);
        // (pixels walked, rows walked, pixels inside a ring) where known.
        let walk: Option<(u64, u64, u64)> = match kind as u8 {
            CLS => Some((W as u64 * H as u64, 0, 0)),
            PSET => Some((1, 0, 0)),
            LINE => side(&[0, 2], 0, i32::MAX)
                .zip(side(&[1, 3], 0, i32::MAX))
                .map(|(a, b)| (a.max(b), 0, 0)),
            RECT | RECTFILL => side(&[0, 2], 0, w)
                .zip(side(&[1, 3], 0, h))
                .map(|(a, b)| (a * b, b, 0)),
            TRIFILL => side(&[0, 2, 4], 1, w)
                .zip(side(&[1, 3, 5], 1, h))
                .map(|(a, b)| (a * b, b, 0)),
            CIRC | CIRCFILL => v[2].map(|r| {
                // (Its box: from the centre's pixel less rr to rr + 1 past it.)
                let rr = r.max(0) + 1;
                let span = |c: Option<i32>, lim: i32| match c {
                    Some(c) => ((c + rr + 1).min(lim - 1) - (c - rr).max(0) + 1).max(0) as u64,
                    None => (2 * rr + 2).min(lim) as u64,
                };
                let (cols, rows) = (span(v[0], w), span(v[1], h));
                let box_ = cols * rows;
                let disc = (r.max(0) as u64 * 2 + 1).pow(2) * 785 / 1000;
                (box_, rows, disc.min(box_))
            }),
            SPR => v[0].map(|at| {
                let hd = SprHead::new(&self.tables, at as u8);
                (hd.w as u64 * hd.h as u64, hd.h as u64, 0)
            }),
            PRINT => v[1].zip(v[2]).map(|(len, font)| {
                let cell = if font == 0 { 35 } else { 24 };
                (len.clamp(0, 32) as u64 * cell, 0, 0)
            }),
            _ => None,
        };
        // Each kind: its setup (beyond the call), a row, a pixel (ns), and
        // its typical walk (pixels, rows) when its values are not constants.
        const SETUP: [u64; 11] = [1_300, 0, 160, 400, 400, 3_100, 3_100, 700, 0, 700, 4_000];
        const ROW: [u64; 11] = [0, 0, 0, 300, 240, 0, 0, 600, 0, 0, 0];
        const EACH: [u64; 11] = [1, 125, 314, 126, 146, 330, 435, 830, 345, 140, 140];
        const TYPICAL: [(u64, u64); 11] = [
            (4096, 0),
            (1, 0),
            (16, 0),
            (80, 10),
            (400, 20),
            (400, 20),
            (144, 12),
            (600, 20),
            (64, 8),
            (175, 0),
            (96, 0),
        ];
        let (px, rows, inside) = walk.unwrap_or((TYPICAL[kind].0, TYPICAL[kind].1, TYPICAL[kind].0 / 2));
        // A ring's pixels inside test four neighbours more.
        let ring = if kind as u8 == CIRC { inside * RING_INSIDE_NS } else { 0 };
        PAINT_CALL_NS + SETUP[kind] + rows * ROW[kind] + px * EACH[kind] + ring
    }

    /// The value of slot `v` as step `at` reads it if it holds a constant
    /// there, `None` for anything else (a slot holds a constant until a step
    /// writes another value into it: slots are used again).
    pub(crate) fn const_value(&self, v: u8, at: usize) -> Option<i32> {
        (!is_lane(v) && v >= S_FIRST_FREE && self.is_const_slot(v, at)).then(|| self.init_at(v))
    }

    /// Scalar slot `v`'s value at the start of a frame (0 past the slots).
    #[inline(always)]
    pub(crate) fn init_at(&self, v: u8) -> i32 {
        self.init.get(v as usize).copied().unwrap_or(0)
    }

    /// Reset a program in place to the empty one (all fields zero but the
    /// defaults), for a zeroed allocation.
    pub fn clear(&mut self) {
        self.ops.fill(Op::default());
        self.init.fill(0);
        self.frame_end = 0;
        self.draw_end = 0;
        self.row_end = 0;
        self.pixel_end = 0;
        self.n_scalars = S_FIRST_FREE;
        self.regs = 0;
        self.uses = 0;
        self.frame_ms = 0;
        self.loop_ms = 8000;
        self.tables.fill(0);
        self.meta = crate::meta::Meta::default();
        self.paint.fill(0);
        self.pals.fill(0);
        self.ins = Ins::default();
    }

    /// The built-in inputs it reads, as (key, what: `world::VALUE`,
    /// `SINCE` or `WAS`), in slot order.
    pub fn builtins(&self) -> impl Iterator<Item = (&'static crate::world::Key, u8)> + '_ {
        self.ins
            .iter()
            .filter_map(|(_, b)| Some((crate::world::key_by_id(b & 63)?, b >> 6)))
    }

    /// What the formula declares: its controls (defaults for the slots a
    /// path leaves unset), and whether it may be a look.
    pub fn meta(&self) -> &crate::meta::Meta {
        &self.meta
    }

    /// A readable listing of the steps (tests, the bench, debugging).
    pub fn listing(&self) -> alloc::string::String {
        let mut s = alloc::string::String::new();
        for n in 0..self.len() {
            s.push_str(&self.listing_line(n));
        }
        s
    }

    /// Step `n` of [`listing`](Self::listing), its line with the `\n`: the
    /// firmware's JIT logs a program a few lines at a time this way, with
    /// no listing of the whole on its heap.
    pub fn listing_line(&self, n: usize) -> alloc::string::String {
        use core::fmt::Write;
        let mut s = alloc::string::String::new();
        let Some(op) = self.ops[..self.len()].get(n) else {
            return s;
        };
        let opnd = |v: u8, n: usize| -> alloc::string::String {
            if is_lane(v) {
                match lane_no(v) {
                    L_X => "x".into(),
                    L_PX => "px".into(),
                    L_I => "i".into(),
                    r => alloc::format!("L{}", r - L_REG0),
                }
            } else if v < S_FIRST_FREE {
                ["t", "hour", "minute", "second", "a", "b", "c", "y", "py"][v as usize].into()
            } else if v == S_SEED && self.uses & SEEDED != 0 {
                "seed".into()
            } else if let Some((_, b)) = self.ins.iter().find(|&(s, _)| s == v) {
                let name = crate::world::key_by_id(b & 63).map_or("?", |k| k.name);
                match b >> 6 {
                    crate::world::SINCE => alloc::format!("since({name})"),
                    crate::world::WAS => alloc::format!("was({name})"),
                    _ => name.into(),
                }
            } else if (v as usize) < MAX_SCALARS && self.is_const_slot(v, n) {
                alloc::format!("{}", self.init[v as usize] as f64 / ONE as f64)
            } else {
                alloc::format!("s{v}")
            }
        };
        {
            let sect = if n < self.frame_end as usize {
                "frame"
            } else if n < self.draw_end as usize {
                "draw "
            } else if n < self.row_end as usize {
                "row  "
            } else {
                "pixel"
            };
            let (v, ar) = code::operands(op);
            let args = if code::is_wide(op.code) {
                // Wide slots `wN`, scalar ones as usual; a compare's kind.
                let (_, nv, w) = code::wide_ops(op.code);
                let mut a: alloc::vec::Vec<alloc::string::String> = [op.a, op.b, op.c][..nv]
                    .iter()
                    .enumerate()
                    .map(|(k, &x)| {
                        if w & (1 << k) != 0 {
                            alloc::format!("w{x}")
                        } else {
                            opnd(x, n)
                        }
                    })
                    .collect();
                if matches!(op.code, code::W_CMP | code::W_MINMAX) {
                    a.push(code::name(op.c).into());
                }
                a.join(", ")
            } else {
                v[..ar]
                    .iter()
                    .map(|&v| opnd(v, n))
                    .collect::<alloc::vec::Vec<_>>()
                    .join(", ")
            };
            // A table read: where its entries are; a loop: its counter and
            // where it ends or goes back to.
            let args = if op.code == code::TAB {
                alloc::format!("{args}; table {}..={}", op.b, op.b as usize + op.c as usize)
            } else if op.code == code::SPR {
                let h = SprHead::new(&self.tables, op.c);
                alloc::format!("{args}; sprite {} x {} at {}", h.w, h.h, op.c)
            } else if op.code == code::FOR {
                alloc::format!("{args}; end {}", op.target())
            } else if op.code == code::END {
                alloc::format!("{args}; back to {}", op.target())
            } else if matches!(op.code, code::IF | code::JUMP) {
                alloc::format!("{args}; to {}", op.target())
            } else if op.code == code::FUSE {
                alloc::format!("{}; {} steps", crate::fuse::name(op.d), op.a)
            } else if op.code == code::TURN {
                alloc::format!("{args}; palette {}", op.b)
            } else if op.code == code::GUARD {
                alloc::format!(
                    "{args}; skip {}, scene {}, mask {:#05x}",
                    op.b,
                    op.c >> 4,
                    guard_mask(op)
                )
            } else if op.code == code::TWEEN {
                let src = |v: u8| match v {
                    v if v < TW_LIVE => alloc::format!("store{v}"),
                    v if v < TW_TEMP => alloc::format!("pal{}", 5 + v - TW_LIVE),
                    v => alloc::format!("work{}", v - TW_TEMP),
                };
                let half = match op.d & (TW_LOW | TW_HIGH) {
                    TW_LOW => " low",
                    TW_HIGH => " high",
                    _ => "",
                };
                alloc::format!("{}, {}, {args}{half}", src(op.a), src(op.b))
            } else if op.code == code::SLOW {
                alloc::format!(
                    "skip {} in the fast part; {} layers, rate {}, reads {:#x}",
                    op.a,
                    op.b,
                    op.d,
                    op.c
                )
            } else if op.code == code::COLUMN {
                alloc::format!("skip {} after the first row", op.a)
            } else if op.code == code::STILL {
                let r = |v: u8| opnd(v, n);
                let regs = [r(op.b), r(op.c)];
                alloc::format!(
                    "skip {}; cache {}",
                    op.a,
                    regs[..(op.d as usize).min(2)].join(", ")
                )
            } else if op.code == code::PICK {
                let t: alloc::vec::Vec<_> = (0..op.c as usize)
                    .map(|j| opnd(self.init_at((op.b as usize + j) as u8) as u8, n))
                    .collect();
                alloc::format!("{args}; {}", t.join(", "))
            } else {
                args
            };
            let d = if code::is_colour(op.code)
                || matches!(
                    op.code,
                    code::END
                        | code::IF
                        | code::JUMP
                        | code::GUARD
                        | code::LINE_FROM
                        | code::LINE_TO
                        | code::DOT
                        | code::STILL
                        | code::COLUMN
                        | code::FUSE
                ) {
                "out".into()
            } else if op.code == code::TURN {
                alloc::format!("pal{}", 5 + op.d as usize)
            } else if op.code == code::TWEEN {
                match (op.d & 7) as usize {
                    d if d < USER_PALETTES => alloc::format!("pal{}", 5 + d),
                    d => alloc::format!("work{}", d - USER_PALETTES),
                }
            } else if op.code == code::SLOW {
                "out".into()
            } else if code::is_wide(op.code) && code::wide_ops(op.code).0 {
                alloc::format!("w{}", op.d)
            } else if op.code == code::W_LAT {
                let k = (op.d as usize).saturating_sub(noise_slot(0, 0));
                alloc::format!("noise{}.{}", k / 3, ["x", "y", "z"][k % 3])
            } else {
                // (What the step writes: after it.)
                opnd(op.d, n + 1)
            };
            let blend = match code::colour_parts(op.code) {
                (_, Some(b)) if code::is_colour(op.code) => {
                    ["over", "add", "multiply", "screen", "max", "min"][(b as usize).min(5)]
                }
                _ => "",
            };
            let _ = writeln!(
                s,
                "{n:3} {sect} {d} {blend}= {}({args})",
                code::name(op.code)
            );
        }
        s
    }

    /// Draw loops that run as lanes ([`vector_loop`](Self::vector_loop)):
    /// psi-check's listing and the tests.
    #[doc(hidden)]
    pub fn lane_loops(&self) -> usize {
        let (fe, de) = (
            self.frame_end as usize,
            (self.draw_end as usize).min(MAX_OPS),
        );
        (fe.min(de)..de)
            .filter(|&k| self.vector_loop(k).is_some())
            .count()
    }

    /// Whether the draw loop whose `FOR` is at `at` runs as lanes (a lane
    /// a turn, `crate::draw::run`): `None` for one that runs a turn at a
    /// time. Its body is run step by step over every turn at once, then
    /// its lines and dots are drawn turn by turn, in order — the same
    /// canvas, since no turn reads another's values (checked here, not
    /// assumed):
    ///
    /// - at least [`VEC_MIN`] turns (its count a constant), no loop inside;
    /// - every body step a value step (`writes_scalar`, not a wide one) or
    ///   a line or a dot, its value written into a slot other than the
    ///   counter;
    /// - no step reads a slot the body writes before the body has written
    ///   it in that turn (a value of the turn before);
    /// - no line or dot reads a slot the body writes after it (it is drawn
    ///   after every value step has run);
    /// - the counter and the slots the body writes fit the lane
    ///   registers ([`REGS`]), with one to spare for a step that writes a
    ///   slot it reads.
    ///
    /// Decided once a program, after the compiler (`mark_lane_loops`, kept
    /// on the FOR as [`LANES`]); this reads that mark and lays the
    /// registers out (one pass over the body), each time a frame runs the
    /// loop. The estimate reads the same.
    #[cfg_attr(all(target_os = "espidf", feature = "sram1-iram"), link_section = ".iram1.s1")]
    pub(crate) fn vector_loop(&self, at: usize) -> Option<VecLoop> {
        let f = self.ops.get(at).filter(|o| o.code == code::FOR)?;
        if f.c & LANES == 0 {
            return None;
        }
        let end = f.target();
        let mut vl = VecLoop {
            end,
            count: (self.init_at(f.a) >> 16) as u32,
            regs: 1,
            slot: [0; REGS + 1],
        };
        vl.slot[1] = f.d;
        for op in &self.ops[at + 1..end] {
            if code::writes_scalar(op.code) && vl.reg(op.d).is_none() {
                vl.regs += 1;
                vl.slot[(vl.regs as usize).min(REGS)] = op.d;
            }
        }
        debug_assert_eq!(
            Some((vl.end, vl.count, vl.regs, vl.slot)),
            self.check_vector_loop(at)
                .map(|v| (v.end, v.count, v.regs, v.slot))
        );
        Some(vl)
    }

    /// What every program gets after it is laid out, by the compiler or by
    /// the player after a portable program is loaded ([`crate::psb`]): the
    /// narrowed steps (`narrow`), the fused runs (`fuse`), and the draw
    /// loops that run as lanes, decided once.
    #[inline(never)]
    /// False when the heap had no room for the narrowing's working memory
    /// (4 KB: nothing changed then).
    pub(crate) fn finish(&mut self, narrow: bool, fuse: bool) -> bool {
        if narrow {
            if !crate::range::narrow(self) {
                return false;
            }
            if fuse {
                crate::fuse::fuse(self);
            }
        }
        self.mark_lane_loops();
        true
    }

    /// Mark each draw loop that may run as lanes (its FOR's `c`,
    /// [`LANES`]): [`check_vector_loop`](Self::check_vector_loop) once a
    /// program, after the compiler has laid it out (and narrowed and fused
    /// its pixel steps, which it does not read).
    pub(crate) fn mark_lane_loops(&mut self) {
        let (fe, de) = (
            self.frame_end as usize,
            (self.draw_end as usize).min(MAX_OPS),
        );
        for at in fe.min(de)..de {
            if self.ops[at].code == code::FOR {
                let lanes = self.check_vector_loop(at).is_some();
                let c = self.ops[at].c & !LANES;
                self.ops[at].c = if lanes { c | LANES } else { c };
            }
        }
    }

    /// [`vector_loop`](Self::vector_loop)'s rules, checked.
    fn check_vector_loop(&self, at: usize) -> Option<VecLoop> {
        let de = (self.draw_end as usize).min(MAX_OPS);
        let f = self.ops.get(at).filter(|o| o.code == code::FOR)?;
        let end = f.target();
        if end <= at || end >= de {
            return None;
        }
        let e = &self.ops[end];
        let k = f.d;
        if e.code != code::END || e.a != k || e.b != f.a || e.target() != at + 1 {
            return None;
        }
        if is_lane(f.a) || !self.is_const_slot(f.a, at) {
            return None;
        }
        let count = self.init_at(f.a) >> 16;
        if count < VEC_MIN as i32 {
            return None;
        }
        let body = &self.ops[at + 1..end];
        let prim = |c: u8| matches!(c, code::LINE_FROM | code::LINE_TO | code::DOT);
        let value = |c: u8| {
            matches!(
                c,
                0..=code::LAST_BIN
                    | 32..=code::LAST_UN
                    | 64..=code::LAST_TERN
                    | code::CMP_FIRST..=code::CMP_LAST
            ) && code::writes_scalar(c)
        };
        // Registers: the counter first, then each slot the body writes as
        // it is first written.
        let mut vl = VecLoop {
            end,
            count: count as u32,
            regs: 1,
            slot: [0; REGS + 1],
        };
        vl.slot[1] = k;
        for op in body {
            if value(op.code) {
                if op.d == k {
                    return None;
                }
                if vl.reg(op.d).is_none() {
                    if vl.regs as usize == REGS {
                        return None;
                    }
                    vl.regs += 1;
                    vl.slot[vl.regs as usize] = op.d;
                }
            } else if !prim(op.code) {
                return None;
            }
        }
        let bit = |v: u8| vl.reg(v).map_or(0u16, |r| 1 << r);
        // Each read after the turn's own write (the counter's is the
        // FOR's); one register to spare when a step writes a slot it reads.
        let mut done = 2u16;
        let mut spare = false;
        for op in body {
            let n = code::arity(op.code);
            let v = [op.a, op.b, op.c];
            if v[..n].iter().any(|&v| bit(v) & !done != 0) {
                return None;
            }
            // A pick's choices are read through its table of slots, as
            // scalars: none may be one the loop writes. (Unreachable from
            // the compiler, kept as a guard: a pick with a choice worked
            // out per turn is compares and selects, `Dag::pick`, so a
            // PICK step's choices are all once-a-frame values.)
            if op.code == code::PICK
                && (0..op.c as usize)
                    .any(|j| bit(self.init_at((op.b as usize + j) as u8) as u8) != 0)
            {
                return None;
            }
            if value(op.code) {
                spare |= v[..n].contains(&op.d);
                done |= bit(op.d);
            }
        }
        // Lines and dots read nothing the body writes after them. (Also
        // unreachable from the compiler, which emits a body's values before
        // its lines: a guard, as the turn cap in `draw::run`.)
        let mut later = 0u16;
        for op in body.iter().rev() {
            if prim(op.code) {
                if [op.a, op.b, op.c].iter().any(|&v| bit(v) & later != 0) {
                    return None;
                }
            } else {
                later |= bit(op.d);
            }
        }
        if spare && vl.regs as usize == REGS {
            return None;
        }
        Some(vl)
    }

    pub(crate) fn is_const_slot(&self, v: u8, at: usize) -> bool {
        (v != S_SEED || self.uses & SEEDED == 0)
            && !self.ins.holds(v)
            && !self.ops[..at.min(self.len())].iter().any(|op| {
                // END counts its loop's counter (which the FOR writes too).
                (op.code == code::END && op.a == v) || (code::writes_scalar(op.code) && op.d == v)
            })
    }
}

/// A draw loop's FOR's mark when it runs as lanes, the top bit of its `c`
/// ([`Program::vector_loop`]; clear when a turn at a time; the bits below
/// are its END's index's high byte, [`Op::target`]).
pub(crate) const LANES: u8 = 0x80;

/// Turns a draw loop needs to run as lanes ([`Program::vector_loop`]):
/// below it a turn at a time is as quick (each lane step works out a whole
/// lane, whatever the count).
pub(crate) const VEC_MIN: u32 = 32;

/// A draw loop run as lanes, ns: each value step's dispatch for a lane of
/// turns (its work is `value_ns` a turn, as a pixel step's a pixel), the
/// counter's lane and the loop round them, and each turn's look-up of its
/// lines' and dots' operands (guesses until psi-probe measures them).
pub(crate) const LANE_STEP_NS: u64 = 800;
pub(crate) const LANE_CHUNK_NS: u64 = 1_000;
/// (s2probe-a, 2026-10-28: a loop of 1,024 off-panel dots 722 ns a turn
/// over the base as lanes, the dot ~420 of it; +70 for a dot reading a
/// lane: `LANE_READ_NS` a line's or dot's operand from a lane.)
pub(crate) const LANE_TURN_NS: u64 = 300;
pub(crate) const LANE_READ_NS: u64 = 70;
/// A draw lane step without a lane loop of its own (`vm::lane_draw`'s
/// other steps, worked out turn by turn by the scalar functions): the
/// calls and their dispatch a turn, ns (s3probe-a, 2026-10-28: `noise`,
/// `digit`, `sqrt` and `step` in a loop of 1,024, 5,270 µs over the base;
/// ~510 a step over their `value_ns`. It was a guess of 350.)
pub(crate) const LANE_EVAL_NS: u32 = 500;

/// Whether `vm::lane_draw` runs step `c` with a lane loop (else turn by
/// turn): its list, kept in step with it.
pub(crate) fn lane_loop(c: u8) -> bool {
    use code::*;
    matches!(
        c,
        ADD | SUB
            | MUL
            | DIV
            | RECIP
            | MOD
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
            | HASH
            | NEG
            | NOT
            | ABS
            | FLOOR
            | FRAC
            | SIN
            | COS
            | TAB
            | SELECT
            | CLAMP
            | SEL_LT..=CNT_GE
    )
}

/// A draw loop that runs as lanes ([`Program::vector_loop`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct VecLoop {
    /// Its `END`'s index.
    pub(crate) end: usize,
    /// Its count (≥ [`VEC_MIN`]).
    pub(crate) count: u32,
    /// Registers used: 1 (the counter) ..= `regs`.
    pub(crate) regs: u8,
    /// The slot each register holds (`slot[1]` the counter's).
    pub(crate) slot: [u8; REGS + 1],
}

impl VecLoop {
    /// The register holding slot `v`, if the body writes it (or it is the
    /// counter).
    #[inline(always)]
    pub(crate) fn reg(&self, v: u8) -> Option<usize> {
        let mut r = self.regs as usize;
        while r > 0 && self.slot[r] != v {
            r -= 1;
        }
        (r > 0).then_some(r)
    }
}

/// A `rate` formula's fast part starts each row from the held picture: a
/// 384 B copy a row, ns a pixel (a guess until psi-probe measures it).
pub(crate) const HELD_COPY_NS: u32 = 5;

/// `cycle`'s turn of a palette, µs a frame over a row step's 10: 32 rows
/// of a 32-entry copy (consoleb-a on the spare: `cycle(fire, x*3, t)` 838
/// µs a frame against `pal(fire, frac(x*3))`'s 735).
pub(crate) const TURN_US: u32 = 90;
/// A tween of a palette once a frame (`code::TWEEN`: 32 entries, 96
/// mixes on bytes), µs: paint80-713cd on the spare, p0-tween's frame
/// steps (six) and its four tweens with the store's unpacking 44 µs a frame.
pub(crate) const TWEEN_US: u32 = 9;
/// A scene's guard in the pixel section, ns a pixel: one test a lane of
/// 128, and while it is off its values' lanes zeroed (a guess until
/// paint-probe measures it); in the kernel a test and a branch a pixel.
pub(crate) const GUARD_NS: u32 = 4;
/// On the VM, a guard's rows off more: its values' lanes zeroed (512 B
/// each a lane). paint80-01e06 on the spare (2026-11-04): three scenes
/// off 2,018 µs against an estimate of 1,871 without it (+12 ns a pixel
/// each).
pub(crate) const GUARD_VM_OFF_NS: u32 = 12;
/// In the kernel (P0b, the guards tested once a row, sites patched by the
/// driver): on a row its scene is on, the run as if there were no guard;
/// off, the run's first bytes are zeros and a jump past it.
/// paint80-01e06 on the spare: three scenes on 3,944 µs against 3,859
/// without scenes (+6.8 ns a pixel each, the path's ×1.4 in: 4 here; the
/// register layout round the runs), off 1,199 µs rows against an estimate
/// of 1,243 at 17, by rows 2,589 against 2,576. (It was a test and a
/// branch a pixel either way, guessed at 8.)
pub(crate) const GUARD_JIT_ON_NS: u32 = 4;
pub(crate) const GUARD_JIT_OFF_NS: u32 = 17;
/// A paint primitive's call: its values gathered, the call, its
/// coordinates to pixels, ns (paint-probe on the spare: `pset` off the
/// panel 16 times a frame).
pub(crate) const PAINT_CALL_NS: u64 = 1_700;
/// A ring's pixel inside it: four neighbours tested more, ns.
pub(crate) const RING_INSIDE_NS: u64 = 810;
/// The paint plane laid over the rows a frame (`draw::paint_row`: a byte a
/// pixel read, a palette entry written where something was painted), µs:
/// paint-probe on the spare, 197 µs alone with nothing painted, 401 with
/// every pixel painted (the VM's rows +196 and +401): a formula that
/// clears the plane (`cls`) paints it all, others are counted at half.
pub(crate) const PAINT_ROWS_US: u32 = 200;
pub(crate) const PAINT_FULL_US: u32 = 400;

/// What a cached lane costs a pixel, ns: its register is pointed at it
/// (read in place), once a lane of pixels — next to nothing. (It was a
/// 512 B copy a lane, guessed at 15, until psi-speed.)
pub(crate) const STILL_COPY_NS: u32 = 1;

/// `noise(x, y, z)` with a whole constant `z`, ns a pixel: the plane's
/// four corners ([`crate::fixed::noise_flat`]), measured on the spare
/// (p2probe-b: 816 ns a pixel for the step row against 1,103 with eight).
pub(crate) const NOISE_FLAT_NS: u32 = 565;

/// A value step's cost, ns a pixel at 128 lanes on the ESP32, from
/// `psi-probe` (2026-10-26): what [`Program::esp32_us`] adds up, and what the
/// compiler weighs when it chooses values for the still cache.
pub(crate) fn value_ns(c: u8) -> u32 {
    use code::*;
    match c {
        ADD | SUB | LT | LE | GT | GE | EQ | NE | AND | OR | MIN | MAX | STEP | MASK => 50,
        // A fused compare (`cmp_lane`: three strided operands, one element
        // a turn). paint80-01e06 on the spare (2026-11-04): four `sel_lt`
        // 136 µs a frame over the compares and multiplies they replace
        // (+8 ns a pixel each over the pair), two `cnt_ge` 55
        // µs under theirs (−7 each against 128): the strided loop is no
        // faster than two of `fast2`'s.
        SEL_LT | SEL_LE => 95,
        CNT_LT..=CNT_GE => 120,
        BIT => 55,
        MUL => 86,
        // The narrowed steps (`range`), measured by psi-probe on the spare
        // (lang-probe-b, 2026-10-28: each row against `gray(x)`'s 163 ns;
        // they were instruction-count guesses of 25, 55, 33 and 28).
        ADD_W | SUB_W => 28,
        MUL_W => 41,
        MUL_N => 37,
        SHR => 33,
        MOD => 113,
        // fast-div (fastdiv-b, 2026-10-29: 32-bit divisions and `nsau`;
        // were 685, 685, 529, 642, 539 with __divdi3 and the bit count),
        // fitted to the step rows as before. DIV holds both `x / (x + t)`
        // (half of it one `quou`) and `128 / v` (the Knuth digit) within
        // ±15 %; RECIP (`1 / v` since fast-div) is `128 / v`'s less the
        // 52 cycles the micro rows saw it save.
        DIV => 360,
        // `fixed::DivBy` along the lane: two multiplies, a compare and the
        // shifts, and the divisor's own once a lane. paint80-ab8b5 on the
        // spare (2026-11-05): four a pixel over `div4 mul`'s shifts, 136 ns
        // each for whole numbers, fractions, a frame's and a row's
        // divisor (`DIV_SMALL_NS` where it may be under ±1).
        DIV_S => 136,
        RECIP => 230,
        ATAN2 => 384,
        LEN => 472,
        HASH => 220,
        NEG | NOT | ABS | FLOOR | FRAC => 40,
        SIN | COS => 110,
        TRI | SAW | SQR => 60,
        SQRT => 229,
        SELECT | CLAMP => 60,
        PICK => 100,
        // A table's entry and a sprite's pixel (a header decoded, a word
        // read, a shift and a mask, the key's number read): Neon city's
        // rows fit these. (paint80-3e6de's `store low`, one `T[…]` and two
        // `s[…, …]` a pixel and little else, drew 3,297 µs against 2,604
        // estimated: −21 %, left as it is, since raising them puts Neon
        // city 16 % over. The store's place, its first numbers or past
        // 900, changes nothing.)
        TAB => 100,
        SPR => 120,
        MIX => 130,
        SMOOTH => 214,
        NOISE => 855,
        // A wide noise: `noise` and three adds of the offsets (a guess
        // until psi-probe measures it).
        NOISE_O..=NOISE_O_LAST => 870,
        // A multiply with a carry in: `mul`'s and an add (a guess).
        MULR => 110,
        DIGIT..=DIGIT_B => 240,
        U_R | U_G | U_B | INK => 70,
        // `ink`'s read, a multiply and three adds and shifts (a guess until
        // psi-probe measures it).
        INKN => 90,
        U_L => 140,
        // `under(x', y')`'s channel: a lane's two coordinates rounded and
        // held to the panel, a byte read (VM, out of line). Fitted to the
        // spare (2026-11-01): pixelate.psi and under-shake.psi, three a
        // pixel each, 4,540 µs both against 1.4 and 1.5 ms estimated
        // without it (215 and 222 ns).
        UAT => 275,
        _ => 0,
    }
}

/// A pixel's `div_s` by a constant ±2^j on the VM, ns (`shr`'s 33 and two
/// instructions: a guess until psi-probe measures it).
pub(crate) const DIV_SHIFT_NS: u32 = 53;
/// A pixel's `div_s` on the VM by a divisor that may be under ±1
/// (paint80-ab8b5's `div4 any`: 253 ns over `div4 mul`'s shift).
pub(crate) const DIV_SMALL_NS: u32 = 253;

/// The JIT's loop around a pixel (its counter, output pointer, branch),
/// ns: jit1-b's `gray(x)` kernel 107 ns a pixel less its colour (27).
pub(crate) const JIT_LOOP_NS: u32 = 80;
/// A lane from before the kernel read at the pixel (a still lane), ns.
pub(crate) const JIT_LANE_NS: u32 = 13;
/// The kernel's words filled for each row, µs a frame (jit1-b: ~93 µs
/// before the frame's constant words were filled once).
pub(crate) const JIT_ROWS_US: u32 = 32;
/// The kernel's steps as an effect draws them against their rows' sum, %
/// (jit1-b; refit on jit2-c: 12 rows within -11..+6 %, tests/estimate.rs).
pub(crate) const JIT_PATH_PCT: u32 = 140;

/// A kernel step's cost, ns a pixel on the ESP32: phase 1's rows on the
/// spare (jit1-b, each over `gray(x)`'s kernel; `tests/measured.rs`
/// `JIT`), instruction counts where a step was not measured alone (~4.3
/// ns an instruction), and a helper call the VM's step less ~60 ns (the
/// lane loop's traffic the call does not pay).
pub(crate) fn jit_value_ns(c: u8) -> u32 {
    use code::*;
    match c {
        ADD_W | SUB_W => 2,
        MUL_W => 5,
        MUL_N => 10,
        SHR | MASK | FRAC | MIN | MAX => 6,
        FLOOR | CLAMP | SELECT => 9,
        ADD | SUB => 37,
        MUL => 54,
        ABS | NEG | NOT | SAW | SQR => 17,
        LT | LE | GT | GE | EQ | NE | STEP => 17,
        // `sel_*`: a move, a branch, a move; `cnt_*`: two branches over
        // two adds, then the shift. paint80-01e06 on the spare: four
        // `sel_lt` 174 µs a frame under the compares and multiplies (−10.6
        // ns a pixel each, ×1.4 the path: 22 − 7.6), two `cnt_ge` 215
        // under theirs (−26.2 each: 36 − 18.7).
        SEL_LT | SEL_LE => 14,
        CNT_LT..=CNT_GE => 17,
        AND | OR => 25,
        TRI => 49,
        SIN => 60,
        COS => 65,
        INK | U_R | U_G | U_B => 26,
        INKN => 34,
        U_L => 70,
        // `under(x, y)`'s template: two coordinates rounded and held (8
        // instructions each), the index, a mask, a byte read and `byte`
        // (~24 in all, U_R's 26 ns for 8). Paint-probe on the spare
        // (paint80-f6aa6): 3, 6 and 12 steps 2,306 / 4,052 / 7,505 µs a
        // frame, pixelate 2,645 on the real path: within -3..+12 %.
        UAT => 75,
        // Inline since paint80-3e6de showed the helper calls slower than
        // the VM's lane loops (3,607 µs a frame against 3,259 for one table
        // and two sprite reads a pixel): a table's entry ~9 instructions
        // (the index against its length, the load), a sprite's pixel ~25
        // (its shape known when the kernel is made: both bounds, the
        // row's word, the pixel's bits by `ssr`/`src`, the key's number).
        // Instruction counts until the spare measures them.
        TAB => 40,
        SPR => 110,
        // `fixed::DivBy`'s steps inline (~18 instructions where the ranges
        // say the divisor is past ±1, ~27 where they don't; a constant's:
        // `jit_step_ns`). Instruction counts until the spare measures them.
        DIV_S => 115,
        c => value_ns(c).saturating_sub(60).max(60),
    }
}

/// `NAME[k]`: entry floor(k) of the table at place `start` ([`store_at`])
/// of `last` + 1 entries, 0 outside it. Indices are masked into the
/// tables, whatever the program.
#[inline(always)]
pub(crate) fn tab(tables: &[i32], start: u8, last: u8, k: i32) -> i32 {
    let i = k >> 16;
    if i < 0 || i > last as i32 {
        return 0;
    }
    word(tables, store_at(start) + i as usize)
}

/// Store word `i` masked into `tabs`, without a bounds check: every store
/// is a power of two of numbers (`Program`'s 1,024, `Prog::try_boxed`'s
/// `store_words`), so the mask keeps any index inside it. On the VM's and
/// the kernel's per-pixel path (a bounds check there cost the draw stage
/// and the row steps a compare and a branch a read).
#[inline(always)]
pub(crate) fn word(tabs: &[i32], i: usize) -> i32 {
    debug_assert!(tabs.len().is_power_of_two());
    // SAFETY: `tabs.len()` is a power of two (above), so `i & (len − 1)`
    // is below it.
    unsafe { *tabs.get_unchecked(i & (tabs.len() - 1)) }
}

/// A sprite's shape (`sprite NAME { … }`), from its header word in the
/// tables at place `at` ([`store_at`]): `w | h << 8 | lb << 16 | keys << 20` (lb the log2 of
/// its bits a pixel: 1, 2 or 4 bits), then its keys' numbers, then its
/// rows, each `wpr` words of `32 >> lb` pixels, the first in the low bits.
#[derive(Clone, Copy)]
pub(crate) struct SprHead {
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) lb: u32,
    pub(crate) lg: u32,
    pub(crate) wpr: usize,
    pub(crate) at: usize,
    pub(crate) data: usize,
}

impl SprHead {
    #[inline(always)]
    pub(crate) fn new(tabs: &[i32], at: u8) -> SprHead {
        let at = store_at(at) & (tabs.len() - 1);
        let hd = word(tabs, at) as u32;
        let (w, h, lb, keys) = (hd & 0xff, (hd >> 8) & 0xff, (hd >> 16) & 3, (hd >> 20) & 31);
        // Pixels a word: 32, 16 or 8 (as its log2).
        let lg = 5 - lb.min(2);
        SprHead {
            w,
            h,
            lb: lb.min(2),
            lg,
            wpr: ((w + (1 << lg) - 1) >> lg) as usize,
            at,
            data: at + 1 + keys as usize,
        }
    }

    /// The pixel at column floor(u), row floor(v) from the top left: its
    /// key's number, 0 outside. Indices are masked into the tables,
    /// whatever the program.
    #[inline(always)]
    pub(crate) fn read(&self, tabs: &[i32], u: i32, v: i32) -> i32 {
        let (x, y) = ((u >> 16) as u32, (v >> 16) as u32);
        // (Negative columns and rows are huge as u32: outside too.)
        if x >= self.w || y >= self.h {
            return 0;
        }
        let w = word(tabs, self.data + y as usize * self.wpr + (x >> self.lg) as usize) as u32;
        let k = (w >> ((x & ((1 << self.lg) - 1)) << self.lb)) & ((1 << (1 << self.lb)) - 1);
        word(tabs, self.at + 1 + k as usize)
    }

    /// The words it takes in the tables, its header first.
    pub(crate) fn words(&self) -> usize {
        self.data - self.at + self.h as usize * self.wpr
    }
}

/// `pick`'s choice for `k` from the `n` slots whose numbers are in the
/// slots from `base` (0 where floor(k) is outside 0..n). Indices are masked
/// into the slots, whatever the program.
#[inline(always)]
pub(crate) fn pick(sc: &Sc, base: u8, n: u8, k: i32) -> i32 {
    let i = k >> 16;
    if i < 0 || i >= n as i32 {
        return 0;
    }
    sc[sc[(base as usize + i as usize) & (SC - 1)] as usize & (SC - 1)]
}

/// The scalar evaluation of one arithmetic op: the per-frame and per-row
/// steps, and the constant folder (which never asks for the steps that read
/// a table, [`code::is_table`], hence the `Option`: `None` there). Out of
/// line: one copy for both (inlined, the ESP32 build carried two, 2.4 KB).
#[inline(never)]
#[cfg_attr(all(target_os = "espidf", feature = "sram1-iram"), link_section = ".iram1.s1")]
pub(crate) fn eval(c: u8, a: i32, b: i32, x: i32, t: Option<&Tables>) -> i32 {
    use code::*;
    match c {
        // (The narrowed steps are per pixel only; their checked forms here
        // give the same values.)
        ADD | ADD_W => fixed::add(a, b),
        SUB | SUB_W => fixed::sub(a, b),
        MUL | MUL_W | MUL_N => fixed::mul(a, b),
        DIV | DIV_S => fixed::div(a, b),
        MOD => fixed::modulo(a, b),
        MASK => fixed::mask(a, b),
        LT => fixed::lt(a, b),
        LE => fixed::le(a, b),
        GT => fixed::gt(a, b),
        GE => fixed::ge(a, b),
        EQ => fixed::eq(a, b),
        NE => fixed::ne(a, b),
        AND => fixed::and(a, b),
        OR => fixed::or(a, b),
        MIN => fixed::min(a, b),
        MAX => fixed::max(a, b),
        STEP => fixed::step(a, b),
        ATAN2 => fixed::atan2(a, b),
        HASH => fixed::hash(a, b),
        LEN => fixed::len(a, b),
        BIT => fixed::bit(a, b),
        NEG => fixed::neg(a),
        NOT => fixed::not(a),
        ABS => fixed::abs(a),
        FLOOR => fixed::floor(a),
        FRAC => fixed::frac(a),
        SIN => t.map_or(0, |t| fixed::sin(t, a)),
        COS => t.map_or(0, |t| fixed::cos(t, a)),
        TRI => fixed::tri(a),
        SAW => fixed::saw(a),
        SQR => fixed::sqr(a),
        SQRT => fixed::sqrt(a),
        RECIP => fixed::recip(a),
        SELECT => fixed::select(a, b, x),
        CLAMP => fixed::clamp(a, b, x),
        SEL_LT..=CNT_GE => fixed::cmp_fused(c, a, b, x),
        MIX => fixed::mix(a, b, x),
        SMOOTH => fixed::smooth(a, b, x),
        NOISE => fixed::noise(a, b, x),
        MULR => fixed::mulr(a, b, x),
        DIGIT..=DIGIT_B => t.map_or(0, |t| fixed::glyph(&glyphs(t, c), a, b, x)),
        _ => 0,
    }
}
