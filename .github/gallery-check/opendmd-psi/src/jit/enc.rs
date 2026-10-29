//! Xtensa LX6 instruction encoders: the forms a Psi kernel needs, as the
//! ESP32 configures the core (windowed registers, the 32-bit multiplier,
//! MINMAX, density, conditional moves). Each appends its bytes to a
//! [`Code`]; the host tests (`super::tests`) assemble the same mnemonics with
//! `xtensa-esp32-elf-as` and compare the bytes.
//!
//! Little-endian: a 24-bit instruction is three bytes, its fields from the
//! low bits up (`op0`, `t`, `s`, `r`, `op1`, `op2`); a 16-bit (density) one
//! two. Branch offsets are from the branch's own address plus 4.

/// An address register, a0..a15.
pub type Reg = u8;

/// The immediates `beqi`/`bnei`/`blti`/`bgei` take (`b4const`).
const B4CONST: [i32; 16] = [-1, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 16, 32, 64, 128, 256];
/// The immediates `bltui`/`bgeui` take (`b4constu`).
const B4CONSTU: [u32; 16] = [
    32768, 65536, 2, 3, 4, 5, 6, 7, 8, 10, 12, 16, 32, 64, 128, 256,
];

/// The `r` field of a two-register branch (`op0` 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    Eq = 1,
    Lt = 2,
    Ltu = 3,
    Ne = 9,
    Ge = 10,
    Geu = 11,
}

/// A branch on one register against zero (`BZ`, the `m` field).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zero {
    Eqz = 0,
    Nez = 1,
    Ltz = 2,
    Gez = 3,
}

/// A branch on one register against a `b4const` immediate (`BI0`, `m`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Imm {
    Eqi = 0,
    Nei = 1,
    Lti = 2,
    Gei = 3,
}

/// Where a forward branch's offset is to be written once its target is
/// known.
#[derive(Clone, Copy, Debug)]
pub struct Fixup {
    at: usize,
    kind: FixKind,
}

#[derive(Clone, Copy, Debug)]
enum FixKind {
    /// imm8 at bits 16..24 (RRI8 and BRI8 branches)
    B8,
    /// imm12 at bits 12..24 (BRI12)
    B12,
    /// offset18 at bits 6..24 (J)
    J18,
}

/// Encoding out of range: the generator gives up on the formula (the VM
/// draws it), never emits a wrong instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range;

/// The instructions so far.
#[derive(Default)]
pub struct Code {
    pub bytes: alloc::vec::Vec<u8>,
    /// Set by an encoder whose operand was out of range.
    pub bad: bool,
}

#[inline]
fn fits_s(v: i32, bits: u32) -> bool {
    let lim = 1i32 << (bits - 1);
    (-lim..lim).contains(&v)
}

