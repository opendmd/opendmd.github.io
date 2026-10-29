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

/// Steps a formula may take in all (per-frame, per-row, per-pixel and the
/// colour): the cap the compiler holds every program to.
pub const MAX_OPS: usize = 256;
/// Scalar slots: the inputs, the constants, and every per-frame and per-row
/// value.
pub const MAX_SCALARS: usize = 128;
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

/// An operand is a scalar slot (0..127) or, with this bit, a lane: 0 = `x`,
/// 1 = `px`, 2 = `i`, 3.. = the registers.
pub(crate) const LANE: u8 = 0x80;
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
    // is `tables[c]` (`c` an immediate), 0 outside it ([`super::SprHead`]).
    // Any level; never folded (a constant u, v: once a frame).
    pub const SPR: u8 = 25;
    pub const LAST_BIN: u8 = 25;
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
    // `NAME[k]`, a table's entry: `d = tables[b + floor(k)]` where floor(k)
    // is 0..=c (the table's length less one), else 0 ([`super::tab`]). Any
    // level; one operand.
    pub const TAB: u8 = 45;
    // `mul(a, b)` where `b` is 2^-n for inputs in range (n = `c`, 0..=16, an
    // immediate): exactly `a >> n`, an arithmetic shift; its checked form
    // (`range::checked`) is `mul(a, b)` (`range::narrow`, per pixel only).
    pub const SHR: u8 = 46;
    pub const LAST_UN: u8 = 46;
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
    // nullary, per pixel: the picture under the formula (its red, green,
    // blue, luminance), 0..1
    pub const U_R: u8 = 80;
    pub const U_G: u8 = 81;
    pub const U_B: u8 = 82;
    pub const U_L: u8 = 83;
    // nullary, per pixel: what the draw stage put at the pixel, 0..1
    pub const INK: u8 = 84;
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

    /// A step that reads a table (the sine, a font): never folded, run once
    /// a frame when its operands are constants.
    pub fn is_table(c: u8) -> bool {
        matches!(c, SIN | COS | SPR | DIGIT..=DIGIT_B | NOISE_O..=NOISE_O_LAST)
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
    pub fn writes_scalar(c: u8) -> bool {
        match c {
            W_FIRST..=W_LAST => !wide_ops(c).0,
            LINE_FROM | LINE_TO | DOT | STILL | COLUMN | FUSE | END => false,
            c => !is_colour(c),
        }
    }

    /// A step that writes pixels rather than a value.
    pub fn is_colour(c: u8) -> bool {
        c >= OUT_RGB
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
    pub fn arity(c: u8) -> usize {
        match c {
            0..=LAST_BIN => 2,
            32..=LAST_UN => 1,
            W_FIRST..=W_LAST => wide_ops(c).1,
            64..=LAST_TERN => 3,
            U_R..=INK => 0,
            FOR => 1,
            END => 2,
            LINE_FROM..=DOT => 3,
            STILL | COLUMN | FUSE => 0,
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
            0..=LAST_BIN => N[c as usize],
            32..=LAST_UN => U[(c - 32) as usize],
            W_FIRST..=W_LAST => WN[(c - W_FIRST) as usize],
            64..=LAST_TERN => T[(c - 64) as usize],
            U_R..=INK => ["ur", "ug", "ub", "ul", "ink"][(c - U_R) as usize],
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

/// `table` entries a formula may hold in all.
pub const MAX_TABLE: usize = 256;

/// A formula's own palettes (`palette NAME = #…, …`): up to three, `pal`'s
/// palettes 5, 6 and 7 after the five built-ins, each 32 entries
/// (0x00RRGGBB) at the top of the tables ([`user_palette_at`]).
pub const USER_PALETTES: usize = 3;

/// Where user palette `j` (0..3) starts in the tables: the top 32 entries
/// for the first, the 32 below for the second, …
#[inline(always)]
pub const fn user_palette_at(j: usize) -> usize {
    MAX_TABLE - 32 * (j + 1)
}

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

/// The formula's own palettes from its tables (out of line: once a frame,
/// and only for a formula with some, `OWNPAL`).
#[inline(never)]
pub(crate) fn unpack_palettes(tabs: &[i32; MAX_TABLE], own: &mut Pals) {
    for (j, p) in own.iter_mut().enumerate() {
        let at = user_palette_at(j);
        for (k, c) in p.iter_mut().enumerate() {
            let v = tabs[at + k] as u32;
            *c = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
        }
    }
}

/// A compiled formula. Plain data (every field an integer or an array of
/// them), so an all-zero value is valid (an empty program) and
/// [`crate::compile_into`] can fill one in place.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Program {
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
    /// The `table` constants (Q16.16), each table's entries in a row.
    pub(crate) tables: [i32; MAX_TABLE],
    /// What the formula declares (its controls, whether it is a look).
    pub(crate) meta: crate::meta::Meta,
}

const _: () = assert!(core::mem::size_of::<Program>() <= 2876);

impl core::fmt::Debug for Program {
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
        Program {
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
            tables: [0; MAX_TABLE],
            meta: crate::meta::Meta::default(),
        }
    }
}

impl Program {
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

    /// Steps in all, as the formula's limit counts them ([`MAX_OPS`]): the
    /// fused runs' markers (`crate::fuse`) not counted.
    pub fn steps(&self) -> usize {
        self.len() - self.fused()
    }

    /// The fused runs' markers.
    fn fused(&self) -> usize {
        let (re, pe) = (self.row_end as usize, self.len().min(MAX_OPS));
        self.ops[re.min(pe)..pe]
            .iter()
            .filter(|o| o.code == code::FUSE)
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

    /// The lines and dots the draw stage draws a frame: every primitive
    /// counted as often as its loops run it.
    pub fn primitives(&self) -> u32 {
        let mut n = 0;
        self.draw_walk(|_, op, mult| {
            if matches!(op.code, code::LINE_TO | code::DOT) {
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
                    let n = (self.init[(op.a & 0x7f) as usize] >> 16).max(1) as u32;
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
    /// under the formula ([`Program::over`]), bit 6 `seed`.
    pub fn inputs(&self) -> u8 {
        let mut m = if self.over() { 32 } else { 0 };
        if self.uses & SEEDED != 0 {
            m |= 64;
        }
        for op in &self.ops[..self.len()] {
            // A wide step reads `t` (every wide value is made from it).
            if code::is_wide(op.code) {
                m |= 8;
            }
            let (v, n) = code::operands(op);
            // A pick's choices are read through its table of slots.
            let table = if op.code == code::PICK {
                &self.init[op.b as usize..][..op.c as usize]
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
        self.estimate_with(None, false, false)
    }

    fn estimate(&self, still: Option<usize>) -> u32 {
        self.estimate_with(still, true, false)
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
        self.estimate_with(None, true, true).min(self.esp32_us())
    }

    /// [`esp32_us_jit`](Self::esp32_us_jit) with a filled still cache, as
    /// [`esp32_us_still`](Self::esp32_us_still).
    pub fn esp32_us_jit_still(&self) -> u32 {
        self.estimate_with(self.still_marker(), true, true).min(self.esp32_us_still())
    }

    /// The first step a kernel draws: after the column run, and after the
    /// still cache's run with what is laid out before its marker (the VM
    /// runs those as lanes).
    pub(crate) fn kernel_start(&self) -> usize {
        let (re, pe) = (self.row_end as usize, self.len());
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

    fn estimate_with(&self, still: Option<usize>, lanes: bool, jit: bool) -> u32 {
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
        let mut k = self.row_end as usize;
        while k < self.len() {
            let op = &self.ops[k];
            if skip.contains(&k) {
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
                px += if col.contains(&k) { ns.div_ceil(H as u32) } else { ns };
            }
            k += 1;
        }
        let row = self.counts().1 as u32;
        // The loop itself (a pixel's colour is counted above: `gray(x)` is
        // 45 + 120, as measured); then what the real paths add to the VM's
        // own frame — the effect's copy into the canvas, the look's rows
        // over the picture, a flash cache shared with the decoder: +13-14 %
        // on the device (plasma as an effect 8,217 against 7,254 µs; Plasma
        // screened at 40 % as a look 8,552 against 7,474). With the JIT: a
        // still lane read in place, the kernel's steps x1.3 (jit1-b as
        // effects against their rows' sum), its words a row.
        let (px, extra) = if jit {
            (px + JIT_LANE_NS * cached + kp * JIT_PATH_PCT / 100, JIT_ROWS_US)
        } else {
            (45 + px + STILL_COPY_NS * cached, 0)
        };
        let px = px.saturating_sub(saved);
        ((px * (W * H) as u32) / 1000 + row * 10 + self.draw_cost(lanes) + extra) * 115 / 100
    }

    /// A kernel step's cost, ns a pixel (see `esp32_us_jit`).
    fn jit_step_ns(&self, at: usize, op: &Op) -> u32 {
        use code::*;
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
        let konst = k & LANE == 0 && k >= S_FIRST_FREE && self.is_const_slot(k, at);
        let covered = n > vals && !(konst && self.init[k as usize] >= ONE);
        let mix = match blend {
            None => 0,
            Some(crate::fixed::BLEND_ADD) => 52,
            Some(crate::fixed::BLEND_MULTIPLY) => 103,
            Some(crate::fixed::BLEND_SCREEN) => 142,
            Some(crate::fixed::BLEND_MAX | crate::fixed::BLEND_MIN) => 39,
            Some(_) => 13,
        };
        let cover = match (blend, covered) {
            (Some(_), true) => 77 + if k & LANE != 0 { 15 } else { 0 },
            _ => 0,
        };
        base + mix + cover
    }

    /// A pixel step's cost, ns a pixel at 128 lanes (see `esp32_us`).
    fn step_ns(&self, at: usize, op: &Op) -> u32 {
        use code::*;
        match op.code {
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
                let lane = v[..vals.min(n)].iter().any(|&o| o & LANE != 0);
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
                let konst = k & LANE == 0 && k >= S_FIRST_FREE && self.is_const_slot(k, at);
                let covered = n > vals && !(konst && self.init[k as usize] >= ONE);
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
            c => value_ns(c),
        }
    }

    /// Whether operand `v` of step `at` takes few values along a row: a
    /// scalar, or a lane last written by a floor, a choice or a comparison.
    fn stepped(&self, at: usize, v: u8) -> bool {
        use code::*;
        if v & LANE == 0 {
            return true;
        }
        let re = self.row_end as usize;
        let w = (re.min(at)..at).rev().find(|&k| {
            let o = &self.ops[k];
            !is_colour(o.code) && !matches!(o.code, STILL | COLUMN | FUSE) && o.d == v
        });
        // (A shift of a stepped value, `floor(q)/8`, is stepped.)
        if let Some(k) = w.filter(|&k| self.ops[k].code == SHR) {
            return self.stepped(k, self.ops[k].a);
        }
        w.map(|k| &self.ops[k]).is_some_and(|o| {
            matches!(
                o.code,
                FLOOR | SELECT | LT | LE | GT | GE | EQ | NE | AND | OR | NOT | STEP | BIT | SPR
            )
        })
    }

    /// Where the still cache's marker is (`code::STILL`), if the formula
    /// has values for it.
    pub(crate) fn still_marker(&self) -> Option<usize> {
        let (re, pe) = (self.row_end as usize, self.len().min(MAX_OPS));
        (re.min(pe)..pe).find(|&k| self.ops[k].code == code::STILL)
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
        self.draw_cost(true)
    }

    /// [`draw_us`](Self::draw_us), with (`lanes`) or without the loops run
    /// as lanes.
    fn draw_cost(&self, lanes: bool) -> u32 {
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
        self.draw_walk(|at, op, mult| {
            if let Some(v) = self.vector_loop(at).filter(|_| lanes) {
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
                _ => 420,
            };
            ns = ns.saturating_add(cost * mult as u64);
        });
        (ns / 1000).min(u32::MAX as u64) as u32
    }

    /// The value of slot `v` as step `at` reads it if it holds a constant
    /// there, `None` for anything else (a slot holds a constant until a step
    /// writes another value into it: slots are used again).
    pub(crate) fn const_value(&self, v: u8, at: usize) -> Option<i32> {
        (v & LANE == 0
            && v >= S_FIRST_FREE
            && (v as usize) < MAX_SCALARS
            && self.is_const_slot(v, at))
        .then(|| self.init[v as usize])
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
    }

    /// What the formula declares: its controls (defaults for the slots a
    /// path leaves unset), and whether it may be a look.
    pub fn meta(&self) -> &crate::meta::Meta {
        &self.meta
    }

    /// A readable listing of the steps (tests, the bench, debugging).
    pub fn listing(&self) -> alloc::string::String {
        use core::fmt::Write;
        let mut s = alloc::string::String::new();
        let opnd = |v: u8, n: usize| -> alloc::string::String {
            if v & LANE != 0 {
                match v & !LANE {
                    L_X => "x".into(),
                    L_PX => "px".into(),
                    L_I => "i".into(),
                    r => alloc::format!("L{}", r - L_REG0),
                }
            } else if v < S_FIRST_FREE {
                ["t", "hour", "minute", "second", "a", "b", "c", "y", "py"][v as usize].into()
            } else if v == S_SEED && self.uses & SEEDED != 0 {
                "seed".into()
            } else if (v as usize) < MAX_SCALARS && self.is_const_slot(v, n) {
                alloc::format!("{}", self.init[v as usize] as f64 / ONE as f64)
            } else {
                alloc::format!("s{v}")
            }
        };
        for (n, op) in self.ops[..self.len()].iter().enumerate() {
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
                alloc::format!("{args}; end {}", op.b)
            } else if op.code == code::END {
                alloc::format!("{args}; back to {}", op.c)
            } else if op.code == code::FUSE {
                alloc::format!("{}; {} steps", crate::fuse::name(op.d), op.a)
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
                    .map(|j| opnd(self.init[(op.b as usize + j) & (MAX_SCALARS - 1)] as u8, n))
                    .collect();
                alloc::format!("{args}; {}", t.join(", "))
            } else {
                args
            };
            let d = if code::is_colour(op.code)
                || matches!(
                    op.code,
                    code::END
                        | code::LINE_FROM
                        | code::LINE_TO
                        | code::DOT
                        | code::STILL
                        | code::COLUMN
                        | code::FUSE
                ) {
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
    pub(crate) fn vector_loop(&self, at: usize) -> Option<VecLoop> {
        let f = self.ops.get(at).filter(|o| o.code == code::FOR)?;
        if f.c != LANES {
            return None;
        }
        let end = f.b as usize;
        let mut vl = VecLoop {
            end,
            count: (self.init[(f.a & 0x7f) as usize] >> 16) as u32,
            regs: 1,
            slot: [0; REGS + 1],
        };
        vl.slot[1] = f.d & 0x7f;
        for op in &self.ops[at + 1..end] {
            if code::writes_scalar(op.code) && vl.reg(op.d).is_none() {
                vl.regs += 1;
                vl.slot[(vl.regs as usize).min(REGS)] = op.d & 0x7f;
            }
        }
        debug_assert_eq!(
            Some((vl.end, vl.count, vl.regs, vl.slot)),
            self.check_vector_loop(at)
                .map(|v| (v.end, v.count, v.regs, v.slot))
        );
        Some(vl)
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
                self.ops[at].c = if lanes { LANES } else { 0 };
            }
        }
    }

    /// [`vector_loop`](Self::vector_loop)'s rules, checked.
    fn check_vector_loop(&self, at: usize) -> Option<VecLoop> {
        let de = (self.draw_end as usize).min(MAX_OPS);
        let f = self.ops.get(at).filter(|o| o.code == code::FOR)?;
        let end = f.b as usize;
        if end <= at || end >= de {
            return None;
        }
        let e = &self.ops[end];
        let k = f.d & 0x7f;
        if e.code != code::END || e.a & 0x7f != k || e.b != f.a || e.c as usize != at + 1 {
            return None;
        }
        if !self.is_const_slot(f.a & 0x7f, at) {
            return None;
        }
        let count = self.init[(f.a & 0x7f) as usize] >> 16;
        if count < VEC_MIN as i32 {
            return None;
        }
        let body = &self.ops[at + 1..end];
        let prim = |c: u8| matches!(c, code::LINE_FROM | code::LINE_TO | code::DOT);
        let value = |c: u8| {
            matches!(c, 0..=code::LAST_BIN | 32..=code::LAST_UN | 64..=code::LAST_TERN)
                && code::writes_scalar(c)
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
                if op.d & 0x7f == k {
                    return None;
                }
                if vl.reg(op.d).is_none() {
                    if vl.regs as usize == REGS {
                        return None;
                    }
                    vl.regs += 1;
                    vl.slot[vl.regs as usize] = op.d & 0x7f;
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
                    .any(|j| bit(self.init[(op.b as usize + j) & (MAX_SCALARS - 1)] as u8) != 0)
            {
                return None;
            }
            if value(op.code) {
                spare |= v[..n].iter().any(|&v| v & 0x7f == op.d & 0x7f);
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
            && !self.ops[..at.min(self.len())].iter().any(|op| {
                // END counts its loop's counter (which the FOR writes too).
                (op.code == code::END && op.a == v) || (code::writes_scalar(op.code) && op.d == v)
            })
    }
}

/// A draw loop's FOR's `c` when it runs as lanes
/// ([`Program::vector_loop`]; 0 when a turn at a time).
pub(crate) const LANES: u8 = 1;

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
        while r > 0 && self.slot[r] != v & 0x7f {
            r -= 1;
        }
        (r > 0).then_some(r)
    }
}

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
        RECIP => 230,
        ATAN2 => 384,
        LEN => 472,
        HASH => 220,
        NEG | NOT | ABS | FLOOR | FRAC => 40,
        SIN | COS => 110,
        TRI | SAW | SQR => 60,
        SQRT => 229,
        SELECT | CLAMP => 60,
        PICK | TAB => 100,
        // A sprite's pixel: a table read, a shift and a mask (a guess until
        // psi-probe measures it, as the design guessed).
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
        U_L => 140,
        _ => 0,
    }
}

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
        AND | OR => 25,
        TRI => 49,
        SIN => 60,
        COS => 65,
        INK | U_R | U_G | U_B => 26,
        U_L => 70,
        c => value_ns(c).saturating_sub(60).max(60),
    }
}

/// `NAME[k]`: entry floor(k) of the table at `start` of `last` + 1
/// entries, 0 outside it. Indices are masked into the tables, whatever the
/// program.
#[inline(always)]
pub(crate) fn tab(tables: &[i32; MAX_TABLE], start: u8, last: u8, k: i32) -> i32 {
    let i = k >> 16;
    if i < 0 || i > last as i32 {
        return 0;
    }
    tables[(start as usize + i as usize) & (MAX_TABLE - 1)]
}

/// A sprite's shape (`sprite NAME { … }`), from its header word in the
/// tables at `at`: `w | h << 8 | lb << 16 | keys << 20` (lb the log2 of
/// its bits a pixel: 1, 2 or 4 bits), then its keys' numbers, then its
/// rows, each `wpr` words of `32 >> lb` pixels, the first in the low bits.
#[derive(Clone, Copy)]
pub(crate) struct SprHead {
    pub(crate) w: u32,
    pub(crate) h: u32,
    lb: u32,
    lg: u32,
    wpr: usize,
    at: usize,
    data: usize,
}

impl SprHead {
    #[inline(always)]
    pub(crate) fn new(tabs: &[i32; MAX_TABLE], at: u8) -> SprHead {
        let at = at as usize;
        let hd = tabs[at] as u32;
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
    pub(crate) fn read(&self, tabs: &[i32; MAX_TABLE], u: i32, v: i32) -> i32 {
        let (x, y) = ((u >> 16) as u32, (v >> 16) as u32);
        // (Negative columns and rows are huge as u32: outside too.)
        if x >= self.w || y >= self.h {
            return 0;
        }
        let word = tabs
            [(self.data + y as usize * self.wpr + (x >> self.lg) as usize) & (MAX_TABLE - 1)]
            as u32;
        let k = (word >> ((x & ((1 << self.lg) - 1)) << self.lb)) & ((1 << (1 << self.lb)) - 1);
        tabs[(self.at + 1 + k as usize) & (MAX_TABLE - 1)]
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
pub(crate) fn pick(sc: &[i32; MAX_SCALARS], base: u8, n: u8, k: i32) -> i32 {
    let i = k >> 16;
    if i < 0 || i >= n as i32 {
        return 0;
    }
    let m = MAX_SCALARS - 1;
    sc[sc[(base as usize + i as usize) & m] as usize & m]
}

/// The scalar evaluation of one arithmetic op: the per-frame and per-row
/// steps, and the constant folder (which never asks for the steps that read
/// a table, [`code::is_table`], hence the `Option`: `None` there). Out of
/// line: one copy for both (inlined, the ESP32 build carried two, 2.4 KB).
#[inline(never)]
pub(crate) fn eval(c: u8, a: i32, b: i32, x: i32, t: Option<&Tables>) -> i32 {
    use code::*;
    match c {
        // (The narrowed steps are per pixel only; their checked forms here
        // give the same values.)
        ADD | ADD_W => fixed::add(a, b),
        SUB | SUB_W => fixed::sub(a, b),
        MUL | MUL_W | MUL_N => fixed::mul(a, b),
        DIV => fixed::div(a, b),
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
        MIX => fixed::mix(a, b, x),
        SMOOTH => fixed::smooth(a, b, x),
        NOISE => fixed::noise(a, b, x),
        MULR => fixed::mulr(a, b, x),
        DIGIT..=DIGIT_B => t.map_or(0, |t| fixed::glyph(&glyphs(t, c), a, b, x)),
        _ => 0,
    }
}
