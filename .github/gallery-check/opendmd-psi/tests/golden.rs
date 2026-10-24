//! Golden frames, the never-panic fuzz, and host/wasm parity: these run
//! natively under `cargo test` and compiled to wasm32 under node
//! (`wasm-pack test --node`, `make test-wasm`) with the same expected
//! hashes, so a pass on both is bit-identical output.

use opendmd_psi::golden::{fnv, inputs, picture, FRAMES as GOLDEN, OVER};
use opendmd_psi::{
    compile, render_rows, render_with, sketches, Program, Scratch, H, MAX_OPS, OWN, W,
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
