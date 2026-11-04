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

use crate::builtins::*;
pub use crate::error::{Error, NO_MEMORY};
use crate::fixed::{self, ONE};
use crate::meta::Meta;
use crate::number;
use crate::program::{code, *};
use crate::tables;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// The language service's notes (`syntax::Sink::note`): not errors, so out
// of this file (whose sentences tests/diagnostics.rs reads as errors).
#[cfg(feature = "syntax")]
#[path = "notes.rs"]
mod notes;

/// The longest source accepted (bytes): comments and indentation count, so
/// it is roomy (4,096 until Formulas were played by reference, 2026-10-29:
/// the readable Endless road is ~6 KB). What the panel compiles is bounded
/// by its heap check, by tokens (`compile_heap_estimate`); a Formula read
/// from the card is compacted first (8 KB at most), an inline path is 6 KB.
pub const MAX_SOURCE: usize = 32 * 1024;
/// Expression nodes (before dead ones are dropped): bounds compile time.
pub const MAX_NODES: usize = 1024;
/// Brackets, operators and function calls nested this deep: bounds the
/// parser's stack. The compiler runs off the device now (the browser's
/// wasm, 1 MB of stack; the CLI and the simulator): 48 levels at the
/// ESP32's ~430 B a level is ~21 KB (18 until psi-rebuild, for the
/// device's 12 KB compile stack; psi-probe builds, which still compile on
/// the device, compile only the bundled Formulas, ≤ 12 deep).
pub const MAX_DEPTH: usize = 48;

// (Unused in the wasm build: it grows infallibly.)
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[cold]
#[inline(never)]
fn no_memory() -> Error {
    Error::new(0, 0, NO_MEMORY)
}

/// What a compile of `src` is expected to take from the heap at its peak,
/// at most (bytes): the firmware asks for this much free, plus the
/// compile's stack and a reserve, before it compiles. Counted by **tokens**
/// (one lexing pass that keeps nothing), not bytes, so a readable Formula
/// with long names and comments costs what its tokens cost: the tokens (12 B
/// each), the nodes (16 B, about one per two tokens), the sharing index
/// (4 KB) and the layout's lists; parts drawn in several places read their
/// tokens again, hence the floor. On the host (64-bit: its lists' entries
/// are as big as the device's or bigger) every bundled and example Formula
/// and the `worst` cases stay under it (`tests/compile_heap.rs`). A source
/// that doesn't lex fails before it takes anything: the floor.
pub fn compile_heap_estimate(src: &str) -> usize {
    let mut count = Toks {
        v: None,
        n: 0,
        nl: false,
    };
    let tokens = if src.len() > u16::MAX as usize {
        0
    } else {
        lex_into(src, &mut count).map_or(0, |()| count.n)
    };
    compile_heap_for_tokens(tokens)
}

/// [`compile_heap_estimate`] for a count of tokens.
pub const fn compile_heap_for_tokens(tokens: usize) -> usize {
    let e = HEAP_BASE + HEAP_PER_TOKEN * tokens;
    if e < HEAP_FLOOR {
        HEAP_FLOOR
    } else {
        e
    }
}
const HEAP_BASE: usize = 6 * 1024;
const HEAP_PER_TOKEN: usize = 30;
const HEAP_FLOOR: usize = 16 * 1024;

/// `v.push(x)`, or [`NO_MEMORY`] when the list can't grow (instead of the
/// abort an infallible push is on the device). In the browser (wasm: the
/// web module's bump heap grows the page's memory) the plain push: the
/// fallible paths cost the download ~2.4 KB gz.
#[inline]
fn try_push<T>(v: &mut Vec<T>, x: T) -> Result<(), Error> {
    #[cfg(not(target_arch = "wasm32"))]
    if v.len() == v.capacity() {
        v.try_reserve(1).map_err(|_| no_memory())?;
    }
    v.push(x);
    Ok(())
}

/// `v.extend_from_slice(s)`, or [`NO_MEMORY`] (wasm: as [`try_push`]).
fn try_extend<T: Copy>(v: &mut Vec<T>, s: &[T]) -> Result<(), Error> {
    #[cfg(not(target_arch = "wasm32"))]
    v.try_reserve(s.len()).map_err(|_| no_memory())?;
    v.extend_from_slice(s);
    Ok(())
}

/// `vec![x; n]`, or [`NO_MEMORY`].
fn try_vec<T: Clone>(x: T, n: usize) -> Result<Vec<T>, Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut v = try_with_capacity(n)?;
        v.resize(n, x);
        Ok(v)
    }
    #[cfg(target_arch = "wasm32")]
    Ok(alloc::vec![x; n])
}

/// An empty list with room for exactly `n`, or [`NO_MEMORY`] (wasm: as
/// [`try_push`]).
fn try_with_capacity<T>(n: usize) -> Result<Vec<T>, Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut v = Vec::new();
        v.try_reserve_exact(n).map_err(|_| no_memory())?;
        Ok(v)
    }
    #[cfg(target_arch = "wasm32")]
    Ok(Vec::with_capacity(n))
}

/// `s[a..b]`, or "" (never a panic: a slice of a `str` that could panic
/// brings ~3 KB of panic formatting into the wasm).
#[inline(never)]
fn sub(s: &str, a: usize, b: usize) -> &str {
    s.get(a..b).unwrap_or("")
}

