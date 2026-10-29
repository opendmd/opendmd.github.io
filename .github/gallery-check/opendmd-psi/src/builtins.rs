//! The built-ins: every name the language knows, in one place. The
//! compiler reads these tables; the web app's highlighter and the editor's
//! cheat sheet are generated from them (the `docs` feature adds the cheat
//! sheet's rows), and `tests/builtins.rs` checks that the generated blocks
//! in `opendmd-web/www/js/psikit.js` and `js/formulas.js` are what these
//! tables give (`PSI_BLESS=1` rewrites them) — docs/research/psi-tooling-plan.md §3.
//!
//! Not part of the crate's API (`#[doc(hidden)]`): the types are the
//! compiler's own.

use crate::fixed;
use crate::program::{code, H, W};
use crate::tables;

/// The built-in values, their level and what they mean (for errors).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Input {
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
    Ink,
    /// `seed` (a `loop forever` formula's: [`LATER`]-like, a name a formula
    /// sets is that name).
    Seed,
}

pub const INPUTS: [(&str, Input, &str); 17] = [
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
    (
        "ink",
        Input::Ink,
        "what `draw { }` put at the pixel, 0 to 1",
    ),
];

/// Values added once people had saved formulas (2026-10-26), about the
/// middle and from the top: `cx = (x − ½)·4`, `cy = y − ½`, `ty = 31 −
/// py`, `r = len(cx, cy)`, `angle = atan2(cy, cx)`, the steps those take
/// written out ([`Parser::centred`]). A name set with one of these is that
/// name ([`Parser::lookup_value`] finds the formula's names first), so a
/// saved `r = …` means what it meant.
pub const LATER: [&str; 5] = ["cx", "cy", "ty", "r", "angle"];

/// `w` and `h`: constants.
pub const CONSTS: [(&str, i32); 2] = [("w", (W as i32) << 16), ("h", (H as i32) << 16)];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum F {
    Op(u8),
    Smoothstep,
    Fract,
    /// `digit`/`glyph`: three values and an optional font word.
    Glyph(u8),
    /// `pick(k, a, b, …)`.
    Pick,
}

/// Functions: name, arity, what, the op.
pub const FUNCS: [(&str, usize, F); 25] = [
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
    ("bit", 2, F::Op(code::BIT)),
    ("digit", 3, F::Glyph(code::DIGIT)),
    ("glyph", 3, F::Glyph(code::GLYPH)),
    ("pick", 0, F::Pick),
];

/// Functions added once people had saved formulas (2026-10-26): a name
/// set with one of these still works as a value (`glyph = …; gray(glyph)`).
pub const LATER_FUNCS: [&str; 4] = ["bit", "digit", "glyph", "pick"];

/// The fonts `digit` and `glyph` take as a last value: core's 5x7 (the
/// panel's text and clock; 4 × 6 digits; the default), 4x6 (3 × 5 digits)
/// and the bold 3 × 5 digits (digits only). As `Tables::glyphs` takes them.
pub const FONTS: [(&str, u8); 3] = [
    ("normal", tables::NORMAL),
    ("small", tables::SMALL),
    ("bold", tables::BOLD),
];

pub const COLOURS: [(&str, usize, u8); 4] = [
    ("rgb", 3, code::OUT_RGB),
    ("hsv", 3, code::OUT_HSV),
    ("pal", 2, code::OUT_PAL),
    ("gray", 1, code::OUT_GRAY),
];

/// Palette names: core's order (fire, ice, chrono, amber, rainbow); `gray`
/// is not a table.
pub const PALETTES: [(&str, u8); 7] = [
    ("fire", 0),
    ("ice", 1),
    ("ocean", 1),
    ("chrono", 2),
    ("amber", 3),
    ("rainbow", 4),
    ("gray", 255),
];

pub const KEYWORDS: [&str; 8] = [
    "loop", "frame", "layer", "part", "use", "draw", "for", "table",
];

/// The draw stage's primitives: statements inside `draw { }`, and names a
/// formula may still set (`line = …`), as `ink` is a value it may.
pub const PRIMITIVES: [&str; 2] = ["line", "dot"];

