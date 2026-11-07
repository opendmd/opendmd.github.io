//! `psi fmt`: one canonical layout for a formula (the house style,
//! opendmd-notes docs/research/psi-tooling-plan.md "Decisions" 6).
//!
//! **Only the space between tokens changes.** The formula's tokens — every
//! name, number, colour, operator and line end the compiler reads — come out
//! as they went in, in the same order (but for line ends just inside a
//! `{ }`, which the compiler skips), so the formula compiles to the same
//! program (`tests/fmt.rs` holds that for every example and gallery formula
//! and for random ones; `psi fmt` checks it again before it writes).
//! Comments are kept, word for word, on the line they were on (a comment
//! after code stays after it; one on its own line stays on its own line).
//!
//! The layout:
//! - one statement a line: `a = 1; b = 2` becomes two lines (a `;` and a
//!   line end are the same to the language);
//! - a block's statements on lines of their own, two spaces in (`frame`,
//!   `layer`, `part`, `draw`, `for`, `sprite`): `layer { gray(x) }` opens
//!   out onto three lines; a line that goes on inside brackets, or after an
//!   operator or a `,`, two more;
//! - a space around `=`, comparisons, `and`/`or`, `?` and `:`; after a `,`,
//!   none before; none inside brackets, around `.` or `..`, after a `-` that
//!   negates, or between a function's name and its `(`;
//! - `+` and `-` spaced; `*`, `/` and `%` written close up where they bind
//!   tighter than a `+` or `-` beside them (`x*3 + t/8`), or in a product
//!   of plain values (`frac(t/8)*96`), and spaced in a product with a
//!   bracket that is itself spaced (`pal(ice, v) * (0.5 + k*0.5)`) — the
//!   style every example is written in;
//! - comments after code on lines next to each other lined up, one space
//!   past the longest line's code;
//! - at most one blank line in a row, none at the start or end, none just
//!   inside a `{ }`; a sprite's rows one a line, as written;
//! - lines are never joined or split otherwise: where a long expression
//!   breaks is the author's.

use crate::compile::{lex, Error, Tok, Token};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// `src` in the canonical layout ([`crate::syntax::format`]).
pub fn format(src: &str) -> Result<String, Error> {
    let toks: Vec<Token> = lex(src)?.iter().map(|t| t.get()).collect();
    Ok(Fmt::new(src, toks).run())
}

/// Ends an operand: a `-` after it subtracts, a line end after it ends the
/// statement.
fn operand_end(t: Tok) -> bool {
    matches!(
        t,
        Tok::Num(_) | Tok::Hex(_) | Tok::Name | Tok::RParen | Tok::RBracket | Tok::Str | Tok::Raw
    )
}

/// Separates the sums and products of an expression from each other and
/// is written with spaces round it.
fn spaced_separator(t: Tok) -> bool {
    matches!(
        t,
        Tok::Lt
            | Tok::Le
            | Tok::Gt
            | Tok::Ge
            | Tok::EqEq
            | Tok::Ne
            | Tok::And
            | Tok::Or
            | Tok::Question
            | Tok::Colon
            | Tok::Assign
    )
}

/// What a gap between two tokens holds, in order.
enum Item<'a> {
    /// A line end (`nl`) or a `;`.
    Break {
        nl: bool,
    },
    Comment(&'a str),
}

struct Fmt<'s> {
    src: &'s str,
    /// The tokens but the line ends (`Tok::Newline`: every one is a line end
    /// or `;` in the gap after the token before it) and the end.
    toks: Vec<Token>,
    /// Per token: a `-`/`+`/`!`/`not` that applies to what follows.
    unary: Vec<bool>,
    /// Per token: a `*`, `/` or `%` written close up.
    close: Vec<bool>,
    /// Per `(`/`[`: its `)`/`]` (`toks.len()` when it has none).
    pair: Vec<usize>,
    /// Whether a line end comes before the token (in the source's tokens).
    nl_before: Vec<bool>,
}

impl<'s> Fmt<'s> {
    fn new(src: &'s str, all: Vec<Token>) -> Fmt<'s> {
        let mut toks = Vec::with_capacity(all.len());
        let mut nl_before = Vec::with_capacity(all.len());
        let mut nl = false;
        for t in all {
            match t.tok {
                Tok::Newline => nl = true,
                Tok::Eof => {}
                _ => {
                    toks.push(t);
                    nl_before.push(nl);
                    nl = false;
                }
            }
        }
        let n = toks.len();
        let mut f = Fmt {
            src,
            toks,
            unary: vec![false; n],
            close: vec![false; n],
            pair: vec![n; n],
            nl_before,
        };
        f.pairs();
        f.unaries();
        f.level(0, n);
        f
    }

    fn text(&self, i: usize) -> &'s str {
        let t = self.toks[i];
        &self.src[t.start..t.end]
    }

