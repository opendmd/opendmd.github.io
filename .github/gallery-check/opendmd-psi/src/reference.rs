//! The language reference's own material (docs/psi-spec.md, the site's
//! /dev/psi/): a tiny live example for every row of the editor's cheat
//! sheet ([`crate::builtins::cheat`]), and a longer word with a before and
//! after for the common error codes ([`crate::errors`]). The tests
//! (`tests/reference.rs`) compile every example and check it lights the
//! panel, check that every cheat-sheet row has one, that every `before`
//! fails with its code and every `after` compiles; `tests/builtins.rs` and
//! `tests/diagnostics.rs` write them into the reference's generated blocks.
//! The compiler never reads any of this (feature `docs`).
//!
//! Not part of the crate's API (`#[doc(hidden)]`).

/// A cheat-sheet row's example: (the row's first column, as in
/// [`crate::builtins::cheat`], a formula). One line where it can be (`;`
/// between statements); a sprite's rows need lines of their own. Drawn on
/// the reference page at their controls' defaults, over the sample picture
/// when they read it.
pub const EXAMPLES: &[(&str, &str)] = &[
    // Values
    ("x, y", "rgb(x, y, 0.5)"),
    ("px, py", "gray(px % 8 == 0 or py % 8 == 0)"),
    ("t", "loop 4; hsv(x + t/4, 1, 1)"),
    ("hour, minute, second", "band = floor(y*3); hsv(band/3, 0.8, x < pick(band, second/60, minute/60, hour/24))"),
    ("a, b, c", "control size 0..1 = 0.5 \"Size\"; gray(r < size*2)"),
    ("w, h", "gray(px == 0 or py == 0 or px == w - 1 or py == h - 1)"),
    ("i", "hsv(i/4096, 1, 1)"),
    ("cx, cy", "rgb(abs(cx)/2, abs(cy)*2, 0.3)"),
    ("r, angle", "loop 4; hsv(angle + t/4, 1, 1 - r/2)"),
    ("ur, ug, ub", "rgb(ub, ug, ur)"),
    ("ul", "pal(fire, ul)"),
    ("under(x, y)", "loop 2; under(floor(px/4)*4 + 2 + floor(sin(t/2)*2), floor(py/4)*4 + 2)"),
    // Functions and statements
    ("sin(v) cos(v)", "loop 4; gray(abs(y - 0.5 - 0.4*sin(x*2 + t/4)) < 0.06)"),
    ("tri(v) saw(v) sqr(v)", "loop 4; k = floor(y*3); v = pick(k, tri(x*2 - t/4), saw(x*2 - t/4), sqr(x*2 - t/4)); hsv(k/3, 1, 0.5 + v/2)"),
    ("abs floor frac sqrt", "loop 4; gray(frac(x*4 - t/4) * sqrt(abs(cy)*2))"),
    ("min max clamp(v, lo, hi)", "gray(clamp(sin(x*2)*2, 0, 1))"),
    ("mix(a, b, k)", "mix(#f40, #04f, x)"),
    ("step(edge, v)", "loop 4; gray(step(frac(t/4), x))"),
    ("smoothstep(e0, e1, v)", "gray(smoothstep(0.3, 0.7, x))"),
    ("len(dx, dy)", "gray(len(px - 63.5, py - 15.5) < 12)"),
    ("atan2(dy, dx)", "hsv(atan2(py - 15.5, px - 63.5), 1, 1)"),
    ("noise(x, y, z)", "loop 8; gray(noise(x*8, y*2, t/2))"),
    ("hash(a, b)", "gray(hash(px, py) > 0.9)"),
    ("select(c, a, b)", "gray(select(x < 0.5, y, 1 - y))"),
    ("pick(k, a, b, …)", "pal(rainbow, pick(floor(x*4), 0, 0.3, 0.6, 0.9))"),
    ("digit(n, col, row)", "gray(digit(second % 10, floor(px/4) - 14, floor(py/4)))"),
    ("glyph('A', col, row)", "gray(glyph('A', floor(px/4) - 14, floor(py/4)))"),
    ("bit(n, i)", "gray(bit(px, floor(py/4)))"),
    ("< <= > >= == !=", "rgb(x < 0.5, y >= 0.5, 0.2)"),
    ("%", "gray((px % 16 < 8) != (py % 16 < 8))"),
    (";", "u = x; v = y; rgb(u, v, 0.5)"),
    ("loop 8", "loop 2; gray(frac(x - t/2))"),
    ("loop forever", "loop forever; hsv(frac(x + t/10), 1, 1)"),
    ("fps 100", "fps 100; loop 1; gray(abs(x - frac(t)) < 0.02)"),
    ("layer add { … }", "layer { gray(y*0.5) }; layer add { rgb(x, 0, 0) }"),
    ("layer rate 4 { … }", "loop 4; layer rate 4 { gray(noise(x*6, y*2, t)) }; layer { rgb(1, 0.5, 0, abs(x - frac(t/4)) < 0.01) }"),
    // (`use` is the editor's: it puts the formula in as a function, as here.)
    ("fn ring(cx, r) = abs(len(px - cx, py - 16) - r) < 1", "loop 4\nfn ring(cx, r) = abs(len(px - cx, py - 15.5) - r) < 1\nfn tree(x0, s) {\n  line(x0, 31, x0, 31 - 10*s, #a62)\n  circfill(x0, 31 - 10*s, 3*s, #2a4)\n}\ndraw { for k < 4 { tree(10 + k*30, 1 + k/4) } }\npal(ice, (ring(64 + sin(t/4)*30, 8) + ring(64, 12))*0.8)"),
    ("fn p(x, y) { … }", "loop 8\nfn p(x, y) {\n  wave = sin(x*2 + t/8) + sin(y*1.5 - t/4)\n  hsv(wave/4 + t/8, 0.9, r < 0.6)\n}\np(abs(x - 0.5)*2, y)"),
    ("use Plasma as p", "fn p(x, y) = hsv(x + t/8, 1, 1)\np(1 - x, y)"),
    // Drawing
    ("draw { … }", "draw { line(0, 0, 127, 31) }; pal(fire, ink)"),
    ("for k < 12 { … }", "draw { for k < 12 { line(k*10 + 8, 4, k*10 + 8, 27, 0.2, 1) } }; gray(ink)"),
    ("line(x0, y0, x1, y1, v0, v1)", "loop 4; draw { line(64, 16, 64 + 40*cos(t/4), 16 + 15*sin(t/4), 0.2, 1) }; pal(ice, ink)"),
    ("dot(x, y, v)", "draw { for k < 64 { dot(hash(k, 1)*128, hash(k, 2)*32, hash(k, 3)) } }; gray(ink)"),
    ("cls(#102)", "draw { cls(#102); circfill(64, 16, 10, #f80) }"),
    ("pset(x, y, #f80)", "loop 4; draw { for k < 64 { pset(hash(k, 1)*128, frac(hash(k, 2) + t/4)*32, #fff) } }"),
    ("rect(x0, y0, x1, y1, c) rectfill(…)", "draw { rectfill(10, 6, 50, 25, #048); rect(10, 6, 50, 25, #fff) }"),
    ("circ(x, y, r, c) circfill(…)", "loop 4; draw { circfill(64 + 40*sin(t/4), 16, 9, #f80); circ(64, 16, 14, #0ff) }"),
    ("trifill(x0, y0, x1, y1, x2, y2, c)", "draw { trifill(40, 28, 64, 4, 88, 28, #0c4) }"),
    ("print(\"TEXT\", x, y, c)", "draw { print(\"HELLO\", 4, 4, #ff0); print(minute, 90, 20, #0ff, small, 2) }"),
    ("spr(s, x, y, flip)", "sprite heart {\n  key . 0, R #e24\n  .R.R.\n  RRRRR\n  .RRR.\n  ..R..\n}\ndraw { spr(heart, 60, 12); spr(heart, 70, 12, 2) }"),
    ("pal(fire, v), gray(v)", "draw { for k < 16 { rectfill(k*8, 0, k*8 + 7, 31, pal(fire, k/15)) } }"),
    ("if c { … } else { … }", "draw { for k < 16 { if k % 2 == 0 { rectfill(k*8, 4, k*8 + 6, 27, #f80) } else { circfill(k*8 + 3, 16, 3, #0ff) } } }"),
    ("ink", "draw { line(0, 16, 127, 16) }; #40c0ff * ink"),
    ("ink(16)", "draw { for k < 16 { line(k*8, 0, k*8 + 7, 31, k/16) } }; pal(rainbow, ink(16)/15)"),
    ("table T = 1, -2, 0.5", "table T = 0.2, 0.6, 1, 0.6; gray(T[floor(x*4)])"),
    ("T[k]", "table hue = 0, 0.1, 0.3, 0.6; hsv(hue[px % 4], 1, 1)"),
    ("sprite s { … }", "sprite s {\n  key . 0, X 1\n  .X.\n  XXX\n  .X.\n}\ngray(s[floor(px/4) - 14, floor(py/4) - 2])"),
    ("s[u, v]", "sprite heart {\n  key . 0, X 1\n  .X.X.\n  XXXXX\n  .XXX.\n  ..X..\n}\nrgb(1, 0.2, 0.3) * heart[px % 6, py % 5]"),
    // Colours
    ("hsv(hue, sat, val)", "hsv(x, 1, y)"),
    ("rgb(r, g, b)", "rgb(x, y, 1 - x)"),
    ("pal(fire, v)", "pal(fire, y)"),
    ("palette sea = #024, #08c, #fff", "palette sea = #024, #08c, #fff; pal(sea, y)"),
    ("palette sea = deep #024, foam #fff", "palette sea = deep #024, mid #08c, foam #fff; pal(sea, sea.mid + sea.step*(y > 0.5))"),
    ("pal(fire, v) * k", "pal(fire, y) * x"),
    ("cycle(fire, v, s)", "loop 4; cycle(fire, x, t/4)"),
    ("gray(v)", "gray(x)"),
    ("#ff8800", "#f80 * y"),
    ("rgb(r, g, b, k)", "layer { pal(ice, x) }; layer { rgb(1, 0.5, 0, r < 0.6) }"),
    // Declarations
    ("name \"Sunrise\"", "name \"Sunrise\"; hsv(0.08, 1, y)"),
    ("about \"…\"", "about \"A warm glow\"; pal(fire, y)"),
    ("control speed 0..1 = 0.5 \"Speed\"", "control speed 0..2 = 1 \"Speed\"; loop 4; gray(frac(x - t*speed/4))"),
    ("control mode = calm, wild", "control mode = calm, wild; hsv(x, 1, pick(mode, 0.5, 1))"),
    ("look screen 40", "look screen 40; pal(fire, y)"),
];

