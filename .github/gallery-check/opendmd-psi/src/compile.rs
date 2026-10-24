//! Source text to [`Program`]: a lexer, a Pratt parser that builds an
//! expression DAG as it goes (constant folding, common subexpressions,
//! division by constants turned into multiplication), a type pass that is
//! really a *level*: constant, per frame, per row, per pixel, so work runs
//! as rarely as its inputs change, and a linear-scan register allocator for
//! the per-pixel lanes.
//!
//! The compiler is the only part of Psi that allocates (its token and node
//! lists, a few KB, freed when it returns); the program it fills is a
//! fixed-size value. Source is capped at [`MAX_SOURCE`] bytes, nodes at
//! [`MAX_NODES`] and nesting at [`MAX_DEPTH`], so compile time and stack
//! are bounded for any input.

use crate::fixed::{self, ONE};
use crate::program::{code, *};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The longest source accepted (bytes).
pub const MAX_SOURCE: usize = 4096;
/// Expression nodes (before dead ones are dropped): bounds compile time.
pub const MAX_NODES: usize = 1024;
/// Brackets and operators nested this deep: bounds the parser's stack.
pub const MAX_DEPTH: usize = 24;

/// A compile error: a byte range of the source and a sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub start: usize,
    pub end: usize,
    pub msg: String,
}

impl Error {
    fn new(start: usize, end: usize, msg: impl Into<String>) -> Error {
        Error {
            start,
            end,
            msg: msg.into(),
        }
    }

    /// 1-based line and column of the start.
    pub fn line_col(&self, src: &str) -> (usize, usize) {
        let upto = &src[..self.start.min(src.len())];
        let line = upto.matches('\n').count() + 1;
        let col = upto.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        (line, col)
    }

    /// "line 2: msg", the line, and carets under the range.
    pub fn render(&self, src: &str) -> String {
        let (line, col) = self.line_col(src);
        let text = src.lines().nth(line - 1).unwrap_or("");
        let width = src[self.start.min(src.len())..self.end.min(src.len())]
            .chars()
            .take_while(|&c| c != '\n')
            .count()
            .max(1);
        format!(
            "line {line}: {}\n  {text}\n  {}{}",
            self.msg,
            " ".repeat(col - 1),
            "^".repeat(width)
        )
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.msg)
    }
}

// --- the lexer ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Num(i32),
    Name,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    Ne,
    And,
    Or,
    Not,
    Question,
    Colon,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Assign,
    Newline,
    Eof,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    tok: Tok,
    start: usize,
    end: usize,
}

fn describe(t: Tok) -> &'static str {
    match t {
        Tok::Num(_) => "a number",
        Tok::Name => "a name",
        Tok::Newline => "the end of the line",
        Tok::Eof => "the end",
        Tok::RParen => "`)`",
        Tok::LParen => "`(`",
        Tok::RBrace => "`}`",
        Tok::LBrace => "`{`",
        Tok::Comma => "`,`",
        Tok::Assign => "`=`",
        Tok::Colon => "`:`",
        Tok::Question => "`?`",
        _ => "an operator",
    }
}

/// A decimal number (digits, an optional `.` and more digits; no sign) as
/// Q16.16, rounded to the nearest 1/65,536: how the compiler reads numbers,
/// for the sliders' `a=` options too.
pub fn number(s: &str) -> Option<i32> {
    if s.is_empty()
        || s == "."
        || s.matches('.').count() > 1
        || !s.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return None;
    }
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    let mut v: i64 = 0;
    for b in int.bytes() {
        v = v * 10 + (b - b'0') as i64;
        if v > 32767 {
            return None;
        }
    }
    // The fraction to 9 digits, rounded once.
    let mut num: i64 = 0;
    let mut den: i64 = 1;
    for b in frac.bytes().take(9) {
        num = num * 10 + (b - b'0') as i64;
        den *= 10;
    }
    let f = (num * 65536 + den / 2) / den;
    let r = (v << 16) + f;
    (r <= i32::MAX as i64).then_some(r as i32)
}

fn lex(src: &str) -> Result<Vec<Token>, Error> {
    let b = src.as_bytes();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0;
    let mut depth = 0usize; // ( ) nesting: newlines inside are spaces
    let push_nl = |out: &mut Vec<Token>, at: usize| {
        if !matches!(
            out.last(),
            None | Some(Token {
                tok: Tok::Newline,
                ..
            })
        ) {
            out.push(Token {
                tok: Tok::Newline,
                start: at,
                end: at,
            });
        }
    };
    while i < b.len() {
        let c = b[i];
        let start = i;
        // Comments: `--` or `//` to the end of the line.
        if (c == b'-' && b.get(i + 1) == Some(&b'-')) || (c == b'/' && b.get(i + 1) == Some(&b'/'))
        {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'\n' || c == b';' {
            if depth == 0 {
                push_nl(&mut out, i);
            }
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            let s = &src[start..i];
            if s.matches('.').count() > 1 {
                return Err(Error::new(start, i, format!("`{s}` is not a number")));
            }
            if i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b'_') {
                let mut j = i;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
                return Err(Error::new(
                    start,
                    j,
                    format!(
                        "`{}` is not a number or a name (to multiply, write `{} * {}`)",
                        &src[start..j],
                        s,
                        &src[i..j]
                    ),
                ));
            }
            let v = number(s)
                .ok_or_else(|| Error::new(start, i, "numbers must be between -32768 and 32767"))?;
            out.push(Token {
                tok: Tok::Num(v),
                start,
                end: i,
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let tok = match &src[start..i] {
                "and" => Tok::And,
                "or" => Tok::Or,
                "not" => Tok::Not,
                _ => Tok::Name,
            };
            out.push(Token { tok, start, end: i });
            continue;
        }
        let two = b.get(i + 1).copied();
        let (tok, n) = match (c, two) {
            (b'<', Some(b'=')) => (Tok::Le, 2),
            (b'>', Some(b'=')) => (Tok::Ge, 2),
            (b'=', Some(b'=')) => (Tok::EqEq, 2),
            (b'!', Some(b'=')) => (Tok::Ne, 2),
            (b'&', Some(b'&')) => (Tok::And, 2),
            (b'|', Some(b'|')) => (Tok::Or, 2),
            (b'+', _) => (Tok::Plus, 1),
            (b'-', _) => (Tok::Minus, 1),
            (b'*', _) => (Tok::Star, 1),
            (b'/', _) => (Tok::Slash, 1),
            (b'%', _) => (Tok::Percent, 1),
            (b'<', _) => (Tok::Lt, 1),
            (b'>', _) => (Tok::Gt, 1),
            (b'!', _) => (Tok::Not, 1),
            (b'?', _) => (Tok::Question, 1),
            (b':', _) => (Tok::Colon, 1),
            (b'(', _) => (Tok::LParen, 1),
            (b')', _) => (Tok::RParen, 1),
            (b'{', _) => (Tok::LBrace, 1),
            (b'}', _) => (Tok::RBrace, 1),
            (b',', _) => (Tok::Comma, 1),
            (b'=', _) => (Tok::Assign, 1),
            _ => {
                let ch = src[i..].chars().next().unwrap_or('?');
                let end = i + ch.len_utf8();
                let hint = match ch {
                    '^' => " (there is no power operator: multiply, e.g. `x * x`)",
                    '&' | '|' => " (write `and` / `or`)",
                    '"' | '\'' => " (there are no strings)",
                    '[' | ']' => " (use `(` and `)`)",
                    _ => "",
                };
                return Err(Error::new(
                    i,
                    end,
                    format!("I don't understand `{ch}`{hint}"),
                ));
            }
        };
        match tok {
            Tok::LParen => depth += 1,
            Tok::RParen => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += n;
        out.push(Token { tok, start, end: i });
    }
    push_nl(&mut out, b.len());
    out.push(Token {
        tok: Tok::Eof,
        start: b.len(),
        end: b.len(),
    });
    Ok(out)
}

/// The same formula on one line, as short as it stays readable: comments
/// dropped, runs of spaces made one, lines joined with `; ` (which the
/// language reads as a line break, and ignores inside brackets, as it does
/// a newline). For `@fx:psi?src=` paths, which are 512 B at most.
pub fn compact(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        // Cut a comment (`--` or `//`) off the line.
        let b = line.as_bytes();
        let mut end = b.len();
        for i in 0..b.len().saturating_sub(1) {
            if (b[i] == b'-' && b[i + 1] == b'-') || (b[i] == b'/' && b[i + 1] == b'/') {
                end = i;
                break;
            }
        }
        let mut code = String::new();
        for w in line[..end].split_whitespace() {
            if !code.is_empty() {
                code.push(' ');
            }
            code.push_str(w);
        }
        if code.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("; ");
        }
        out.push_str(&code);
    }
    out
}

// --- names ----------------------------------------------------------------------

/// The built-in values, their level and what they mean (for errors).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Input {
    X,
    Y,
    Px,
    Py,
    I,
    T,
    Hour,
    Minute,
    Second,
    A,
    B,
    C,
    Ur,
    Ug,
    Ub,
    Ul,
}