/// How a layer goes onto what is below it (`layer add { … }`; `over` when
/// none is named).
pub const BLENDS: [(&str, u8); fixed::BLENDS] = [
    ("over", fixed::BLEND_OVER),
    ("add", fixed::BLEND_ADD),
    ("multiply", fixed::BLEND_MULTIPLY),
    ("screen", fixed::BLEND_SCREEN),
    ("max", fixed::BLEND_MAX),
    ("min", fixed::BLEND_MIN),
];


/// The declarations (formula-apps.md §2.1): statements at the top of a
/// formula that say what it is rather than draw. None is a keyword: each is
/// one only where its shape says so (`name "…"`, `control x …`, `look`
/// alone or with a blend or amount, `psi 1`), so a formula that set a value
/// called `name` or `look` before them still compiles.
pub const DECLARATIONS: [&str; 5] = ["psi", "name", "about", "control", "look"];

/// The language version this compiler is (`psi 1`; an unversioned formula
/// is version 1).
pub const VERSION: usize = 1;

/// Words that are keywords only in one place: `fps` before a number,
/// `forever` after `loop`, `sprite NAME {`, `palette NAME =`, a sprite's
/// `key` line.
pub const CONTEXT_WORDS: [&str; 5] = ["fps", "forever", "sprite", "palette", "key"];

