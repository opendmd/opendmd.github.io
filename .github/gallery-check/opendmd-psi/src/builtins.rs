//! The built-ins: every name the language knows, in one place. The
//! compiler reads these tables; the web app's highlighter and the editor's
//! cheat sheet are generated from them (the `docs` feature adds the cheat
//! sheet's rows), and `tests/builtins.rs` checks that the generated blocks
//! in `opendmd-web/www/js/psikit.js` and `js/fhint.js` (with the completions) are what these
//! tables give (`PSI_BLESS=1` rewrites them) — opendmd-notes docs/research/psi-tooling-plan.md §3.
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

/// The values a formula reads by name. Any of them may also be set as a
/// formula's own name (then that name is the formula's: `t = 3` is a value
/// called `t`, `c = 0.5` no longer the third control).
pub const INPUTS: [(&str, Input, &str); 17] = [
    (
        "x",
        Input::X,
        "the pixel's position across, 0 at the left to 1 at the right",
    ),
    (
        "y",
        Input::Y,
        "the pixel's position down, 0 at the top to 1 at the bottom",
    ),
    (
        "px",
        Input::Px,
        "the pixel's column, 0 to 127 from the left",
    ),
    ("py", Input::Py, "the pixel's row, 0 to 31 from the top"),
    ("i", Input::I, "the pixel's number, 0 to 4095 (py*128 + px)"),
    ("t", Input::T, "seconds into the loop"),
    ("hour", Input::Hour, "the hour, 0 to 23"),
    ("minute", Input::Minute, "the minute"),
    ("second", Input::Second, "the second"),
    ("a", Input::A, "the first control"),
    ("b", Input::B, "the second control"),
    ("c", Input::C, "the third control"),
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
pub const LATER: [&str; 4] = ["cx", "cy", "r", "angle"];

/// The panel's size: constants.
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
/// (`cycle`, a colour, since console-b 2026-10-29.)
pub const LATER_FUNCS: [&str; 5] = ["bit", "digit", "glyph", "pick", "cycle"];

/// The fonts `digit` and `glyph` take as a last value: core's 5x7 (the
/// panel's text and clock; 4 × 6 digits; the default), 4x6 (3 × 5 digits)
/// and the bold 3 × 5 digits (digits only). As `Tables::glyphs` takes them.
pub const FONTS: [(&str, u8); 3] = [
    ("normal", tables::NORMAL),
    ("small", tables::SMALL),
    ("bold", tables::BOLD),
];

pub const COLOURS: [(&str, usize, u8); 5] = [
    ("rgb", 3, code::OUT_RGB),
    ("hsv", 3, code::OUT_HSV),
    ("pal", 2, code::OUT_PAL),
    ("gray", 1, code::OUT_GRAY),
    // `cycle(P, v, s)`: palette P turned by s (`code::TURN`), at v.
    ("cycle", 3, code::OUT_PAL),
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

pub const KEYWORDS: [&str; 8] = ["loop", "layer", "part", "use", "draw", "for", "table", "fn"];

/// The draw stage's primitives: statements inside `draw { }`, and names a
/// formula may still set (`line = …`), as `ink` is a value it may.
pub const PRIMITIVES: [&str; 11] = [
    "line", "dot", "pset", "rect", "rectfill", "circ", "circfill", "trifill", "cls", "spr", "print",
];

/// The ink primitives: `line` and `dot` with a number (a brightness) last
/// draw ink; with a colour last they paint.
pub const INK_CALLS: [&str; 2] = ["line", "dot"];

/// The paint primitives: statements inside `draw { }` that draw in colour
/// onto the paint canvas (the last drawn wins). `line` and `dot` paint too
/// when their last value is a colour.
pub const PAINT_CALLS: [&str; 9] = [
    "pset", "rect", "rectfill", "circ", "circfill", "trifill", "cls", "spr", "print",
];

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
/// `key` line, `rate N` after `layer` (and its blend).
pub const CONTEXT_WORDS: [&str; 6] = ["fps", "forever", "sprite", "palette", "key", "rate"];

/// The editor's cheat sheet (`js/fhint.js` `VARS FUNCS DRAW COLS
/// DECLS`, generated from these): what to write, what it does, and what a
/// tap inserts when that is not the first column.
#[cfg(feature = "docs")]
pub mod cheat {
    /// A row: (what to write, what it does, what a tap inserts or "").
    pub type Row = (&'static str, &'static str, &'static str);

    pub const VARS: &[Row] = &[
        ("x, y", "where the pixel is: x 0 at the left to 1 at the right, y 0 at the top to 1 at the bottom", ""),
        ("px, py", "the pixel’s column 0–127 from the left and row 0–31 from the top", ""),
        ("t", "seconds into the loop", ""),
        ("hour, minute, second", "the time of day", ""),
        ("a, b, c", "the controls, in the order the control lines declare them (0 to 1 when undeclared); any built-in name may be set as your own", ""),
        ("w, h", "128 and 32", ""),
        ("i", "the pixel’s number, 0–4095", ""),
        ("cx, cy", "from the middle: −2 to 2 across, −0.5 to 0.5 down", ""),
        ("r, angle", "distance and angle (turns, clockwise from the right) from the middle", ""),
        ("ur, ug, ub", "the colour under the formula: the GIF it is drawn over, 0 to 1", ""),
        ("ul", "how bright that is", ""),
        ("under(x, y)", "the picture under the formula at pixel x, y (the nearest, held to the panel), as a #… colour. under(px + 2, py) shifts it; under(floor(px/4)*4, floor(py/4)*4) pixelates it. under alone is this pixel's", ""),
        ("daylight", "how light it is outside where the panel is, 0 at night to 1 by day (twilight between); the panel's location in Settings, else 1", ""),
        ("sun.alt", "the sun's height in turns: 0 on the horizon, 0.25 straight up, below 0 at night; 0.1 without a location", ""),
        ("moon.phase, season, yearday", "the moon (0 new, 0.5 full), the time of year (0 midwinter, 0.5 midsummer) and the day of the year (0 on 1 January)", ""),
        ("night, bright", "1 while night mode dims the panel; the panel's brightness, 0 to 1", ""),
        ("replay.playing, replay.platform, replay.game", "RePlayOS: 1 while a game plays; its platform as a number (0 none); a number 0 to 1 for the game, the same each time it plays", ""),
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
        ("since(replay.game)", "seconds since a built-in input last changed, 32767 if it never has. max(0, 1 - since(replay.game)) flashes on a change", ""),
        ("was(replay.game)", "a built-in input's value before its last change. mix(was(k), k, min(1, since(k))) fades from the old to the new", ""),
        ("digit(n, col, row)", "1 where digit n has a pixel in the panel’s font; col 0–4, row 0–6 from the top. Add , small or , bold (chunky digits) for another font", ""),
        ("glyph('A', col, row)", "the same for a character", ""),
        ("bit(n, i)", "bit i of a whole number: 1 or 0", ""),
        ("< <= > >= == !=", "compare: 1 or 0; and, or, not", ""),
        ("%", "remainder: v % 1 is the fraction", ""),
        (";", "a new line, on the same line", ""),
        ("loop 8", "the loop’s length in seconds (first line)", ""),
        ("loop forever", "goes on without repeating: t grows from when it starts (use frac, %, sin, noise of it); seed is 0 to 1, new each time it plays", ""),
        ("fps 100", "frames a second: 25, 50 (else) or 100, for fast motion; a light formula only", ""),
        ("layer add { … }", "a layer with its own names and colour, on what is below: over, add, multiply, screen, max or min", ""),
        ("layer rate 4 { … }", "a slow bottom layer: a band of its rows is redrawn each frame, so every row every 4th frame, and all of it when the clock or a control it reads changes. The layers above draw over it every frame", "layer rate 4 {\n  \n}\n"),
        ("fn ring(cx, r) = abs(len(px - cx, py - 16) - r) < 1", "a function: a value worked out from its values, used as ring(64, 8). With { … } instead of = …, it holds lines that draw, called on a line of their own in draw", "fn f(a) = "),
        ("fn p(x, y) { … }", "a function whose values are x and y, so px, py, r, angle … inside follow them. With a colour as its last line, p(1 - x, y) draws it mirrored", "fn p(x, y) {\n  \n}\n"),
        ("use Plasma as p", "puts in one of your formulas or an example, as a function fn p(x, y) { … }", "use Plasma as p\n"),
    ];

    pub const DRAW: &[Row] = &[
        ("draw { … }", "once a frame, before the pixels: values, loops, lines and dots into ink", "draw {\n  \n}\n"),
        ("for k < 12 { … }", "inside draw: k = 0, 1, … 11; two deep at most, 2048 turns a frame", "for k < 12 {\n  \n}"),
        ("line(x0, y0, x1, y1, v0, v1)", "a line, in pixels from the top left; v0, v1 its ink at each end, 0 to 1 (else 1)", ""),
        ("dot(x, y, v)", "one pixel of ink v", ""),
        ("cls(#102)", "paint the whole panel one colour. Paint is drawn over the colour line, and the last painted wins", ""),
        ("pset(x, y, #f80)", "paint one pixel; line(x0, y0, x1, y1, c) and dot(x, y, c) paint too when their last value is a colour", ""),
        ("rect(x0, y0, x1, y1, c) rectfill(…)", "paint a box (its corners included), or a filled one", ""),
        ("circ(x, y, r, c) circfill(…)", "paint a circle, or a filled one", ""),
        ("trifill(x0, y0, x1, y1, x2, y2, c)", "paint a filled triangle", ""),
        ("print(\"TEXT\", x, y, c)", "paint text in the panel's font (add small); or a number: print(minute, x, y, c, small, 2) with at least 2 digits", ""),
        ("spr(s, x, y, flip)", "paint sprite s with its top left at x, y, in its key's colours (key . 0, R #f00); flip 1 across, 2 down, 3 both", ""),
        ("pal(fire, v), gray(v)", "colours to paint with besides #…: a palette's or a gray, v 0 to 1", ""),
        ("if c { … } else { … }", "inside draw: only when c is not 0", "if  {\n  \n}"),
        ("ink", "what draw put at the pixel, 0 to 1: pal(ice, ink), #40c0ff * ink", ""),
        ("ink(16)", "the same in 16 levels, as a whole number. Ink k/16 reads exactly k, for pick(ink(16), …); 1 to 255 levels", ""),
        ("table T = 1, -2, 0.5", "numbers, 256 a table and 1024 in all (tables and sprites)", "table T = "),
        ("T[k]", "number k of table T, from 0 (0 outside it)", ""),
        ("sprite s { … }", "a small picture in characters. First the key, key . 0, X 1, gives each character the number it reads as (up to 16); then rows up to 64 wide and 32 tall", "sprite s {\n  key . 0, X 1\n  .X.\n}\n"),
        ("s[u, v]", "sprite s at column u, row v from its top left: its key’s number (0 outside)", ""),
    ];

    pub const COLS: &[Row] = &[
        ("hsv(hue, sat, val)", "colour: hue in turns, 0 to 1 each", ""),
        ("rgb(r, g, b)", "red, green, blue: 0 to 1", ""),
        ("pal(fire, v)", "a palette: fire, ice, ocean, chrono, amber, rainbow, gray", ""),
        ("palette sea = #024, #08c, #fff", "your own palette of up to 32 colours (8 palettes; pal and cycle read 3). pal(sea, 0) is the first, pal(sea, 1) the last, pal(sea, 0.5) the middle one", "palette p = #000, #f80, #fff\n"),
        ("scene s when c { v = … }", "values worked out only while c is not 0 (c once a frame or a row). After it, its names read 0 while it is off. The estimate counts two scenes at once", "scene s when  {\n  \n}\n"),
        ("palette sky = tween(day, night, w)", "a palette mixed once a frame, day towards night by w (0 to 1, of t or sliders); tweens nest; tween(a, b, w), tween(c, d, v) fills the low and high 16 colours", "palette p = tween(, , )\n"),
        ("palette sea = deep #024, foam #fff", "a colour may have a name: sea.foam is where pal finds it (sea.step from one colour to the next, sea.n how many)", ""),
        ("pal(fire, v) * k", "a palette colour, dimmed (or lit) by k", ""),
        ("cycle(fire, v, s)", "palette cycling: the palette read at v, turned by s (one turn per 1, so t/4 turns it every 4 s). It wraps where pal stops; s may change by row; cheap when v is the same every frame", ""),
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
        ("look screen 40", "it can be the panel’s look, drawn over the GIF with a blend and an amount (look alone is over, 100)", "look\n"),
    ];
}
