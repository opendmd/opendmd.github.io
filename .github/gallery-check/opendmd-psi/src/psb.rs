//! A compiled Formula as bytes: what the browser (or the firmware's build,
//! or the `psi` command) hands the player, which never compiles.
//!
//! **Why.** The compiler is ~110 KB of the firmware and needs 12 KB of
//! stack and up to ~40 KB of heap while it runs; the browser compiles
//! every Formula for its preview anyway. So the player gets a program
//! ([`encode`]), checks it ([`verify`]: it comes over the network, and the
//! JIT makes machine code from it), and only then finishes it for itself
//! ([`Program::finish`]: the narrowed steps, the fused runs and the lane
//! loops, which it works out from the program and never takes on trust).
//!
//! **The format** (little-endian):
//!
//! ```text
//! "PSB" ABI(u8)  flags(u8: bit 0, a look program follows; bit 1, the
//!                       programs declare controls or a look: their meta)
//! name_len(u8) name(UTF-8, ≤ 60)
//! program [look program]
//! program = len(u16) header ops init tables meta   (len: the bytes after it)
//! header  = frame_end draw_end row_end pixel_end (u16 each)
//!           n_scalars regs uses frame_ms (u8 each)  loop_ms(u32)
//! ops     = pixel_end × (code d a b c)
//! init    = (n_scalars − 9) × i32   (the inputs' slots are always 0)
//! tables  = lo(u16) pals(u8)  lo × i32 (from 0)  pals × 32 × i32 (from the top)
//! paint   = n(u8)  n × u32 (the paint palette from entry 0)
//! meta    = kind[3] look amount (u8)  lo[3] hi[3] def[3] (i32)   (with flag bit 1)
//! ```
//!
//! A program is *portable*: laid out by the compiler but not narrowed or
//! fused, and with no lane-loop marks (every `FOR`'s `c` 0). [`ABI`]
//! changes whenever what a program means does; a player refuses another
//! ABI's bytes, and the web app prepares the Formula again.

use crate::fixed;
use crate::meta::Meta;
use crate::program::{
    code, Op, Program, LANE, L_REG0, MAX_OPS, MAX_SCALARS, MAX_TABLE, REGS, S_FIRST_FREE, WIDE,
};
use alloc::string::String;
use alloc::vec::Vec;

/// The program format and the meaning of its steps. Bumped with any change
/// to either; the web app reads the player's from `/api/status`
/// (`psi_abi`) and the www pack carries the compiler that writes it.
pub const ABI: u8 = 1;

const MAGIC: &[u8; 3] = b"PSB";
const F_LOOK: u8 = 1;
const F_META: u8 = 2;
/// A Formula's name: 60 characters (the compiler's `MAX_NAME_TEXT`), so
/// at most this many bytes.
pub const MAX_NAME: usize = 240;

/// Why bytes are not a program the player can play.
pub type Refusal = &'static str;

/// Not a compiled Formula at all.
pub const NOT_PSB: Refusal = "not a compiled Formula";
/// Compiled for another version of the player.
pub const OTHER_ABI: Refusal =
    "this Formula was prepared for another version of OpenDMD: open the OpenDMD app to prepare it again";

/// Which program of a file to load.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Which {
    /// The Formula as it plays on its own.
    Play,
    /// As the panel's look (its `look` line's blend and amount composed
    /// in): the play program when the file has no look program of its own.
    Look,
}

/// The bytes for a Formula: its `name` and its programs (portable: see the
/// module's docs; [`crate::compile_portable`] makes them).
pub fn encode(name: &str, play: &Program, look: Option<&Program>) -> Vec<u8> {
    let mut o = Vec::with_capacity(1024);
    o.extend_from_slice(MAGIC);
    o.push(ABI);
    let meta =
        *play.meta() != Meta::default() || look.is_some_and(|l| *l.meta() != Meta::default());
    o.push(if look.is_some() { F_LOOK } else { 0 } | if meta { F_META } else { 0 });
    let cut = name
        .char_indices()
        .nth(60)
        .map_or(name.len(), |(i, _)| i)
        .min(MAX_NAME);
    o.push(cut as u8);
    o.extend_from_slice(&name.as_bytes()[..cut]);
    put_program(&mut o, play, meta);
    if let Some(l) = look {
        put_program(&mut o, l, meta);
    }
    o
}

