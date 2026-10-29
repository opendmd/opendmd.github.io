//! Any Formula as a look: drawn over every GIF with a blend, at an amount.
//!
//! A formula says it can be a look with its own `look [blend] [amount]`
//! line ([`declared`]; the web app's Formula settings sheet writes the line
//! with [`set`], a text edit), and the player draws the composition that
//! line asks for as any other formula: nothing in the firmware's drawing
//! is new. The text stands alone, as shared Formulas do — the formula
//! pasted in as a part:
//!
//! ```text
//! loop 8                         -- the formula's loop, when it isn't 8 s
//!                                -- (`loop forever` for one that never repeats)
//! part look {
//!   <the formula, indented 2>
//! }
//! layer screen { look(x, y, 0.4) }   -- `look(x, y, 0.4)` alone for over
//! ```
//!
//! - **over at 100%** is the formula itself, unchanged: a formula that
//!   already draws over the GIF is the look it always was, byte for byte.
//!   (One that covers the GIF entirely is still refused there, by the
//!   player: `fx_psi::COVERS`.)
//! - **The loop** goes outside too: inside a part `t` is `t % (its loop)`,
//!   so a part with a loop longer than the formula's (8 s by default) would
//!   never reach its end.
//! - **The amount** is the coverage, `0` to `1` in hundredths; at 100% none
//!   is written (`layer screen { look }`).
//! - **A formula with layers of its own** keeps its blends: as a part it
//!   can only go over (`layer { p }`, or the last line, with coverage: each
//!   of its layers covers that much less). Another blend for one is the
//!   compiler's error, which the web app does not offer.
//! - **`ur ug ub ul`** inside the part read the real pixel's picture, the
//!   GIF: a formula that reads them grades the GIF, then the blend puts
//!   that onto the GIF again.
//!
//! **A formula's own `look` line** (`look screen 40`,
//! docs/research/formula-apps.md §2.1) is this composition written in the
//! formula: [`declared`] gives the composed text the line asks for (the
//! player draws that as the look; played on its own the line is ignored).
//! The line itself is left out of the part ([`wrap`] cuts it), so a look
//! composed from a formula with one is the same text as from the formula
//! without it, and the formula's `control` lines inside the part still
//! describe the slots (the compiler takes a part's controls when the
//! formula's own lines declare none).

use crate::{compile_into, Error, Program};
use alloc::string::String;

/// The blends a look may use, as `layer` names them (`max` and `min` are
/// the web app's Lighten and Darken).
pub const BLENDS: [&str; 6] = ["over", "screen", "add", "multiply", "max", "min"];

/// The part's name in a composed look.
const PART: &str = "look";

/// The look's source: `src` drawn with `blend` (one of [`BLENDS`]) at
/// `amount` percent (0..=100, clamped). Errors: `src` does not compile, or
/// the blend is not one of [`BLENDS`]. The result is not compiled here
/// (a layered formula with a blend other than over is the compiler's
/// error, as is a look too long for the panel).
pub fn compose(src: &str, blend: &str, amount: u8) -> Result<String, Error> {
    // Only its loop and declarations are wanted (boxed: ~2.9 KB).
    let mut p = alloc::boxed::Box::<Program>::default();
    compile_into(src, &mut p)?;
    wrap(src, &p, blend, amount)
}

/// The look a formula's own `look` line asks for, composed as [`compose`]
/// would: `None` when it has none, or it is `look` (over, 100: the formula
/// itself). `prog` is `src` compiled.
pub fn declared(src: &str, prog: &Program) -> Option<Result<String, Error>> {
    let (blend, amount) = prog.meta().look()?;
    (blend != "over" || amount < 100).then(|| wrap(src, prog, blend, amount))
}