    fn pairs(&mut self) {
        let mut open: Vec<usize> = Vec::new();
        for i in 0..self.toks.len() {
            match self.toks[i].tok {
                Tok::LParen | Tok::LBracket => open.push(i),
                Tok::RParen | Tok::RBracket => {
                    if let Some(o) = open.pop() {
                        self.pair[o] = i;
                    }
                }
                // A block's brace closes what it holds (as the parser
                // would refuse an open bracket across it anyway).
                Tok::LBrace | Tok::RBrace => open.clear(),
                _ => {}
            }
        }
    }

    /// Which `-`, `+` negate: one with no operand just before it on its
    /// line (or a line that goes on after an operator), and every one on a
    /// `control` line (`control k -1..1`).
    fn unaries(&mut self) {
        let mut control = false;
        for i in 0..self.toks.len() {
            let t = self.toks[i];
            let prev = i.checked_sub(1).map(|p| self.toks[p].tok);
            let starts = match prev {
                None => true,
                Some(p) => {
                    (self.nl_before[i] && operand_end(p)) || matches!(p, Tok::LBrace | Tok::RBrace)
                }
            };
            if starts {
                control = t.tok == Tok::Name && self.text(i) == "control";
            }
            self.unary[i] = match t.tok {
                Tok::Minus | Tok::Plus => control || !prev.is_some_and(operand_end) || starts,
                Tok::Not => true,
                _ => false,
            };
        }
    }

    /// The tokens from `a` to before `b`, at one depth of brackets: decides
    /// which of their products are written close up. Whether anything in
    /// them is written with spaces round it.
    fn level(&mut self, a: usize, b: usize) -> bool {
        let mut spaced = false;
        let mut seg = Segment::default();
        let mut i = a;
        while i < b {
            let t = self.toks[i];
            // A line end before an operand-ended token ends the statement:
            // a new segment.
            if i > a && self.nl_before[i] && operand_end(self.toks[i - 1].tok) {
                spaced |= self.finish(&mut seg);
            }
            match t.tok {
                Tok::LParen | Tok::LBracket => {
                    let c = self.pair[i].min(b);
                    let inner = self.level(i + 1, c);
                    spaced |= inner;
                    seg.complex |= inner;
                    i = c + 1;
                    continue;
                }
                Tok::Plus | Tok::Minus if !self.unary[i] => seg.add = true,
                Tok::Star | Tok::Slash | Tok::Percent => seg.mul.push(i),
                Tok::Plus | Tok::Minus | Tok::Num(_) | Tok::Hex(_) | Tok::Name | Tok::Dot => {}
                Tok::Not if self.text(i) == "!" => {}
                _ => {
                    spaced |= self.finish(&mut seg);
                    spaced |= spaced_separator(t.tok) || t.tok == Tok::Not;
                }
            }
            i += 1;
        }
        spaced |= self.finish(&mut seg);
        spaced
    }

    /// Ends a segment (a sum of products between separators): its products
    /// close up when it also adds, or when none of its brackets is spaced.
    /// Whether it is written with spaces.
    fn finish(&mut self, seg: &mut Segment) -> bool {
        let close = seg.add || !seg.complex;
        for &m in &seg.mul {
            self.close[m] = close;
        }
        let spaced = seg.add || (!seg.mul.is_empty() && !close);
        *seg = Segment::default();
        spaced
    }

    /// No space between `p` and `i` on a line.
    fn glued(&self, p: usize, i: usize) -> bool {
        let (pt, t) = (self.toks[p].tok, self.toks[i].tok);
        if matches!(
            t,
            Tok::Comma | Tok::RParen | Tok::RBracket | Tok::Dot | Tok::DotDot
        ) || matches!(pt, Tok::LParen | Tok::LBracket | Tok::Dot | Tok::DotDot)
        {
            return true;
        }
        if self.unary[p] && self.text(p) != "not" {
            return true;
        }
        if matches!(t, Tok::LParen | Tok::LBracket) && pt == Tok::Name {
            return true;
        }
        self.close[p] || self.close[i]
    }