// --- the lexer ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    Num(i32),
    /// `#rrggbb` (`#rgb` widened): a colour, 0xRRGGBB.
    Hex(u32),
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
    LBracket,
    RBracket,
    Comma,
    Dot,
    Assign,
    Newline,
    /// A row of a `sprite NAME { … }` block (its key line or a row of
    /// characters), as written: the lexer reads those lines raw.
    Raw,
    /// `"…"`, its quotes included: only on a `name`, `about` or `control`
    /// line ([`text_line`]); elsewhere a `"` is an error.
    Str,
    /// `..`, a control's range (`control zoom 1..4`).
    DotDot,
    Eof,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Token {
    pub(crate) tok: Tok,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// A [`Token`] as the lexer's list keeps it: 12 B instead of 16 on the
/// ESP32 (24 on the host). The list is the compiler's biggest; a source is
/// at most [`MAX_SOURCE`] bytes (the language service's at most 64 KB,
/// [`lex`]), so its places fit in 16 bits.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
pub(crate) struct Packed {
    tok: Tok,
    start: u16,
    end: u16,
}

/// (The browser keeps the tokens as they are: no heap to spare there, and
/// a smaller download.)
#[cfg(target_arch = "wasm32")]
pub(crate) type Packed = Token;

#[cfg(target_arch = "wasm32")]
impl Token {
    #[inline]
    pub(crate) fn get(self) -> Token {
        self
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Packed {
    #[inline]
    pub(crate) fn get(self) -> Token {
        Token {
            tok: self.tok,
            start: self.start as usize,
            end: self.end as usize,
        }
    }
}

fn describe(t: Tok) -> &'static str {
    match t {
        Tok::Num(_) => "a number",
        Tok::Hex(_) => "a colour",
        Tok::Name => "a name",
        Tok::Newline => "the end of the line",
        Tok::Eof => "the end",
        Tok::RParen => "`)`",
        Tok::LParen => "`(`",
        Tok::RBrace => "`}`",
        Tok::LBrace => "`{`",
        Tok::RBracket => "`]`",
        Tok::LBracket => "`[`",
        Tok::Comma => "`,`",
        Tok::Dot => "`.`",
        Tok::Assign => "`=`",
        Tok::Colon => "`:`",
        Tok::Question => "`?`",
        Tok::Raw => "a sprite's row",
        Tok::Str => "text in quotes",
        Tok::DotDot => "`..`",
        _ => "an operator",
    }
}

#[inline(never)]
pub(crate) fn lex(src: &str) -> Result<Vec<Packed>, Error> {
    if src.len() > u16::MAX as usize {
        return Err(Error::new(0, 0, "too long"));
    }
    // Twice: counted, then kept in a list of exactly that length. The
    // tokens are the compiler's biggest list (a 2 KB formula has ~1,500,
    // 16 B each on the ESP32), and a list grown by doubling holds the old
    // and the new block at once (the ~48 KB that aborted the device on
    // Endless road, 2026-10-29).
    let mut count = Toks {
        v: None,
        n: 0,
        nl: false,
    };
    lex_into(src, &mut count)?;
    let mut out = Toks {
        v: Some(try_with_capacity(count.n)?),
        n: 0,
        nl: false,
    };
    lex_into(src, &mut out)?;
    debug_assert_eq!(out.n, count.n);
    Ok(out.v.unwrap_or_default())
}

/// The lexer's output: only counted (`v` is `None`: [`lex`]'s first pass)
/// or kept; how many, and whether the last was a line's end (or there is
/// none yet).
struct Toks {
    v: Option<Vec<Packed>>,
    n: usize,
    nl: bool,
}

impl Toks {
    #[inline]
    fn push(&mut self, t: Token) {
        self.n += 1;
        self.nl = t.tok == Tok::Newline;
        if let Some(v) = &mut self.v {
            // (Never grows: `lex` reserved the count. `lex` checked the
            // source's length: the places fit.)
            #[cfg(not(target_arch = "wasm32"))]
            v.push(Packed {
                tok: t.tok,
                start: t.start as u16,
                end: t.end as u16,
            });
            #[cfg(target_arch = "wasm32")]
            v.push(t);
        }
    }
}

fn lex_into(src: &str, out: &mut Toks) -> Result<(), Error> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut depth = 0usize; // ( ) nesting: newlines inside are spaces
                            // At the start of a line (nothing but spaces before, outside brackets):
                            // where `sprite NAME {` may start a sprite block.
    let mut line_start = true;
    // On a `name`, `about` or `control` line: where text in quotes may be.
    let mut text = false;
    let push_nl = |out: &mut Toks, at: usize| {
        if out.n > 0 && !out.nl {
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
                push_nl(out, i);
                line_start = true;
            }
            text &= depth > 0;
            i += 1;
            continue;
        }
        if c == b'"' && text {
            i = quoted(src, i, out)?;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if line_start && depth == 0 {
            if let Some(j) = sprite_head(b, i) {
                i = sprite_rows(src, i, j, out);
                line_start = false;
                continue;
            }
        }
        let first = line_start && depth == 0;
        line_start = false;
        // `#ff8800`, `#f80`: a colour.
        if c == b'#' {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let h = sub(src, start + 1, i);
            let mut v = Some(0u32);
            for c in h.chars() {
                v = v.zip(c.to_digit(16)).map(|(v, d)| v << 4 | d);
            }
            let (Some(v), 3 | 6) = (v, h.len()) else {
                return Err(Error::new(
                    start,
                    i,
                    "a colour is # and 3 or 6 hex digits (0-9, a-f), as #f80 or #ff8800",
                ));
            };
            // #rgb: each digit twice.
            let v = if h.len() == 3 {
                (v >> 8) * 0x11_0000 + (v >> 4 & 15) * 0x1100 + (v & 15) * 0x11
            } else {
                v
            };
            out.push(Token {
                tok: Tok::Hex(v),
                start,
                end: i,
            });
            continue;
        }
        // `'A'`: a character's code (65), for `glyph`.
        if c == b'\'' {
            if let (Some(&ch @ 32..=126), Some(b'\'')) = (b.get(i + 1), b.get(i + 2)) {
                i += 3;
                out.push(Token {
                    tok: Tok::Num((ch as i32) << 16),
                    start,
                    end: i,
                });
                continue;
            }
            let end = i + sub(src, i + 1, b.len())
                .chars()
                .next()
                .map_or(1, |ch| ch.len_utf8() + 1);
            return Err(Error::new(
                start,
                end.min(b.len()),
                "a character goes in quotes, one at a time: 'A', '7' or ':' (there are no strings)",
            ));
        }
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            // (Not into `..`: `1..4` is a range.)
            while i < b.len()
                && (b[i].is_ascii_digit() || (b[i] == b'.' && b.get(i + 1) != Some(&b'.')))
            {
                i += 1;
            }
            let s = sub(src, start, i);
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
                        sub(src, start, j),
                        s,
                        sub(src, i, j)
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
        // `p.v`: a part's value.
        if c == b'.'
            && b.get(i + 1)
                .is_some_and(|&n| n.is_ascii_alphabetic() || n == b'_')
        {
            i += 1;
            out.push(Token {
                tok: Tok::Dot,
                start,
                end: i,
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let tok = match sub(src, start, i) {
                "and" => Tok::And,
                "or" => Tok::Or,
                "not" => Tok::Not,
                w => {
                    text |= (first && text_line(w)) || w == "print";
                    Tok::Name
                }
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
            (b'.', Some(b'.')) => (Tok::DotDot, 2),
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
            (b'[', _) => (Tok::LBracket, 1),
            (b']', _) => (Tok::RBracket, 1),
            (b',', _) => (Tok::Comma, 1),
            (b'=', _) => (Tok::Assign, 1),
            _ => {
                let ch = sub(src, i, b.len()).chars().next().unwrap_or('?');
                let end = i + ch.len_utf8();
                let hint = match ch {
                    '^' => " (there is no power operator: multiply, e.g. `x * x`)",
                    '&' | '|' => " (write `and` / `or`)",
                    // E001.
                    '"' => " (there are no strings but on `name`, `about` and `control` lines: for a character's code write it in single quotes, 'A')",
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
            Tok::LParen | Tok::LBracket => depth += 1,
            Tok::RParen | Tok::RBracket => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += n;
        out.push(Token { tok, start, end: i });
    }
    push_nl(out, b.len());
    out.push(Token {
        tok: Tok::Eof,
        start: b.len(),
        end: b.len(),
    });
    Ok(())
}

/// The lexer's tokens as the language service sees them ([`crate::syntax`]).
#[cfg(feature = "syntax")]
pub(crate) fn lex_tokens(src: &str) -> Result<Vec<(crate::syntax::Kind, usize, usize)>, Error> {
    use crate::syntax::Kind;
    Ok(lex(src)?
        .iter()
        .map(|t| t.get())
        .filter(|t| t.tok != Tok::Eof)
        .map(|t| {
            let k = match t.tok {
                Tok::Num(_) => Kind::Number,
                Tok::Hex(_) => Kind::Colour,
                Tok::Name => Kind::Name,
                Tok::Str => Kind::Text,
                Tok::Raw => Kind::Row,
                Tok::Newline => Kind::End,
                Tok::LParen
                | Tok::RParen
                | Tok::LBrace
                | Tok::RBrace
                | Tok::LBracket
                | Tok::RBracket
                | Tok::Comma
                | Tok::Dot
                | Tok::DotDot
                | Tok::Assign => Kind::Punctuation,
                _ => Kind::Operator,
            };
            (k, t.start, t.end)
        })
        .collect())
}

/// The words that start a line where text in quotes may be.
fn text_line(w: &str) -> bool {
    matches!(w, "name" | "about" | "control")
}

/// The longest a text in quotes may be (characters): a `name`, an `about`,
/// a control's label.
pub const MAX_NAME_TEXT: usize = 60;
pub const MAX_ABOUT_TEXT: usize = 120;
pub const MAX_LABEL_TEXT: usize = 24;

/// `"…"` at `i` (a `name`, `about` or `control` line): one token, quotes
/// and all, to the next `"` on the line. No escapes; no `<`, `>`, `\` or
/// control characters inside (the text goes into the web app's pages and
/// JSON as it is). Where it ends.
#[cold]
#[inline(never)]
fn quoted(src: &str, i: usize, out: &mut Toks) -> Result<usize, Error> {
    let b = src.as_bytes();
    let mut j = i + 1;
    while j < b.len() && b[j] != b'"' {
        let c = b[j];
        if c == b'\n' {
            break;
        }
        if c < 0x20 || c == 0x7f || matches!(c, b'<' | b'>' | b'\\') {
            // E954.
            return Err(Error::new(
                j,
                j + 1,
                "text in quotes can't have `<`, `>`, `\\` or a control character (a tab too) in it",
            ));
        }
        if c >= 0x80 {
            // A character of more than a byte: C1 controls (E954), and the
            // ones that reorder or hide text (E969: bidirectional controls,
            // zero-width characters, line and paragraph separators).
            let ch = sub(src, j, b.len()).chars().next().unwrap_or(' ');
            let n = ch.len_utf8();
            if matches!(ch, '\u{80}'..='\u{9f}') {
                return Err(Error::new(j, j + n, "text in quotes can't have `<`, `>`, `\\` or a control character (a tab too) in it"));
            }
            if matches!(ch, '\u{200b}'..='\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
            {
                // E969.
                return Err(Error::new(
                    j,
                    j + n,
                    "text in quotes can't have characters that hide or reorder text (zero-width, direction marks, line separators)",
                ));
            }
            j += n;
            continue;
        }
        j += 1;
    }
    if j >= b.len() || b[j] != b'"' {
        // E953.
        return Err(Error::new(
            i,
            j,
            "text in quotes ends with `\"` on the same line",
        ));
    }
    out.push(Token {
        tok: Tok::Str,
        start: i,
        end: j + 1,
    });
    Ok(j + 1)
}

/// `sprite NAME {` and nothing else (but spaces and a comment) on the line
/// from `i`: where the `{` is.
fn sprite_head(b: &[u8], i: usize) -> Option<usize> {
    let word = |j: usize| j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_');
    let space = |j: usize| j < b.len() && (b[j] == b' ' || b[j] == b'\t');
    if b.get(i..i + 6) != Some(b"sprite".as_slice()) || !space(i + 6) {
        return None;
    }
    let mut j = i + 6;
    while space(j) {
        j += 1;
    }
    if !(j < b.len() && (b[j].is_ascii_alphabetic() || b[j] == b'_')) {
        return None;
    }
    while word(j) {
        j += 1;
    }
    while space(j) {
        j += 1;
    }
    if b.get(j) != Some(&b'{') {
        return None;
    }
    let brace = j;
    j += 1;
    while space(j) {
        j += 1;
    }
    let comment = matches!(b.get(j..j + 2), Some(b"--") | Some(b"//"));
    (comment || j >= b.len() || b[j] == b'\n' || b[j] == b';').then_some(brace)
}

/// A sprite block from `i` (its `sprite`, the `{` at `brace`): `sprite`,
/// its name and `{` as tokens, then each line (to a newline or `;`, a
/// comment cut off, trimmed, blank ones left out) as a [`Tok::Raw`] up to a
/// line that is `}` alone, its [`Tok::RBrace`]. Where lexing goes on.
fn sprite_rows(src: &str, i: usize, brace: usize, out: &mut Toks) -> usize {
    let b = src.as_bytes();
    let mut name = i + 6;
    while b[name] == b' ' || b[name] == b'\t' {
        name += 1;
    }
    let mut e = name;
    while e < b.len() && (b[e].is_ascii_alphanumeric() || b[e] == b'_') {
        e += 1;
    }
    for (tok, s, e) in [
        (Tok::Name, i, i + 6),
        (Tok::Name, name, e),
        (Tok::LBrace, brace, brace + 1),
    ] {
        out.push(Token {
            tok,
            start: s,
            end: e,
        });
    }
    // To the end of the `{`'s line (a comment, a newline or `;`).
    let mut k = brace + 1;
    while k < b.len() && b[k] != b'\n' && b[k] != b';' {
        k += 1;
    }
    while k < b.len() {
        k += 1; // past the newline or `;`
        let start = k;
        let mut end = k;
        while k < b.len() && b[k] != b'\n' && b[k] != b';' {
            if matches!(b.get(k..k + 2), Some(b"--") | Some(b"//")) {
                // A comment, to the end of the line (as everywhere else).
                while k < b.len() && b[k] != b'\n' {
                    k += 1;
                }
                break;
            }
            k += 1;
            end = k;
        }
        let (mut s, mut e) = (start, end);
        while s < e && b[s].is_ascii_whitespace() {
            s += 1;
        }
        while e > s && b[e - 1].is_ascii_whitespace() {
            e -= 1;
        }
        if s == e {
            continue;
        }
        if &b[s..e] == b"}" {
            out.push(Token {
                tok: Tok::RBrace,
                start: s,
                end: e,
            });
            return e;
        }
        out.push(Token {
            tok: Tok::Raw,
            start: s,
            end: e,
        });
    }
    k.min(b.len())
}

/// Whether a line starts with a declaration (`name "…"`, `control …`; not
/// `name = …`, an assignment): where text in quotes may be. The first word,
/// past anything but letters and digits, then anything but `=`.
pub(crate) fn declaration_line(line: &str) -> bool {
    if line.contains("print") {
        return true;
    }
    let b = line.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let Some(a) = b.iter().position(|&c| word(c)) else {
        return false;
    };
    let e = a + b[a..].iter().take_while(|&&c| word(c)).count();
    DECLARATIONS.contains(&sub(line, a, e))
        && b[e..].iter().find(|c| !c.is_ascii_whitespace()) != Some(&b'=')
}

/// The same formula on one line, as short as it stays readable: comments
/// dropped, runs of spaces made one, lines joined with `; ` (which the
/// language reads as a line break, and ignores inside brackets, as it does
/// a newline). For `@formula?src=` paths, which are 2,048 B at most. Text
/// in quotes on a `name`, `about` or `control` line is kept as written
/// (spaces, `--` and `;` inside it too): on a line that starts with a
/// declaration, the only lines text in quotes can be on.
pub fn compact(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let text = declaration_line(line);
        let b = line.as_bytes();
        let mut code = String::new();
        let mut i = 0;
        let mut space = false;
        while i < b.len() {
            let c = b[i];
            // `'"'`, `'-'`: a character in quotes is one token, as the lexer
            // reads it (before a comment or a quote).
            if c == b'\''
                && b.get(i + 2) == Some(&b'\'')
                && b.get(i + 1).is_some_and(|&x| (32..=126).contains(&x))
            {
                if space && !code.is_empty() {
                    code.push(' ');
                }
                code.push_str(sub(line, i, i + 3));
                (i, space) = (i + 3, false);
                continue;
            }
            if c == b'"' && text {
                // To the closing quote (or the line's end), as written.
                let e = line[i + 1..].find('"').map_or(b.len(), |k| i + 2 + k);
                if space && !code.is_empty() {
                    code.push(' ');
                }
                code.push_str(sub(line, i, e));
                (i, space) = (e, false);
                continue;
            }
            if (c == b'-' || c == b'/') && b.get(i + 1) == Some(&c) {
                break;
            }
            if c.is_ascii_whitespace() {
                space = true;
                i += 1;
                continue;
            }
            if space && !code.is_empty() {
                code.push(' ');
            }
            space = false;
            // One character (whole, not a byte of one).
            let n = line[i..].chars().next().map_or(1, char::len_utf8);
            code.push_str(sub(line, i, i + n));
            i += n;
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

/// The same formula as short as it goes, line for line: comment lines,
/// blank lines, comments after code and indentation dropped, and the spaces
/// next to an operator or bracket (`= , * + - / < > ( ) [ ] { } ? : ;`)
/// too, so `x = a * -3` is `x=a*-3`. Never two `-` or `/` joined (that
/// would start a comment); text in quotes and a character in quotes kept
/// as written. It compiles to the very same program (tests/squeeze.rs holds
/// that for every example): the firmware's built-in bundled Formulas are
/// kept this way (build.rs), where every byte of the image counts.
pub fn squeeze(src: &str) -> String {
    const OPS: &[u8] = b"=,*+-/<>()[]{}?:;";
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let b = line.as_bytes();
        // The line's code: up to a comment outside quotes.
        let (mut end, mut i, mut quote) = (b.len(), 0, false);
        while i < b.len() {
            let c = b[i];
            if !quote && c == b'\'' && b.get(i + 2) == Some(&b'\'') {
                i += 3;
                continue;
            }
            if c == b'"' {
                quote = !quote;
            } else if !quote && (c == b'-' || c == b'/') && b.get(i + 1) == Some(&c) {
                end = i;
                break;
            }
            i += 1;
        }
        let code = line[..end].trim();
        if code.is_empty() {
            continue;
        }
        // Spaces out where nothing depends on them.
        let c = code.as_bytes();
        let mut kept = String::with_capacity(code.len());
        let (mut i, mut quote) = (0, false);
        while i < c.len() {
            let ch = c[i];
            if !quote && ch == b'\'' && c.get(i + 2) == Some(&b'\'') {
                kept.push_str(&code[i..i + 3]);
                i += 3;
                continue;
            }
            if ch == b'"' {
                quote = !quote;
            }
            if !quote && ch.is_ascii_whitespace() {
                let mut j = i;
                while j < c.len() && c[j].is_ascii_whitespace() {
                    j += 1;
                }
                let prev = kept.as_bytes().last().copied().unwrap_or(b' ');
                let next = c.get(j).copied().unwrap_or(b' ');
                let glue = (prev == next && (prev == b'-' || prev == b'/'))
                    || !(OPS.contains(&prev) || OPS.contains(&next));
                if glue {
                    kept.push(' ');
                }
                i = j;
                continue;
            }
            // One character (whole, not a byte of one).
            let n = code[i..].chars().next().map_or(1, char::len_utf8);
            kept.push_str(&code[i..i + n]);
            i += n;
        }
        out.push_str(&kept);
        out.push('\n');
    }
    out
}

// --- names ----------------------------------------------------------------------

/// `loop forever`'s loop (Q16 seconds, as `loop N`'s): longer than any.
const FOREVER: i32 = i32::MAX;

/// [`Parser::centred`]'s steps: (op, a, b) over 0 `x`, 1 `y`, 2 `py`, 3
/// ½, 4 four, 5 31 and, from 6, the steps before.
const CENTRED: [(u8, u8, u8); 6] = [
    (code::SUB, 0, 3),   // 6: x - ½
    (code::MUL, 6, 4),   // 7: cx
    (code::SUB, 1, 3),   // 8: cy
    (code::SUB, 5, 2),   // 9: ty
    (code::LEN, 7, 8),   // 10: r
    (code::ATAN2, 8, 7), // 11: angle
];

/// `f80` or `ff8800` as 0xRRGGBB.
fn hex_colour(h: &str) -> Option<u32> {
    let mut v = 0u32;
    for c in h.chars() {
        v = v << 4 | c.to_digit(16)?;
    }
    match h.len() {
        3 => Some(((v >> 8) & 15) * 0x11_0000 + ((v >> 4) & 15) * 0x1100 + (v & 15) * 0x11),
        6 => Some(v),
        _ => None,
    }
}

/// Optimal string alignment distance (edits, adjacent swaps count one),
/// in three rolling rows of bytes (it runs at the deepest point of an
/// error: a 25 × 25 table of words was 5 KB of stack there).
fn distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() > 24 || b.len() > 24 {
        return usize::MAX;
    }
    // Row k of the table is r[k % 3]: rows i-2 and i-1 are all row i needs.
    let mut r: [[u8; 25]; 3] = [[0; 25]; 3];
    for (j, v) in r[0].iter_mut().enumerate().take(b.len() + 1) {
        *v = j as u8;
    }
    // Row i is written over row i-3 in place, read from the other two (no
    // copies of rows: they were half this frame, at the deepest point).
    for i in 1..=a.len() {
        let (p2, p, c) = ((i + 1) % 3, (i - 1) % 3, i % 3);
        r[c][0] = i as u8;
        for j in 1..=b.len() {
            let cost = u8::from(a[i - 1] != b[j - 1]);
            let mut v = (r[p][j] + 1).min(r[c][j - 1] + 1).min(r[p][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(r[p2][j - 2] + 1);
            }
            r[c][j] = v;
        }
    }
    r[a.len() % 3][b.len()] as usize
}

#[cfg(test)]
pub(crate) fn distance_for_tests(a: &str, b: &str) -> usize {
    distance(a, b)
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
/// Inside a `for` of the draw stage (`LV_LOOP2`: one inside another): a
/// value that depends on its counter, worked out each turn.
const LV_LOOP1: u8 = 2;
const LV_LOOP2: u8 = 3;
const LV_ROW: u8 = 4;
const LV_PIXEL: u8 = 5;

/// Node kinds that are not steps: above every op code (the fused
/// compares are 0xf0..=0xf5), `code >= K_CONST` is "not a step".
const K_CONST: u8 = 250;
const _: () = assert!(code::CMP_LAST < K_CONST);
const K_INPUT: u8 = 251;
/// A `for` loop's counter (`val`: the loop's number), given a scalar slot
/// when the loop is laid out.
const K_COUNTER: u8 = 252;
/// A value that grows without end plus a small one that varies more than
/// once a frame (`t*0.15 + x`: `a` the wide part, `b` the small one), at
/// the small one's level. Never laid out: whatever takes it is rewritten
/// ([`Dag::wide_op`]) into the wide part's way back to a Q16.16 and the
/// small one (`noise(w + s)` is a `NOISE_O` of `frac(w) + s`).
const K_SPLIT: u8 = 253;
/// A `K_INPUT`'s `val` for `t` in a `loop forever` formula: wide slot 0.
const WIDE_T: i32 = 0x100;

/// E301, E708, E931, each followed by what makes the value vary
/// (`Parser::varies_err`).
const DRAW_ONLY: &str = "`draw { }` runs once a frame, before any pixel, but ";
const CYCLE_ROW: &str = "`cycle` turns a whole row at once (by t, a frame value or py), but ";
/// E970.
const SCENE_WHEN: &str = "a scene's `when` is worked out once a frame or once a row (by t, a frame value or py), but ";
/// Scenes a formula may have (`code::GUARD`'s `c` keeps its number).
const MAX_SCENES: usize = 15;
/// E936.
const TWEEN_FRAME: &str = "`tween` mixes a palette once a frame (by t or a frame value), but ";

/// E901: a value that grows without end where it can't go.
const GROWS: &str = "`t` grows without end in `loop forever`, and this can't take it: \
use frac(t*k), t % n, sin(t*k), noise(t*k, y, z), a comparison, min or max";

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
    /// The multiplies that per-pixel divisions by a constant became, as
    /// (the multiply, the dividend, the divisor): a `floor` of one is
    /// rebuilt with 1/k rounded up ([`fixed::recip_up`]).
    divs: Vec<(Id, Id, i32)>,
    /// Each `pick`'s choices (a `PICK` node's `b` is its index here).
    picks: Vec<Vec<Id>>,
    /// A `loop forever` formula: `t` is wide ([`WIDE_T`]).
    forever: bool,
    /// The wide noises' contexts: each axis's wide part (`NO`: none), the
    /// context `NOISE_O + k` of a `NOISE_O` node.
    ctx: Vec<[Id; 3]>,
    /// The steps a node needs before it that it doesn't read as a value: a
    /// `NOISE_O` node's (`val` the index here) `W_LAT`s.
    deps: Vec<Vec<Id>>,
    /// Each node as it was made, and where in the source (`syntax`: the
    /// language service's per-op costs).
    #[cfg(feature = "syntax")]
    made: Vec<(Id, usize, usize)>,
    /// `scene NAME when c { … }`: the nodes each made, `(first, end, its
    /// number from 1)` (`end` `NO` while it is read), and the one being
    /// read (0: none). Its row and pixel values are its own
    /// ([`Dag::scene_of`]): never shared with a value outside it.
    scenes: Vec<(Id, Id, u8)>,
    scene: u8,
    /// Each scene's `when` (scene k's at k − 1).
    conds: Vec<Id>,
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
    /// The scene whose steps work node `i` out (0: none): a row or pixel
    /// value made while the scene was read.
    fn scene_of(&self, i: usize) -> u8 {
        let m = &self.nodes[i];
        if m.level < LV_ROW || m.code >= K_CONST {
            return 0;
        }
        self.scenes
            .iter()
            .find(|&&(a, e, _)| i >= a as usize && (e == NO || i < e as usize))
            .map_or(0, |s| s.2)
    }
}

impl Dag {
    fn level(&self, id: Id) -> u8 {
        self.nodes[id as usize].level
    }

    #[inline(never)]
    fn konst(&self, id: Id) -> Option<i32> {
        let n = &self.nodes[id as usize];
        (n.code == K_CONST).then_some(n.val)
    }

    #[inline(never)]
    fn push(&mut self, n: Node, at: (usize, usize)) -> Result<Id, Error> {
        let mut slot = n.hash() & (SLOTS - 1);
        let tag = if self.scene != 0 && n.level >= LV_ROW && n.code < K_CONST {
            self.scene
        } else {
            0
        };
        loop {
            match self.index[slot] {
                NO => break,
                i if self.nodes[i as usize] == n && self.scene_of(i as usize) == tag => {
                    return Ok(i)
                }
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
        // Grown by half, never past MAX_NODES: the old and the new block are
        // held at once while it grows (`compile_heap_estimate` counts that).
        #[cfg(not(target_arch = "wasm32"))]
        if self.nodes.len() == self.nodes.capacity() {
            let cap = self.nodes.capacity();
            let more = (cap / 2).max(16).min(MAX_NODES - cap);
            self.nodes
                .try_reserve_exact(more)
                .map_err(|_| no_memory())?;
        }
        try_push(&mut self.nodes, n)?;
        let id = (self.nodes.len() - 1) as Id;
        self.index[slot] = id;
        #[cfg(feature = "syntax")]
        self.made.push((id, at.0, at.1));
        Ok(id)
    }

    #[inline(never)]
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

    #[inline(never)]
    fn input(&mut self, inp: Input, at: (usize, usize)) -> Result<Id, Error> {
        let why = INPUTS.iter().position(|e| e.1 == inp).unwrap_or(0) as u8;
        // What is under the formula: loaded into a lane by a step of its own.
        let under = match inp {
            Input::Ur => Some(code::U_R),
            Input::Ug => Some(code::U_G),
            Input::Ub => Some(code::U_B),
            Input::Ul => Some(code::U_L),
            Input::Ink => Some(code::INK),
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
            Input::T if self.forever => (WIDE_T, LV_FRAME),
            Input::Seed => (S_SEED as i32, LV_FRAME),
            Input::X => (lane(L_X) as i32, LV_PIXEL),
            Input::Px => (lane(L_PX) as i32, LV_PIXEL),
            Input::I => (lane(L_I) as i32, LV_PIXEL),
            Input::Y => (S_Y as i32, LV_ROW),
            Input::Py => (S_PY as i32, LV_ROW),
            Input::T => (S_T as i32, LV_FRAME),
            Input::Hour => (S_HOUR as i32, LV_FRAME),
            Input::Minute => (S_MINUTE as i32, LV_FRAME),
            Input::Second => (S_SECOND as i32, LV_FRAME),
            Input::A => (S_A as i32, LV_FRAME),
            Input::B => (S_B as i32, LV_FRAME),
            Input::C => (S_C as i32, LV_FRAME),
            Input::Ur | Input::Ug | Input::Ub | Input::Ul | Input::Ink => (0, LV_PIXEL),
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
    #[inline(never)]
    fn op(&mut self, c: u8, args: [Id; 3], at: (usize, usize)) -> Result<Id, Error> {
        let n = code::arity(c);
        if self.forever && args[..n].iter().any(|&x| self.grows(x)) {
            return self.wide_op(c, args, at);
        }
        let ks: [Option<i32>; 3] =
            core::array::from_fn(|i| if i < n { self.konst(args[i]) } else { Some(0) });
        // Fold (all but the table functions, which run once a frame).
        if ks.iter().all(Option::is_some) && !code::is_table(c) {
            let v = eval(
                c,
                ks[0].unwrap_or(0),
                ks[1].unwrap_or(0),
                ks[2].unwrap_or(0),
                None,
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
            // x / k, x per pixel: x · (1/k), exact for powers of two. Once a
            // frame or a row it stays a true division (exact, and it costs
            // nothing per pixel), so `floor(minute/10)` is 5 at 50.
            (code::DIV, None, Some(k)) if k != 0 && self.level(a) == LV_PIXEL => {
                let r = self.constant(fixed::recip(k), at)?;
                let m = self.op(code::MUL, [a, r, 0], at)?;
                if self.konst(m).is_none() && !self.divs.iter().any(|d| d.0 == m) {
                    self.divs.push((m, a, k));
                }
                return Ok(m);
            }
            // floor(x / k) per pixel: 1/k rounded up instead, so a whole
            // number over a whole number that divides it floors to exactly
            // that (rounded down, floor(px/10) was 1 at px = 20).
            (code::FLOOR, None, _) if self.divs.iter().any(|d| d.0 == a) => {
                let &(_, x, k) = self.divs.iter().find(|d| d.0 == a).unwrap();
                let r = self.constant(fixed::recip_up(k), at)?;
                let m = self.op(code::MUL, [x, r, 0], at)?;
                if m != a {
                    return self.op(code::FLOOR, [m, 0, 0], at);
                }
            }
            // 1 / x: `recip`, exactly `div(1, x)` (on the ESP32 one 32-bit
            // division where `div`'s general path takes two and a Knuth
            // digit).
            (code::DIV, Some(ONE), None) => return self.op(code::RECIP, [b, 0, 0], at),
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
            // A table (sin, a font) read at a constant: once a frame.
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

impl Dag {
    /// `NAME[k]`, the table of `len` entries at `start` of `tables`: a
    /// constant `k` is its entry (or 0), any other a `TAB` step at `k`'s
    /// level.
    #[inline(never)]
    fn tab(
        &mut self,
        k: Id,
        start: usize,
        len: usize,
        tables: &[i32],
        at: (usize, usize),
    ) -> Result<Id, Error> {
        if self.grows(k) {
            return Err(Error::new(at.0, at.1, GROWS));
        }
        if let Some(v) = self.konst(k) {
            let i = v >> 16;
            let e = if (0..len as i32).contains(&i) {
                tables.get(start + i as usize).copied().unwrap_or(0)
            } else {
                0
            };
            return self.constant(e, at);
        }
        let m = self.nodes[k as usize];
        self.push(
            Node {
                code: code::TAB,
                a: k,
                b: start as Id,
                c: (len - 1) as Id,
                val: 0,
                level: m.level,
                why: m.why,
            },
            at,
        )
    }

    /// `pick(k, choices…)`: the choice floor(k) counts to from 0, or 0. A
    /// constant `k` is that choice; a per-pixel `k` of choices that are the
    /// same for every pixel, one `PICK` step; otherwise compares and
    /// selects, `k < 0 ? 0 : k < 1 ? a : … : k < n ? z : 0`.
    #[inline(never)]
    fn pick(&mut self, k: Id, opts: Vec<Id>, at: (usize, usize)) -> Result<Id, Error> {
        if opts.iter().any(|&o| self.grows(o)) || (self.grows(k) && !self.is_wide(k)) {
            return Err(Error::new(at.0, at.1, GROWS));
        }
        let zero = self.constant(0, at)?;
        let n = opts.len() as i32;
        if let Some(v) = self.konst(k) {
            let i = v >> 16;
            return Ok(if (0..n).contains(&i) {
                opts[i as usize]
            } else {
                zero
            });
        }
        if self.level(k) < LV_PIXEL || opts.iter().any(|&o| self.level(o) == LV_PIXEL) {
            let mut acc = zero;
            for i in (-1..n).rev() {
                let o = if i < 0 { zero } else { opts[i as usize] };
                let c = self.constant((i + 1) << 16, at)?;
                let c = self.op(code::LT, [k, c, 0], at)?;
                acc = self.op(code::SELECT, [c, o, acc], at)?;
            }
            return Ok(acc);
        }
        // The same choices, the same table (a colour expression reads a
        // pick three times: one node, one step).
        let b = match self.picks.iter().position(|p| *p == opts) {
            Some(j) => j,
            None => {
                try_push(&mut self.picks, opts)?;
                self.picks.len() - 1
            }
        };
        let why = self.nodes[k as usize].why;
        self.push(
            Node {
                code: code::PICK,
                a: k,
                b: b as Id,
                c: 0,
                val: 0,
                level: LV_PIXEL,
                why,
            },
            at,
        )
    }
}

/// What makes `id` vary as much as it does: the nearest value on the way
/// down (breadth first, through the operands at `id`'s level) that `named`
/// knows (a local's index), else the input there. For the errors that name
/// the formula's own value and for the language service, never per frame.
#[cold]
#[inline(never)]
fn cause(dag: &Dag, id: Id, named: &dyn Fn(Id) -> Option<usize>) -> (Option<usize>, Id) {
    let lv = dag.level(id);
    let mut q: Vec<Id> = Vec::new();
    q.push(id);
    let mut k = 0;
    while k < q.len() && k < 256 {
        let m = q[k];
        k += 1;
        if m != id {
            if let Some(j) = named(m) {
                return (Some(j), m);
            }
        }
        let n = &dag.nodes[m as usize];
        if n.code == K_INPUT || (code::U_R..=code::INKN).contains(&n.code) {
            return (None, m);
        }
        let split = [n.a, n.b];
        let more: &[Id] = if n.code == K_SPLIT { &split } else { &[] };
        for x in reads(dag, m as usize).chain(more.iter().copied()) {
            if dag.level(x) == lv && !q.contains(&x) {
                q.push(x);
            }
        }
    }
    (None, id)
}

/// An input node's word and what it means (`seed` is not in [`INPUTS`]).
fn input_word(n: &Node) -> (&'static str, &'static str) {
    if n.code == K_INPUT && n.val == S_SEED as i32 {
        return ("seed", "a new number each play");
    }
    let e = INPUTS[n.why as usize];
    (e.0, e.2)
}

/// The compares the other way round (`a < b` is `b > a`).
fn flip(c: u8) -> u8 {
    match c {
        code::LT => code::GT,
        code::GT => code::LT,
        code::LE => code::GE,
        code::GE => code::LE,
        c => c,
    }
}

impl Dag {
    /// A value that grows without end, worked out once a frame: `t` in a
    /// `loop forever` formula and what `W_*` steps make of it.
    #[inline(never)]
    fn is_wide(&self, id: Id) -> bool {
        let n = &self.nodes[id as usize];
        (n.code == K_INPUT && n.val == WIDE_T)
            || (code::is_wide(n.code) && code::wide_ops(n.code).0)
    }

    /// A wide value or one with a wide part ([`K_SPLIT`]).
    #[inline(never)]
    fn grows(&self, id: Id) -> bool {
        self.is_wide(id) || self.nodes[id as usize].code == K_SPLIT
    }

    /// A value the same all frame that doesn't grow: what a wide step may
    /// take as its scalar operand.
    #[inline(never)]
    fn per_frame(&self, id: Id) -> bool {
        !self.grows(id) && self.level(id) <= LV_FRAME
    }

    /// A value's wide part and its small part (either may be missing).
    #[inline(never)]
    fn parts(&self, id: Id) -> (Option<Id>, Option<Id>) {
        let n = &self.nodes[id as usize];
        if n.code == K_SPLIT {
            (Some(n.a), Some(n.b))
        } else if self.is_wide(id) {
            (Some(id), None)
        } else {
            (None, Some(id))
        }
    }

    /// A wide step's node (`imm`: a compare's or min/max's kind in `c`;
    /// `val`: a `W_LAT`'s offset).
    #[inline(never)]
    fn wnode(
        &mut self,
        c: u8,
        a: Id,
        b: Id,
        imm: u8,
        val: i32,
        at: (usize, usize),
    ) -> Result<Id, Error> {
        let n = code::wide_ops(c).1;
        // w ± 0, w · 1, w / 1: w (exactly).
        match (c, self.konst(b)) {
            (code::W_ADDS | code::W_SUBS, Some(0)) | (code::W_MULS | code::W_DIVS, Some(ONE)) => {
                return Ok(a)
            }
            _ => {}
        }
        let (mut level, mut why) = (LV_FRAME, 0);
        for &x in &[a, b][..n] {
            let m = &self.nodes[x as usize];
            if m.level > level {
                (level, why) = (m.level, m.why);
            } else if m.level == level && why == 0 {
                why = m.why;
            }
        }
        self.push(
            Node {
                code: c,
                a,
                b: if n > 1 { b } else { 0 },
                c: imm as Id,
                val,
                level,
                why,
            },
            at,
        )
    }

    /// A wide part plus a small one: a wide value when the small one is the
    /// same all frame, else a [`K_SPLIT`].
    #[inline(never)]
    fn make(&mut self, w: Option<Id>, s: Option<Id>, at: (usize, usize)) -> Result<Id, Error> {
        match (w, s) {
            (Some(w), None) => Ok(w),
            (None, Some(s)) => Ok(s),
            (None, None) => self.constant(0, at),
            (Some(w), Some(s)) if self.per_frame(s) => self.wnode(code::W_ADDS, w, s, 0, 0, at),
            (Some(w), Some(s)) => {
                let m = self.nodes[s as usize];
                self.push(
                    Node {
                        code: K_SPLIT,
                        a: w,
                        b: s,
                        c: 0,
                        val: 0,
                        level: m.level,
                        why: m.why,
                    },
                    at,
                )
            }
        }
    }

    /// The value's fraction-preserving small form: frac(w) for a wide value,
    /// frac(w) + s for a split (`sin`, `frac`, `noise` read only that much
    /// of the wide part: exactly, as long as the sum fits).
    #[inline(never)]
    fn small(&mut self, id: Id, at: (usize, usize)) -> Result<Id, Error> {
        match self.parts(id) {
            (Some(w), None) => self.wnode(code::W_FRAC, w, 0, 0, 0, at),
            (Some(w), Some(s)) => {
                let f = self.wnode(code::W_FRAC, w, 0, 0, 0, at)?;
                self.op(code::ADD, [f, s, 0], at)
            }
            (None, Some(s)) => Ok(s),
            (None, None) => self.constant(0, at),
        }
    }

    /// A wide value held to a Q16.16 (saturated).
    #[inline(never)]
    fn held(&mut self, w: Id, at: (usize, usize)) -> Result<Id, Error> {
        let lo = self.constant(i32::MIN, at)?;
        self.wnode(code::W_MINMAX, w, lo, code::MAX, 0, at)
    }

    /// A wide value (never a split) or E901.
    #[inline(never)]
    fn wide_only(&self, id: Id, at: (usize, usize)) -> Result<Id, Error> {
        if self.is_wide(id) {
            Ok(id)
        } else if self.nodes[id as usize].code == K_SPLIT {
            Err(Error::new(at.0, at.1, GROWS_SPLIT))
        } else {
            Err(Error::new(at.0, at.1, GROWS))
        }
    }

    /// Op `c` of operands at least one of which grows without end (`t` in a
    /// `loop forever` formula): rewritten into wide steps once a frame and
    /// the ways back to a Q16.16, each exactly what the Q16.16 operation
    /// gives wherever the values fit one (`crate::fixed`'s `w_*`); E901
    /// where there is no such way (a growing value per pixel, `t*t`,
    /// `sqrt(t)`, …).
    #[inline(never)]
    fn wide_op(&mut self, c: u8, args: [Id; 3], at: (usize, usize)) -> Result<Id, Error> {
        use code::*;
        let [a, b, k3] = args;
        let grows = |d: &Self, x: Id| d.grows(x);
        // (A growing value with a part per pixel or row: say where it can go.)
        let split = args[..code::arity(c)]
            .iter()
            .any(|&x| self.nodes[x as usize].code == K_SPLIT);
        let bad = || {
            Err(Error::new(
                at.0,
                at.1,
                if split { GROWS_SPLIT } else { GROWS },
            ))
        };
        match c {
            ADD | SUB => {
                let ((wa, sa), (wb, sb)) = (self.parts(a), self.parts(b));
                // w - s and s - w as they are (no negation that saturates).
                if c == SUB {
                    if let (Some(w), None, None, Some(s)) = (wa, sa, wb, sb) {
                        return if self.per_frame(s) {
                            self.wnode(W_SUBS, w, s, 0, 0, at)
                        } else {
                            let n = self.op(NEG, [s, 0, 0], at)?;
                            self.make(Some(w), Some(n), at)
                        };
                    }
                }
                let w = match (wa, wb) {
                    (Some(x), Some(y)) => {
                        Some(self.wnode(if c == ADD { W_ADD } else { W_SUB }, x, y, 0, 0, at)?)
                    }
                    (Some(x), None) => Some(x),
                    (None, Some(y)) if c == ADD => Some(y),
                    (None, Some(y)) => Some(self.wnode(W_NEG, y, 0, 0, 0, at)?),
                    (None, None) => None,
                };
                let s = match (sa, sb) {
                    (Some(x), Some(y)) => Some(self.op(c, [x, y, 0], at)?),
                    (Some(x), None) => Some(x),
                    (None, Some(y)) if c == ADD => Some(y),
                    (None, Some(y)) => Some(self.op(NEG, [y, 0, 0], at)?),
                    (None, None) => None,
                };
                self.make(w, s, at)
            }
            NEG => {
                let (w, s) = self.parts(a);
                let w = match w {
                    Some(w) => Some(self.wnode(W_NEG, w, 0, 0, 0, at)?),
                    None => None,
                };
                let s = match s {
                    Some(s) => Some(self.op(NEG, [s, 0, 0], at)?),
                    None => None,
                };
                self.make(w, s, at)
            }
            MUL | DIV => {
                let (g, k) = if grows(self, a) { (a, b) } else { (b, a) };
                if !self.per_frame(k) || (c == DIV && g != a) {
                    return bad();
                }
                if c == MUL && self.konst(k) == Some(0) {
                    return self.constant(0, at);
                }
                let (w, s) = self.parts(g);
                let Some(w) = w else { return bad() };
                let Some(s) = s else {
                    return self.wnode(if c == MUL { W_MULS } else { W_DIVS }, w, k, 0, 0, at);
                };
                // (w + s)·k exactly: w·k wide, and s·k with w·k's 16 bits
                // below the point carried in (`MULR`), so the floor is the
                // sum's. A split divided has no such cheap form: E901.
                if c == DIV {
                    return bad();
                }
                let wk = self.wnode(W_MULS, w, k, 0, 0, at)?;
                let r = self.wnode(W_REM, w, k, 0, 0, at)?;
                let sk = self.op(MULR, [s, k, r], at)?;
                self.make(Some(wk), Some(sk), at)
            }
            MOD => {
                if grows(self, b) || !self.per_frame(b) {
                    return bad();
                }
                match self.parts(a) {
                    (Some(w), None) => self.wnode(W_MOD, w, b, 0, 0, at),
                    // (w + s) % n = (w % n + s) % n.
                    (Some(w), Some(s)) => {
                        let m = self.wnode(W_MOD, w, b, 0, 0, at)?;
                        let x = self.op(ADD, [m, s, 0], at)?;
                        self.op(MOD, [x, b, 0], at)
                    }
                    _ => bad(),
                }
            }
            FRAC | SIN | COS | TRI | SAW | SQR => {
                let x = self.small(a, at)?;
                if c == FRAC && self.parts(a).1.is_none() {
                    return Ok(x);
                }
                self.op(c, [x, 0, 0], at)
            }
            FLOOR => {
                let w = self.wide_only(a, at)?;
                self.wnode(W_FLOOR, w, 0, 0, 0, at)
            }
            LT..=NE => {
                let (x, y, cc) = if grows(self, a) {
                    (a, b, c)
                } else {
                    (b, a, flip(c))
                };
                let x = self.wide_only(x, at)?;
                if self.is_wide(y) {
                    let d = self.wnode(W_SUB, x, y, 0, 0, at)?;
                    let z = self.constant(0, at)?;
                    return self.wnode(W_CMP, d, z, cc, 0, at);
                }
                if !self.per_frame(y) {
                    return bad();
                }
                self.wnode(W_CMP, x, y, cc, 0, at)
            }
            // step(e, x) is x >= e.
            STEP => self.op(GE, [b, a, 0], at),
            NOT => {
                let z = self.constant(0, at)?;
                self.op(EQ, [a, z, 0], at)
            }
            AND | OR => {
                let z = self.constant(0, at)?;
                let mut v = [a, b];
                for x in &mut v {
                    if grows(self, *x) {
                        let w = self.wide_only(*x, at)?;
                        *x = self.wnode(W_CMP, w, z, NE, 0, at)?;
                    }
                }
                self.op(c, [v[0], v[1], 0], at)
            }
            // min, max and clamp of a wide value and any other: of it held
            // to a Q16.16 (the same: the result is held so anyway), or in
            // one step with a value the same all frame.
            MIN | MAX => {
                let (x, y) = if grows(self, a) { (a, b) } else { (b, a) };
                let x = self.wide_only(x, at)?;
                if grows(self, y) {
                    return bad();
                }
                if self.per_frame(y) {
                    return self.wnode(W_MINMAX, x, y, c, 0, at);
                }
                let h = self.held(x, at)?;
                self.op(c, [h, y, 0], at)
            }
            CLAMP => {
                let x = self.wide_only(a, at)?;
                if grows(self, b) || grows(self, k3) {
                    return bad();
                }
                let m = if self.per_frame(b) {
                    self.wnode(W_MINMAX, x, b, MAX, 0, at)?
                } else {
                    let h = self.held(x, at)?;
                    self.op(MAX, [h, b, 0], at)?
                };
                self.op(MIN, [m, k3, 0], at)
            }
            // mix(a, b, t) = a + (b - a)·t, a and b the same all frame.
            MIX => {
                let t = self.wide_only(k3, at)?;
                if !self.per_frame(a) || !self.per_frame(b) {
                    return bad();
                }
                let d = self.op(SUB, [b, a, 0], at)?;
                let m = self.wnode(W_MULS, t, d, 0, 0, at)?;
                self.wnode(W_ADDS, m, a, 0, 0, at)
            }
            // smoothstep's smooth(x, e0, 1/(e1 - e0)): its clamped ramp
            // worked out wide, then the curve of that.
            SMOOTH => {
                let x = self.wide_only(a, at)?;
                if !self.per_frame(b) || !self.per_frame(k3) {
                    return bad();
                }
                let d = self.wnode(W_SUBS, x, b, 0, 0, at)?;
                let m = self.wnode(W_MULS, d, k3, 0, 0, at)?;
                let (z, one) = (self.constant(0, at)?, self.constant(ONE, at)?);
                let lo = self.wnode(W_MINMAX, m, z, MAX, 0, at)?;
                let r = self.op(MIN, [lo, one, 0], at)?;
                self.op(SMOOTH, [r, z, one], at)
            }
            SELECT => {
                if grows(self, b) || grows(self, k3) {
                    return bad();
                }
                let w = self.wide_only(a, at)?;
                let z = self.constant(0, at)?;
                let cnd = self.wnode(W_CMP, w, z, NE, 0, at)?;
                self.op(SELECT, [cnd, b, k3], at)
            }
            HASH => {
                let mut v = [a, b];
                for x in &mut v {
                    if grows(self, *x) {
                        let w = self.wide_only(*x, at)?;
                        *x = self.wnode(W_RED, w, 0, 0, 0, at)?;
                    }
                }
                self.op(HASH, [v[0], v[1], 0], at)
            }
            NOISE => self.noise_o(args, at),
            _ => bad(),
        }
    }

    /// `noise(x, y, z)` with an axis that grows without end: a `NOISE_O`
    /// of the axes' small forms ([`Dag::small`]) in a context whose
    /// lattice offsets `W_LAT` steps set from the wide parts once a frame.
    #[inline(never)]
    fn noise_o(&mut self, args: [Id; 3], at: (usize, usize)) -> Result<Id, Error> {
        // Once a frame, only x wide (`noise(u, 5, 0)`): one step, no
        // context.
        let [x, y, z] = args;
        if self.is_wide(x) && self.per_frame(y) && self.per_frame(z) {
            let m = self.nodes[x as usize];
            return self.push(
                Node {
                    code: code::W_NOISEX,
                    a: x,
                    b: y,
                    c: z,
                    val: 0,
                    level: LV_FRAME,
                    why: m.why,
                },
                at,
            );
        }
        let mut key = [NO; 3];
        let mut small = [0 as Id; 3];
        for k in 0..3 {
            key[k] = self.parts(args[k]).0.unwrap_or(NO);
            small[k] = self.small(args[k], at)?;
        }
        let ctx = match self.ctx.iter().position(|c| *c == key) {
            Some(j) => j,
            None if self.ctx.len() < NOISE_CTX => {
                try_push(&mut self.ctx, key)?;
                self.ctx.len() - 1
            }
            None => {
                return Err(Error::new(
                    at.0,
                    at.1,
                    "noise of values that grow without end: four different ones at most — share one, as noise(u + k, …)",
                ))
            }
        };
        let mut lats = Vec::new();
        for (axis, &w) in key.iter().enumerate() {
            if w != NO {
                lats.push(self.wnode(code::W_LAT, w, 0, 0, (ctx * 3 + axis) as i32, at)?);
            }
        }
        let di = match self.deps.iter().position(|d| *d == lats) {
            Some(j) => j,
            None => {
                try_push(&mut self.deps, lats)?;
                self.deps.len() - 1
            }
        };
        let (mut level, mut why) = (LV_FRAME, 0);
        for &x in &small {
            let m = &self.nodes[x as usize];
            if m.level > level {
                (level, why) = (m.level, m.why);
            }
        }
        self.push(
            Node {
                code: code::NOISE_O + ctx as u8,
                a: small[0],
                b: small[1],
                c: small[2],
                val: di as i32,
                level,
                why,
            },
            at,
        )
    }
}

/// Names from other languages (GLSL, Shadertoy, PICO-8) and what Psi has
/// instead: an unknown name or function among them says so.
const HABITS: &[(&str, &str)] = &[
    ("pow", "Psi has no `pow`: multiply (`x*x*x`), or `sqrt(v)`"),
    ("exp", "Psi has no `exp`: try `smoothstep(e0, e1, v)` or a palette for a curve"),
    ("log", "Psi has no `log`: try `sqrt(v)` or `smoothstep(e0, e1, v)` for a curve"),
    ("mod", "`a % b` is the remainder (floored: `-1 % 3` is 2)"),
    ("fmod", "`a % b` is the remainder (floored: `-1 % 3` is 2)"),
    ("atan", "`atan2(dy, dx)` is the angle, in turns (0 to 1)"),
    ("lerp", "`mix(a, b, k)` goes from a to b as k goes from 0 to 1"),
    ("saturate", "`clamp(v, 0, 1)` holds it to 0..1"),
    ("length", "`len(dx, dy)` is the distance"),
    ("distance", "`len(x1 - x0, y1 - y0)` is the distance"),
    ("vec2", "Psi has no vectors: work out each value on its own and end with `rgb(r, g, b)`"),
    ("vec3", "Psi has no vectors: work out each value on its own and end with `rgb(r, g, b)` or `#rrggbb`"),
    ("vec4", "Psi has no vectors: work out each value on its own and end with `rgb(r, g, b)`"),
    ("texture", "the picture under the formula is `under(x, y)` (a colour), or `ur ug ub ul` at the pixel"),
    ("random", "`hash(a, b)` is random 0 to 1 (the same for the same a, b), `noise(x, y, z)` smooth random hills"),
    ("rand", "`hash(a, b)` is random 0 to 1 (the same for the same a, b), `noise(x, y, z)` smooth random hills"),
    ("rnd", "`hash(a, b)` is random 0 to 1 (the same for the same a, b): the frame's `t` in it changes it each frame"),
    ("iTime", "`t` is the time, in seconds into the loop"),
    ("time", "`t` is the time, in seconds into the loop"),
    ("u_time", "`t` is the time, in seconds into the loop"),
    ("fragCoord", "`px`, `py` are the pixel (0..127 across, 0..31 down from the top); `x`, `y` the same from 0 to 1"),
    ("gl_FragCoord", "`px`, `py` are the pixel (0..127 across, 0..31 down from the top); `x`, `y` the same from 0 to 1"),
    ("uv", "`x`, `y` go from 0 to 1 across and down the panel; `cx`, `cy` are centred"),
    ("iResolution", "the panel is 128 × 32: `px` 0..127, `py` 0..31, `w` and `h` its size"),
    ("resolution", "the panel is 128 × 32: `px` 0..127, `py` 0..31, `w` and `h` its size"),
    ("PI", "angles are turns: `sin(v)` goes round once as v goes from 0 to 1, so there is no pi"),
    ("pi", "angles are turns: `sin(v)` goes round once as v goes from 0 to 1, so there is no pi"),
    ("TAU", "angles are turns: `sin(v)` goes round once as v goes from 0 to 1, so there is no tau"),
    ("tau", "angles are turns: `sin(v)` goes round once as v goes from 0 to 1, so there is no tau"),
    ("color", "a colour is `#rrggbb`, `rgb(r, g, b)`, `hsv(h, s, v)`, `pal(P, v)` or `gray(v)`, on the last line"),
    ("colour", "a colour is `#rrggbb`, `rgb(r, g, b)`, `hsv(h, s, v)`, `pal(P, v)` or `gray(v)`, on the last line"),
    ("if", "`if c { … }` is for lines that draw, inside `draw { }`; for a value write `c ? a : b`"),
    ("else", "`if c { … } else { … }` is for lines that draw; for a value write `c ? a : b`"),
    ("true", "a comparison is 1 or 0: write 1"),
    ("false", "a comparison is 1 or 0: write 0"),
];

// --- the parser -------------------------------------------------------------------

/// Tokens read again for function calls (each reading the function's
/// text, the calls in it included), in all: bounds the compile's time
/// whatever the nesting (64 Ki tokens read again take ~10 ms in the
/// browser).
const MAX_WORK: usize = 65536;

/// `fn NAME(a, b) = value` or `fn NAME(a, b) { … }`: a function, read
/// again (its tokens `body..end`) with its values bound at each call.
#[derive(Clone)]
struct Func<'s> {
    name: &'s str,
    /// Where its name is.
    at: usize,
    /// Its values' names, and where each is written.
    params: Vec<(&'s str, usize)>,
    body: usize,
    end: usize,
    /// `{ … }`: lines (names set, then its value; or lines that draw,
    /// called on a line of their own in draw).
    block: bool,
    /// Where its last line starts, and what it is: [`F_VALUE`], a colour
    /// read by channel (`#…`, `rgb(…)`, `under`: [`F_CHANNELS`]) or a whole
    /// colour (`hsv(…)`, `pal(…)`, `gray(…)`: [`F_WHOLE`]).
    last: usize,
    colour: u8,
}

const F_VALUE: u8 = 0;
const F_CHANNELS: u8 = 1;
const F_WHOLE: u8 = 2;
/// Its last line draws (`pset(…)`, `for`, `if`): called only in draw.
const F_DRAWS: u8 = 3;

struct Local<'s> {
    name: &'s str,
    id: Id,
    /// Where its name is (the line is worked out only for an error: per
    /// local it made a long formula quadratic).
    at: usize,
    in_frame: bool,
    /// A `for` loop's counter (it can't be set).
    counter: bool,
    /// A `control` line's name (it keeps it: E203).
    control: bool,
    /// The scope it was set in ([`Parser::scopes`]).
    scope: u16,
}

/// `table NAME = …`: its entries are `Parser::tables[start..start + len]`.
struct Table<'s> {
    name: &'s str,
    at: usize,
    scope: u16,
    start: usize,
    len: usize,
}

/// A formula's own palette: its colours (its 32 entries, and over 16 when
/// it has 16 or fewer: for a tween's half), or a tween of palettes worked
/// out once a frame (`tween`: its parts' roots in `Parser::pnodes`, the
/// second a half's or `NO_PART`); the live slot `pal` and `cycle` read it
/// from, once one does.
struct OwnPal {
    entries: [i32; 32],
    half: Option<[i32; 16]>,
    tween: Option<[u16; 2]>,
    live: Option<u8>,
}

const NO_PART: u16 = u16::MAX;

/// A tween's palette: one of the formula's own (`Parser::palettes`), a
/// built-in (`PALETTES`' id), or another tween (`Parser::pnodes`), with
/// where it is written (errors).
#[derive(Clone, Copy)]
enum PSrc {
    Own(usize, (usize, usize)),
    Builtin(u8, (usize, usize)),
    Node(u16),
}

/// `tween(p, q, w)`.
#[derive(Clone, Copy)]
struct PNode {
    p: PSrc,
    q: PSrc,
    w: Id,
}

/// A tween step as `emit` lays it out: `TWEEN d, a, b` with its weight's
/// node.
#[derive(Clone, Copy)]
struct TweenOp {
    d: u8,
    a: u8,
    b: u8,
    w: Id,
}

/// `palette NAME = #…, …` (or `= tween(…)`): the formula's own palette
/// `j` (`Parser::palettes`).
struct Palette<'s> {
    name: &'s str,
    at: usize,
    scope: u16,
    j: usize,
    /// Its colours, and where its entries are written (tokens `from..to`:
    /// a name may come before a colour, `kerb #806`; `NAME.kerb` finds it
    /// there).
    n: u16,
    from: u32,
    to: u32,
}

/// `sprite NAME { … }`: its header is `Parser::tables[start]`.
struct Sprite<'s> {
    name: &'s str,
    at: usize,
    scope: u16,
    start: usize,
}

/// The characters a sprite's key may use: printable, not a space, nor
/// what would end its line or start a comment (`; { } , - /`).
fn key_char(c: u8) -> bool {
    c.is_ascii_graphic() && !matches!(c, b';' | b'{' | b'}' | b',' | b'-' | b'/')
}

/// The draw stage as written, in order: loops (their counter node and
/// count) around lines and dots (their values: x0, y0, x1, y1, v0, v1; x,
/// y, v).
#[derive(Clone, Copy)]
enum DItem {
    For(Id, u16),
    End,
    Line([Id; 6]),
    Dot([Id; 3]),
    /// A paint primitive (`program::paint`): its values, how many.
    Paint(u8, [Id; 9], u8),
    If(Id),
    Else,
    EndIf,
}

/// Where the parser is: the formula's loop, and the channel being read.
#[derive(Clone, Copy)]
struct Env {
    loop_v: i32,
    /// In a colour expression, the channel being read (0 red, 1 green, 2
    /// blue: `#f80` is that channel's value, `rgb(r, g, b)` that value);
    /// `NO_CH` elsewhere.
    ch: u8,
}

const NO_CH: u8 = 3;
/// Inside `rgb(r, g, b)`'s values (for the error a colour there gets).
const IN_RGB: u8 = 4;

/// What a name is being set as ([`Parser::new_name`]): a value (set
/// again, or shadowing one from outside, as often as you like), a loop
/// counter (may shadow a value), a control, or a part, table, sprite or
/// palette (a name of their own).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Naming {
    Value,
    Counter,
    Control,
    Thing,
}

struct Parser<'s> {
    src: &'s str,
    toks: Vec<Packed>,
    pos: usize,
    dag: Dag,
    locals: Vec<Local<'s>>,
    depth: usize,
    /// The scope being read, and every scope's parent and whether it is a
    /// part's own (which sees nothing outside it). 0 is the formula's; a
    /// layer's sees the names around it.
    scope: u16,
    scopes: Vec<(u16, bool)>,
    env: Env,
    work: usize,
    /// `table` entries (Q16.16; the same run of numbers is stored once) and
    /// the tables' names.
    tables: Vec<i32>,
    table_names: Vec<Table<'s>>,
    sprites: Vec<Sprite<'s>>,
    /// The formula's own palettes (32 entries each, 0x00RRGGBB; the same
    /// colours stored once; tweens), their names, and the tweens' nodes.
    palettes: Vec<OwnPal>,
    palette_names: Vec<Palette<'s>>,
    pnodes: Vec<PNode>,
    /// The draw stage: its items, the `draw` that wrote them, whether the
    /// lines being read are recorded (not for a part read again at another
    /// place: its draw stage is the one read where it is written), how deep
    /// inside `draw { }` the parser is (0 outside, 1 its own lines, 2 and 3
    /// in loops), the loops' counts around it, loops numbered so far, and
    /// loop turns a frame.
    draw: Vec<DItem>,
    /// The formula's functions, and the ones being read (a function can't
    /// call itself).
    funcs: Vec<Func<'s>>,
    calling: Vec<usize>,
    /// The paint palette (0x00RRGGBB; entry 0 none) and where a palette's
    /// 32 entries start in it (palette id, start).
    paint: Vec<u32>,
    paint_pals: Vec<(u8, u8)>,
    drawn: Option<Token>,
    record: bool,
    in_draw: u8,
    counts: [u32; 2],
    loops: u16,
    turns: u32,
    /// The declarations read so far: boxed (the parser's own frame is on
    /// the compiler's deepest path, and the firmware holds that stack),
    /// and only once there is one.
    decl: Option<alloc::boxed::Box<Decls>>,
    /// The language service's: the names read (where, and the definition's
    /// place), the declarations (the word's place, the line's end), and
    /// whom to tell ([`crate::syntax`]).
    #[cfg(feature = "syntax")]
    reads: Vec<(usize, usize, usize)>,
    #[cfg(feature = "syntax")]
    decls_seen: Vec<(usize, usize)>,
    #[cfg(feature = "syntax")]
    sink: Option<&'s mut (dyn crate::syntax::Sink + 's)>,
    /// The language service's notes (`Sink::note`: place, text) and each
    /// recorded draw loop's place (`for` to its count).
    #[cfg(feature = "syntax")]
    notes: Vec<(usize, usize, String, Id)>,
    #[cfg(feature = "syntax")]
    loop_at: Vec<(usize, usize)>,
}

/// What the declarations said, while compiling: the program's [`Meta`],
/// the value each declared slot reads as (the input clamped to its range,
/// `NO` for an undeclared slot), how many controls the formula's own lines
/// declared, and where the text is ([`Declared`]).
#[derive(Default)]
struct Decls {
    meta: Meta,
    slots: [Id; 3],
    own: u8,
    text: Declared,
}

/// Where a formula's declarations are in its source (byte ranges), for
/// the web module's `controls()`: the browser shows a name, a label and a
/// choice's words as written, read back from the source rather than
/// stored in the [`Program`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Declared {
    /// `name "…"`'s and `about "…"`'s text (inside the quotes).
    pub name: Option<(u32, u32)>,
    pub about: Option<(u32, u32)>,
    /// The `look` line (from `look` to the line's end, its comment out).
    pub look: Option<(u32, u32)>,
    /// Each slot's control: its name, its label (inside the quotes) and,
    /// for a choice, the words (`gentle, strong, off`, as written).
    pub controls: [Option<DeclaredControl>; 3],
    /// How many of them are the formula's own `control` lines (slots a, b,
    /// c in order); the rest are its parts'.
    pub own: u8,
}

/// A `control` line's pieces ([`Declared`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeclaredControl {
    pub name: (u32, u32),
    pub label: Option<(u32, u32)>,
    pub words: Option<(u32, u32)>,
}

/// A colour: an `OUT_*` code, its arguments, the palette, how much of the
/// pixel it covers (`None`: all of it) and how it goes onto what is below.
#[derive(Clone, Copy)]
struct Colour {
    code: u8,
    args: [Id; 3],
    pal: u8,
    alpha: Option<Id>,
    blend: u8,
    /// `pal(P, v) * k`: the palette's colour times k (each channel).
    scale: Option<Id>,
}

/// A colour's palette flag: `cycle(P, v, s)`, its turn (a `code::TURN`
/// node) in `args[1]` (`pal` is P | this until `finish` gives the turned
/// palette its slot). In the colour itself, so a colour stays its size
/// (parts hold many: the compile's heap).
const TURNED: u8 = 0x40;

/// What a formula's or a part's lines come to: its colours, bottom to top
/// (the last line's, or its layers'), the line that made the top one,
/// whether they are layers, `layer` blocks written, and its `loop`.
struct Body {
    colours: Vec<Colour>,
    top: Option<Token>,
    layered: bool,
    blocks: usize,
    loop_v: Option<i32>,
    /// `fps N`: the frame time it asks for, ms (10, 20 or 40).
    fps: Option<u8>,
    /// `layer rate N { }`: each such layer's colour (its index) and N.
    rates: Vec<(usize, u8)>,
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
    /// Token `k`, or the closing `Eof` past the end (a look ahead that runs
    /// off the input never panics).
    #[inline(never)]
    fn tok(&self, k: usize) -> Token {
        self.toks
            .get(k)
            .copied()
            .unwrap_or(self.toks[self.toks.len() - 1])
            .get()
    }

    #[inline(never)]
    fn peek(&self) -> Token {
        self.toks[self.pos].get()
    }

    #[inline(never)]
    fn next(&mut self) -> Token {
        let t = self.toks[self.pos].get();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn text(&self, t: Token) -> &'s str {
        sub(self.src, t.start, t.end)
    }

    #[inline(never)]
    fn line_of(&self, at: usize) -> usize {
        sub(self.src, 0, at).matches('\n').count() + 1
    }

    #[cold]
    #[inline(never)]
    fn err<T>(&self, t: Token, msg: impl Into<String>) -> Result<T, Error> {
        Err(Error::new(t.start, t.end, msg))
    }

    // The errors are built out of line (`#[cold]`, never inlined), so the
    // recursive parser's frames carry no `format!` temporaries: the
    // compiler's stack depth is what the firmware's main task must hold.

    #[cold]
    #[inline(never)]
    fn fail(&self, t: Token, msg: &str) -> Error {
        Error::new(t.start, t.end, msg)
    }

    #[cold]
    #[inline(never)]
    fn fail_at(&self, t: Token, msg: String) -> Error {
        Error::new(t.start, t.end, msg)
    }

    #[inline(never)]
    fn expect(&mut self, want: Tok, what: &str) -> Result<Token, Error> {
        let t = self.peek();
        if t.tok == want {
            return Ok(self.next());
        }
        Err(self.expected(t, what))
    }

    #[cold]
    #[inline(never)]
    fn expected(&self, t: Token, what: &str) -> Error {
        self.fail_at(t, format!("expected {what} here, found {}", self.found(t)))
    }

    /// `close` where the `)` of `open` should be: never closed (the newline
    /// was swallowed), or something else in the way.
    #[cold]
    #[inline(never)]
    fn not_closed(&self, open: Token, close: Token, name: &str) -> Error {
        if let Err(e) = self.unclosed(open, close) {
            return e;
        }
        if name.is_empty() {
            return self.expected(close, "`)`");
        }
        self.fail_at(
            close,
            format!(
                "expected `,` or `)` in `{name}(…)`, found {}",
                self.found(close)
            ),
        )
    }

    fn found(&self, t: Token) -> String {
        match t.tok {
            Tok::Newline | Tok::Eof => describe(t.tok).to_string(),
            _ => format!("`{}`", self.text(t)),
        }
    }

    /// A `(` whose `)` was expected at `found` on a later line: the
    /// newline was swallowed, so say which bracket is open.
    #[inline(never)]
    fn unclosed(&self, open: Token, found: Token) -> Result<(), Error> {
        if found.tok == Tok::Eof || self.line_of(found.start) > self.line_of(open.start) {
            return self.err(open, "this `(` is never closed — add `)`");
        }
        Ok(())
    }

    #[inline(never)]
    fn skip_newlines(&mut self) {
        while self.peek().tok == Tok::Newline {
            self.next();
        }
    }

    /// An expression (Pratt): operators binding tighter than `min_bp`.
    #[inline(never)]
    fn expr(&mut self, min_bp: u8) -> Result<Id, Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.too_deep());
        }
        let r = self.expr_inner(min_bp);
        self.depth -= 1;
        r
    }

    #[cold]
    #[inline(never)]
    fn too_deep(&self) -> Error {
        let t = self.peek();
        self.fail(t, "this is nested too deeply — split it into named steps")
    }

    #[cold]
    #[inline(never)]
    fn no_value(&self, t: Token) -> Error {
        match t.tok {
            Tok::Newline | Tok::Eof => self.fail(t, "a value is missing here"),
            _ => self.fail_at(t, format!("expected a value, found {}", self.found(t))),
        }
    }

    fn expr_inner(&mut self, min_bp: u8) -> Result<Id, Error> {
        // After an operator, the expression may continue on the next line.
        self.skip_newlines();
        let t = self.next();
        let mut lhs =
            match t.tok {
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
                        return Err(self.not_closed(t, close, ""));
                    }
                    self.next();
                    e
                }
                // `pick(…)` here, not in `name`: there it made every level of
                // nesting's frame bigger (a part called `pick` is still the part).
                Tok::Name if self.peek().tok == Tok::LParen && self.text(t) == "pick" => {
                    self.pick(t)?
                }
                Tok::Name if self.env.ch < NO_CH && self.text(t) == "under" && self.lookup_local("under").is_none() && self.func_named("under").is_none() => {
                    self.under_atom(t)?
                }
                Tok::Name => self.name(t)?,
                Tok::Hex(v) if self.env.ch < NO_CH => self.dag.constant(
                    fixed::byte((v >> (16 - 8 * self.env.ch as u32)) as u8),
                    (t.start, t.end),
                )?,
                Tok::Hex(_) => return self.err(
                    t,
                    if self.env.ch == IN_RGB {
                        "`rgb(r, g, b)` takes three numbers, not colours: write a colour alone, as `#f80 * k`"
                    } else {
                        "`#…` is a colour, not a number: use it on the last line, as `#f80 * k` or `mix(#000, #f80, k)`"
                    },
                ),
                _ => return Err(self.no_value(t)),
            };
        // (The language service's: the operator before and its right-hand
        // side's place, for the split note.)
        #[cfg(feature = "syntax")]
        let mut prev: Option<(Token, (usize, usize), Id, Id)> = None;
        loop {
            let op = self.peek();
            if op.tok == Tok::Question {
                #[cfg(feature = "syntax")]
                {
                    prev = None;
                }
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
            #[cfg(feature = "syntax")]
            let rs = self.peek().start;
            let rhs = self.expr(rbp)?;
            #[cfg(feature = "syntax")]
            let span = (rs, self.toks[self.pos - 1].get().end);
            #[cfg(feature = "syntax")]
            let note = prev
                .take()
                .and_then(|p| self.split_note(t.start, p, (op, span, lhs, rhs)));
            #[cfg(feature = "syntax")]
            let before = lhs;
            lhs = self.dag.op(c, [lhs, rhs, 0], (op.start, op.end))?;
            #[cfg(feature = "syntax")]
            {
                if let Some((a, b, text)) = note {
                    self.notes.push((a, b, text, lhs));
                }
                prev = Some((op, span, before, rhs));
            }
        }
        Ok(lhs)
    }

    /// Whether names set in `scope` can be seen here: this scope's and its
    /// parents'.
    fn visible(&self, scope: u16) -> bool {
        let mut s = self.scope;
        loop {
            if s == scope {
                return true;
            }
            let (parent, own) = self.scopes[s as usize];
            if own || s == 0 {
                return false;
            }
            s = parent;
        }
    }

    /// The candidates for a misspelt value name.
    fn value_names(&self) -> impl Iterator<Item = &str> + use<'_, 's> {
        INPUTS
            .iter()
            .map(|e| e.0)
            .chain(CONSTS.iter().map(|e| e.0))
            .chain(LATER)
            .chain(
                self.locals
                    .iter()
                    .filter(|l| self.visible(l.scope))
                    .map(|l| l.name),
            )
    }

    /// [`Self::name`] as a call of its own, for `pick`'s rare part (inlined
    /// there it made `pick`'s frame, on the recursive path too, bigger).
    #[inline(never)]
    fn name_out_of_line(&mut self, t: Token) -> Result<Id, Error> {
        self.name(t)
    }

    /// A name where a value goes: a part's value, a special form, a table,
    /// a function call or a value. Inlined into `expr` (its one caller on
    /// the recursive path): a frame of its own between `expr` and `args` was
    /// ~100 B of the compile stack per level of nesting, ×[`MAX_DEPTH`].
    #[inline(always)]
    fn name(&mut self, t: Token) -> Result<Id, Error> {
        let name = self.text(t);
        let at = (t.start, t.end);
        #[cfg(feature = "syntax")]
        self.note_read(t);
        if let Some(r) = self.special(t, name) {
            return r;
        }
        if self.reads_table(name) {
            return self.table_read(t);
        }
        if self.peek().tok == Tok::LParen {
            if let Some(k) = self.func_named(name) {
                return self.call_value(t, k);
            }
            if name == "rgb" && self.env.ch < NO_CH {
                return self.rgb_channel();
            }
            if let Some(&(_, ar, f)) = FUNCS.iter().find(|e| e.0 == name) {
                if let F::Glyph(c) = f {
                    let (args, font) = self.glyph_args(t, name)?;
                    let c = match font {
                        tables::NORMAL => c,
                        tables::SMALL => c + 2,
                        _ => code::DIGIT_B,
                    };
                    return self.dag.op(c, args, at);
                }
                let args = self.args(t, name, ar)?;
                return match f {
                    F::Op(c) | F::Glyph(c) => self.dag.op(c, args, at),
                    // (`pick` is read in `expr_inner`.)
                    F::Fract | F::Pick => self.dag.op(code::FRAC, args, at),
                    F::Smoothstep => {
                        // smoothstep(e0, e1, x) = smooth(x, e0, 1/(e1-e0))
                        let span = self.dag.op(code::SUB, [args[1], args[0], 0], at)?;
                        let inv = self.dag.op(code::RECIP, [span, 0, 0], at)?;
                        self.dag.op(code::SMOOTH, [args[2], args[0], inv], at)
                    }
                };
            }
        } else if name == "seed" && self.seed_refused() {
            // E903.
            return Err(self.fail(
                t,
                "`seed` is for formulas that go on without repeating (`loop forever`): a new one each time it plays",
            ));
        } else if let Some(id) = self.lookup_value(name) {
            return id;
        }
        Err(self.unknown(t))
    }

    /// E903: `seed` in a formula that loops, unless the formula set its
    /// own (out of line: the recursive parser's frames stay small).
    #[inline(never)]
    fn seed_refused(&self) -> bool {
        !self.dag.forever
            && !self
                .locals
                .iter()
                .any(|l| l.name == "seed" && self.visible(l.scope))
    }

    /// Why the name at `t` is not a value here (or, before `(`, a function).
    #[cold]
    #[inline(never)]
    fn unknown(&mut self, t: Token) -> Error {
        let name = self.text(t);
        let msg = if self.peek().tok == Tok::LParen {
            if self.env.ch < NO_CH && COLOURS.iter().any(|e| e.0 == name) {
                format!(
                    "`{name}(…)` is a whole colour: it can't be added to others (only `#…` and `rgb(r, g, b)` can)"
                )
            } else if COLOURS.iter().any(|e| e.0 == name) {
                format!(
                    "`{name}(…)` is a whole colour: it can only be the last line (of the formula or a layer)"
                )
            } else if PRIMITIVES.contains(&name) {
                format!("`{name}(…)` draws: it goes on a line of its own inside `draw {{ }}`")
            } else if self.lookup_value(name).is_some() {
                format!("`{name}` is a value, not a function")
            } else if let Some(&(_, hint)) = HABITS.iter().find(|e| e.0 == name) {
                format!("unknown function `{name}` — {hint}")
            } else {
                let s = suggest(
                    name,
                    FUNCS.iter().map(|e| e.0).chain(COLOURS.iter().map(|e| e.0)),
                );
                format!("unknown function `{name}`{}", did_you_mean(s))
            }
        } else if FUNCS.iter().any(|e| e.0 == name) || COLOURS.iter().any(|e| e.0 == name) {
            format!("`{name}` is a function: write `{name}(…)`")
        } else if KEYWORDS.contains(&name) {
            format!("`{name}` can only start a line")
        } else if PALETTES.iter().any(|e| e.0 == name) {
            format!("`{name}` is a palette: use it as `pal({name}, k)` on the last line")
        } else if FONTS.iter().any(|e| e.0 == name) {
            format!("`{name}` is a font: use it as `digit(n, x, y, {name})`")
        } else if let Some(&(_, hint)) = HABITS.iter().find(|e| e.0 == name) {
            format!("unknown name `{name}` — {hint}")
        } else {
            let s = suggest(name, self.value_names());
            format!("unknown name `{name}`{}", did_you_mean(s))
        };
        self.fail_at(t, msg)
    }

    /// The special forms read where a name is: `ink(n)` (unless the
    /// formula set `ink`) and a palette's `NAME.colour`. Out of line: `name`
    /// is on the parser's deepest path (the firmware's compile stack).
    #[inline(never)]
    fn special(&mut self, t: Token, name: &str) -> Option<Result<Id, Error>> {
        match self.peek().tok {
            Tok::LParen if name == "ink" && self.lookup_local(name).is_none() => {
                Some(self.ink_levels(t))
            }
            Tok::Dot => {
                let k = self.palette_named(name)?;
                Some(self.palette_entry(t, k))
            }
            _ => None,
        }
    }

    /// `ink(n)` at `t` (`ink`): the canvas in `n` levels, 1..=255, a
    /// whole number (`INKN`, never folded: it reads the canvas).
    #[inline(never)]
    fn ink_levels(&mut self, t: Token) -> Result<Id, Error> {
        // E214.
        const HOW: &str =
            "`ink(n)` takes a whole number from 1 to 255: `ink(16)` is the canvas in 16 levels";
        self.next();
        let num = self.next();
        let close = self.next();
        let n = match (num.tok, close.tok) {
            (Tok::Num(v), Tok::RParen) if v & 0xffff == 0 && (ONE..=255 * ONE).contains(&v) => v,
            _ => return self.err(num, HOW),
        };
        let at = (t.start, close.end);
        let c = self.dag.constant(n, at)?;
        let why = INPUTS.iter().position(|e| e.1 == Input::Ink).unwrap_or(0) as u8;
        self.dag.push(
            Node {
                code: code::INKN,
                a: c,
                b: 0,
                c: 0,
                val: 0,
                level: LV_PIXEL,
                why,
            },
            at,
        )
    }

    /// The formula's own value `name` seen here (the latest set).
    fn lookup_local(&self, name: &str) -> Option<&Local<'s>> {
        self.locals
            .iter()
            .rev()
            .find(|l| l.name == name && self.visible(l.scope))
    }

    #[inline(never)]
    fn lookup_value(&mut self, name: &str) -> Option<Result<Id, Error>> {
        // (The latest: a name may be set again, or shadow one from outside.)
        if let Some(l) = self.lookup_local(name) {
            return Some(Ok(l.id));
        }
        if let Some(&(_, inp, _)) = INPUTS.iter().find(|e| e.0 == name) {
            return Some(self.input(inp));
        }
        if let Some(&(_, v)) = CONSTS.iter().find(|e| e.0 == name) {
            return Some(self.dag.constant(v, (0, 0)));
        }
        if name == "seed" {
            return Some(self.input(Input::Seed));
        }
        let k = LATER.iter().position(|&e| e == name)?;
        Some(self.centred(k))
    }

    /// [`LATER`]'s value `k` (unused steps cost nothing: they are dropped).
    #[inline(never)]
    fn centred(&mut self, k: usize) -> Result<Id, Error> {
        let mut v = [0 as Id; 12];
        for (i, inp) in [Input::X, Input::Y, Input::Py].into_iter().enumerate() {
            v[i] = self.input(inp)?;
        }
        for (i, c) in [ONE / 2, 4 * ONE, 31 * ONE].into_iter().enumerate() {
            v[3 + i] = self.dag.constant(c, (0, 0))?;
        }
        for (i, &(c, a, b)) in CENTRED.iter().enumerate() {
            v[6 + i] = self.dag.op(c, [v[a as usize], v[b as usize], 0], (0, 0))?;
        }
        // cx, cy, r, angle (step 9, `ty`, is no name: rows count down).
        Ok(v[7 + [0, 1, 3, 4][k]])
    }

    /// A built-in value here. Where the formula has set `x` or `y` (or a
    /// function names its values `x, y`), what comes from them follows:
    /// `px = floor(x·127 + ½)`, `py = floor(y·31 + ½)`, `i = py·128 + px`,
    /// as on the panel (`x = px/127`), and `cx cy r angle`.
    #[inline(never)]
    fn input(&mut self, inp: Input) -> Result<Id, Error> {
        if matches!(inp, Input::X | Input::Y | Input::Px | Input::Py | Input::I) {
            let own = |p: &Self, n: &str| {
                p.lookup_local(n)
                    .filter(|l| !l.control && !l.counter)
                    .map(|l| l.id)
            };
            let (x, y) = (own(self, "x"), own(self, "y"));
            if x.is_some() || y.is_some() {
                return self.moved(inp, x, y);
            }
        }
        let at = (0, 0);
        // A declared control's slot reads as the value its line made (the
        // input held to its range).
        let k = match inp {
            Input::A => 0,
            Input::B => 1,
            Input::C => 2,
            _ => 3,
        };
        if let Some(d) = self.decl.as_ref().filter(|d| k < 3 && d.slots[k] != NO) {
            return Ok(d.slots[k]);
        }
        self.dag.input(inp, at)
    }

    /// [`input`](Self::input)'s `x y px py i` from the formula's own `x`
    /// or `y` (the built-in where it set only the other).
    #[inline(never)]
    fn moved(&mut self, inp: Input, x: Option<Id>, y: Option<Id>) -> Result<Id, Error> {
        let at = (0, 0);
        let x = match x {
            Some(x) => x,
            None => self.dag.input(Input::X, at)?,
        };
        let y = match y {
            Some(y) => y,
            None => self.dag.input(Input::Y, at)?,
        };
        let grid = |p: &mut Self, v: Id, n: i32| -> Result<Id, Error> {
            let k = p.dag.constant(n << 16, at)?;
            let half = p.dag.constant(ONE / 2, at)?;
            let m = p.dag.op(code::MUL, [v, k, 0], at)?;
            let r = p.dag.op(code::ADD, [m, half, 0], at)?;
            p.dag.op(code::FLOOR, [r, 0, 0], at)
        };
        match inp {
            Input::X => Ok(x),
            Input::Y => Ok(y),
            Input::Px => grid(self, x, W as i32 - 1),
            Input::Py => grid(self, y, H as i32 - 1),
            _ => {
                let px = grid(self, x, W as i32 - 1)?;
                let py = grid(self, y, H as i32 - 1)?;
                let w = self.dag.constant((W as i32) << 16, at)?;
                let r = self.dag.op(code::MUL, [py, w, 0], at)?;
                self.dag.op(code::ADD, [r, px, 0], at)
            }
        }
    }

    /// `( a, b, … )` with exactly `n` values. Always inlined: as a frame of
    /// its own (64 B on the ESP32) it sat in `expr`'s recursion.
    #[inline(always)]
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
        // `expect` by hand: its `Result<Token, Error>` was a slot in this
        // frame, on the recursive path (as in `glyph_args`).
        let open = self.peek();
        if open.tok != Tok::LParen {
            return Err(self.expected(open, "`(`"));
        }
        self.next();
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
            return Err(self.not_closed(open, close, name));
        }
        self.next();
        if k != n && !(cover && k == n + 1) {
            return Err(self.arity(f, close, name, n, k, cover));
        }
        Ok((out, k))
    }

    /// `pick(k, a, b, …)` at `t` (out of line: the recursive parser's
    /// frames stay small).
    #[inline(never)]
    fn pick(&mut self, t: Token) -> Result<Id, Error> {
        if self.func_named("pick").is_some() {
            return self.name_out_of_line(t);
        }
        let open = self.next();
        let k = self.expr(0)?;
        let mut v = Vec::new();
        #[cfg(feature = "syntax")]
        let mut spans = Vec::new();
        while self.peek().tok == Tok::Comma {
            self.next();
            #[cfg(feature = "syntax")]
            let st = self.peek().start;
            v.push(self.expr(0)?);
            #[cfg(feature = "syntax")]
            spans.push((st, self.toks[self.pos - 1].get().end));
        }
        let close = self.peek();
        if close.tok != Tok::RParen {
            return Err(self.not_closed(open, close, "pick"));
        }
        self.next();
        if v.is_empty() {
            return self.err(
                t,
                "`pick` takes a number, then the values to pick from: `pick(k, a, b, …)`",
            );
        }
        #[cfg(feature = "syntax")]
        let note = self.pick_note(t, k, &v, &spans);
        let id = self.dag.pick(k, v, (t.start, t.end))?;
        #[cfg(feature = "syntax")]
        if let Some((a, b, text)) = note {
            self.notes.push((a, b, text, id));
        }
        Ok(id)
    }

    /// `digit(n, x, y)` / `glyph(c, x, y)`, with a font (`, small`, …)
    /// last if you like: the three values and the font.
    #[inline(never)]
    fn glyph_args(&mut self, f: Token, name: &str) -> Result<([Id; 3], u8), Error> {
        let open = self.peek();
        if open.tok != Tok::LParen {
            return Err(self.expected(open, "`(`"));
        }
        self.next();
        let mut out = [0 as Id; 3];
        let (mut k, mut font_of) = (0, tables::NORMAL);
        if self.peek().tok != Tok::RParen {
            loop {
                if k == 3 {
                    let p = self.peek();
                    let font = FONTS
                        .iter()
                        .find(|e| p.tok == Tok::Name && e.0 == self.text(p));
                    let Some(&(_, n)) = font else {
                        return Err(self.no_font(p, name));
                    };
                    if n == tables::BOLD && name != "digit" {
                        return self.err(
                            p,
                            "the bold font has digits only: use `digit(n, x, y, bold)`",
                        );
                    }
                    self.next();
                    font_of = n;
                } else {
                    let e = self.expr(0)?;
                    if let Some(o) = out.get_mut(k) {
                        *o = e;
                    }
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
            return Err(self.not_closed(open, close, name));
        }
        self.next();
        if k != 3 && k != 4 {
            return Err(self.glyph_arity(f, close, name, k));
        }
        Ok((out, font_of))
    }

    #[cold]
    #[inline(never)]
    fn glyph_arity(&self, f: Token, close: Token, name: &str, k: usize) -> Error {
        let what = if name == "digit" {
            "the digit"
        } else {
            "the character"
        };
        Error::new(
            f.start,
            close.end,
            format!(
                "`{name}` takes 3 values ({what}, a column and a row; and a font, normal, small or bold, if you like), but got {k}"
            ),
        )
    }

    #[cold]
    #[inline(never)]
    fn no_font(&self, t: Token, name: &str) -> Error {
        self.fail_at(
            t,
            format!(
                "the last value of `{name}` is a font: normal (the default), small or bold, found {}",
                self.found(t)
            ),
        )
    }

    /// `name(…)` from `f` to `close` given `k` values for `n` (`cover`: or
    /// `n + 1`).
    #[cold]
    #[inline(never)]
    fn arity(&self, f: Token, close: Token, name: &str, n: usize, k: usize, cover: bool) -> Error {
        let s = if n == 1 { "" } else { "s" };
        let what = "";
        let msg = if cover {
            format!(
                "`{name}` takes {n} value{s}{what} (or {}: the last is how much of the pixel it covers, 0 to 1), but got {k}",
                n + 1
            )
        } else {
            format!("`{name}` takes {n} value{s}{what}, but got {k}")
        };
        Error::new(f.start, close.end, msg)
    }

    /// The colour call at `t` (its name): `rgb(r, g, b)`, `hsv(h, s, v)`,
    /// `pal(name, k)`, `gray(v)`.
    #[inline(never)]
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    fn colour(&mut self, t: Token) -> Result<Colour, Error> {
        let name = self.text(t);
        let &(_, ar, c) = COLOURS.iter().find(|e| e.0 == name).unwrap_or(&COLOURS[0]);
        // `rgb(#f80)`, `rgb(mix(…), k)`: a colour, and maybe its coverage.
        if c == code::OUT_RGB && self.has_colour(self.pos + 1, true) {
            let open = self.next();
            let mut col = self.colour_line()?;
            if self.peek().tok == Tok::Comma {
                self.next();
                col.alpha = Some(self.expr(0)?);
            }
            let close = self.peek();
            if close.tok != Tok::RParen {
                self.unclosed(open, close)?;
                return self.err(
                    close,
                    "`rgb` of a colour takes the colour and, if you like, how much of the pixel it covers (0 to 1)",
                );
            }
            self.next();
            return Ok(col);
        }
        if c != code::OUT_PAL {
            if c == code::OUT_RGB {
                self.env.ch = IN_RGB;
            }
            let r = self.args_or_cover(t, name, ar, true);
            self.env.ch = NO_CH;
            let (a, k) = r?;
            return Ok(Colour {
                code: c,
                args: [a[0], a[1], a[2]],
                pal: 0,
                alpha: (k > ar).then_some(a[ar]),
                blend: fixed::BLEND_OVER,
                scale: None,
            });
        }
        let cycle = name == "cycle";
        self.expect(Tok::LParen, "`(`")?;
        let p = self.peek();
        if p.tok != Tok::Name {
            return self.err(
                p,
                if cycle {
                    "the first value of `cycle` is a palette: fire, ocean, ice, chrono, amber, rainbow or one of yours"
                } else {
                    "the first value of `pal` is a palette: fire, ocean, ice, chrono, amber, rainbow or gray"
                },
            );
        }
        self.next();
        let pname = self.text(p);
        let own = self.palette_named(pname).map(|k| self.palette_names[k].j);
        let own = own.map(|j| 5 + self.live_of(j));
        let Some(pal) = PALETTES.iter().find(|e| e.0 == pname).map(|e| e.1).or(own) else {
            return Err(self.unknown_palette(p));
        };
        if cycle && pal == 255 {
            // E932.
            return self.err(
                p,
                "`cycle` turns a palette of colours, and gray is not one: write your own, `palette g = #000, #fff`",
            );
        }
        self.expect(
            Tok::Comma,
            if cycle {
                "`,` (as in `cycle(fire, v, t/4)`)"
            } else {
                "`,` (as in `pal(fire, k)`)"
            },
        )?;
        let mut k = self.expr(0)?;
        let mut turn = None;
        if cycle {
            self.expect(Tok::Comma, "`,` and the turn (as in `cycle(fire, v, t/4)`)")?;
            let st = self.peek();
            let s = self.expr(0)?;
            let at = (
                st.start,
                self.toks[self.pos.saturating_sub(1)].get().end.max(st.end),
            );
            if self.dag.level(s) == LV_PIXEL {
                // E931.
                return Err(self.varies_err(at, CYCLE_ROW, s));
            }
            // The entry is (floor(v·32) + floor(s·32)) mod 32: the fraction
            // of each is all it reads (a value that grows without end too).
            let s = if self.dag.grows(s) {
                self.dag.op(code::FRAC, [s, 0, 0], at)?
            } else {
                s
            };
            k = self.dag.op(code::FRAC, [k, 0, 0], at)?;
            turn = Some(self.dag.push(
                Node {
                    code: code::TURN,
                    a: s,
                    b: 0,
                    c: 0,
                    val: i32::from(pal),
                    level: LV_ROW,
                    why: 0,
                },
                at,
            )?);
        }
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
                    "`{name}` takes a palette{} (and how much of the pixel it covers, 0 to 1, if you like); found {}",
                    if cycle { ", the picture and the turn" } else { " and one value" },
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
            args: [k, turn.unwrap_or(0), 0],
            pal: if turn.is_some() { pal | TURNED } else { pal },
            alpha,
            blend: fixed::BLEND_OVER,
            scale: None,
        })
    }

    /// Whether there is a colour (`#f80` or `rgb(`) from token `k` to the
    /// end of the line, or with `arg`, of the value (a `,` or `)` that
    /// closes it).
    fn has_colour(&self, mut k: usize, arg: bool) -> bool {
        let mut depth = 0;
        loop {
            let t = self.toks[k].get();
            match t.tok {
                Tok::Hex(_) => return true,
                Tok::Name if self.toks[k + 1].get().tok == Tok::LParen && self.text(t) == "rgb" => {
                    return true
                }
                Tok::Name if self.text(t) == "under" && self.lookup_local("under").is_none() => {
                    return true
                }
                Tok::Name
                    if self.toks[k + 1].get().tok == Tok::LParen
                        && self
                            .func_named(self.text(t))
                            .is_some_and(|f| self.funcs[f].colour == F_CHANNELS) =>
                {
                    return true
                }
                Tok::LParen => depth += 1,
                Tok::RParen if depth > 0 => depth -= 1,
                Tok::Comma if depth > 0 || !arg => {}
                Tok::Comma | Tok::RParen | Tok::Newline | Tok::Eof | Tok::RBrace => return false,
                _ => {}
            }
            k += 1;
        }
    }

    /// `under` (the picture under the formula at this pixel) or `under(x,
    /// y)` (at pixel x, y), in a colour expression: the channel read now.
    #[inline(never)]
    fn under_atom(&mut self, t: Token) -> Result<Id, Error> {
        let inp = [Input::Ur, Input::Ug, Input::Ub][(self.env.ch as usize).min(2)];
        if self.peek().tok != Tok::LParen {
            return self.input(inp);
        }
        let open = self.next();
        let x = self.expr(0)?;
        self.expect(Tok::Comma, "`,` (as in `under(px + 1, py)`)")?;
        let y = self.expr(0)?;
        let close = self.peek();
        if close.tok != Tok::RParen {
            return Err(self.not_closed(open, close, "under"));
        }
        self.next();
        let (mut level, mut why) = (LV_PIXEL, 0);
        for v in [x, y] {
            let m = &self.dag.nodes[v as usize];
            if m.level > level {
                (level, why) = (m.level, m.why);
            }
        }
        let _ = level;
        self.dag.push(
            Node {
                code: code::UAT,
                a: x,
                b: y,
                c: self.env.ch as Id,
                val: 0,
                level: LV_PIXEL,
                why,
            },
            (t.start, close.end),
        )
    }

    /// A colour expression at the cursor, read once for each channel: its
    /// red, green and blue (`#f80 * k` is `rgb(k, k*0.53, 0)`).
    fn channels(&mut self) -> Result<[Id; 3], Error> {
        let start = self.pos;
        let mut out = [0; 3];
        for (ch, o) in out.iter_mut().enumerate() {
            self.pos = start;
            self.env.ch = ch as u8;
            let r = self.expr(0);
            self.env.ch = NO_CH;
            *o = r?;
        }
        Ok(out)
    }

    /// `rgb(r, g, b)` (the cursor at its `(`) inside a colour expression: the channel's
    /// value (its values are plain values).
    #[inline(never)]
    fn rgb_channel(&mut self) -> Result<Id, Error> {
        let ch = self.env.ch;
        let open = self.next();
        // `rgb(#f80)` inside: the colour; how much it covers is the line's.
        if self.has_colour(self.pos, true) {
            let v = self.expr(0)?;
            if self.peek().tok == Tok::Comma {
                return self.err(
                    self.peek(),
                    "how much a colour covers goes on the whole line: `rgb(mix(…), k)`, not on an `rgb(…)` inside it",
                );
            }
            self.expect(Tok::RParen, "`)`")?;
            return Ok(v);
        }
        // Only this channel's value is read (the others' in their own
        // reading: no values made early, held in registers until then).
        let mut v = 0;
        for i in 0..3u8 {
            if i > 0 {
                self.expect(Tok::Comma, "`,` (`rgb(r, g, b)` takes three values)")?;
            }
            if i == ch {
                self.env.ch = IN_RGB;
                let r = self.expr(0);
                self.env.ch = ch;
                v = r?;
            } else {
                self.skip_value();
            }
        }
        let close = self.peek();
        if close.tok != Tok::RParen {
            return Err(self.not_closed(open, close, "rgb"));
        }
        self.next();
        Ok(v)
    }

    /// Past one value of a call (to its `,` or `)`).
    fn skip_value(&mut self) {
        let mut depth = 0;
        loop {
            match self.peek().tok {
                Tok::LParen => depth += 1,
                Tok::RParen | Tok::Comma if depth == 0 => return,
                Tok::RParen => depth -= 1,
                Tok::Newline | Tok::Eof | Tok::RBrace => return,
                _ => {}
            }
            self.next();
        }
    }

    /// `rgb(…)` or a colour on its own as a colour line: its channels.
    fn colour_line(&mut self) -> Result<Colour, Error> {
        Ok(Colour {
            code: code::OUT_RGB,
            args: self.channels()?,
            pal: 0,
            alpha: None,
            blend: fixed::BLEND_OVER,
            scale: None,
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

    /// A name about to be set at `t` as `kind`: not a built-in, not a
    /// loop counter, and for anything but a value or a counter (which may
    /// set a name again, or shadow one from outside: the latest is read)
    /// not a name already set here (E203).
    fn new_name(&self, t: Token, kind: Naming) -> Result<&'s str, Error> {
        let name = self.text(t);
        // A built-in value or function's name may be the formula's own (it
        // then reads as the formula's; a call is still the function): only
        // the keywords are not names.
        if KEYWORDS.contains(&name) {
            return self.err(
                t,
                format!("`{name}` is a word of the language; pick another name"),
            );
        }
        if PALETTES.iter().any(|e| e.0 == name) {
            return self.err(
                t,
                format!("`{name}` is a palette's name; pick another name"),
            );
        }
        let local = self.lookup_local(name);
        if kind == Naming::Value && local.is_some_and(|l| l.counter) {
            return self.err(t, format!("`{name}` is a loop counter: it can't be set"));
        }
        // (A control keeps its name: set again, its slider would do nothing.)
        let control = local.filter(|l| l.control);
        let (what, at) = if let Some(k) = self.func_named(name) {
            ("function", self.funcs[k].at)
        } else if let Some(k) = self.table_named(name) {
            ("table", self.table_names[k].at)
        } else if let Some(k) = self.sprite_named(name) {
            ("sprite", self.sprites[k].at)
        } else if let Some(k) = self.palette_named(name) {
            ("palette", self.palette_names[k].at)
        } else {
            match (control, local) {
                (Some(l), _) => ("control", l.at),
                (_, Some(l)) if matches!(kind, Naming::Thing | Naming::Control) => {
                    (if l.counter { "loop counter" } else { "value" }, l.at)
                }
                _ => return Ok(name),
            }
        };
        // E203.
        self.err(
            t,
            format!(
                "`{name}` is already the name of a {what} (line {}) — pick another name",
                self.line_of(at)
            ),
        )
    }

    /// `name = expr`.
    fn assignment(&mut self, in_frame: bool) -> Result<(), Error> {
        let t = self.next();
        let name = self.new_name(t, Naming::Value)?;
        let eq = self.expect(Tok::Assign, "`=`")?;
        if matches!(self.peek().tok, Tok::Newline | Tok::Eof) {
            return self.err(eq, format!("a value is missing after `=` (`{name} = …`)"));
        }
        let start = self.peek().start;
        let id = self.expr(0)?;
        let end = self.toks[self.pos.saturating_sub(1)].get().end;
        if self.in_draw > 0 {
            self.drawable(id, start, end)?;
        }
        try_push(
            &mut self.locals,
            Local {
                name,
                id,
                at: t.start,
                in_frame,
                counter: false,
                control: false,
                scope: self.scope,
            },
        )?;
        self.end_of_statement()
    }

    /// `scene NAME when …` at the cursor (not a keyword: `scene = 3` still
    /// sets a name).
    fn is_scene(&self) -> bool {
        let t = self.peek();
        let w = self.tok(self.pos + 2);
        t.tok == Tok::Name
            && self.text(t) == "scene"
            && self.tok(self.pos + 1).tok == Tok::Name
            && w.tok == Tok::Name
            && self.text(w) == "when"
    }

    /// `scene NAME when c { name = … }`: values whose row and pixel steps
    /// run only while `c` (once a frame or once a row) is not 0; a name it
    /// sets reads 0 after it while it is off (its row and pixel values
    /// are zeroed by its guard, `code::GUARD`; any other, a constant, a
    /// frame value or one from outside, is `c ? v : 0`).
    #[inline(never)]
    fn scene_block(&mut self) -> Result<(), Error> {
        let kw = self.next();
        self.next(); // its name
        self.next(); // `when`
        let st = self.peek();
        let mut c = self.expr(0)?;
        let at = (
            st.start,
            self.toks[self.pos.saturating_sub(1)].get().end.max(st.end),
        );
        if self.dag.level(c) > LV_ROW {
            // E970.
            return Err(self.varies_err(at, SCENE_WHEN, c));
        }
        if self.dag.grows(c) {
            // (Not 0 where it is not: `min(c, 1)`.)
            let one = self.dag.constant(ONE, at)?;
            c = self.dag.op(code::MIN, [c, one, 0], at)?;
        }
        if self.dag.scenes.len() >= MAX_SCENES {
            // E972.
            return self.err(kw, format!("a formula can have {MAX_SCENES} scenes at most"));
        }
        self.expect(Tok::LBrace, "`{` (as in `scene city when c { … }`)")?;
        self.skip_newlines();
        let k = self.dag.scenes.len() as u8 + 1;
        try_push(&mut self.dag.scenes, (self.dag.nodes.len() as Id, NO, k))?;
        try_push(&mut self.dag.conds, c)?;
        self.dag.scene = k;
        let first_local = self.locals.len();
        loop {
            let t = self.peek();
            if t.tok == Tok::RBrace {
                self.next();
                break;
            }
            if !self.is_assignment() {
                self.dag.scene = 0;
                // E971.
                return self.err(
                    t,
                    "a scene holds values: `name = …` lines (its colours, drawing and palettes go outside it)",
                );
            }
            self.assignment(false)?;
            self.skip_newlines();
        }
        self.dag.scene = 0;
        if let Some(s) = self.dag.scenes.last_mut() {
            s.1 = self.dag.nodes.len() as Id;
        }
        // Its names: 0 while it is off.
        let zero = self.dag.constant(0, at)?;
        for j in first_local..self.locals.len() {
            let id = self.locals[j].id;
            if self.dag.scene_of(id as usize) != k {
                self.locals[j].id = self.dag.op(code::SELECT, [c, id, zero], at)?;
            }
        }
        self.end_of_statement()
    }

    #[inline(never)]
    fn is_assignment(&self) -> bool {
        self.peek().tok == Tok::Name && self.toks[self.pos + 1].get().tok == Tok::Assign
    }

    /// `fps N` at the cursor: the word `fps` with a number (or nothing)
    /// after it. Not a keyword: `fps = 3` still sets a name, as formulas
    /// written before it may.
    fn is_fps(&self) -> bool {
        let t = self.peek();
        t.tok == Tok::Name
            && self.text(t) == "fps"
            && matches!(
                self.toks[self.pos + 1].get().tok,
                Tok::Num(_) | Tok::Newline | Tok::RBrace | Tok::Eof
            )
    }

    /// `fps N` (E801): the frame time it asks for, ms. The panel's frame
    /// grids are 10, 20 and 40 ms.
    #[inline(never)]
    fn fps_line(&mut self) -> Result<u8, Error> {
        let kw = self.next();
        let n = self.peek();
        let ms = match n.tok {
            Tok::Num(v) if v == 25 * ONE => 40,
            Tok::Num(v) if v == 50 * ONE => 20,
            Tok::Num(v) if v == 100 * ONE => 10,
            Tok::Num(_) => return self.err(n, "`fps` is 25, 50 or 100 frames a second"),
            _ => {
                return self.err(
                    kw,
                    "`fps` takes 25, 50 or 100 frames a second, e.g. `fps 100`",
                )
            }
        };
        self.next();
        self.end_of_statement()?;
        Ok(ms)
    }

    /// The word starting the statement at the cursor, if it is a keyword
    /// used as one (`loop = 3` is an assignment, and refused as such).
    fn keyword(&self) -> &'s str {
        let t = self.peek();
        if t.tok != Tok::Name || self.toks[self.pos + 1].get().tok == Tok::Assign {
            return "";
        }
        let w = self.text(t);
        if KEYWORDS.contains(&w) {
            w
        } else {
            ""
        }
    }

    /// Inside a `layer { }` (not a formula's or a part's own lines).
    #[inline(never)]
    fn in_layer(&self) -> bool {
        self.scope != 0 && !self.scopes[self.scope as usize].1
    }

    /// Whether the statement at the cursor is a name, maybe a bracket, and
    /// nothing else (`p` or `p(x*2, y)`: a part's colour, not `p.v + 1`).
    /// (`open`: what a `(` never closed counts as.)
    fn whole_line(&self, open: bool) -> bool {
        let mut k = self.pos + 1;
        if self.toks[k].get().tok == Tok::LParen {
            let mut depth = 0;
            loop {
                match self.toks[k].get().tok {
                    Tok::LParen => depth += 1,
                    Tok::RParen => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Tok::Eof => return open,
                    _ => {}
                }
                k += 1;
            }
            k += 1;
        }
        matches!(
            self.toks[k].get().tok,
            Tok::Newline | Tok::Eof | Tok::RBrace
        )
    }

    /// A colour line at `t` (its name), a part drawn (`p`, `p(x', y')`,
    /// `p(x', y', k)`), or a bare value as the last line of the formula or
    /// a part (`end` Eof or RBrace) or of a layer (RBrace): its gray. The
    /// colours (several for a part with layers) and whether they are layers.
    fn colour_or_value(&mut self, t: Token, end: Tok) -> Result<(Vec<Colour>, bool), Error> {
        let word = if t.tok == Tok::Name { self.text(t) } else { "" };
        if let Some(k) = self.func_named(word).filter(|&f| {
            self.funcs[f].colour == F_WHOLE && self.toks[self.pos + 1].get().tok == Tok::LParen
        }) {
            return self.call_colour(t, k);
        }
        if t.tok == Tok::Name
            && COLOURS.iter().any(|e| e.0 == word)
            && self.toks[self.pos + 1].get().tok == Tok::LParen
            && (word != "rgb" || self.whole_line(true))
        {
            self.next();
            let mut c = self.colour(t)?;
            let mut e = self.peek();
            // `pal(P, v) * k`: the palette's colour, each channel times k
            // (a light on a palette, in one step); `* k * m` too.
            if (word == "pal" || word == "cycle")
                && e.tok == Tok::Star
                && !self.has_colour(self.pos + 1, false)
            {
                let at = (e.start, e.end);
                let mut k = None;
                while self.peek().tok == Tok::Star {
                    self.next();
                    let v = self.expr(10)?;
                    k = Some(match k {
                        Some(k) => self.dag.op(code::MUL, [k, v, 0], at)?,
                        None => v,
                    });
                }
                if let Some(k) = k {
                    if c.code == code::OUT_GRAY {
                        c.args[0] = self.dag.op(code::MUL, [c.args[0], k, 0], at)?;
                    } else {
                        c.scale = Some(k);
                    }
                }
                e = self.peek();
            }
            if infix_bp(e.tok).is_some() && word != "rgb" {
                return self.err(
                    e,
                        if word == "cycle" {
                        String::from(
                            "`cycle(…)` is a whole colour: it can't go into a sum, only be scaled, `cycle(fire, v, s) * k`"
                        )
                    } else if word == "pal" {
                        String::from(
                            "`pal(…)` is a whole colour: it can't go into a sum, only be scaled, `pal(fire, v) * k`"
                        )
                    } else {
                        format!(
                            "`{word}(…)` is a whole colour: it can't go into a sum (only `#…` and `rgb(r, g, b)` can)"
                        )
                    },
                );
            }
            self.end_of_statement()?;
            return Ok((alloc::vec![c], false));
        }
        // A colour on its own: `#102040`, `mix(#000, #f80, k)`, `#f80 * k`.
        if self.has_colour(self.pos, false) {
            let c = self.colour_line()?;
            self.end_of_statement()?;
            return Ok((alloc::vec![c], false));
        }
        let id = self.expr(0)?;
        self.end_of_statement()?;
        self.skip_newlines();
        if self.peek().tok != end {
            return Err(Error::new(
                t.start,
                self.toks[self.pos.saturating_sub(1)].get().start.max(t.end),
                if self.in_layer() {
                    "only a layer's last line can be a bare value (its colour) — did you mean `name = …`?"
                } else {
                    "only the last line can be a bare value (the colour) — did you mean `name = …`?"
                },
            ));
        }
        Ok((
            alloc::vec![Colour {
                code: code::OUT_GRAY,
                args: [id, 0, 0],
                pal: 0,
                alpha: None,
                blend: fixed::BLEND_OVER,
                scale: None,
            }],
            false,
        ))
    }

    /// `layer [blend] { … }` at `kw`: its own values (and `frame { }`),
    /// ending in its colour (several: a part with layers drawn whole).
    #[cfg_attr(target_arch = "xtensa", optimize(size))]
    #[inline(never)]
    fn layer(&mut self, kw: Token) -> Result<(Vec<Colour>, Token, u8), Error> {
        self.next();
        let mut blend = None;
        let b = self.peek();
        if b.tok == Tok::Name && self.text(b) != "rate" {
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
            blend = Some(m);
        }
        // `rate N`: redrawn every Nth frame, a band of rows a frame
        // (`crate::rate`).
        let mut rate = 0u8;
        let r = self.peek();
        if r.tok == Tok::Name && self.text(r) == "rate" {
            self.next();
            let n = self.next();
            rate = match n.tok {
                Tok::Num(v) if v & 0xffff == 0 && (2..=crate::rate::MAX_RATE as i32).contains(&(v >> 16)) => {
                    (v >> 16) as u8
                }
                // E942.
                _ => {
                    return self.err(
                        n,
                        format!("`rate` takes a whole number of frames, 2 to {}: `layer rate 4 {{ … }}` is redrawn every 4th frame", crate::rate::MAX_RATE),
                    )
                }
            };
        }
        let open = self.peek();
        if open.tok != Tok::LBrace
            && rate > 0
            && open.tok == Tok::Name
            && BLENDS.iter().any(|e| e.0 == self.text(open))
        {
            return self.err(open, "write the blend first: `layer add rate 4 { … }`");
        }
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
        let outer = self.scope;
        try_push(&mut self.scopes, (outer, false))?;
        self.scope = (self.scopes.len() - 1) as u16;
        let mut colour: Option<((Vec<Colour>, bool), Token)> = None;
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
                "fn" => self.fn_def(t)?,
                // E750.
                "part" => {
                    return self.err(t, "there are no parts: write a function, `fn p(x, y) { … }`, and call it, `p(x*2, y)`")
                }
                _ if self.is_fps() => {
                    return self.err(
                        t,
                        "`fps` is the whole formula's: put it on a line of its own, outside the layers",
                    )
                }
                _ if self.text(t) == "frame" && self.toks[self.pos + 1].get().tok == Tok::LBrace => return self.err(t, "there are no `frame { }` blocks: a value is worked out once a frame by itself when it reads only the time, the clock and the controls; write its lines without the block"),
                "draw" | "table" | "for" => return Err(self.misplaced(t)),
                _ if self.is_sprite() || self.is_palette() => return Err(self.misplaced(t)),
                _ if self.is_assignment() => self.assignment(false)?,
                _ => colour = Some((self.colour_or_value(t, Tok::RBrace)?, t)),
            }
            self.skip_newlines();
        }
        let close = self.next();
        self.scope = outer;
        let Some(((mut cs, _), ct)) = colour else {
            // (At its `}`: where the colour line goes.)
            return self.err(
                close,
                "this layer ends without a colour: its last line is `rgb(…)`, `hsv(…)`, `pal(…)` or `gray(…)`",
            );
        };
        cs[0].blend = blend.unwrap_or(fixed::BLEND_OVER);
        self.end_of_statement()?;
        Ok((cs, ct, rate))
    }

    /// The `loop` the lines from token `k` set (to their `}`, or the end),
    /// read ahead: a part's `t` is its own from its first line.
    fn loop_ahead(&self, mut k: usize) -> Option<i32> {
        let mut depth = 0;
        let mut start = true;
        loop {
            let t = self.toks[k].get();
            match t.tok {
                Tok::LBrace => depth += 1,
                Tok::RBrace if depth == 0 => return None,
                Tok::RBrace => depth -= 1,
                Tok::Eof => return None,
                Tok::Name if depth == 0 && start && self.text(t) == "loop" => {
                    match self.toks[k + 1].get().tok {
                        Tok::Num(v) => return Some(v),
                        Tok::Name if self.text(self.toks[k + 1].get()) == "forever" => {
                            return Some(FOREVER)
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            start = matches!(t.tok, Tok::Newline | Tok::LBrace | Tok::RBrace);
            k += 1;
        }
    }

    /// `draw`, `table` or `for` at `t` where it can't go (inside a layer;
    /// `for` outside `draw { }`).
    #[cold]
    #[inline(never)]
    fn misplaced(&self, t: Token) -> Error {
        self.fail(
            t,
            match self.text(t) {
                "draw" => "`draw { }` goes at the top of the formula, not inside a layer",
                // E911.
                "sprite" => "a sprite goes at the top of the formula, not inside a layer",
                // E921.
                "palette" => "a palette goes at the top of the formula, not inside a layer",
                "table" => "a table goes at the top of the formula, not inside a layer",
                _ => "`for` goes inside `draw { }`: `draw { for k < 12 { … } }`",
            },
        )
    }

    /// The table called `name` that can be seen here.
    #[inline(never)]
    fn table_named(&self, name: &str) -> Option<usize> {
        self.table_names
            .iter()
            .rposition(|tb| tb.name == name && self.visible(tb.scope))
    }

    /// `NAME[k]` at `t` (its name), or why not: a table not read with
    /// `[ ]`, or `[ ]` after something that is not a table.
    #[inline(never)]
    fn table_read(&mut self, t: Token) -> Result<Id, Error> {
        let name = self.text(t);
        if let Some(k) = self.sprite_named(name) {
            return self.sprite_read(t, k);
        }
        let Some(k) = self.table_named(name) else {
            return self.err(
                t,
                format!("`{name}[…]` reads a table, and `{name}` is not one"),
            );
        };
        let open = self.peek();
        if open.tok != Tok::LBracket {
            return self.err(
                t,
                format!("`{name}` is a table: read one of its numbers, `{name}[k]` (k from 0)"),
            );
        }
        self.next();
        let i = self.expr(0)?;
        let close = self.peek();
        if close.tok != Tok::RBracket {
            if close.tok == Tok::Eof || self.line_of(close.start) > self.line_of(open.start) {
                return self.err(open, "this `[` is never closed — add `]`");
            }
            return Err(self.expected(close, "`]`"));
        }
        self.next();
        let (start, len) = (self.table_names[k].start, self.table_names[k].len);
        self.dag
            .tab(i, start, len, &self.tables, (t.start, close.end))
    }

    /// The palette of the formula's own called `name` that can be seen
    /// here.
    #[inline(never)]
    /// The entry named `w` among the palette tokens `from..to`.
    fn entry_at(&self, from: u32, to: u32, w: &str) -> Option<u16> {
        let mut e = 0;
        for k in from as usize..to as usize {
            let t = self.tok(k);
            match t.tok {
                Tok::Hex(_) => e += 1,
                Tok::Name if self.text(t) == w => return Some(e),
                _ => {}
            }
        }
        None
    }

    /// `NAME.kerb` at `t` (the palette's name; the `.` next): the
    /// fraction `pal` maps back to that colour, i/(n − 1) as `i/(n − 1)`
    /// is written; `NAME.step` 1/(n − 1); `NAME.n` the count. Constants.
    #[inline(never)]
    fn palette_entry(&mut self, t: Token, k: usize) -> Result<Id, Error> {
        self.next();
        let w = self.expect(
            Tok::Name,
            "the name of one of the palette's colours, `n` or `step`",
        )?;
        let wn = self.text(w);
        let p = &self.palette_names[k];
        let (n, at) = (i32::from(p.n), (t.start, w.end));
        let i = match wn {
            "n" => return self.dag.constant(n << 16, at),
            "step" => 1,
            _ => match self.entry_at(p.from, p.to, wn) {
                Some(i) => i32::from(i),
                None => {
                    // E927.
                    let pn = p.name;
                    return self.err(
                        w,
                        format!("palette `{pn}` has no colour named `{wn}`: name it in its line, as `{wn} #f80`"),
                    );
                }
            },
        };
        let v = if n > 1 {
            eval(code::DIV, i << 16, (n - 1) << 16, 0, None)
        } else {
            0
        };
        self.dag.constant(v, at)
    }

    fn palette_named(&self, name: &str) -> Option<usize> {
        self.palette_names
            .iter()
            .rposition(|pl| pl.name == name && self.visible(pl.scope))
    }

    #[cold]
    #[inline(never)]
    fn unknown_palette(&self, p: Token) -> Error {
        let pname = self.text(p);
        let own = || {
            self.palette_names
                .iter()
                .filter(|pl| self.visible(pl.scope))
                .map(|pl| pl.name)
        };
        let s = suggest(pname, PALETTES.iter().map(|e| e.0).chain(own()));
        let mut yours = String::new();
        for (k, n) in own().enumerate() {
            yours.push_str(if k == 0 { ", and yours: " } else { ", " });
            yours.push_str(n);
        }
        self.fail_at(
            p,
            format!(
                "unknown palette `{pname}`{} (there are fire, ocean, ice, chrono, amber, rainbow and gray{yours})",
                did_you_mean(s)
            ),
        )
    }

    /// Whether the statement at the cursor is `palette NAME = …`.
    fn is_palette(&self) -> bool {
        let t = self.peek();
        t.tok == Tok::Name
            && self.text(t) == "palette"
            && self.tok(self.pos + 1).tok == Tok::Name
            && self.tok(self.pos + 2).tok == Tok::Assign
    }

    /// `palette NAME = #…, #…, …` at `kw`: 1 to 32 colours (a `,` may end a
    /// line), spread over the palette's 32 entries so that `pal(NAME, i/(n −
    /// 1))` is colour `i` (the first at 0, the last at 1, the others in
    /// steps between), and with 16 colours or fewer over 16 too (a tween's
    /// half reads those). Or `palette NAME = tween(…)` ([`tween_palette`]).
    #[inline(never)]
    fn palette(&mut self, kw: Token) -> Result<(), Error> {
        self.next();
        let n = self.next();
        let name = self.new_name(n, Naming::Thing)?;
        self.next(); // `=`
        let t = self.peek();
        if t.tok == Tok::Name
            && self.text(t) == "tween"
            && self.tok(self.pos + 1).tok == Tok::LParen
        {
            return self.tween_palette(name, n);
        }
        let mut cols: Vec<u32> = Vec::new();
        let from = self.pos as u32;
        loop {
            // A colour may have a name first (`kerb #806`): one of its
            // own, not `n` or `step` (E925).
            let w = self.peek();
            if w.tok == Tok::Name && matches!(self.tok(self.pos + 1).tok, Tok::Hex(_)) {
                let wn = self.text(w);
                if wn == "n" || wn == "step" {
                    // E926.
                    return self.err(
                        w,
                        format!("`n` and `step` are every palette's own (`{name}.n`, `{name}.step`): name this colour another way"),
                    );
                }
                if self.entry_at(from, self.pos as u32, wn).is_some() {
                    // E925.
                    return self.err(
                        w,
                        format!(
                            "`{wn}` already names a colour of `{name}`: give this one another name"
                        ),
                    );
                }
                self.next();
            }
            let c = self.peek();
            let Tok::Hex(v) = c.tok else {
                // E922.
                return self.err(c, PALETTE_COLOURS);
            };
            self.next();
            cols.push(v);
            if self.peek().tok != Tok::Comma {
                break;
            }
            self.next();
            self.skip_newlines();
        }
        if cols.len() > 32 {
            return self.err(kw, PALETTE_COLOURS);
        }
        // Entry e (where `pal` reads k in e/32..(e + 1)/32) is the colour
        // nearest its middle, (e + ½)/32·(n − 1): so k = i/(n − 1), exact
        // or a hair below (a division per pixel is a multiply), is colour i.
        // Over 16 entries the same way (16 colours: entry e is colour e).
        let last = cols.len() - 1;
        let mut entries = [0i32; 32];
        for (e, v) in entries.iter_mut().enumerate() {
            *v = cols[((2 * e + 1) * last + 32) / 64] as i32;
        }
        let half = (cols.len() <= 16).then(|| {
            let mut h = [0i32; 16];
            for (e, v) in h.iter_mut().enumerate() {
                *v = cols[((2 * e + 1) * last + 16) / 32] as i32;
            }
            h
        });
        let same = |p: &OwnPal| p.tween.is_none() && p.entries == entries && p.half == half;
        let j = match self.palettes.iter().position(same) {
            Some(j) => j,
            None => {
                // (In halves of a store place, as the store lays them out: a
                // palette of 16 colours or fewer may go in a tween's half;
                // the layout's own check, `lay_palettes`, has the last word.)
                let halves = |p: &OwnPal| if p.half.is_some() { 1 } else { 2 };
                let held: usize = self.palettes.iter().filter(|p| p.tween.is_none()).map(halves).sum();
                if held + if half.is_some() { 1 } else { 2 } > 2 * MAX_PALETTES {
                    // E923.
                    return self.err(
                        kw,
                        format!("a formula can have {MAX_PALETTES} palettes of its own at most"),
                    );
                }
                try_push(
                    &mut self.palettes,
                    OwnPal {
                        entries,
                        half,
                        tween: None,
                        live: None,
                    },
                )?;
                self.palettes.len() - 1
            }
        };
        try_push(
            &mut self.palette_names,
            Palette {
                name,
                at: n.start,
                scope: self.scope,
                j,
                n: cols.len() as u16,
                from,
                to: self.pos as u32,
            },
        )?;
        self.end_of_statement()
    }

    /// `palette NAME = tween(P, Q, w)` (the cursor at `tween`), or two of
    /// them, `tween(…), tween(…)`: the palette's low and high 16 entries,
    /// each of palettes of 16 colours at most (E937). Its 32 entries are
    /// worked out once a frame (`code::TWEEN`); `NAME.n` is 32, `NAME.step`
    /// one entry.
    #[inline(never)]
    fn tween_palette(&mut self, name: &'s str, n: Token) -> Result<(), Error> {
        let from = self.pos as u32;
        let mut parts = [NO_PART; 2];
        for (k, part) in parts.iter_mut().enumerate() {
            let t = self.peek();
            if t.tok != Tok::Name || self.text(t) != "tween" {
                return Err(self.expected(
                    t,
                    "another `tween(…)`: a palette is one tween, or two (its halves)",
                ));
            }
            *part = self.tween_node()?;
            if k == 1 || self.peek().tok != Tok::Comma {
                break;
            }
            self.next();
            self.skip_newlines();
        }
        if parts[1] != NO_PART {
            // Each half: palettes of the formula's own of 16 colours or
            // fewer (their entries over 16), and tweens of them.
            let mut stack: Vec<PSrc> = Vec::new();
            for &r in &parts {
                stack.push(PSrc::Node(r));
            }
            while let Some(s) = stack.pop() {
                let at = match s {
                    PSrc::Node(k) => {
                        let m = self.pnodes[k as usize];
                        stack.push(m.p);
                        stack.push(m.q);
                        continue;
                    }
                    PSrc::Own(j, _) if self.palettes[j].half.is_some() => continue,
                    PSrc::Own(_, at) | PSrc::Builtin(_, at) => at,
                };
                // E937.
                return Err(Error::new(
                    at.0,
                    at.1,
                    format!(
                        "a palette's half is a tween of palettes of your own with 16 colours at most: `{}` is not one",
                        sub(self.src, at.0, at.1)
                    ),
                ));
            }
        }
        try_push(
            &mut self.palettes,
            OwnPal {
                entries: [0; 32],
                half: None,
                tween: Some(parts),
                live: None,
            },
        )?;
        try_push(
            &mut self.palette_names,
            Palette {
                name,
                at: n.start,
                scope: self.scope,
                j: self.palettes.len() - 1,
                n: 32,
                from,
                to: from,
            },
        )?;
        self.end_of_statement()
    }

    /// `tween(P, Q, w)` at the cursor: its node.
    #[inline(never)]
    fn tween_node(&mut self) -> Result<u16, Error> {
        let kw = self.next();
        self.expect(Tok::LParen, "`(` (as in `tween(day, night, w)`)")?;
        let p = self.tween_src()?;
        self.expect(
            Tok::Comma,
            "`,` and the palette to mix towards (as in `tween(day, night, w)`)",
        )?;
        let q = self.tween_src()?;
        self.expect(
            Tok::Comma,
            "`,` and how far to mix, 0 to 1 (as in `tween(day, night, w)`)",
        )?;
        let st = self.peek();
        let mut w = self.expr(0)?;
        let at = (
            st.start,
            self.toks[self.pos.saturating_sub(1)].get().end.max(st.end),
        );
        if self.dag.level(w) > LV_FRAME {
            // E936.
            return Err(self.varies_err(at, TWEEN_FRAME, w));
        }
        if self.dag.grows(w) {
            // (Held to 0..1 anyway: past 1 is 1.)
            let one = self.dag.constant(ONE, at)?;
            w = self.dag.op(code::MIN, [w, one, 0], at)?;
        }
        self.expect(Tok::RParen, "`)` (as in `tween(day, night, w)`)")?;
        if self.pnodes.len() >= u16::MAX as usize - 1 {
            return self.err(kw, "this is nested too deeply — split it into named steps");
        }
        try_push(&mut self.pnodes, PNode { p, q, w })?;
        Ok((self.pnodes.len() - 1) as u16)
    }

    /// A tween's palette at the cursor: a name (the formula's own or a
    /// built-in) or another `tween(…)`.
    fn tween_src(&mut self) -> Result<PSrc, Error> {
        let t = self.peek();
        if t.tok != Tok::Name {
            return Err(self.expected(t, "a palette (as in `tween(day, night, w)`)"));
        }
        let name = self.text(t);
        if name == "tween" && self.tok(self.pos + 1).tok == Tok::LParen {
            return Ok(PSrc::Node(self.tween_node()?));
        }
        self.next();
        let at = (t.start, t.end);
        if let Some(k) = self.palette_named(name) {
            return Ok(PSrc::Own(self.palette_names[k].j, at));
        }
        match PALETTES.iter().find(|e| e.0 == name).map(|e| e.1) {
            // E932.
            Some(255) => self.err(
                t,
                "`tween` mixes palettes of colours, and gray is not one: write your own, `palette g = #000, #fff`",
            ),
            Some(b) => Ok(PSrc::Builtin(b, at)),
            None => Err(self.unknown_palette(t)),
        }
    }

    /// The live slot `pal` and `cycle` read the formula's own palette `j`
    /// from: given at its first (E933 past three, with the turns, once
    /// the formula is read).
    fn live_of(&mut self, j: usize) -> u8 {
        if let Some(l) = self.palettes[j].live {
            return l;
        }
        let l = self.palettes.iter().filter(|p| p.live.is_some()).count() as u8;
        self.palettes[j].live = Some(l);
        l
    }

    /// E924: the tables and sprites would pass the 256 numbers.
    #[cold]
    #[inline(never)]
    fn too_full(&self, kw: Token, more: usize) -> Error {
        self.fail_at(
            kw,
            format!(
                "tables and sprites hold {MAX_TABLE} numbers in all — this would make {}",
                self.tables.len() + more
            ),
        )
    }

    /// Whether `name` at the cursor reads a table or a sprite (`[` after
    /// it, or a table's or a sprite's name). Out of line: the recursive
    /// parser's frames stay small.
    #[inline(never)]
    fn reads_table(&self, name: &str) -> bool {
        self.peek().tok == Tok::LBracket
            || (!self.table_names.is_empty() && self.table_named(name).is_some())
            || (!self.sprites.is_empty() && self.sprite_named(name).is_some())
    }

    /// The sprite called `name` that can be seen here.
    #[inline(never)]
    fn sprite_named(&self, name: &str) -> Option<usize> {
        self.sprites
            .iter()
            .rposition(|sp| sp.name == name && self.visible(sp.scope))
    }

    /// `NAME[u, v]` at `t`: sprite `k`'s pixel at column `u`, row `v`. Kept
    /// small (its errors and its node out of line), and counted as two
    /// levels of nesting: it holds a frame across two values.
    #[inline(never)]
    fn sprite_read(&mut self, t: Token, k: usize) -> Result<Id, Error> {
        let open = self.peek();
        if open.tok != Tok::LBracket {
            return Err(self.sprite_misread(t, false));
        }
        self.next();
        self.depth += 1;
        let u = self.expr(0);
        let u = match u {
            Ok(u) if self.peek().tok == Tok::Comma => u,
            Ok(_) => {
                self.depth -= 1;
                return Err(self.sprite_misread(t, true));
            }
            Err(e) => {
                self.depth -= 1;
                return Err(e);
            }
        };
        self.next();
        let v = self.expr(0);
        self.depth -= 1;
        let v = v?;
        let close = self.peek();
        if close.tok != Tok::RBracket {
            return Err(self.unclosed_bracket(open, close));
        }
        self.next();
        self.sprite_node(u, v, self.sprites[k].start, (t.start, close.end))
    }

    /// E914: the character at `at` of a sprite's row is not in its key (the
    /// whole character, however many bytes).
    #[cold]
    #[inline(never)]
    fn not_in_key(&self, at: usize) -> Error {
        let ch = sub(self.src, at, self.src.len())
            .chars()
            .next()
            .unwrap_or('?');
        Error::new(
            at,
            at + ch.len_utf8(),
            format!("`{ch}` is not in the sprite's key"),
        )
    }

    /// A sprite read without `[u, v]` (`two`: with one value in it).
    #[cold]
    #[inline(never)]
    fn sprite_misread(&self, t: Token, two: bool) -> Error {
        let name = self.text(t);
        if two {
            // E918.
            return self.fail_at(
                self.peek(),
                format!("a sprite is read at a column and a row: `{name}[u, v]`"),
            );
        }
        self.fail_at(
            t,
            format!("`{name}` is a sprite: read its pixels, `{name}[u, v]`"),
        )
    }

    /// A `[` whose `]` was expected at `close`.
    #[cold]
    #[inline(never)]
    fn unclosed_bracket(&self, open: Token, close: Token) -> Error {
        if close.tok == Tok::Eof || self.line_of(close.start) > self.line_of(open.start) {
            return self.fail(open, "this `[` is never closed — add `]`");
        }
        self.expected(close, "`]`")
    }

    /// The node of a sprite read at `u, v` of the sprite at `start`.
    #[inline(never)]
    fn sprite_node(&mut self, u: Id, v: Id, start: usize, at: (usize, usize)) -> Result<Id, Error> {
        if self.dag.grows(u) || self.dag.grows(v) {
            return Err(Error::new(at.0, at.1, GROWS));
        }
        let (mut level, mut why) = (LV_FRAME, 0);
        for x in [u, v] {
            let m = &self.dag.nodes[x as usize];
            if m.level > level {
                (level, why) = (m.level, m.why);
            }
        }
        self.dag.push(
            Node {
                code: code::SPR,
                a: u,
                b: v,
                c: start as Id,
                val: 0,
                level,
                why,
            },
            at,
        )
    }

    /// Whether the statement at the cursor is a sprite block (`sprite NAME
    /// {` as the lexer reads one: its rows are raw).
    fn is_sprite(&self) -> bool {
        let t = self.peek();
        t.tok == Tok::Name
            && self.text(t) == "sprite"
            && self.tok(self.pos + 1).tok == Tok::Name
            && self.tok(self.pos + 2).tok == Tok::LBrace
            && matches!(
                self.tok(self.pos + 3).tok,
                Tok::Raw | Tok::RBrace | Tok::Eof
            )
    }

    /// `sprite NAME { key …; rows }` at `kw`: its key (characters and the
    /// numbers they read as, up to 16) and its rows of characters (equal
    /// widths up to 64, up to 32 rows), packed into the tables: a header
    /// word, the keys' numbers, the rows at 1, 2 or 4 bits a pixel.
    #[inline(never)]
    fn sprite(&mut self, kw: Token) -> Result<(), Error> {
        self.next();
        let n = self.next();
        let name = self.new_name(n, Naming::Thing)?;
        self.next(); // `{`
        let mut raws: Vec<Token> = Vec::new();
        loop {
            let t = self.peek();
            match t.tok {
                Tok::Raw => {
                    raws.push(t);
                    self.next();
                }
                Tok::RBrace => {
                    self.next();
                    break;
                }
                // E919.
                _ => {
                    return self.err(
                        kw,
                        "this `sprite {` is never closed — add `}` on a line of its own",
                    )
                }
            }
        }
        let Some((&key, rows)) = raws.split_first() else {
            // E912.
            return self.err(kw, SPRITE_KEY);
        };
        // The key: `key C n, C n, …` (bytes: `str`'s trims are Unicode's).
        let kb = self.text(key).as_bytes();
        if kb.len() < 4 || &kb[..3] != b"key" || !matches!(kb[3], b' ' | b'\t') {
            return self.err(key, SPRITE_KEY);
        }
        let mut chars: Vec<u8> = Vec::new();
        let mut nums: Vec<i32> = Vec::new();
        let mut i = 3;
        while i < kb.len() {
            let ws = |i: &mut usize| {
                while *i < kb.len() && matches!(kb[*i], b' ' | b'\t') {
                    *i += 1;
                }
            };
            ws(&mut i);
            let s = i;
            let mut e = i;
            while e < kb.len() && kb[e] != b',' {
                e += 1;
            }
            let mut te = e;
            while te > s && matches!(kb[te - 1], b' ' | b'\t') {
                te -= 1;
            }
            let bad = Error::new(key.start + s, key.start + te.max(s + 1), SPRITE_KEY_ENTRY);
            if te < s + 3 || !key_char(kb[s]) || !matches!(kb[s + 1], b' ' | b'\t') {
                return Err(bad);
            }
            let mut ns = s + 1;
            while matches!(kb[ns], b' ' | b'\t') {
                ns += 1;
            }
            let neg = kb[ns] == b'-';
            // `R #f00`: a colour, the paint index `spr` draws it in.
            let hex = (kb[ns] == b'#')
                .then(|| hex_colour(sub(self.src, key.start + ns + 1, key.start + te)))
                .flatten();
            let v = match hex {
                Some(rgb) => Some((self.paint_index(rgb, key)? as i32) << 16),
                None => number(sub(
                    self.src,
                    key.start + ns + usize::from(neg),
                    key.start + te,
                )),
            };
            let Some(v) = v else {
                return Err(bad);
            };
            if chars.contains(&kb[s]) {
                return Err(Error::new(
                    key.start + s,
                    key.start + s + 1,
                    format!("`{}` is in the key twice", kb[s] as char),
                ));
            }
            chars.push(kb[s]);
            nums.push(if neg { -v } else { v });
            i = e + 1;
        }
        if chars.len() > 16 {
            return self.err(key, "a sprite's key has 16 characters at most");
        }
        // Its rows.
        let w = rows.first().map_or(0, |r| r.end - r.start);
        if rows.is_empty() || rows.len() > 32 {
            // E916.
            return self.err(kw, "a sprite has 1 to 32 rows of characters, after its key");
        }
        let lb: u32 = match chars.len() {
            0..=2 => 0,
            3..=4 => 1,
            _ => 2,
        };
        let ppw = 32 >> lb;
        let wpr = w.div_ceil(ppw);
        let mut words: Vec<i32> = Vec::with_capacity(1 + chars.len() + rows.len() * wpr);
        words.push(
            (w as u32 | (rows.len() as u32) << 8 | lb << 16 | (chars.len() as u32) << 20) as i32,
        );
        words.extend_from_slice(&nums);
        for r in rows {
            let text = self.text(*r).as_bytes();
            if text.len() != w || w > 64 {
                // E915.
                return self.err(
                    *r,
                    format!(
                        "a sprite's rows are as wide as its first ({w} characters), at most 64"
                    ),
                );
            }
            let mut row = alloc::vec![0u32; wpr];
            for (x, &c) in text.iter().enumerate() {
                let Some(k) = chars.iter().position(|&kc| kc == c) else {
                    return Err(self.not_in_key(r.start + x));
                };
                row[x / ppw] |= (k as u32) << ((x % ppw) << lb);
            }
            words.extend(row.iter().map(|&v| v as i32));
        }
        let start = match self
            .tables
            .windows(words.len())
            .position(|win| win == words.as_slice())
        {
            Some(k) => k,
            None => {
                if self.tables.len() + words.len() > MAX_TABLE {
                    // E917.
                    return Err(self.too_full(kw, words.len()));
                }
                try_extend(&mut self.tables, &words)?;
                self.tables.len() - words.len()
            }
        };
        try_push(
            &mut self.sprites,
            Sprite {
                name,
                at: n.start,
                scope: self.scope,
                start,
            },
        )?;
        self.end_of_statement()
    }

    /// `table NAME = n, n, …` at `kw`: numbers only (a `,` may end a line
    /// and the numbers go on), 256 in all.
    #[inline(never)]
    fn table(&mut self, kw: Token) -> Result<(), Error> {
        self.next();
        let n = self.peek();
        if n.tok != Tok::Name {
            return self.err(n, "a table needs a name, as in `table T = 1, 2, 3`");
        }
        let name = self.new_name(n, Naming::Thing)?;
        self.next();
        self.expect(Tok::Assign, "`=` (as in `table T = 1, 2, 3`)")?;
        let mut vals: Vec<i32> = Vec::new();
        loop {
            let neg = self.peek().tok == Tok::Minus;
            if neg {
                self.next();
            }
            let v = self.peek();
            let Tok::Num(x) = v.tok else {
                return self.err(
                    v,
                    "a table holds numbers only, up to 256 in all, as in `table T = 1, -2, 0.5`",
                );
            };
            self.next();
            try_push(&mut vals, if neg { -x } else { x })?;
            if self.peek().tok != Tok::Comma {
                break;
            }
            self.next();
            self.skip_newlines();
        }
        // The same numbers already held (a part read again, or two tables
        // alike): stored once.
        let start = match self
            .tables
            .windows(vals.len())
            .position(|w| w == vals.as_slice())
        {
            Some(k) => k,
            None => {
                if self.tables.len() + vals.len() > MAX_TABLE {
                    return self.err(
                        kw,
                        format!(
                            "a table holds numbers only, up to {MAX_TABLE} in all — this would make {}",
                            self.tables.len() + vals.len()
                        ),
                    );
                }
                try_extend(&mut self.tables, &vals)?;
                self.tables.len() - vals.len()
            }
        };
        try_push(
            &mut self.table_names,
            Table {
                name,
                at: n.start,
                scope: self.scope,
                start,
                len: vals.len(),
            },
        )?;
        self.end_of_statement()
    }

    /// Whether `id` (a value from `start` to `end`) can be used inside
    /// `draw { }`: the same for every pixel.
    #[inline(never)]
    fn drawable(&self, id: Id, start: usize, end: usize) -> Result<(), Error> {
        if self.dag.level(id) < LV_ROW {
            return Ok(());
        }
        Err(self.varies_err((start, end.max(start)), DRAW_ONLY, id))
    }

    /// E301, E708, E931: `head` and what makes `id` vary (out of line:
    /// the callers are on the parser's deepest path).
    #[cold]
    #[inline(never)]
    fn varies_err(&self, (a, b): (usize, usize), head: &str, id: Id) -> Error {
        let mut m = String::from(head);
        m.push_str(&self.varies(id));
        Error::new(a, b, m)
    }

    /// What makes `id` vary across the panel, for an error: "`k` (line 2)
    /// is per pixel: it reads `x`" when one of the formula's own values is
    /// on the way (the nearest), else "this uses `x` (…)" (E301, E708,
    /// E931).
    #[cold]
    #[inline(never)]
    fn varies(&self, id: Id) -> String {
        let named = |m: Id| {
            self.locals
                .iter()
                .rposition(|l| l.id == m && !l.counter && self.visible(l.scope))
        };
        let (j, root) = match named(id) {
            Some(j) => (Some(j), cause(&self.dag, id, &|_| None).1),
            None => cause(&self.dag, id, &named),
        };
        match j {
            Some(j) => {
                let l = &self.locals[j];
                let root = cause(&self.dag, l.id, &|_| None).1;
                let w = input_word(&self.dag.nodes[root as usize]).0;
                let per = if self.dag.level(l.id) == LV_ROW {
                    "row"
                } else {
                    "pixel"
                };
                format!(
                    "`{}` (line {}) is per {per}: it reads `{w}`",
                    l.name,
                    self.line_of(l.at)
                )
            }
            None => {
                let (w, what) = input_word(&self.dag.nodes[root as usize]);
                format!("this uses `{w}` ({what})")
            }
        }
    }

    /// `draw { … }` at `kw`: once a frame, before the rows — values, `for`
    /// loops, `line(…)` and `dot(…)` into the canvas `ink` reads.
    #[inline(never)]
    fn draw_block(&mut self, kw: Token) -> Result<(), Error> {
        self.next();
        if self.in_layer() {
            return Err(self.misplaced(kw));
        }
        if self.record && self.drawn.is_some() {
            return self.err(
                kw,
                "there can be one `draw { }` block (functions that draw are called inside it)",
            );
        }
        let open = self.peek();
        if open.tok != Tok::LBrace {
            return Err(self.expected(open, "`{` (as in `draw { … }`)"));
        }
        self.next();
        if self.record {
            self.drawn = Some(kw);
        }
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.too_deep());
        }
        self.in_draw = 1;
        let r = self.draw_lines(kw);
        self.in_draw = 0;
        self.depth -= 1;
        r?;
        self.next();
        self.end_of_statement()
    }

    /// The lines of a `draw { }` or a `for` (at `kw`) up to its `}`.
    fn draw_lines(&mut self, kw: Token) -> Result<(), Error> {
        self.skip_newlines();
        loop {
            let t = self.peek();
            match t.tok {
                Tok::RBrace => return Ok(()),
                Tok::Eof => {
                    return self.err(
                        kw,
                        if self.text(kw) == "for" {
                            "this `for {` is never closed — add `}`"
                        } else {
                            "this `draw {` is never closed — add `}`"
                        },
                    )
                }
                _ => {}
            }
            let word = if t.tok == Tok::Name { self.text(t) } else { "" };
            let next = self.tok(self.pos + 1).tok;
            if next == Tok::Assign {
                self.assignment(false)?;
            } else if word == "for" {
                self.for_loop(t)?;
            } else if word == "if" {
                self.if_block(t)?;
            } else if next == Tok::LParen && self.func_named(word).is_some() {
                self.call_block(t)?;
            } else if next == Tok::LParen
                && (PAINT_CALLS.contains(&word)
                    || INK_CALLS.contains(&word) && self.paints_with_colour())
            {
                self.paint_call(t)?;
            } else if INK_CALLS.contains(&word) && next == Tok::LParen {
                self.primitive(t)?;
            } else if !self.closed() {
                return self.err(
                    kw,
                    if self.text(kw) == "for" {
                        "this `for {` is never closed — add `}`"
                    } else {
                        "this `draw {` is never closed — add `}`"
                    },
                );
            } else {
                return self.err(
                    t,
                    "inside draw { } go `name = value`, `for k < N { … }`, `if c { … }` and drawing: `line`, `dot`, `pset`, `rect`, `rectfill`, `circ`, `circfill`, `trifill`, `cls`, `spr`, `print`",
                );
            }
            self.skip_newlines();
        }
    }

    /// Whether the block the cursor is in has its `}` ahead.
    fn closed(&self) -> bool {
        let mut depth = 0;
        for t in self.toks[self.pos..].iter().map(|t| t.get()) {
            match t.tok {
                Tok::LBrace => depth += 1,
                Tok::RBrace if depth == 0 => return true,
                Tok::RBrace => depth -= 1,
                _ => {}
            }
        }
        false
    }

    /// `for k < N { … }` at `kw`: `k` = 0, 1, … N − 1, N a whole number
    /// from 1 to 1024; two deep at most; 2048 turns a frame in all.
    #[inline(never)]
    fn for_loop(&mut self, kw: Token) -> Result<(), Error> {
        self.next();
        if self.in_draw > 2 {
            return self.err(kw, "`for` loops go two deep at most");
        }
        let (n, lt, num) = (self.peek(), self.tok(self.pos + 1), self.tok(self.pos + 2));
        let count = match (n.tok, lt.tok, num.tok) {
            (Tok::Name, Tok::Lt, Tok::Num(v)) if v & 0xffff == 0 && (ONE..=1024 * ONE).contains(&v) => {
                (v >> 16) as u32
            }
            _ => {
                return self.err(
                    kw,
                    "`for` takes a name and a count, e.g. `for k < 12 { … }` (a whole number from 1 to 1024)",
                )
            }
        };
        let name = self.text(n);
        if self
            .locals
            .iter()
            .any(|l| l.counter && l.name == name && self.visible(l.scope))
        {
            return self.err(n, format!("`{name}` is already a loop counter here"));
        }
        let name = self.new_name(n, Naming::Counter)?;
        self.pos += 3;
        let open = self.peek();
        if open.tok != Tok::LBrace {
            return Err(self.expected(open, "`{` (as in `for k < 12 { … }`)"));
        }
        self.next();
        let depth = (self.in_draw - 1) as usize;
        let turns = if depth == 0 {
            count
        } else {
            count * self.counts[0]
        };
        if self.record {
            self.turns += turns;
            if self.turns > crate::draw::TURNS {
                return self.err(
                    kw,
                    format!(
                        "this formula draws too much a frame ({} loop turns in all; the limit is {}) — fewer or shorter loops",
                        self.turns,
                        crate::draw::TURNS
                    ),
                );
            }
        }
        self.counts[depth] = count;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.too_deep());
        }
        let outer = self.scope;
        try_push(&mut self.scopes, (outer, false))?;
        self.scope = (self.scopes.len() - 1) as u16;
        let ctr = self.dag.push(
            Node {
                code: K_COUNTER,
                a: 0,
                b: 0,
                c: 0,
                val: self.loops as i32,
                level: if depth == 0 { LV_LOOP1 } else { LV_LOOP2 },
                why: 0,
            },
            (n.start, n.end),
        )?;
        self.loops += 1;
        try_push(
            &mut self.locals,
            Local {
                name,
                id: ctr,
                at: n.start,
                in_frame: false,
                counter: true,
                control: false,
                scope: self.scope,
            },
        )?;
        if self.record {
            try_push(&mut self.draw, DItem::For(ctr, count as u16))?;
            #[cfg(feature = "syntax")]
            self.loop_at.push((kw.start, num.end));
        }
        self.in_draw += 1;
        let r = self.draw_lines(kw);
        self.in_draw -= 1;
        self.scope = outer;
        self.depth -= 1;
        r?;
        self.next();
        if self.record {
            try_push(&mut self.draw, DItem::End)?;
        }
        self.end_of_statement()
    }

    /// `line(x0, y0, x1, y1[, v0[, v1]])` or `dot(x, y[, v])` at `t`.
    #[inline(never)]
    fn primitive(&mut self, t: Token) -> Result<(), Error> {
        let line = self.text(t) == "line";
        let (need, most) = if line { (4, 6) } else { (2, 3) };
        self.next();
        let open = self.next();
        let mut v = [0 as Id; 6];
        let mut k = 0;
        if self.peek().tok != Tok::RParen {
            loop {
                let start = self.peek().start;
                let e = self.expr(0)?;
                let end = self.toks[self.pos.saturating_sub(1)].get().end;
                self.drawable(e, start, end)?;
                if self.dag.grows(e) {
                    // E906.
                    return Err(Error::new(
                        start,
                        end.max(start),
                        "a value that grows without end can't be drawn at: use frac(t*k)*128, t % n or min(t, k)",
                    ));
                }
                if k < 6 {
                    v[k] = e;
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
            return Err(self.not_closed(open, close, self.text(t)));
        }
        self.next();
        if k < need || k > most {
            return self.err(
                t,
                if line {
                    format!("`line` takes x0, y0, x1, y1 (pixels from the top left) and 1 or 2 inks: {k} given")
                } else {
                    format!("`dot` takes x, y (pixels from the top left) and an ink: {k} given")
                },
            );
        }
        let at = (t.start, close.end);
        if line {
            if k < 5 {
                v[4] = self.dag.constant(ONE, at)?;
            }
            if k < 6 {
                v[5] = v[4];
            }
        } else if k < 3 {
            v[2] = self.dag.constant(ONE, at)?;
        }
        if self.record {
            try_push(
                &mut self.draw,
                if line {
                    DItem::Line(v)
                } else {
                    DItem::Dot([v[0], v[1], v[2]])
                },
            )?;
        }
        self.end_of_statement()
    }

    /// The function called `name`, if the formula has one.
    fn func_named(&self, name: &str) -> Option<usize> {
        self.funcs.iter().rposition(|f| f.name == name)
    }

    /// `fn NAME(a, b) = value` or `fn NAME(a, b) { … }` at `kw`: read
    /// later, at each call.
    #[inline(never)]
    fn fn_def(&mut self, kw: Token) -> Result<(), Error> {
        self.next();
        let n = self.next();
        if n.tok != Tok::Name || self.tok(self.pos).tok != Tok::LParen {
            return self.err(kw, "a function is `fn name(a, b) = value`, or `fn name(a, b) { … }` for lines that draw");
        }
        let name = self.new_name(n, Naming::Value)?;
        self.next();
        let mut params = Vec::new();
        while self.peek().tok != Tok::RParen {
            let p = self.next();
            if p.tok != Tok::Name {
                return self.err(p, "a function's values are names: `fn name(a, b) = …`");
            }
            try_push(&mut params, (self.text(p), p.start))?;
            if self.peek().tok == Tok::Comma {
                self.next();
            } else if self.peek().tok != Tok::RParen {
                return Err(self.expected(self.peek(), "`,` or `)`"));
            }
        }
        self.next();
        let t = self.next();
        let block = match t.tok {
            Tok::Assign => false,
            Tok::LBrace => true,
            _ => {
                return self.err(
                    t,
                    "after a function's values: `= value`, or `{ … }` for lines that draw",
                )
            }
        };
        let body = self.pos;
        // To the end of its line (a value) or its `}` (lines).
        let mut depth = 0i32;
        loop {
            let t = self.peek();
            match t.tok {
                Tok::Eof if block => {
                    return self.err(
                        kw,
                        format!("this `fn {name} {{` is never closed — add `}}`"),
                    )
                }
                Tok::Eof => break,
                Tok::LBrace | Tok::LParen | Tok::LBracket => depth += 1,
                Tok::RBrace if block && depth == 0 => break,
                Tok::RBrace | Tok::RParen | Tok::RBracket => depth -= 1,
                Tok::Newline if !block && depth <= 0 => break,
                _ => {}
            }
            self.next();
        }
        let end = self.pos;
        let last = self.last_line(body, end);
        if last == end {
            return self.err(kw, format!("function `{name}` has nothing in it"));
        }
        let colour = self.line_colour(last);
        try_push(
            &mut self.funcs,
            Func {
                name,
                at: n.start,
                params,
                body,
                end,
                block,
                last,
                colour,
            },
        )?;
        if block {
            self.next();
        }
        self.end_of_statement()
    }

    /// Where the last line from `body` to `end` starts (`end` if none).
    fn last_line(&self, body: usize, end: usize) -> usize {
        let (mut last, mut depth, mut start) = (end, 0i32, true);
        for k in body..end {
            let t = self.toks[k].get().tok;
            if start && t != Tok::Newline {
                last = k;
            }
            start = t == Tok::Newline && depth <= 0;
            match t {
                Tok::LBrace | Tok::LParen | Tok::LBracket => depth += 1,
                Tok::RBrace | Tok::RParen | Tok::RBracket => depth -= 1,
                _ => {}
            }
        }
        last
    }

    /// What the line at `k` is, as a function's last: a value, a colour
    /// read by channel or a whole colour.
    fn line_colour(&self, k: usize) -> u8 {
        let t = self.toks[k].get();
        if t.tok == Tok::Name
            && matches!(self.text(t), w if PRIMITIVES.contains(&w) || w == "for" || w == "if")
        {
            return F_DRAWS;
        }
        if t.tok == Tok::Name && self.toks[k + 1].get().tok == Tok::LParen {
            let w = self.text(t);
            if let Some(f) = self.func_named(w) {
                if self.funcs[f].colour == F_WHOLE {
                    return F_WHOLE;
                }
            } else if w == "rgb" {
                // `rgb(r, g, b, cover)`: a whole colour (its coverage).
                let (mut depth, mut commas, mut j) = (0, 0, k + 1);
                loop {
                    match self.toks[j].get().tok {
                        Tok::LParen | Tok::LBracket => depth += 1,
                        Tok::RParen | Tok::RBracket => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Tok::Comma if depth == 1 => commas += 1,
                        Tok::Eof => break,
                        _ => {}
                    }
                    j += 1;
                }
                if commas == 3 || (commas == 1 && self.has_colour(k + 2, true)) {
                    return F_WHOLE;
                }
            } else if w != "mix" && COLOURS.iter().any(|e| e.0 == w) {
                return F_WHOLE;
            }
        }
        if self.has_colour(k, false) {
            F_CHANNELS
        } else {
            F_VALUE
        }
    }

    /// A function's body at the cursor: the names it sets, up to its last
    /// line (E746 for a line that draws).
    fn body_names(&mut self, t: Token, k: usize) -> Result<(), Error> {
        loop {
            self.skip_newlines();
            if self.pos >= self.funcs[k].last {
                return Ok(());
            }
            let w = self.peek();
            if !self.is_assignment() {
                let name = self.funcs[k].name;
                let word = self.text(w);
                if PRIMITIVES.contains(&word) || word == "for" || word == "if" {
                    return self.err(
                        t,
                        format!("`{name}` draws: call it on a line of its own inside `draw {{ }}`"),
                    );
                }
                return self.err(w, "only a function's last line is its value: set a name here (`v = …`), or move it last");
            }
            self.assignment(false)?;
        }
    }

    /// A function whose last line is a whole colour, on the colour line:
    /// its colour.
    #[inline(never)]
    fn call_colour(&mut self, t: Token, k: usize) -> Result<(Vec<Colour>, bool), Error> {
        self.next();
        let args = self.call_args(t, k)?;
        if !matches!(self.peek().tok, Tok::Newline | Tok::Eof | Tok::RBrace) {
            let name = self.funcs[k].name;
            return self.err(
                self.peek(),
                format!("`{name}(…)` is a whole colour: it can't go into a sum (only `#…` and `rgb(r, g, b)` can)"),
            );
        }
        let outer = self.bind(t, k, &args)?;
        let save = self.pos;
        self.pos = self.funcs[k].body;
        self.calling.push(k);
        let r = match self.body_names(t, k) {
            Ok(()) => {
                let l = self.peek();
                self.colour_or_value(l, Tok::RBrace)
            }
            Err(e) => Err(e),
        };
        self.calling.pop();
        self.pos = save;
        self.scope = outer;
        self.depth -= 1;
        let r = r?;
        self.end_of_statement()?;
        Ok(r)
    }

    /// A call's values at the cursor (after the name), as many as `f` takes.
    fn call_args(&mut self, t: Token, k: usize) -> Result<Vec<Id>, Error> {
        let name = self.funcs[k].name;
        let want = self.funcs[k].params.len();
        if self.calling.contains(&k) {
            return self.err(
                t,
                format!("`{name}` calls itself: a function can't (there are no loops of calls)"),
            );
        }
        let open = self.next();
        let mut args = Vec::new();
        if self.peek().tok != Tok::RParen {
            loop {
                try_push(&mut args, self.expr(0)?)?;
                if self.peek().tok == Tok::Comma {
                    self.next();
                    continue;
                }
                break;
            }
        }
        let close = self.peek();
        if close.tok != Tok::RParen {
            return Err(self.not_closed(open, close, name));
        }
        self.next();
        if args.len() != want {
            return self.err(
                t,
                format!("`{name}` takes {want} values, but got {}", args.len()),
            );
        }
        Ok(args)
    }

    /// Bind `f`'s values to `args` in a scope of their own (seen from the
    /// call's), for reading its body; the scope the call was in.
    fn bind(&mut self, t: Token, k: usize, args: &[Id]) -> Result<u16, Error> {
        let outer = self.scope;
        try_push(&mut self.scopes, (outer, false))?;
        self.scope = (self.scopes.len() - 1) as u16;
        for (i, &id) in args.iter().enumerate() {
            let (name, at) = self.funcs[k].params[i];
            try_push(
                &mut self.locals,
                Local {
                    name,
                    id,
                    at,
                    in_frame: false,
                    counter: false,
                    control: false,
                    scope: self.scope,
                },
            )?;
        }
        self.work += self.funcs[k].end - self.funcs[k].body;
        if self.work > MAX_WORK {
            // E749.
            return self.err(
                t,
                "this formula calls its functions too many times over — simplify it",
            );
        }
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.too_deep());
        }
        Ok(outer)
    }

    /// `name(…)` of a function in a value: its value.
    #[inline(never)]
    fn call_value(&mut self, t: Token, k: usize) -> Result<Id, Error> {
        if self.funcs[k].colour == F_DRAWS {
            let name = self.funcs[k].name;
            // E746.
            return self.err(
                t,
                format!("`{name}` draws: call it on a line of its own inside `draw {{ }}`"),
            );
        }
        if self.funcs[k].colour == F_WHOLE {
            let name = self.funcs[k].name;
            return self.err(
                t,
                format!("`{name}(…)` is a whole colour: use it alone on the colour line (only `#…` and `rgb(r, g, b)` go into sums)"),
            );
        }
        let args = self.call_args(t, k)?;
        let outer = self.bind(t, k, &args)?;
        let save = self.pos;
        self.pos = self.funcs[k].body;
        self.calling.push(k);
        let v = match self.body_names(t, k) {
            Ok(()) => self.expr(0),
            Err(e) => Err(e),
        };
        if self.funcs[k].block {
            self.skip_newlines();
        }
        let at_end = self.pos == self.funcs[k].end;
        self.calling.pop();
        self.pos = save;
        self.scope = outer;
        self.depth -= 1;
        let v = v?;
        if !at_end {
            let name = self.funcs[k].name;
            return self.err(
                t,
                format!("function `{name}`'s last line is one value: `fn {name}(…) = value`"),
            );
        }
        Ok(v)
    }

    /// `name(…)` of a function on a line of its own in draw: its lines.
    #[inline(never)]
    fn call_block(&mut self, t: Token) -> Result<(), Error> {
        let k = self.func_named(self.text(t)).unwrap_or(0);
        if !self.funcs[k].block {
            let name = self.funcs[k].name;
            return self.err(
                t,
                format!("`{name}` is a value, not lines that draw: use it in one, `v = {name}(…)`"),
            );
        }
        self.next();
        let args = self.call_args(t, k)?;
        let outer = self.bind(t, k, &args)?;
        let save = self.pos;
        self.pos = self.funcs[k].body;
        self.calling.push(k);
        let r = self.draw_lines(t);
        self.calling.pop();
        self.pos = save;
        self.scope = outer;
        self.depth -= 1;
        r?;
        self.end_of_statement()
    }

    /// Whether the call at the cursor (`line(` or `dot(`) ends in a colour
    /// (`#…`, `pal(…)`, `gray(…)`): it paints, else it draws ink.
    fn paints_with_colour(&self) -> bool {
        let mut depth = 0;
        let mut last = self.pos + 2;
        let mut k = self.pos + 1;
        loop {
            let t = self.tok(k);
            match t.tok {
                Tok::LParen | Tok::LBracket => depth += 1,
                Tok::RParen | Tok::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                Tok::Comma if depth == 1 => last = k + 1,
                Tok::Eof => return false,
                Tok::Newline if depth <= 1 => return false,
                _ => {}
            }
            k += 1;
        }
        let t = self.tok(last);
        matches!(t.tok, Tok::Hex(_))
            || (t.tok == Tok::Name
                && matches!(self.text(t), "pal" | "gray")
                && self.tok(last + 1).tok == Tok::LParen)
    }

    /// Colour `rgb` in the paint palette (added once).
    fn paint_index(&mut self, rgb: u32, at: Token) -> Result<u8, Error> {
        if let Some(k) = self.paint.iter().skip(1).position(|&c| c == rgb) {
            return Ok(k as u8 + 1);
        }
        if self.paint.len() >= MAX_PAINT {
            return self.err(
                at,
                format!(
                    "a formula paints with {} colours at most (each of a palette's 32 counts)",
                    MAX_PAINT - 1
                ),
            );
        }
        try_push(&mut self.paint, rgb)?;
        Ok((self.paint.len() - 1) as u8)
    }

    /// Palette `pal`'s 32 entries in the paint palette (once): where they
    /// start.
    fn paint_palette(&mut self, pal: u8, at: Token) -> Result<u8, Error> {
        if let Some(&(_, b)) = self.paint_pals.iter().find(|e| e.0 == pal) {
            return Ok(b);
        }
        if self.paint.len() + 32 > MAX_PAINT {
            return self.err(
                at,
                format!(
                    "a formula paints with {} colours at most (each of a palette's 32 counts)",
                    MAX_PAINT - 1
                ),
            );
        }
        let base = self.paint.len() as u8;
        for e in 0..32 {
            let c = match pal {
                255 => {
                    let v = fixed::chan((e as i32) * ONE / 31) as u32;
                    v << 16 | v << 8 | v
                }
                0..=4 => {
                    let c = crate::tables::PALETTES[pal as usize][e];
                    (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32
                }
                _ => self
                    .palettes
                    .get((pal - 5) as usize)
                    .map_or(0, |p| p.entries[e] as u32),
            };
            try_push(&mut self.paint, c)?;
        }
        try_push(&mut self.paint_pals, (pal, base))?;
        Ok(base)
    }

    /// A paint colour at the cursor: `#…`, `pal(P, v)` or `gray(v)` (its
    /// index, a value the same for every pixel).
    #[inline(never)]
    fn paint_colour(&mut self) -> Result<Id, Error> {
        let t = self.peek();
        let at = (t.start, t.end);
        if let Tok::Hex(rgb) = t.tok {
            self.next();
            let k = self.paint_index(rgb, t)?;
            return self.dag.constant((k as i32) << 16, at);
        }
        let word = if t.tok == Tok::Name { self.text(t) } else { "" };
        if !matches!(word, "pal" | "gray") || self.tok(self.pos + 1).tok != Tok::LParen {
            return self.err(
                t,
                "a colour to paint with is `#…` (as #f80), `pal(palette, v)` or `gray(v)`",
            );
        }
        self.next();
        self.next();
        let pal = if word == "gray" {
            255
        } else {
            let pt = self.next();
            let pname = self.text(pt);
            // (The paint palette's colours are fixed: a palette of the
            // formula's own by its index here, never a live slot.)
            let own = self.palette_named(pname).map(|k| self.palette_names[k].j);
            if own.is_some_and(|j| self.palettes[j].tween.is_some()) {
                // E938.
                return self.err(
                    pt,
                    format!("`{pname}` is a tween, worked out every frame: paint takes a palette of colours (`palette p = #000, #fff`)"),
                );
            }
            let own = own.map(|j| 5 + j as u8);
            let Some(pal) = PALETTES.iter().find(|e| e.0 == pname).map(|e| e.1).or(own) else {
                return Err(self.unknown_palette(pt));
            };
            self.expect(Tok::Comma, "`,` after the palette")?;
            pal
        };
        let start = self.peek().start;
        let v = self.expr(0)?;
        let end = self.toks[self.pos.saturating_sub(1)].get().end;
        self.drawable(v, start, end)?;
        self.expect(Tok::RParen, "`)`")?;
        let base = self.paint_palette(pal, t)?;
        let m = self.dag.nodes[v as usize];
        self.dag.push(
            Node {
                code: code::PALIDX,
                a: v,
                b: base as Id,
                c: 0,
                val: 0,
                level: m.level.max(LV_FRAME),
                why: m.why,
            },
            at,
        )
    }

    /// A paint primitive at `t` (`pset(x, y, c)`, `line(x0, y0, x1, y1,
    /// c)`, `rect`/`rectfill(x0, y0, x1, y1, c)`, `circ`/`circfill(x, y, r,
    /// c)`, `trifill(x0, y0, x1, y1, x2, y2, c)`, `cls(c)`, `spr(s, x, y
    /// [, flip])`, `print(text or value, x, y, c [, font [, digits]])`).
    #[inline(never)]
    fn paint_call(&mut self, t: Token) -> Result<(), Error> {
        use crate::program::paint;
        let name = self.text(t);
        let kind = match name {
            "cls" => paint::CLS,
            "pset" | "dot" => paint::PSET,
            "line" => paint::LINE,
            "rect" => paint::RECT,
            "rectfill" => paint::RECTFILL,
            "circ" => paint::CIRC,
            "circfill" => paint::CIRCFILL,
            "trifill" => paint::TRIFILL,
            "spr" => paint::SPR,
            _ => paint::PRINT,
        };
        self.next();
        let open = self.next();
        let mut v = [0 as Id; 9];
        let mut n;
        let mut kind = kind;
        let usage = match kind {
            paint::CLS => "`cls(colour)`",
            paint::PSET => "`pset(x, y, colour)` (or `dot`)",
            paint::LINE => "`line(x0, y0, x1, y1, colour)`",
            paint::RECT | paint::RECTFILL => "`rect(x0, y0, x1, y1, colour)` (corners included; `rectfill` filled)",
            paint::CIRC | paint::CIRCFILL => "`circ(x, y, radius, colour)` (`circfill` filled)",
            paint::TRIFILL => "`trifill(x0, y0, x1, y1, x2, y2, colour)`",
            paint::SPR => "`spr(sprite, x, y)` or `spr(sprite, x, y, flip)` (1 across, 2 down, 3 both)",
            _ => "`print(\"TEXT\" or a value, x, y, colour)`, then maybe `small` or `bold` and the digits a value has at least",
        };
        let at = (t.start, open.end);
        // The values before the colour, the colour, then what follows it.
        let before = match kind {
            paint::SPR | paint::PRINT => 0,
            k => paint::ARGS[k as usize] - 1,
        };
        if kind == paint::SPR {
            let st = self.next();
            let Some(k) = (st.tok == Tok::Name)
                .then(|| self.sprite_named(self.text(st)))
                .flatten()
            else {
                return self.err(
                    st,
                    "`spr` draws a sprite: `spr(name, x, y)`, its name a `sprite name { … }`'s",
                );
            };
            let at_tab = self.sprites[k].start;
            v[0] = self.dag.constant((at_tab as i32) << 16, at)?;
            n = 1;
            for _ in 0..3 {
                if self.peek().tok != Tok::Comma {
                    break;
                }
                self.next();
                v[n] = self.paint_value()?;
                n += 1;
            }
            if n == 3 {
                v[3] = self.dag.constant(0, at)?;
                n = 4;
            }
            if n != 4 {
                return self.err(t, format!("{usage} takes a sprite and its place"));
            }
        } else if kind == paint::PRINT {
            // The text: in quotes (its characters into the tables), or a value.
            let st = self.peek();
            if st.tok == Tok::Str {
                self.next();
                let text = sub(self.src, st.start + 1, st.end.saturating_sub(1));
                let vals: Vec<i32> = text.bytes().map(|c| (c as i32) << 16).collect();
                if vals.len() > 32 {
                    return self.err(st, "`print` writes 32 characters at most");
                }
                let start = match self
                    .tables
                    .windows(vals.len().max(1))
                    .position(|w| w == vals.as_slice())
                {
                    Some(k) if !vals.is_empty() => k,
                    _ => {
                        if self.tables.len() + vals.len() > MAX_TABLE {
                            return self.err(st, "this text doesn't fit: tables, sprites and texts hold 256 numbers in all");
                        }
                        try_extend(&mut self.tables, &vals)?;
                        self.tables.len() - vals.len()
                    }
                };
                v[0] = self.dag.constant((start as i32) << 16, at)?;
                v[1] = self.dag.constant((vals.len() as i32) << 16, at)?;
            } else {
                kind = paint::PRINTNUM;
                v[0] = self.paint_value()?;
                v[1] = self.dag.constant(ONE, at)?;
            }
            for slot in &mut v[3..5] {
                self.expect(Tok::Comma, "`,` and where to write it")?;
                *slot = self.paint_value()?;
            }
            self.expect(Tok::Comma, "`,` and the colour")?;
            v[5] = self.paint_colour()?;
            v[2] = self.dag.constant(0, at)?;
            if self.peek().tok == Tok::Comma {
                self.next();
                let f = self.next();
                let font = match (f.tok, self.text(f)) {
                    (Tok::Name, "normal") => 0,
                    (Tok::Name, "small") => 1,
                    (Tok::Name, "bold") if kind == paint::PRINTNUM => 2,
                    _ => {
                        return self.err(
                            f,
                            "`print`'s font is `normal`, `small` or (for a value) `bold`",
                        )
                    }
                };
                v[2] = self.dag.constant(font << 16, at)?;
                if kind == paint::PRINTNUM && self.peek().tok == Tok::Comma {
                    self.next();
                    v[1] = self.paint_value()?;
                }
            }
            n = 6;
        } else {
            for (k, slot) in v.iter_mut().enumerate().take(before) {
                if k > 0 {
                    self.expect(Tok::Comma, "`,`")?;
                }
                if self.peek().tok == Tok::RParen {
                    return self.err(t, format!("{usage}: a value is missing"));
                }
                *slot = self.paint_value()?;
            }
            if before > 0 {
                self.expect(Tok::Comma, "`,` and the colour")?;
            }
            v[before] = self.paint_colour()?;
            n = before + 1;
        }
        let close = self.peek();
        if close.tok != Tok::RParen {
            return self.err(close, format!("{usage}: expected `)` here"));
        }
        self.next();
        if self.record {
            try_push(&mut self.draw, DItem::Paint(kind, v, n as u8))?;
        }
        self.end_of_statement()
    }

    /// A value a paint primitive takes (the same for every pixel).
    fn paint_value(&mut self) -> Result<Id, Error> {
        let start = self.peek().start;
        let e = self.expr(0)?;
        let end = self.toks[self.pos.saturating_sub(1)].get().end;
        self.drawable(e, start, end)?;
        if self.dag.grows(e) {
            return Err(Error::new(
                start,
                end.max(start),
                "a value that grows without end can't be drawn at: use frac(t*k)*128, t % n or min(t, k)",
            ));
        }
        Ok(e)
    }

    /// `if c { … } [else { … }]` at `kw`, inside draw.
    #[inline(never)]
    fn if_block(&mut self, kw: Token) -> Result<(), Error> {
        self.next();
        let c = self.paint_value()?;
        let open = self.peek();
        if open.tok != Tok::LBrace {
            return Err(self.expected(open, "`{` (as in `if k > 2 { … }`)"));
        }
        self.next();
        if self.record {
            try_push(&mut self.draw, DItem::If(c))?;
        }
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.too_deep());
        }
        self.draw_lines(kw)?;
        self.next();
        // `else {` on the same line or the next.
        let mut k = self.pos;
        while self.tok(k).tok == Tok::Newline {
            k += 1;
        }
        let e = self.tok(k);
        if e.tok == Tok::Name && self.text(e) == "else" && self.tok(k + 1).tok == Tok::LBrace {
            self.pos = k + 2;
            if self.record {
                try_push(&mut self.draw, DItem::Else)?;
            }
            self.draw_lines(e)?;
            self.next();
        }
        self.depth -= 1;
        if self.record {
            try_push(&mut self.draw, DItem::EndIf)?;
        }
        self.end_of_statement()
    }

    /// The lines of a formula (`end` Eof) or a part (`end` RBrace, the
    /// cursor after its `{`): names, `frame { }`, `loop`, parts, layers and
    /// the colour.
    fn body(&mut self, end: Tok) -> Result<Body, Error> {
        let mut b = Body {
            colours: Vec::new(),
            top: None,
            layered: false,
            blocks: 0,
            loop_v: None,
            fps: None,
            rates: Vec::new(),
        };
        let mut last: Option<Token> = None;
        self.skip_newlines();
        self.header(end == Tok::Eof)?;
        loop {
            let t = self.peek();
            if t.tok == end {
                break;
            }
            if let Some(ct) = last {
                return Err(Error::new(
                    ct.start,
                    ct.end,
                    if end == Tok::Eof {
                        "the colour has to be the last line (move this to the end)"
                    } else {
                        "the colour has to be the part's last line (move this to just before `}`)"
                    },
                ));
            }
            match self.keyword() {
                "fn" => self.fn_def(t)?,
                "loop" => {
                    self.next();
                    if b.loop_v.is_some() {
                        return self.err(t, "`loop` is already set");
                    }
                    let n = self.next();
                    let v = match n.tok {
                        Tok::Num(v) => v,
                        // E907 (none: it is the line's own).
                        Tok::Name if self.text(n) == "forever" => FOREVER,
                        _ => {
                            return self.err(
                                n,
                                "`loop` takes a number of seconds, e.g. `loop 8`, or `forever` for one that goes on without repeating",
                            )
                        }
                    };
                    if v != FOREVER && !(ONE / 10..=3600 * ONE).contains(&v) {
                        return self.err(n, "the loop must be between 0.1 and 3600 seconds");
                    }
                    b.loop_v = Some(v);
                    self.end_of_statement()?;
                }
                _ if self.is_fps() => {
                    let ms = self.fps_line()?;
                    if b.fps.replace(ms).is_some() {
                        return self.err(t, "`fps` is already set");
                    }
                }
                _ if self.text(t) == "frame" && self.toks[self.pos + 1].get().tok == Tok::LBrace => return self.err(t, "there are no `frame { }` blocks: a value is worked out once a frame by itself when it reads only the time, the clock and the controls; write its lines without the block"),
                "layer" => {
                    let (cs, ct, rate) = self.layer(t)?;
                    if rate > 0 {
                        try_push(&mut b.rates, (b.colours.len(), rate))?;
                    }
                    b.colours.extend(cs);
                    b.top = Some(ct);
                    b.layered = true;
                    b.blocks += 1;
                }
                // E750.
                "part" => {
                    return self.err(t, "there are no parts: write a function, `fn p(x, y) { … }`, and call it, `p(x*2, y)`")
                }
                "draw" => self.draw_block(t)?,
                "table" => self.table(t)?,
                _ if self.is_sprite() => self.sprite(t)?,
                _ if self.is_palette() => self.palette(t)?,
                _ if end == Tok::Eof && self.is_scene() => self.scene_block()?,
                "for" => return Err(self.misplaced(t)),
                "use" => {
                    return self.err(
                        t,
                        "`use` puts a formula in by its name, as a function `fn p(x, y) { … }`, when you leave the line",
                    )
                }
                _ if self.is_assignment() => self.assignment(false)?,
                _ if self.is_declaration() => return Err(self.late(t)),
                _ => {
                    let (cs, layered) = self.colour_or_value(t, end)?;
                    if b.blocks > 0 {
                        return Err(Error::new(
                            t.start,
                            self.toks[self.pos.saturating_sub(1)].get().end.max(t.end),
                            "with layers, every colour goes inside one: put this line in `layer { … }`",
                        ));
                    }
                    b.colours = cs;
                    b.layered = layered;
                    b.top = Some(t);
                    last = Some(t);
                }
            }
            self.skip_newlines();
        }
        Ok(b)
    }
}