const INPUTS: [(&str, Input, &str); 16] = [
    ("x", Input::X, "the pixel's position across, 0 to 1"),
    (
        "y",
        Input::Y,
        "the pixel's height, 0 at the bottom to 1 at the top",
    ),
    ("px", Input::Px, "the pixel's column, 0 to 127"),
    ("py", Input::Py, "the pixel's row from the bottom, 0 to 31"),
    ("i", Input::I, "the pixel's number, 0 to 4095"),
    ("t", Input::T, "seconds into the loop"),
    ("hour", Input::Hour, "the hour, 0 to 23"),
    ("minute", Input::Minute, "the minute"),
    ("second", Input::Second, "the second"),
    ("a", Input::A, "slider a"),
    ("b", Input::B, "slider b"),
    ("c", Input::C, "slider c"),
    (
        "ur",
        Input::Ur,
        "the red of the picture under the formula, 0 to 1",
    ),
    (
        "ug",
        Input::Ug,
        "the green of the picture under the formula, 0 to 1",
    ),
    (
        "ub",
        Input::Ub,
        "the blue of the picture under the formula, 0 to 1",
    ),
    (
        "ul",
        Input::Ul,
        "the brightness of the picture under the formula, 0 to 1",
    ),
];

/// `w` and `h`: constants.
const CONSTS: [(&str, i32); 2] = [("w", (W as i32) << 16), ("h", (H as i32) << 16)];

#[derive(Clone, Copy, PartialEq, Eq)]
enum F {
    Op(u8),
    Smoothstep,
    Fract,
}

/// Functions: name, arity, what, the op.
const FUNCS: [(&str, usize, F); 21] = [
    ("sin", 1, F::Op(code::SIN)),
    ("cos", 1, F::Op(code::COS)),
    ("tri", 1, F::Op(code::TRI)),
    ("saw", 1, F::Op(code::SAW)),
    ("sqr", 1, F::Op(code::SQR)),
    ("abs", 1, F::Op(code::ABS)),
    ("floor", 1, F::Op(code::FLOOR)),
    ("frac", 1, F::Op(code::FRAC)),
    ("fract", 1, F::Fract),
    ("sqrt", 1, F::Op(code::SQRT)),
    ("min", 2, F::Op(code::MIN)),
    ("max", 2, F::Op(code::MAX)),
    ("step", 2, F::Op(code::STEP)),
    ("hash", 2, F::Op(code::HASH)),
    ("len", 2, F::Op(code::LEN)),
    ("atan2", 2, F::Op(code::ATAN2)),
    ("clamp", 3, F::Op(code::CLAMP)),
    ("mix", 3, F::Op(code::MIX)),
    ("smoothstep", 3, F::Smoothstep),
    ("noise", 3, F::Op(code::NOISE)),
    ("select", 3, F::Op(code::SELECT)),
];

const COLOURS: [(&str, usize, u8); 4] = [
    ("rgb", 3, code::OUT_RGB),
    ("hsv", 3, code::OUT_HSV),
    ("pal", 2, code::OUT_PAL),
    ("gray", 1, code::OUT_GRAY),
];

/// Palette names: core's order (fire, ice, chrono, amber, rainbow); `gray`
/// is not a table.
const PALETTES: [(&str, u8); 7] = [
    ("fire", 0),
    ("ice", 1),
    ("ocean", 1),
    ("chrono", 2),
    ("amber", 3),
    ("rainbow", 4),
    ("gray", 255),
];

const KEYWORDS: [&str; 3] = ["loop", "frame", "layer"];

/// How a layer goes onto what is below it (`layer add { … }`; `over` when
/// none is named).
const BLENDS: [(&str, u8); fixed::BLENDS] = [
    ("over", fixed::BLEND_OVER),
    ("add", fixed::BLEND_ADD),
    ("multiply", fixed::BLEND_MULTIPLY),
    ("screen", fixed::BLEND_SCREEN),
    ("max", fixed::BLEND_MAX),
    ("min", fixed::BLEND_MIN),
];

/// Optimal string alignment distance (edits, adjacent swaps count one).
fn distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() > 24 || b.len() > 24 {
        return usize::MAX;
    }
    let mut d = [[0usize; 25]; 25];
    for (i, row) in d.iter_mut().enumerate().take(a.len() + 1) {
        row[0] = i;
    }
    for (j, v) in d[0].iter_mut().enumerate().take(b.len() + 1) {
        *v = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[a.len()][b.len()]
}

