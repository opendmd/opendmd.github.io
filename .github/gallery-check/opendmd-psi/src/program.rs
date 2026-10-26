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
    pub const LAST_BIN: u8 = 20;
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
    pub const LAST_UN: u8 = 44;
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
    pub const LAST_TERN: u8 = 73;
    // nullary, per pixel: the picture under the formula (its red, green,
    // blue, luminance), 0..1
    pub const U_R: u8 = 80;
    pub const U_G: u8 = 81;
    pub const U_B: u8 = 82;
    pub const U_L: u8 = 83;
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
        matches!(c, SIN | COS | DIGIT..=DIGIT_B)
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
            64..=LAST_TERN => 3,
            U_R..=U_L => 0,
            _ => match colour_parts(c).0 {
                OUT_RGB | OUT_HSV => 3,
                _ => 1,
            },
        }
    }

    /// Every operand a step reads (its value operands, then a layer's
    /// coverage) and how many.
    pub fn operands(op: &super::Op) -> ([u8; 4], usize) {
        let n = arity(op.code);
        let mut v = [op.a, op.b, op.c, 0];
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
        const N: [&str; 21] = [
            "add", "sub", "mul", "div", "mod", "mask", "lt", "le", "gt", "ge", "eq", "ne", "and",
            "or", "min", "max", "step", "atan2", "hash", "len", "bit",
        ];
        const U: [&str; 13] = [
            "neg", "not", "abs", "floor", "frac", "sin", "cos", "tri", "saw", "sqr", "sqrt",
            "recip", "pick",
        ];
        const T: [&str; 10] = [
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
        ];
        match c {
            0..=LAST_BIN => N[c as usize],
            32..=LAST_UN => U[(c - 32) as usize],
            64..=LAST_TERN => T[(c - 64) as usize],
            U_R..=U_L => ["ur", "ug", "ub", "ul"][(c - U_R) as usize],
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

/// A compiled formula. Plain data (every field an integer or an array of
/// them), so an all-zero value is valid (an empty program) and
/// [`crate::compile_into`] can fill one in place.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Program {
    pub(crate) ops: [Op; MAX_OPS],
    /// The scalar slots' values at the start of a frame: the constants.
    pub(crate) init: [i32; MAX_SCALARS],
    /// `ops[..frame_end]` run once a frame, `..row_end` once a row,
    /// `..pixel_end` per pixel: values, and a colour step per layer
    /// (bottom to top; the last step is always one).
    pub(crate) frame_end: u16,
    pub(crate) row_end: u16,
    pub(crate) pixel_end: u16,
    pub(crate) n_scalars: u8,
    pub(crate) regs: u8,
    pub(crate) uses: u8,
    pub(crate) _pad: [u8; 3],
    /// The loop, ms (`loop N`, default 8 s).
    pub(crate) loop_ms: u32,
}

const _: () = assert!(core::mem::size_of::<Program>() <= 1824);

impl core::fmt::Debug for Program {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (fr, row, px) = self.counts();
        write!(
            f,
            "Program {{ steps: {} ({fr}/{row}/{px}), regs: {}, loop_ms: {} }}",
            self.len(),
            self.regs,
            self.loop_ms
        )
    }
}