// --- the declarations ---------------------------------------------------------
//
// `psi 1`, `name "…"`, `about "…"`, `control …`, `look …`: lines at the top
// of a formula (or of a part, where only `control` does anything: it names
// the part's value of that slot) that say what it is. Read here, out of the
// recursive parser (`header` returns before any expression is read deeper
// than a number), so the compiler's deepest stack is unchanged.
// docs/research/formula-apps.md §2.1; the codes in psi-tooling-plan.md §4.

/// The words a choice may have: 2 to 6.
const CHOICE_WORDS: core::ops::RangeInclusive<usize> = 2..=6;

impl<'s> Parser<'s> {
    /// The word at the cursor starts a declaration (by its shape: `name`
    /// and `about` before text in quotes, `control` before a name, `psi`
    /// before anything but `=`, `look` alone or before a blend or a number
    /// — and no value or part of the formula's called `look`).
    #[inline(never)]
    fn is_declaration(&self) -> bool {
        let t = self.peek();
        if t.tok != Tok::Name {
            return false;
        }
        let n = self.tok(self.pos + 1);
        let w = self.text(t);
        // (The word alone, or before a number or a word: nothing else it
        // could be, but a part or a value of the formula's of that name.)
        let bare = matches!(
            n.tok,
            Tok::Newline | Tok::Eof | Tok::RBrace | Tok::Num(_) | Tok::Name
        ) && self.func_named(w).is_none()
            && !self
                .locals
                .iter()
                .any(|l| l.name == w && self.visible(l.scope));
        match w {
            "name" | "about" => n.tok == Tok::Str || bare,
            "control" => n.tok == Tok::Name,
            "psi" => n.tok != Tok::Assign,
            "look" => bare,
            _ => false,
        }
    }