/// The closest of `cands` to `name`, if it is close enough to be a typo.
fn suggest<'a>(name: &str, cands: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let limit = if name.len() <= 3 {
        2
    } else {
        2 + name.len() / 6
    };
    let mut best: Option<(usize, &str)> = None;
    for c in cands {
        let d = distance(name, c);
        if d <= limit && d < c.len().max(name.len()) && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, c));
        }
    }
    best.map(|(_, c)| c)
}

fn did_you_mean(s: Option<&str>) -> String {
    s.map_or(String::new(), |s| format!(" — did you mean `{s}`?"))
}

// --- the expression DAG ---------------------------------------------------------

type Id = u16;

const LV_CONST: u8 = 0;
const LV_FRAME: u8 = 1;
const LV_ROW: u8 = 2;
const LV_PIXEL: u8 = 3;

const K_CONST: u8 = 200;
const K_INPUT: u8 = 201;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Node {
    /// An op code, `K_CONST` (value in `val`) or `K_INPUT` (slot or lane in
    /// `val`).
    code: u8,
    a: Id,
    b: Id,
    c: Id,
    val: i32,
    level: u8,
    /// The input that made this node vary as much as it does (errors).
    why: u8,
}

struct Dag {
    nodes: Vec<Node>,
    /// Open-addressing index of `nodes` for sharing ([`Dag::push`]):
    /// [`SLOTS`] ids, `NO` where empty. A linear scan made compiling a long
    /// formula quadratic (~0.2-0.35 ms on the host, tens of ms on the
    /// ESP32, per live edit).
    index: Vec<Id>,
}

/// Index slots: a power of two, twice [`MAX_NODES`] (4 KB, freed with the
/// rest when `compile_into` returns).
const SLOTS: usize = 2 * MAX_NODES;
const NO: Id = Id::MAX;

impl Node {
    fn hash(&self) -> usize {
        let mut h: u32 = 0x811c_9dc5;
        for v in [
            self.code as u32,
            self.a as u32,
            self.b as u32,
            self.c as u32,
            self.val as u32,
            (self.level as u32) << 8 | self.why as u32,
        ] {
            h = (h ^ v).wrapping_mul(0x0100_0193);
        }
        (h ^ (h >> 15)) as usize
    }
}

impl Dag {
    fn level(&self, id: Id) -> u8 {
        self.nodes[id as usize].level
    }

    fn konst(&self, id: Id) -> Option<i32> {
        let n = &self.nodes[id as usize];
        (n.code == K_CONST).then_some(n.val)
    }

    fn push(&mut self, n: Node, at: (usize, usize)) -> Result<Id, Error> {
        let mut slot = n.hash() & (SLOTS - 1);
        loop {
            match self.index[slot] {
                NO => break,
                i if self.nodes[i as usize] == n => return Ok(i),
                _ => slot = (slot + 1) & (SLOTS - 1),
            }
        }
        if self.nodes.len() >= MAX_NODES {
            return Err(Error::new(
                at.0,
                at.1,
                "this formula is too long for the panel — simplify it",
            ));
        }
        self.nodes.push(n);
        let id = (self.nodes.len() - 1) as Id;
        self.index[slot] = id;
        Ok(id)
    }

    fn constant(&mut self, v: i32, at: (usize, usize)) -> Result<Id, Error> {
        self.push(
            Node {
                code: K_CONST,
                a: 0,
                b: 0,
                c: 0,
                val: v,
                level: LV_CONST,
                why: 0,
            },
            at,
        )
    }

    fn input(&mut self, inp: Input, at: (usize, usize)) -> Result<Id, Error> {
        let why = INPUTS.iter().position(|e| e.1 == inp).unwrap_or(0) as u8;
        // What is under the formula: loaded into a lane by a step of its own.
        let under = match inp {
            Input::Ur => Some(code::U_R),
            Input::Ug => Some(code::U_G),
            Input::Ub => Some(code::U_B),
            Input::Ul => Some(code::U_L),
            _ => None,
        };
        if let Some(c) = under {
            return self.push(
                Node {
                    code: c,
                    a: 0,
                    b: 0,
                    c: 0,
                    val: 0,
                    level: LV_PIXEL,
                    why,
                },
                at,
            );
        }
        let (val, level) = match inp {
            Input::X => ((LANE | L_X) as i32, LV_PIXEL),
            Input::Px => ((LANE | L_PX) as i32, LV_PIXEL),
            Input::I => ((LANE | L_I) as i32, LV_PIXEL),
            Input::Y => (S_Y as i32, LV_ROW),
            Input::Py => (S_PY as i32, LV_ROW),
            Input::T => (S_T as i32, LV_FRAME),
            Input::Hour => (S_HOUR as i32, LV_FRAME),
            Input::Minute => (S_MINUTE as i32, LV_FRAME),
            Input::Second => (S_SECOND as i32, LV_FRAME),
            Input::A => (S_A as i32, LV_FRAME),
            Input::B => (S_B as i32, LV_FRAME),
            Input::C => (S_C as i32, LV_FRAME),
            Input::Ur | Input::Ug | Input::Ub | Input::Ul => (0, LV_PIXEL),
        };
        self.push(
            Node {
                code: K_INPUT,
                a: 0,
                b: 0,
                c: 0,
                val,
                level,
                why,
            },
            at,
        )
    }

    /// An op node: folded when its operands are constants, simplified
    /// where that is exact, shared when it already exists.
    fn op(&mut self, c: u8, args: [Id; 3], at: (usize, usize)) -> Result<Id, Error> {
        let n = code::arity(c);
        let ks: [Option<i32>; 3] =
            core::array::from_fn(|i| if i < n { self.konst(args[i]) } else { Some(0) });
        // Fold (all but the table functions, which run once a frame).
        if ks.iter().all(Option::is_some) && c != code::SIN && c != code::COS {
            let v = eval(
                c,
                ks[0].unwrap_or(0),
                ks[1].unwrap_or(0),
                ks[2].unwrap_or(0),
                |_| 0,
            );
            return self.constant(v, at);
        }
        let (a, b) = (args[0], args[1]);
        match (c, ks[0], ks[1]) {
            (code::ADD, _, Some(0)) | (code::SUB, _, Some(0)) | (code::MUL, _, Some(ONE)) => {
                return Ok(a)
            }
            (code::ADD, Some(0), _) | (code::MUL, Some(ONE), _) => return Ok(b),
            (code::MUL, Some(0), _) | (code::MUL, _, Some(0)) => return self.constant(0, at),
            // x / k: x · (1/k), exact for powers of two.
            (code::DIV, None, Some(k)) if k != 0 => {
                let r = self.constant(fixed::recip(k), at)?;
                return self.op(code::MUL, [a, r, 0], at);
            }
            // x / s, s per frame or per row: one division there, a multiply
            // per pixel.
            (code::DIV, None, None) if self.level(a) == LV_PIXEL && self.level(b) < LV_PIXEL => {
                let r = self.op(code::RECIP, [b, 0, 0], at)?;
                return self.op(code::MUL, [a, r, 0], at);
            }
            // x % 2^k: a mask.
            (code::MOD, None, Some(k)) if k > 0 && (k as u32).is_power_of_two() => {
                let m = self.constant(k - 1, at)?;
                return self.op(code::MASK, [a, m, 0], at);
            }
            _ => {}
        }
        let mut level = LV_CONST;
        let mut why = 0;
        for &x in &args[..n] {
            let m = &self.nodes[x as usize];
            if m.level > level {
                level = m.level;
                why = m.why;
            }
        }
        if level == LV_CONST {
            // sin/cos of a constant: once a frame.
            level = LV_FRAME;
        }
        let mut args = args;
        for x in &mut args[n..] {
            *x = 0;
        }
        self.push(
            Node {
                code: c,
                a: args[0],
                b: args[1],
                c: args[2],
                val: 0,
                level,
                why,
            },
            at,
        )
    }
}

