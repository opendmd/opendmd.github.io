//! Golden frames, the never-panic fuzz, and host/wasm parity: these run
//! natively under `cargo test` and compiled to wasm32 under node
//! (`wasm-pack test --node`, `make test-wasm`) with the same expected
//! hashes, so a pass on both is bit-identical output.

use opendmd_psi::golden::{fnv, inputs, picture, FRAMES as GOLDEN, OVER};
use opendmd_psi::{
    compile, render_rows, render_with, sketches, Program, Scratch, FONT_4X6, FONT_5X7, FONT_BOLD,
    H, MAX_OPS, OWN, W,
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

fn frame<const N: usize>(p: &Program, ms: u32) -> Vec<u8> {
    let mut s = Scratch::<N>::new();
    let mut out = vec![0u8; W * H * 3];
    render_with::<N>(p, &mut s, &inputs(ms), &OWN, &mut out);
    out
}

/// The frame's hash at both lane widths (which must agree).
fn hash(src: &str, ms: u32) -> u64 {
    let p = compile(src).unwrap_or_else(|e| panic!("{}", e.render(src)));
    let a = frame::<128>(&p, ms);
    let b = frame::<64>(&p, ms);
    assert!(
        a == b,
        "lane widths 128 and 64 differ for {src:?} at {ms} ms"
    );
    fnv(&a)
}

#[test]
fn golden_frames() {
    let mut bad = Vec::new();
    for &(name, src, ms, want) in GOLDEN {
        let got = hash(src, ms);
        if got != want {
            bad.push(format!("    (\"{name}\", …, {ms}, {got:#x}),"));
        }
    }
    assert!(
        bad.is_empty(),
        "golden frames differ (got):\n{}",
        bad.join("\n")
    );
}

fn frame_over<const N: usize>(p: &Program, ms: u32, under: Option<&[u8]>) -> Vec<u8> {
    let mut s = Scratch::<N>::new();
    let mut out = vec![0x5a; W * H * 3];
    render_rows::<N>(p, &mut s, &inputs(ms), &OWN, under, u32::MAX, &mut out);
    out
}

/// Layers, and formulas over a picture: the same hashes at both lane
/// widths, here and in wasm (and on the ESP32, through `psi-probe`).
#[test]
fn golden_frames_over() {
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    let mut bad = Vec::new();
    for &(name, src, ms, over, want) in OVER {
        let p = compile(src).unwrap_or_else(|e| panic!("{name}: {}", e.render(src)));
        let u = over.then_some(under.as_slice());
        let a = frame_over::<128>(&p, ms, u);
        assert!(
            a == frame_over::<64>(&p, ms, u),
            "{name}: lane widths differ"
        );
        let got = fnv(&a);
        if got != want {
            bad.push(format!("    (\"{name}\", …, {ms}, {over}, {got:#x}),"));
        }
    }
    assert!(
        bad.is_empty(),
        "golden frames over differ (got):\n{}",
        bad.join("\n")
    );
}

/// Which pixels of `src`'s frame are lit (`gray` of 0 or 1: 255 or 0),
/// `[row from the top][column]`.
fn lit(src: &str) -> Vec<[bool; W]> {
    let p = compile(src).unwrap_or_else(|e| panic!("{}", e.render(src)));
    let f = frame::<128>(&p, 0);
    assert!(f == frame::<64>(&p, 0), "{src}: lane widths differ");
    (0..H)
        .map(|r| {
            core::array::from_fn(|c| {
                let v = f[(r * W + c) * 3];
                assert!(v == 0 || v == 255, "{src}: pixel {c}, {r} is {v}");
                v == 255
            })
        })
        .collect()
}

/// The font's pixel, as `opendmd_core::text` draws it: rows of bytes, the
/// most significant bit leftmost; blank outside the cell or the font.
fn font_pixel(rows: &[u8], w: i32, h: i32, glyph: i32, x: i32, y: i32) -> bool {
    (0..95).contains(&glyph)
        && (0..w).contains(&x)
        && (0..h).contains(&y)
        && rows[(glyph * h + y) as usize] & (0x80 >> x) != 0
}

/// `digit` and `glyph`, in both fonts: every pixel of every character, and
/// around each cell (a column and a row before and after it, which are
/// blank), against core's font tables, per pixel; out-of-range and
/// fractional characters (floored; blank).
#[test]
fn digits_and_glyphs_are_the_fonts() {
    for (font, rows, w, h) in [
        ("", FONT_5X7.as_flattened(), 5, 7),
        (", small", FONT_4X6.as_flattened(), 4, 6),
    ] {
        let (cw, ch) = (w + 2, h + 2); // the cell and a pixel either side
        let per = W as i32 / cw; // cells across
        for first in [0, 1, 2, 3, 4].map(|k| 32 - 2 + k * per * (H as i32 / ch)) {
            // Characters from `first` (some below 32 and above 126), a
            // quarter added (floored away).
            let src = format!(
                "k = {first} + floor(px/{cw}) + floor((31 - py)/{ch})*{per}\n\
                 gray(glyph(k + 0.25, px % {cw} - 1, (31 - py) % {ch} - 1{font}))"
            );
            let got = lit(&src);
            for (r, row) in got.iter().enumerate() {
                for (c, &l) in row.iter().enumerate() {
                    let (c, r) = (c as i32, r as i32);
                    let k = first + c / cw + r / ch * per;
                    let want = font_pixel(rows, w, h, k - 32, c % cw - 1, r % ch - 1);
                    assert_eq!(l, want, "glyph {k} ({src}) at {c}, {r}");
                }
            }
        }
        // Digits: -1 to 10 (blank but 0..9), 0.75 less (floored).
        let src = format!(
            "n = floor(px/{cw}) % 12 - 1.75 + 1\n\
             gray(digit(n, px % {cw} - 1, (31 - py) % {ch} - 1{font}))"
        );
        for (r, row) in lit(&src).iter().enumerate() {
            for (c, &l) in row.iter().enumerate() {
                let (c, r) = (c as i32, r as i32);
                let n = c / cw % 12 - 1;
                let want =
                    (0..10).contains(&n) && font_pixel(rows, w, h, 16 + n, c % cw - 1, r % ch - 1);
                assert_eq!(l, want, "digit {n} ({src}) at {c}, {r}");
            }
        }
        // Once a row (the character a constant, the row from py) and once a
        // frame (all constants: run, not folded).
        for n in [-1, 0, 7, 9, 10] {
            for x in -1..=w {
                let src = format!("gray(digit({n}, {x}, 31 - py{font}))");
                let src = src
                    .replace("digit(-1", "digit(0 - 1")
                    .replace(", -1,", ", 0 - 1,");
                for (r, row) in lit(&src).iter().enumerate() {
                    let want = (0..10).contains(&n) && font_pixel(rows, w, h, 16 + n, x, r as i32);
                    assert!(row.iter().all(|&l| l == want), "{src}: row {r}");
                }
            }
        }
        for (k, x, y) in [(65, 0, 2), (65, 2, 0), (58, 1, 1), (126, 1, 0), (127, 0, 0)] {
            let src = format!("gray(glyph({k}, {x}, {y}{font}))");
            let want = font_pixel(rows, w, h, k - 32, x, y);
            assert!(lit(&src).iter().flatten().all(|&l| l == want), "{src}");
        }
    }
    // The bold digits (digits only, a 4 × 6 cell, the table's 5 rows):
    // every pixel and around each cell, per pixel and once a row.
    let bold = FONT_BOLD.as_flattened();
    let src = "n = floor(px/6) % 12 - 1.75 + 1\n\
               gray(digit(n, px % 6 - 1, (31 - py) % 8 - 1, bold))";
    for (r, row) in lit(src).iter().enumerate() {
        for (c, &l) in row.iter().enumerate() {
            let (c, r) = (c as i32, r as i32);
            let n = c / 6 % 12 - 1;
            let (x, y) = (c % 6 - 1, r % 8 - 1);
            let want = (0..10).contains(&n) && x < 4 && font_pixel(bold, 4, 5, n, x, y);
            assert_eq!(l, want, "bold digit {n} at {c}, {r}");
        }
    }
    for n in [-1, 0, 4, 9, 10] {
        for x in -1..=4 {
            let src = format!("gray(digit({n}, {x}, 31 - py, bold))")
                .replace("digit(-1", "digit(0 - 1")
                .replace(", -1,", ", 0 - 1,");
            for (r, row) in lit(&src).iter().enumerate() {
                let want = (0..10).contains(&n) && font_pixel(bold, 4, 5, n, x, r as i32);
                assert!(row.iter().all(|&l| l == want), "{src}: row {r}");
            }
        }
    }
    // A character in quotes is its code.
    assert!(
        lit("gray('A' == 65 and ' ' == 32 and '~' == 126 and ''' == 39 and '-' - 1 == 44)")
            .iter()
            .flatten()
            .all(|&l| l)
    );
}

/// `bit(n, i)`: bit i of floor(n), two's complement; i floored, below 0
/// nothing; per pixel against the reference, and once (folded) at the edges.
#[test]
fn bits_are_twos_complement() {
    let reference = |n: f64, i: f64| -> bool {
        let (n, i) = (n.floor() as i64, i.floor() as i64);
        i >= 0 && (n >> i.min(40)) & 1 == 1
    };
    let got = lit("gray(bit(px*64.25 - 4096, py*0.75 - 4))");
    for (r, row) in got.iter().enumerate() {
        let py = (H - 1 - r) as f64;
        for (c, &l) in row.iter().enumerate() {
            let (n, i) = (c as f64 * 64.25 - 4096.0, py * 0.75 - 4.0);
            assert_eq!(l, reference(n, i), "bit({n}, {i})");
        }
    }
    for (n, i, want) in [
        ("5", "0", true),
        ("5", "1", false),
        ("5", "2", true),
        ("5", "0.99", true),
        ("5.99", "1", false),
        ("5", "0 - 1", false),
        ("5", "0 - 0.5", false),
        ("0.5", "0", false),
        ("32767", "14", true),
        ("32767", "15", false),
        ("16384", "14", true),
        ("0 - 1", "0", true),
        ("0 - 1", "15", true),
        ("0 - 1", "99", true),
        ("0 - 2", "0", false),
        ("0 - 0.25", "3", true),
        ("0", "99", false),
    ] {
        let src = format!("gray(bit({n}, {i}))");
        assert!(
            lit(&src).iter().flatten().all(|&l| l == want),
            "{src}: want {want}"
        );
    }
}

/// Any set of rows drawn alone is the same bytes as in the whole frame;
/// rows not asked for are left alone.
#[test]
fn rows_alone_are_the_same() {
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    for &(name, src, ms, over, _) in OVER.iter().chain(
        GOLDEN
            .iter()
            .map(|&(n, s, m, _)| (n, s, m, false, 0u64))
            .collect::<Vec<_>>()
            .iter(),
    ) {
        let p = compile(src).unwrap();
        let u = over.then_some(under.as_slice());
        let all = frame_over::<128>(&p, ms, u);
        for rows in [1u32, 0x8000_0001, 0x0f0f_0f0f, 0x1234_5678] {
            let mut s = Scratch::<128>::new();
            let mut out = vec![0x5a; W * H * 3];
            render_rows::<128>(&p, &mut s, &inputs(ms), &OWN, u, rows, &mut out);
            for r in 0..H {
                let row = r * W * 3..(r + 1) * W * 3;
                if rows & (1 << r) != 0 {
                    assert!(out[row.clone()] == all[row], "{name}: row {r}");
                } else {
                    assert!(
                        out[row].iter().all(|&b| b == 0x5a),
                        "{name}: row {r} touched"
                    );
                }
            }
        }
    }
}

#[test]
fn sketches_compile_small() {
    for (name, src) in sketches::ALL {
        let p = compile(src).unwrap();
        assert!(p.len() <= 40, "{name}: {} steps", p.len());
    }
}

/// xorshift64*: a reproducible stream for the fuzz.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PIECES: &[&str] = &[
    "x",
    "y",
    "px",
    "py",
    "i",
    "t",
    "w",
    "h",
    "hour",
    "minute",
    "second",
    "a",
    "b",
    "c",
    "0",
    "1",
    "0.5",
    "32767",
    "-",
    "+",
    "*",
    "/",
    "%",
    "<",
    "<=",
    ">",
    ">=",
    "==",
    "!=",
    "and",
    "or",
    "not",
    "?",
    ":",
    "(",
    ")",
    ",",
    "=",
    "\n",
    "{",
    "}",
    "frame",
    "loop",
    "4",
    "sin(",
    "cos(",
    "tri(",
    "saw(",
    "sqr(",
    "abs(",
    "min(",
    "max(",
    "clamp(",
    "floor(",
    "frac(",
    "mix(",
    "step(",
    "smoothstep(",
    "hash(",
    "noise(",
    "len(",
    "atan2(",
    "sqrt(",
    "rgb(",
    "hsv(",
    "pal(",
    "fire",
    "gray(",
    "v",
    "v =",
    "q =",
    "--",
    "//",
    ";",
    " ",
    "ψ",
    "é",
    "@",
    "\"",
    "99999",
    ".",
    "1e3",
    "_",
    "layer",
    "layer {",
    "layer add {",
    "multiply",
    "screen",
    "max",
    "ur",
    "ug",
    "ub",
    "ul",
    "digit(",
    "glyph(",
    "bit(",
    "'A'",
    "'",
    "small",
    "normal",
];