/// [`compose`] for a formula already compiled into `prog` (its loop, and
/// where a `look` line is, which is left out). The web module compiles
/// once and calls this.
pub fn wrap(src: &str, prog: &Program, blend: &str, amount: u8) -> Result<String, Error> {
    let loop_ms = prog.loop_ms();
    // Its `look` line, left out.
    let cut = prog.meta().look().and_then(|_| look_statement(src));
    let owned;
    let src = match cut {
        Some((a, e)) => {
            // (Not `concat`: its generic join is ~1 KB of the web module.)
            let mut o = String::with_capacity(src.len());
            o.push_str(src.get(..a).unwrap_or(""));
            o.push_str(src.get(e..).unwrap_or(""));
            owned = o;
            owned.as_str()
        }
        None => src,
    };
    // (No `format!`: core::fmt is ~8 KB gz of the web module.)
    if !BLENDS.contains(&blend) {
        return Err(Error {
            start: 0,
            end: 0,
            msg: String::from("a look's blend is one of over, screen, add, multiply, max, min"),
        });
    }
    let amount = amount.min(100);
    if blend == "over" && amount == 100 {
        return Ok(String::from(src));
    }
    let mut s = String::with_capacity(src.len() + 64);
    if loop_ms == 0 {
        // A formula that never repeats: so does its look.
        s.push_str("loop forever\n");
    } else if loop_ms != 8000 {
        s.push_str("loop ");
        decimal(&mut s, loop_ms / 1000, loop_ms % 1000, 3);
        s.push('\n');
    }
    s.push_str("part look {\n");
    // Its lines, blank ones left out (bytes: `trim` is Unicode's).
    for l in src.split('\n') {
        let l = l.strip_suffix('\r').unwrap_or(l);
        if l.bytes().any(|b| !b.is_ascii_whitespace()) {
            s.push_str("  ");
            s.push_str(l);
            s.push('\n');
        }
    }
    s.push_str("}\n");
    if blend != "over" {
        s.push_str("layer ");
        s.push_str(blend);
        s.push_str(" { ");
    }
    s.push_str(PART);
    if amount < 100 {
        s.push_str("(x, y, ");
        decimal(&mut s, 0, u32::from(amount), 2);
        s.push(')');
    }
    if blend != "over" {
        s.push_str(" }");
    }
    Ok(s)
}

/// Where a formula's `look` line is (from `look` to its `;` or its line's
/// end, a comment after it included): the formula compiled, so its
/// declarations are the statements at its top whose first word is one
/// (`name`, `about` and `control` lines may have text in quotes, with
/// `;` or `--` inside).
fn look_statement(src: &str) -> Option<(usize, usize)> {
    let b = src.as_bytes();
    let mut i = 0;
    loop {
        // The next statement: past spaces, line ends, `;` and comments.
        while i < b.len() {
            match b[i] {
                b' ' | b'\t' | b'\r' | b'\n' | b';' => i += 1,
                b'-' | b'/' if b.get(i + 1) == Some(&b[i]) => {
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                }
                _ => break,
            }
        }
        let start = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        let word = src.get(start..i)?;
        if !crate::builtins::DECLARATIONS.contains(&word) {
            return None;
        }
        // To the statement's end: `;` or the line's end outside quotes.
        let mut quote = false;
        while i < b.len() && b[i] != b'\n' && (quote || b[i] != b';') {
            if b[i] == b'"' {
                quote = !quote;
            }
            if !quote && (b[i] == b'-' || b[i] == b'/') && b.get(i + 1) == Some(&b[i]) {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                break;
            }
            i += 1;
        }
        if word == "look" {
            return Some((start, (i + usize::from(b.get(i) == Some(&b';'))).min(b.len())));
        }
    }
}

/// A formula's `look` line set to `blend` at `amount` percent (`look`
/// alone for over at 100, `look screen`, `look screen 40`, `look over
/// 30`), or taken out when `blend` is empty: a text edit ([`declare`]),
/// the one way the web app writes a look (its settings sheet). `None`
/// for a blend that is not one of [`BLENDS`].
pub fn set(src: &str, blend: &str, amount: u8) -> Option<String> {
    if blend.is_empty() {
        return Some(declare(src, "look", 0, ""));
    }
    if !BLENDS.contains(&blend) {
        return None;
    }
    let amount = amount.min(100);
    let mut l = String::from("look");
    if blend != "over" || amount < 100 {
        l.push(' ');
        l.push_str(blend);
    }
    if amount < 100 {
        l.push(' ');
        decimal(&mut l, u32::from(amount), 0, 0);
    }
    Some(declare(src, "look", 0, &l))
}

