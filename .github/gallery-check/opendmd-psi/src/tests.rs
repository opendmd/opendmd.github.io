//! Unit tests: the maths, the compiler's levels and folding, and its error
//! messages (golden frames and the fuzz are in `tests/golden.rs`).

use crate::fixed::{self, ONE};
use crate::program::code;
use crate::*;

fn q(v: f64) -> i32 {
    (v * ONE as f64).round() as i32
}

fn f(v: i32) -> f64 {
    v as f64 / ONE as f64
}

#[test]
fn own_tables_match_the_formulas() {
    for (i, &s) in SIN.iter().enumerate() {
        let want = ((i as f64) * std::f64::consts::TAU / 256.0).sin() * 16384.0;
        assert!((s as f64 - want).abs() <= 0.5, "SIN[{i}]");
    }
    for (i, &a) in ATAN.iter().enumerate() {
        let want = (i as f64 / 256.0).atan() / std::f64::consts::TAU * 65536.0;
        assert!((a as f64 - want).abs() <= 0.5, "ATAN[{i}]");
    }
}

#[test]
fn arithmetic_saturates() {
    assert_eq!(fixed::add(i32::MAX, ONE), i32::MAX);
    assert_eq!(fixed::sub(i32::MIN, ONE), i32::MIN);
    assert_eq!(fixed::mul(q(200.0), q(200.0)), i32::MAX);
    assert_eq!(fixed::mul(q(-200.0), q(200.0)), i32::MIN);
    assert_eq!(fixed::mul(q(1.5), q(-2.0)), q(-3.0));
    assert_eq!(fixed::div(q(1.0), 0), i32::MAX);
    assert_eq!(fixed::div(q(-1.0), 0), i32::MIN);
    assert_eq!(fixed::div(0, 0), 0);
    assert_eq!(fixed::div(q(3.0), q(-2.0)), q(-1.5));
    assert_eq!(fixed::neg(i32::MIN), i32::MAX);
    assert_eq!(fixed::abs(i32::MIN), i32::MAX);
}

#[test]
fn modulo_is_floored() {
    assert_eq!(fixed::modulo(q(5.5), q(2.0)), q(1.5));
    assert_eq!(fixed::modulo(q(-0.5), q(2.0)), q(1.5));
    assert_eq!(fixed::modulo(q(0.5), q(-2.0)), q(-1.5));
    assert_eq!(fixed::modulo(q(1.0), 0), 0);
    assert_eq!(fixed::modulo(i32::MIN, -1), 0);
    assert_eq!(fixed::floor(q(-0.25)), q(-1.0));
    assert_eq!(fixed::frac(q(-0.25)), q(0.75));
}

#[test]
fn waves_in_turns() {
    let t = &OWN;
    assert_eq!(fixed::sin(t, 0), 0);
    assert_eq!(fixed::sin(t, q(0.25)), ONE);
    assert_eq!(fixed::sin(t, q(1.25)), ONE);
    assert_eq!(fixed::sin(t, q(-0.25)), -ONE);
    assert_eq!(fixed::cos(t, 0), ONE);
    for k in 0..1000 {
        let p = k as f64 / 1000.0 * 3.0 - 1.0;
        let s = f(fixed::sin(t, q(p)));
        assert!(
            (s - (p * std::f64::consts::TAU).sin()).abs() < 1e-3,
            "sin({p}) = {s}"
        );
    }
    assert_eq!(fixed::tri(q(0.25)), ONE);
    assert_eq!(fixed::tri(q(0.75)), -ONE);
    assert_eq!(fixed::tri(q(0.125)), q(0.5));
    assert_eq!(fixed::saw(0), -ONE);
    assert_eq!(fixed::saw(q(0.5)), 0);
    assert_eq!(fixed::sqr(q(0.25)), ONE);
    assert_eq!(fixed::sqr(q(0.75)), -ONE);
}

#[test]
fn roots_and_angles_are_close() {
    for k in 1..4000 {
        let v = k as f64 * k as f64 / 1000.0;
        let r = f(fixed::sqrt(q(v)));
        assert!(
            (r - v.sqrt()).abs() <= v.sqrt() * 2e-4 + 2e-4,
            "sqrt({v}) = {r}"
        );
    }
    assert_eq!(fixed::sqrt(-ONE), 0);
    assert_eq!(fixed::len(q(3.0), q(-4.0)), q(5.0));
    assert_eq!(fixed::len(q(30000.0), q(30000.0)), i32::MAX);
    for k in 0..720 {
        let a = (k as f64 / 720.0 - 0.5) * std::f64::consts::TAU;
        let (x, y) = (a.cos() * 0.7, a.sin() * 0.7);
        let got = f(fixed::atan2(q(y), q(x)));
        let want = y.atan2(x) / std::f64::consts::TAU;
        let d = (got - want).abs().min(1.0 - (got - want).abs());
        assert!(d < 2e-4, "atan2({y}, {x}) = {got}, want {want}");
    }
    assert_eq!(fixed::atan2(0, 0), 0);
    assert_eq!(fixed::atan2(i32::MIN, i32::MIN), -q(0.375));
}