fn put_program(o: &mut Vec<u8>, p: &Program, meta: bool) {
    let at = o.len();
    o.extend_from_slice(&[0, 0]);
    for v in [p.frame_end, p.draw_end, p.row_end, p.pixel_end] {
        o.extend_from_slice(&v.to_le_bytes());
    }
    o.extend_from_slice(&[p.n_scalars, p.regs, p.uses, p.frame_ms]);
    o.extend_from_slice(&p.loop_ms.to_le_bytes());
    for op in &p.ops[..(p.pixel_end as usize).min(MAX_OPS)] {
        // A lane loop's mark is the player's to make.
        let c = if op.code == code::FOR { 0 } else { op.c };
        o.extend_from_slice(&[op.code, op.d, op.a, op.b, c]);
    }
    debug_assert!(p.init[..S_FIRST_FREE as usize].iter().all(|&v| v == 0));
    for v in &p.init[S_FIRST_FREE as usize..(p.n_scalars as usize).min(MAX_SCALARS)] {
        o.extend_from_slice(&v.to_le_bytes());
    }
    // The formula's own palettes sit at the top of the tables, the rest
    // from the bottom: the zeros between them are left out.
    let pals = (0..crate::program::USER_PALETTES)
        .rev()
        .find(|&j| {
            let a = crate::program::user_palette_at(j);
            p.tables[a..a + 32].iter().any(|&v| v != 0)
        })
        .map_or(0, |j| j + 1);
    let top = MAX_TABLE - 32 * pals;
    let lo = p.tables[..top]
        .iter()
        .rposition(|&v| v != 0)
        .map_or(0, |k| k + 1);
    o.extend_from_slice(&(lo as u16).to_le_bytes());
    o.push(pals as u8);
    for v in p.tables[..lo].iter().chain(&p.tables[top..]) {
        o.extend_from_slice(&v.to_le_bytes());
    }
    let np = p.paint.iter().rposition(|&c| c != 0).map_or(0, |k| k + 1);
    o.push(np as u8);
    for c in &p.paint[..np] {
        o.extend_from_slice(&c.to_le_bytes());
    }
    let m = &p.meta;
    if !meta {
        let len = (o.len() - at - 2) as u16;
        o[at..at + 2].copy_from_slice(&len.to_le_bytes());
        return;
    }
    o.extend_from_slice(&m.kind);
    o.push(m.look);
    o.push(m.amount);
    for v in m.lo.iter().chain(&m.hi).chain(&m.def) {
        o.extend_from_slice(&v.to_le_bytes());
    }
    let len = (o.len() - at - 2) as u16;
    o[at..at + 2].copy_from_slice(&len.to_le_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Refusal> {
        let s = self
            .b
            .get(self.at..self.at.checked_add(n).ok_or(NOT_PSB)?)
            .ok_or(NOT_PSB)?;
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, Refusal> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Refusal> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, Refusal> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32, Refusal> {
        Ok(self.u32()? as i32)
    }
}

/// The header: (whether it has a look program, its name, where its first
/// program starts).
fn head(b: &[u8]) -> Result<(u8, &str, usize), Refusal> {
    if b.len() < 6 || &b[..3] != MAGIC {
        return Err(NOT_PSB);
    }
    if b[3] != ABI {
        return Err(OTHER_ABI);
    }
    let mut r = Reader { b, at: 4 };
    let flags = r.u8()?;
    if flags & !(F_LOOK | F_META) != 0 {
        return Err(NOT_PSB);
    }
    let n = r.u8()? as usize;
    if n > MAX_NAME {
        return Err(NOT_PSB);
    }
    let name = core::str::from_utf8(r.take(n)?).map_err(|_| NOT_PSB)?;
    Ok((flags, name, r.at))
}

/// A compiled Formula's name ("" when it has none), or why the bytes are
/// not one.
pub fn name(b: &[u8]) -> Result<&str, Refusal> {
    head(b).map(|h| h.1)
}

/// Whether the bytes carry a look program of their own.
pub fn has_look(b: &[u8]) -> Result<bool, Refusal> {
    head(b).map(|h| h.0 & F_LOOK != 0)
}

/// Load program `which` of `b` into `prog`, check it ([`verify`]) and
/// finish it for the player ([`Program::finish`]). On a refusal `prog` is
/// left empty.
pub fn load_into(b: &[u8], which: Which, prog: &mut Program) -> Result<(), Refusal> {
    let r = load_raw(b, which, prog).and_then(|()| verify(prog));
    match r {
        Ok(()) => {
            prog.finish(true, true);
            Ok(())
        }
        Err(e) => {
            prog.clear();
            Err(e)
        }
    }
}

/// [`load_into`] without finishing (the program as encoded, verified):
/// tests, and tools that re-encode.
#[doc(hidden)]
pub fn load_portable(b: &[u8], which: Which, prog: &mut Program) -> Result<(), Refusal> {
    let r = load_raw(b, which, prog).and_then(|()| verify(prog));
    if r.is_err() {
        prog.clear();
    }
    r
}

fn load_raw(b: &[u8], which: Which, prog: &mut Program) -> Result<(), Refusal> {
    let (flags, _, at) = head(b)?;
    let look = flags & F_LOOK != 0;
    let mut r = Reader { b, at };
    let len = r.u16()? as usize;
    let play = r.take(len)?;
    let own = if look {
        let len = r.u16()? as usize;
        Some(r.take(len)?)
    } else {
        None
    };
    if r.at != b.len() {
        return Err(NOT_PSB);
    }
    let body = match which {
        Which::Look => own.unwrap_or(play),
        Which::Play => play,
    };
    prog.clear();
    let mut r = Reader { b: body, at: 0 };
    prog.frame_end = r.u16()?;
    prog.draw_end = r.u16()?;
    prog.row_end = r.u16()?;
    prog.pixel_end = r.u16()?;
    prog.n_scalars = r.u8()?;
    prog.regs = r.u8()?;
    prog.uses = r.u8()?;
    prog.frame_ms = r.u8()?;
    prog.loop_ms = r.u32()?;
    let n = prog.pixel_end as usize;
    if n > MAX_OPS || prog.n_scalars as usize > MAX_SCALARS {
        return Err("a program bigger than the player holds");
    }
    for k in 0..n {
        let s = r.take(5)?;
        prog.ops[k] = Op {
            code: s[0],
            d: s[1],
            a: s[2],
            b: s[3],
            c: s[4],
        };
    }
    if (prog.n_scalars as usize) < S_FIRST_FREE as usize {
        return Err(NOT_PSB);
    }
    for k in S_FIRST_FREE as usize..prog.n_scalars as usize {
        prog.init[k] = r.i32()?;
    }
    let lo = r.u16()? as usize;
    let pals = r.u8()? as usize;
    if pals > crate::program::USER_PALETTES || lo > MAX_TABLE - 32 * pals {
        return Err(NOT_PSB);
    }
    for k in 0..lo {
        prog.tables[k] = r.i32()?;
    }
    for k in MAX_TABLE - 32 * pals..MAX_TABLE {
        prog.tables[k] = r.i32()?;
    }
    let np = r.u8()? as usize;
    if np > crate::program::MAX_PAINT {
        return Err(NOT_PSB);
    }
    for k in 0..np {
        prog.paint[k] = r.u32()? & 0xff_ffff;
    }
    let mut m = Meta::default();
    if flags & F_META == 0 {
        prog.meta = m;
        return if r.at == body.len() {
            Ok(())
        } else {
            Err(NOT_PSB)
        };
    }
    for k in 0..3 {
        m.kind[k] = r.u8()?;
    }
    m.look = r.u8()?;
    m.amount = r.u8()?;
    for v in
        m.lo.iter_mut()
            .chain(m.hi.iter_mut())
            .chain(m.def.iter_mut())
    {
        *v = r.i32()?;
    }
    prog.meta = m;
    if r.at != body.len() {
        return Err(NOT_PSB);
    }
    Ok(())
}

/// Check a portable program: everything the VM, the draw stage, the still
/// cache, the slow layers and the JIT's lowering take as given from the
/// compiler. Rejects, never repairs.
pub fn verify(p: &Program) -> Result<(), Refusal> {
    use code::*;
    let bad = Err("a damaged Formula: open the OpenDMD app to prepare it again");
    let (fe, de, re, pe) = (
        p.frame_end as usize,
        p.draw_end as usize,
        p.row_end as usize,
        p.pixel_end as usize,
    );
    let ns = p.n_scalars as usize;
    if !(fe <= de && de <= re && re < pe && pe <= MAX_OPS) {
        return bad;
    }
    if !(S_FIRST_FREE as usize..=MAX_SCALARS).contains(&ns) || p.regs as usize > REGS {
        return bad;
    }
    if p.uses & !127 != 0 || !matches!(p.frame_ms, 0 | 10 | 20 | 40) || p.loop_ms > 3_600_000 {
        return bad;
    }
    if verify_meta(&p.meta).is_err() {
        return bad;
    }
    let regs = p.regs;
    // A scalar operand: a slot in use. A lane: x, px, i or a register in use.
    let scalar = |v: u8| v & LANE == 0 && (v as usize) < ns;
    let lane = |v: u8| v & LANE != 0 && ((v & !LANE) < L_REG0 || (v & !LANE) - L_REG0 < regs);
    let reg = |v: u8| v & LANE != 0 && (v & !LANE) >= L_REG0 && (v & !LANE) - L_REG0 < regs;
    let operand = |v: u8, pixel: bool| scalar(v) || (pixel && lane(v));
    // The steps a section may hold, and their operands.
    let mut depth = 0usize;
    let mut fors = [0usize; 2];
    let mut line_open = false;
    for (k, op) in p.ops[..pe].iter().enumerate() {
        let c = op.code;
        let pixel = k >= re;
        let draw = (fe..de).contains(&k);
        let row = (de..re).contains(&k);
        // Steps the player works out itself (narrowing, fusing).
        if matches!(c, ADD_W | SUB_W | MUL_W | MUL_N | SHR | FUSE) {
            return bad;
        }
        let known = matches!(c, 0..=LAST_BIN | ARGS..=UAT | 32..=LAST_UN | PALIDX | W_FIRST..=W_LAST | 64..=LAST_TERN)
            || matches!(c, U_R..=INKN | TURN | SLOW | FOR..=COLUMN)
            || is_colour(c) && colour_ok(c);
        if !known {
            return bad;
        }
        // Where a step may be.
        let placed = match c {
            FOR | END | LINE_FROM | LINE_TO | DOT | ARGS | PAINT | IF | JUMP => draw,
            PALIDX => !pixel,
            TURN => row,
            SLOW => k == re,
            STILL | COLUMN | U_R..=INKN | PICK | UAT => pixel,
            W_FIRST..=W_LAST => !pixel && !row,
            _ if is_colour(c) => pixel,
            _ => true,
        };
        if !placed {
            return bad;
        }
        if c == TURN {
            // The row section's last steps only.
            if p.ops[k + 1..re].iter().any(|o| o.code != TURN)
                || op.d > 2
                || op.b > 7
                || !scalar(op.a)
            {
                return bad;
            }
            continue;
        }
        if is_wide(c) {
            let (wide_out, n, w) = wide_ops(c);
            for (i, &v) in [op.a, op.b, op.c].iter().take(n).enumerate() {
                let ok = if w & (1 << i) != 0 {
                    (v as usize) < WIDE
                } else {
                    scalar(v)
                };
                if !ok {
                    return bad;
                }
            }
            let d_ok = if c == W_LAT {
                (op.d as usize) < MAX_SCALARS
            } else if wide_out {
                (op.d as usize) < WIDE
            } else {
                scalar(op.d)
            };
            if !d_ok
                || (c == W_CMP && !(LT..=NE).contains(&op.c))
                || (c == W_MINMAX && !matches!(op.c, MIN | MAX))
            {
                return bad;
            }
            continue;
        }
        match c {
            FOR => {
                if depth == 2 || !scalar(op.d) || !scalar(op.a) || op.c != 0 {
                    return bad;
                }
                let n = p.init[op.a as usize];
                if n & 0xffff != 0 || !(1..=1024).contains(&(n >> 16)) {
                    return bad;
                }
                // Its END.
                let e = op.b as usize;
                if e <= k
                    || e >= de
                    || p.ops[e].code != END
                    || p.ops[e].a != op.d
                    || p.ops[e].b != op.a
                    || p.ops[e].c as usize != k + 1
                {
                    return bad;
                }
                fors[depth] = k;
                depth += 1;
                continue;
            }
            END => {
                if depth == 0 || p.ops[fors[depth - 1]].b as usize != k {
                    return bad;
                }
                depth -= 1;
                continue;
            }
            LINE_FROM => {
                if line_open
                    || ![op.a, op.b, op.c].iter().all(|&v| scalar(v))
                    || p.ops.get(k + 1).map(|o| o.code) != Some(LINE_TO)
                {
                    return bad;
                }
                line_open = true;
                continue;
            }
            LINE_TO => {
                if !line_open || ![op.a, op.b, op.c].iter().all(|&v| scalar(v)) {
                    return bad;
                }
                line_open = false;
                continue;
            }
            DOT | ARGS => {
                if ![op.a, op.b, op.c].iter().all(|&v| scalar(v)) {
                    return bad;
                }
                continue;
            }
            PAINT => {
                if op.d > crate::program::paint::LAST
                    || ![op.a, op.b, op.c].iter().all(|&v| scalar(v))
                {
                    return bad;
                }
                continue;
            }
            IF | JUMP => {
                // Forward, inside the draw stage, and not into or out of a
                // loop (the loops' FOR and END balance between).
                let to = op.b as usize;
                if (c == IF && !scalar(op.a)) || to <= k || to > de {
                    return bad;
                }
                let mut d = 0i32;
                for o in &p.ops[k + 1..to] {
                    d += i32::from(o.code == FOR) - i32::from(o.code == END);
                    if d < 0 {
                        return bad;
                    }
                }
                if d != 0 {
                    return bad;
                }
                continue;
            }
            PALIDX => {
                if !scalar(op.a)
                    || !scalar(op.d)
                    || op.d < S_FIRST_FREE
                    || op.b as usize + 32 > crate::program::MAX_PAINT
                {
                    return bad;
                }
                continue;
            }
            SLOW => {
                if !(2..=crate::rate::MAX_RATE as u8).contains(&op.d)
                    || k + 1 + op.a as usize > pe
                    || op.c & !15 != 0
                {
                    return bad;
                }
                continue;
            }
            STILL => {
                let n = op.d as usize;
                if !(1..=crate::still::STILL_LANES).contains(&n)
                    || k + 1 + op.a as usize > pe
                    || !reg(op.b)
                    || (n == 2 && (!reg(op.c) || op.c == op.b))
                    || p.ops[re..pe].iter().filter(|o| o.code == STILL).count() != 1
                {
                    return bad;
                }
                continue;
            }
            COLUMN => {
                if k + 1 + op.a as usize > pe
                    || p.ops[re..pe].iter().filter(|o| o.code == COLUMN).count() != 1
                {
                    return bad;
                }
                continue;
            }
            _ => {}
        }
        // Value operands, then the destination.
        let (vs, n) = operands(op);
        let n_vals = values(op);
        for (i, &v) in vs.iter().take(n).enumerate() {
            // A pixel step's immediates are not operands: `pick`'s and
            // `tab`'s `b`/`c`, `spr`'s `c`, `inkn`'s `a`.
            let imm = match c {
                PICK | TAB => i > 0,
                SPR => i > 1,
                _ => false,
            };
            if imm {
                continue;
            }
            // A colour's coverage (past its values) is a value too.
            let _ = n_vals;
            if !operand(v, pixel) {
                return bad;
            }
        }
        match c {
            TAB => {
                if op.b as usize + op.c as usize >= MAX_TABLE {
                    return bad;
                }
            }
            SPR => {
                if !spr_ok(p, op.c) {
                    return bad;
                }
            }
            PICK => {
                let (b, n) = (op.b as usize, op.c as usize);
                if n == 0
                    || b + n > ns
                    || p.init[b..b + n].iter().any(|&s| s < 0 || s as usize >= ns)
                {
                    return bad;
                }
            }
            // `ink(n)`: n a constant slot (1..=255).
            INKN if !scalar(op.a) => return bad,
            UAT if op.c > 2 => return bad,
            _ => {}
        }
        if is_colour(c) {
            let (kind, _) = colour_parts(c);
            if kind == OUT_PAL {
                let shift = (op.d >> PAL_SHIFT) & 15;
                if op.d & PAL_SCALED == 0 && op.d & !crate::program::code::PAL_MASK != 0
                    || shift > 16
                {
                    return bad;
                }
            }
            continue;
        }
        // The destination: a scalar slot before the pixels (not an input),
        // a register among them.
        let d_ok = if pixel {
            reg(op.d)
        } else {
            scalar(op.d) && op.d >= S_FIRST_FREE
        };
        if !d_ok {
            return bad;
        }
    }
    if depth != 0 || line_open {
        return bad;
    }
    // A program that paints says so: the player gives the paint plane
    // only to one that does (`Ink::try_fit`).
    if !p.paints() && p.ops[fe..de].iter().any(|o| o.code == PAINT) {
        return bad;
    }
    // The colour is the last step; every pixel section ends in one.
    if !is_colour(p.ops[pe - 1].code) {
        return bad;
    }
    // Marker runs: inside the pixel section, values only (the slow layers'
    // their colours too), never another marker; the column run before the
    // still cache's.
    let marker = |c: u8| matches!(c, SLOW | COLUMN | STILL);
    let (mut column_at, mut still_at) = (None, None);
    for k in re..pe {
        let op = &p.ops[k];
        if !marker(op.code) {
            continue;
        }
        let run = &p.ops[k + 1..k + 1 + op.a as usize];
        if run.iter().any(|o| marker(o.code)) {
            return bad;
        }
        let colours = run.iter().filter(|o| is_colour(o.code)).count();
        match op.code {
            SLOW => {
                if colours == 0
                    || colours != op.b as usize
                    || !run.last().is_some_and(|o| is_colour(o.code))
                    || !p.ops[k + 1 + op.a as usize..pe]
                        .iter()
                        .any(|o| is_colour(o.code))
                {
                    return bad;
                }
            }
            COLUMN => {
                if colours != 0 {
                    return bad;
                }
                column_at = Some(k);
            }
            _ => {
                if colours != 0 {
                    return bad;
                }
                still_at = Some(k);
            }
        }
    }
    if let (Some(c), Some(s)) = (column_at, still_at) {
        if s < c + 1 + p.ops[c].a as usize {
            return bad;
        }
    }
    // The colours come after the column and still runs (the JIT's driver
    // runs everything before them as values).
    let runs_end = [column_at, still_at]
        .iter()
        .flatten()
        .map(|&k| k + 1 + p.ops[k].a as usize)
        .max()
        .unwrap_or(0);
    if p.ops[p.fast_start()..runs_end.max(p.fast_start())]
        .iter()
        .any(|o| is_colour(o.code))
    {
        return bad;
    }
    // Nothing but values between the slow layers (or the row steps) and the
    // first colour that is not a slow layer's.
    Ok(())
}

fn colour_ok(c: u8) -> bool {
    use code::*;
    let blends = fixed::BLENDS as u8;
    c >= OUT_RGB && (c < OUT_LAYER || c - OUT_LAYER < 4 * blends)
}

fn spr_ok(p: &Program, at: u8) -> bool {
    let at = at as usize;
    if at >= MAX_TABLE {
        return false;
    }
    let h = crate::program::SprHead::new(&p.tables, at as u8);
    let keys = ((p.tables[at] as u32) >> 20) & 31;
    h.w >= 1 && h.w <= 64 && h.h >= 1 && h.h <= 32 && keys <= 16 && at + h.words() <= MAX_TABLE
}

fn verify_meta(m: &Meta) -> Result<(), ()> {
    use crate::meta::{CHOICE, NONE, SLIDER};
    for k in 0..3 {
        match m.kind[k] {
            NONE => {
                if (m.lo[k], m.hi[k], m.def[k]) != (0, 0, 0) {
                    return Err(());
                }
            }
            SLIDER => {
                if !(m.lo[k] <= m.def[k] && m.def[k] <= m.hi[k]) {
                    return Err(());
                }
            }
            CHOICE => {
                let n = (m.hi[k] >> 16) + 1;
                if m.lo[k] != 0 || m.def[k] != 0 || m.hi[k] & 0xffff != 0 || !(2..=6).contains(&n) {
                    return Err(());
                }
            }
            _ => return Err(()),
        }
    }
    if m.look as usize > crate::meta::BLENDS.len() || m.amount > 100 {
        return Err(());
    }
    Ok(())
}

/// Bytes as base64url without padding (a path's `psb=`).
pub fn to_base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut s = String::with_capacity(b.len() * 4 / 3 + 3);
    for ch in b.chunks(3) {
        let n = ch
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &v)| n | (v as u32) << (16 - 8 * i));
        for i in 0..=ch.len() {
            s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    s
}

/// [`to_base64`]'s inverse (`None` on anything else).
pub fn from_base64(s: &[u8]) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32)
    };
    let mut o = Vec::with_capacity(s.len() * 3 / 4);
    for ch in s.chunks(4) {
        if ch.len() == 1 {
            return None;
        }
        let mut n = 0u32;
        for (i, &c) in ch.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        for i in 0..ch.len() - 1 {
            o.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(o)
}

/// Tests' sources and mutations (here so the JIT's tests share them).
#[cfg(test)]
pub(crate) mod fuzz {
    use alloc::string::String;
    use alloc::vec::Vec;

    /// Every formula the tests know: the goldens, the sketches and the
    /// examples (`opendmd-web/formulas/`).
    pub(crate) fn sources() -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = Vec::new();
        for &(n, s, _, _) in crate::golden::FRAMES {
            v.push((n.into(), s.into()));
        }
        for &(n, s, _, _, _) in crate::golden::OVER.iter().chain(crate::golden::DRAWN) {
            v.push((n.into(), s.into()));
        }
        for (n, s) in crate::sketches::ALL {
            v.push((n.into(), s.into()));
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../opendmd-web/formulas");
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        files.sort();
        for f in files {
            if f.extension().is_some_and(|e| e == "psi") {
                v.push((
                    f.file_stem().unwrap().to_string_lossy().into(),
                    std::fs::read_to_string(&f).unwrap(),
                ));
            }
        }
        v
    }

    pub(crate) struct Rng(pub u64);

    impl Rng {
        pub(crate) fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        pub(crate) fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// `b` with one to three bytes past its name changed: a random byte,
    /// or one up or down (the near misses a verifier must also catch).
    pub(crate) fn mutate(r: &mut Rng, b: &[u8]) -> Vec<u8> {
        let mut m = b.to_vec();
        let start = 6 + b[5] as usize + 2;
        for _ in 0..1 + r.below(3) {
            let k = start + r.below(m.len() - start);
            m[k] = match r.below(3) {
                0 => r.next() as u8,
                1 => m[k].wrapping_add(1),
                _ => m[k].wrapping_sub(1),
            };
        }
        m
    }
}

#[cfg(all(test, feature = "compiler"))]
mod tests {
    use super::fuzz::{mutate, sources, Rng};
    use super::*;
    use crate::{compile, golden, render_rows_still, Ink, Scratch, Still, H, W};

    #[test]
    fn a_built_formula_loads_as_the_compiled_program() {
        for (name, src) in sources() {
            let b = crate::build(&src).unwrap_or_else(|e| panic!("{name}: {}", e.render(&src)));
            let mut p = Program::default();
            load_into(&b, Which::Play, &mut p).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                p == compile(&src).unwrap(),
                "{name}: loaded program differs"
            );
            let mut l = Program::default();
            load_into(&b, Which::Look, &mut l).unwrap();
            match p.meta().look() {
                Some((b, a)) if b != "over" || a < 100 => {
                    let mut c = Program::default();
                    crate::compile_look(&src, &mut c).unwrap();
                    c.finish(true, true);
                    assert!(l == c, "{name}: look differs");
                }
                _ => assert!(l == p),
            }
            let back = from_base64(to_base64(&b).as_bytes()).unwrap();
            assert_eq!(back, b);
        }
    }

    /// The paint plane is a painter's only: a formula that draws lines and
    /// dots gets the ink plane (4 KB), one that paints both (8 KB), and a
    /// canvas changes with the formula. A painter without its plane (the
    /// heap hadn't it) draws its lines and colours, no paint, no panic.
    #[test]
    fn only_a_program_that_paints_gets_the_paint_plane() {
        let lines = compile("draw { line(0, 0, 127, 31, 1) }\ngray(ink)").unwrap();
        let paint =
            compile("draw { circfill(64, 16, 10, #f00); line(0, 0, 127, 31, 0.5) }\ngray(ink)")
                .unwrap();
        assert!(lines.draws() && !lines.paints());
        assert!(paint.paints());
        let mut k = Ink::try_boxed_for(&lines).unwrap();
        assert!(k.1.is_none());
        assert!(k.try_fit(&paint) && k.1.is_some());
        assert!(k.try_fit(&lines) && k.1.is_none());
        let frame = |p: &Program, k: &mut Ink| {
            let mut out = alloc::vec![0u8; W * H * 3];
            crate::render_rows_ink::<128>(
                p,
                &mut Scratch::new(),
                Some(k),
                &golden::inputs(0),
                &crate::tables::OWN,
                None,
                u32::MAX,
                &mut out,
            );
            out
        };
        // The same frame from a fitted canvas as from a host's (both planes).
        for p in [&lines, &paint] {
            let mut fitted = Ink::try_boxed_for(p).unwrap();
            assert!(frame(p, &mut fitted) == frame(p, &mut Ink::boxed()));
        }
        // Without the plane: the ink is drawn, the red disc is not.
        let mut bare = Ink::try_boxed().unwrap();
        let out = frame(&paint, &mut bare);
        let at = |x: usize, y: usize| &out[(y * W + x) * 3..][..3];
        assert_eq!(at(64, 16), &[128, 128, 128][..], "ink only");
        assert!(
            frame(&paint, &mut Ink::boxed())[(16 * W + 64) * 3] > 200,
            "painted red"
        );
    }

    /// A program with paint steps must say it paints (the player gives the
    /// plane by the flag).
    #[test]
    fn paint_steps_without_the_flag_are_refused() {
        let mut b = Program::default();
        load_portable(
            &crate::build("draw { circfill(64, 16, 10, #f00) }").unwrap(),
            Which::Play,
            &mut b,
        )
        .unwrap();
        assert!(verify(&b).is_ok());
        b.uses &= !crate::program::PAINTS;
        assert!(verify(&b).is_err());
    }

    #[test]
    fn the_name_is_read_without_loading() {
        let b = crate::build("name \"Söme\"\ngray(x)").unwrap();
        assert_eq!(name(&b), Ok("Söme"));
        assert_eq!(name(b"PSB\x63\0\0"), Err(OTHER_ABI));
        assert_eq!(name(b"GIF89a"), Err(NOT_PSB));
    }

    fn draw(p: &Program, ms: u32) {
        let mut s = Scratch::<128>::new();
        let mut ink = Ink::boxed();
        let mut st = Still::new();
        st.take(p);
        let mut under = alloc::vec![0u8; W * H * 3];
        golden::picture(&mut under);
        for m in [ms, ms + 20] {
            let mut out = alloc::vec![0u8; W * H * 3];
            render_rows_still::<128>(
                p,
                &mut s,
                Some(&mut ink),
                Some(&mut st),
                &golden::inputs(m),
                &crate::tables::OWN,
                Some(&under),
                u32::MAX,
                &mut out,
            );
        }
        let mut s = Scratch::<64>::new();
        let mut out = alloc::vec![0u8; W * H * 3];
        crate::render_rows_ink::<64>(
            p,
            &mut s,
            Some(&mut ink),
            &golden::inputs(ms),
            &crate::tables::OWN,
            None,
            u32::MAX,
            &mut out,
        );
        let _ = (
            p.esp32_us(),
            p.esp32_us_still(),
            p.esp32_us_jit(),
            p.rate_rows(),
            p.primitives(),
        );
    }

    /// Damaged bytes are refused or play without a panic: the VM, the
    /// still cache, the draw stage and the estimate (the JIT's lowering:
    /// `jit::tests`).
    #[test]
    fn damaged_programs_are_refused_or_play() {
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        let (mut tried, mut taken) = (0, 0);
        for (_, src) in sources() {
            let b = crate::build(&src).unwrap();
            for _ in 0..200 {
                let m = mutate(&mut r, &b);
                let mut p = Program::default();
                tried += 1;
                if load_into(&m, Which::Play, &mut p).is_ok() {
                    taken += 1;
                    draw(&p, (r.next() % 8000) as u32);
                }
            }
        }
        std::println!("{taken} of {tried} damaged programs taken");
    }
}