// --- the parser -------------------------------------------------------------------

struct Local<'s> {
    name: &'s str,
    id: Id,
    /// Where its name is (the line is worked out only for an error: per
    /// local it made a long formula quadratic).
    at: usize,
    in_frame: bool,
    /// 0: the whole formula's; k: the `k`th layer's own.
    scope: u16,
}

struct Parser<'s> {
    src: &'s str,
    toks: Vec<Token>,
    pos: usize,
    dag: Dag,
    locals: Vec<Local<'s>>,
    depth: usize,
    /// The layer being read (0: outside the layers) and the last one's.
    scope: u16,
    layers: u16,
}

/// A colour: an `OUT_*` code, its arguments, the palette, how much of the
/// pixel it covers (`None`: all of it) and how it goes onto what is below.
struct Colour {
    code: u8,
    args: [Id; 3],
    pal: u8,
    alpha: Option<Id>,
    blend: u8,
}

const BP_TERNARY: u8 = 1;
const BP_UNARY: u8 = 11;
const BP_NOT: u8 = 4;

fn infix_bp(t: Tok) -> Option<(u8, u8, u8)> {
    // (left bp, right bp, op)
    Some(match t {
        Tok::Or => (2, 3, code::OR),
        Tok::And => (3, 4, code::AND),
        Tok::Lt => (5, 6, code::LT),
        Tok::Le => (5, 6, code::LE),
        Tok::Gt => (5, 6, code::GT),
        Tok::Ge => (5, 6, code::GE),
        Tok::EqEq => (5, 6, code::EQ),
        Tok::Ne => (5, 6, code::NE),
        Tok::Plus => (7, 8, code::ADD),
        Tok::Minus => (7, 8, code::SUB),
        Tok::Star => (9, 10, code::MUL),
        Tok::Slash => (9, 10, code::DIV),
        Tok::Percent => (9, 10, code::MOD),
        _ => return None,
    })
}

impl<'s> Parser<'s> {
    fn peek(&self) -> Token {
        self.toks[self.pos]
    }

    fn next(&mut self) -> Token {
        let t = self.toks[self.pos];
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn text(&self, t: Token) -> &'s str {
        &self.src[t.start..t.end]
    }

    fn line_of(&self, at: usize) -> usize {
        self.src[..at].matches('\n').count() + 1
    }

    fn err<T>(&self, t: Token, msg: impl Into<String>) -> Result<T, Error> {
        Err(Error::new(t.start, t.end, msg))
    }

    fn expect(&mut self, want: Tok, what: &str) -> Result<Token, Error> {
        let t = self.peek();
        if t.tok == want {
            return Ok(self.next());
        }
        self.err(t, format!("expected {what} here, found {}", self.found(t)))
    }

    fn found(&self, t: Token) -> String {
        match t.tok {
            Tok::Name | Tok::Num(_) => format!("`{}`", self.text(t)),
            Tok::Newline | Tok::Eof => describe(t.tok).to_string(),
            _ => format!("`{}`", self.text(t)),
        }
    }

    /// A `(` whose `)` was expected at `found` on a later line: the
    /// newline was swallowed, so say which bracket is open.
    fn unclosed(&self, open: Token, found: Token) -> Result<(), Error> {
        if found.tok == Tok::Eof || self.line_of(found.start) > self.line_of(open.start) {
            return self.err(open, "this `(` is never closed — add `)`");
        }
        Ok(())
    }

    fn skip_newlines(&mut self) {
        while self.peek().tok == Tok::Newline {
            self.next();
        }
    }

