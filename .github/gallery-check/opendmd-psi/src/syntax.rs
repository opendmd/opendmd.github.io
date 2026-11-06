//! The language service's hook (feature `syntax`, opendmd-notes docs/research/
//! psi-tooling-plan.md §2): the lexer's tokens, and a [`Sink`] the compiler
//! tells what it saw — the names a formula defines (where, what, at which
//! level), every name read and its definition, each op it made (where in
//! the source, its level, its cost), and the declarations. One parser, one
//! truth: a language service never parses Psi again.
//!
//! The firmware never turns the feature on (opendmd-core's `psi` feature
//! does not), so none of this, nor the parser's bookkeeping for it, is in
//! the player: the calls are compiled out.

use crate::compile::{lex_tokens, Error, Info};

mod fmt;
use crate::Program;
use alloc::vec::Vec;
pub use fmt::format;

/// What a token is ([`tokens`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `1.5`, `'A'`.
    Number,
    /// `#f80`.
    Colour,
    /// A name, a keyword, a built-in (the compiler tells them apart by
    /// where they are: [`Sink`]).
    Name,
    /// `"…"` on a `name`, `about` or `control` line.
    Text,
    /// A sprite's row or key line, as written.
    Row,
    /// `+ - * / % < <= > >= == != and or not ? :`.
    Operator,
    /// `( ) { } [ ] , . = ..`.
    Punctuation,
    /// A line's end (or `;`).
    End,
}

/// The tokens of `src` (comments are not tokens): what each is and where
/// (bytes). The lexer's error when it stops.
pub fn tokens(src: &str) -> Result<Vec<(Kind, usize, usize)>, Error> {
    lex_tokens(src)
}

/// What a defined name is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defined {
    /// `v = …`, or a `control` line's name.
    Value,
    /// A draw stage's `for k < n`.
    Counter,
    /// `fn name(…)`.
    Function,
    Table,
    Sprite,
    Palette,
}

/// How often a value is worked out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Constant,
    Frame,
    /// In the draw stage's loops.
    Draw,
    Row,
    Pixel,
}

/// What a compile saw ([`compile_observed`]). Every method has an empty
/// default: implement what you need. Told after the formula is read (or up
/// to the error that stopped it), before the program is laid out.
#[allow(unused_variables)]
pub trait Sink {
    /// A name the formula defines, the name's place, what it is, its scope
    /// (0 the formula's own lines; parts and layers have scopes of their
    /// own), how often it is worked out.
    fn defined(&mut self, name: &str, at: (usize, usize), what: Defined, scope: u16, level: Level) {
    }
    /// A name read at `at`, and the place of its definition when the
    /// formula defined it (`None`: a built-in, or unknown).
    fn read(&mut self, at: (usize, usize), defined: Option<(usize, usize)>) {}
    /// An op the compiler made for the source at `at` (a constant folded
    /// away or a value shared makes none): its level, its code, its cost in
    /// ns a pixel on the ESP32 (the estimate's; times the pixels, rows or
    /// frames its level runs over).
    fn op(&mut self, at: (usize, usize), level: Level, code: u8, cost_ns: u32) {}
    /// A declaration (`psi`, `name`, `about`, `control`, `look`): its word.
    fn declaration(&mut self, word: &str, at: (usize, usize)) {}
    /// Why the value defined at `at` (told after [`defined`](Self::defined))
    /// varies as much as it does: the nearest of the formula's own values
    /// on the way (`via`, its definition's place), else the built-in it
    /// reads (`input`, as `px`). Not told for constants.
    fn why(&mut self, at: (usize, usize), via: Option<(usize, usize)>, input: &str) {}
    /// A note about the cost (`psi check` prints them): a `pick` worked out
    /// per pixel, a sum or product that could be bracketed, what the still
    /// cache and the column run took or left, a draw loop run as lanes.
    /// Told once the program is laid out (not after an error).
    fn note(&mut self, at: (usize, usize), text: &str) {}
}

/// [`crate::compile_into`], telling `sink` what it saw.
pub fn compile_observed<'s>(
    src: &'s str,
    prog: &mut Program,
    sink: &'s mut (dyn Sink + 's),
) -> Result<Info, Error> {
    crate::compile::compile_observed(src, prog, sink)
}