    /// E951: a declaration below the formula's other lines.
    #[cold]
    #[inline(never)]
    fn late(&self, t: Token) -> Error {
        let w = self.text(t);
        if w == "psi" {
            // E604.
            return self.fail(t, "`psi 1` goes on the formula's first line");
        }
        self.fail_at(
            t,
            format!("`{w}` goes at the top of the formula, before its other lines"),
        )
    }

    /// The declarations at the top of a formula (`host`) or a part: read,
    /// checked, and — for the formula's own — kept in [`Decls`].
    #[inline(never)]
    fn header(&mut self, host: bool) -> Result<(), Error> {
        // Where each of `name`, `about` and `look` was set (0: not yet).
        let mut seen = [0usize; 3];
        let mut controls = 0usize;
        let mut first = true;
        while self.is_declaration() {
            let t = self.next();
            #[cfg(feature = "syntax")]
            self.decls_seen.push((t.start, t.end));
            match self.text(t) {
                "psi" => {
                    if !first {
                        return Err(self.late(t));
                    }
                    self.version(t)?;
                }
                w @ ("name" | "about") => {
                    let k = usize::from(w == "about");
                    self.once(t, &mut seen[k])?;
                    let q = self.next();
                    let most = if k == 0 {
                        MAX_NAME_TEXT
                    } else {
                        MAX_ABOUT_TEXT
                    };
                    let inner = self.text_in(q, most)?;
                    if host {
                        let d = &mut self.decls().text;
                        *(if k == 0 { &mut d.name } else { &mut d.about }) = Some(inner);
                    }
                }
                "look" => {
                    self.once(t, &mut seen[2])?;
                    let (blend, amount) = self.look_line()?;
                    if host {
                        let end = self.peek().start as u32;
                        let d = self.decls();
                        d.meta.set_look(blend, amount);
                        d.text.look = Some((t.start as u32, end));
                    }
                }
                _ => {
                    if controls == 3 {
                        // E958.
                        return self.err(t, "a formula has three controls at most (a, b and c)");
                    }
                    self.control(t, controls, host)?;
                    controls += 1;
                }
            }
            first = false;
            self.declaration_end(t)?;
            self.skip_newlines();
        }
        if host && controls > 0 {
            self.decls().own = controls as u8;
        }
        Ok(())
    }