    /// An expression (Pratt): operators binding tighter than `min_bp`.
    fn expr(&mut self, min_bp: u8) -> Result<Id, Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            let t = self.peek();
            return self.err(t, "this is nested too deeply — split it into named steps");
        }
        let r = self.expr_inner(min_bp);
        self.depth -= 1;
        r
    }

    fn expr_inner(&mut self, min_bp: u8) -> Result<Id, Error> {
        // After an operator, the expression may continue on the next line.
        self.skip_newlines();
        let t = self.next();
        let mut lhs = match t.tok {
            Tok::Num(v) => self.dag.constant(v, (t.start, t.end))?,
            Tok::Minus => {
                let a = self.expr(BP_UNARY)?;
                self.dag.op(code::NEG, [a, 0, 0], (t.start, t.end))?
            }
            Tok::Plus => self.expr(BP_UNARY)?,
            Tok::Not => {
                let a = self.expr(BP_NOT)?;
                self.dag.op(code::NOT, [a, 0, 0], (t.start, t.end))?
            }
            Tok::LParen => {
                let e = self.expr(0)?;
                let close = self.peek();
                if close.tok != Tok::RParen {
                    self.unclosed(t, close)?;
                }
                self.expect(Tok::RParen, "`)`")?;
                e
            }
            Tok::Name => self.name(t)?,
            Tok::Newline | Tok::Eof => {
                return self.err(t, "a value is missing here");
            }
            _ => return self.err(t, format!("expected a value, found {}", self.found(t))),
        };
        loop {
            let op = self.peek();
            if op.tok == Tok::Question {
                if BP_TERNARY < min_bp {
                    break;
                }
                self.next();
                let a = self.expr(0)?;
                self.skip_newlines();
                self.expect(Tok::Colon, "`:` (as in `cond ? this : that`)")?;
                let b = self.expr(BP_TERNARY)?;
                lhs = self.dag.op(code::SELECT, [lhs, a, b], (op.start, op.end))?;
                continue;
            }
            let Some((lbp, rbp, c)) = infix_bp(op.tok) else {
                break;
            };
            if lbp < min_bp {
                break;
            }
            self.next();
            let rhs = self.expr(rbp)?;
            lhs = self.dag.op(c, [lhs, rhs, 0], (op.start, op.end))?;
        }
        Ok(lhs)
    }

    /// Names set in another layer are that layer's own.
    fn visible(&self, l: &Local) -> bool {
        l.scope == 0 || l.scope == self.scope
    }

    /// The candidates for a misspelt value name.
    fn value_names(&self) -> impl Iterator<Item = &str> + '_ {
        INPUTS
            .iter()
            .map(|e| e.0)
            .chain(CONSTS.iter().map(|e| e.0))
            .chain(
                self.locals
                    .iter()
                    .filter(|l| self.visible(l))
                    .map(|l| l.name),
            )
    }

    fn name(&mut self, t: Token) -> Result<Id, Error> {
        let name = self.text(t);
        let at = (t.start, t.end);
        let call = self.peek().tok == Tok::LParen;
        if call {
            if let Some(&(_, ar, f)) = FUNCS.iter().find(|e| e.0 == name) {
                let args = self.args(t, name, ar)?;
                return match f {
                    F::Op(c) => self.dag.op(c, args, at),
                    F::Fract => self.dag.op(code::FRAC, args, at),
                    F::Smoothstep => {
                        // smoothstep(e0, e1, x) = smooth(x, e0, 1/(e1-e0))
                        let span = self.dag.op(code::SUB, [args[1], args[0], 0], at)?;
                        let inv = self.dag.op(code::RECIP, [span, 0, 0], at)?;
                        self.dag.op(code::SMOOTH, [args[2], args[0], inv], at)
                    }
                };
            }
            if COLOURS.iter().any(|e| e.0 == name) {
                return self.err(
                    t,
                    format!(
                        "`{name}(…)` makes the colour, so it can only be the whole last line \
                         (of the formula, or of a layer)"
                    ),
                );
            }
            if self.lookup_value(name).is_some() {
                return self.err(t, format!("`{name}` is a value, not a function"));
            }
            let s = suggest(
                name,
                FUNCS.iter().map(|e| e.0).chain(COLOURS.iter().map(|e| e.0)),
            );
            return self.err(t, format!("unknown function `{name}`{}", did_you_mean(s)));
        }
        if let Some(id) = self.lookup_value(name) {
            return id;
        }
        if FUNCS.iter().any(|e| e.0 == name) || COLOURS.iter().any(|e| e.0 == name) {
            return self.err(t, format!("`{name}` is a function: write `{name}(…)`"));
        }
        if KEYWORDS.contains(&name) {
            return self.err(t, format!("`{name}` can only start a line"));
        }
        if PALETTES.iter().any(|e| e.0 == name) {
            return self.err(
                t,
                format!("`{name}` is a palette: use it as `pal({name}, k)` on the last line"),
            );
        }
        let s = suggest(name, self.value_names());
        self.err(t, format!("unknown name `{name}`{}", did_you_mean(s)))
    }

    fn lookup_value(&mut self, name: &str) -> Option<Result<Id, Error>> {
        if let Some(l) = self
            .locals
            .iter()
            .find(|l| l.name == name && self.visible(l))
        {
            return Some(Ok(l.id));
        }
        if let Some(&(_, inp, _)) = INPUTS.iter().find(|e| e.0 == name) {
            return Some(self.dag.input(inp, (0, 0)));
        }
        if let Some(&(_, v)) = CONSTS.iter().find(|e| e.0 == name) {
            return Some(self.dag.constant(v, (0, 0)));
        }
        None
    }

    /// `( a, b, … )` with exactly `n` values.
    fn args(&mut self, f: Token, name: &str, n: usize) -> Result<[Id; 3], Error> {
        let (a, _) = self.args_or_cover(f, name, n, false)?;
        Ok([a[0], a[1], a[2]])
    }

    /// `( a, b, … )` with `n` values, or with `cover` also `n + 1` (a
    /// colour's last: how much of the pixel it covers); the values and how
    /// many.
    fn args_or_cover(
        &mut self,
        f: Token,
        name: &str,
        n: usize,
        cover: bool,
    ) -> Result<([Id; 4], usize), Error> {
        let open = self.expect(Tok::LParen, "`(`")?;
        let mut out = [0 as Id; 4];
        let mut k = 0;
        if self.peek().tok != Tok::RParen {
            loop {
                let e = self.expr(0)?;
                if k < 4 {
                    out[k] = e;
                }
                k += 1;
                if self.peek().tok == Tok::Comma {
                    self.next();
                    continue;
                }
                break;
            }
        }
        let close = self.peek();
        if close.tok != Tok::RParen {
            self.unclosed(open, close)?;
            return self.err(
                close,
                format!(
                    "expected `,` or `)` in `{name}(…)`, found {}",
                    self.found(close)
                ),
            );
        }
        self.next();
        if k != n && !(cover && k == n + 1) {
            let span = Error::new(f.start, close.end, "");
            let s = if n == 1 { "" } else { "s" };
            return Err(Error {
                msg: if cover {
                    format!(
                        "`{name}` takes {n} value{s} (or {}: the last is how much of the pixel it covers, 0 to 1), but got {k}",
                        n + 1
                    )
                } else {
                    format!("`{name}` takes {n} value{s}, but got {k}")
                },
                ..span
            });
        }
        Ok((out, k))
    }

    /// The colour call at `t` (its name): `rgb(r, g, b)`, `hsv(h, s, v)`,
    /// `pal(name, k)`, `gray(v)`.
    fn colour(&mut self, t: Token) -> Result<Colour, Error> {
        let name = self.text(t);
        let &(_, ar, c) = COLOURS.iter().find(|e| e.0 == name).unwrap_or(&COLOURS[0]);
        if c != code::OUT_PAL {
            let (a, k) = self.args_or_cover(t, name, ar, true)?;
            return Ok(Colour {
                code: c,
                args: [a[0], a[1], a[2]],
                pal: 0,
                alpha: (k > ar).then_some(a[ar]),
                blend: fixed::BLEND_OVER,
            });
        }
        self.expect(Tok::LParen, "`(`")?;
        let p = self.peek();
        if p.tok != Tok::Name {
            return self.err(
                p,
                "the first value of `pal` is a palette: fire, ocean, ice, chrono, amber, rainbow or gray",
            );
        }
        self.next();
        let pname = self.text(p);
        let Some(&(_, pal)) = PALETTES.iter().find(|e| e.0 == pname) else {
            let s = suggest(pname, PALETTES.iter().map(|e| e.0));
            return self.err(
                p,
                format!(
                    "unknown palette `{pname}`{} (there are fire, ocean, ice, chrono, amber, rainbow and gray)",
                    did_you_mean(s)
                ),
            );
        };
        self.expect(Tok::Comma, "`,` (as in `pal(fire, k)`)")?;
        let k = self.expr(0)?;
        let mut alpha = None;
        if self.peek().tok == Tok::Comma {
            self.next();
            alpha = Some(self.expr(0)?);
        }
        let close = self.peek();
        if close.tok != Tok::RParen {
            return self.err(
                close,
                format!(
                    "`pal` takes a palette and one value (and how much of the pixel it covers, 0 to 1, if you like); found {}",
                    self.found(close)
                ),
            );
        }
        self.next();
        let (code, pal) = if pal == 255 {
            (code::OUT_GRAY, 0)
        } else {
            (code::OUT_PAL, pal)
        };
        Ok(Colour {
            code,
            args: [k, 0, 0],
            pal,
            alpha,
            blend: fixed::BLEND_OVER,
        })
    }

    fn end_of_statement(&mut self) -> Result<(), Error> {
        let t = self.peek();
        match t.tok {
            Tok::Newline | Tok::Eof | Tok::RBrace => {
                if t.tok == Tok::Newline {
                    self.next();
                }
                Ok(())
            }
            Tok::Name | Tok::Num(_) | Tok::LParen => self.err(
                t,
                format!(
                    "expected the end of the line, found {} (missing an operator?)",
                    self.found(t)
                ),
            ),
            _ => self.err(
                t,
                format!("expected the end of the line, found {}", self.found(t)),
            ),
        }
    }

    /// `name = expr`.
    fn assignment(&mut self, in_frame: bool) -> Result<(), Error> {
        let t = self.next();
        let name = self.text(t);
        if let Some(e) = INPUTS.iter().find(|e| e.0 == name) {
            return self.err(
                t,
                format!("`{name}` is built in ({}); pick another name", e.2),
            );
        }
        if CONSTS.iter().any(|e| e.0 == name)
            || FUNCS.iter().any(|e| e.0 == name)
            || COLOURS.iter().any(|e| e.0 == name)
            || KEYWORDS.contains(&name)
        {
            return self.err(t, format!("`{name}` is built in; pick another name"));
        }
        if PALETTES.iter().any(|e| e.0 == name) {
            return self.err(
                t,
                format!("`{name}` is a palette's name; pick another name"),
            );
        }
        if let Some(l) = self
            .locals
            .iter()
            .find(|l| l.name == name && self.visible(l))
        {
            return self.err(
                t,
                format!(
                    "`{name}` is already set on line {} — each name can be set once",
                    self.line_of(l.at)
                ),
            );
        }
        let eq = self.expect(Tok::Assign, "`=`")?;
        if matches!(self.peek().tok, Tok::Newline | Tok::Eof) {
            return self.err(eq, format!("a value is missing after `=` (`{name} = …`)"));
        }
        let start = self.peek().start;
        let id = self.expr(0)?;
        let end = self.toks[self.pos.saturating_sub(1)].end;
        if in_frame && self.dag.level(id) > LV_FRAME {
            let why = INPUTS[self.dag.nodes[id as usize].why as usize];
            return Err(Error::new(
                start,
                end.max(start),
                format!(
                    "inside frame {{ }} only values that are the same for every pixel can be used, \
                     but this uses `{}` ({})",
                    why.0, why.2
                ),
            ));
        }
        self.locals.push(Local {
            name,
            id,
            at: t.start,
            in_frame,
            scope: self.scope,
        });
        self.end_of_statement()
    }

    fn is_assignment(&self) -> bool {
        self.peek().tok == Tok::Name && self.toks[self.pos + 1].tok == Tok::Assign
    }

    /// The word starting the statement at the cursor, if it is a keyword
    /// used as one (`loop = 3` is an assignment, and refused as such).
    fn keyword(&self) -> &'s str {
        let t = self.peek();
        if t.tok != Tok::Name || self.toks[self.pos + 1].tok == Tok::Assign {
            return "";
        }
        let w = self.text(t);
        if KEYWORDS.contains(&w) {
            w
        } else {
            ""
        }
    }

    /// `frame { name = value … }` at `t` (one per formula, and one per
    /// layer).
    fn frame_block(&mut self, t: Token, seen: &mut bool) -> Result<(), Error> {
        self.next();
        if *seen {
            return self.err(
                t,
                if self.scope == 0 {
                    "there can be one `frame { }` block"
                } else {
                    "a layer can have one `frame { }` block"
                },
            );
        }
        *seen = true;
        self.expect(Tok::LBrace, "`{` (as in `frame { … }`)")?;
        self.skip_newlines();
        while self.peek().tok != Tok::RBrace {
            if self.peek().tok == Tok::Eof {
                let t = self.peek();
                return self.err(t, "`frame {` is never closed: add `}`");
            }
            if !self.is_assignment() {
                let t = self.peek();
                return self.err(t, "only `name = value` lines can go inside frame { }");
            }
            self.assignment(true)?;
            self.skip_newlines();
        }
        self.next();
        self.end_of_statement()
    }

    /// A colour line at `t` (its name), or a bare value as the last line
    /// of the formula (`end` Eof) or of a layer (`end` RBrace): its gray.
    fn colour_or_value(&mut self, t: Token, end: Tok) -> Result<Colour, Error> {
        let word = if t.tok == Tok::Name { self.text(t) } else { "" };
        if t.tok == Tok::Name
            && COLOURS.iter().any(|e| e.0 == word)
            && self.toks[self.pos + 1].tok == Tok::LParen
        {
            self.next();
            let c = self.colour(t)?;
            self.end_of_statement()?;
            return Ok(c);
        }
        let id = self.expr(0)?;
        self.end_of_statement()?;
        self.skip_newlines();
        if self.peek().tok != end {
            return Err(Error::new(
                t.start,
                self.toks[self.pos.saturating_sub(1)].start.max(t.end),
                if end == Tok::Eof {
                    "only the last line can be a bare value (the colour) — did you mean `name = …`?"
                } else {
                    "only a layer's last line can be a bare value (its colour) — did you mean `name = …`?"
                },
            ));
        }
        Ok(Colour {
            code: code::OUT_GRAY,
            args: [id, 0, 0],
            pal: 0,
            alpha: None,
            blend: fixed::BLEND_OVER,
        })
    }

    /// `layer [blend] { … }` at `kw`: its own values (and `frame { }`),
    /// ending in its colour.
    fn layer(&mut self, kw: Token) -> Result<(Colour, Token), Error> {
        self.next();
        let mut blend = fixed::BLEND_OVER;
        let b = self.peek();
        if b.tok == Tok::Name {
            let name = self.text(b);
            let Some(&(_, m)) = BLENDS.iter().find(|e| e.0 == name) else {
                let s = suggest(name, BLENDS.iter().map(|e| e.0));
                return self.err(
                    b,
                    format!(
                        "unknown blend `{name}`{} (a layer goes over, add, multiply, screen, max or min)",
                        did_you_mean(s)
                    ),
                );
            };
            self.next();
            blend = m;
        }
        let open = self.peek();
        if open.tok != Tok::LBrace {
            return self.err(
                open,
                format!(
                    "expected `{{` here (as in `layer add {{ … }}`), found {}",
                    self.found(open)
                ),
            );
        }
        self.next();
        self.layers += 1;
        self.scope = self.layers;
        let mut seen_frame = false;
        let mut colour: Option<(Colour, Token)> = None;
        self.skip_newlines();
        loop {
            let t = self.peek();
            match t.tok {
                Tok::RBrace => break,
                Tok::Eof => return self.err(kw, "this `layer {` is never closed — add `}`"),
                _ => {}
            }
            if let Some((_, ct)) = &colour {
                let ct = *ct;
                return self.err(
                    ct,
                    "the colour has to be the layer's last line (move this to just before `}`)",
                );
            }
            match self.keyword() {
                "loop" => {
                    return self.err(
                        t,
                        "`loop` is the whole formula's: put it on a line of its own, outside the layers",
                    )
                }
                "layer" => {
                    return self.err(
                        t,
                        "a layer can't go inside another — close this one with `}` first",
                    )
                }
                "frame" => self.frame_block(t, &mut seen_frame)?,
                _ if self.is_assignment() => self.assignment(false)?,
                _ => colour = Some((self.colour_or_value(t, Tok::RBrace)?, t)),
            }
            self.skip_newlines();
        }
        self.next();
        self.scope = 0;
        let Some((mut c, ct)) = colour else {
            return self.err(
                kw,
                "this layer has no colour: end it with `rgb(…)`, `hsv(…)`, `pal(…)` or `gray(…)`",
            );
        };
        c.blend = blend;
        self.end_of_statement()?;
        Ok((c, ct))
    }
}

