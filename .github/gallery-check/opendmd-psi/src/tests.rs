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
    ("hsv(x, 1)", "`hsv` takes 3 values, but got 2", 1, 1),
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
        "pal(fire, x, y)",
        "`pal` takes a palette and one value",
        1,
        12,
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