/// `src` with its `nth` declaration whose first word is `word` (`name`,
/// `about`, `control`, `look`; `nth` counts `control`s) replaced by
/// `line`, taken out when `line` is empty, or — when there is none —
/// `line` put in among the declarations at the top, in their usual order
/// (`psi`, `name`, `about`, `control`, `look`). Text only: the result is
/// not compiled (the caller does), and `line` is written as given. The
/// declarations are the statements at the formula's top whose first word
/// is one (blank lines and comments between them skipped), as the
/// compiler requires them to be: each ends at its line's end or at a `;`
/// outside quotes (a path's text has them all on one line, `; `-joined).
#[inline(never)]
pub fn declare(src: &str, word: &str, nth: usize, line: &str) -> String {
    const ORDER: [&str; 5] = ["psi", "name", "about", "control", "look"];
    let rank = |w: &str| ORDER.iter().position(|&o| o == w).unwrap_or(5);
    let me = rank(word);
    // (Bytes and `get`, never `[a..b]` of a str: a slice's panic brings
    // core::fmt, ~8 KB gz of the web module.)
    let b = src.as_bytes();
    let part = |a: usize, e: usize| src.get(a..e).unwrap_or("");
    // A statement: (start, end before its `;` or line break, past that).
    // The match, and where a new one goes: after the last declaration
    // ranked at or before `word` (and how it ended), else before the first.
    let (mut found, mut after, mut first, mut seen, mut at) = (None, None, None, 0, 0);
    while at < b.len() {
        while at < b.len() && matches!(b[at], b' ' | b'\t' | b'\r' | b'\n' | b';') {
            at += 1;
        }
        let i = at;
        let mut e = i;
        while e < b.len() && (b[e].is_ascii_alphanumeric() || b[e] == b'_') {
            e += 1;
        }
        let w = part(i, e);
        let mut k = e;
        while k < b.len() && matches!(b[k], b' ' | b'\t') {
            k += 1;
        }
        // Its end: `;` or the line's, outside quotes; a comment runs to
        // the line's end.
        let (mut end, mut quote) = (i, false);
        while end < b.len() && b[end] != b'\n' && (quote || b[end] != b';') {
            if !quote && matches!(b[end], b'-' | b'/') && b.get(end + 1) == Some(&b[end]) {
                while end < b.len() && b[end] != b'\n' {
                    end += 1;
                }
                break;
            }
            quote ^= b[end] == b'"';
            end += 1;
        }
        let next = (end + 1).min(b.len());
        if e == i && end > i && matches!(b[i], b'-' | b'/') && b.get(i + 1) == Some(&b[i]) {
            // A comment alone.
            at = next;
            continue;
        }
        if i >= b.len() || rank(w) == 5 || b.get(k) == Some(&b'=') {
            break;
        }
        first.get_or_insert(i);
        if rank(w) <= me {
            after = Some((next, b.get(end) == Some(&b';')));
        }
        if w == word {
            if seen == nth {
                found = Some((i, end, next));
            }
            seen += 1;
        }
        at = next;
    }
    let mut o = String::with_capacity(src.len() + line.len() + 2);
    match found {
        Some((a, e, next)) => {
            o.push_str(part(0, a));
            if line.is_empty() {
                // The statement, its `;` or line break, the spaces after.
                let mut n = next;
                while b.get(e) == Some(&b';') && b.get(n) == Some(&b' ') {
                    n += 1;
                }
                o.push_str(part(n, b.len()));
            } else {
                o.push_str(line);
                o.push_str(part(e, b.len()));
            }
        }
        None if line.is_empty() => o.push_str(src),
        None => {
            let (p, semi) = match after {
                Some(x) => x,
                None => (first.unwrap_or(0), false),
            };
            o.push_str(part(0, p));
            if semi {
                // On a `; `-joined line: one more statement there.
                o.push(' ');
                o.push_str(line);
                o.push(';');
            } else {
                if p > 0 && b.get(p - 1) != Some(&b'\n') {
                    o.push('\n');
                }
                o.push_str(line);
                o.push('\n');
            }
            o.push_str(part(p, b.len()));
        }
    }
    o
}

/// `whole.frac` with `frac` in `places` decimal places, trailing zeros
/// (and a point with none after it) left out.
fn decimal(s: &mut String, whole: u32, frac: u32, places: u32) {
    let mut b = [0u8; 16];
    let mut n = 10;
    let mut w = whole;
    loop {
        n -= 1;
        b[n] = b'0' + (w % 10) as u8;
        w /= 10;
        if w == 0 {
            break;
        }
    }
    let mut end = 10;
    if frac > 0 {
        b[10] = b'.';
        let mut f = frac;
        for i in (0..places as usize).rev() {
            b[11 + i] = b'0' + (f % 10) as u8;
            f /= 10;
        }
        end = 11 + places as usize;
        while b[end - 1] == b'0' {
            end -= 1;
        }
    }
    for &c in &b[n..end] {
        s.push(char::from(c));
    }
}