/// What a compile also reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Info {
    /// Values set inside `frame { }` blocks.
    pub frame_values: usize,
    /// `layer` blocks written (0: the formula is one picture, its last
    /// line).
    pub layers: usize,
}

/// Compile `src` into `prog` (which is cleared first). On an error `prog`
/// is left empty.
pub fn compile_into(src: &str, prog: &mut Program) -> Result<Info, Error> {
    prog.clear();
    let r = compile_inner(src, prog);
    if r.is_err() {
        prog.clear();
    }
    r
}

fn compile_inner(src: &str, prog: &mut Program) -> Result<Info, Error> {
    if src.len() > MAX_SOURCE {
        return Err(Error::new(
            MAX_SOURCE,
            src.len(),
            format!("too long: a formula can be {MAX_SOURCE} characters at most"),
        ));
    }
    let toks = lex(src)?;
    let mut p = Parser {
        src,
        toks,
        pos: 0,
        dag: Dag {
            nodes: Vec::with_capacity(64),
            index: alloc::vec![NO; SLOTS],
        },
        locals: Vec::new(),
        depth: 0,
        scope: 0,
        layers: 0,
    };
    // The last line's colour, or the layers' (bottom to top).
    let mut colour: Option<(Colour, Token)> = None;
    let mut layers: Vec<(Colour, Token)> = Vec::new();
    let mut seen_loop = false;
    let mut seen_frame = false;
    p.skip_newlines();
    loop {
        let t = p.peek();
        if t.tok == Tok::Eof {
            break;
        }
        if let Some((_, ct)) = &colour {
            let ct = *ct;
            return Err(Error::new(
                ct.start,
                ct.end,
                "the colour has to be the last line (move this to the end)",
            ));
        }
        match p.keyword() {
            "loop" => {
                p.next();
                if seen_loop {
                    return p.err(t, "`loop` is already set");
                }
                seen_loop = true;
                let n = p.next();
                let Tok::Num(v) = n.tok else {
                    return p.err(n, "`loop` takes a number of seconds, e.g. `loop 8`");
                };
                if !(ONE / 10..=3600 * ONE).contains(&v) {
                    return p.err(n, "the loop must be between 0.1 and 3600 seconds");
                }
                prog.loop_ms = ((v as i64 * 1000 + ONE as i64 / 2) >> 16) as u32;
                p.end_of_statement()?;
            }
            "frame" => p.frame_block(t, &mut seen_frame)?,
            "layer" => layers.push(p.layer(t)?),
            _ if p.is_assignment() => p.assignment(false)?,
            _ => {
                let c = p.colour_or_value(t, Tok::Eof)?;
                if !layers.is_empty() {
                    return Err(Error::new(
                        t.start,
                        p.toks[p.pos.saturating_sub(1)].end.max(t.end),
                        "with layers, every colour goes inside one: put this line in `layer { … }`",
                    ));
                }
                colour = Some((c, t));
            }
        }
        p.skip_newlines();
    }
    let n_layers = layers.len();
    if let Some(c) = colour {
        layers.push(c);
    }
    if layers.is_empty() {
        let at = src.len();
        let msg = if p.locals.is_empty() {
            "empty: write a colour, e.g. `hsv(x + t/4, 1, 1)`"
        } else {
            "the last line should be the colour, e.g. `rgb(r, g, b)`, `hsv(h, s, v)` or `pal(fire, k)`"
        };
        return Err(Error::new(at, at, msg));
    }
    let frame_values = p.locals.iter().filter(|l| l.in_frame).count();
    // A layer under one that covers everything (`over`, coverage 1) is never
    // seen: from the topmost such layer up. If it is the bottom one it is
    // written straight over the pixels (as v0's last line); otherwise the
    // picture under the formula shows through and every layer is blended.
    let dag = &p.dag;
    let opaque = |c: &Colour| {
        c.blend == fixed::BLEND_OVER
            && c.alpha
                .is_none_or(|a| dag.konst(a).is_some_and(|v| v >= ONE))
    };
    if let Some(k) = layers.iter().rposition(|(c, _)| opaque(c)) {
        layers.drain(..k);
    }
    let backdrop = !opaque(&layers[0].0);
    let ct = layers[layers.len() - 1].1;
    let mut steps = Vec::with_capacity(layers.len());
    for (k, (c, _)) in layers.iter().enumerate() {
        let plain = k == 0 && !backdrop;
        let alpha = match (plain, c.alpha) {
            (true, _) => None,
            (false, Some(a)) => Some(a),
            (false, None) => Some(p.dag.constant(ONE, (ct.start, ct.end))?),
        };
        steps.push(Step {
            code: if plain {
                c.code
            } else {
                code::OUT_LAYER + (c.code - code::OUT_RGB) + 4 * c.blend
            },
            args: c.args,
            pal: c.pal,
            alpha,
        });
    }
    // A limit hit while laying out the steps is the formula's as a whole:
    // point at its (top) colour line.
    emit(&p.dag, &steps, backdrop, prog).map_err(|e| Error {
        start: ct.start,
        end: ct.end,
        ..e
    })?;
    Ok(Info {
        frame_values,
        layers: n_layers,
    })
}