impl Code {
    pub fn new() -> Code {
        Code::default()
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    fn check(&mut self, ok: bool) -> bool {
        if !ok {
            self.bad = true;
        }
        ok
    }

    fn reg(&mut self, r: Reg) -> u32 {
        self.check(r < 16);
        (r & 15) as u32
    }

    fn put24(&mut self, w: u32) {
        self.bytes
            .extend_from_slice(&[w as u8, (w >> 8) as u8, (w >> 16) as u8]);
    }

    fn put16(&mut self, w: u32) {
        self.bytes.extend_from_slice(&[w as u8, (w >> 8) as u8]);
    }

    /// RRR: op0 t s r op1 op2.
    fn rrr(&mut self, op0: u32, t: u32, s: u32, r: u32, op1: u32, op2: u32) {
        self.put24(op0 | t << 4 | s << 8 | r << 12 | op1 << 16 | op2 << 20);
    }

    /// RRI8: op0 t s r imm8.
    fn rri8(&mut self, op0: u32, t: u32, s: u32, r: u32, imm8: u32) {
        self.put24(op0 | t << 4 | s << 8 | r << 12 | (imm8 & 0xff) << 16);
    }

    // --- ALU (RRR, op0 0) ------------------------------------------------

    fn op_rrr(&mut self, op1: u32, op2: u32, ar: Reg, as_: Reg, at: Reg) {
        let (r, s, t) = (self.reg(ar), self.reg(as_), self.reg(at));
        self.rrr(0, t, s, r, op1, op2);
    }

    pub fn and(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(0, 1, ar, as_, at)
    }
    pub fn or(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(0, 2, ar, as_, at)
    }
    /// `mov ar, as` is `or ar, as, as`.
    pub fn mov(&mut self, ar: Reg, as_: Reg) {
        if ar != as_ {
            self.mov_n(ar, as_)
        }
    }
    pub fn xor(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(0, 3, ar, as_, at)
    }
    pub fn add(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(0, 8, ar, as_, at)
    }
    /// `ar = (as << 1) + at`
    pub fn addx2(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(0, 9, ar, as_, at)
    }
    /// `ar = (as << 2) + at`
    pub fn addx4(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(0, 10, ar, as_, at)
    }
    pub fn sub(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(0, 12, ar, as_, at)
    }
    /// `ar = -at` (wrapping)
    pub fn neg(&mut self, ar: Reg, at: Reg) {
        let (r, t) = (self.reg(ar), self.reg(at));
        self.rrr(0, t, 0, r, 0, 6);
    }
    /// `ar = |at|` (wrapping: |i32::MIN| is i32::MIN)
    pub fn abs(&mut self, ar: Reg, at: Reg) {
        let (r, t) = (self.reg(ar), self.reg(at));
        self.rrr(0, t, 1, r, 0, 6);
    }
    /// Low 32 bits of `as · at`.
    pub fn mull(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(2, 8, ar, as_, at)
    }
    /// High 32 bits of the unsigned product.
    pub fn muluh(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(2, 10, ar, as_, at)
    }
    /// High 32 bits of the signed product.
    pub fn mulsh(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(2, 11, ar, as_, at)
    }
    pub fn min(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(3, 4, ar, as_, at)
    }
    pub fn max(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(3, 5, ar, as_, at)
    }
    pub fn minu(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(3, 6, ar, as_, at)
    }
    pub fn maxu(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(3, 7, ar, as_, at)
    }
    /// `if at == 0 { ar = as }`
    pub fn moveqz(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(3, 8, ar, as_, at)
    }
    /// `if at != 0 { ar = as }`
    pub fn movnez(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(3, 9, ar, as_, at)
    }
    /// `if at < 0 { ar = as }`
    pub fn movltz(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(3, 10, ar, as_, at)
    }
    /// `if at >= 0 { ar = as }`
    pub fn movgez(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(3, 11, ar, as_, at)
    }

    // --- Shifts ------------------------------------------------------------

    /// `ar = as << sa`, sa 1..=31.
    pub fn slli(&mut self, ar: Reg, as_: Reg, sa: u32) {
        if !self.check((1..32).contains(&sa)) {
            return;
        }
        let (r, s) = (self.reg(ar), self.reg(as_));
        let k = 32 - sa;
        self.rrr(0, k & 15, s, r, 1, k >> 4);
    }
    /// `ar = at >> sa` (arithmetic), sa 0..=31.
    pub fn srai(&mut self, ar: Reg, at: Reg, sa: u32) {
        if !self.check(sa < 32) {
            return;
        }
        let (r, t) = (self.reg(ar), self.reg(at));
        self.rrr(0, t, sa & 15, r, 1, 2 | (sa >> 4));
    }
    /// `ar = at >> sa` (logical), sa 0..=15.
    pub fn srli(&mut self, ar: Reg, at: Reg, sa: u32) {
        if !self.check(sa < 16) {
            return;
        }
        let (r, t) = (self.reg(ar), self.reg(at));
        self.rrr(0, t, sa, r, 1, 4);
    }
    /// `ar = (at >> shift) & (2^bits - 1)`, shift 0..=31, bits 1..=16.
    pub fn extui(&mut self, ar: Reg, at: Reg, shift: u32, bits: u32) {
        if !self.check(shift < 32 && (1..=16).contains(&bits)) {
            return;
        }
        let (r, t) = (self.reg(ar), self.reg(at));
        self.rrr(0, t, shift & 15, r, 4 | (shift >> 4), bits - 1);
    }
    /// SAR = sa (a right shift amount for `src`), 0..=31.
    pub fn ssai(&mut self, sa: u32) {
        if !self.check(sa < 32) {
            return;
        }
        self.rrr(0, sa >> 4, sa & 15, 4, 0, 4);
    }
    /// `ar = (as:at) >> SAR` (the low 32 bits of the 64-bit pair, as high).
    pub fn src(&mut self, ar: Reg, as_: Reg, at: Reg) {
        self.op_rrr(1, 8, ar, as_, at)
    }

    // --- Loads, stores, immediates (RRI8, op0 2) ------------------------------

    fn lsai(&mut self, sub: u32, at: Reg, as_: Reg, imm8: i32) {
        if !self.check((0..256).contains(&imm8)) {
            return;
        }
        let (t, s) = (self.reg(at), self.reg(as_));
        self.rri8(2, t, s, sub, imm8 as u32);
    }

    pub fn l8ui(&mut self, at: Reg, as_: Reg, off: i32) {
        self.lsai(0, at, as_, off)
    }
    pub fn l16si(&mut self, at: Reg, as_: Reg, off: i32) {
        if self.check(off & 1 == 0) {
            self.lsai(9, at, as_, off >> 1)
        }
    }
    /// A 32-bit load; `l32i.n` when the offset allows.
    pub fn l32i(&mut self, at: Reg, as_: Reg, off: i32) {
        if !self.check(off & 3 == 0) {
            return;
        }
        if (0..=60).contains(&off) {
            let (t, s) = (self.reg(at), self.reg(as_));
            self.put16(8 | t << 4 | s << 8 | ((off >> 2) as u32) << 12);
        } else {
            self.lsai(2, at, as_, off >> 2)
        }
    }
    /// `l32i` in its 24-bit form only (the tests).
    pub fn l32i_wide(&mut self, at: Reg, as_: Reg, off: i32) {
        if self.check(off & 3 == 0) {
            self.lsai(2, at, as_, off >> 2)
        }
    }
    pub fn s8i(&mut self, at: Reg, as_: Reg, off: i32) {
        self.lsai(4, at, as_, off)
    }
    /// A 32-bit store; `s32i.n` when the offset allows.
    pub fn s32i(&mut self, at: Reg, as_: Reg, off: i32) {
        if !self.check(off & 3 == 0) {
            return;
        }
        if (0..=60).contains(&off) {
            let (t, s) = (self.reg(at), self.reg(as_));
            self.put16(9 | t << 4 | s << 8 | ((off >> 2) as u32) << 12);
        } else {
            self.lsai(6, at, as_, off >> 2)
        }
    }
    /// `s32i` in its 24-bit form only (the tests).
    pub fn s32i_wide(&mut self, at: Reg, as_: Reg, off: i32) {
        if self.check(off & 3 == 0) {
            self.lsai(6, at, as_, off >> 2)
        }
    }
    /// `at = imm`, -2048..=2047; `movi.n` when it allows (-32..=95).
    pub fn movi(&mut self, at: Reg, imm: i32) {
        if (-32..=95).contains(&imm) {
            return self.movi_n(at, imm);
        }
        self.movi_wide(at, imm)
    }
    /// `movi` in its 24-bit form only.
    pub fn movi_wide(&mut self, at: Reg, imm: i32) {
        if !self.check(fits_s(imm, 12)) {
            return;
        }
        let t = self.reg(at);
        let v = imm as u32 & 0xfff;
        self.rri8(2, t, v >> 8, 10, v & 0xff);
    }
    /// `at = as + imm`, -128..=127; `addi.n` when it allows (-1, 1..=15).
    pub fn addi(&mut self, at: Reg, as_: Reg, imm: i32) {
        if imm == 0 {
            return self.mov(at, as_);
        }
        if imm == -1 || (1..=15).contains(&imm) {
            return self.addi_n(at, as_, imm);
        }
        self.addi_wide(at, as_, imm)
    }
    pub fn addi_wide(&mut self, at: Reg, as_: Reg, imm: i32) {
        if !self.check(fits_s(imm, 8)) {
            return;
        }
        let (t, s) = (self.reg(at), self.reg(as_));
        self.rri8(2, t, s, 12, imm as u32 & 0xff);
    }
    /// `at = as + imm`, imm a multiple of 256 in -32768..=32512.
    pub fn addmi(&mut self, at: Reg, as_: Reg, imm: i32) {
        if !self.check(imm & 255 == 0 && fits_s(imm >> 8, 8)) {
            return;
        }
        let (t, s) = (self.reg(at), self.reg(as_));
        self.rri8(2, t, s, 13, ((imm >> 8) as u32) & 0xff);
    }

    // --- Density -------------------------------------------------------------

    pub fn add_n(&mut self, ar: Reg, as_: Reg, at: Reg) {
        let (r, s, t) = (self.reg(ar), self.reg(as_), self.reg(at));
        self.put16(10 | t << 4 | s << 8 | r << 12);
    }
    pub fn addi_n(&mut self, ar: Reg, as_: Reg, imm: i32) {
        if !self.check(imm == -1 || (1..=15).contains(&imm)) {
            return;
        }
        let (r, s) = (self.reg(ar), self.reg(as_));
        let t = if imm == -1 { 0 } else { imm as u32 };
        self.put16(11 | t << 4 | s << 8 | r << 12);
    }
    pub fn mov_n(&mut self, at: Reg, as_: Reg) {
        let (t, s) = (self.reg(at), self.reg(as_));
        self.put16(13 | t << 4 | s << 8);
    }
    pub fn movi_n(&mut self, as_: Reg, imm: i32) {
        if !self.check((-32..=95).contains(&imm)) {
            return;
        }
        let s = self.reg(as_);
        let v = imm as u32 & 0x7f;
        self.put16(12 | (v >> 4) << 4 | s << 8 | (v & 15) << 12);
    }
    pub fn retw_n(&mut self) {
        self.put16(0xf01d);
    }

    // --- Windowed calls ----------------------------------------------------

    /// `entry a1, frame`: frame a multiple of 8, ≤ 32,760.
    pub fn entry(&mut self, frame: u32) {
        if !self.check(frame.is_multiple_of(8) && frame < 32768) {
            return;
        }
        self.put24(6 | 3 << 4 | 1 << 8 | (frame >> 3) << 12);
    }
    /// `callx8 as`: windowed call through a register (callee's arguments
    /// in a10.., its result in a10; a8..a15 not kept).
    pub fn callx8(&mut self, as_: Reg) {
        let s = self.reg(as_);
        self.rrr(0, 0xe, s, 0, 0, 0);
    }
    pub fn memw(&mut self) {
        self.rrr(0, 0xc, 0, 2, 0, 0);
    }

    // --- Branches ----------------------------------------------------------

    /// The current address, a branch target.
    pub fn here(&self) -> usize {
        self.bytes.len()
    }

    /// A two-register branch to `to` (an address already emitted, or the
    /// branch's own `here()` and a fixup later).
    pub fn b(&mut self, c: Cond, as_: Reg, at: Reg, to: usize) {
        let off = to as i32 - self.here() as i32 - 4;
        let (s, t) = (self.reg(as_), self.reg(at));
        if !self.check(fits_s(off, 8)) {
            self.rri8(7, t, s, c as u32, 0);
            return;
        }
        self.rri8(7, t, s, c as u32, off as u32 & 0xff);
    }
    /// `bbci as, bit, to`: branch if bit `bit` (0..=31) of `as` is clear.
    pub fn bbci(&mut self, as_: Reg, bit: u32, to: usize) {
        let off = to as i32 - self.here() as i32 - 4;
        let s = self.reg(as_);
        let ok = self.check(fits_s(off, 8) && bit < 32);
        self.rri8(
            7,
            bit & 15,
            s,
            6 | (bit >> 4 & 1),
            if ok { off as u32 } else { 0 },
        );
    }
    /// A branch on `as` against zero (12-bit offset).
    pub fn bz(&mut self, z: Zero, as_: Reg, to: usize) {
        let off = to as i32 - self.here() as i32 - 4;
        let s = self.reg(as_);
        let ok = self.check(fits_s(off, 12));
        let imm = if ok { off as u32 & 0xfff } else { 0 };
        self.put24(6 | 1 << 4 | (z as u32) << 6 | s << 8 | imm << 12);
    }
    /// A branch on `as` against a `b4const` immediate.
    pub fn bi(&mut self, m: Imm, as_: Reg, imm: i32, to: usize) {
        let off = to as i32 - self.here() as i32 - 4;
        let s = self.reg(as_);
        let k = B4CONST.iter().position(|&c| c == imm);
        let ok = self.check(fits_s(off, 8) && k.is_some());
        let k = k.unwrap_or(0) as u32;
        self.put24(
            6 | 2 << 4
                | (m as u32) << 6
                | s << 8
                | k << 12
                | if ok { (off as u32 & 0xff) << 16 } else { 0 },
        );
    }
    /// `bltui`/`bgeui` (`ge`): `as` unsigned against a `b4constu`.
    pub fn bui(&mut self, ge: bool, as_: Reg, imm: u32, to: usize) {
        let off = to as i32 - self.here() as i32 - 4;
        let s = self.reg(as_);
        let k = B4CONSTU.iter().position(|&c| c == imm);
        let ok = self.check(fits_s(off, 8) && k.is_some());
        let k = k.unwrap_or(0) as u32;
        let m = if ge { 3 } else { 2 };
        self.put24(
            6 | 3 << 4 | m << 6 | s << 8 | k << 12 | if ok { (off as u32 & 0xff) << 16 } else { 0 },
        );
    }
    /// `j to` (18-bit offset).
    pub fn j(&mut self, to: usize) {
        let off = to as i32 - self.here() as i32 - 4;
        let ok = self.check(fits_s(off, 18));
        let imm = if ok { off as u32 & 0x3ffff } else { 0 };
        self.put24(6 | imm << 6);
    }

    /// Whether `b4const` has `imm` (for `bi`).
    pub fn b4const(imm: i32) -> bool {
        B4CONST.contains(&imm)
    }

    // Forward branches: emit with a placeholder target, patch later.

    /// A two-register branch to a later place.
    pub fn b_fwd(&mut self, c: Cond, as_: Reg, at: Reg) -> Fixup {
        let at_ = self.here();
        self.b(c, as_, at, at_ + 4);
        Fixup {
            at: at_,
            kind: FixKind::B8,
        }
    }
    pub fn bbci_fwd(&mut self, as_: Reg, bit: u32) -> Fixup {
        let at_ = self.here();
        self.bbci(as_, bit, at_ + 4);
        Fixup {
            at: at_,
            kind: FixKind::B8,
        }
    }
    pub fn bz_fwd(&mut self, z: Zero, as_: Reg) -> Fixup {
        let at_ = self.here();
        self.bz(z, as_, at_ + 4);
        Fixup {
            at: at_,
            kind: FixKind::B12,
        }
    }
    pub fn bi_fwd(&mut self, m: Imm, as_: Reg, imm: i32) -> Fixup {
        let at_ = self.here();
        self.bi(m, as_, imm, at_ + 4);
        Fixup {
            at: at_,
            kind: FixKind::B8,
        }
    }
    pub fn bui_fwd(&mut self, ge: bool, as_: Reg, imm: u32) -> Fixup {
        let at_ = self.here();
        self.bui(ge, as_, imm, at_ + 4);
        Fixup {
            at: at_,
            kind: FixKind::B8,
        }
    }
    pub fn j_fwd(&mut self) -> Fixup {
        let at_ = self.here();
        self.j(at_ + 4);
        Fixup {
            at: at_,
            kind: FixKind::J18,
        }
    }

    /// Point `f` at the current address.
    pub fn bind(&mut self, f: Fixup) {
        let to = self.here();
        self.bind_to(f, to)
    }

    pub fn bind_to(&mut self, f: Fixup, to: usize) {
        let off = to as i32 - f.at as i32 - 4;
        let w = u32::from_le_bytes([
            self.bytes[f.at],
            self.bytes[f.at + 1],
            self.bytes[f.at + 2],
            0,
        ]);
        let w = match f.kind {
            FixKind::B8 => {
                if !self.check(fits_s(off, 8)) {
                    return;
                }
                (w & 0x00ffff) | (off as u32 & 0xff) << 16
            }
            FixKind::B12 => {
                if !self.check(fits_s(off, 12)) {
                    return;
                }
                (w & 0x000fff) | (off as u32 & 0xfff) << 12
            }
            FixKind::J18 => {
                if !self.check(fits_s(off, 18)) {
                    return;
                }
                (w & 0x3f) | (off as u32 & 0x3ffff) << 6
            }
        };
        self.bytes[f.at..f.at + 3].copy_from_slice(&w.to_le_bytes()[..3]);
    }
}