#[test]
fn noise_and_hash_stay_in_range() {
    let mut lo = i32::MAX;
    let mut hi = i32::MIN;
    for k in 0..5000 {
        let (x, y, z) = (k * 7919 - 100_000, k * 104_729, -k * 1301);
        let n = fixed::noise(x, y, z);
        assert!((0..ONE).contains(&n));
        lo = lo.min(n);
        hi = hi.max(n);
        let h = fixed::hash(x, y);
        assert!((0..ONE).contains(&h));
    }
    assert!(lo < q(0.2) && hi > q(0.8), "noise spans {lo}..{hi}");
    // Continuous across a lattice line.
    let a = fixed::noise(q(2.0) - 1, q(0.3), 0);
    let b = fixed::noise(q(2.0), q(0.3), 0);
    assert!((a - b).abs() < 64);
}

#[test]
fn colours() {
    assert_eq!(fixed::hsv(0, ONE, ONE), [255, 0, 0]);
    assert_eq!(fixed::hsv(q(1.0 / 3.0), ONE, ONE), [0, 255, 0]);
    assert_eq!(fixed::hsv(q(0.5), 0, q(0.5)), [128, 128, 128]);
    assert_eq!(fixed::chan(q(2.0)), 255);
    assert_eq!(fixed::chan(-5), 0);
    assert_eq!(fixed::pal(OWN.palettes[0], ONE), OWN.palettes[0][31]);
    assert_eq!(fixed::pal(OWN.palettes[0], -ONE), OWN.palettes[0][0]);
}

#[test]
fn numbers_round_to_the_nearest_step() {
    let p = compile("gray(0.1)").unwrap();
    assert_eq!(p.init[p.ops[0].a as usize], 6554);
    let p = compile("gray(.5 + 1.25)").unwrap();
    assert_eq!(p.init[p.ops[0].a as usize], q(1.75));
}

#[test]
fn constants_fold_and_values_share() {
    // Everything constant: just the colour.
    let p = compile("rgb(1 + 2*3 - 6.5, min(1, 0.25) * 2, step(0.5, 0.75))").unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(p.counts(), (0, 0, 1));
    // `x - 0.5` written twice is one step; x*1 and x+0 are x.
    let p = compile("u = x - 0.5\nv = (x - 0.5) * 1 + 0\nrgb(u, v, 0)").unwrap();
    assert_eq!(p.len(), 2, "{}", p.listing());
    // Division by a constant is a multiply; % by a power of two a mask.
    let p = compile("gray(x / 4 + x % 0.5)").unwrap();
    let codes: Vec<u8> = p.ops[..p.len()].iter().map(|o| o.code).collect();
    assert!(codes.contains(&code::MUL) && codes.contains(&code::MASK));
    assert!(!codes.contains(&code::DIV) && !codes.contains(&code::MOD));
}

#[test]
fn levels_run_as_rarely_as_they_can() {
    // t-only: per frame; y-only: per row; x: per pixel.
    let p = compile("a1 = sin(t/4)\nb1 = y * a1\nrgb(x + b1, 0, 0)").unwrap();
    assert_eq!(p.counts(), (2, 1, 2), "{}", p.listing());
    // Division by a per-row value: one reciprocal a row, a multiply a pixel.
    let p = compile("gray(x / (y + 1))").unwrap();
    assert_eq!(p.counts(), (0, 2, 2), "{}", p.listing());
    let plasma = compile(sketches::PLASMA).unwrap();
    assert_eq!(plasma.counts(), (2, 5, 13));
    let ring = compile(sketches::RING_CLOCK).unwrap();
    assert_eq!(ring.counts().0, 8);
}

#[test]
fn loop_header() {
    assert_eq!(compile("gray(x)").unwrap().loop_ms(), 8000);
    assert_eq!(compile("loop 2.5\ngray(x)").unwrap().loop_ms(), 2500);
    assert_eq!(
        compile("-- hi\n\nloop 60 ; gray(x)").unwrap().loop_ms(),
        60000
    );
}

#[test]
fn registers_are_reused() {
    let p = compile(sketches::RING_CLOCK).unwrap();
    assert!(p.regs() <= 4, "{}", p.listing());
    // A step never writes a register it reads.
    for op in &p.ops[p.row_end as usize..p.pixel_end as usize] {
        let ar = code::arity(op.code);
        assert!(![op.a, op.b, op.c][..ar].contains(&op.d), "{op:?}");
    }
}

#[test]
fn too_many_live_values() {
    let mut src = String::new();
    for k in 1..=14 {
        src += &format!("v{k} = sin(x * {k})\n");
    }
    let sum: Vec<String> = (1..=14).map(|k| format!("v{k}")).collect();
    src += &format!("gray({})", sum.join(" + "));
    let e = compile(&src).unwrap_err();
    assert!(e.msg.contains("too many values at once"), "{}", e.msg);
}