/// A colour step to lay out: its code (plain or a layer's), its values,
/// the palette and the coverage (a layer's).
struct Step {
    code: u8,
    args: [Id; 3],
    pal: u8,
    alpha: Option<Id>,
}

impl Step {
    /// What it reads, in operand order (the coverage last).
    fn reads(&self) -> impl Iterator<Item = Id> + '_ {
        self.args[..code::arity(self.code)]
            .iter()
            .copied()
            .chain(self.alpha)
    }
}

/// Lay the reachable nodes out as steps: scalar slots for the constants
/// and the per-frame and per-row values, lane registers for the per-pixel
/// ones (linear scan: a register is free again after its value's last
/// use, and a step never writes a register it reads). The per-pixel values
/// go in the order the layers need them: each layer's colour step right
/// after the values it is the first to read.
fn emit(dag: &Dag, steps: &[Step], backdrop: bool, prog: &mut Program) -> Result<(), Error> {
    let nodes = &dag.nodes;
    let n = nodes.len();
    let too_long = |steps: usize| {
        Error::new(
            0,
            0,
            format!(
                "this formula is too long for the panel ({steps} steps; the limit is {MAX_OPS}) — simplify it"
            ),
        )
    };
    // Reachability, and the first colour step each value is needed for
    // (children always precede their parents).
    const NONE: usize = usize::MAX;
    let mut need = alloc::vec![NONE; n];
    for (k, st) in steps.iter().enumerate() {
        for a in st.reads() {
            need[a as usize] = need[a as usize].min(k);
        }
    }
    for i in (0..n).rev() {
        let m = &nodes[i];
        if need[i] == NONE || m.code >= K_CONST {
            continue;
        }
        for &x in [m.a, m.b, m.c][..code::arity(m.code)].iter() {
            need[x as usize] = need[x as usize].min(need[i]);
        }
    }
    let live = |i: usize| need[i] != NONE;
    let is_op = |i: usize| live(i) && nodes[i].code < K_CONST;
    let total = (0..n).filter(|&i| is_op(i)).count() + steps.len();
    if total > MAX_OPS {
        return Err(too_long(total));
    }
    // Where each node's value lives.
    let mut loc = alloc::vec![0u8; n];
    let mut next_slot = S_FIRST_FREE as usize;
    let mut new_slot = |v: Option<i32>, prog: &mut Program| -> Result<u8, Error> {
        if next_slot >= MAX_SCALARS {
            return Err(Error::new(
                0,
                0,
                format!(
                    "this formula has too many numbers and steps (more than {MAX_SCALARS} values) — simplify it"
                ),
            ));
        }
        prog.init[next_slot] = v.unwrap_or(0);
        next_slot += 1;
        Ok((next_slot - 1) as u8)
    };
    for i in 0..n {
        if live(i) && nodes[i].code == K_CONST {
            loc[i] = new_slot(Some(nodes[i].val), prog)?;
        } else if live(i) && nodes[i].code == K_INPUT {
            loc[i] = nodes[i].val as u8;
            if nodes[i].val as u8 == LANE | L_I {
                prog.uses |= USES_I;
            }
        } else if is_op(i) && (code::U_R..=code::U_L).contains(&nodes[i].code) {
            prog.uses |= UNDER;
        }
    }
    if backdrop {
        prog.uses |= BACKDROP;
    }
    let mut ops: Vec<Op> = Vec::new();
    let mk = |m: &Node, d: u8, loc: &[u8]| Op {
        code: m.code,
        d,
        a: if code::arity(m.code) > 0 {
            loc[m.a as usize]
        } else {
            0
        },
        b: if code::arity(m.code) > 1 {
            loc[m.b as usize]
        } else {
            0
        },
        c: if code::arity(m.code) > 2 {
            loc[m.c as usize]
        } else {
            0
        },
    };
    for level in [LV_FRAME, LV_ROW] {
        for i in 0..n {
            let m = &nodes[i];
            if is_op(i) && m.level == level {
                let d = new_slot(None, prog)?;
                loc[i] = d;
                ops.push(mk(m, d, &loc));
            }
        }
        if level == LV_FRAME {
            prog.frame_end = ops.len().min(MAX_OPS) as u16;
        } else {
            prog.row_end = ops.len().min(MAX_OPS) as u16;
        }
    }
    // Per pixel: the order (a value, or colour step k), each value's last
    // reader in it, then registers by linear scan.
    #[derive(Clone, Copy)]
    enum Item {
        Value(usize),
        Colour(usize),
    }
    let mut seq: Vec<Item> = Vec::new();
    for k in 0..steps.len() {
        for i in 0..n {
            if is_op(i) && nodes[i].level == LV_PIXEL && need[i] == k {
                seq.push(Item::Value(i));
            }
        }
        seq.push(Item::Colour(k));
    }
    let mut last_use = alloc::vec![0usize; n];
    for (at, item) in seq.iter().enumerate() {
        match *item {
            Item::Value(i) => {
                let m = &nodes[i];
                for &x in [m.a, m.b, m.c][..code::arity(m.code)].iter() {
                    last_use[x as usize] = at;
                }
            }
            Item::Colour(k) => {
                for x in steps[k].reads() {
                    last_use[x as usize] = at;
                }
            }
        }
    }
    let mut free: Vec<u8> = (0..REGS as u8).rev().collect();
    let mut holder: [Option<usize>; REGS] = [None; REGS];
    let mut used = 0usize;
    for (at, item) in seq.iter().enumerate() {
        let wrote = match *item {
            Item::Value(i) => {
                let Some(r) = free.pop() else {
                    return Err(Error::new(
                        0,
                        0,
                        format!(
                            "this formula keeps too many values at once (more than {REGS}) — simplify it"
                        ),
                    ));
                };
                used = used.max(REGS - free.len());
                let d = LANE | (L_REG0 + r);
                loc[i] = d;
                ops.push(mk(&nodes[i], d, &loc));
                holder[r as usize] = Some(i);
                Some(i)
            }
            Item::Colour(k) => {
                let st = &steps[k];
                let l = |x: Id| loc[x as usize];
                let (kind, _) = code::colour_parts(st.code);
                let cover = st.alpha.map_or(0, l);
                let rgb = kind == code::OUT_RGB || kind == code::OUT_HSV;
                ops.push(Op {
                    code: st.code,
                    d: if rgb { cover } else { st.pal },
                    a: l(st.args[0]),
                    b: if rgb { l(st.args[1]) } else { cover },
                    c: if rgb { l(st.args[2]) } else { 0 },
                });
                None
            }
        };
        // Free the registers whose values this step read for the last time
        // (after picking the destination, so it is never one of them).
        for (reg, h) in holder.iter_mut().enumerate() {
            if let Some(v) = *h {
                if Some(v) != wrote && last_use[v] <= at {
                    *h = None;
                    free.push(reg as u8);
                }
            }
        }
        if ops.len() > MAX_OPS {
            return Err(too_long(ops.len()));
        }
    }
    prog.pixel_end = ops.len() as u16;
    prog.ops[..ops.len()].copy_from_slice(&ops);
    prog.n_scalars = next_slot as u8;
    prog.regs = used as u8;
    Ok(())
}