impl Default for Program {
    fn default() -> Self {
        Program {
            ops: [Op::default(); MAX_OPS],
            init: [0; MAX_SCALARS],
            frame_end: 0,
            row_end: 0,
            pixel_end: 0,
            n_scalars: S_FIRST_FREE,
            regs: 0,
            uses: 0,
            _pad: [0; 3],
            loop_ms: 8000,
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
    /// counted with the pixel steps).
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.frame_end as usize,
            (self.row_end - self.frame_end) as usize,
            (self.pixel_end - self.row_end) as usize,
        )
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

    /// The loop's length in ms.
    pub fn loop_ms(&self) -> u32 {
        self.loop_ms
    }

    /// Which inputs the steps read: bits 0-2 the sliders `a`, `b`, `c`
    /// (the web app's editor shows a slider for each), bit 3 `t`, bit 4
    /// the wall clock (`hour`, `minute` or `second`), bit 5 the picture
    /// under the formula ([`Program::over`]).
    pub fn inputs(&self) -> u8 {
        let mut m = if self.over() { 32 } else { 0 };
        for op in &self.ops[..self.len()] {
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
                    S_T => 8,
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
        use code::*;
        let px: u32 = self.ops[self.row_end as usize..self.len()]
            .iter()
            .map(|op| match op.code {
                ADD | SUB | LT | LE | GT | GE | EQ | NE | AND | OR | MIN | MAX | STEP | MASK => 50,
                BIT => 55,
                MUL => 86,
                MOD => 113,
                DIV | RECIP => 685,
                ATAN2 => 529,
                LEN => 642,
                HASH => 220,
                NEG | NOT | ABS | FLOOR | FRAC => 40,
                SIN | COS => 110,
                TRI | SAW | SQR => 60,
                SQRT => 539,
                SELECT | CLAMP => 60,
                PICK => 100,
                MIX => 130,
                SMOOTH => 214,
                NOISE => 855,
                DIGIT..=DIGIT_B => 240,
                U_R | U_G | U_B => 70,
                U_L => 140,
                c if is_colour(c) => {
                    // Measured on the device (psi-probe, 2026-10-26): a
                    // colour's write — a lane's `chan` per pixel, ~5 for one
                    // value filled along the row — plus its kind, and for a
                    // layer the blend loop: one per blend when it covers all,
                    // a shared loop with the coverage (and the blend chosen
                    // per channel) when it covers less.
                    let (kind, blend) = colour_parts(c);
                    let (v, n) = operands(op);
                    let vals = arity(c);
                    let lane = v[..vals.min(n)].iter().any(|&o| o & LANE != 0);
                    let write = if lane { 120 } else { 5 };
                    // (Of one value: worked out once, filled along the row.)
                    let base = match kind {
                        OUT_HSV if lane => 195,
                        OUT_RGB if lane => 120,
                        _ => 0,
                    };
                    // Covered unless the coverage is a constant slot of 1 or
                    // more (a lane or an input never is one).
                    let k = v[n.saturating_sub(1)];
                    let konst = k & LANE == 0 && k >= S_FIRST_FREE && self.is_const_slot(k);
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
                _ => 0,
            })
            .sum();
        let row = (self.row_end - self.frame_end) as u32;
        // The loop itself (a pixel's colour is counted above: `gray(x)` is
        // 45 + 120, as measured); then what the real paths add to the VM's
        // own frame — the effect's copy into the canvas, the look's rows
        // over the picture, a flash cache shared with the decoder: +13-14 %
        // on the device (plasma as an effect 8,217 against 7,254 µs; Plasma
        // screened at 40 % as a look 8,552 against 7,474).
        (((45 + px) * (W * H) as u32) / 1000 + row * 10) * 115 / 100
    }

    /// Reset a program in place to the empty one (all fields zero but the
    /// defaults), for a zeroed allocation.
    pub fn clear(&mut self) {
        self.ops.fill(Op::default());
        self.init.fill(0);
        self.frame_end = 0;
        self.row_end = 0;
        self.pixel_end = 0;
        self.n_scalars = S_FIRST_FREE;
        self.regs = 0;
        self.uses = 0;
        self.loop_ms = 8000;
    }

    /// A readable listing of the steps (tests, the bench, debugging).
    pub fn listing(&self) -> alloc::string::String {
        use core::fmt::Write;
        let mut s = alloc::string::String::new();
        let opnd = |v: u8| -> alloc::string::String {
            if v & LANE != 0 {
                match v & !LANE {
                    L_X => "x".into(),
                    L_PX => "px".into(),
                    L_I => "i".into(),
                    r => alloc::format!("L{}", r - L_REG0),
                }
            } else if v < S_FIRST_FREE {
                ["t", "hour", "minute", "second", "a", "b", "c", "y", "py"][v as usize].into()
            } else if (v as usize) < MAX_SCALARS && self.is_const_slot(v) {
                alloc::format!("{}", self.init[v as usize] as f64 / ONE as f64)
            } else {
                alloc::format!("s{v}")
            }
        };
        for (n, op) in self.ops[..self.len()].iter().enumerate() {
            let sect = if n < self.frame_end as usize {
                "frame"
            } else if n < self.row_end as usize {
                "row  "
            } else {
                "pixel"
            };
            let (v, ar) = code::operands(op);
            let args = v[..ar]
                .iter()
                .map(|&v| opnd(v))
                .collect::<alloc::vec::Vec<_>>()
                .join(", ");
            // A pick's table: the slots it picks from.
            let args = if op.code == code::PICK {
                let t: alloc::vec::Vec<_> = (0..op.c as usize)
                    .map(|j| opnd(self.init[(op.b as usize + j) & (MAX_SCALARS - 1)] as u8))
                    .collect();
                alloc::format!("{args}; {}", t.join(", "))
            } else {
                args
            };
            let d = if code::is_colour(op.code) {
                "out".into()
            } else {
                opnd(op.d)
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

    fn is_const_slot(&self, v: u8) -> bool {
        !self.ops[..self.len()]
            .iter()
            .any(|op| !code::is_colour(op.code) && op.d == v)
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
        ADD => fixed::add(a, b),
        SUB => fixed::sub(a, b),
        MUL => fixed::mul(a, b),
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
        DIGIT..=DIGIT_B => t.map_or(0, |t| fixed::glyph(&glyphs(t, c), a, b, x)),
        _ => 0,
    }
}