fn check(src: &str) {
    match compile(src) {
        Ok(p) => {
            assert!(p.len() <= MAX_OPS, "{src:?}: {} steps", p.len());
            let _ = frame::<128>(&p, 1234);
            let mut under = vec![0u8; W * H * 3];
            picture(&mut under);
            let _ = frame_over::<128>(&p, 1234, Some(&under));
        }
        Err(e) => {
            assert!(
                e.start <= e.end && e.end <= src.len().max(e.start),
                "{src:?}: span {e:?}"
            );
            assert!(
                src.is_char_boundary(e.start.min(src.len())),
                "{src:?}: span {e:?}"
            );
            let _ = e.render(src);
        }
    }
}

#[test]
fn garbage_never_panics() {
    let mut r = Rng(0x5eed_1234_abcd_ef01);
    for _ in 0..3000 {
        // Token soup.
        let n = 1 + r.below(40);
        let src: String = (0..n)
            .map(|_| PIECES[r.below(PIECES.len())])
            .collect::<Vec<_>>()
            .join(" ");
        check(&src);
        // Raw bytes (as UTF-8, lossily).
        let raw: Vec<u8> = (0..r.below(64)).map(|_| r.next() as u8).collect();
        check(&String::from_utf8_lossy(&raw));
    }
}