/// (source, message, line, column)
const ERRORS: &[(&str, &str, usize, usize)] = &[
    (
        "hsv(tme, 1, 1)",
        "unknown name `tme` — did you mean `t`?",
        1,
        5,
    ),
    (
        "v = sin(x\nhsv(v,1,1)",
        "this `(` is never closed — add `)`",
        1,
        8,
    ),
    (
        "hsv(x, 1)",
        "`hsv` takes 3 values (or 4: the last is how much of the pixel it covers, 0 to 1), but got 2",
        1,
        1,
    ),
    ("gray(x, 1, 2)", "`gray` takes 1 value (or 2", 1, 1),
    ("sin(x, 1)", "`sin` takes 1 value, but got 2", 1, 1),
    (
        "gray(smoothstp(0, 1, x))",
        "unknown function `smoothstp` — did you mean `smoothstep`?",
        1,
        6,
    ),
    (
        "pal(fir, x)",
        "unknown palette `fir` — did you mean `fire`?",
        1,
        5,
    ),
    (
        "frame {\n  g = x * 2\n}\ngray(g)",
        "only values that are the same for every pixel",
        2,
        7,
    ),
    ("frame {\n  g = y\n}\ngray(g)", "this uses `y`", 2, 7),
    (
        "v = 1\nv = 2\ngray(v)",
        "`v` is already set on line 1",
        2,
        1,
    ),
    (
        "t = 3\ngray(t)",
        "`t` is built in (seconds into the loop)",
        1,
        1,
    ),
    ("sin = 3\ngray(x)", "`sin` is built in", 1, 1),
    (
        "x * 2\ngray(x)",
        "only the last line can be a bare value",
        1,
        1,
    ),
    (
        "rgb(x,0,0)\ngray(y)",
        "the colour has to be the last line",
        1,
        1,
    ),
    ("v = x", "the last line should be the colour", 1, 6),
    ("", "empty: write a colour", 1, 1),
    ("-- nothing\n", "empty", 2, 1),
    ("gray(x ^ 2)", "there is no power operator", 1, 8),
    ("gray(2x)", "to multiply, write `2 * x`", 1, 6),
    (
        "gray(rgb(1,1,1))",
        "makes the colour, so it can only be the whole last line",
        1,
        6,
    ),
    ("loop -3\ngray(x)", "`loop` takes a number of seconds", 1, 6),
    ("loop 9999\ngray(x)", "between 0.1 and 3600 seconds", 1, 6),
    ("loop 2\nloop 3\ngray(x)", "`loop` is already set", 2, 1),
    (
        "gray(99999)",
        "numbers must be between -32768 and 32767",
        1,
        6,
    ),
    ("gray(1.2.3)", "`1.2.3` is not a number", 1, 6),
    (
        "gray(x y)",
        "expected `,` or `)` in `gray(…)`, found `y`",
        1,
        8,
    ),
    ("gray(x ? 1)", "expected `:`", 1, 11),
    ("gray(sin)", "`sin` is a function: write `sin(…)`", 1, 6),
    ("gray(fire)", "`fire` is a palette", 1, 6),
    ("gray(t(1))", "`t` is a value, not a function", 1, 6),
    (
        "frame { v = 1\ngray(v)",
        "only `name = value` lines can go inside frame",
        2,
        1,
    ),
    ("frame {\nv = 1\n", "`frame {` is never closed", 3, 1),
    (
        "frame { v = 1 }\nframe { u = 2 }\ngray(v)",
        "there can be one `frame { }` block",
        2,
        1,
    ),
    ("v = \ngray(x)", "a value is missing after `=`", 1, 3),
    ("gray(x) )", "expected the end of the line, found `)`", 1, 9),
    ("gray(x # 2)", "I don't understand `#`", 1, 8),
    ("gray(\"x\")", "there are no strings", 1, 6),
    ("pal(3, x)", "the first value of `pal` is a palette", 1, 5),
    (
        "pal(fire, x, y, x)",
        "`pal` takes a palette and one value (and how much",
        1,
        15,
    ),
    ("gray(loop)", "`loop` can only start a line", 1, 6),
    ("fire = 2\ngray(fire)", "is a palette's name", 1, 1),
    (
        "gray(((((((((((((((((((((((((((x)))))))))))))))))))))))))))",
        "nested too deeply",
        1,
        30,
    ),
];