    /// The declarations so far (made at the first).
    #[inline(never)]
    fn decls(&mut self) -> &mut Decls {
        self.decl.get_or_insert_with(|| {
            alloc::boxed::Box::new(Decls {
                slots: [NO; 3],
                ..Decls::default()
            })
        })
    }

    /// The end of a declaration's line (E967; a `control` line's own
    /// shape is E957, checked in [`Self::control`]).
    #[inline(never)]
    fn declaration_end(&mut self, t: Token) -> Result<(), Error> {
        let n = self.peek();
        match n.tok {
            Tok::Newline => {
                self.next();
                Ok(())
            }
            Tok::Eof | Tok::RBrace => Ok(()),
            // E967.
            _ => Err(self.fail_at(
                n,
                format!(
                    "expected the end of the `{}` line, found {} (one declaration a line)",
                    self.text(t),
                    self.found(n)
                ),
            )),
        }
    }

    /// E952: `name`, `about` or `look` a second time.
    fn once(&self, t: Token, seen: &mut usize) -> Result<(), Error> {
        if *seen != 0 {
            return Err(self.fail_at(
                t,
                format!(
                    "`{}` is already set on line {}",
                    self.text(t),
                    self.line_of(*seen - 1)
                ),
            ));
        }
        *seen = t.start + 1;
        Ok(())
    }