/// A random well-formed expression of depth ≤ `d`.
fn expr(r: &mut Rng, d: u32) -> String {
    const LEAVES: &[&str] = &[
        "x", "y", "px", "py", "i", "t", "hour", "a", "0.5", "3", "-2.25", "100", "w", "ur", "ul",
    ];
    if d == 0 || r.below(4) == 0 {
        return LEAVES[r.below(LEAVES.len())].to_string();
    }
    let e = |r: &mut Rng| expr(r, d - 1);
    match r.below(10) {
        0..=3 => {
            let op = ["+", "-", "*", "/", "%", "<", ">=", "==", "and", "or"][r.below(10)];
            format!("({} {op} {})", e(r), e(r))
        }
        4 => format!("-({})", e(r)),
        5 => format!("({} ? {} : {})", e(r), e(r), e(r)),
        6 | 7 => {
            let f = [
                "sin", "cos", "tri", "saw", "sqr", "abs", "floor", "frac", "sqrt",
            ][r.below(9)];
            format!("{f}({})", e(r))
        }
        8 => {
            let f = ["min", "max", "step", "hash", "len", "atan2"][r.below(6)];
            format!("{f}({}, {})", e(r), e(r))
        }
        _ => {
            let f = ["clamp", "mix", "smoothstep", "noise"][r.below(4)];
            format!("{f}({}, {}, {})", e(r), e(r), e(r))
        }
    }
}

