//! A compile or load error: a byte range of the source (0..0 when there
//! is none) and a sentence.

use alloc::format;
use alloc::string::String;

/// A slice of `s` (empty where the range is not on a boundary).
fn sub(s: &str, a: usize, b: usize) -> &str {
    s.get(a..b).unwrap_or("")
}

/// A compile error: a byte range of the source and a sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub start: usize,
    pub end: usize,
    pub msg: String,
}

impl Error {
    #[inline(never)]
    pub(crate) fn new(start: usize, end: usize, msg: impl Into<String>) -> Error {
        Error {
            start,
            end,
            msg: msg.into(),
        }
    }

    /// 1-based line and column of the start.
    pub fn line_col(&self, src: &str) -> (usize, usize) {
        let upto = sub(src, 0, self.start.min(src.len()));
        let line = upto.matches('\n').count() + 1;
        let col = upto.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        (line, col)
    }

    /// "line 2: msg", the line, and carets under the range.
    pub fn render(&self, src: &str) -> String {
        let (line, col) = self.line_col(src);
        let text = src.lines().nth(line - 1).unwrap_or("");
        let width = sub(src, self.start, self.end.min(src.len()))
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

/// The sentence for a compile the heap can't hold just now (a list the
/// compiler couldn't grow): the player refuses the Formula and goes on,
/// as with any other compile error, instead of aborting.
pub const NO_MEMORY: &str = "not enough memory for this Formula right now";