    /// `psi N`: this compiler is version [`VERSION`]; a newer one is
    /// E601, anything but a whole number from 1 E603.
    #[inline(never)]
    fn version(&mut self, t: Token) -> Result<(), Error> {
        let n = self.peek();
        let v = match n.tok {
            // (Written as a plain whole number: not `01`, not `1.0`.)
            Tok::Num(v)
                if v >= ONE
                    && v & 0xffff == 0
                    && !self.text(n).starts_with('0')
                    && !self.text(n).contains('.') =>
            {
                (v >> 16) as usize
            }
            // E603.
            _ => {
                return self.err(
                    t,
                    "`psi` takes the language's version, a whole number: `psi 1`",
                )
            }
        };
        self.next();
        if v > VERSION {
            // E601.
            return Err(self.fail_at(
                n,
                format!(
                    "this formula needs a newer OpenDMD: it is written for Psi {v}, this one has Psi {VERSION}"
                ),
            ));
        }
        Ok(())
    }

    /// Text in quotes at `q` (E956 if there is none), at most `most`
    /// characters (E955): its range inside the quotes.
    #[inline(never)]
    fn text_in(&self, q: Token, most: usize) -> Result<(u32, u32), Error> {
        if q.tok != Tok::Str {
            // E956.
            return self.err(q, "this takes its text in quotes, e.g. `name \"Sunrise\"`");
        }
        let inner = sub(self.src, q.start + 1, q.end - 1);
        // (Characters: UTF-8's lead bytes.)
        if inner.bytes().filter(|&b| b & 0xc0 != 0x80).count() > most {
            // E955.
            return Err(self.fail_at(
                q,
                format!("too long: this text can be {most} characters at most"),
            ));
        }
        Ok(((q.start + 1) as u32, (q.end - 1) as u32))
    }

    /// `look [blend] [amount]` after `look` (E964): the blend's
    /// place in [`crate::look::BLENDS`] (over when none) and the amount
    /// (100 when none).
    #[inline(never)]
    fn look_line(&mut self) -> Result<(usize, u8), Error> {
        const HOW: &str = "`look` takes a blend (over, screen, add, multiply, max or min) and an amount from 0 to 100";
        let mut blend = 0;
        let n = self.peek();
        if n.tok == Tok::Name {
            blend = crate::look::BLENDS
                .iter()
                .position(|&b| b == self.text(n))
                .ok_or_else(|| self.fail(n, HOW))?;
            self.next();
        }
        let mut amount = 100;
        let n = self.peek();
        match n.tok {
            Tok::Num(v) if v & 0xffff == 0 && v <= 100 * ONE => {
                amount = (v >> 16) as u8;
                self.next();
            }
            Tok::Newline | Tok::Eof | Tok::RBrace => {}
            _ => return self.err(n, HOW),
        }
        Ok((blend, amount))
    }

    /// A signed number at the cursor (`-0.5`, `4`), or `None`.
    fn signed(&mut self) -> Option<i32> {
        let neg = self.peek().tok == Tok::Minus;
        let k = self.pos + usize::from(neg);
        let Tok::Num(v) = self.tok(k).tok else {
            return None;
        };
        self.pos = k;
        self.next();
        Some(if neg { -v } else { v })
    }

    /// `control NAME [lo..hi] [= default] ["Label"]` or `control NAME =
    /// w1, w2, … ["Label"]` after `control` at `kw`: the formula's
    /// control `k` (slot a, b or c). Its name reads as the slot's value
    /// held to the range (a choice's: its number, 0 to n − 1, whole), in
    /// the formula or — `host` false — the part it is written in. A
    /// part's controls describe the slots only when the formula's own
    /// lines declare none (so a formula pasted in as `part look { … }`
    /// keeps its controls).
    #[inline(never)]
    fn control(&mut self, kw: Token, k: usize, host: bool) -> Result<(), Error> {
        const HOW: &str = "a control is `control NAME lo..hi = default \"Label\"` or `control NAME = word, word, …`";
        let nt = self.next();
        let name = self.text(nt);
        let letter = ["a", "b", "c"].iter().position(|&l| l == name);
        match letter {
            Some(j) if j != k => {
                // E963.
                return Err(self.fail_at(
                    nt,
                    format!(
                        "`{name}` is the {} control's letter: give this one another name (controls take a, b, c in order)",
                        ["first", "second", "third"][j]
                    ),
                ));
            }
            Some(_) => {}
            None => {
                self.new_name(nt, Naming::Control)?;
            }
        }
        let (mut lo, mut hi, mut def) = (0, ONE, None);
        let mut ranged = false;
        if matches!(self.peek().tok, Tok::Num(_) | Tok::Minus) {
            let start = self.peek();
            let a = self.signed().ok_or_else(|| self.fail(start, HOW))?;
            if self.next().tok != Tok::DotDot {
                // E957.
                return self.err(start, HOW);
            }
            let bt = self.peek();
            let b = self.signed().ok_or_else(|| self.fail(bt, HOW))?;
            if a >= b {
                // E959.
                return self.err(
                    start,
                    "a range goes from the lower number to the higher, e.g. `0..1`",
                );
            }
            (lo, hi, ranged) = (a, b, true);
        }
        let mut words: Option<(Token, Token, usize)> = None;
        if self.peek().tok == Tok::Assign {
            self.next();
            let v = self.peek();
            if v.tok == Tok::Name && !ranged {
                words = Some(self.words()?);
            } else {
                def = Some(self.signed().ok_or_else(|| self.fail(v, HOW))?);
            }
        }
        let mut label = None;
        let q = self.peek();
        if q.tok == Tok::Str {
            self.next();
            label = Some(self.text_in(q, MAX_LABEL_TEXT)?);
        }
        // The label is the line's last (E957).
        let e = self.peek();
        if !matches!(e.tok, Tok::Newline | Tok::Eof | Tok::RBrace) {
            return self.err(e, HOW);
        }
        let at = (kw.start, nt.end);
        let input = self.dag.input([Input::A, Input::B, Input::C][k], at)?;
        let (lo_k, hi_k) = match words {
            Some((_, _, n)) => (0, ((n as i32) - 1) << 16),
            None => (lo, hi),
        };
        let def = def.unwrap_or(lo_k);
        if !(lo_k..=hi_k).contains(&def) {
            // E960.
            return self.err(nt, "the default is outside the control's range");
        }
        let a = self.dag.constant(lo_k, at)?;
        let b = self.dag.constant(hi_k, at)?;
        let mut v = self.dag.op(code::CLAMP, [input, a, b], at)?;
        if words.is_some() {
            v = self.dag.op(code::FLOOR, [v, 0, 0], at)?;
        }
        if letter.is_none() {
            try_push(
                &mut self.locals,
                Local {
                    name,
                    id: v,
                    at: nt.start,
                    in_frame: false,
                    counter: false,
                    control: true,
                    scope: self.scope,
                },
            )?;
        }
        // The slots: the formula's own controls, or a part's for a slot the
        // formula's own lines leave undeclared (the first part to declare it).
        let d = self.decls();
        if host || d.slots[k] == NO {
            d.slots[k] = v;
            match words {
                Some((_, _, n)) => d.meta.set_choice(k, n),
                None => d.meta.set_slider(k, lo, hi, def),
            }
            d.text.controls[k] = Some(DeclaredControl {
                name: (nt.start as u32, nt.end as u32),
                label,
                words: words.map(|(a, b, _)| (a.start as u32, b.end as u32)),
            });
        }
        Ok(())
    }

    /// A choice's words, `w1, w2, …` (E961: 2 to 6; E962: each once): the
    /// first and last word and how many.
    #[inline(never)]
    fn words(&mut self) -> Result<(Token, Token, usize), Error> {
        let first = self.peek();
        let mut seen: [&str; 7] = [""; 7];
        let mut n = 0;
        loop {
            let w = self.peek();
            if w.tok != Tok::Name {
                // E966.
                return self.err(
                    w,
                    "a choice is words (names) between commas: `control NAME = word, word, …`",
                );
            }
            self.next();
            let text = self.text(w);
            if seen[..n.min(7)].contains(&text) {
                // E962.
                return Err(self.fail_at(w, format!("`{text}` is a choice already")));
            }
            if n < 7 {
                seen[n] = text;
            }
            n += 1;
            if self.peek().tok != Tok::Comma {
                if !CHOICE_WORDS.contains(&n) {
                    // E961.
                    return Err(Error::new(first.start, w.end, "a choice has 2 to 6 words"));
                }
                return Ok((first, w, n));
            }
            self.next();
        }
    }
}

/// What a compile also reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Info {
    /// Values set inside `frame { }` blocks (a part's once for each place
    /// it is drawn at).
    pub frame_values: usize,
    /// `layer` blocks written in the formula's own lines (0: it is one
    /// picture, its last line).
    pub layers: usize,
}

/// Compile `src` into `prog` (which is cleared first). On an error `prog`
/// is left empty.
pub fn compile_into(src: &str, prog: &mut Program) -> Result<Info, Error> {
    compile_layout(src, prog, true)
}

/// [`compile_into`] with (`runs`) or without the column run and the still
/// cache's run: without, every pixel step runs on every pixel of every
/// frame, as before they existed (the same bytes). For measuring a step's
/// cost on the device (`psi-probe`) and the estimate's tests.
#[doc(hidden)]
pub fn compile_layout(src: &str, prog: &mut Program, runs: bool) -> Result<Info, Error> {
    compile_with(src, prog, runs, true)
}

/// [`compile_layout`] with (`narrow`) or without the narrowed pixel steps
/// (`crate::range`: `add_w`, `mul_n`, `shr`, …; the same bytes): without,
/// the checked steps as before them, for `psi-probe`'s before and after.
#[doc(hidden)]
pub fn compile_with(
    src: &str,
    prog: &mut Program,
    runs: bool,
    narrow: bool,
) -> Result<Info, Error> {
    compile_impl(src, prog, runs, narrow, true, true)
}

/// [`compile_into`] as the player takes it ([`crate::psb`]): laid out, but
/// not narrowed or fused (the player does both itself, from the program).
pub fn compile_portable(src: &str, prog: &mut Program) -> Result<Info, Error> {
    compile_impl(src, prog, true, false, true, false)
}

/// [`compile_portable`] with (`cmp`) or without the fused compares (P0b-4;
/// the same bytes): the paint probe's before and after.
#[doc(hidden)]
pub fn compile_portable_with(src: &str, prog: &mut Program, cmp: bool) -> Result<Info, Error> {
    compile_full(
        src,
        prog,
        (true, false, true, false, false, cmp),
        None,
        #[cfg(feature = "syntax")]
        None,
    )
}

/// [`compile_into`] without the fused runs' markers (`crate::fuse`; the
/// same bytes): for the tests that compare, and psi-probe's before and
/// after.
#[doc(hidden)]
pub fn compile_unfused(src: &str, prog: &mut Program) -> Result<Info, Error> {
    compile_impl(src, prog, true, true, true, false)
}

/// [`compile_with`] with (`cmp`) or without the fused compares (P0b-4,
/// `fuse_compares`; the same bytes): without, each comparison a step of
/// its own, as the device measured the estimate's rows before them (the
/// estimate's tests, psi-probe's before and after, the tests that
/// compare).
#[doc(hidden)]
pub fn compile_measured(
    src: &str,
    prog: &mut Program,
    runs: bool,
    narrow: bool,
    cmp: bool,
) -> Result<Info, Error> {
    compile_full(
        src,
        prog,
        (runs, narrow, true, true, false, cmp),
        None,
        #[cfg(feature = "syntax")]
        None,
    )
}

/// [`compile_with`] with (`reuse`) or without scalar slots used again once
/// a value's last use has passed (`assign_slots`): without, every value
/// has a slot of its own, as before (the same bytes; the tests compare).
#[doc(hidden)]
pub fn compile_opts(
    src: &str,
    prog: &mut Program,
    runs: bool,
    narrow: bool,
    reuse: bool,
) -> Result<Info, Error> {
    compile_impl(src, prog, runs, narrow, reuse, true)
}

/// [`compile_opts`]'s body, in each of its callers (one frame fewer under
/// the compiler's recursion: its stack is the firmware's to hold).
#[inline(always)]
fn compile_impl(
    src: &str,
    prog: &mut Program,
    runs: bool,
    narrow: bool,
    reuse: bool,
    fuse: bool,
) -> Result<Info, Error> {
    compile_full(
        src,
        prog,
        (runs, narrow, reuse, fuse, false, true),
        None,
        #[cfg(feature = "syntax")]
        None,
    )
}

/// Every compile's one way through: read, lay out (again without the
/// runs on a layout bug), then the program's finishing passes
/// ([`finish_program`]); `declared` and the `syntax` sink for those who
/// want them. (`runs, narrow, reuse, fuse` as [`compile_opts`] and
/// [`compile_unfused`].)
#[inline(always)]
#[allow(clippy::needless_lifetimes)]
fn compile_full<'s>(
    src: &'s str,
    prog: &mut Program,
    (runs, narrow, reuse, fuse, look, cmp): (bool, bool, bool, bool, bool, bool),
    mut declared: Option<&mut Declared>,
    #[cfg(feature = "syntax")] sink: Option<&'s mut (dyn crate::syntax::Sink + 's)>,
) -> Result<Info, Error> {
    prog.clear();
    let mut r = compile_inner(
        src,
        prog,
        runs,
        (reuse, cmp),
        look,
        declared.as_deref_mut(),
        #[cfg(feature = "syntax")]
        sink,
    );
    if runs && r.as_ref().is_err_and(|e| e.msg == LAYOUT_BUG) {
        debug_assert!(false, "{LAYOUT_BUG}: {src:?}");
        prog.clear();
        r = compile_inner(
            src,
            prog,
            false,
            (reuse, cmp),
            look,
            declared,
            #[cfg(feature = "syntax")]
            None,
        );
    }
    if r.is_err() {
        prog.clear();
    } else {
        finish_program(prog, narrow, fuse);
    }
    r
}

/// What every compiled program gets after it is laid out: the narrowed
/// steps (`narrow`), the fused runs (`fuse`), and the draw loops that run
/// as lanes, decided once. One place, so no way of compiling can miss one.
#[inline(never)]
fn finish_program(prog: &mut Program, narrow: bool, fuse: bool) {
    prog.finish(narrow, fuse);
}

/// [`compile_into`], and where the formula's declarations are in `src`
/// ([`Declared`]: the web module's `controls()` reads a name, a label or
/// a choice's words from there; the program keeps what the player needs,
/// [`Program::meta`]). The program is not narrowed or fused (the same
/// pixels, the checked steps): it is for what the formula declares, not
/// for drawing on the panel.
pub fn compile_declared(src: &str, prog: &mut Program) -> Result<Declared, Error> {
    let mut d = Declared::default();
    compile_full(
        src,
        prog,
        (true, false, true, false, false, true),
        Some(&mut d),
        #[cfg(feature = "syntax")]
        None,
    )?;
    Ok(d)
}

// ('s: the sink's, with `syntax`.)
#[allow(clippy::needless_lifetimes)]
fn compile_inner<'s>(
    src: &'s str,
    prog: &mut Program,
    runs: bool,
    (reuse, cmp): (bool, bool),
    look: bool,
    declared: Option<&mut Declared>,
    #[cfg(feature = "syntax")] sink: Option<&'s mut (dyn crate::syntax::Sink + 's)>,
) -> Result<Info, Error> {
    if src.len() > MAX_SOURCE {
        return Err(Error::new(
            MAX_SOURCE,
            src.len(),
            format!("too long: a formula can be {MAX_SOURCE} characters at most"),
        ));
    }
    let toks = lex(src)?;
    // Room for the nodes from the start, by the tokens (about one node for
    // every two; the bundled Formulas 0.3-0.45): a list grown by doubling
    // holds the old block and the new at once.
    let nodes = try_with_capacity((toks.len() / 2).clamp(64, MAX_NODES))?;
    let index = try_vec(NO, SLOTS)?;
    let mut p = Parser {
        src,
        toks,
        pos: 0,
        dag: Dag {
            nodes,
            index,
            divs: Vec::new(),
            picks: Vec::new(),
            forever: false,
            ctx: Vec::new(),
            deps: Vec::new(),
            #[cfg(feature = "syntax")]
            made: Vec::new(),
            scenes: Vec::new(),
            scene: 0,
            conds: Vec::new(),
        },
        locals: Vec::new(),
        depth: 0,
        scope: 0,
        scopes: alloc::vec![(0, false)],
        env: Env {
            loop_v: 0,
            ch: NO_CH,
        },
        work: 0,
        tables: Vec::new(),
        table_names: Vec::new(),
        sprites: Vec::new(),
        palettes: Vec::new(),
        palette_names: Vec::new(),
        pnodes: Vec::new(),
        draw: Vec::new(),
        funcs: Vec::new(),
        calling: Vec::new(),
        paint: alloc::vec![0],
        paint_pals: Vec::new(),
        drawn: None,
        record: true,
        in_draw: 0,
        counts: [0; 2],
        loops: 0,
        turns: 0,
        decl: None,
        #[cfg(feature = "syntax")]
        reads: Vec::new(),
        #[cfg(feature = "syntax")]
        decls_seen: Vec::new(),
        #[cfg(feature = "syntax")]
        sink,
        #[cfg(feature = "syntax")]
        notes: Vec::new(),
        #[cfg(feature = "syntax")]
        loop_at: Vec::new(),
    };
    p.env.loop_v = p.loop_ahead(0).unwrap_or(8 * ONE);
    p.dag.forever = p.env.loop_v == FOREVER;
    let body = p.body(Tok::Eof);
    #[cfg(feature = "syntax")]
    p.observe();
    finish(src, &mut p, body?, prog, (runs, reuse, look, cmp), declared)
}

/// The rest of [`compile_inner`], once the formula is read: the program
/// laid out. Out of line, so its locals are not in the frame under the
/// parser's deepest (the firmware's compile stack).
#[inline(never)]
fn finish(
    src: &str,
    p: &mut Parser<'_>,
    body: Body,
    prog: &mut Program,
    (runs, reuse, look, cmp): (bool, bool, bool, bool),
    declared: Option<&mut Declared>,
) -> Result<Info, Error> {
    if let Some(v) = body.loop_v {
        prog.loop_ms = if v == FOREVER {
            0
        } else {
            ((v as i64 * 1000 + ONE as i64 / 2) >> 16) as u32
        };
    }
    // A part's `fps` (a whole formula pasted in) is left to the formula's.
    prog.frame_ms = body.fps.unwrap_or(0);
    if let Some(d) = &p.decl {
        declarations(d, body.layered, prog, declared)?;
    }
    let mut layers = body.colours;
    let mut top = body.top;
    // A formula that only paints: under its paint, the picture under it
    // (black when it plays alone), as a look over every GIF would want.
    if top.is_none() && layers.is_empty() && p.draw.iter().any(|d| matches!(d, DItem::Paint(..))) {
        let args = [
            p.input(Input::Ur)?,
            p.input(Input::Ug)?,
            p.input(Input::Ub)?,
        ];
        try_push(
            &mut layers,
            Colour {
                code: code::OUT_RGB,
                args,
                pal: 0,
                alpha: None,
                blend: fixed::BLEND_OVER,
                scale: None,
            },
        )?;
        top = p.drawn;
    }
    let (Some(ct), false) = (top, layers.is_empty()) else {
        let at = src.len();
        let msg = if p.locals.is_empty() && p.funcs.is_empty() {
            "empty: write a colour, e.g. `hsv(x + t/4, 1, 1)`"
        } else {
            "the last line should be the colour, e.g. `rgb(r, g, b)`, `hsv(h, s, v)` or `pal(fire, k)`"
        };
        return Err(Error::new(at, at, msg));
    };
    // (A look drawn with a blend or an amount redraws its layers every
    // frame: a slow layer has to cover the whole panel.)
    let slow = if look && as_look(p, prog, &mut layers, (ct.start, ct.end))? {
        &[][..]
    } else {
        &body.rates[..]
    };
    let frame_values = p.locals.iter().filter(|l| l.in_frame).count();
    // `ink` with nothing drawn: always a mistake (unlike `ur` alone).
    if p.drawn.is_none()
        && p.dag
            .nodes
            .iter()
            .any(|m| matches!(m.code, code::INK | code::INKN))
    {
        let t = (0..p.toks.len())
            .map(|k| p.toks[k].get())
            .find(|&t| t.tok == Tok::Name && p.text(t) == "ink")
            .unwrap_or(ct);
        return Err(Error::new(
            t.start,
            t.end,
            "`ink` needs a `draw { }` block to read from",
        ));
    }
    prog.tables[..p.tables.len()].copy_from_slice(&p.tables);
    prog.paint[..p.paint.len()].copy_from_slice(&p.paint);
    if p.dag.forever {
        // E905 at the formula's own value that grows and varies across the
        // panel, when one is on the way.
        let locals = &p.locals;
        let named = |dag: &Dag, v: Id| -> Option<Error> {
            let j = locals.iter().rposition(|l| l.id == v).or_else(|| {
                cause(dag, v, &|m| {
                    locals.iter().rposition(|l| l.id == m && dag.grows(m))
                })
                .0
            })?;
            let l = &locals[j];
            // (Bytes, not `&src[..]`: a str slice brings its panic's
            // Debug and Unicode tables, ~4 KB of wasm.)
            let line = src.bytes().take(l.at).filter(|&b| b == b'\n').count() + 1;
            Some(Error::new(
                l.at,
                l.at + l.name.len(),
                format!(
                    "a colour can't take a value that grows without end and changes across the panel, \
                     as `{0}` (line {line}) does: use frac({0}) or min({0}/k, 1)",
                    l.name
                ),
            ))
        };
        for c in &mut layers {
            grown_colour(&mut p.dag, c, (ct.start, ct.end), &named)?;
        }
    }
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
    // Each layer's `rate` (0: every frame), by its index after the drain.
    let mut rates = try_vec(0u8, layers.len())?;
    for &(i, n) in slow {
        if let Some(r) = rates.get_mut(i) {
            *r = n;
        }
    }
    if let Some(k) = layers.iter().rposition(opaque) {
        layers.drain(..k);
        rates.drain(..k);
    }
    let backdrop = !opaque(&layers[0]);
    // `cycle`'s turned palettes: a slot each after the formula's own, one
    // for each palette and turn (E933 past three in all).
    let mut turns: Vec<Id> = Vec::new();
    let turn_of =
        |c: &Colour| (c.code == code::OUT_PAL && c.pal & TURNED != 0).then_some(c.args[1]);
    for c in &layers {
        if let Some(tn) = turn_of(c).filter(|tn| !turns.contains(tn)) {
            turns.push(tn);
        }
    }
    let n_own = p.palettes.iter().filter(|o| o.live.is_some()).count();
    if n_own + turns.len() > USER_PALETTES {
        // E933.
        return Err(Error::new(
            ct.start,
            ct.end,
            format!(
                "a formula holds {USER_PALETTES} palettes that `pal` and `cycle` read: its own ({n_own} here) and one more for each turn `cycle` gives ({} here)",
                turns.len()
            ),
        ));
    }
    for (j, &tn) in turns.iter().enumerate() {
        p.dag.nodes[tn as usize].b = (n_own + j) as Id;
    }
    // The palette store and the tweens.
    let tweens = lay_palettes(p, prog, (ct.start, ct.end))?;
    let mut steps = Vec::with_capacity(layers.len());
    for (k, c) in layers.iter().enumerate() {
        let plain = k == 0 && !backdrop;
        // A scale that is a value over 2, 4, … 2^15 (`floor(q)/8`, a
        // multiply by 1/8): the value, and the division as a shift in the
        // colour step (one step fewer a pixel; the same bytes).
        let (mut scale, mut shift) = (c.scale, 0u8);
        if let Some(sid) = c.scale {
            let m = p.dag.nodes[sid as usize];
            if m.code == code::MUL && m.level == LV_PIXEL {
                for (x, kk) in [(m.a, m.b), (m.b, m.a)] {
                    if let Some(v) = p.dag.konst(kk) {
                        if v > 0 && v < ONE && (v as u32).is_power_of_two() {
                            scale = Some(x);
                            shift = (ONE as u32 / v as u32).trailing_zeros() as u8;
                            break;
                        }
                    }
                }
            }
        }
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
            pal: match turn_of(c) {
                Some(tn) => 5 + p.dag.nodes[tn as usize].b as u8,
                None => c.pal,
            },
            alpha,
            scale,
            shift,
            turn: turn_of(c),
        });
    }
    // `layer rate N { }`: the slow layers, a run from the bottom with one
    // rate, under at least one that moves, covering the panel.
    let n_slow = rates.iter().take_while(|&&r| r > 0).count();
    let rate_err = |msg: &str| Err(Error::new(ct.start, ct.end, String::from(msg)));
    if rates[n_slow..].iter().any(|&r| r > 0) {
        // E941.
        return rate_err("`rate` goes on the bottom layers only: the first layer, or the first few one after another");
    }
    let slow = if n_slow > 0 {
        let n = rates[0];
        if rates[..n_slow].iter().any(|&r| r != n) {
            // E943.
            return rate_err("the slow layers share one `rate`: give each the same number");
        }
        if n_slow == layers.len() {
            // E947.
            return rate_err("a `rate` layer is the slow one under a layer that moves: add a layer above it, or leave `rate` out");
        }
        if backdrop {
            // E948.
            return rate_err("the bottom `rate` layer must cover the whole panel: `layer rate N { … }`, no blend, no coverage");
        }
        let reads = split_slow(&mut p.dag, &mut steps, n_slow).map_err(|e| Error {
            start: ct.start,
            end: ct.end,
            ..e
        })?;
        Some(Slow {
            layers: n_slow,
            rate: n,
            reads,
        })
    } else {
        None
    };
    // What only reading needed goes before the layout: the tokens (the
    // biggest list), the names, the sharing index. Laying out holds a
    // dozen lists of its own; the compile's peak was the two at once.
    p.toks = Vec::new();
    // (The language service names values in its notes after the layout.)
    #[cfg(not(feature = "syntax"))]
    {
        p.locals = Vec::new();
    }
    p.funcs = Vec::new();
    p.tables = Vec::new();
    p.table_names = Vec::new();
    p.sprites = Vec::new();
    p.palettes = Vec::new();
    p.palette_names = Vec::new();
    p.pnodes = Vec::new();
    p.dag.index = Vec::new();
    p.dag.divs = Vec::new();
    // A limit hit while laying out the steps is the formula's as a whole:
    // point at its (top) colour line.
    #[cfg(feature = "syntax")]
    let mut seen = (Vec::new(), Vec::new(), None);
    if cmp {
        fuse_compares(&mut p.dag, &steps, &p.draw, &tweens)?;
    }
    emit(
        &p.dag,
        &steps,
        &p.draw,
        &tweens,
        backdrop,
        runs,
        reuse,
        slow,
        prog,
        #[cfg(feature = "syntax")]
        &mut seen,
    )
    .map_err(|e| Error {
        start: ct.start,
        end: ct.end,
        ..e
    })?;
    #[cfg(feature = "syntax")]
    p.layout_notes(prog, &seen);
    Ok(Info {
        frame_values,
        layers: body.blocks,
    })
}

/// The palette store (`Program::pals`) and the tween steps for the
/// formula's own palettes: each palette of colours `pal` or `cycle` reads
/// in the store slot of its live slot (the frame starts from those), then
/// the palettes the tweens read, a slot each or a half (a palette in a
/// tween's half: its colours over 16), E923 past [`MAX_PALETTES`]; each
/// tween `pal` or `cycle` reads into its live slot, and the tweens those
/// read into working palettes (E939 past [`TWEEN_TEMPS`] at once), in
/// order. A tween inside another is worked out into the outer one's
/// destination when it is its first palette (in place: `own_palettes`
/// reads an entry's sources before it writes it), else into a working
/// palette.
#[inline(never)]
fn lay_palettes(
    p: &Parser<'_>,
    prog: &mut Program,
    at: (usize, usize),
) -> Result<Vec<TweenOp>, Error> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Lay {
        Full(usize),
        Builtin(u8),
        Low(usize),
        High(usize),
    }
    struct L<'a> {
        pals: &'a [OwnPal],
        nodes: &'a [PNode],
        prog: &'a mut Program,
        map: Vec<(Lay, u8)>,
        lo: [bool; MAX_PALETTES],
        hi: [bool; MAX_PALETTES],
        temps: [bool; TWEEN_TEMPS],
        loc: Vec<u8>,
        ops: Vec<TweenOp>,
        at: (usize, usize),
    }
    impl L<'_> {
        fn put(&mut self, lay: Lay) -> Result<u8, Error> {
            if let Some(&(_, s)) = self.map.iter().find(|m| m.0 == lay) {
                return Ok(s);
            }
            let (lo, hi) = match lay {
                Lay::Low(_) => (true, false),
                Lay::High(_) => (false, true),
                _ => (true, true),
            };
            let Some(s) = (0..MAX_PALETTES).find(|&s| !(lo && self.lo[s]) && !(hi && self.hi[s]))
            else {
                // E923.
                return Err(Error::new(
                    self.at.0,
                    self.at.1,
                    format!("a formula can have {MAX_PALETTES} palettes of its own at most"),
                ));
            };
            self.lo[s] |= lo;
            self.hi[s] |= hi;
            let out = &mut self.prog.pals[32 * s..32 * s + 32];
            match lay {
                Lay::Full(j) => {
                    for (o, &v) in out.iter_mut().zip(&self.pals[j].entries) {
                        *o = v as u32 & 0xff_ffff;
                    }
                }
                Lay::Builtin(b) => {
                    for (o, c) in out.iter_mut().zip(crate::tables::PALETTES[b as usize % 5]) {
                        *o = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
                    }
                }
                Lay::Low(j) | Lay::High(j) => {
                    let h = self.pals[j].half.unwrap_or([0; 16]);
                    let o = if lo { &mut out[..16] } else { &mut out[16..] };
                    for (o, &v) in o.iter_mut().zip(&h) {
                        *o = v as u32 & 0xff_ffff;
                    }
                }
            }
            try_push(&mut self.map, (lay, s as u8))?;
            Ok(s as u8)
        }

        fn temp(&mut self) -> Result<u8, Error> {
            let Some(k) = self.temps.iter().position(|&u| !u) else {
                // E939.
                return Err(Error::new(
                    self.at.0,
                    self.at.1,
                    format!("the tweens need more than {TWEEN_TEMPS} palettes worked out at once: nest them less deeply, or tween fewer palettes that only tweens read"),
                ));
            };
            self.temps[k] = true;
            Ok((USER_PALETTES + k) as u8)
        }

        fn free(&mut self, d: u8) {
            if let Some(k) = (d as usize).checked_sub(USER_PALETTES) {
                self.temps[k % TWEEN_TEMPS] = false;
            }
        }

        /// A destination as a source.
        fn src(d: u8) -> u8 {
            if (d as usize) < USER_PALETTES {
                TW_LIVE + d
            } else {
                TW_TEMP + d - USER_PALETTES as u8
            }
        }

        fn leaf(&mut self, s: PSrc, half: u8) -> Result<u8, Error> {
            match s {
                PSrc::Own(j, _) if self.pals[j].tween.is_some() => Ok(self.loc[j]),
                PSrc::Own(j, _) => self.put(match half {
                    TW_LOW => Lay::Low(j),
                    TW_HIGH => Lay::High(j),
                    _ => Lay::Full(j),
                }),
                PSrc::Builtin(b, _) => self.put(Lay::Builtin(b)),
                PSrc::Node(_) => Ok(0),
            }
        }

        /// Node `r` into destination `d` (its entries `half` says).
        fn gen(&mut self, r: u16, d: u8, half: u8) -> Result<(), Error> {
            let m = self.nodes[r as usize];
            let a = match m.p {
                PSrc::Node(k) => {
                    self.gen(k, d, half)?;
                    Self::src(d)
                }
                s => self.leaf(s, half)?,
            };
            let (b, t) = match m.q {
                PSrc::Node(k) => {
                    let t = self.temp()?;
                    self.gen(k, t, half)?;
                    (Self::src(t), Some(t))
                }
                s => (self.leaf(s, half)?, None),
            };
            try_push(
                &mut self.ops,
                TweenOp {
                    d: d | half,
                    a,
                    b,
                    w: m.w,
                },
            )?;
            if let Some(t) = t {
                self.free(t);
            }
            Ok(())
        }
    }
    let pals = &p.palettes[..];
    let n = pals.len();
    let leaves = |parts: [u16; 2], f: &mut dyn FnMut(usize)| {
        let mut stack: Vec<u16> = parts.iter().copied().filter(|&r| r != NO_PART).collect();
        while let Some(r) = stack.pop() {
            let m = p.pnodes[r as usize];
            for s in [m.p, m.q] {
                match s {
                    PSrc::Node(k) => stack.push(k),
                    PSrc::Own(j, _) => f(j),
                    PSrc::Builtin(..) => {}
                }
            }
        }
    };
    // The tweens worked out: those `pal` or `cycle` reads, and those they
    // read; for one of the others, the last that reads it.
    let mut need = try_vec(false, n)?;
    let mut last = try_vec(0usize, n)?;
    for j in (0..n).rev() {
        need[j] |= pals[j].live.is_some();
        if let (true, Some(parts)) = (need[j], pals[j].tween) {
            leaves(parts, &mut |k| {
                need[k] = true;
                last[k] = last[k].max(j);
            });
        }
    }
    let mut l = L {
        pals,
        nodes: &p.pnodes,
        prog,
        map: Vec::new(),
        lo: [false; MAX_PALETTES],
        hi: [false; MAX_PALETTES],
        temps: [false; TWEEN_TEMPS],
        loc: try_vec(0u8, n)?,
        ops: Vec::new(),
        at,
    };
    // The live palettes of colours first: in their own slots.
    for (j, o) in pals.iter().enumerate() {
        if let (None, Some(s)) = (o.tween, o.live) {
            let s = s as usize % MAX_PALETTES;
            (l.lo[s], l.hi[s]) = (true, true);
            for (d, &v) in l.prog.pals[32 * s..32 * s + 32].iter_mut().zip(&o.entries) {
                *d = v as u32 & 0xff_ffff;
            }
            try_push(&mut l.map, (Lay::Full(j), s as u8))?;
        }
    }
    for j in 0..n {
        let (true, Some(parts)) = (need[j], pals[j].tween) else {
            continue;
        };
        let d = match pals[j].live {
            Some(s) => s,
            None => l.temp()?,
        };
        for (k, &r) in parts.iter().enumerate() {
            if r == NO_PART {
                continue;
            }
            let half = match (k, parts[1] == NO_PART) {
                (_, true) => 0,
                (0, _) => TW_LOW,
                _ => TW_HIGH,
            };
            l.gen(r, d, half)?;
        }
        l.loc[j] = L::src(d);
        // The working palettes of tweens this was the last to read.
        for k in 0..j {
            if need[k] && last[k] == j && pals[k].tween.is_some() && pals[k].live.is_none() {
                let s = l.loc[k];
                l.free(s - TW_TEMP + USER_PALETTES as u8);
            }
        }
    }
    Ok(l.ops)
}