/// The editor's cheat sheet (`js/formulas.js` `VARS FUNCS DRAW COLS
/// DECLS`, generated from these): what to write, what it does, and what a
/// tap inserts when that is not the first column.
#[cfg(feature = "docs")]
pub mod cheat {
    /// A row: (what to write, what it does, what a tap inserts or "").
    pub type Row = (&'static str, &'static str, &'static str);

    pub const VARS: &[Row] = &[
        ("x, y", "where the pixel is: 0 to 1 across, and up from the bottom", ""),
        ("px, py", "the pixel’s column 0–127 and row 0–31 from the bottom", ""),
        ("t", "seconds into the loop", ""),
        ("hour, minute, second", "the time of day", ""),
        ("a, b, c", "the controls, in the order the control lines declare them (0 to 1 when undeclared)", ""),
        ("w, h", "128 and 32", ""),
        ("i", "the pixel’s number, 0–4095", ""),
        ("cx, cy", "from the middle: −2 to 2 across, −0.5 to 0.5 up", ""),
        ("r, angle", "distance and angle (turns) from the middle", ""),
        ("ty", "row 0–31 from the top", ""),
        ("ur, ug, ub", "the colour under the formula: the GIF it is drawn over, 0 to 1", ""),
        ("ul", "how bright that is", ""),
    ];

    pub const FUNCS: &[Row] = &[
        ("sin(v) cos(v)", "a wave: one turn per 1, between −1 and 1", ""),
        ("tri(v) saw(v) sqr(v)", "triangle, sawtooth and square waves", ""),
        ("abs floor frac sqrt", "size, whole part, fraction, square root", ""),
        ("min max clamp(v, lo, hi)", "limits", ""),
        ("mix(a, b, k)", "from a to b as k goes 0 to 1", ""),
        ("step(edge, v)", "1 past the edge, else 0", ""),
        ("smoothstep(e0, e1, v)", "a soft step", ""),
        ("len(dx, dy)", "distance", ""),
        ("atan2(dy, dx)", "angle, in turns", ""),
        ("noise(x, y, z)", "smooth random hills, 0 to 1", ""),
        ("hash(a, b)", "random 0 to 1", ""),
        ("select(c, a, b)", "a when c is true (not 0), else b; or c ? a : b", ""),
        ("pick(k, a, b, …)", "a at k = 0, b at 1, …; else 0", ""),
        ("digit(n, col, row)", "the panel’s font: 1 where digit n has a pixel; col 0–4, row 0–6 from the top; add , small or , bold (chunky digits) for another font", ""),
        ("glyph('A', col, row)", "the same for a character", ""),
        ("bit(n, i)", "bit i of a whole number: 1 or 0", ""),
        ("< <= > >= == !=", "compare: 1 or 0; and, or, not", ""),
        ("%", "remainder: v % 1 is the fraction", ""),
        (";", "a new line, on the same line", ""),
        ("loop 8", "the loop’s length in seconds (first line)", ""),
        ("loop forever", "goes on without repeating: t grows from when it starts (use frac, %, sin, noise of it); seed is 0 to 1, new each time it plays", ""),
        ("fps 100", "frames a second: 25, 50 (else) or 100, for fast motion; a light formula only", ""),
        ("frame { … }", "values worked out once a frame", ""),
        ("layer add { … }", "a layer with its own names and colour, on what is below: over, add, multiply, screen, max or min", ""),
        ("use Plasma as p", "puts in one of your formulas or an example, as part p { … }", "use Plasma as p\n"),
        ("p.v", "a value part p sets", ""),
        ("p(x, y, k)", "part p as a colour, drawn at another place; k: how much it covers", ""),
    ];

    pub const DRAW: &[Row] = &[
        ("draw { … }", "once a frame, before the pixels: values, loops, lines and dots into ink", "draw {\n  \n}\n"),
        ("for k < 12 { … }", "inside draw: k = 0, 1, … 11; two deep at most, 2048 turns a frame", "for k < 12 {\n  \n}"),
        ("line(x0, y0, x1, y1, v0, v1)", "a line, in pixels up from the bottom left; v0, v1 its ink at each end, 0 to 1 (else 1)", ""),
        ("dot(x, y, v)", "one pixel of ink v", ""),
        ("ink", "what draw put at the pixel, 0 to 1: pal(ice, ink), #40c0ff * ink", ""),
        ("table T = 1, -2, 0.5", "numbers, 256 in all", "table T = "),
        ("T[k]", "number k of table T, from 0 (0 outside it)", ""),
        ("sprite s { … }", "a small picture in characters: first its key, key . 0, X 1 (each character and the number it reads as, up to 16), then rows up to 64 wide and 32 tall", "sprite s {\n  key . 0, X 1\n  .X.\n}\n"),
        ("s[u, v]", "sprite s at column u, row v from its top left: its key’s number (0 outside)", ""),
    ];

    pub const COLS: &[Row] = &[
        ("hsv(hue, sat, val)", "colour: hue in turns, 0 to 1 each", ""),
        ("rgb(r, g, b)", "red, green, blue: 0 to 1", ""),
        ("pal(fire, v)", "a palette: fire, ice, ocean, chrono, amber, rainbow, gray", ""),
        ("palette sea = #024, #08c, #fff", "your own palette, up to 32 colours (3 palettes): pal(sea, 0) the first, pal(sea, 1) the last, pal(sea, 0.5) the middle one", "palette p = #000, #f80, #fff\n"),
        ("pal(fire, v) * k", "a palette colour, dimmed (or lit) by k", ""),
        ("gray(v)", "white, 0 to 1", ""),
        ("#ff8800", "a colour: alone, or #f80 * v, mix(#000, #f80, v)", ""),
        ("rgb(r, g, b, k)", "a last value: how much it covers, 0 to 1 (what is under shows through); rgb(#f80, k)", ""),
    ];

    /// The declarations (at the top, before everything else).
    pub const DECLS: &[Row] = &[
        ("name \"Sunrise\"", "the formula’s name (60 characters at most)", "name \"\"\n"),
        ("about \"…\"", "one line about it (120 at most)", "about \"\"\n"),
        ("control speed 0..1 = 0.5 \"Speed\"", "a slider: its name in the formula, range, default and label (three controls at most)", "control speed 0..1 = 0.5 \"Speed\"\n"),
        ("control mode = calm, wild", "buttons: the value is the choice’s number, 0 for the first (pick(mode, …)); two words make a switch", "control mode = calm, wild\n"),
        ("look screen 40", "it can be the panel’s look: drawn over the GIF with a blend and an amount (look alone: over, 100)", "look\n"),
    ];
}
