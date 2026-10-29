//! Golden frames, the never-panic fuzz, and host/wasm parity: these run
//! natively under `cargo test` and compiled to wasm32 under node
//! (`wasm-pack test --node`, `make test-wasm`) with the same expected
//! hashes, so a pass on both is bit-identical output.

use opendmd_psi::golden::{fnv, inputs, picture, DRAWN, FRAMES as GOLDEN, OVER};
use opendmd_psi::{
    compile, render_rows, render_rows_ink, render_rows_still, render_with, sketches, Ink, Program,
    Scratch, Still, FONT_4X6, FONT_5X7, FONT_BOLD, H, MAX_OPS, OWN, W,
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

/// A frame with the draw stage's canvas (what every player gives a
/// formula that draws).
fn frame_ink<const N: usize>(p: &Program, ms: u32, under: Option<&[u8]>) -> Vec<u8> {
    let mut s = Scratch::<N>::new();
    let mut ink = Ink::boxed();
    let mut out = vec![0x5a; W * H * 3];
    render_rows_ink::<N>(
        p,
        &mut s,
        Some(&mut ink),
        &inputs(ms),
        &OWN,
        under,
        u32::MAX,
        &mut out,
    );
    out
}

/// Frames at each of `ms` with a still cache (kept on the first, drawn
/// from after: the second frame and on skip the cached steps), checked
/// against [`frame_ink`] without one; also a partial redraw (some rows) from
/// the kept values.
fn still_same<const N: usize>(src: &str, p: &Program, ms: &[u32], under: Option<&[u8]>) {
    let mut s = Scratch::<N>::new();
    let mut ink = Ink::boxed();
    let mut st = Still::new();
    assert!(st.take(p), "{src:?}: no still lanes");
    for (k, &m) in ms.iter().enumerate() {
        let mut out = vec![0x5a; W * H * 3];
        render_rows_still::<N>(
            p,
            &mut s,
            Some(&mut ink),
            Some(&mut st),
            &inputs(m),
            &OWN,
            under,
            u32::MAX,
            &mut out,
        );
        assert!(st.holds(p), "{src:?}: the cache kept nothing");
        let want = frame_ink::<N>(p, m, under);
        assert!(
            out == want,
            "{src:?} at {m} ms (frame {k}, {N} lanes): the still cache drew other bytes"
        );
        // Some rows again, from the kept values (the canvas as left).
        let mut part = out.clone();
        render_rows_still::<N>(
            p,
            &mut s,
            Some(&mut ink),
            Some(&mut st),
            &inputs(m),
            &OWN,
            under,
            0x8000_0101,
            &mut part,
        );
        assert!(part == want, "{src:?} at {m} ms: a partial redraw differs");
    }
}

/// Formulas with values for the still cache (a tunnel, rings, the plasma
/// sketch), and every golden formula that has any: the same bytes with the
/// cache as without, kept and drawn from, at both lane widths.
#[test]
fn the_still_cache_draws_the_same_bytes() {
    const STILL: &[&str] = &[
        "loop 4.8\nd = len(cx, cy*2)\ndepth = 1/d\nu = frac(angle*2 + t/4.8 + depth*a/32)\nv = frac(depth/4 + t/4.8)\ncell = (floor(u*8) + floor(v*4)) % 2\nramp = clamp((d - 0.25)*2.4, 0, 1)\nlayer { pal(chrono, frac(mix(0.28, 0.78, cell) + t/4.8)) }\nlayer multiply { gray(floor(ramp*ramp*(0.8 + 0.2*cos(angle + t/4.8))*8)/8) }",
        "loop 4\npal(rainbow, frac(r*1.5 - t/4))",
        "q = sqrt(px*py) + noise(x*8, y*4, 0)\nhsv(q/64 + t/8, 1, step(0.3, frac(q)))",
        "k = pick(px/32, len(x, y), atan2(y, x), 1/(x + 1), hash(px, py))\ngray(frac(k*3 + t))",
        "loop 2\nd = 1/(len(px - 63.5, (py - 15.5)*2) + 1)\nlayer { gray(frac(d*8 + t/2)) }\nlayer screen { rgb(ur, ug*d, 0, 0.5) }",
        sketches::PLASMA,
        sketches::RING_CLOCK,
    ];
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    let mut cases: Vec<(String, bool)> = STILL.iter().map(|s| (s.to_string(), true)).collect();
    for &(_, src, _, _) in GOLDEN {
        cases.push((src.to_string(), false));
    }
    for &(_, src, _, _, _) in OVER.iter().chain(DRAWN) {
        cases.push((src.to_string(), false));
    }
    let mut cached = 0;
    for (src, must) in &cases {
        let p = compile(src).unwrap_or_else(|e| panic!("{}", e.render(src)));
        if p.still_lanes() == 0 {
            assert!(
                !must,
                "{src:?}: expected values for the still cache\n{}",
                p.listing()
            );
            continue;
        }
        cached += 1;
        assert!(p.esp32_us_still() < p.esp32_us(), "{src:?}");
        let u = p.over().then_some(&under[..]);
        still_same::<128>(src, &p, &[0, 1234, 2500, 777], u);
        still_same::<64>(src, &p, &[0, 1234], u);
    }
    assert!(
        cached >= STILL.len(),
        "{cached} formulas with a still cache"
    );
}

/// Layouts the review found broken (a value before the still marker read a
/// cached one, so it read slot 0, `t`): the same frames as the plain layout,
/// without a cache and with one kept and drawn from.
#[test]
fn a_value_before_the_marker_never_reads_the_run() {
    for src in [
        "loop 4\ncx = px - 63.5\ncy = (15.5 - py)*2\nang = atan2(cy, cx)\nab = abs(ang)\nzz = sqrt(ab*2)\ngray(frac(ang + t/4) * frac(ab + t/4) * frac(zz + t/4))",
        "loop 4\ncx = px - 63.5\ncy = (15.5 - py)*2\nang = atan2(cy, cx)\nab = abs(ang) + 0.5\nzz = len(ab*2, cx)\ngray(frac(ang + t/4) * frac(ab + t/4) * frac(zz + t/4))",
    ] {
        let p = compile(src).unwrap();
        let plain = opendmd_psi::compile_plain(src).unwrap();
        for ms in [0, 700, 2100] {
            assert!(frame_ink::<128>(&p, ms, None) == frame_ink::<128>(&plain, ms, None), "{src:?} at {ms}");
        }
        if p.still_lanes() > 0 {
            still_same::<128>(src, &p, &[0, 700, 2100], None);
        }
    }
}

/// A program changed in place under a cache that kept another's values is
/// drawn as it is, never from the other's values.
#[test]
fn the_still_cache_never_draws_another_programs_values() {
    let a = compile("loop 2\ngray(frac(1/len(cx, cy*2)/4 + t/2))").unwrap();
    let b = compile("loop 2\ngray(frac(1/len(cx, cy*3)/4 + t/2))").unwrap();
    assert_eq!(a.still_lanes(), 1);
    assert_eq!(a.listing().lines().count(), b.listing().lines().count());
    let mut s = Scratch::<128>::new();
    let mut ink = Ink::boxed();
    let mut st = Still::new();
    assert!(st.take(&a));
    let mut out = vec![0u8; W * H * 3];
    render_rows_still::<128>(
        &a,
        &mut s,
        Some(&mut ink),
        Some(&mut st),
        &inputs(500),
        &OWN,
        None,
        u32::MAX,
        &mut out,
    );
    assert!(st.holds(&a) && !st.holds(&b));
    render_rows_still::<128>(
        &b,
        &mut s,
        Some(&mut ink),
        Some(&mut st),
        &inputs(500),
        &OWN,
        None,
        u32::MAX,
        &mut out,
    );
    assert_eq!(out, frame_ink::<128>(&b, 500, None));
    // Only a table's entries changed: not the kept values.
    let ta = compile("loop 4\ntable T = 0.1, 0.5, 0.9, 0.3\nk = T[floor(len(px - 63.5, (15.5 - py)*2)/8)]\ngray(frac(k + t/4))").unwrap();
    let tb = compile("loop 4\ntable T = 0.9, 0.2, 0.1, 0.6\nk = T[floor(len(px - 63.5, (15.5 - py)*2)/8)]\ngray(frac(k + t/4))").unwrap();
    assert!(ta.still_lanes() > 0);
    let mut st2 = Still::new();
    assert!(st2.take(&ta));
    render_rows_still::<128>(
        &ta,
        &mut s,
        Some(&mut ink),
        Some(&mut st2),
        &inputs(500),
        &OWN,
        None,
        u32::MAX,
        &mut out,
    );
    assert!(st2.holds(&ta) && !st2.holds(&tb));
    render_rows_still::<128>(
        &tb,
        &mut s,
        Some(&mut ink),
        Some(&mut st2),
        &inputs(500),
        &OWN,
        None,
        u32::MAX,
        &mut out,
    );
    assert_eq!(out, frame_ink::<128>(&tb, 500, None));
    // Only a sprite's pixels changed: not the kept values either.
    let spr = |rows: &str| {
        compile(&format!("loop 4\nsprite S {{\nkey . 0, a 0.3, b 0.7, c 1\n{rows}\n}}\nd = len(px - 63.5, (15.5 - py)*2)\nk = S[d/8, atan2(15.5 - py, px - 63.5)*4 + 2]\ngray(frac(k + t/4 + 1/(d + 1)))")).unwrap()
    };
    let (sa, sb) = (
        spr("abc.cab\n.cba.ab\nccaabb."),
        spr("cba.bac\n.abc.ca\naabbcc."),
    );
    assert!(sa.still_lanes() > 0, "{}", sa.listing());
    let mut st3 = Still::new();
    assert!(st3.take(&sa));
    for p in [&sa, &sb] {
        render_rows_still::<128>(
            p,
            &mut s,
            Some(&mut ink),
            Some(&mut st3),
            &inputs(500),
            &OWN,
            None,
            u32::MAX,
            &mut out,
        );
    }
    assert_eq!(out, frame_ink::<128>(&sb, 500, None));
    // A partial redraw of a program the cache doesn't hold runs every step.
    assert!(st.holds(&b));
    render_rows_still::<128>(
        &a,
        &mut s,
        Some(&mut ink),
        Some(&mut st),
        &inputs(500),
        &OWN,
        None,
        0xff,
        &mut out,
    );
    assert_eq!(
        &out[..W * 8 * 3],
        &frame_ink::<128>(&a, 500, None)[..W * 8 * 3]
    );
    // Let go: drawn as without.
    st.release();
    assert!(!st.is_held() && st.held_bytes() == 0);
    render_rows_still::<128>(
        &a,
        &mut s,
        Some(&mut ink),
        Some(&mut st),
        &inputs(9),
        &OWN,
        None,
        u32::MAX,
        &mut out,
    );
    assert_eq!(out, frame_ink::<128>(&a, 9, None));
}

/// Formulas that draw: the same hashes at both lane widths, here and in
/// wasm (and on the ESP32, through `psi-probe`).
#[test]
fn golden_frames_drawn() {
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    let mut bad = Vec::new();
    for &(name, src, ms, over, want) in DRAWN {
        let p = compile(src).unwrap_or_else(|e| panic!("{name}: {}", e.render(src)));
        assert!(p.draws(), "{name} draws nothing");
        let u = over.then_some(under.as_slice());
        let a = frame_ink::<128>(&p, ms, u);
        assert!(
            a == frame_ink::<64>(&p, ms, u),
            "{name}: lane widths differ"
        );
        let got = fnv(&a);
        if got != want {
            bad.push(format!("    (\"{name}\", …, {ms}, {over}, {got:#x}),"));
        }
    }
    assert!(
        bad.is_empty(),
        "drawn golden frames differ (got):\n{}",
        bad.join("\n")
    );
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
    "#f80",
    "#ff8800",
    "#",
    "#12",
    "pick(",
    "cx",
    "cy",
    "r",
    "angle",
    "ty",
    "r =",
    "draw {",
    "draw { for",
    "draw { for k <",
    "draw { for k < 3 {",
    "draw {\nfor k",
    "draw",
    "for k < 12 {",
    "for j < 3 {",
    "for",
    "k",
    "<",
    "line(",
    "dot(",
    "ink",
    "table T = 1, 2, -3",
    "table",
    "T[",
    "T",
    "[",
    "]",
    "1024",
    "loop forever",
    "forever",
    "seed",
    "t*0.15 +",
    "\nsprite S {\nkey . 0, X 1, # -2\n.X#.\nX..#\n}\n",
    "sprite S {",
    "key . 0",
    "S[",
    "S[px, ty]",
    "palette P = #f00, #0f0,\n#00f",
    "palette",
    "pal(P,",
];

fn check(src: &str) {
    match compile(src) {
        Ok(p) => {
            assert!(p.len() <= MAX_OPS, "{src:?}: {} steps", p.len());
            let _ = frame::<128>(&p, 1234);
            let mut under = vec![0u8; W * H * 3];
            picture(&mut under);
            let _ = frame_over::<128>(&p, 1234, Some(&under));
            let with = frame_ink::<64>(&p, 1234, Some(&under));
            // The column and still runs change no byte: against the formula
            // laid out without them, at the other lane width (where the
            // column run is worked out on every lane).
            let plain = opendmd_psi::compile_plain(src).expect("compiles plain too");
            assert!(
                frame_ink::<128>(&plain, 1234, Some(&under)) == with,
                "{src:?}: the runs changed the frame"
            );
            let mut rows = vec![0u8; W * H * 3];
            render_rows::<128>(
                &p,
                &mut Scratch::new(),
                &inputs(1234),
                &OWN,
                Some(&under),
                0x0f0f_0f0f,
                &mut rows,
            );
            if !p.draws() {
                // Some rows alone (the column run on the first drawn one).
                for r in (0..H).filter(|r| 0x0f0f_0f0fu32 & (1 << r) != 0) {
                    let span = r * W * 3..(r + 1) * W * 3;
                    assert!(
                        rows[span.clone()] == with[span],
                        "{src:?}: row {r} alone differs"
                    );
                }
            }
            if p.still_lanes() > 0 {
                still_same::<128>(src, &p, &[1234, 99], Some(&under));
            }
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

/// Random formulas built from named values the cache would keep (`len`,
/// `atan2`, `sqrt`, `1/…` of the position) shared with moving ones and
/// with each other: every layout draws the plain layout's frames, without a
/// cache and with one.
#[test]
fn random_still_layouts_are_the_plain_frames() {
    let mut r = Rng(0x57111_c4c4e);
    let still = [
        "len(px - 63.5, (15.5 - py)*2)",
        "atan2(15.5 - py, px - 63.5)",
        "sqrt(px*py + 1)",
        "1/(len(cx, cy) + 0.1)",
        "abs(cx*cy)",
        "x*y",
    ];
    let moving = ["t/4", "a", "sin(t)", "x + t"];
    let ops = ["+", "-", "*"];
    let funs = ["abs", "sqrt", "frac", "sin", "floor"];
    for case in 0..400 {
        let mut src = String::from("loop 4\n");
        let n = 3 + r.below(5);
        for k in 0..n {
            let pick = |r: &mut Rng, k: usize| -> String {
                match r.below(4) {
                    0 if k > 0 => format!("v{}", r.below(k)),
                    1 => moving[r.below(moving.len())].to_string(),
                    2 => format!("{}.{}", r.below(4), r.below(10)),
                    _ => still[r.below(still.len())].to_string(),
                }
            };
            let (x, y) = (pick(&mut r, k), pick(&mut r, k));
            let e = if r.below(2) == 0 {
                format!(
                    "{}({} {} {})",
                    funs[r.below(funs.len())],
                    x,
                    ops[r.below(3)],
                    y
                )
            } else {
                format!("({}) {} ({})", x, ops[r.below(3)], y)
            };
            src += &format!("v{k} = {e}\n");
        }
        let used: Vec<String> = (0..n)
            .filter(|_| r.below(3) > 0)
            .map(|k| format!("frac(v{k} + t/4)"))
            .collect();
        src += &format!(
            "gray({} * 0.5)",
            if used.is_empty() {
                "v0".to_string()
            } else {
                used.join(" + ")
            }
        );
        let Ok(p) = compile(&src) else { continue };
        let plain = opendmd_psi::compile_plain(&src).expect("plain compiles");
        for ms in [0, 1300] {
            assert!(
                frame_ink::<128>(&p, ms, None) == frame_ink::<128>(&plain, ms, None),
                "case {case}: {src:?} at {ms} ms\n{}",
                p.listing()
            );
        }
        if p.still_lanes() > 0 {
            still_same::<128>(&src, &p, &[0, 1300], None);
        }
    }
}

#[test]
fn garbage_never_panics() {
    let mut r = Rng(0x5eed_1234_abcd_ef01);
    for _ in 0..3000 {
        // Token soup.
        let n = 1 + r.below(40);
        let pieces: Vec<&str> = (0..n).map(|_| PIECES[r.below(PIECES.len())]).collect();
        check(&pieces.join(" "));
        // And cut off after every piece: a look ahead past the end of the
        // input (`draw { for` at the end) must be an error, never a panic.
        for k in 1..pieces.len() {
            check(&pieces[..k].join(" "));
        }
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

/// The same for pick, the values about the middle and colours: random
/// well-formed uses render the same at both lane widths, alone and over
/// a picture, or are refused only for their size.
#[test]
fn random_picks_and_colours_render_or_refuse() {
    let mut r = Rng(0x0dd_c010_75ee_d5ed);
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    let leaf = |r: &mut Rng| {
        [
            "cx",
            "cy",
            "r",
            "angle",
            "ty",
            "px/16",
            "y*3",
            "second % 5",
            "v",
        ][r.below(9)]
        .to_string()
    };
    let hex = |r: &mut Rng| format!("#{:06x}", r.next() & 0xff_ffff);
    let mut ok = 0;
    for k in 0..400 {
        let n = 1 + r.below(5);
        let opts: Vec<String> = (0..n)
            .map(|_| {
                if r.below(3) == 0 {
                    leaf(&mut r)
                } else {
                    expr(&mut r, 2)
                }
            })
            .collect();
        let pick = format!("pick({}, {})", leaf(&mut r), opts.join(", "));
        let colour = match k % 4 {
            0 => hex(&mut r),
            1 => format!("mix({}, {}, {pick})", hex(&mut r), hex(&mut r)),
            2 => format!(
                "rgb(mix(rgb({pick}, {}, ur), {}, {}), {})",
                leaf(&mut r),
                hex(&mut r),
                leaf(&mut r),
                expr(&mut r, 2)
            ),
            _ => format!(
                "layer multiply {{ mix(#fff, mix({}, rgb(v, {pick}, 0.5), cy*2 + 1), r) }}",
                hex(&mut r)
            ),
        };
        let src = format!("loop 4\nv = {}\nw2 = {pick}\n{colour}", expr(&mut r, 3));
        match compile(&src) {
            Ok(p) => {
                ok += 1;
                assert!(frame::<128>(&p, 777) == frame::<64>(&p, 777), "{src}");
                assert!(
                    frame_over::<128>(&p, 777, Some(&under))
                        == frame_over::<64>(&p, 777, Some(&under)),
                    "{src}"
                );
            }
            Err(e) => assert!(
                e.msg.contains("too long")
                    || e.msg.contains("too many")
                    || e.msg.contains("nested"),
                "{src}\n{}",
                e.render(&src)
            ),
        }
    }
    assert!(ok > 350, "only {ok} of 400 compiled");
}

/// Random draw blocks: loops (one or two deep), tables, values from the
/// loop counters and the frame, lines and dots anywhere (far off the panel
/// too): they render the same at both lane widths, alone and over a
/// picture, within the caps, or are refused only for their size.
#[test]
fn random_draw_blocks_render_or_refuse() {
    let mut r = Rng(0x0d2a_35ee_d0f1_a7e5);
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    // A value inside a loop: the counters, the tables, the time.
    fn val(r: &mut Rng, ks: &[&str], d: u32) -> String {
        let leaf = |r: &mut Rng| -> String {
            match r.below(6) {
                0 | 1 if !ks.is_empty() => ks[r.below(ks.len())].to_string(),
                2 => format!("T[{}]", ks.first().copied().unwrap_or("t")),
                3 => ["t", "a", "second", "f"][r.below(4)].to_string(),
                _ => format!("{}", r.below(2000) as i32 - 900),
            }
        };
        if d == 0 || r.below(3) == 0 {
            return leaf(r);
        }
        match r.below(6) {
            0 => format!("({} + {})", val(r, ks, d - 1), val(r, ks, d - 1)),
            1 => format!("({} * {})", val(r, ks, d - 1), val(r, ks, d - 1)),
            2 => format!("({} / {})", val(r, ks, d - 1), val(r, ks, d - 1)),
            3 => format!("sin({})", val(r, ks, d - 1)),
            4 => format!("hash({}, 3)*{}", val(r, ks, d - 1), r.below(200)),
            _ => format!("({} % 37)", val(r, ks, d - 1)),
        }
    }
    let prim = |r: &mut Rng, ks: &[&str]| -> String {
        if r.below(3) == 0 {
            format!(
                "dot({}, {}, {})",
                val(r, ks, 2),
                val(r, ks, 2),
                val(r, ks, 1)
            )
        } else {
            format!(
                "line({}, {}, {}, {}, {}, {})",
                val(r, ks, 2),
                val(r, ks, 2),
                val(r, ks, 2),
                val(r, ks, 2),
                val(r, ks, 1),
                val(r, ks, 1)
            )
        }
    };
    let mut ok = 0;
    for k in 0..300 {
        let n1 = [1, 3, 12, 100, 1024][r.below(5)];
        let mut body = format!(
            "v = {}\n{}\n",
            val(&mut r, &["k"], 3),
            prim(&mut r, &["k", "v"])
        );
        if r.below(2) == 0 {
            let n2 = 1 + r.below(8);
            body += &format!("for j < {n2} {{\n{}\n}}\n", prim(&mut r, &["k", "j", "v"]));
        }
        let colour = [
            "gray(ink)",
            "pal(fire, ink)",
            "#40c0ff * ink",
            "layer screen { rgb(ink, ur, 0.5) }",
            "rgb(ink, T[px % 4], x)",
        ][k % 5];
        let src = format!(
            "loop {}\ntable T = 0.5, -2, 30, 100\nframe {{ f = t*{} }}\ndraw {{\n{}\nfor k < {n1} {{\n{body}}}\n}}\n{colour}",
            1 + k % 7,
            r.below(50),
            prim(&mut r, &[]),
        );
        match compile(&src) {
            Ok(p) => {
                ok += 1;
                assert!(p.draws(), "{src}");
                assert!(p.len() <= MAX_OPS);
                for ms in [0, 777] {
                    assert!(
                        frame_ink::<128>(&p, ms, None) == frame_ink::<64>(&p, ms, None),
                        "{src}"
                    );
                    assert!(
                        frame_ink::<128>(&p, ms, Some(&under))
                            == frame_ink::<64>(&p, ms, Some(&under)),
                        "{src}"
                    );
                }
            }
            Err(e) => assert!(
                e.msg.contains("too long")
                    || e.msg.contains("too many")
                    || e.msg.contains("too much"),
                "{src}\n{}",
                e.render(&src)
            ),
        }
    }
    assert!(ok > 250, "only {ok} of 300 compiled");
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
                ..Inputs::default()
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

/// A frame of `p` at `inputs` with the draw stage's canvas and a picture
/// under it.
fn frame_at(p: &Program, inputs: &opendmd_psi::Inputs, under: &[u8]) -> Vec<u8> {
    let mut s = Scratch::<128>::new();
    let mut ink = Ink::boxed();
    let mut out = vec![0x5a; W * H * 3];
    render_rows_ink::<128>(
        p,
        &mut s,
        Some(&mut ink),
        inputs,
        &OWN,
        Some(under),
        u32::MAX,
        &mut out,
    );
    out
}

/// The narrowed steps (`add_w`, `mul_n`, `shr`, …: the ranges say they
/// never saturate) draw what the checked ones draw: random formulas with
/// values near the ends of the range, divisions by values whose range
/// holds 0, values that are constant only for inputs in range, at times
/// across the loop and past it, with the sliders and the clock in range
/// and out (where the VM runs the checked steps). `PSI_FUZZ_N` sets the
/// number of formulas (1,500 by default).
#[test]
fn narrowed_steps_are_the_checked_frames() {
    let mut r = Rng(0x0a77_0e5d_57e9_5000);
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    const BIG: &[&str] = &[
        "181",
        "30000",
        "t*100",
        "px*py*8",
        "t*t",
        "a*3000",
        "1/8",
        "0.0625",
        "px*256",
        "-px*256",
        "4",
        "x*x",
        "len(px, py)",
        "atan2(py, px)",
        "floor(t*7)",
        "frac(t*3)",
        // Divisions by a value whose range holds 0; values constant only
        // while the inputs are in range (a power of two then: a shift).
        "1/(floor(a) - 1)",
        "1/(a - 0.5)",
        "1/(t - 2)",
        "1/(floor(b) - 1)",
        "min(a + 1, 0.5)",
        "max(0.25, a - 3)",
        "floor(b) - 1",
        "min(5 - t, 0.5)",
        "min(c + 1, 0.25)",
        // Small products (one 32-bit multiply).
        "x * 0.3",
        "y * 0.45",
        "sin(x) * 0.6",
    ];
    // Slider values: in range (0, 1, ½, ⅓) and out (-1, 3).
    const SLIDE: [i32; 6] = [0, 1 << 16, 1 << 15, 21845, -(1 << 16), 3 << 16];
    let n: usize = std::env::var("PSI_FUZZ_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1500);
    let (mut narrowed, mut shr, mut mul_n, mut frames) = (0, 0, 0, 0);
    for k in 0..n {
        let leaf = |r: &mut Rng| -> String {
            if r.below(2) == 0 {
                BIG[r.below(BIG.len())].to_string()
            } else {
                expr(r, 1)
            }
        };
        let e = |r: &mut Rng| -> String {
            let (a, b, c) = (leaf(r), leaf(r), leaf(r));
            match r.below(7) {
                0 => format!("{a} * {b} + {c}"),
                1 => format!("({a} - {b}) * {c}"),
                2 => format!("{a} * {b} * {c}"),
                3 => format!("({a} + {b}) / 8 - {c}"),
                4 => format!("min({a}, 10) * min({b}, 10) * {c}"),
                5 => format!("x * {a} + {b}"),
                _ => format!("{a} + {b} - {c} * 0.25"),
            }
        };
        let lp = 1 + k % 6;
        let src = format!(
            "loop {lp}\nv = {}\nu = {}\n{}",
            e(&mut r),
            e(&mut r),
            if k % 3 == 0 {
                "pal(fire, frac(v)) * (floor(u % 9)/8)".to_string()
            } else {
                "hsv(v, 1, frac(u))".to_string()
            }
        );
        let (mut p, mut q) = (Program::default(), Program::default());
        if opendmd_psi::compile_with(&src, &mut p, true, true).is_err() {
            continue;
        }
        opendmd_psi::compile_with(&src, &mut q, true, false).expect("compiles unnarrowed too");
        if p != q {
            narrowed += 1;
        }
        let listing = p.listing();
        shr += listing.matches("shr(").count();
        mul_n += listing.matches("mul_n(").count();
        let end = lp as u32 * 1000;
        let times = [0, end / 3, end / 2, end - 1, end, end + 5000];
        for j in 0..8 {
            let ms = times[r.below(times.len())];
            let (a, b, c) = (
                SLIDE[r.below(SLIDE.len())],
                SLIDE[r.below(SLIDE.len())],
                SLIDE[r.below(SLIDE.len())],
            );
            let (hour, second) = if j % 2 == 0 { (23, 59) } else { (10, 30) };
            let inputs = opendmd_psi::Inputs {
                a,
                b,
                c,
                hour,
                second,
                ..inputs(ms)
            };
            frames += 1;
            assert!(
                frame_at(&p, &inputs, &under) == frame_at(&q, &inputs, &under),
                "case {k}: {src:?} at {ms} ms, a {a} b {b} c {c}\n{listing}"
            );
        }
    }
    // The generator must keep reaching the rewrites.
    assert!(narrowed * 3 > n * 2, "only {narrowed} of {n} narrowed");
    assert!(shr * 10 > n, "only {shr} shr in {n} formulas");
    assert!(mul_n * 10 > n, "only {mul_n} mul_n in {n} formulas");
    eprintln!(
        "{n} formulas, {narrowed} narrowed, {shr} shr, {mul_n} mul_n, {frames} frames compared"
    );
}

/// The review's two cases (psi-speed): a division whose divisor's range
/// holds 0 (`1/(floor(a) - 1)` at a = 1: a division by 0 is the
/// numerator's end, the top one here), and a shift by a value that is a
/// power of two only for inputs in range (`min(a + 1, 0.5)` at a = -1, and
/// `min(5 - t, 0.5)` past the loop).
#[test]
fn the_reviews_narrowing_cases() {
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    for (src, ms, a) in [
        (
            "loop 4\nk = 1/(floor(a) - 1)\nrgb(x + k + 1, 0, 0)",
            1000,
            1 << 16,
        ),
        (
            "loop 4\nk = min(a + 1, 0.5)\nrgb(x * k, 0, 0)",
            1000,
            -(1 << 16),
        ),
        (
            "loop 4\nk = min(5 - t, 0.5)\nrgb(x * k, 0, 0)",
            9000,
            1 << 15,
        ),
    ] {
        let (mut p, mut q) = (Program::default(), Program::default());
        opendmd_psi::compile_with(src, &mut p, true, true).unwrap();
        opendmd_psi::compile_with(src, &mut q, true, false).unwrap();
        let inputs = opendmd_psi::Inputs { a, ..inputs(ms) };
        assert!(
            frame_at(&p, &inputs, &under) == frame_at(&q, &inputs, &under),
            "{src:?} at {ms} ms, a {a}\n{}",
            p.listing()
        );
    }
}

/// A frame of `p` at `ms` (both clocks: `t` and a `loop forever`
/// formula's), seed `seed`, over `under`, at lane width `N`.
fn frame_wide<const N: usize>(p: &Program, ms: u32, seed: i32, under: &[u8]) -> Vec<u8> {
    let mut s = Scratch::<N>::new();
    let mut ink = Ink::boxed();
    let mut out = vec![0x5a; W * H * 3];
    let mut i = opendmd_psi::Inputs { seed, ..inputs(ms) };
    i.set_ms(p, ms as u64);
    render_rows_ink::<N>(
        p,
        &mut s,
        Some(&mut ink),
        &i,
        &OWN,
        Some(under),
        u32::MAX,
        &mut out,
    );
    out
}

/// `loop forever` is the same formula with a very long loop, wherever its
/// values fit a Q16.16: random formulas that use `t` in every way a value
/// that grows without end may be used (sums and products with values the
/// same all frame, a part per pixel or per row, fractions, waves, `%`,
/// noise and hashes of it, compares, min, max, clamp, smoothstep, mix,
/// select, a part with its own loop, the draw stage), drawn as `loop
/// forever` and as `loop 3600` up to 300 s in: the same bytes, at both lane
/// widths, narrowed or not, with and without the column and still runs.
/// `PSI_FUZZ_N` sets the number of formulas (600 by default).
#[test]
fn loop_forever_is_a_long_loop_where_it_fits() {
    let mut r = Rng(0x00f0_7e7e_5eed_1e55);
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    // Wide values: t scaled by values the same all frame (kept small, so
    // the looped formula never saturates within 300 s).
    const WIDE: &[&str] = &[
        "t",
        "t*0.15",
        "t/8",
        "t*2.5",
        "t*10 + 3",
        "t*0.5 - a*7",
        "(t + 1.25)/3",
        "t*0.01 + t*0.2",
        "-t*0.3",
        "t*a",
    ];
    const SMALL: &[&str] = &[
        "x*3",
        "px/16",
        "y",
        "py/9",
        "sin(x*2)",
        "noise(x*4, y, 0)",
        "a",
        "0.375",
        "i/4096",
        "x*x",
    ];
    let n: usize = std::env::var("PSI_FUZZ_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let mut compiled = 0;
    for k in 0..n {
        let w = |r: &mut Rng| format!("({})", WIDE[r.below(WIDE.len())]);
        let s = |r: &mut Rng| SMALL[r.below(SMALL.len())];
        let use_t = |r: &mut Rng| -> String {
            let (a, b, c) = (w(r), s(r), s(r));
            match r.below(19) {
                0 => format!("frac({a})"),
                1 => format!("sin({a} + {b})"),
                2 => format!("cos({a})*{b}"),
                3 => format!("tri({a} - {b}) + saw({a})*0.25 + sqr({a})*0.125"),
                4 => format!("({a}) % 7 / 7"),
                5 => format!("(({a}) + {b}) % 1.5"),
                6 => format!("noise({a} + {b}, {c}*4, 0)"),
                7 => format!("noise(x*8, {a}, {w2})", w2 = w(r)),
                8 => format!("hash(floor({a}), {b})"),
                9 => format!("({a} < 40) + ({a} >= 150)*0.5 + step(90, {a})*0.25"),
                10 => format!("min({a}, 2)*0.25 + max({a}/300, {b})"),
                11 => format!("clamp({a}/60, 0, 1)"),
                12 => format!("smoothstep(0, 20, {a})"),
                13 => format!("mix(0.2, 0.8, ({a})/300)"),
                14 => format!("({a}) > 100 ? {b} : {c}"),
                15 => format!("frac({a} + {b} - {c})*{b}"),
                16 => format!("noise({a}*0.5 + {b}, {c}, {a})"),
                // A split times a value the same all frame (exact: the wide
                // part's remainder carried in).
                17 => format!(
                    "frac(({a} + {b})*{k}) + frac(({a} - {c})*a*{k})",
                    k = ["0.3", "0.37", "1.5", "-0.7", "3"][r.below(5)]
                ),
                _ => format!("not ({a} < 9) and {b} > 0.2"),
            }
        };
        let body = match k % 5 {
            // A colour of values made from t.
            0 | 1 => format!(
                "v = {}\nu = {}\nhsv(v + {}, 1, frac(u) * 0.5 + 0.5)",
                use_t(&mut r),
                use_t(&mut r),
                w(&mut r)
            ),
            // Once a frame, and per row.
            2 => format!(
                "frame {{ f = {} }}\nq = noise({} + y*3, py, 0)\nrgb(f, q, {})",
                use_t(&mut r),
                w(&mut r),
                use_t(&mut r)
            ),
            // A part with its own loop, and the draw stage.
            3 => format!(
                "part p {{\n  loop 4\n  gray(frac(t/4 + x))\n}}\ndraw {{ for k < 16 {{ dot(frac({} + k/16)*128, noise({} + k, 0, 0)*31) }} }}\nlayer {{ p }}\nlayer add {{ gray(ink, {}) }}",
                w(&mut r),
                w(&mut r),
                use_t(&mut r)
            ),
            // A wide value straight into a colour (held to 0..1).
            _ => format!("gray({} / 60)", w(&mut r)),
        };
        let (fsrc, lsrc) = (
            format!("loop forever\n{body}"),
            format!("loop 3600\n{body}"),
        );
        let lp = match compile(&lsrc) {
            Ok(p) => p,
            Err(e) => {
                // (The generator puts pixel values in `frame { }` now and
                // then.) Refused looped, refused never repeating.
                assert!(
                    compile(&fsrc).is_err(),
                    "{fsrc:?} compiles, but looped: {}",
                    e.msg
                );
                continue;
            }
        };
        let fp = compile(&fsrc).unwrap_or_else(|e| panic!("{fsrc:?}\n{}", e.render(&fsrc)));
        assert!(fp.forever() && !lp.forever());
        compiled += 1;
        let mut q = Program::default();
        opendmd_psi::compile_with(&fsrc, &mut q, false, false).expect("plain, unnarrowed");
        for ms in [0, 1234, 59_999, 123_460, 299_980] {
            let want = frame_wide::<128>(&lp, ms, 0, &under);
            let got = frame_wide::<128>(&fp, ms, 0, &under);
            assert!(
                got == want,
                "case {k} at {ms} ms:\n{fsrc}\n{}\nlooped:\n{}",
                fp.listing(),
                lp.listing()
            );
            assert!(
                frame_wide::<64>(&fp, ms, 0, &under) == want,
                "case {k} at {ms} ms, 64 lanes:\n{fsrc}"
            );
            assert!(
                frame_wide::<128>(&q, ms, 0, &under) == want,
                "case {k} at {ms} ms, plain:\n{fsrc}"
            );
        }
        // Far past the Q16.16 range, and with a seed and sliders out of
        // theirs: both widths agree, and the narrowed steps draw what the
        // checked ones draw.
        for (ms, seed, a) in [
            (40_000_000, 7, 1 << 15),
            (4_000_000_000, -(3 << 16), 3 << 16),
            (77_777, 5 << 16, -(1 << 16)),
        ] {
            let at = |p: &Program| {
                let mut s = Scratch::<128>::new();
                let mut ink = Ink::boxed();
                let mut out = vec![0x5a; W * H * 3];
                let mut i = opendmd_psi::Inputs {
                    seed,
                    a,
                    ..inputs(ms)
                };
                i.set_ms(p, ms as u64);
                render_rows_ink::<128>(
                    p,
                    &mut s,
                    Some(&mut ink),
                    &i,
                    &OWN,
                    Some(&under),
                    u32::MAX,
                    &mut out,
                );
                out
            };
            assert!(
                at(&fp) == at(&q),
                "case {k} at {ms} ms, seed {seed}, a {a}: narrowed\n{fsrc}"
            );
            assert!(
                frame_wide::<128>(&fp, ms, seed, &under) == frame_wide::<64>(&fp, ms, seed, &under)
            );
        }
    }
    assert!(compiled * 10 > n * 7, "only {compiled} of {n} compiled");
    eprintln!("{n} formulas, {compiled} compiled both ways and drew the same");
}

/// A formula that never repeats doesn't: frames an hour, a day and a
/// month in differ from the start's, and a new `seed` is another drive.
#[test]
fn loop_forever_goes_on() {
    let src = "loop forever\nfps 100\nframe { u = t*0.15 + seed*1000 }\nhz = noise(u + y, 0, 0)\nhsv(hz + x*0.1, 1, noise(x*8 + t, y*4, u))";
    let p = compile(src).unwrap();
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    let at = |ms: u32, seed: i32| fnv(&frame_wide::<128>(&p, ms, seed, &under));
    let start = at(1000, 0);
    for ms in [3_601_000, 86_401_000, 2_592_001_000] {
        assert_ne!(at(ms, 0), start, "{ms} ms");
    }
    assert_ne!(at(1000, 1 << 15), start, "another seed, another drive");
    assert_eq!(
        at(86_401_000, 3),
        at(86_401_000, 3),
        "a pure function of t and seed"
    );
}

/// Sprites' values in the narrowed steps: random sprites with key numbers
/// across the range (near its ends too), read at every level and used in
/// sums and products, draw what the checked steps draw, with the sliders
/// in range and out.
#[test]
fn sprites_narrow_soundly() {
    let mut r = Rng(0x5b17_e5ee_d000_0001);
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    const NUMS: &[&str] = &[
        "0", "1", "-1", "0.5", "181", "-181", "30000", "-30000", "32767", "0.001",
    ];
    let mut n = 0;
    for k in 0..300 {
        let keys = 1 + r.below(6);
        let chars = b".XYZW#";
        let key: Vec<String> = (0..keys)
            .map(|j| format!("{} {}", chars[j] as char, NUMS[r.below(NUMS.len())]))
            .collect();
        let (w, h) = (1 + r.below(20), 1 + r.below(8));
        let rows: String = (0..h)
            .map(|_| {
                (0..w)
                    .map(|_| chars[r.below(keys)] as char)
                    .collect::<String>()
                    + "\n"
            })
            .collect();
        let u = ["px/4", "x*20", "a*8", "3", "px - 60"][r.below(5)];
        let v = ["ty/4", "py", "b*5", "1", "t"][r.below(5)];
        let e = [
            "S[U, V]*x + a",
            "S[U, V]*S[V, U] - 2",
            "(S[U, V] + 181)*(x + 1)",
            "frac(S[U, V]*0.01 + t)",
            "S[U, V]*S[U, V]*0.5",
        ][r.below(5)]
        .replace('U', u)
        .replace('V', v);
        let src = format!(
            "loop 3\nsprite S {{\nkey {}\n{rows}}}\nv = {e}\nhsv(v, 1, frac(v*0.37))",
            key.join(", ")
        );
        let (mut p, mut q) = (Program::default(), Program::default());
        if opendmd_psi::compile_with(&src, &mut p, true, true).is_err() {
            continue;
        }
        opendmd_psi::compile_with(&src, &mut q, true, false).expect("compiles unnarrowed too");
        n += 1;
        for (ms, a) in [(0, 0), (1500, 1 << 16), (2999, -(1 << 16)), (9000, 3 << 16)] {
            let i = opendmd_psi::Inputs {
                a,
                b: 1 << 15,
                ..inputs(ms)
            };
            assert!(
                frame_at(&p, &i, &under) == frame_at(&q, &i, &under),
                "case {k}: {src:?} at {ms}, a {a}\n{}",
                p.listing()
            );
        }
        assert!(frame::<128>(&p, 777) == frame::<64>(&p, 777), "{src}");
    }
    assert!(n > 250, "only {n} compiled");
}

/// A program compiled with scalar slots used again (the default) draws what
/// the same formula with a slot for every value draws: the golden formulas,
/// and random ones with values once a frame (some only read there), draw
/// loops two deep reading values from before them, rows, pixels, picks,
/// layers and `loop forever`, at times in and out of the ranges, both lane
/// widths. Reuse never needs more slots, and does save some.
#[test]
fn slots_used_again_draw_the_same() {
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    let mut r = Rng(0x0005_1075_bacc_0001);
    let mut srcs: Vec<String> = GOLDEN.iter().map(|g| g.1.to_string()).collect();
    srcs.extend(DRAWN.iter().map(|g| g.1.to_string()));
    srcs.extend(OVER.iter().map(|g| g.1.to_string()));
    let n: usize = std::env::var("PSI_FUZZ_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400);
    for k in 0..n {
        let e = |r: &mut Rng| expr(r, 2);
        let mut s = format!("loop {}\n", 1 + k % 5);
        if k % 7 == 0 {
            s = "loop forever\n".into();
        }
        s += &format!(
            "frame {{ f1 = {}; f2 = f1*{} + a; f3 = sin(f2 + t) }}\n",
            ["t*0.5", "a + b", "hour/24", "t % 3"][r.below(4)],
            r.below(9)
        );
        if r.below(2) == 0 {
            s += &format!(
                "draw {{\n  g = f1*3 + 2\n  for q < {} {{\n    u = q*f3*7 + g\n    for j < {} {{ dot(u + j*4, j*3 + f2, hash(q, j)) }}\n    line(u, 0, g, 31, f3)\n  }}\n}}\n",
                1 + r.below(12),
                1 + r.below(5)
            );
        }
        s += &format!("rw = py*{} + f3\nv = {}\n", r.below(5), e(&mut r));
        s += match r.below(4) {
            0 => "layer { hsv(v + rw*0.1, 1, frac(v)) }\nlayer add { gray(f1*0.1 + x, 0.5) }"
                .to_string(),
            1 => format!("rgb(pick(px/40, f1, f2, rw), frac(v), {})", e(&mut r)),
            2 => "pal(fire, frac(v + rw)) * (f3*0.5 + 0.5)".to_string(),
            _ => format!("gray(frac(v*{} + rw + f2))", r.below(4)),
        }
        .as_str();
        srcs.push(s);
    }
    let (mut saved, mut compared) = (0, 0);
    for src in &srcs {
        let (mut p, mut q) = (Program::default(), Program::default());
        if opendmd_psi::compile_opts(src, &mut p, true, true, true).is_err() {
            continue;
        }
        if opendmd_psi::compile_opts(src, &mut q, true, true, false).is_err() {
            // Only for its size: with every value its own slot it may not fit.
            continue;
        }
        compared += 1;
        assert!(p.values() <= q.values(), "{src}");
        saved += usize::from(p.values() < q.values());
        for (ms, a, seed) in [
            (0u32, 1 << 15, 0),
            (1234, 0, 9000),
            (2999, 3 << 16, 60_000),
            (40_000_000, -(1 << 16), -5),
        ] {
            let mut i = opendmd_psi::Inputs {
                a,
                seed,
                ..inputs(ms)
            };
            i.set_ms(&p, ms as u64);
            {
                let (x, y) = (&p, &q);
                let fx = {
                    let mut s = Scratch::<128>::new();
                    let mut ink = Ink::boxed();
                    let mut out = vec![0x5a; W * H * 3];
                    render_rows_ink::<128>(
                        x,
                        &mut s,
                        Some(&mut ink),
                        &i,
                        &OWN,
                        Some(&under),
                        u32::MAX,
                        &mut out,
                    );
                    out
                };
                let fy = {
                    let mut s = Scratch::<64>::new();
                    let mut ink = Ink::boxed();
                    let mut out = vec![0x5a; W * H * 3];
                    render_rows_ink::<64>(
                        y,
                        &mut s,
                        Some(&mut ink),
                        &i,
                        &OWN,
                        Some(&under),
                        u32::MAX,
                        &mut out,
                    );
                    out
                };
                assert!(
                    fx == fy,
                    "{src:?} at {ms} ms\n{}\nwithout reuse:\n{}",
                    x.listing(),
                    y.listing()
                );
            }
        }
    }
    assert!(
        compared * 10 > srcs.len() * 8,
        "only {compared} of {} compared",
        srcs.len()
    );
    assert!(
        saved * 2 > compared,
        "only {saved} of {compared} saved slots"
    );
    eprintln!("{compared} formulas compared, {saved} with fewer slots");
}

/// A frame and its canvas, the draw loops run as lanes or (`turns`) a turn
/// at a time.
fn frame_and_ink<const N: usize>(
    p: &Program,
    i: &opendmd_psi::Inputs,
    under: &[u8],
    turns: bool,
) -> (Vec<u8>, Vec<u8>) {
    let mut s = Scratch::<N>::new();
    let mut ink = Ink::boxed();
    let mut out = vec![0x5a; W * H * 3];
    if turns {
        opendmd_psi::render_rows_ink_turns::<N>(
            p,
            &mut s,
            Some(&mut ink),
            i,
            &OWN,
            Some(under),
            u32::MAX,
            &mut out,
        );
    } else {
        render_rows_ink::<N>(
            p,
            &mut s,
            Some(&mut ink),
            i,
            &OWN,
            Some(under),
            u32::MAX,
            &mut out,
        );
    }
    (out, ink.0.to_vec())
}

/// Draw loops run as lanes (`Program::vector_loop`: each body step over
/// every turn at once, then the lines and dots turn by turn) draw what a
/// turn at a time draws: random loop bodies (chains of values of the
/// counter, the frame's values, tables, picks, noise, divisions that can
/// meet 0, hashes, glyphs), counts either side of the lane width and of
/// `VEC_MIN`, loops in sequence up to the frame's 2,048 turns, loops in
/// loops, values read after their loop, at both lane widths, with the
/// sliders and `t` in range and out. `PSI_FUZZ_N` sets the number of
/// formulas (800 by default).
#[test]
fn draw_loops_as_lanes_are_a_turn_at_a_time() {
    let mut r = Rng(0x1a7e_5d4a_3b00_0c11);
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    let n: usize = std::env::var("PSI_FUZZ_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(800);
    fn val(r: &mut Rng, ks: &[&str], d: u32) -> String {
        if d == 0 || r.below(4) == 0 {
            let leaves = ["0.5", "7", "-3.25", "f", "g", "a", "t", "100", "1/3"];
            return if !ks.is_empty() && r.below(3) != 0 {
                ks[r.below(ks.len())].to_string()
            } else {
                leaves[r.below(leaves.len())].to_string()
            };
        }
        let e = |r: &mut Rng| val(r, ks, d - 1);
        match r.below(19) {
            0..=3 => {
                let op = [
                    "+", "-", "*", "/", "%", "<", ">=", "and", "<=", "==", "!=", ">", "or",
                ][r.below(13)];
                format!("({} {op} {})", e(r), e(r))
            }
            4 => format!("{}({})", ["sin", "cos"][r.below(2)], e(r)),
            // (Laned since psi-speed3: `%`, the comparisons, `not`, `-`,
            // `cos`, `T[k]`; and steps still worked out turn by turn.)
            16 => match r.below(4) {
                0 => format!("not {}", e(r)),
                1 => format!("-{}", e(r)),
                2 => format!("T[{}]", e(r)),
                _ => format!("({} % {})", e(r), ["0", "3", "-2.5", "f"][r.below(4)]),
            },
            17 => match r.below(5) {
                0 => format!("sqrt({})", e(r)),
                1 => format!("step({}, {})", e(r), e(r)),
                2 => format!("mix({}, {}, {})", e(r), e(r), e(r)),
                3 => format!("smoothstep({}, {}, {})", e(r), e(r), e(r)),
                _ => format!("tri({})", e(r)),
            },
            18 => format!(
                "noise({}, {}, {}) + digit({} % 10, 1, {} % 7)",
                e(r),
                e(r),
                e(r),
                e(r),
                e(r)
            ),
            5 => format!("hash({}, {})", e(r), r.below(20)),
            6 => format!("frac({})", e(r)),
            7 => format!("({} ? {} : {})", e(r), e(r), e(r)),
            8 => format!("T[{} % 6]", e(r)),
            9 => format!("pick({} % 4, f, 0.25, g)", e(r)),
            10 => format!("noise({}, {}, {})", e(r), e(r), ["0", "2", "f"][r.below(3)]),
            11 => format!("1/({} - {})", e(r), e(r)),
            12 => format!("clamp({}, -4, {})", e(r), e(r)),
            13 => format!("digit({} % 10, {} % 5, {} % 7)", e(r), e(r), e(r)),
            14 => format!("abs({}) > 64 or {} < 0", e(r), e(r)),
            _ => format!("min({}, {})*{}", e(r), e(r), e(r)),
        }
    }
    let prim = |r: &mut Rng, ks: &[&str]| -> String {
        if r.below(3) == 0 {
            format!(
                "dot({}, {}, {})",
                val(r, ks, 2),
                val(r, ks, 2),
                val(r, ks, 1)
            )
        } else {
            format!(
                "line({}, {}, {}, {}, {}, {})",
                val(r, ks, 2),
                val(r, ks, 2),
                val(r, ks, 2),
                val(r, ks, 2),
                val(r, ks, 1),
                val(r, ks, 1)
            )
        }
    };
    const COUNTS: [u32; 12] = [1, 12, 31, 32, 33, 64, 127, 128, 129, 192, 300, 1024];
    let (mut ok, mut lanes, mut frames) = (0, 0, 0);
    for k in 0..n {
        let mut body = String::new();
        let mut ks = vec!["k"];
        let names = ["v1", "v2", "v3", "v4", "v5", "v6", "v7"];
        for name in names.iter().take(1 + r.below(names.len())) {
            body += &format!("{name} = {}\n", val(&mut r, &ks, 3));
            ks.push(name);
        }
        // (Up to 10 lines' and dots' steps: past `draw::PRIMS` too.)
        for _ in 0..1 + r.below(5) {
            body += &format!("{}\n", prim(&mut r, &ks));
        }
        if r.below(4) == 0 {
            let n2 = COUNTS[r.below(COUNTS.len())];
            let mut inner = ks.clone();
            inner.push("j");
            body += &format!(
                "for j < {n2} {{\nhh = {}\n{}\n}}\n",
                val(&mut r, &inner, 2),
                prim(&mut r, &[&inner[..], &["hh"]].concat())
            );
        }
        let n1 = COUNTS[r.below(COUNTS.len())];
        let mut draw = format!("for k < {n1} {{\n{body}}}\n");
        if r.below(3) == 0 {
            let n3 = COUNTS[r.below(COUNTS.len())];
            draw += &format!(
                "for m < {n3} {{\nbb = {}\n{}\n}}\n",
                val(&mut r, &["m"], 3),
                prim(&mut r, &["m", "bb"])
            );
        }
        let src = format!(
            "loop {}\ntable T = 0.5, -2, 30, 100, 7, 0.125\nframe {{ f = t*{}; g = a*3 - 1 }}\ndraw {{\n{draw}}}\n{}",
            1 + k % 5,
            r.below(50),
            ["gray(ink)", "pal(fire, ink)", "#40c0ff * ink", "rgb(ink, ur, x)"][k % 4],
        );
        let p = match compile(&src) {
            Ok(p) => p,
            Err(e) => {
                if k < 400 && std::env::var("PSI_SHOW").is_ok() {
                    eprintln!("{}", e.render(&src));
                }
                continue;
            }
        };
        ok += 1;
        lanes += p.lane_loops();
        for j in 0..4 {
            let ms = [0, 333, 2999, 9000][j];
            let a = [0, 1 << 15, 1 << 16, -(3 << 16)][r.below(4)];
            let i = opendmd_psi::Inputs { a, ..inputs(ms) };
            frames += 1;
            let want = frame_and_ink::<128>(&p, &i, &under, true);
            assert!(
                frame_and_ink::<128>(&p, &i, &under, false) == want,
                "case {k} at {ms} ms, a {a}: {src}\n{}",
                p.listing()
            );
            assert!(
                frame_and_ink::<64>(&p, &i, &under, false) == want,
                "case {k} (64 lanes) at {ms} ms: {src}"
            );
        }
    }
    assert!(ok * 10 > n * 7, "only {ok} of {n} compiled");
    assert!(
        lanes * 3 > n,
        "only {lanes} loops ran as lanes in {n} formulas"
    );
    eprintln!("{n} formulas, {ok} compiled, {lanes} loops as lanes, {frames} frames compared");
}

/// An example's source (`opendmd-web/formulas/<slug>.psi`, the file
/// without its `-- author:`/`-- licence:`/`-- bundle` header). Built in
/// (`include_str!`): the wasm run has no file system.
macro_rules! example {
    ($slug:literal) => {
        strip_header(include_str!(concat!("../../opendmd-web/formulas/", $slug, ".psi")))
    };
}

fn strip_header(text: &str) -> String {
    text.split_inclusive('\n')
        .skip_while(|l| ["-- author:", "-- licence:", "-- license:", "-- bundle"].iter().any(|h| l.starts_with(h)))
        .collect()
}

/// The examples that draw run their big loops as lanes, and draw the same.
#[test]
fn the_drawing_examples_run_as_lanes() {
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    for (name, src, want) in [
        ("Starfield", example!("starfield"), 1),
        ("Wireframe cube", example!("wireframe-cube"), 0),
        ("Endless road", example!("endless-road"), 0),
    ] {
        let p = compile(&src).unwrap();
        assert_eq!(p.lane_loops(), want, "{name}\n{}", p.listing());
        for ms in [0, 100, 1234, 2399] {
            let i = inputs(ms);
            assert!(
                frame_and_ink::<128>(&p, &i, &under, false)
                    == frame_and_ink::<128>(&p, &i, &under, true),
                "{name} at {ms} ms"
            );
        }
    }
}

/// A frame of `p` at `i` over `under` with the draw canvas and, when `p`
/// has still values, a still cache kept on a first frame at `i` and then
/// drawn from (so the fused runs read cached lanes too).
fn frame_full<const N: usize>(p: &Program, i: &opendmd_psi::Inputs, under: &[u8]) -> Vec<u8> {
    let mut s = Scratch::<N>::new();
    let mut ink = Ink::boxed();
    let mut st = Still::new();
    let mut out = vec![0x5a; W * H * 3];
    let cached = p.still_lanes() > 0 && st.take(p);
    for _ in 0..1 + usize::from(cached) {
        render_rows_still::<N>(
            p,
            &mut s,
            Some(&mut ink),
            cached.then_some(&mut st),
            i,
            &OWN,
            Some(under),
            u32::MAX,
            &mut out,
        );
    }
    out
}

/// Fused runs (`crate::fuse`: a checker's floors, add and mask; a multiply
/// and an add; a palette colour of a choice, of a pick, of the canvas) draw
/// what their steps draw: random formulas full of the patterns, with the
/// values on the way sometimes read again later (then the run must not be
/// fused), colours in layers, scales that are lanes or values for the
/// lane, still and column runs, compared with the same formula compiled
/// without the markers (`compile_unfused`) at times across the loop and
/// past it, sliders in range and out (the checked frames run the steps),
/// at both lane widths. `PSI_FUZZ_N` sets the number of formulas (1,000 by
/// default).
#[test]
fn fused_runs_are_their_steps() {
    let mut r = Rng(0x0f05_ed00_5ca1_ab1e);
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    let n: usize = std::env::var("PSI_FUZZ_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    const LEAF: &[&str] = &[
        "px",
        "py",
        "x",
        "y",
        "t",
        "a",
        "px*0.37",
        "py*1.5",
        "t/0.3",
        "len(px - 63.5, py - 15.5)",
        "atan2(py - 15.5, px - 63.5)*16",
        "sin(x*3 + t)",
        "px*a",
        "-px*2",
        "1000*a",
        "i",
    ];
    let leaf = |r: &mut Rng| -> String {
        if r.below(3) == 0 {
            expr(r, 1)
        } else {
            LEAF[r.below(LEAF.len())].to_string()
        }
    };
    const KINDS: [&str; 7] = [
        "mul_add",
        "checker",
        "checker_3",
        "pal_select",
        "pal_select_k",
        "pal_pick",
        "colour_of_ink_1",
    ];
    let mut kinds = [0usize; 7];
    let (mut compiled, mut frames) = (0, 0);
    for k in 0..n {
        // (Mostly values of the pixel, not of its column or row alone: those
        // go to the column run or the row steps.)
        let mut uv = || {
            let (p, q) = (leaf(&mut r), leaf(&mut r));
            match r.below(4) {
                0 => p,
                1 => format!("{p} + {q}*0.5"),
                2 => format!("{p}*0.25 - {q}"),
                _ => format!("({p}) + px*py*0.01"),
            }
        };
        let (u, v) = (uv(), uv());
        let m = [2, 2, 4, 8][r.below(4)];
        // (A first term that is not a floor must not take the checker's
        // kernel, which floors it again: `fuse::floored`.)
        let sum = [
            "floor(u) + floor(v)",
            "floor(u) + floor(v)",
            "u + floor(v)",
            "frac(u)*4 + floor(v)",
            "floor(v) + (u - 0.5)",
            "floor(u/2) + floor(v)",
        ][r.below(6)];
        let mut lines = vec![
            format!("u = {u}"),
            format!("v = {v}"),
            format!("cell = ({sum}) % {m}"),
            format!(
                "side = {} + {}*{}",
                ["0.0635", "t", "py*0.01"][r.below(3)],
                ["0.0159", "a", "2"][r.below(3)],
                ["cos(u/16)", "u", "px", "v*0.5"][r.below(4)]
            ),
            "lit = floor(min(v, 10)*side + (u > 0)) / 8".to_string(),
            if r.below(3) == 0 {
                // A sprite's value (its key's 0..8) as the pick's choice.
                format!("m = S[px/{} - t, py % 5]", [2, 4, 1][r.below(3)])
            } else {
                format!(
                    "m = floor(px/{}) % {}",
                    [8, 16, 3][r.below(3)],
                    [9, 4, 17, 20][r.below(4)]
                )
            },
        ];
        // The formula's own palette (`P`) as often as a built-in one.
        let own = r.below(2) == 0;
        let (chrono, fire, rainbow, ice) = if own {
            ("P", "P", "P", "P")
        } else {
            ("chrono", "fire", "rainbow", "ice")
        };
        let reuse = r.below(3) == 0;
        let ca = ["frac(0.711 + t/4.8)", "0.78", "a", "t"][r.below(4)];
        let cb = ["frac(0.336 + t/4.8)", "0.28", "1 - a", "py/31"][r.below(4)];
        let colour = match r.below(6) {
            0 => format!("pal({chrono}, cell ? {ca} : {cb}) * lit"),
            1 => format!(
                "pal({fire}, cell ? {ca} : {cb}) * {}",
                ["(py/31)", "0.5", "fog"][r.below(3)]
            ),
            2 => format!("pal({fire}, cell ? {ca} : {cb})"),
            3 => {
                let c: Vec<String> = (0..[3, 9, 16, 18][r.below(4)])
                    .map(|j| {
                        ["t/9", "py/40", "0.3", "a"][j % 4].to_string()
                            + &format!(" + {}", j as f64 / 40.0)
                    })
                    .collect();
                format!(
                    "pal({rainbow}, pick(m, {})){}",
                    c.join(", "),
                    ["", " * (py/31)", " * lit"][r.below(3)]
                )
            }
            4 if own && r.below(2) == 0 => {
                [format!("pal({ice}, ink)"), format!("pal({ice}, ink) * fog")][r.below(2)].clone()
            }
            4 => [
                "pal(ice, ink)",
                "#40c0ff * ink",
                "gray(ink)",
                "rgb(ink, ink*ink, 1 - ink)",
                "hsv(ink*0.5 + t/8, 1, ink)",
                "pal(fire, ink*0.8 + 0.1) * (ink > 0.5 ? 1 : 0.5)",
                "pal(fire, T[floor(ink*3)]) * (floor(ink*8)/8)",
                "rgb(ink, ur, 0.5)",
                "pal(ice, ink) * fog",
                "gray(sin(ink*3 + t)*0.5 + 0.5)",
                "gray(ink)",
                "pal(fire, ink)",
                "pal(rainbow, ink)",
            ][r.below(13)]
            .to_string(),
            5 if r.below(2) == 0 => {
                // A random chain of the canvas: each step's function as the
                // VM works it out for one pixel (`eval`) and along a lane.
                const INK: &[&str] = &[
                    "ink",
                    "ink*ink",
                    "sin(ink*7 + t)",
                    "floor(ink*5)/5",
                    "hash(ink, 3)",
                    "noise(ink*4, 2, 0)",
                    "noise(ink*4, t, 1.5)",
                    "T[floor(ink*4)]",
                    "pick(floor(ink*3), 0.2, t/9, 0.8)",
                    "abs(ink - 0.5)",
                    "smoothstep(0.2, 0.8, ink)",
                    "1/(ink + 0.1)",
                    "ink % 0.3",
                    "sqrt(ink)",
                    "atan2(ink, 0.5)",
                    "len(ink, a)",
                    "mix(0.2, 0.9, ink)",
                    "clamp(ink*2, a, 1)",
                    "digit(floor(ink*9), 1, 2)",
                    "tri(ink + t)",
                    "ink*1000 + a*30000",
                ];
                let mut e = || INK[r.below(INK.len())];
                format!("hsv({} + {}, {}, {})", e(), e(), e(), e())
            }
            _ => "rgb(cell, side, lit)".to_string(),
        };
        lines.insert(0, "fog = py < 3 ? 0 : py < 10 ? 0.625 : 1".into());
        // (Sparse dots and a line, or streaks whose bytes change at nearly
        // every pixel: a colour of ink is kept per byte on the first and
        // drawn by its steps on the second, `fuse::INK_CHANGES`.)
        let draw = match (colour.contains("ink"), r.below(2)) {
            (false, _) => "",
            (true, 0) => "table T = 0.1, 0.5, 0.9, 0.3\ndraw { for k < 40 { dot(hash(k, 3)*128, hash(k, 5)*32 + t*3, frac(k/7)) }\nline(0, 3, 127, 20 + t, 0.2, 1) }\n",
            _ => "table T = 0.1, 0.5, 0.9, 0.3\ndraw { for k < 64 { line(k*2, 0, k*2 + 30 + t, 31, 0, 1) } }\n",
        };
        let body = if reuse {
            // The values on the way read again in another layer.
            format!(
                "layer {{ {colour} }}\nlayer add {{ rgb(floor(u)*0.01, cell, side*0.1 + {}) }}",
                ["floor(v)*0.02", "lit", "m/20", "0"][r.below(4)]
            )
        } else if r.below(4) == 0 {
            format!("layer {{ gray(x) }}\nlayer screen {{ {colour} }}")
        } else {
            colour
        };
        let decl = "palette P = #200010, #ff2000, #ffd040, #40c0ff, #ffffff, #0000ff\nsprite S {\nkey . 0, R 6, D 7, L 8, a 1, b 2, c 3\n.RRDDLLabc..\nRD.abc.LLRDa\n..cbaRRDDL.b\nLLabcRD....c\n}\n";
        let src = format!(
            "loop {}\n{decl}{}{}\n{body}",
            1 + k % 5,
            draw,
            lines.join("\n")
        );
        let mut p = Program::default();
        let mut q = Program::default();
        if opendmd_psi::compile_into(&src, &mut p).is_err() {
            if k < 50 && std::env::var("PSI_SHOW").is_ok() {
                let e = opendmd_psi::compile_into(&src, &mut p).unwrap_err();
                eprintln!("{}", e.render(&src));
            }
            continue;
        }
        opendmd_psi::compile_unfused(&src, &mut q).expect("compiles unfused");
        compiled += 1;
        let listing = p.listing();
        for (j, name) in KINDS.iter().enumerate() {
            kinds[j] += listing.matches(&format!("fuse({name};")).count();
        }
        for j in 0..6 {
            let ms = [0, 777, 2500, 4999, 9000, 60_000][j];
            let a = [0, 1 << 15, 1 << 16, -(1 << 16), 3 << 16, 21845][r.below(6)];
            let i = opendmd_psi::Inputs { a, ..inputs(ms) };
            frames += 1;
            let want = frame_full::<128>(&q, &i, &under);
            assert!(
                frame_full::<128>(&p, &i, &under) == want,
                "case {k} at {ms} ms, a {a}: {src}\n{listing}"
            );
            assert!(
                frame_full::<64>(&p, &i, &under) == frame_full::<64>(&q, &i, &under),
                "case {k} (64 lanes) at {ms} ms, a {a}: {src}"
            );
        }
    }
    eprintln!(
        "{n} formulas, {compiled} compiled, {frames} frames; fused {:?}",
        KINDS.iter().zip(kinds).collect::<Vec<_>>()
    );
    assert!(compiled * 10 > n * 8, "only {compiled} of {n} compiled");
    for (name, &c) in KINDS.iter().zip(&kinds) {
        assert!(c * 30 > n, "{name}: only {c} fused runs in {n} formulas");
    }
}

/// `render_rows_changed` writes the same bytes as `render_rows_still` and
/// says exactly which 4-pixel groups of the rows it drew differ from what
/// the picture held (rows not drawn left as given), over random formulas
/// and frames, full and partial.
#[test]
fn what_changed_is_exactly_what_differs() {
    let mut r = Rng(0x5eed_c0a5_01e5_0a0a);
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    let srcs: Vec<String> = sketches::ALL
        .iter()
        .map(|(_, s)| s.to_string())
        .chain(GOLDEN.iter().map(|g| g.1.to_string()))
        .collect();
    for (k, src) in srcs.iter().enumerate() {
        let Ok(p) = compile(src) else { continue };
        let mut s = Scratch::<128>::new();
        let mut ink = Ink::boxed();
        let mut was = vec![0u8; W * H * 3];
        for j in 0..6u32 {
            let ms = [0, 20, 40, 1000, 1020, 33_333][j as usize];
            let rows = if j % 3 == 2 { r.next() as u32 } else { u32::MAX };
            let mut want = was.clone();
            render_rows_ink::<128>(&p, &mut s, Some(&mut ink), &inputs(ms), &OWN, Some(&under), rows, &mut want);
            let mut got = was.clone();
            let mut ch = [0xdead_beefu32; H];
            opendmd_psi::render_rows_changed::<128>(
                &p, &mut s, Some(&mut ink), None, &inputs(ms), &OWN, Some(&under), rows, &mut got, &mut ch,
            );
            assert!(got == want, "case {k} frame {j}: bytes differ");
            for (y, &bits) in ch.iter().enumerate() {
                if rows & (1 << y) == 0 {
                    assert_eq!(bits, 0xdead_beef, "case {k}: row {y} not drawn but written");
                    continue;
                }
                for g in 0..W / 4 {
                    let o = (y * W + 4 * g) * 3;
                    let differs = was[o..o + 12] != got[o..o + 12];
                    assert_eq!(bits & (1 << g) != 0, differs, "case {k} frame {j}: row {y} group {g}");
                }
            }
            was = got;
        }
    }
}