/// The formula as its own `look` line asks to be drawn over a GIF (`look
/// screen 40`: its colour put on with that blend, covering that much):
/// the player's look program. A formula with no `look` line, or `look`
/// (over, 100), is drawn as it is.
pub fn compile_look(src: &str, prog: &mut Program) -> Result<Info, Error> {
    compile_full(
        src,
        prog,
        (true, false, true, false, true, true),
        None,
        #[cfg(feature = "syntax")]
        None,
    )
}

/// [`compile_look`] with (`runs`) or without the runs and (`narrow`) or
/// without the narrowed steps, as [`compile_with`]: the estimate's tests.
#[doc(hidden)]
pub fn compile_look_with(
    src: &str,
    prog: &mut Program,
    runs: bool,
    narrow: bool,
) -> Result<Info, Error> {
    compile_full(
        src,
        prog,
        (runs, narrow, true, true, true, true),
        None,
        #[cfg(feature = "syntax")]
        None,
    )
}

/// [`compile_look`]'s blend and amount, put on the formula's colours (a
/// formula with layers goes over only: [`declarations`] refused another).
#[inline(never)]
fn as_look(
    p: &mut Parser<'_>,
    prog: &Program,
    layers: &mut [Colour],
    at: (usize, usize),
) -> Result<bool, Error> {
    let Some((blend, amount)) = prog.meta().look().filter(|&(b, a)| b != "over" || a < 100) else {
        return Ok(false);
    };
    let mode = BLENDS
        .iter()
        .find(|e| e.0 == blend)
        .map_or(fixed::BLEND_OVER, |e| e.1);
    let k = if amount < 100 {
        Some(p.dag.constant((i32::from(amount) * ONE + 50) / 100, at)?)
    } else {
        None
    };
    for c in layers {
        if mode != fixed::BLEND_OVER {
            c.blend = mode;
        }
        if let Some(k) = k {
            c.alpha = Some(match c.alpha {
                Some(a) => p.dag.op(code::MUL, [a, k, 0], at)?,
                None => k,
            });
        }
    }
    Ok(true)
}

/// [`compile_into`], telling `sink` what the compiler saw (the language
/// service's hook: [`crate::syntax`]). Also on an error, up to it.
#[cfg(feature = "syntax")]
pub fn compile_observed<'s>(
    src: &'s str,
    prog: &mut Program,
    sink: &'s mut (dyn crate::syntax::Sink + 's),
) -> Result<Info, Error> {
    compile_full(
        src,
        prog,
        (true, true, true, true, false, true),
        None,
        Some(sink),
    )
}

#[cfg(feature = "syntax")]
impl<'s> Parser<'s> {
    /// A name read at `t`: where it is defined, if the formula set it.
    #[inline(never)]
    fn note_read(&mut self, t: Token) {
        let name = self.text(t);
        let def = self
            .locals
            .iter()
            .rev()
            .find(|l| l.name == name && self.visible(l.scope))
            .map(|l| l.at)
            .or(self.func_named(name).map(|k| self.funcs[k].at))
            .or(self.table_named(name).map(|k| self.table_names[k].at))
            .or(self.sprite_named(name).map(|k| self.sprites[k].at))
            .or(self.palette_named(name).map(|k| self.palette_names[k].at));
        self.reads.push((t.start, t.end, def.unwrap_or(usize::MAX)));
    }

    /// Whether a name set in `scope` is seen from `from` (as
    /// [`visible`](Self::visible) from the scope being read).
    fn seen_from(&self, mut from: u16, scope: u16) -> bool {
        loop {
            if from == scope {
                return true;
            }
            let (parent, own) = self.scopes[from as usize];
            if own || from == 0 {
                return false;
            }
            from = parent;
        }
    }

    /// Tell the sink what was read (after the formula, or up to an error).
    fn observe(&mut self) {
        use crate::syntax::{Defined, Level};
        let Some(sink) = self.sink.take() else {
            return;
        };
        let level = |l: u8| match l {
            LV_CONST => Level::Constant,
            LV_FRAME => Level::Frame,
            LV_LOOP1 | LV_LOOP2 => Level::Draw,
            LV_ROW => Level::Row,
            _ => Level::Pixel,
        };
        let word = |p: &Self, at: usize| {
            let b = p.src.as_bytes();
            let mut e = at;
            while e < b.len() && (b[e].is_ascii_alphanumeric() || b[e] == b'_') {
                e += 1;
            }
            (at, e)
        };
        for &(a, e) in &self.decls_seen {
            sink.declaration(sub(self.src, a, e), (a, e));
        }
        // A habit from other languages: radians in a wave (`sin(x*6.28)`).
        self.radians_notes(&mut *sink);
        for (i, l) in self.locals.iter().enumerate() {
            let kind = if l.counter {
                Defined::Counter
            } else {
                Defined::Value
            };
            let lv = self.dag.level(l.id);
            sink.defined(l.name, word(self, l.at), kind, l.scope, level(lv));
            if l.counter || lv == LV_CONST {
                continue;
            }
            // The nearest value set before it (and seen from it) that makes
            // it vary as it does, else the built-in.
            let named = |m: Id| {
                self.locals[..i]
                    .iter()
                    .rposition(|e| e.id == m && self.seen_from(l.scope, e.scope))
            };
            let via = match named(l.id) {
                Some(j) => Some(j),
                None => cause(&self.dag, l.id, &named).0,
            };
            let root = cause(&self.dag, l.id, &|_| None).1;
            let n = &self.dag.nodes[root as usize];
            let input = if n.code == K_INPUT || (code::U_R..=code::INKN).contains(&n.code) {
                input_word(n).0
            } else {
                ""
            };
            sink.why(
                word(self, l.at),
                via.map(|j| word(self, self.locals[j].at)),
                input,
            );
        }
        for f in &self.funcs {
            sink.defined(f.name, word(self, f.at), Defined::Function, 0, Level::Pixel);
        }
        for t in &self.table_names {
            sink.defined(
                t.name,
                word(self, t.at),
                Defined::Table,
                t.scope,
                Level::Constant,
            );
        }
        for t in &self.sprites {
            sink.defined(
                t.name,
                word(self, t.at),
                Defined::Sprite,
                t.scope,
                Level::Constant,
            );
        }
        for t in &self.palette_names {
            sink.defined(
                t.name,
                word(self, t.at),
                Defined::Palette,
                t.scope,
                Level::Constant,
            );
        }
        for &(a, e, d) in &self.reads {
            sink.read((a, e), (d != usize::MAX).then(|| word(self, d)));
        }
        for &(id, a, e) in &self.dag.made {
            let n = self.dag.nodes[id as usize];
            if n.code < K_CONST {
                sink.op(
                    (a, e),
                    level(n.level),
                    n.code,
                    crate::program::value_ns(n.code),
                );
            }
        }
        self.sink = Some(sink);
    }
}

/// The declarations' end: E965 (a part with layers can only go over, so a
/// formula with layers can only be a look over the GIF), the program's
/// [`Meta`], and where the text is for [`compile_declared`]. Out of line:
/// `compile_inner`'s frame is under the parser's deepest.
#[inline(never)]
fn declarations(
    decl: &Decls,
    layered: bool,
    prog: &mut Program,
    declared: Option<&mut Declared>,
) -> Result<(), Error> {
    if layered && decl.meta.look().is_some_and(|(b, _)| b != "over") {
        let at = decl.text.look.map_or(0, |l| l.0 as usize);
        return Err(Error::new(
            at,
            at + 4,
            "a formula with layers goes over the GIF as a look: write `look` or `look over N`",
        ));
    }
    prog.meta = decl.meta;
    if let Some(d) = declared {
        *d = decl.text;
        d.own = decl.own;
    }
    Ok(())
}

/// A colour's values that grow without end (a `loop forever` formula's
/// `t`, `gray(t/60)`): an hsv's hue by its fraction (the hue is a turn),
/// the others held to 0..1 (the colour step clamps them so anyway); a
/// coverage, a palette's scale or a value with a part per pixel can't
/// (E905).
fn grown_colour(
    dag: &mut Dag,
    c: &mut Colour,
    at: (usize, usize),
    named: &dyn Fn(&Dag, Id) -> Option<Error>,
) -> Result<(), Error> {
    // Held to 0..1, as the colour step and the coverage hold it anyway.
    let unit = |dag: &mut Dag, v: Id| -> Result<Id, Error> {
        if !dag.is_wide(v) {
            return Err(named(dag, v).unwrap_or_else(|| Error::new(at.0, at.1, GROWN_COLOUR)));
        }
        let (z, one) = (dag.constant(0, at)?, dag.constant(ONE, at)?);
        let lo = dag.op(code::MAX, [v, z, 0], at)?;
        dag.op(code::MIN, [lo, one, 0], at)
    };
    let n = code::arity(c.code);
    for k in 0..n {
        let v = c.args[k];
        if !dag.grows(v) {
            continue;
        }
        c.args[k] = if c.code == code::OUT_HSV && k == 0 {
            dag.op(code::FRAC, [v, 0, 0], at)?
        } else {
            unit(dag, v)?
        };
    }
    if let Some(a) = c.alpha.filter(|&a| dag.grows(a)) {
        c.alpha = Some(unit(dag, a)?);
    }
    if let Some(k) = c.scale.filter(|&k| dag.grows(k)) {
        return Err(named(dag, k).unwrap_or_else(|| Error::new(at.0, at.1, GROWN_COLOUR)));
    }
    Ok(())
}

/// E922: a palette's colours.
const PALETTE_COLOURS: &str = "a palette is 1 to 32 colours: `palette sea = #003, #06c, #fff`";

/// E912/E913: a sprite's key.
const SPRITE_KEY: &str =
    "a sprite starts with its key: `key . 0, R 1, D 2` (each character, then the number it reads as)";
const SPRITE_KEY_ENTRY: &str =
    "a key entry is a character (not ; { } , - / or a space) and its number, `R 1`, after a comma";

/// E901 for a growing value with a part per pixel or row.
const GROWS_SPLIT: &str = "a growing value plus a per-pixel or per-row part only goes into frac, \
the waves, noise, % or a multiply: as frac(t*k) + x or sin(t*k + x)";

/// E905.
const GROWN_COLOUR: &str =
    "a colour can't take a value that grows without end and changes across the panel: \
use frac(…) of it, or min(t/k, 1)";

/// A colour step to lay out: its code (plain or a layer's), its values,
/// the palette and the coverage (a layer's).
struct Step {
    code: u8,
    args: [Id; 3],
    pal: u8,
    alpha: Option<Id>,
    /// `pal(P, v) * k`'s k, and a shift its value is divided by (`k = x/8`:
    /// x, 3).
    scale: Option<Id>,
    shift: u8,
    /// `cycle`'s turn of its palette (a row step, `code::TURN`).
    turn: Option<Id>,
}

impl Step {
    /// What it reads, in operand order (a palette's scale, then the
    /// coverage), then the turn of its palette (not an operand: a row step
    /// before it).
    fn reads(&self) -> impl Iterator<Item = Id> + '_ {
        self.args[..code::arity(self.code)]
            .iter()
            .copied()
            .chain(self.scale)
            .chain(self.alpha)
            .chain(self.turn)
    }
}

/// A comparison node as `x op y`, op `LT` or `LE` (`a > b` is `b < a`,
/// `step(e, x)` is `e <= x`), or `None` for any other node.
fn as_lt_le(m: &Node) -> Option<(u8, Id, Id)> {
    match m.code {
        code::LT => Some((code::LT, m.a, m.b)),
        code::GT => Some((code::LT, m.b, m.a)),
        code::LE => Some((code::LE, m.a, m.b)),
        code::GE => Some((code::LE, m.b, m.a)),
        // step(e, x): x >= e
        code::STEP => Some((code::LE, m.a, m.b)),
        _ => None,
    }
}

/// A comparison node's two readings as `s op t` (op `LT LE GT GE`): with
/// its first operand as `s`, and with its second.
fn as_sided(m: &Node) -> Option<[(u8, Id, Id); 2]> {
    use code::*;
    let (a, b) = (m.a, m.b);
    Some(match m.code {
        LT => [(LT, a, b), (GT, b, a)],
        GT => [(GT, a, b), (LT, b, a)],
        LE => [(LE, a, b), (GE, b, a)],
        GE => [(GE, a, b), (LE, b, a)],
        // step(e, x): x >= e
        STEP => [(LE, a, b), (GE, b, a)],
        _ => return None,
    })
}

/// P0b-4: a comparison only one step reads, fused into that step, so the
/// two are one step (one less against the cap, and one loop or a short
/// branch instead of a compare and a multiply or an add):
///
/// - `cmp * v`, `v * cmp`, `cmp ? v : 0`, `cmp ? 0 : v` → `SEL_LT`/`SEL_LE`;
/// - `cmp1 + cmp2` of one value against two (`(v >= lo) + (v >= hi)`) →
///   `CNT_*`.
///
/// Exact: `mul(1, v)` is `v` and `mul(0, v)` is 0 (neither saturates), and
/// the count is the sum of two 0s or 1s. The comparison stays a node, read
/// by nothing (`emit` leaves it out). Only where the comparison is worked
/// out as often as the step that reads it (a row's comparison read per
/// pixel stays a row step: once a row against a test a pixel), and never
/// across scenes.
#[inline(never)]
fn fuse_compares(
    dag: &mut Dag,
    steps: &[Step],
    draw: &[DItem],
    tweens: &[TweenOp],
) -> Result<(), Error> {
    let n = dag.nodes.len();
    // What is live, and how many live steps read each node.
    let mut live = try_vec(false, n)?;
    let mut uses = try_vec(0u16, n)?;
    let root = |x: Id, live: &mut Vec<bool>, uses: &mut Vec<u16>| {
        if let Some(l) = live.get_mut(x as usize) {
            *l = true;
            uses[x as usize] = uses[x as usize].saturating_add(1);
        }
    };
    for st in steps {
        for x in st.reads() {
            root(x, &mut live, &mut uses);
        }
    }
    for item in draw {
        let one;
        let ids: &[Id] = match item {
            DItem::Line(v) => v,
            DItem::Dot(v) => v,
            DItem::Paint(_, v, k) => &v[..*k as usize],
            DItem::If(c) | DItem::For(c, _) => {
                one = [*c];
                &one
            }
            _ => &[],
        };
        for &x in ids {
            root(x, &mut live, &mut uses);
        }
    }
    for tw in tweens {
        root(tw.w, &mut live, &mut uses);
    }
    for k in 0..dag.conds.len() {
        let c = dag.conds[k];
        root(c, &mut live, &mut uses);
    }
    for i in (0..n).rev() {
        if !live[i] || dag.nodes[i].code >= K_CONST {
            continue;
        }
        let xs: Vec<Id> = reads(dag, i).collect();
        for x in xs {
            live[x as usize] = true;
            uses[x as usize] = uses[x as usize].saturating_add(1);
        }
    }
    let sc: Vec<u8> = (0..n).map(|i| dag.scene_of(i)).collect();
    // What the column run and the still cache could take: a comparison
    // they would work out once a column (or keep) is not fused into a step
    // they would not (crt's channel masks: once a column, against a
    // compare a pixel).
    let (col, still) = (column_nodes(dag), still_nodes(dag));
    let apart = |x: Id, y: Id| {
        let (x, y) = (x as usize, y as usize);
        (col[x] && !col[y]) || (still[x] && !still[y])
    };
    // A comparison `x` the step `i` may take in: read once (by `i`), as
    // often as `i`, in `i`'s scene or none.
    let takes = |dag: &Dag, i: usize, x: Id| {
        let x = x as usize;
        x < i
            && uses[x] == 1
            && dag.nodes[x].level == dag.nodes[i].level
            && (sc[x] == 0 || sc[x] == sc[i])
            && as_lt_le(&dag.nodes[x]).is_some()
    };
    for i in 0..n {
        if !live[i] {
            continue;
        }
        let m = dag.nodes[i];
        let zero = |x: Id| dag.konst(x) == Some(0);
        let sel = match m.code {
            code::MUL if takes(dag, i, m.a) => Some((m.a, m.b, false)),
            code::MUL if takes(dag, i, m.b) => Some((m.b, m.a, false)),
            code::SELECT if zero(m.c) && takes(dag, i, m.a) => Some((m.a, m.b, false)),
            code::SELECT if zero(m.b) && takes(dag, i, m.a) => Some((m.a, m.c, true)),
            _ => None,
        };
        if let Some((cmp, v, not)) = sel {
            let Some((op, x, y)) = as_lt_le(&dag.nodes[cmp as usize]) else {
                continue;
            };
            if apart(cmp, v) {
                continue;
            }
            // not (x < y) is y <= x; not (x <= y) is y < x.
            let (code, a, b) = match (op, not) {
                (code::LT, false) => (code::SEL_LT, x, y),
                (_, false) => (code::SEL_LE, x, y),
                (code::LT, true) => (code::SEL_LE, y, x),
                (_, true) => (code::SEL_LT, y, x),
            };
            dag.nodes[i] = Node {
                code,
                a,
                b,
                c: v,
                val: 0,
                ..m
            };
            live[cmp as usize] = false;
            continue;
        }
        if m.code == code::ADD
            && m.a != m.b
            && takes(dag, i, m.a)
            && takes(dag, i, m.b)
            && !apart(m.a, m.b)
            && !apart(m.b, m.a)
        {
            let (Some(p), Some(q)) = (
                as_sided(&dag.nodes[m.a as usize]),
                as_sided(&dag.nodes[m.b as usize]),
            ) else {
                continue;
            };
            let found = p.iter().find_map(|&(o1, s1, t1)| {
                q.iter()
                    .find(|&&(o2, s2, _)| o2 == o1 && s2 == s1)
                    .map(|&(_, _, t2)| (o1, s1, t1, t2))
            });
            if let Some((op, s, t1, t2)) = found {
                let code = match op {
                    code::LT => code::CNT_LT,
                    code::LE => code::CNT_LE,
                    code::GT => code::CNT_GT,
                    _ => code::CNT_GE,
                };
                live[m.a as usize] = false;
                live[m.b as usize] = false;
                dag.nodes[i] = Node {
                    code,
                    a: s,
                    b: t1,
                    c: t2,
                    val: 0,
                    ..m
                };
            }
        }
    }
    Ok(())
}

/// A formula's slow layers (`layer rate N { }`): the first `layers`
/// colour steps, redrawn every `rate`th frame, reading what `reads` says
/// (`crate::rate::READS_*`).
#[derive(Clone, Copy)]
struct Slow {
    layers: usize,
    rate: u8,
    reads: u8,
}

/// Give the slow layers (the first `n` steps) pixel values of their own:
/// one the layers above read too is copied for them (the fast part runs
/// without the slow one's steps, the slow part without the fast one's).
/// What they read, transitively (`rate::READS_*`); E944 if they read the
/// canvas or the picture under.
#[inline(never)]
#[cfg_attr(target_arch = "xtensa", optimize(size))]
fn split_slow(dag: &mut Dag, steps: &mut [Step], n: usize) -> Result<u8, Error> {
    let len = dag.nodes.len();
    // Who reads each node: bit 0 the slow layers, bit 1 the others.
    let mut by = try_vec(0u8, len)?;
    for (k, st) in steps.iter().enumerate() {
        let b = if k < n { 1 } else { 2 };
        for x in st.reads() {
            by[x as usize] |= b;
        }
    }
    for i in (0..len).rev() {
        let b = by[i];
        if b != 0 && dag.nodes[i].code < K_CONST {
            for x in reads(dag, i) {
                by[x as usize] |= b;
            }
        }
    }
    // Copies, in node order (operands first): a pixel value both read is
    // copied for the slow layers, and so is one only they read that reads
    // a copy.
    let mut what = 0u8;
    let mut map = try_vec(NO, len)?;
    for i in 0..len {
        if by[i] & 1 == 0 {
            continue;
        }
        let mut c = dag.nodes[i];
        if (code::U_R..=code::INKN).contains(&c.code) || c.code == code::UAT {
            // E944.
            return Err(Error::new(
                0,
                0,
                "a `rate` layer is drawn on its own: it can't read `ink`, `ur`, `ug`, `ub` or `ul`",
            ));
        }
        if c.code == K_INPUT {
            what |= match c.val {
                WIDE_T | 0 => crate::rate::READS_T,
                1..=3 => crate::rate::READS_CLOCK,
                4..=6 => crate::rate::READS_SLIDERS,
                v if v == S_SEED as i32 => crate::rate::READS_SEED,
                _ => 0,
            };
            continue;
        }
        if c.level != LV_PIXEL || c.code >= K_CONST {
            continue;
        }
        let mut moved = false;
        for x in [&mut c.a, &mut c.b, &mut c.c]
            .into_iter()
            .take(code::arity(c.code))
        {
            if map[*x as usize] != NO {
                *x = map[*x as usize];
                moved = true;
            }
        }
        // (A node only the slow layers read, of a copy: copied too, after
        // it — every node after the nodes it reads.)
        if by[i] & 2 == 0 && !moved {
            continue;
        } else if dag.nodes.len() >= MAX_NODES {
            return Err(Error::new(
                0,
                0,
                "this formula is too long for the panel — simplify it",
            ));
        } else {
            try_push(&mut dag.nodes, c)?;
            map[i] = (dag.nodes.len() - 1) as Id;
        }
    }
    for st in &mut steps[..n] {
        let ar = code::arity(st.code);
        for x in st.args[..ar]
            .iter_mut()
            .chain(st.scale.as_mut())
            .chain(st.alpha.as_mut())
        {
            if map[*x as usize] != NO {
                *x = map[*x as usize];
            }
        }
    }
    Ok(what)
}

/// Lay the reachable nodes out as steps: scalar slots for the constants
/// and the per-frame and per-row values, lane registers for the per-pixel
/// ones (linear scan: a register is free again after its value's last
/// use, and a step never writes a register it reads). The per-pixel values
/// go in the order the layers need them: each layer's colour step right
/// after the values it is the first to read. The draw stage, if something
/// reads `ink`, goes between the per-frame and the per-row steps in the
/// order it is written: each loop's values (those that depend on its
/// counter) at the top of its body.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn emit(
    dag: &Dag,
    steps: &[Step],
    draw: &[DItem],
    tweens: &[TweenOp],
    backdrop: bool,
    runs: bool,
    reuse: bool,
    slow: Option<Slow>,
    prog: &mut Program,
    #[cfg(feature = "syntax")] seen: &mut (Vec<usize>, Vec<usize>, Option<&'static str>),
) -> Result<(), Error> {
    let nodes = &dag.nodes;
    let n = nodes.len();
    // (E402 tells the steps of each section: where to look.)
    let too_long = |steps: usize, [f, d, r, p]: [usize; 4]| {
        Error::new(
            0,
            0,
            format!(
                "this formula is too long for the panel: {steps} steps (per frame {f}, draw {d}, per row {r}, per pixel {p}), the limit is {MAX_STEPS}"
            ),
        )
    };
    // Reachability, and the first colour step each value is needed for
    // (children always precede their parents).
    const NONE: usize = usize::MAX;
    let mut need = try_vec(NONE, n)?;
    for (k, st) in steps.iter().enumerate() {
        for a in st.reads() {
            need[a as usize] = need[a as usize].min(k);
        }
    }
    let close = |need: &mut [usize]| {
        for i in (0..n).rev() {
            let m = &nodes[i];
            if need[i] == NONE || m.code >= K_CONST {
                continue;
            }
            for x in reads(dag, i) {
                need[x as usize] = need[x as usize].min(need[i]);
            }
        }
    };
    close(&mut need);
    // The draw stage only when something shown reads `ink` (else, like a
    // value nothing uses, it is left out).
    let inked = (0..n).any(|i| need[i] != NONE && matches!(nodes[i].code, code::INK | code::INKN));
    let painted = draw.iter().any(|d| matches!(d, DItem::Paint(..)));
    let draw = if inked || painted { draw } else { &[] };
    for item in draw {
        let one;
        let ids: &[Id] = match item {
            DItem::Line(v) => v,
            DItem::Dot(v) => v,
            DItem::Paint(_, v, n) => &v[..*n as usize],
            DItem::If(c) => {
                one = [*c];
                &one
            }
            _ => &[],
        };
        for &x in ids {
            need[x as usize] = 0;
        }
    }
    // The tweens' weights (once a frame, before the palettes).
    for tw in tweens {
        need[tw.w as usize] = 0;
    }
    close(&mut need);
    // Each node's scene (row and pixel values only), and each scene's
    // `when` where the scene has a step.
    let sc: Vec<u8> = (0..n).map(|i| dag.scene_of(i)).collect();
    let mut guarded = [[false; 2]; MAX_SCENES + 1];
    for i in 0..n {
        if sc[i] != 0 && need[i] != NONE && nodes[i].code < K_CONST {
            guarded[sc[i] as usize][usize::from(nodes[i].level == LV_PIXEL)] = true;
        }
    }
    for (k, &c) in dag.conds.iter().enumerate() {
        if guarded[k + 1] != [false; 2] {
            need[c as usize] = 0;
        }
    }
    close(&mut need);
    let n_guards: usize = guarded.iter().flatten().filter(|&&g| g).count();
    if inked {
        prog.uses |= INKED;
    }
    if painted {
        prog.uses |= PAINTS;
    }
    let live = |i: usize| need[i] != NONE;
    // A value that grows without end with a part per pixel, where nothing
    // took it apart (E901).
    if (0..n).any(|i| live(i) && nodes[i].code == K_SPLIT) {
        return Err(Error::new(0, 0, GROWS));
    }
    let is_op = |i: usize| live(i) && nodes[i].code < K_CONST;
    let control: usize = draw
        .iter()
        .map(|d| match d {
            DItem::Line(_) => 2,
            _ => 1,
        })
        .sum();
    let total = (0..n).filter(|&i| is_op(i)).count()
        + steps.len()
        + control
        + usize::from(slow.is_some())
        + tweens.len()
        + n_guards;
    // The steps of each section (per frame, the draw stage, per row, per
    // pixel with the colours), as `Program::counts` gives them.
    let sections = |markers: usize| {
        let mut sec = [0usize; 4];
        for i in (0..n).filter(|&i| is_op(i)) {
            sec[match nodes[i].level {
                LV_FRAME => 0,
                LV_LOOP1 | LV_LOOP2 => 1,
                LV_ROW => 2,
                _ => 3,
            }] += 1;
        }
        sec[0] += tweens.len();
        sec[2] += guarded.iter().filter(|g| g[0]).count();
        sec[3] += guarded.iter().filter(|g| g[1]).count();
        sec[1] += control;
        sec[3] += steps.len() + usize::from(slow.is_some()) + markers;
        sec
    };
    if total > MAX_STEPS {
        return Err(too_long(total, sections(0)));
    }
    // Where each node's value lives: a slot of `vs` (`VS` + its index,
    // given a scalar slot once the steps are laid out: `assign_slots`), or
    // as it is (an input, a lane, a wide slot, a noise's offset).
    let mut loc = try_vec(0u16, n)?;
    // A slot for each constant and each value a frame, a row or a loop
    // works out, two for each loop and one for each pick's table (at most:
    // a wide value or a noise's offset has its place elsewhere), reserved
    // at once: grown by doubling it was the 4 KB the device couldn't find
    // (2026-10-29).
    let n_for = draw.iter().filter(|d| matches!(d, DItem::For(..))).count();
    let mut vs: Vec<VSlot> = try_with_capacity(
        (0..n)
            .filter(|&i| {
                live(i) && (nodes[i].code == K_CONST || (is_op(i) && nodes[i].level != LV_PIXEL))
            })
            .count()
            + 2 * n_for
            + dag.picks.len(),
    )?;
    // The picks' tables' choices (`VSlot::Pick`'s index here).
    let mut ch: Vec<Vec<u16>> = Vec::new();
    // Which nodes have a place yet: a step may read only those (an operand
    // without one would read slot 0, `t`, silently).
    let mut set = try_vec(false, n)?;
    let mut next_slot = S_FIRST_FREE as usize;
    // `seed` has the first free slot; the wide noises' offsets the slots
    // after it (`noise_slot`; `seed`'s kept for it then).
    if (0..n).any(|i| live(i) && nodes[i].code == K_INPUT && nodes[i].val == S_SEED as i32) {
        prog.uses |= SEEDED;
        next_slot += 1;
    }
    let nctx = (0..n)
        .filter(|&i| is_op(i) && is_noise_o(nodes[i].code))
        .map(|i| (nodes[i].code - code::NOISE_O) as usize + 1)
        .max()
        .unwrap_or(0);
    if nctx > 0 {
        next_slot = noise_slot(nctx, 0);
    }
    // Wide slots: 0 is `t`.
    let mut next_wide = 1usize;
    fn new_slot(vs: &mut Vec<VSlot>, v: Option<i32>) -> Result<u16, Error> {
        try_push(
            vs,
            match v {
                Some(v) => VSlot::Const(v),
                None => VSlot::Value,
            },
        )?;
        Ok(VS + (vs.len() - 1) as u16)
    }
    for i in 0..n {
        if live(i) && nodes[i].code == K_CONST {
            loc[i] = new_slot(&mut vs, Some(nodes[i].val))?;
            set[i] = true;
        } else if live(i) && nodes[i].code == K_INPUT {
            loc[i] = if nodes[i].val == WIDE_T {
                0
            } else {
                nodes[i].val as u8 as u16
            };
            set[i] = true;
            if nodes[i].val as u8 == lane(L_I) {
                prog.uses |= USES_I;
            }
        } else if is_op(i)
            && ((code::U_R..=code::U_L).contains(&nodes[i].code) || nodes[i].code == code::UAT)
        {
            prog.uses |= UNDER;
        }
    }
    if backdrop {
        prog.uses |= BACKDROP;
    }
    // A `rate` formula's fast part draws over the held picture, which it
    // reads as the picture under: none of it may read what is under (E944).
    if slow.is_some() && prog.uses & UNDER != 0 {
        return Err(Error::new(
            0,
            0,
            "a formula with `rate` draws its own picture: it can't read `ur`, `ug`, `ub` or `ul`",
        ));
    }
    let mut ops: Vec<WOp> = try_with_capacity(total + 2)?;
    // A step for node `m` into `d`; a pick's table of its choices' slots
    // goes in constant slots of its own, one for every pick of the same
    // choices (`tabs`: where each went, 0 for not yet; the choices all
    // have a slot by then: they come before it and run once a frame or a
    // row).
    let mut tabs = alloc::vec![0u16; dag.picks.len()];
    let mk = |m: &Node,
              d: u16,
              loc: &[u16],
              set: &[bool],
              vs: &mut Vec<VSlot>,
              ch: &mut Vec<Vec<u16>>,
              tabs: &mut [u16]|
     -> Result<WOp, Error> {
        let n = code::arity(m.code);
        let unset = [m.a, m.b, m.c][..n].iter().any(|&x| !set[x as usize])
            || (m.code == code::PICK && dag.picks[m.b as usize].iter().any(|&x| !set[x as usize]))
            || (is_noise_o(m.code) && dag.deps[m.val as usize].iter().any(|&x| !set[x as usize]));
        if unset {
            return Err(Error::new(0, 0, String::from(LAYOUT_BUG)));
        }
        let l = |k: usize, x: Id| if n > k { loc[x as usize] } else { 0 };
        let (mut b, mut c) = (l(1, m.b), l(2, m.c));
        if m.code == code::TAB || m.code == code::PALIDX {
            (b, c) = (m.b, m.c);
        }
        if matches!(m.code, code::W_CMP | code::W_MINMAX | code::SPR | code::UAT) {
            c = m.c;
        }
        if m.code == code::PICK {
            let opts = &dag.picks[m.b as usize];
            let tab = &mut tabs[m.b as usize];
            if *tab == 0 {
                ch.push(opts.iter().map(|&o| loc[o as usize]).collect());
                try_push(vs, VSlot::Pick((ch.len() - 1) as u16))?;
                *tab = VS + (vs.len() - 1) as u16;
            }
            (b, c) = (*tab, opts.len() as u16);
        }
        Ok(WOp {
            code: m.code,
            d,
            a: l(0, m.a),
            b,
            c,
        })
    };
    // Each loop's own values: those whose deepest counter is its (a value
    // at a loop's level takes the loop of its operand at that level).
    let mut owner = try_vec(u16::MAX, n)?;
    for i in 0..n {
        let m = &nodes[i];
        if m.code == K_COUNTER {
            owner[i] = m.val as u16;
        } else if m.code < K_CONST && (m.level == LV_LOOP1 || m.level == LV_LOOP2) {
            for &x in [m.a, m.b, m.c][..code::arity(m.code)].iter() {
                if nodes[x as usize].level == m.level {
                    owner[i] = owner[x as usize];
                    break;
                }
            }
        }
    }
    // A scene's row steps (together in node order: it made them in a run)
    // after its guard; how many it skips, once they are laid out.
    let mut open: Option<(usize, u8)> = None;
    let close_guard = |ops: &mut [WOp], open: &mut Option<(usize, u8)>| -> Result<(), Error> {
        if let Some((g, _)) = open.take() {
            let n = ops.len() - g - 1;
            if n > 255 {
                return Err(Error::new(0, 0, format!("this formula is too long for the panel: a scene takes {n} steps a row, the limit is 255")));
            }
            ops[g].b = n as u16;
        }
        Ok(())
    };
    for level in [LV_FRAME, LV_ROW] {
        for i in 0..n {
            let m = &nodes[i];
            if is_op(i) && m.level == level && m.code != code::TURN {
                if level == LV_ROW && open.map_or(0, |o| o.1) != sc[i] {
                    close_guard(&mut ops, &mut open)?;
                    if sc[i] != 0 {
                        let c = dag.conds[sc[i] as usize - 1];
                        if !set[c as usize] {
                            return Err(Error::new(0, 0, String::from(LAYOUT_BUG)));
                        }
                        open = Some((ops.len(), sc[i]));
                        ops.push(WOp {
                            code: code::GUARD,
                            d: 0,
                            a: loc[c as usize],
                            b: 0,
                            c: u16::from(sc[i]) << 4,
                        });
                    }
                }
                // A wide value has a wide slot; a noise's offset its place.
                let d = if code::is_wide(m.code) && code::wide_ops(m.code).0 {
                    if next_wide >= WIDE {
                        return Err(Error::new(
                            0,
                            0,
                            format!("too many values that grow without end (more than {}): share one, as frac(u*k) of one `u`", WIDE - 1),
                        ));
                    }
                    next_wide += 1;
                    (next_wide - 1) as u16
                } else if m.code == code::W_LAT {
                    noise_slot(m.val as usize / 3, m.val as usize % 3) as u16
                } else {
                    new_slot(&mut vs, None)?
                };
                loc[i] = d;
                set[i] = true;
                ops.push(mk(m, d, &loc, &set, &mut vs, &mut ch, &mut tabs)?);
            }
        }
        if level == LV_FRAME {
            // The tweens, the section's last steps (`Program::tweens`).
            for tw in tweens {
                if !set[tw.w as usize] {
                    return Err(Error::new(0, 0, String::from(LAYOUT_BUG)));
                }
                prog.uses |= OWNPAL;
                ops.push(WOp {
                    code: code::TWEEN,
                    d: u16::from(tw.d),
                    a: u16::from(tw.a),
                    b: u16::from(tw.b),
                    c: loc[tw.w as usize],
                });
            }
            prog.frame_end = ops.len().min(MAX_OPS) as u16;
            // The draw stage: FOR (its END patched in when it comes), the
            // loop's values, its lines and inner loops; END back to just
            // after the FOR.
            let mut open: Vec<(usize, u16, u16)> = Vec::new();
            // `if`: the IF (or the else's JUMP) whose target is the end.
            let mut ifs: Vec<usize> = Vec::new();
            for item in draw {
                match *item {
                    DItem::Paint(kind, v, n) => {
                        let l = |k: usize| loc[v[k.min(n as usize - 1)] as usize];
                        let groups = (n as usize).div_ceil(3);
                        for g in 0..groups {
                            let k = 3 * g;
                            ops.push(WOp {
                                code: if g + 1 == groups {
                                    code::PAINT
                                } else {
                                    code::ARGS
                                },
                                d: if g + 1 == groups { kind as u16 } else { 0 },
                                a: l(k),
                                b: l(k + 1),
                                c: l(k + 2),
                            });
                        }
                    }
                    DItem::If(cond) => {
                        ifs.push(ops.len());
                        ops.push(WOp {
                            code: code::IF,
                            d: 0,
                            a: loc[cond as usize],
                            b: 0,
                            c: 0,
                        });
                    }
                    DItem::Else => {
                        if let Some(at) = ifs.pop() {
                            ifs.push(ops.len());
                            ops.push(WOp {
                                code: code::JUMP,
                                d: 0,
                                a: 0,
                                b: 0,
                                c: 0,
                            });
                            ops[at].b = ops.len() as u16;
                        }
                    }
                    DItem::EndIf => {
                        if let Some(at) = ifs.pop() {
                            ops[at].b = ops.len() as u16;
                        }
                    }
                    DItem::For(ctr, count) => {
                        let cs = new_slot(&mut vs, Some((count as i32) << 16))?;
                        let ks = new_slot(&mut vs, None)?;
                        loc[ctr as usize] = ks;
                        set[ctr as usize] = true;
                        open.push((ops.len(), ks, cs));
                        ops.push(WOp {
                            code: code::FOR,
                            d: ks,
                            a: cs,
                            b: 0,
                            c: 0,
                        });
                        let lp = nodes[ctr as usize].val as u16;
                        for i in 0..n {
                            if is_op(i) && owner[i] == lp {
                                let d = new_slot(&mut vs, None)?;
                                loc[i] = d;
                                set[i] = true;
                                ops.push(mk(
                                    &nodes[i], d, &loc, &set, &mut vs, &mut ch, &mut tabs,
                                )?);
                            }
                        }
                    }
                    DItem::End => {
                        let Some((at, ks, cs)) = open.pop() else {
                            continue;
                        };
                        ops[at].b = ops.len() as u16;
                        ops.push(WOp {
                            code: code::END,
                            d: 0,
                            a: ks,
                            b: cs,
                            c: (at + 1) as u16,
                        });
                    }
                    DItem::Line(v) => {
                        let l = |k: usize| loc[v[k] as usize];
                        ops.push(WOp {
                            code: code::LINE_FROM,
                            d: 0,
                            a: l(0),
                            b: l(1),
                            c: l(4),
                        });
                        ops.push(WOp {
                            code: code::LINE_TO,
                            d: 0,
                            a: l(2),
                            b: l(3),
                            c: l(5),
                        });
                    }
                    DItem::Dot(v) => ops.push(WOp {
                        code: code::DOT,
                        d: 0,
                        a: loc[v[0] as usize],
                        b: loc[v[1] as usize],
                        c: loc[v[2] as usize],
                    }),
                }
            }
            prog.draw_end = ops.len().min(MAX_OPS) as u16;
        } else {
            close_guard(&mut ops, &mut open)?;
            // `cycle`'s turns, the row section's last steps (the JIT's
            // driver runs them after its row function): palette slot `b`
            // (`finish`'s) as palette `val` turned by `a`.
            for i in 0..n {
                let m = &nodes[i];
                if is_op(i) && m.code == code::TURN {
                    if !set[m.a as usize] {
                        return Err(Error::new(0, 0, String::from(LAYOUT_BUG)));
                    }
                    loc[i] = 0;
                    set[i] = true;
                    prog.uses |= OWNPAL;
                    ops.push(WOp {
                        code: code::TURN,
                        d: m.b,
                        a: loc[m.a as usize],
                        b: m.val as u16,
                        c: 0,
                    });
                }
            }
            prog.row_end = ops.len().min(MAX_OPS) as u16;
        }
    }
    // Per pixel: the order (a value, or colour step k), each value's last
    // reader in it, then registers by linear scan. With values for the
    // still cache (`still::choose`), first the per-frame values the cached
    // run reads, the marker, the run (skipped once the cache holds its
    // values), then the rest as usual.
    let px_op = |i: usize| is_op(i) && nodes[i].level == LV_PIXEL;
    // The per-column run: values of `x`/`px` and per-frame values only,
    // worked out on a frame's first row and kept (`Item::Column`), when
    // they are worth a step; then the still cache's values. Each tried
    // with the registers, dropped when they don't fit.
    // The slow layers' pixel values (`split_slow` gave them their own):
    // never in the column or the still run (the slow part runs without
    // them, the fast part without the slow steps).
    // (Lists of their own only for a `rate` formula: this is where the
    // compile's heap peaks.)
    let none = alloc::vec![false; n];
    let mut in_slow = Vec::new();
    if let Some(sl) = slow {
        in_slow = try_vec(false, n)?;
        for st in &steps[..sl.layers] {
            for x in st.reads() {
                in_slow[x as usize] = true;
            }
        }
        for i in (0..n).rev() {
            if in_slow[i] && nodes[i].code < K_CONST {
                for x in reads(dag, i) {
                    in_slow[x as usize] = true;
                }
            }
        }
    }
    let slow_px = |i: usize| in_slow.get(i).copied().unwrap_or(false);
    if (0..n).any(|i| sc[i] != 0 && px_op(i) && slow_px(i)) {
        // E973.
        return Err(Error::new(
            0,
            0,
            "a `rate` layer can't read a scene's per-pixel values: work them out outside the scene, or leave `rate` out",
        ));
    }
    let col = column_nodes(dag);
    let in_col: Vec<bool> = (0..n).map(|i| px_op(i) && col[i] && !slow_px(i)).collect();
    let mut no_still = Vec::new();
    if slow.is_some() {
        no_still = in_slow.clone();
        for (o, &c) in no_still.iter_mut().zip(&in_col) {
            *o |= c;
        }
    }
    let col_ns: u32 = (0..n)
        .filter(|&i| in_col[i])
        .map(|i| value_ns(nodes[i].code))
        .sum();
    let mut layout = None;
    // Why a column run the formula has was left out, the first time a
    // layout with it is given up (said in a note: it costs a 32nd as much).
    let mut col_lost: Option<&'static str> = None;
    // A formula that cycles a palette (`cycle`) asks the still cache first:
    // its picture is usually still, and with the index cached a frame is
    // the palette's turn and a look-up (console-lessons 2.1). The others
    // keep the column run first, as before.
    let cycles = steps.iter().any(|st| st.turn.is_some());
    let orders = if cycles {
        [(true, true), (false, true), (true, false), (false, false)]
    } else {
        [(true, true), (true, false), (false, true), (false, false)]
    };
    for (use_col, use_still) in orders {
        let (use_col, use_still) = (use_col && runs, use_still && runs);
        let use_col = use_col && col_ns >= COLUMN_GAIN_NS;
        let excl = if use_col { &in_col } else { &none };
        let markers = usize::from(use_col);
        let cached = if use_still && total + markers < MAX_OPS {
            choose_still(
                dag,
                steps,
                &px_op,
                match (slow.is_some(), use_col) {
                    (false, _) => excl,
                    (true, true) => &no_still,
                    (true, false) => &in_slow,
                },
            )
        } else {
            Vec::new()
        };
        if (use_col && col_ns < COLUMN_GAIN_NS)
            || (use_still && cached.is_empty() && (!use_col || cycles))
        {
            continue;
        }
        // The markers are not the formula's steps: `MAX_OPS` has room for
        // both past `MAX_STEPS`, so this holds at the cap.
        if total + markers + usize::from(!cached.is_empty()) > MAX_OPS {
            if use_col {
                col_lost.get_or_insert("its marker does not fit the program");
            }
            continue;
        }
        let lay = pixel_seq(
            dag,
            steps,
            &px_op,
            &need,
            &cached,
            excl,
            use_col,
            slow.map_or(0, |s| s.layers),
        );
        // A marker's run is a byte (`COLUMN a`, `STILL a`, `SLOW a`).
        if lay.slow_len > 255 {
            return Err(Error::new(
                0,
                0,
                format!(
                    "this formula is too long for the panel: its slow layers take {} steps a pixel, the limit is 255",
                    lay.slow_len
                ),
            ));
        }
        if lay.run_len > 255 || lay.col_len > 255 {
            if lay.col_len > 255 {
                col_lost.get_or_insert("it is over 255 steps");
            }
            continue;
        }
        let mut lay = lay;
        if n_guards > 0 {
            let runs = [lay.run_len, lay.col_len, lay.slow_len];
            match scene_blocks(dag, steps, &sc, &lay.seq, runs) {
                Some(seq) => lay.seq = seq,
                None => {
                    if lay.col_len > 0 {
                        col_lost.get_or_insert("the scenes' steps could not be laid out around it");
                    }
                    continue;
                }
            }
        }
        if let Some(r) = registers(nodes, steps, &lay.seq, &lay.pinned) {
            layout = Some((lay, cached, r));
            break;
        }
        if use_col {
            col_lost.get_or_insert("it holds more values at once than the registers");
        }
    }
    let Some((lay, cached, (regs, used))) = layout else {
        return Err(Error::new(
            0,
            0,
            format!("this formula holds more than {REGS} values at once (the panel's registers): use each sooner"),
        ));
    };
    let (seq, run_len, col_len, slow_len) = (lay.seq, lay.run_len, lay.col_len, lay.slow_len);
    // (The language service's cache note: the still cache's values and the
    // column run's.)
    #[cfg(feature = "syntax")]
    {
        seen.0 = if run_len > 0 {
            cached.clone()
        } else {
            Vec::new()
        };
        seen.1 = if col_len > 0 {
            (0..n).filter(|&i| in_col[i]).collect()
        } else {
            Vec::new()
        };
        seen.2 = if col_len > 0 { None } else { col_lost };
    }
    #[cfg(not(feature = "syntax"))]
    let _ = col_lost;
    let mut marker = None;
    // Each guard: its step, its item, its scene.
    let mut guards: Vec<(usize, usize, u8)> = Vec::new();
    for (at, item) in seq.iter().enumerate() {
        match *item {
            Item::Guard(s) => {
                let c = dag.conds[s as usize - 1];
                if !set[c as usize] {
                    return Err(Error::new(0, 0, String::from(LAYOUT_BUG)));
                }
                guards.push((ops.len(), at, s));
                ops.push(WOp {
                    code: code::GUARD,
                    d: 0,
                    a: loc[c as usize],
                    b: 0,
                    c: u16::from(s) << 4,
                });
            }
            Item::Column => ops.push(WOp {
                code: code::COLUMN,
                d: 0,
                a: col_len as u16,
                b: 0,
                c: 0,
            }),
            Item::Slow => {
                let sl = slow.unwrap_or(Slow {
                    layers: 0,
                    rate: 0,
                    reads: 0,
                });
                ops.push(WOp {
                    code: code::SLOW,
                    d: u16::from(sl.rate),
                    a: slow_len as u16,
                    b: sl.layers as u16,
                    c: u16::from(sl.reads),
                });
            }
            Item::Still => {
                marker = Some(ops.len());
                ops.push(WOp {
                    code: code::STILL,
                    d: cached.len() as u16,
                    a: run_len as u16,
                    b: 0,
                    c: 0,
                });
            }
            Item::Value(i) => {
                let d = lane(L_REG0 + regs[at]) as u16;
                loc[i] = d;
                set[i] = true;
                ops.push(mk(&nodes[i], d, &loc, &set, &mut vs, &mut ch, &mut tabs)?);
            }
            Item::Colour(k) => {
                let st = &steps[k];
                if st.reads().any(|x| !set[x as usize]) {
                    return Err(Error::new(0, 0, String::from(LAYOUT_BUG)));
                }
                let l = |x: Id| loc[x as usize];
                let (kind, _) = code::colour_parts(st.code);
                let cover = st.alpha.map_or(0, l);
                let rgb = kind == code::OUT_RGB || kind == code::OUT_HSV;
                // A scaled palette: the flag on its palette, the scale in c.
                if kind == code::OUT_PAL && st.pal >= 5 {
                    prog.uses |= OWNPAL;
                }
                let (pal, scale) = match st.scale {
                    Some(k) if kind == code::OUT_PAL => {
                        ((st.pal | PAL_SCALED | (st.shift << PAL_SHIFT)) as u16, l(k))
                    }
                    _ => (st.pal as u16, 0),
                };
                ops.push(WOp {
                    code: st.code,
                    d: if rgb { cover } else { pal },
                    a: l(st.args[0]),
                    b: if rgb { l(st.args[1]) } else { cover },
                    c: if rgb { l(st.args[2]) } else { scale },
                });
            }
        }
        // (The column run's and the still cache's markers are not the
        // formula's steps: `MAX_OPS` has room for them past the cap.)
        let marks = ops
            .iter()
            .filter(|o| matches!(o.code, code::COLUMN | code::STILL))
            .count();
        if ops.len() > MAX_OPS || ops.len() - marks > MAX_STEPS {
            let steps = ops.len() - marks;
            return Err(too_long(steps, sections(steps.saturating_sub(total))));
        }
    }
    // Each guard's run (its scene's values after it, a step each) and the
    // registers of those read after the run (zeroed while it is off).
    for &(g, at, s) in &guards {
        let end = seq[at + 1..]
            .iter()
            .position(|it| !matches!(*it, Item::Value(i) if sc[i] == s))
            .map_or(seq.len(), |k| at + 1 + k);
        let m = end - at - 1;
        if m > 255 {
            return Err(Error::new(0, 0, format!("this formula is too long for the panel: a scene takes {m} steps a pixel, the limit is 255")));
        }
        let mut later = alloc::vec![false; n];
        for it in &seq[end..] {
            match *it {
                Item::Value(j) => reads(dag, j).for_each(|x| later[x as usize] = true),
                Item::Colour(k) => steps[k].reads().for_each(|x| later[x as usize] = true),
                _ => {}
            }
        }
        let mut mask = 0u16;
        for p in at + 1..end {
            if let Item::Value(i) = seq[p] {
                if later[i] {
                    mask |= 1 << regs[p];
                }
            }
        }
        ops[g].b = m as u16;
        ops[g].d = mask & 0xff;
        ops[g].c |= mask >> 8;
    }
    // The marker's skip: the run is every value after it up to the first
    // that isn't in it (`still_seq` puts them together); its registers.
    if let Some(m) = marker {
        debug_assert!(m + run_len < ops.len());
        ops[m].b = loc[cached[0]];
        ops[m].c = cached.get(1).map_or(0, |&c| loc[c]);
    }
    prog.pixel_end = ops.len() as u16;
    let (de, re) = (prog.draw_end as usize, prog.row_end as usize);
    let n_scalars = assign_slots(&ops, &vs, &ch, next_slot, de, re, reuse, prog)?;
    prog.n_scalars = n_scalars as u8;
    prog.regs = used as u8;
    Ok(())
}