#[test]
fn random_formulas_render_or_refuse() {
    let mut r = Rng(0xfeed_beef_0bad_cafe);
    let mut ok = 0;
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    for k in 0..600 {
        let src = if k % 2 == 0 {
            format!(
                "loop {}\nv = {}\nu = {}\nhsv({}, 1, {})",
                1 + k % 9,
                expr(&mut r, 4),
                expr(&mut r, 4),
                expr(&mut r, 3),
                expr(&mut r, 3)
            )
        } else {
            // Layers: a shared value, two or three layers of their own.
            let blends = ["", "add", "multiply", "screen", "max", "min", "over"];
            let mut s = format!("loop {}\nv = {}\n", 1 + k % 9, expr(&mut r, 3));
            for _ in 0..2 + r.below(2) {
                let cover = if r.below(2) == 0 {
                    format!(", {}", expr(&mut r, 2))
                } else {
                    String::new()
                };
                s += &format!(
                    "layer {} {{\nu = {}\nrgb(u, v, {}{cover})\n}}\n",
                    blends[r.below(blends.len())],
                    expr(&mut r, 3),
                    expr(&mut r, 2)
                );
            }
            s
        };
        match compile(&src) {
            Ok(p) => {
                ok += 1;
                assert!(p.len() <= MAX_OPS);
                // Both lane widths, bit for bit, alone and over a picture.
                assert!(frame::<128>(&p, 777) == frame::<64>(&p, 777), "{src}");
                assert!(
                    frame_over::<128>(&p, 777, Some(&under))
                        == frame_over::<64>(&p, 777, Some(&under)),
                    "{src}"
                );
            }
            Err(e) => {
                // Only the size limits may refuse a well-formed formula.
                assert!(
                    e.msg.contains("too long")
                        || e.msg.contains("too many")
                        || e.msg.contains("nested"),
                    "{src}\n{}",
                    e.render(&src)
                );
            }
        }
    }
    assert!(ok > 500, "only {ok} of 600 compiled");
}