#[test]
fn error_messages() {
    let mut bad = Vec::new();
    for &(src, want, line, col) in ERRORS {
        match compile(src) {
            Ok(_) => bad.push(format!("{src:?}: compiled")),
            Err(e) => {
                let lc = e.line_col(src);
                if !e.msg.contains(want) || lc != (line, col) {
                    bad.push(format!(
                        "{src:?}: got {:?} at {lc:?}, want {want:?} at {:?}\n{}",
                        e.msg,
                        (line, col),
                        e.render(src)
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

#[test]
fn render_shows_the_line_and_carets() {
    let src = "loop 4\nv = sin(tme)\ngray(v)";
    let e = compile(src).unwrap_err();
    assert_eq!(
        e.render(src),
        "line 2: unknown name `tme` — did you mean `t`?\n  v = sin(tme)\n          ^^^"
    );
}

#[test]
fn continuation_and_separators() {
    assert!(compile("v = x +\n  y\ngray(v)").is_ok());
    assert!(compile("v = min(x,\n  y)\ngray(v)").is_ok());
    assert!(compile("v = x; u = y; rgb(v, u, 0)").is_ok());
    assert!(compile("v = x // a comment\n-- another\ngray(v) -- the end").is_ok());
    assert!(compile("v = x && y || !x\ngray(v)").is_ok());
    assert!(compile("gray(x < 0.5 ? 1 : y > 0.5 ? 0.5 : 0)").is_ok());
}

#[test]
fn program_is_plain_data() {
    // compile_into works on a zeroed value and leaves it empty on an error.
    let mut p = Program::default();
    assert!(compile_into("gray(nope)", &mut p).is_err());
    assert_eq!(p, Program::default());
    assert!(core::mem::size_of::<Program>() <= 1824);
}

#[test]
fn compact_compiles_the_same() {
    let extra = [
        "v = min(x,\n  y) -- c\n\n  // d\ngray(v)",
        "a1 = x +\n y\nframe {\n q = t\n}\nrgb(a1, q, - -x)",
    ];
    for src in sketches::ALL.iter().map(|s| s.1).chain(extra) {
        let c = compact(src);
        assert!(!c.contains('\n') && !c.contains("--"), "{c}");
        assert_eq!(compile(&c).unwrap(), compile(src).unwrap(), "{c}");
    }
    assert_eq!(
        compact(sketches::FIRE),
        "loop 4; heat = noise(x*8, y*4 + t*2, t) * (1.3 - y); pal(fire, clamp(heat, 0, 1))"
    );
}

#[test]
fn sizes() {
    // What the docs say (Program, the scratch at 128 and 64 lanes).
    assert_eq!(core::mem::size_of::<Program>(), 1808);
    assert_eq!(core::mem::size_of::<Scratch<128>>(), 8192);
    assert_eq!(core::mem::size_of::<Scratch<64>>(), 4864);
}

#[test]
fn sliders_and_the_esp32_estimate() {
    let s = |src: &str| compile(src).unwrap().sliders();
    assert_eq!(s("gray(x)"), 0);
    assert_eq!(s("gray(a)"), 1);
    assert_eq!(s("frame { k = c * 2 }\nrgb(x, k, b)"), 0b110);
    assert_eq!(s("k = a\ngray(x)"), 0, "an unused value is dropped");
    let i = |src: &str| compile(src).unwrap().inputs();
    assert_eq!(i("gray(x)"), 0);
    assert_eq!(i("gray(sin(t))"), 8);
    assert_eq!(i("gray(minute / 60 + b)"), 16 | 2);
    // Within 25 % of what psi-probe measured on the ESP32 (effect frame p50, µs).
    for (src, measured) in [
        (sketches::PLASMA, 7245),
        (sketches::FIRE, 4964),
        (sketches::RING_CLOCK, 11817),
    ] {
        let est = compile(src).unwrap().esp32_us();
        assert!(
            est * 4 >= measured * 3 && est * 4 <= measured * 5,
            "estimate {est} µs vs measured {measured} µs for {src}"
        );
    }
}

/// The worst formulas a panel accepts compile (or say why not) and fit
/// its path; compiling them is linear (a node index, lines found only for
/// errors): ~50-80 µs each on the host (`make psi-bench`).
#[test]
fn worst_case_formulas() {
    for (name, f) in crate::worst::ALL {
        let src = f();
        let n = crate::worst::path_len(&src);
        assert!(
            matches!(name, "parts" | "nested" | "parts in layers") || (1900..=2048).contains(&n),
            "{name}: {n} B"
        );
        match compile(&src) {
            Ok(p) => assert!(p.len() <= crate::MAX_OPS, "{name}"),
            Err(e) => assert!(
                e.msg.contains("too long for the panel")
                    || e.msg.contains("too many places")
                    || e.msg.starts_with("unknown name"),
                "{name}: {}",
                e.msg
            ),
        }
    }
    assert!(compile(&crate::worst::many_locals()).is_ok());
}

// --- Layers and what is under ---------------------------------------------------

fn over(src: &str, under: Option<&[u8]>, ms: u32) -> alloc::vec::Vec<u8> {
    let p = compile(src).unwrap_or_else(|e| panic!("{}", e.render(src)));
    let mut out = alloc::vec![0u8; W * H * 3];
    render_rows::<128>(
        &p,
        &mut Scratch::new(),
        &golden::inputs(ms),
        &OWN,
        under,
        u32::MAX,
        &mut out,
    );
    out
}

fn pic() -> alloc::vec::Vec<u8> {
    let mut u = alloc::vec![0u8; W * H * 3];
    golden::picture(&mut u);
    u
}

#[test]
fn bytes_blends_and_cover() {
    for b in 0..=255u8 {
        assert_eq!(fixed::chan(fixed::byte(b)), b, "byte {b} round trip");
        assert_eq!(fixed::mul255(b, 255), b);
        assert_eq!(fixed::mul255(b, 0), 0);
        for d in [0u8, 1, 77, 128, 200, 254, 255] {
            let exact = (b as f64 * d as f64 / 255.0).round() as u8;
            assert_eq!(fixed::mul255(b, d), exact, "{b} x {d}");
            let s = fixed::blend(fixed::BLEND_SCREEN, d, b) as i32;
            let want = 255.0 - (255.0 - d as f64) * (255.0 - b as f64) / 255.0;
            assert!(
                (s as f64 - want).abs() <= 1.0,
                "screen {d} {b}: {s} vs {want}"
            );
            assert_eq!(fixed::cover(d, b, ONE), b);
            assert_eq!(fixed::cover(d, b, 0), d);
            assert_eq!(fixed::cover(d, b, ONE * 3), b, "clamped");
            assert_eq!(fixed::cover(d, b, -5), d, "clamped");
            let half = fixed::cover(d, b, ONE / 2) as i32;
            assert!(
                (half * 2 - (d as i32 + b as i32)).abs() <= 1,
                "{d} {b}: {half}"
            );
        }
    }
    assert_eq!(fixed::byte(255), ONE);
    assert_eq!(fixed::byte(0), 0);
}

#[test]
fn what_is_under_reads_back_exactly() {
    let u = pic();
    assert!(over("rgb(ur, ug, ub)", Some(&u), 0) == u, "identity");
    let l = over("gray(ul)", Some(&u), 0);
    for (p, q) in u.chunks(3).zip(l.chunks(3)) {
        let y = fixed::luma(p[0], p[1], p[2]);
        assert_eq!(q, [y, y, y]);
    }
    // Nothing under: black.
    assert!(over("rgb(ur + 0.25, ug, ul)", None, 0)
        .chunks(3)
        .all(|p| p == [64, 0, 0]));
    // The same as a black picture under it.
    let black = alloc::vec![0u8; W * H * 3];
    for src in [
        "layer { hsv(x, 1, 1) }\nlayer add { gray(ur + 0.2, 0.5) }",
        "rgb(1, x, 0, y)",
        "layer screen { pal(fire, x) }",
    ] {
        assert!(over(src, None, 0) == over(src, Some(&black), 0), "{src}");
    }
}

#[test]
fn layers_blend_as_the_reference_does() {
    let u = pic();
    // Each layer by hand: its colour as v0 writes it, blended, covered.
    let got = over(
        "layer multiply { gray(py % 2 ? 1 : 0.55) }\nlayer add { rgb(x, 0, 0.25, 0.5) }\nlayer max { pal(ice, y, 0.75) }",
        Some(&u),
        0,
    );
    let pal = &PALETTES[1];
    for (i, (d, g)) in u.chunks(3).zip(got.chunks(3)).enumerate() {
        let (px, row) = (i % W, i / W);
        let py = H - 1 - row;
        let (x, y) = (
            fixed::div((px as i32) << 16, 127 << 16),
            fixed::div((py as i32) << 16, 31 << 16),
        );
        let lines = fixed::chan(if py % 2 == 1 { ONE } else { q(0.55) });
        let mut want = [0u8; 3];
        for c in 0..3 {
            let mut v = fixed::mul255(d[c], lines);
            let s = [fixed::chan(x), 0, fixed::chan(ONE / 4)][c];
            v = fixed::cover(v, v.saturating_add(s), ONE / 2);
            let k = fixed::pal(pal, y)[c];
            v = fixed::cover(v, v.max(k), q(0.75));
            want[c] = v;
        }
        assert_eq!(g, want, "pixel {i}");
    }
}

#[test]
fn a_layer_alone_is_the_plain_formula() {
    for (body, plain) in [
        ("hsv(x + t/4, 1, y)", "hsv(x + t/4, 1, y)"),
        (
            "v = noise(x*8, y*4, t)\npal(fire, v)",
            "v = noise(x*8, y*4, t)\npal(fire, v)",
        ),
        ("gray(x, 1)", "gray(x)"),
        ("rgb(x, y, 0, 2)", "rgb(x, y, 0)"),
    ] {
        let a = compile(&alloc::format!("layer {{\n{body}\n}}")).unwrap();
        let b = compile(plain).unwrap();
        assert!(a == b, "{body}:\n{}\nvs\n{}", a.listing(), b.listing());
        assert!(!a.over() && a.layers() == 1);
    }
    // One that covers everything hides what is below it: left out.
    let a = compile("layer add { noise(x*8, y, t) }\nlayer { rgb(1, x, 0) }").unwrap();
    assert!(a == compile("rgb(1, x, 0)").unwrap(), "{}", a.listing());
    let u = pic();
    assert!(
        over("layer add { noise(x*8, y, t) }\nlayer { hsv(x, 1, 1) }\nlayer multiply { gray(1 - ul*0.5) }", Some(&u), 0)
            == over("layer { hsv(x, 1, 1) }\nlayer multiply { gray(1 - ul*0.5) }", Some(&u), 0)
    );
}

#[test]
fn layers_count_their_steps_and_say_what_they_read() {
    let p = compile("layer { gray(x) }\nlayer add { gray(y, 0.5) }").unwrap();
    assert_eq!((p.layers(), p.len()), (2, 2), "{}", p.listing());
    assert!(!p.over(), "the bottom layer covers everything");
    assert!(compile("layer add { gray(y) }").unwrap().over());
    assert!(compile("rgb(1, 0, 0, 0.5)").unwrap().over());
    assert!(compile("rgb(ur, 0, 0)").unwrap().over());
    assert!(!compile("rgb(x, 0, 0, 1)").unwrap().over());
    assert_eq!(compile("gray(ul)").unwrap().inputs(), 32);
    assert!(compile("layer add { gray(y) }")
        .unwrap()
        .listing()
        .contains("out add= gray("));
    // Coverage counts as an operand (sliders, the step's reads).
    assert_eq!(compile("layer screen { gray(x, a) }").unwrap().sliders(), 1);
    // A step never writes a register it reads, across layers too.
    let p = compile(
        "d = len(x - 0.5, y - 0.5)\nlayer { v = sin(d*4 - t)\nhsv(v, 1, 1) }\nlayer add { q = cos(d*8 + t)\nrgb(q, q*ur, 0, d) }\nlayer multiply { gray(1 - d) }",
    )
    .unwrap();
    for op in &p.ops[p.row_end as usize..p.len()] {
        let (v, n) = code::operands(op);
        if !code::is_colour(op.code) {
            assert!(!v[..n].contains(&op.d), "{op:?}\n{}", p.listing());
        }
    }
    let info = compile_into(
        "layer { gray(x) }\nlayer add { gray(y) }",
        &mut Program::default(),
    )
    .unwrap();
    assert_eq!(info.layers, 2);
}

#[test]
fn names_belong_to_their_layer() {
    // The same name in two layers; a shared one in both.
    assert!(
        compile("k = x*2\nlayer { v = k\ngray(v) }\nlayer add { v = k*2\ngray(v, 0.5) }").is_ok()
    );
    // A frame { } block in each layer, and the formula's.
    assert!(compile("frame { s = t }\nlayer { frame { u = s*2 }\ngray(u) }\nlayer add { frame { u = s }\ngray(u) }").is_ok());
    // A layer's names are its own.
    let e = compile("layer { v = x\ngray(v) }\nlayer add { gray(v) }").unwrap_err();
    assert!(e.msg.starts_with("unknown name `v`"), "{}", e.msg);
    // Names after a layer can reuse its names.
    assert!(compile("layer { v = x\ngray(v) }\nv = y\nlayer add { gray(v) }").is_ok());
}

/// (source, message, line, column)
const LAYER_ERRORS: &[(&str, &str, usize, usize)] = &[
    (
        "layer blur { gray(x) }",
        "unknown blend `blur` (a layer goes over, add, multiply, screen, max or min)",
        1,
        7,
    ),
    (
        "layer mutliply { gray(x) }",
        "did you mean `multiply`?",
        1,
        7,
    ),
    (
        "layer add\n{ gray(x) }",
        "expected `{` here (as in `layer add { … }`), found the end of the line",
        1,
        10,
    ),
    ("layer {\ngray(x)\n", "this `layer {` is never closed", 1, 1),
    ("layer { v = x }", "this layer has no colour", 1, 1),
    (
        "layer { gray(x)\nv = 1 }",
        "the colour has to be the layer's last line",
        1,
        9,
    ),
    (
        "layer { x * 2\ngray(x) }",
        "only a layer's last line can be a bare value",
        1,
        9,
    ),
    (
        "layer { layer { gray(x) } }",
        "a layer can't go inside another",
        1,
        9,
    ),
    (
        "layer { loop 4\ngray(x) }",
        "`loop` is the whole formula's",
        1,
        9,
    ),
    (
        "layer { gray(x) }\ngray(y)",
        "with layers, every colour goes inside one",
        2,
        1,
    ),
    (
        "gray(y)\nlayer { gray(x) }",
        "the colour has to be the last line",
        1,
        1,
    ),
    ("layer = 3\ngray(x)", "`layer` is built in", 1, 1),
    (
        "layer { frame { a1 = 1 }\nframe { a2 = 2 }\ngray(a1) }",
        "a layer can have one `frame { }` block",
        2,
        1,
    ),
    (
        "v = 1\nlayer { v = 2\ngray(v) }",
        "`v` is already set on line 1",
        2,
        9,
    ),
    (
        "ur = 2\ngray(ur)",
        "`ur` is built in (the red of the picture under the formula",
        1,
        1,
    ),
    (
        "frame { k = ul }\ngray(k)",
        "but this uses `ul` (the brightness of the picture under",
        1,
        13,
    ),
    (
        "layer add { rgb(1, 1) }",
        "`rgb` takes 3 values (or 4",
        1,
        13,
    ),
];

#[test]
fn layer_error_messages() {
    let mut bad = alloc::vec::Vec::new();
    for &(src, want, line, col) in LAYER_ERRORS {
        match compile(src) {
            Ok(_) => bad.push(alloc::format!("{src:?}: compiled")),
            Err(e) => {
                let lc = e.line_col(src);
                if !e.msg.contains(want) || lc != (line, col) {
                    bad.push(alloc::format!(
                        "{src:?}: got {:?} at {lc:?}, want {want:?} at {:?}\n{}",
                        e.msg,
                        (line, col),
                        e.render(src)
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

// --- Parts: one formula inside another --------------------------------------------

/// `src` pasted in as `part p { … }` and drawn whole (as the last line and
/// as a layer), under a `loop` of its own length: the same bytes.
fn pasted(src: &str) -> [String; 2] {
    let lp = compile(src).unwrap().loop_ms();
    let inner: String = src.lines().map(|l| alloc::format!("  {l}\n")).collect();
    let head = alloc::format!("loop {}\npart p {{\n{inner}}}\n", lp as f64 / 1000.0);
    [
        alloc::format!("{head}p"),
        alloc::format!("{head}layer {{ p }}"),
    ]
}

#[test]
fn a_part_drawn_whole_is_the_formula() {
    let u = pic();
    let all = sketches::ALL
        .iter()
        .map(|s| (s.1, 0u32))
        .chain(golden::FRAMES.iter().map(|f| (f.1, f.2)))
        .chain(golden::OVER.iter().map(|f| (f.1, f.2)));
    for (src, ms) in all {
        for ms in [ms, ms + 1230, ms + 7000] {
            let want = over(src, Some(&u), ms);
            for p in pasted(src) {
                assert!(over(&p, Some(&u), ms) == want, "{p}\nat {ms} ms");
            }
        }
    }
    // Drawn where the pixel is, written out: the same program.
    let p = "part p { v = sin(x*3 + t)\nhsv(v, 1, y) }\n";
    assert_eq!(
        compile(&alloc::format!("{p}p(x, y)")).unwrap(),
        compile(&alloc::format!("{p}p")).unwrap()
    );
}

#[test]
fn parts_have_their_own_names_values_and_time() {
    let same = |a: &str, b: &str, ms: u32| {
        assert!(
            over(a, None, ms) == over(b, None, ms),
            "{a}\nvs\n{b}\nat {ms} ms"
        )
    };
    // The same names inside and outside; outside names unseen inside.
    same(
        "v = y\npart p { v = x\ngray(v) }\nlayer { p }\nlayer add { gray(v, 0.5) }",
        "layer { gray(x) }\nlayer add { gray(y, 0.5) }",
        0,
    );
    let e = compile("k = 1\npart p { gray(k) }\np").unwrap_err();
    assert!(e.msg.starts_with("unknown name `k`"), "{}", e.msg);
    // Its values: `p.v`, and at another place `p(x', y').v`.
    same("part p { v = x*2\ngray(v) }\ngray(p.v * 0.5)", "gray(x)", 0);
    same(
        "part p { v = x\ngray(v) }\ngray(p(1 - x, y).v)",
        "gray(1 - x)",
        0,
    );
    // Drawn elsewhere: x and y, and px, py, i from them.
    same(
        "part p { rgb(x, y, 0) }\np(1 - x, y*0.5)",
        "rgb(1 - x, y*0.5, 0)",
        0,
    );
    same(
        "part p { rgb(px % 3 / 2, py % 4 / 3, i % 7 / 6) }\np(x*2, 1 - y)",
        "qx = floor(x*2*127 + 0.5); qy = floor((1 - y)*31 + 0.5)\nrgb(qx % 3 / 2, qy % 4 / 3, ((31 - qy)*128 + qx) % 7 / 6)",
        0,
    );
    // Its own loop: `t` modulo it, unless the formula's is no longer.
    same(
        "loop 8\npart p { loop 2\ngray(t/2) }\np",
        "loop 2\ngray(t/2)",
        1000,
    );
    assert!(
        over("loop 8\npart p { loop 2\ngray(t/2) }\np", None, 3000)
            == over("loop 2\ngray(t/2)", None, 1000)
    );
    let plain = compile("loop 2\npart p { loop 4\ngray(t/4) }\np").unwrap();
    assert_eq!(
        plain,
        compile("loop 2\ngray(t/4)").unwrap(),
        "no modulo needed"
    );
    // Nested: the inner part's place is the outer's.
    same(
        "part q { part r { gray(x) }\nr(x*0.5, y) }\nq(1 - x, y)",
        "gray((1 - x)*0.5)",
        0,
    );
    // Coverage: a part's colour takes one more value, like every colour.
    same(
        "layer { gray(0.2) }\npart p { gray(x) }\nlayer { p(x, y, 0.5) }",
        "layer { gray(0.2) }\nlayer { gray(x, 0.5) }",
        0,
    );
    same(
        "layer { gray(0.2) }\npart p { gray(x, y) }\nlayer add { p(x, y, 0.5) }",
        "layer { gray(0.2) }\nlayer add { gray(x, y*0.5) }",
        0,
    );
    // A part with layers keeps them (each covering that much less).
    same("part p { layer { gray(x) }\nlayer multiply { gray(y) } }\nlayer { gray(1) }\nlayer { p(x, y, 0.5) }",
        "layer { gray(1) }\nlayer { gray(x, 0.5) }\nlayer multiply { gray(y, 0.5) }", 0);
    // Unused, it costs nothing.
    assert_eq!(
        compile("part p { gray(noise(x, y, t)) }\ngray(x)").unwrap(),
        compile("gray(x)").unwrap()
    );
    // A part only for its values needs no colour.
    same(
        "part m { k = step(0.5, x) }\ngray(m.k)",
        "gray(step(0.5, x))",
        0,
    );
}

#[test]
fn parts_stay_within_the_limits() {
    // Drawn at many places: each is steps of its own, and the 256 hold.
    let mut s = String::from("part p { hsv(sin(x*3 + y)*0.5 + noise(x*4, y, t), 1, 1) }\n");
    for k in 0..40 {
        s += &alloc::format!("layer add {{ p(x + {k}.5, y) }}\n");
    }
    let e = compile(&s).unwrap_err();
    assert!(e.msg.contains("too long for the panel"), "{}", e.msg);
    // Read again too often (nested, each drawn at two places).
    let mut s = String::from("part p0 { gray(x) }\n");
    for k in 1..8 {
        s = alloc::format!("part p{k} {{\n{s}layer {{ p{}(x*0.5, y) }}\nlayer {{ p{}(x*0.25 + 0.1, y, 0.5) }}\n}}\n", k - 1, k - 1);
    }
    s += "p7";
    let e = compile(&s).unwrap_err();
    assert!(e.msg.contains("parts go too deep"), "{}", e.msg);
    // Not as deep, but read again and again: bounded too.
    let mut s = String::from("part p0 { v = x\ngray(v) }\n");
    for k in 1..5 {
        let j = k - 1;
        s = alloc::format!("part p{k} {{\n{s}v = p{j}(x*0.5, y).v + p{j}(y, x).v + p{j}(x + 0.1, y).v + p{j}(x, y*2).v + p{j}(1 - x, y).v\ngray(v)\n}}\n");
    }
    s += "p4";
    let e = compile(&s).unwrap_err();
    assert!(e.msg.contains("too many places"), "{}", e.msg);
}

/// (source, message, line, column)
const PART_ERRORS: &[(&str, &str, usize, usize)] = &[
    (
        "part { gray(x) }\ngray(x)",
        "a part needs a name, as in `part p { … }`",
        1,
        6,
    ),
    (
        "part p gray(x)",
        "expected `{` here (as in `part p { … }`), found `gray`",
        1,
        8,
    ),
    ("part p {\ngray(x)\n", "this `part {` is never closed", 1, 1),
    (
        "v = 1\npart v { gray(x) }\ngray(x)",
        "`v` is already set on line 1",
        2,
        6,
    ),
    (
        "part p { gray(x) }\np = 2\ngray(x)",
        "`p` is already set on line 1",
        2,
        1,
    ),
    ("part t { gray(x) }\ngray(x)", "`t` is built in", 1, 6),
    (
        "part p { gray(x) }\ngray(p)",
        "`p` is a part: draw it as a whole last line",
        2,
        6,
    ),
    (
        "part p { gray(x) }\ngray(p.w)",
        "part `p` sets no value `w`",
        2,
        8,
    ),
    (
        "part p { wave = x\ngray(x) }\ngray(p.wav)",
        "did you mean `wave`?",
        3,
        8,
    ),
    (
        "part p { gray(x) }\np(0.5)",
        "`p` takes 2 values, where to draw it (x, y) (or 3: the last is how much",
        2,
        1,
    ),
    (
        "part p { v = x\ngray(x) }\ngray(p(x, y, 1).v)",
        "`p` takes 2 values, where to draw it (x, y), but got 3",
        3,
        6,
    ),
    ("part m { k = x }\nm", "part `m` has no colour", 2, 1),
    (
        "part p { gray(x)\nv = 1 }\np",
        "the colour has to be the part's last line",
        1,
        10,
    ),
    (
        "part p { gray(tme) }\np",
        "unknown name `tme` — did you mean `t`?",
        1,
        15,
    ),
    (
        "part p { layer { gray(x) }\nlayer add { gray(y) } }\nlayer add { p }",
        "this part has layers of its own",
        3,
        7,
    ),
    (
        "layer { part p { gray(x) } }",
        "a part goes outside the layers",
        1,
        9,
    ),
    (
        "part p { loop 2\nloop 3\ngray(x) }\np",
        "`loop` is already set",
        2,
        1,
    ),
    (
        "use Plasma as p\np",
        "`use` puts a formula in by its name",
        1,
        1,
    ),
    ("part = 2\ngray(x)", "`part` is built in", 1, 1),
    (
        "gray(x.y)",
        "expected `,` or `)` in `gray(…)`, found `.`",
        1,
        7,
    ),
];

#[test]
fn part_error_messages() {
    let mut bad = alloc::vec::Vec::new();
    for &(src, want, line, col) in PART_ERRORS {
        match compile(src) {
            Ok(_) => bad.push(alloc::format!("{src:?}: compiled")),
            Err(e) => {
                let lc = e.line_col(src);
                if !e.msg.contains(want) || lc != (line, col) {
                    bad.push(alloc::format!(
                        "{src:?}: got {:?} at {lc:?}, want {want:?} at {:?}\n{}",
                        e.msg,
                        (line, col),
                        e.render(src)
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

#[test]
fn compact_keeps_parts() {
    let src = "loop 8\npart p {\n  -- Plasma\n  loop 4\n  v = sin(x*2 + t/4)\n  hsv(v, 1, 1)\n}\nlayer { p }\nlayer add { p(1 - x, y, 0.5) }";
    assert_eq!(compile(&compact(src)).unwrap(), compile(src).unwrap());
}

#[test]
fn suggestions_measure_edits_and_swaps() {
    use crate::compile::distance_for_tests as d;
    assert_eq!(d("tme", "t"), 2);
    assert_eq!(d("smoothstp", "smoothstep"), 1);
    assert_eq!(d("mutliply", "multiply"), 1);
    assert_eq!(d("", "abc"), 3);
    assert_eq!(d("kitten", "sitting"), 3);
    assert_eq!(d("ca", "abc"), 3);
    assert_eq!(d(&"a".repeat(24), &"b".repeat(24)), 24);
}
