//! A compiled formula: a fixed-size value (no pointers, no heap), so it can
//! sit inline in an effect, be copied, or be built in place in a zeroed
//! allocation ([`Program::zeroed_in_place`]).

use crate::fixed::{self, ONE};

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
/// broadcast); the `OUT_*` op is always the last and writes the colour.
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
    pub const LAST_BIN: u8 = 19;
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
    pub const LAST_UN: u8 = 43;
    // ternary
    pub const SELECT: u8 = 64;
    pub const CLAMP: u8 = 65;
    pub const MIX: u8 = 66;
    pub const SMOOTH: u8 = 67;
    pub const NOISE: u8 = 68;
    pub const LAST_TERN: u8 = 68;
    // the colour
    pub const OUT_RGB: u8 = 96;
    pub const OUT_HSV: u8 = 97;
    pub const OUT_PAL: u8 = 98;
    pub const OUT_GRAY: u8 = 99;

    pub fn arity(c: u8) -> usize {
        match c {
            0..=LAST_BIN => 2,
            32..=LAST_UN => 1,
            64..=LAST_TERN => 3,
            OUT_RGB | OUT_HSV => 3,
            _ => 1,
        }
    }

    /// The step's name, for listings.
    pub fn name(c: u8) -> &'static str {
        const N: [&str; 20] = [
            "add", "sub", "mul", "div", "mod", "mask", "lt", "le", "gt", "ge", "eq", "ne", "and",
            "or", "min", "max", "step", "atan2", "hash", "len",
        ];
        const U: [&str; 12] = [
            "neg", "not", "abs", "floor", "frac", "sin", "cos", "tri", "saw", "sqr", "sqrt",
            "recip",
        ];
        const T: [&str; 5] = ["select", "clamp", "mix", "smooth", "noise"];
        match c {
            0..=LAST_BIN => N[c as usize],
            32..=LAST_UN => U[(c - 32) as usize],
            64..=LAST_TERN => T[(c - 64) as usize],
            OUT_RGB => "rgb",
            OUT_HSV => "hsv",
            OUT_PAL => "pal",
            _ => "gray",
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
    /// `..pixel_end` per pixel; `ops[pixel_end]` is the colour.
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
    /// Steps in all, the colour included (≤ [`MAX_OPS`]).
    pub fn len(&self) -> usize {
        self.pixel_end as usize + 1
    }

    /// Never: a compiled program has at least its colour.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Steps run once a frame, once a row, and per pixel (the colour
    /// counted with the pixel steps).
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.frame_end as usize,
            (self.row_end - self.frame_end) as usize,
            (self.pixel_end - self.row_end) as usize + 1,
        )
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
    /// the wall clock (`hour`, `minute` or `second`).
    pub fn inputs(&self) -> u8 {
        let mut m = 0;
        for op in &self.ops[..self.len()] {
            for &v in &[op.a, op.b, op.c][..code::arity(op.code)] {
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

    /// An estimate of a frame's render time on the ESP32 in µs, from the
    /// per-step costs `psi-probe` measured (docs/research/psi-spike.md,
    /// ns a pixel at 128 lanes over the `gray(x)` baseline of 160): the
    /// per-pixel steps are nearly all of it; a per-row step is counted at
    /// 32 runs of ~0.3 µs. Good to ~±25 % on the measured sketches; the
    /// web app's editor shows it as light / medium / heavy.
    pub fn esp32_us(&self) -> u32 {
        use code::*;
        let px: u32 = self.ops[self.row_end as usize..self.len()]
            .iter()
            .map(|op| match op.code {
                ADD | SUB | LT | LE | GT | GE | EQ | NE | AND | OR | MIN | MAX | STEP | MASK => 50,
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
                MIX => 130,
                SMOOTH => 214,
                NOISE => 855,
                OUT_HSV => 195,
                OUT_RGB => 120,
                OUT_PAL => 0,
                _ => 0,
            })
            .sum();
        let row = (self.row_end - self.frame_end) as u32;
        ((160 + px) * (W * H) as u32) / 1000 + row * 10
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
            let ar = code::arity(op.code);
            let args = [op.a, op.b, op.c][..ar]
                .iter()
                .map(|&v| opnd(v))
                .collect::<alloc::vec::Vec<_>>()
                .join(", ");
            let d = if op.code >= code::OUT_RGB {
                "out".into()
            } else {
                opnd(op.d)
            };
            let _ = writeln!(s, "{n:3} {sect} {d} = {}({args})", code::name(op.code));
        }
        s
    }

    fn is_const_slot(&self, v: u8) -> bool {
        !self.ops[..self.len()]
            .iter()
            .any(|op| op.code < code::OUT_RGB && op.d == v)
    }
}

/// The scalar evaluation of one arithmetic op: the per-frame and per-row
/// steps, and the constant folder (which never asks for `sin`/`cos`, the
/// only ops that need a table, hence the closure).
#[inline]
pub(crate) fn eval(c: u8, a: i32, b: i32, x: i32, sin: impl Fn(i32) -> i32) -> i32 {
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
        NEG => fixed::neg(a),
        NOT => fixed::not(a),
        ABS => fixed::abs(a),
        FLOOR => fixed::floor(a),
        FRAC => fixed::frac(a),
        SIN => sin(a),
        COS => sin(a.wrapping_add(ONE / 4)),
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
        _ => 0,
    }
}