/// The example for a cheat-sheet row.
pub fn example(row: &str) -> Option<&'static str> {
    EXAMPLES.iter().find(|e| e.0 == row).map(|e| e.1)
}

/// More about a common error: why it happens, and a formula that gives it
/// (`before`) beside the same formula put right (`after`); both empty where
/// a snippet would be too long to help (E402).
pub struct Help {
    pub code: u16,
    pub why: &'static str,
    pub before: &'static str,
    pub after: &'static str,
}

/// The common errors' help, ascending.
pub const HELP: &[Help] = &[
    Help {
        code: 1,
        why: "Psi has numbers, names, a few operators and brackets; anything else, such as `^`, `$` or a double quote outside a declaration, is not part of it.",
        before: "gray(x ^ 2)",
        after: "gray(x * x)",
    },
    Help {
        code: 5,
        why: "Psi never multiplies by putting things side by side, as maths on paper does.",
        before: "gray(2x)",
        after: "gray(2 * x)",
    },
    Help {
        code: 101,
        why: "Two values side by side with nothing between them: usually an operator or a comma is missing.",
        before: "v = x y\ngray(v)",
        after: "v = x * y\ngray(v)",
    },
    Help {
        code: 102,
        why: "Every `(` needs its `)`: the message points at the one left open.",
        before: "v = (x + 1\ngray(v / 2)",
        after: "v = (x + 1)\ngray(v / 2)",
    },
    Help {
        code: 103,
        why: "A block (`layer`, `draw`, `fn`, `for`, `if`, `sprite`) runs to its `}`; the formula ended first.",
        before: "layer {\n  gray(x)",
        after: "layer {\n  gray(x)\n}",
    },
    Help {
        code: 104,
        why: "The last line is the colour, and only the last: lines after it would never be used.",
        before: "gray(x)\nv = 1",
        after: "v = 1\ngray(x * v)",
    },
    Help {
        code: 106,
        why: "An operator, a `=` or a `,` with nothing after it.",
        before: "v =\ngray(v)",
        after: "v = x\ngray(v)",
    },
    Help {
        code: 108,
        why: "A line that is only a value, not the last line: it would be worked out and thrown away, so it is most likely a name left out.",
        before: "x + 1\ngray(x)",
        after: "v = x + 1\ngray(v / 2)",
    },
    Help {
        code: 114,
        why: "Each function takes a fixed number of values (the reference's tables say how many); colours may take one more, how much of the pixel they cover.",
        before: "gray(x, 2, 3)",
        after: "gray(x, 0.5)",
    },
    Help {
        code: 201,
        why: "A name is used before it is set, or is spelled differently where it is set.",
        before: "gray(level)",
        after: "level = x\ngray(level)",
    },
    Help {
        code: 202,
        why: "Only the built-in functions can be called; the message suggests the nearest one.",
        before: "gray(sine(x))",
        after: "gray(sin(x))",
    },
    Help {
        code: 203,
        why: "A value may be set again (`v = v * 2` reads the `v` before it), but functions, tables, sprites, palettes and controls keep their names: neither a value nor another of them can take one. Give it a name of its own.",
        before: "table T = 0.5, 1\nT = 2\ngray(T[0])",
        after: "table T = 0.5, 1\nk = 2\ngray(T[0] * k)",
    },
    Help {
        code: 204,
        why: "The language's own words (`loop`, `layer`, `part`, `use`, `draw`, `for`, `table`, `fn`) can't be names; any built-in value or function's name can (it is then yours).",
        before: "layer = 1\ngray(x * layer)",
        after: "level = 1\ngray(x * level)",
    },
    Help {
        code: 205,
        why: "A function's name alone is not a value: it needs its brackets and values.",
        before: "gray(sin)",
        after: "gray(sin(x))",
    },
    Help {
        code: 211,
        why: "The palettes are fire, ice, ocean, chrono, amber, rainbow and gray, and the ones the formula makes with `palette`.",
        before: "pal(lava, x)",
        after: "pal(fire, x)",
    },
    Help {
        code: 301,
        why: "There are no `frame { }` blocks: a value that reads only the time, the clock and the controls is worked out once a frame by itself.",
        before: "frame {\n  k = t / 8\n}\ngray(x * k)",
        after: "k = t / 8\ngray(x * k)",
    },
    Help {
        code: 302,
        why: "A colour such as `#f80` is not a number: it goes in the colour line, where it can be dimmed or mixed.",
        before: "v = #f80\ngray(v)",
        after: "v = x\n#f80 * v",
    },
    Help {
        code: 303,
        why: "Once a formula has a layer, each colour belongs to one: a colour line outside them would have nothing to go on.",
        before: "layer { gray(x) }\ngray(y)",
        after: "layer { gray(x) }\nlayer add { gray(y) }",
    },
    Help {
        code: 402,
        why: "The panel works a formula out for 4,096 pixels at every frame, so the steps a formula takes are limited. Values that change less often (once a row, once a frame) cost much less: move what you can out of the pixel.",
        before: "",
        after: "",
    },
    Help {
        code: 408,
        why: "The panel didn't have the memory to compile this Formula just then (a GIF, a look or a transition was holding it): not the formula's fault. It plays when it is next due; a shorter formula needs less.",
        before: "",
        after: "",
    },
    Help {
        code: 706,
        why: "`line` and `dot` draw into the frame's ink once a frame, so they live in `draw { }`; the pixels read what they drew as `ink`.",
        before: "line(0, 0, 127, 31)\ngray(1)",
        after: "draw {\n  line(0, 0, 127, 31)\n}\ngray(ink)",
    },
    Help {
        code: 708,
        why: "`draw { }` runs once a frame, before the pixels: there is no pixel position there. Work in pixels, with numbers and the time.",
        before: "draw { dot(x, 1) }\ngray(ink)",
        after: "draw { dot(t * 16, 1) }\ngray(ink)",
    },
    Help {
        code: 801,
        why: "The panel shows 25, 50 or 100 frames a second; nothing in between.",
        before: "fps 60\ngray(x)",
        after: "fps 50\ngray(x)",
    },
    Help {
        code: 901,
        why: "In a `loop forever` formula `t` grows without end, and most arithmetic on it would lose its precision or overflow. Wrap it first: `frac`, `%`, `sin` and `noise` keep it in range.",
        before: "loop forever\ngray(t*x)",
        after: "loop forever\ngray(frac(t/4 + x))",
    },
    Help {
        code: 905,
        why: "A colour has to be worked out from values that stay in range; `t` in a `loop forever` formula does not.",
        before: "loop forever\nv = t + x\ngray(v)",
        after: "loop forever\nv = frac(t/8 + x)\ngray(v)",
    },
    Help {
        code: 931,
        why: "`cycle` turns a whole row's palette at once, so how far it turns can change with the time or the row, not along the row.",
        before: "cycle(fire, x, x)",
        after: "loop 4\ncycle(fire, x, t/4 + y)",
    },
    Help {
        code: 951,
        why: "Declarations (`name`, `about`, `control`, `look`) say what the formula is, so they come first, before anything it works out.",
        before: "loop 4\nname \"Late\"\ngray(x)",
        after: "name \"Late\"\nloop 4\ngray(x)",
    },
    Help {
        code: 957,
        why: "A control is a slider (`control NAME lo..hi = default \"Label\"`) or a row of buttons (`control NAME = word, word`).",
        before: "control k 1 = 2\ngray(k)",
        after: "control k 0..2 = 1 \"Level\"\ngray(k / 2)",
    },
];

/// The codes that are a bug in Psi, never a formula's fault. The reference
/// marks them as internal. (E408, the panel short of memory for a moment,
/// is not a bug: it has its own help.)
pub const INTERNAL: &[u16] = &[407];

/// The help for `code`.
pub fn help(code: u16) -> Option<&'static Help> {
    HELP.iter().find(|h| h.code == code)
}