/// A slot while the steps are laid out: a constant (its value at the start
/// of every frame), a value a step writes, or a pick's table of its
/// choices' slots (consecutive constants).
enum VSlot {
    Const(i32),
    Value,
    Pick(u16),
}

/// A step while it is laid out: as [`Op`], but an operand of `VS` or more is
/// slot `n - VS` of the `VSlot`s, given a scalar slot by [`assign_slots`];
/// anything below is as it will be (an input's slot, a lane, an immediate).
#[derive(Clone, Copy)]
struct WOp {
    code: u8,
    d: u16,
    a: u16,
    b: u16,
    c: u16,
}

const VS: u16 = 0x100;

/// Give every slot of `vs` a scalar slot from `base` up — one whose last
/// value is no longer needed is used again — and write the steps into
/// `prog` with them; how many scalar slots that takes (E404 past
/// [`MAX_SCALARS`]).
///
/// A slot is needed from the step that writes it (from the frame's start
/// for a constant) to the last step that reads it, and longer where the
/// steps run again: a value from before the rows that the rows or pixels
/// read, and every value the pixels read, to the end (rows and lanes run
/// the same steps again); one from before a draw stage loop that the loop
/// reads, to its END. Within those, two values may share a slot: the VM
/// reads every operand before it writes, and nothing reads a value after
/// its interval, so the frames are the same bytes (`range::narrow` walks
/// the steps in the same order, so its intervals follow the slots too).
/// A pick's table goes after all of them (its choices are read by the pick,
/// per pixel).
#[allow(clippy::too_many_arguments)]
fn assign_slots(
    ops: &[WOp],
    vs: &[VSlot],
    ch: &[Vec<u16>],
    base: usize,
    de: usize,
    re: usize,
    reuse: bool,
    prog: &mut Program,
) -> Result<usize, Error> {
    let n = vs.len();
    // (u16 throughout: positions are at most MAX_OPS + 2; the compile's heap peaks
    // here, and on the ESP32.)
    const INF: u16 = u16::MAX;
    // Positions: a step's index + 1 (0 is the frame's start, the constants').
    let (mut start, mut end) = (alloc::vec![INF; n], alloc::vec![0u16; n]);
    for (k, v) in vs.iter().enumerate() {
        if matches!(v, VSlot::Const(_)) {
            start[k] = 0;
        }
    }
    let use_at = |end: &mut [u16], v: u16, p: u16| {
        if v >= VS {
            let k = (v - VS) as usize;
            end[k] = end[k].max(p);
            if let VSlot::Pick(j) = vs[k] {
                for &c in &ch[j as usize] {
                    if c >= VS {
                        let j = (c - VS) as usize;
                        end[j] = end[j].max(p);
                    }
                }
            }
        }
    };
    let mut loops: Vec<(u16, u16)> = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        let p = i as u16 + 1;
        if op.code == code::FOR {
            loops.push((p, op.b + 1));
        }
        // (A draw stage step's target is a step's index, not a slot.)
        let [d, a, b, c] = operand_fields(op);
        if (code::writes_scalar(op.code) || op.code == code::FOR) && d >= VS {
            let k = (d - VS) as usize;
            start[k] = start[k].min(p);
        } else {
            use_at(&mut end, d, p);
        }
        for v in [a, b, c] {
            use_at(&mut end, v, p);
        }
    }
    // Inner loops first (they start later), so an outer one sees their END.
    loops.reverse();
    let (rows, pixels) = (de as u16 + 1, re as u16 + 1);
    for k in 0..n {
        if start[k] == INF {
            start[k] = end[k];
        }
        end[k] = end[k].max(start[k]);
        if end[k] >= pixels || (start[k] < rows && end[k] >= rows) {
            end[k] = INF;
            continue;
        }
        for &(f, e) in &loops {
            if start[k] < f && end[k] >= f {
                end[k] = end[k].max(e);
            }
        }
    }
    // By where each starts (a counting sort: no sort's code in the wasm).
    let top = ops.len() + 2;
    let mut count = alloc::vec![0u16; top + 1];
    let is_pick = |k: usize| matches!(vs[k], VSlot::Pick(_));
    for k in (0..n).filter(|&k| !is_pick(k)) {
        count[(start[k] as usize).min(top) + 1] += 1;
    }
    for p in 1..=top {
        count[p] += count[p - 1];
    }
    let mut order = alloc::vec![0u16; count[top] as usize];
    for k in (0..n).filter(|&k| !is_pick(k)) {
        let p = &mut count[(start[k] as usize).min(top)];
        order[*p as usize] = k as u16;
        *p += 1;
    }
    // Slots from `base` up, counted on past the limit (E404 tells how
    // many the formula needs, and its steps by section: where to look).
    let mut phys = alloc::vec![0u16; n];
    let mut active: Vec<u16> = Vec::new();
    let mut free: Vec<u16> = Vec::new();
    let mut next = base;
    for &k in &order {
        let k = k as usize;
        let s = start[k];
        active.retain(|&a| {
            let gone = reuse && end[a as usize] < s;
            if gone {
                free.push(phys[a as usize]);
            }
            !gone
        });
        phys[k] = match free.pop() {
            Some(p) => p,
            None => {
                next += 1;
                (next - 1) as u16
            }
        };
        active.push(k as u16);
    }
    let tables: usize = (0..n)
        .filter_map(|k| match vs[k] {
            VSlot::Pick(j) => Some(ch[j as usize].len()),
            _ => None,
        })
        .sum();
    if next + tables > MAX_SCALARS {
        let f = prog.frame_end as usize;
        return Err(Error::new(
            0,
            0,
            format!(
                "this formula needs {} numbers and once-a-frame values, more than the {MAX_SCALARS} it can hold (steps: per frame {f}, draw {}, per row {}, per pixel {}): use fewer",
                next + tables,
                de.saturating_sub(f),
                re.saturating_sub(de),
                ops.len().saturating_sub(re),
            ),
        ));
    }
    for k in 0..n {
        if let VSlot::Const(v) = vs[k] {
            prog.init[phys[k] as usize] = v;
        }
    }
    // The picks' tables, each its choices' slots in a row.
    let map = |phys: &[u16], v: u16| {
        if v >= VS {
            phys[(v - VS) as usize] as u8
        } else {
            v as u8
        }
    };
    for k in 0..n {
        if let VSlot::Pick(j) = vs[k] {
            phys[k] = next as u16;
            for &c in &ch[j as usize] {
                prog.init[next] = map(&phys, c) as i32;
                next += 1;
            }
        }
    }
    for (o, w) in prog.ops.iter_mut().zip(ops) {
        let [d, a, b, c] = operand_fields(w);
        *o = Op {
            code: w.code,
            d: map(&phys, d),
            a: map(&phys, a),
            b: map(&phys, b),
            c: map(&phys, c),
        };
        if let Some(at) = target_of(w) {
            o.set_target(at as usize);
        }
    }
    Ok(next)
}

/// A draw stage step's target as laid out (`IF`'s, `JUMP`'s, a `FOR`'s
/// END in `b`, an `END`'s step back in `c`: [`Op::target`]).
fn target_of(w: &WOp) -> Option<u16> {
    match w.code {
        code::IF | code::JUMP | code::FOR => Some(w.b),
        code::END => Some(w.c),
        _ => None,
    }
}

/// A step's `d a b c` as slots: its target (`target_of`) 0.
fn operand_fields(w: &WOp) -> [u16; 4] {
    match w.code {
        code::IF | code::JUMP | code::FOR => [w.d, w.a, 0, 0],
        code::END => [0, w.a, w.b, 0],
        _ => [w.d, w.a, w.b, w.c],
    }
}

/// What a step reading a value with no place yet says (never seen: a bug in
/// the layout; `compile_layout` then lays the formula out without its
/// runs).
const LAYOUT_BUG: &str = "this formula could not be laid out (a bug in Psi) — please report it";

/// A pixel step to lay out: a value, colour step k, or the still cache's
/// marker.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    Value(usize),
    Colour(usize),
    Still,
    Column,
    /// The slow layers' marker (`code::SLOW`).
    Slow,
    /// A scene's guard (`code::GUARD`): the values after it of that scene.
    Guard(u8),
}

/// A wide noise's code.
fn is_noise_o(c: u8) -> bool {
    (code::NOISE_O..=code::NOISE_O_LAST).contains(&c)
}

/// A value's operands that are nodes (a pick's choices included, and the
/// steps a wide noise needs before it).
fn reads(dag: &Dag, i: usize) -> impl Iterator<Item = Id> + '_ {
    let m = &dag.nodes[i];
    let k = if m.code < K_CONST {
        code::arity(m.code)
    } else {
        0
    };
    let picks: &[Id] = if m.code == code::PICK {
        &dag.picks[m.b as usize]
    } else if is_noise_o(m.code) {
        &dag.deps[m.val as usize]
    } else {
        &[]
    };
    [m.a, m.b, m.c]
        .into_iter()
        .take(k)
        .chain(picks.iter().copied())
}

/// Registers for the values of `seq` by linear scan (a register is free
/// again after its value's last use, and a step never writes a register it
/// reads): the register of each item (0 for the others) and how many were
/// used, `None` when more than [`REGS`] are live at once.
fn registers(
    nodes: &[Node],
    steps: &[Step],
    seq: &[Item],
    pinned: &[bool],
) -> Option<(Vec<u8>, usize)> {
    let n = nodes.len();
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
            Item::Still | Item::Column | Item::Slow | Item::Guard(_) => {}
        }
    }
    // A kept column value is never given up: the rows after the first
    // read it without working it out again.
    for (i, l) in last_use.iter_mut().enumerate() {
        if pinned[i] {
            *l = usize::MAX;
        }
    }
    let mut regs = alloc::vec![0u8; seq.len()];
    let mut free: Vec<u8> = (0..REGS as u8).rev().collect();
    let mut holder: [Option<usize>; REGS] = [None; REGS];
    let mut used = 0usize;
    for (at, item) in seq.iter().enumerate() {
        let wrote = match *item {
            Item::Value(i) => {
                let r = free.pop()?;
                used = used.max(REGS - free.len());
                regs[at] = r;
                holder[r as usize] = Some(i);
                Some(i)
            }
            _ => None,
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
    }
    Some((regs, used))
}

/// A cached value must save at least this much a pixel (ns on the ESP32,
/// `program::value_ns`): ~1.6 ms a frame for 16 KB of heap.
const STILL_GAIN_NS: u32 = 400;

/// Still values weighed for the cache, the heaviest by an upper bound
/// (bounds the compile time: each is a walk of the formula per lane).
const STILL_CANDIDATES: usize = 24;

/// Whether each node is *still*: worked out from the pixel's position
/// (`x y px py i`) and constants only, the same every frame.
fn still_nodes(dag: &Dag) -> Vec<bool> {
    let nodes = &dag.nodes;
    let mut still = alloc::vec![false; nodes.len()];
    for i in 0..nodes.len() {
        let m = &nodes[i];
        still[i] = match m.code {
            K_CONST => true,
            K_INPUT => {
                let v = m.val as u8;
                v == lane(L_X) || v == lane(L_PX) || v == lane(L_I) || v == S_Y || v == S_PY
            }
            K_COUNTER | K_SPLIT => false,
            // (A wide noise's offsets move every frame.)
            c if (code::U_R..=code::INKN).contains(&c) || c == code::UAT || is_noise_o(c) => false,
            // (A scene's values are 0 while it is off.)
            _ if dag.scene_of(i) != 0 => false,
            _ => reads(dag, i).all(|x| still[x as usize]),
        };
    }
    still
}

/// The per-pixel values worked out every frame when the values in
/// `cached` come from the still cache: everything a colour step reads,
/// through anything but a cached value.
fn per_frame(dag: &Dag, steps: &[Step], cached: &[usize]) -> Vec<bool> {
    let n = dag.nodes.len();
    let mut f = alloc::vec![false; n];
    for st in steps {
        for a in st.reads() {
            f[a as usize] = true;
        }
    }
    for i in (0..n).rev() {
        if !f[i] || cached.contains(&i) || dag.nodes[i].code >= K_CONST {
            continue;
        }
        for x in reads(dag, i) {
            f[x as usize] = true;
        }
    }
    f
}

/// The values for the still cache, best first (at most
/// [`crate::still::STILL_LANES`]): still per-pixel values chosen greedily
/// by what they save a pixel per frame, each at least [`STILL_GAIN_NS`].
fn choose_still(
    dag: &Dag,
    steps: &[Step],
    px_op: &dyn Fn(usize) -> bool,
    col: &[bool],
) -> Vec<usize> {
    let nodes = &dag.nodes;
    let n = nodes.len();
    let still = still_nodes(dag);
    // (A value of the column run is worked out once a frame a column
    // anyway: never cached, and nothing to save.)
    let px_op = |i: usize| px_op(i) && !col[i];
    // What each still value's own steps cost at most (shared steps counted
    // in every reader: an upper bound, to leave out the cheap ones early).
    let mut sub = alloc::vec![0u32; n];
    for i in 0..n {
        if px_op(i) && still[i] {
            sub[i] = value_ns(nodes[i].code)
                + reads(dag, i)
                    .map(|x| sub[x as usize])
                    .fold(0u32, u32::saturating_add);
        }
    }
    // Any still value may be the one kept (often one inside: `q` read by
    // `q/64` and `frac(q)`, both still too); the heaviest few are weighed.
    // (The heaviest by a scan, not a sort: a sort's code is ~3 KB of wasm.)
    let mut cands: Vec<usize> = Vec::new();
    for i in (0..n).filter(|&i| px_op(i) && still[i] && sub[i] >= STILL_GAIN_NS) {
        if cands.len() < STILL_CANDIDATES {
            cands.push(i);
        } else if let Some(m) = (0..cands.len()).min_by_key(|&k| sub[cands[k]]) {
            if sub[cands[m]] < sub[i] {
                cands[m] = i;
            }
        }
    }
    if cands.is_empty() {
        return Vec::new();
    }
    let cost = |cached: &[usize]| -> u32 {
        let f = per_frame(dag, steps, cached);
        (0..n)
            .filter(|&i| f[i] && px_op(i) && !cached.contains(&i))
            .map(|i| value_ns(nodes[i].code))
            .sum()
    };
    let mut cached: Vec<usize> = Vec::new();
    let mut now = cost(&cached);
    for _ in 0..crate::still::STILL_LANES {
        let mut best: Option<(usize, u32)> = None;
        for &c in &cands {
            if cached.contains(&c) {
                continue;
            }
            cached.push(c);
            let ok = still_run(dag, steps, &px_op, &cached).is_some();
            let v = cost(&cached);
            cached.pop();
            if !ok {
                continue;
            }
            if best.is_none_or(|(_, b)| v < b) {
                best = Some((c, v));
            }
        }
        match best {
            Some((c, v)) if now.saturating_sub(v) >= STILL_GAIN_NS => {
                cached.push(c);
                now = v;
            }
            _ => break,
        }
    }
    cached
}

/// The still cache's run for `cached` (the cached values and the steps only
/// they need) and the per-frame values the run reads (laid out before its
/// marker), or `None` when those can't go before it: one of them reads a
/// cached value (`ab = abs(ang)` read by the frame and by a cached
/// `sqrt(ab*2)`, with `ang` cached too) — skipping the run would leave it
/// reading a register nothing wrote.
fn still_run(
    dag: &Dag,
    steps: &[Step],
    px: &dyn Fn(usize) -> bool,
    cached: &[usize],
) -> Option<(Vec<bool>, Vec<bool>)> {
    let n = dag.nodes.len();
    let f = per_frame(dag, steps, cached);
    let mut run = alloc::vec![false; n];
    let mut first = alloc::vec![false; n];
    for &c in cached {
        run[c] = true;
    }
    for i in (0..n).rev() {
        if run[i] {
            for x in reads(dag, i) {
                if px(x as usize) && !f[x as usize] {
                    run[x as usize] = true;
                }
            }
        }
    }
    for i in (0..n).rev() {
        if run[i] || first[i] {
            for x in reads(dag, i) {
                let x = x as usize;
                if px(x) && !run[x] {
                    first[x] = true;
                }
            }
        }
    }
    // A value before the marker reads only values before it (or rows,
    // columns, constants): never one in the run.
    let ok = (0..n).all(|i| !first[i] || reads(dag, i).all(|x| !run[x as usize]));
    ok.then_some((run, first))
}

/// The pixel order with each scene's values together after its guard
/// (`Item::Guard`), or `None` when that can't be had in this layout (a
/// scene's values among the runs' or the slow layers'). For each scene, the
/// items from its first value to its last are put in three groups, each in
/// its order: those that need none of its values (before), its values, and
/// those that need them (after) — the same dependencies, since a value of
/// the scene needs nothing that needs one.
fn scene_blocks(
    dag: &Dag,
    steps: &[Step],
    sc: &[u8],
    seq: &[Item],
    [run_len, col_len, slow_len]: [usize; 3],
) -> Option<Vec<Item>> {
    let n = dag.nodes.len();
    let mut seq = seq.to_vec();
    // Where the runs and the slow layers end (the JIT's kernel starts
    // after the runs): a scene's values after.
    let mut from = 0;
    for (at, it) in seq.iter().enumerate() {
        let len = match it {
            Item::Still => run_len,
            Item::Column => col_len,
            Item::Slow => slow_len,
            _ => continue,
        };
        from = from.max(at + 1 + len);
    }
    let scenes = dag.conds.len() as u8;
    for k in 1..=scenes {
        let of = |it: &Item| matches!(*it, Item::Value(i) if sc[i] == k);
        let Some(first) = seq.iter().position(of) else {
            continue;
        };
        let last = seq.iter().rposition(of).unwrap_or(first);
        if first < from {
            return None;
        }
        let mut dep = alloc::vec![false; n];
        for i in 0..n {
            dep[i] = sc[i] == k || reads(dag, i).any(|x| dep[x as usize]);
        }
        let needs = |it: &Item| match *it {
            Item::Value(i) => dep[i],
            Item::Colour(c) => steps[c].reads().any(|x| dep[x as usize]),
            _ => false,
        };
        if seq[first..=last]
            .iter()
            .any(|it| matches!(it, Item::Still | Item::Column | Item::Slow))
        {
            return None;
        }
        // The three groups, in place: a stable partition by rank.
        let rank = |it: &Item| {
            if of(it) {
                1u8
            } else {
                2 * u8::from(needs(it))
            }
        };
        let mut out: Vec<Item> = Vec::with_capacity(last + 1 - first);
        for g in 0..3 {
            for it in &seq[first..=last] {
                if rank(it) == g {
                    out.push(*it);
                }
            }
        }
        seq[first..=last].copy_from_slice(&out);
    }
    // A guard before each scene's values (one run each, as above).
    let mut out = Vec::with_capacity(seq.len() + scenes as usize);
    let mut cur = 0u8;
    for it in seq {
        let s = match it {
            Item::Value(i) => sc[i],
            _ => 0,
        };
        if s != 0 && s != cur {
            out.push(Item::Guard(s));
        }
        cur = s;
        out.push(it);
    }
    Some(out)
}

/// A laid-out pixel section: the order, the cached run's and the column
/// run's lengths, and the values kept in their registers for the frame.
struct PixelSeq {
    seq: Vec<Item>,
    run_len: usize,
    col_len: usize,
    /// The slow layers' items after their marker.
    slow_len: usize,
    pinned: Vec<bool>,
}

/// The pixel order: the column run (with `col`: its marker and every
/// value of `x`, `px` and per-frame values, in node order), then with the
/// still cache the per-frame values the cached run reads, the marker and
/// the run (the cached values and the steps only they need), then the other
/// values before the colour step that first needs them, as without either.
#[allow(clippy::too_many_arguments)]
fn pixel_seq(
    dag: &Dag,
    steps: &[Step],
    px_op: &dyn Fn(usize) -> bool,
    need: &[usize],
    cached: &[usize],
    in_col: &[bool],
    col: bool,
    slow: usize,
) -> PixelSeq {
    let n = dag.nodes.len();
    let mut seq: Vec<Item> = Vec::new();
    let mut pinned = alloc::vec![false; n];
    let mut col_len = 0;
    // The slow layers (`layer rate N`) first, after their marker: their
    // values and colours (none in the runs below); the slow part runs only
    // them, the fast part starts after them. (A whole frame never runs a
    // `rate` formula's steps in one go: the runs' kept registers are not
    // the slow steps' to keep.)
    // (One loop for a layer's values and colour, the slow ones' and the
    // others'.)
    let layer = |seq: &mut Vec<Item>, k: usize, ok: &dyn Fn(usize) -> bool| {
        for (i, &nd) in need.iter().enumerate().take(n) {
            if nd == k && ok(i) {
                seq.push(Item::Value(i));
            }
        }
        seq.push(Item::Colour(k));
    };
    let mut slow_len = 0;
    if slow > 0 {
        seq.push(Item::Slow);
        for k in 0..slow {
            layer(&mut seq, k, px_op);
        }
        slow_len = seq.len() - 1;
    }
    if col {
        seq.push(Item::Column);
        let at = seq.len();
        seq.extend((0..n).filter(|&i| in_col[i]).map(Item::Value));
        col_len = seq.len() - at;
        // Kept: the column values something after the run reads.
        for i in 0..n {
            if px_op(i) && !in_col[i] {
                for x in reads(dag, i) {
                    pinned[x as usize] |= in_col[x as usize];
                }
            }
        }
        for st in steps {
            for x in st.reads() {
                pinned[x as usize] |= in_col[x as usize];
            }
        }
    }
    let px = |i: usize| px_op(i) && !(col && in_col[i]);
    let mut run = alloc::vec![false; n];
    let mut first = alloc::vec![false; n];
    let mut run_len = 0;
    if !cached.is_empty() {
        // (`choose_still` took only sets this lays out: see `still_run`.)
        if let Some((r, f)) = still_run(dag, steps, &px, cached) {
            (run, first) = (r, f);
            seq.extend((0..n).filter(|&i| first[i]).map(Item::Value));
            seq.push(Item::Still);
            let before = seq.len();
            seq.extend((0..n).filter(|&i| run[i]).map(Item::Value));
            run_len = seq.len() - before;
        }
    }
    for k in slow..steps.len() {
        layer(&mut seq, k, &|i| px(i) && !run[i] && !first[i]);
    }
    PixelSeq {
        seq,
        run_len,
        col_len,
        slow_len,
        pinned,
    }
}

/// The column run is laid out only when its values cost at least this a
/// pixel (ns: its marker is a step of the 256, and its kept values hold
/// registers for the whole row).
const COLUMN_GAIN_NS: u32 = 130;

/// Whether each node is the same down a column: worked out from `x`, `px`,
/// constants and per-frame values only.
fn column_nodes(dag: &Dag) -> Vec<bool> {
    let nodes = &dag.nodes;
    let mut col = alloc::vec![false; nodes.len()];
    for i in 0..nodes.len() {
        let m = &nodes[i];
        col[i] = match m.level {
            LV_CONST | LV_FRAME => true,
            LV_PIXEL => match m.code {
                K_INPUT => {
                    let v = m.val as u8;
                    v == lane(L_X) || v == lane(L_PX)
                }
                K_CONST => true,
                K_COUNTER => false,
                c if (code::U_R..=code::INKN).contains(&c) || c == code::UAT => false,
                _ if dag.scene_of(i) != 0 => false,
                _ => reads(dag, i).all(|x| col[x as usize]),
            },
            _ => false,
        };
    }
    col
}