#[test]
fn scalar_cap_is_a_friendly_error() {
    let body: Vec<String> = (1..=120).map(|k| format!("x*{k}.{k}")).collect();
    let src = format!("gray(frac({}))", body.join(" + "));
    let e = compile(&src).unwrap_err();
    assert!(e.msg.contains("too many numbers"), "{}", e.msg);
}

#[test]
fn op_cap_is_a_friendly_error() {
    // 2 steps a line, 150 lines.
    let mut src = String::from("v0 = x\n");
    for k in 1..=150 {
        src += &format!("v{k} = sin(v{}) + x\n", k - 1);
    }
    src += "gray(v150)";
    let e = compile(&src).unwrap_err();
    assert!(e.msg.contains("too long for the panel"), "{}", e.msg);
}

/// Division by a constant (2026-10-26): once a frame or a row it is a true
/// division, so `floor(minute/10)` is 5 at 50..59 and `floor(hour/10)` 2 at
/// 20..23 (it used to be a multiply by 1/10 rounded down: 20/10 came out
/// 1.99998); per pixel it is a multiply by 1/k rounded up, exact under
/// `floor` for whole numbers n with n·k < 65,536 (px, py, and i for k ≤ 16).
#[test]
fn division_by_a_constant_floors_whole_numbers_right() {
    use opendmd_psi::Inputs;
    let decode = |f: &[u8], at: usize| f[at * 3] as u32 + 256 * f[at * 3 + 1] as u32;
    for k in [10u32, 60, 3, 7, 12] {
        // Once a frame: n = hour·3600 + minute·60 + second, 0..=3600.
        let src = format!(
            "frame {{ n = hour*3600 + minute*60 + second; q = floor(n/{k}) }}\nrgb((q % 256)/255, floor(q/256)/255, n/{k} == q)"
        );
        let p = compile(&src).unwrap_or_else(|e| panic!("{}", e.render(&src)));
        let mut s = Scratch::<128>::new();
        let mut out = vec![0u8; W * H * 3];
        for n in 0..=3600u32 {
            let inp = Inputs {
                t: 0,
                hour: (n / 3600) as u8,
                minute: (n % 3600 / 60) as u8,
                second: (n % 60) as u8,
                a: 0,
                b: 0,
                c: 0,
            };
            render_with::<128>(&p, &mut s, &inp, &OWN, &mut out);
            assert_eq!(decode(&out, 0), n / k, "floor({n}/{k}) once a frame");
            assert_eq!(out[2] == 255, n % k == 0, "{n}/{k} exact once a frame");
        }
        // Per pixel: n = i, 0..4095 (k = 60: px, 0..127).
        let (n_src, top) = if k == 60 { ("px", 127) } else { ("i", 4095) };
        let src = format!("q = floor({n_src}/{k})\nrgb((q % 256)/255, floor(q/256)/255, 0)");
        let p = compile(&src).unwrap_or_else(|e| panic!("{}", e.render(&src)));
        render_with::<128>(&p, &mut s, &inputs(0), &OWN, &mut out);
        for at in 0..W * H {
            let n = if k == 60 { (at % W) as u32 } else { at as u32 };
            assert!(n <= top);
            assert_eq!(
                decode(&out, at),
                n / k,
                "floor({n}/{k}) per pixel (rgb {:?})",
                &out[at * 3..at * 3 + 3]
            );
        }
    }
}