    /// The gap before token `i` (or after the last, `i == len`).
    fn gap(&self, i: usize) -> &'s str {
        let from = i.checked_sub(1).map_or(0, |p| self.toks[p].end);
        let to = self.toks.get(i).map_or(self.src.len(), |t| t.start);
        &self.src[from..to]
    }

    fn items(gap: &str) -> Vec<Item<'_>> {
        let b = gap.as_bytes();
        let mut out = Vec::new();
        let mut j = 0;
        while j < b.len() {
            match b[j] {
                b'\n' => out.push(Item::Break { nl: true }),
                b';' => out.push(Item::Break { nl: false }),
                b'-' | b'/' if b.get(j + 1) == Some(&b[j]) => {
                    let e = gap[j..].find('\n').map_or(b.len(), |k| j + k);
                    out.push(Item::Comment(gap[j..e].trim_end()));
                    j = e;
                    continue;
                }
                _ => {}
            }
            j += 1;
        }
        out
    }

    fn run(&self) -> String {
        let n = self.toks.len();
        let mut out = Out::default();
        let mut braces = 0usize;
        let mut brackets: Vec<usize> = Vec::new();
        let mut cont = false;
        for i in 0..=n {
            let prev = i.checked_sub(1);
            let tok = self.toks.get(i).map(|t| t.tok);
            // Where a line starting here goes: inside how many braces and
            // brackets, and whether it goes on from an operator.
            let mut decided = false;
            let mut line_cont = |cont: &mut bool| {
                if !decided && brackets.is_empty() {
                    if let Some(p) = prev {
                        let pt = self.toks[p].tok;
                        *cont = !(operand_end(pt) || matches!(pt, Tok::LBrace | Tok::RBrace));
                    }
                    decided = true;
                }
            };
            let indent = |cont: bool, closing: bool| {
                let b = if closing && tok == Some(Tok::RBrace) {
                    braces.saturating_sub(1)
                } else {
                    braces
                };
                let k = if closing && matches!(tok, Some(Tok::RParen | Tok::RBracket)) {
                    brackets.len().saturating_sub(1)
                } else {
                    brackets.len()
                };
                2 * (b + k + usize::from(cont && tok != Some(Tok::RBrace)))
            };
            let after_open = prev.is_some_and(|p| self.toks[p].tok == Tok::LBrace);
            let mut broke = prev.is_none();
            let (mut nl_run, mut blank) = (0, false);
            let mut since_content = true; // nothing but the token before since its line began
            for item in Self::items(self.gap(i)) {
                match item {
                    Item::Break { nl } => {
                        broke = true;
                        if nl {
                            nl_run += 1;
                            blank |= nl_run >= 2;
                        }
                    }
                    Item::Comment(c) => {
                        if !broke {
                            out.cur().comment = Some(c);
                        } else {
                            line_cont(&mut cont);
                            out.line(blank && !(after_open && since_content), indent(cont, false));
                            out.cur().code.push_str(c);
                        }
                        broke = false;
                        since_content = false;
                        (nl_run, blank) = (0, false);
                    }
                }
            }
            let Some(t) = tok else { break };
            // A block's statements go on lines of their own: `frame { a = 1 }`
            // opens out (line ends next to a brace change nothing).
            broke |= after_open || (t == Tok::RBrace && prev.is_some());
            if broke {
                line_cont(&mut cont);
                let keep = blank && t != Tok::RBrace && !(after_open && since_content);
                out.line(keep, indent(cont, true));
            } else if let Some(p) = prev {
                let s = self.text(i);
                let line = &out.cur().code;
                let last = line.as_bytes().last().copied().unwrap_or(b' ');
                let first = s.as_bytes()[0];
                let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
                let clash = (last == first && matches!(last, b'-' | b'/' | b'.'))
                    || (word(last) && word(first));
                if !self.glued(p, i) || clash {
                    out.cur().code.push(' ');
                }
            }
            out.cur().code.push_str(self.text(i));
            match t {
                Tok::LBrace => braces += 1,
                Tok::RBrace => {
                    braces = braces.saturating_sub(1);
                    brackets.clear();
                }
                Tok::LParen | Tok::LBracket => brackets.push(i),
                Tok::RParen | Tok::RBracket => {
                    brackets.pop();
                }
                _ => {}
            }
        }
        out.finish()
    }
}

#[derive(Default)]
struct Segment {
    add: bool,
    mul: Vec<usize>,
    /// A bracket in it is written with spaces.
    complex: bool,
}

/// The lines written so far: code, and a comment after it.
#[derive(Default)]
struct Out<'s> {
    lines: Vec<Line<'s>>,
}

#[derive(Default)]
struct Line<'s> {
    blank_before: bool,
    code: String,
    comment: Option<&'s str>,
}

impl<'s> Out<'s> {
    fn cur(&mut self) -> &mut Line<'s> {
        if self.lines.is_empty() {
            self.lines.push(Line::default());
        }
        let k = self.lines.len() - 1;
        &mut self.lines[k]
    }

    /// A new line (after a blank one, if `blank`), indented.
    fn line(&mut self, blank: bool, indent: usize) {
        let mut l = Line {
            blank_before: blank && !self.lines.is_empty(),
            ..Line::default()
        };
        l.code.extend(core::iter::repeat_n(' ', indent));
        self.lines.push(l);
    }

    /// The text: the comments after code on lines next to each other lined
    /// up, a space past the longest line's code.
    fn finish(self) -> String {
        let mut s = String::new();
        let mut k = 0;
        while k < self.lines.len() {
            // A run of lines with comments after their code.
            let mut e = k;
            while e < self.lines.len()
                && self.lines[e].comment.is_some()
                && (e == k || !self.lines[e].blank_before)
            {
                e += 1;
            }
            let run = if e > k { k..e } else { k..k + 1 };
            let width = self.lines[run.clone()]
                .iter()
                .map(|l| l.code.chars().count())
                .max()
                .unwrap_or(0);
            for l in &self.lines[run.clone()] {
                if l.blank_before {
                    s.push('\n');
                }
                s.push_str(&l.code);
                if let Some(c) = l.comment {
                    let pad = if l.code.is_empty() {
                        0
                    } else {
                        width + 1 - l.code.chars().count()
                    };
                    s.extend(core::iter::repeat_n(' ', pad));
                    s.push_str(c);
                }
                s.push('\n');
            }
            k = run.end;
        }
        s
    }
}
