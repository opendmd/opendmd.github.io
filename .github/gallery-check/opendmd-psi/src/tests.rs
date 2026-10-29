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
fn flat_noise_is_noise_at_a_whole_z() {
    let mut h: u32 = 12345;
    let mut r = || {
        h ^= h << 13;
        h ^= h >> 17;
        h ^= h << 5;
        h as i32
    };
    for _ in 0..20000 {
        let (x, y) = (r() >> 8, r() >> 8);
        let z = (r() >> 16) << 16;
        assert_eq!(
            fixed::noise_flat(x, y, z),
            fixed::noise(x, y, z),
            "{x} {y} {z}"
        );
    }
    // The VM takes it for a whole scalar z, the eight corners otherwise.
    let flat = compile("gray(noise(x*8, y*4, 3) + noise(px, py, -1)*0.25)").unwrap();
    let full = compile("frame { z = 3 + t*0 + floor(a*0.1) }\ngray(noise(x*8, y*4, z) + noise(px, py, -1 + z*0)*0.25)").unwrap();
    let mut s = Scratch::<128>::new();
    let (mut o1, mut o2) = (vec![0u8; W * H * 3], vec![0u8; W * H * 3]);
    render(&flat, &mut s, &Inputs::default(), &OWN, &mut o1);
    render(&full, &mut s, &Inputs::default(), &OWN, &mut o2);
    assert_eq!(o1, o2);
    assert!(flat.esp32_us() < full.esp32_us());
}

#[test]
fn a_scaled_palette_is_exactly_its_channels_times_k() {
    // Every entry of every palette at scales across the range, the edges
    // included: the fast paths agree with chan(mul(byte(b), s)).
    let mut scales: Vec<i32> = vec![
        i32::MIN,
        -ONE,
        -1,
        0,
        1,
        2,
        255,
        256,
        ONE - 1,
        ONE,
        ONE + 1,
        2 * ONE,
        300 * ONE,
        i32::MAX,
    ];
    scales.extend((0..=4096).map(|k| k * 17));
    for p in &PALETTES {
        for i in 0..32 {
            let k = (i << 11) + 5;
            let c = fixed::pal(p, k);
            for &s in &scales {
                let want: Vec<u8> = c
                    .iter()
                    .map(|&b| fixed::chan(fixed::mul(fixed::byte(b), s)))
                    .collect();
                assert_eq!(&fixed::pal_lit(p, k, s)[..], &want[..], "entry {i} at {s}");
            }
        }
    }
}

#[test]
fn a_scaled_palette_is_its_channels_scaled() {
    // `pal(P, v) * k` against the same colour written as rgb of its
    // channels (the palette entries' bytes over 255, times k).
    let p = compile("pal(fire, x) * y").unwrap();
    assert_eq!(p.layers(), 1);
    assert_eq!(p.inputs() & 7, 0);
    let mut s = Scratch::<128>::new();
    let mut out = vec![0u8; W * H * 3];
    render(&p, &mut s, &Inputs::default(), &OWN, &mut out);
    for (i, px) in out.chunks(3).enumerate() {
        let (x, row) = (i % W, i / W);
        let xv = fixed::div((x as i32) << 16, ((W - 1) as i32) << 16);
        let yv = fixed::div(((H - 1 - row) as i32) << 16, ((H - 1) as i32) << 16);
        let c = fixed::pal(&PALETTES[0], xv);
        let want: Vec<u8> = c
            .iter()
            .map(|&b| fixed::chan(fixed::mul(fixed::byte(b), yv)))
            .collect();
        assert_eq!(px, &want[..], "pixel {i}");
    }
    // A scale over 2^n is the value and a shift in the step: the same bytes
    // as the multiply (and one step fewer).
    let p = compile("pal(chrono, x) * (floor(y*8 + px % 3 - 1)/8)").unwrap();
    let q = compile("pal(chrono, x) * (floor(y*8 + px % 3 - 1)/8*(1 + a))").unwrap();
    assert!(p.len() < q.len(), "{}", p.listing());
    let (mut o1, mut o2) = (vec![0u8; W * H * 3], vec![0u8; W * H * 3]);
    render(&p, &mut s, &Inputs::default(), &OWN, &mut o1);
    render(&q, &mut s, &Inputs::default(), &OWN, &mut o2);
    assert_eq!(o1, o2);
    // Times a slider, chained, in a layer with coverage; gray is gray(v*k).
    let p = compile("layer { hsv(x, 1, 1) }\nlayer screen { pal(ice, y, 0.5) * a * 2 }").unwrap();
    assert_eq!(p.sliders(), 1);
    assert!(compile("pal(gray, x) * 0.5")
        .unwrap()
        .listing()
        .contains("gray"));
    assert_eq!(
        compile("pal(gray, x) * y").unwrap().esp32_us(),
        compile("gray(x * y)").unwrap().esp32_us()
    );
    // Not into sums.
    let e = compile("pal(fire, x) * y + 0.1").unwrap_err();
    assert!(e.msg.contains("can only be scaled"), "{}", e.msg);
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
    // (Before narrowing: then the multiply by 1/4 is a shift.)
    let mut p = Program::default();
    crate::compile_with("gray(x / 4 + x % 0.5)", &mut p, true, false).unwrap();
    let codes: Vec<u8> = p.ops[..p.len()].iter().map(|o| o.code).collect();
    assert!(codes.contains(&code::MUL) && codes.contains(&code::MASK));
    assert!(!codes.contains(&code::DIV) && !codes.contains(&code::MOD));
    let p = compile("gray(x / 4 + x % 0.5)").unwrap();
    let codes: Vec<u8> = p.ops[..p.len()].iter().map(|o| o.code).collect();
    assert!(codes.contains(&code::SHR) && codes.contains(&code::ADD_W));
}

#[test]
fn levels_run_as_rarely_as_they_can() {
    // t-only: per frame; y-only: per row; x: per pixel.
    let p = compile("a1 = sin(t/4)\nb1 = y * a1\nrgb(x + b1, 0, 0)").unwrap();
    assert_eq!(p.counts(), (2, 1, 2), "{}", p.listing());
    // Division by a per-row value: one reciprocal a row, a multiply a pixel.
    let p = compile("gray(x / (y + 1))").unwrap();
    assert_eq!(p.counts(), (0, 2, 2), "{}", p.listing());
    // One over a pixel value: `recip` (the same bits as the division).
    let p = compile("gray(1 / (px + 1))").unwrap();
    assert!(p.listing().contains("recip"), "{}", p.listing());
    assert!(!p.listing().contains("div"), "{}", p.listing());
    let plasma = compile(sketches::PLASMA).unwrap();
    // (its `len(x-0.5, y-0.5)*6` is worked out once: the still cache's
    // marker is one step more)
    // and its `sin(x*3 + t/8)` once a column: a column run, one more)
    assert_eq!(plasma.counts(), (2, 5, 15));
    assert_eq!(plasma.still_lanes(), 1);
    assert_eq!(plasma.column_run().len(), 4, "{}", plasma.listing());
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

/// `fps N` (E801-E802): the frame time it asks for; `fps` stays a name a
/// formula may set; a part's (a whole formula pasted in) is the host's.
#[test]
fn fps_line() {
    assert_eq!(compile("gray(x)").unwrap().frame_ms(), None);
    assert_eq!(compile("fps 100\ngray(x)").unwrap().frame_ms(), Some(10));
    assert_eq!(
        compile("loop 4; fps 50\ngray(x)").unwrap().frame_ms(),
        Some(20)
    );
    assert_eq!(compile("fps 25\ngray(x)").unwrap().frame_ms(), Some(40));
    // The same picture: only the player's frame grid changes.
    let (a, b) = (
        compile("gray(x + t)").unwrap(),
        compile("fps 100\ngray(x + t)").unwrap(),
    );
    assert_eq!(a.ops[..a.len()], b.ops[..b.len()]);
    assert_eq!(compile("fps = 3\ngray(x*fps)").unwrap().frame_ms(), None);
    let p = compile("part p {\n  fps 100\n  gray(x)\n}\np").unwrap();
    assert_eq!(p.frame_ms(), None, "a part's fps is left to the formula's");
    let p = compile("fps 25\npart p { fps 100; gray(x) }\np").unwrap();
    assert_eq!(p.frame_ms(), Some(40));
    for (src, want, line, col) in [
        (
            "fps 60\ngray(x)",
            "`fps` is 25, 50 or 100 frames a second",
            1,
            5,
        ),
        ("fps 100.5\ngray(x)", "`fps` is 25, 50 or 100", 1, 5),
        (
            "fps\ngray(x)",
            "`fps` takes 25, 50 or 100 frames a second, e.g. `fps 100`",
            1,
            1,
        ),
        ("fps 100\nfps 50\ngray(x)", "`fps` is already set", 2, 1),
        (
            "fps 100 fast\ngray(x)",
            "expected the end of the line",
            1,
            9,
        ),
        (
            "layer { fps 100\ngray(x) }",
            "`fps` is the whole formula's",
            1,
            9,
        ),
    ] {
        let e = compile(src).unwrap_err();
        assert!(e.msg.contains(want), "{src:?}: {}", e.msg);
        assert_eq!(e.line_col(src), (line, col), "{src:?}: {}", e.msg);
    }
}

#[test]
fn registers_are_reused() {
    let p = compile(sketches::RING_CLOCK).unwrap();
    // (One of them holds `cx`, kept down the columns.)
    assert!(p.regs() <= 5, "{}", p.listing());
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
    ("gray(x @ 2)", "I don't understand `@`", 1, 8),
    ("gray(\"x\")", "there are no strings", 1, 6),
    ("pal(3, x)", "the first value of `pal` is a palette", 1, 5),
    (
        "pal(fire, x, y, x)",
        "`pal` takes a palette and one value (and how much",
        1,
        15,
    ),
    ("gray(loop)", "`loop` can only start a line", 1, 6),
    (
        "gray(digit(1, 2))",
        "`digit` takes 3 values (the digit, a column and a row; and a font, normal, small or bold, if you like), but got 2",
        1,
        6,
    ),
    (
        "gray(glyph(65, 1, 2, small, 3))",
        "`glyph` takes 3 values (the character, a column and a row",
        1,
        6,
    ),
    (
        "gray(digit(1, 2, 3, big))",
        "the last value of `digit` is a font: normal (the default), small or bold, found `big`",
        1,
        21,
    ),
    (
        "gray(digit(1, 2, 3, 0.5))",
        "the last value of `digit` is a font",
        1,
        21,
    ),
    (
        "gray(glyph(48, 0, 0, bold))",
        "the bold font has digits only: use `digit(n, x, y, bold)`",
        1,
        22,
    ),
    ("gray(bit(5))", "`bit` takes 2 values, but got 1", 1, 6),
    ("gray(small)", "`small` is a font: use it as `digit(n, x, y, small)`", 1, 6),
    ("gray(glyph('AB', 0, 0))", "a character goes in quotes, one at a time", 1, 12),
    ("gray(glyph('é', 0, 0))", "a character goes in quotes, one at a time", 1, 12),
    ("gray(glyph(\"A\", 0, 0))", "for a character's code write it in single quotes", 1, 12),
    ("sin = 2\ngray(sin)", "`sin` is built in", 1, 1),
    ("gray(digit)", "`digit` is a function: write `digit(…)`", 1, 6),
    ("fire = 2\ngray(fire)", "is a palette's name", 1, 1),
    (
        "gray(((((((((((((((((((((((((((x)))))))))))))))))))))))))))",
        "nested too deeply",
        1,
        24,
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
    assert!(core::mem::size_of::<Program>() <= 2876);
}

/// Names that became functions later stay usable as values in saved
/// formulas; a call is still the function.
#[test]
fn later_functions_stay_names() {
    let a = compile("glyph = x\ndigit = y\nbit = 0.5\nrgb(glyph, digit, bit)").unwrap();
    assert_eq!(a, compile("rgb(x, y, 0.5)").unwrap());
    let b = compile("glyph = 0.5\ngray(glyph('A', px % 5, 31 - py) * glyph)").unwrap();
    assert!(b.len() > 2);
}

#[test]
fn compact_compiles_the_same() {
    let extra = [
        "v = min(x,\n  y) -- c\n\n  // d\ngray(v)",
        "a1 = x +\n y\nframe {\n q = t\n}\nrgb(a1, q, - -x)",
        "k = ' ' + ';' + '-'-'/' -- c\ngray(glyph(k, px % 5, py) + digit(3, 1, 1, small))",
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
    // What the docs say (Program, the scratch at 128 and 64 lanes, the
    // draw stage's canvas).
    assert_eq!(core::mem::size_of::<Program>(), 2876);
    assert_eq!(core::mem::size_of::<crate::Ink>(), 4096);
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
        // (7,245 before fast-div, less its `len`'s saving on the spare: 848
        // → 683 ns a pixel at fastdiv-a/-b.)
        (sketches::PLASMA, 6569),
        (sketches::FIRE, 4964),
        // (11,817 before fast-div, less its `atan2`'s and `len`'s savings:
        // 147 and 165 ns a pixel.)
        (sketches::RING_CLOCK, 10539),
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
            matches!(
                name,
                "parts"
                    | "nested"
                    | "nested digit"
                    | "nested mix"
                    | "nested pick"
                    | "parts in layers"
                    | "draw"
                    | "nested sprite"
                    | "nested forever"
                    | "nested palette"
                    | "declared"
                    | "declared part"
            ) && n <= 2048
                || (1900..=2048).contains(&n),
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
    // Two loops and the draw block are three levels of the nesting cap.
    let d = crate::worst::draw();
    assert_eq!(
        d.matches("sin(").count() + 3,
        crate::worst::nested().matches("sin(").count(),
        "{d}"
    );
    // As deep as `sin(` nesting goes.
    let d = crate::worst::nested_digit();
    assert_eq!(
        d.matches("digit(").count(),
        crate::worst::nested().matches("sin(").count(),
        "{d}"
    );
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
    let lp = if lp == 0 {
        "forever".into()
    } else {
        alloc::format!("{}", lp as f64 / 1000.0)
    };
    let head = alloc::format!("loop {lp}\npart p {{\n{inner}}}\n");
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
    for k in 1..4 {
        let j = k - 1;
        s = alloc::format!("part p{k} {{\n{s}v = p{j}(x*0.5, y).v + p{j}(y, x).v + p{j}(x + 0.1, y).v + p{j}(x, y*2).v + p{j}(1 - x, y).v + p{j}(y, 1 - x).v + p{j}(x*2, y).v + p{j}(y*2, x).v\ngray(v)\n}}\n");
    }
    s += "p3";
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

// --- Centred values, pick and colours (2026-10-26) --------------------------------

/// `cx cy ty r angle` are the formulas they stand for, node for node (the
/// same program), wherever the pixel is drawn; a name set with one of them
/// is that name.
#[test]
fn centred_values_are_what_they_stand_for() {
    let same = |a: &str, b: &str| {
        let (p, q) = (compile(a), compile(b));
        assert_eq!(p, q, "{a} vs {b}");
    };
    same("gray(cx)", "gray((x - 0.5)*4)");
    same("gray(cy)", "gray(y - 0.5)");
    same("gray(ty/31)", "gray((31 - py)/31)");
    same("gray(r)", "gray(len((x - 0.5)*4, y - 0.5))");
    same("gray(angle)", "gray(atan2(y - 0.5, (x - 0.5)*4))");
    // Several: the same steps (maybe in another order), the same bytes.
    let (a, b) = (
        "part p { hsv(angle, 1, r + ty/31) }\np(1 - x, y*0.5)",
        "part p { hsv(atan2(y - 0.5, (x - 0.5)*4), 1, len((x - 0.5)*4, y - 0.5) + (31 - py)/31) }\np(1 - x, y*0.5)",
    );
    assert_eq!(compile(a).unwrap().counts(), compile(b).unwrap().counts());
    assert!(over(a, None, 0) == over(b, None, 0));
    same(
        "r = x\ncx = y\nty = 0.5\nangle = 1\ncy = 0\npick = 0.25\nrgb(r, cx, ty + angle + cy + pick)",
        "rgb(x, y, 0.5 + 1 + 0 + 0.25)",
    );
    // A part called `pick` is still the part.
    same(
        "part pick { gray(x) }\npick(1 - x, y)",
        "part q { gray(x) }\nq(1 - x, y)",
    );
}

/// `pick(k, …)` against the chain of `?:` it replaces, byte for byte; one
/// step a pixel when its choices are the same for every pixel.
#[test]
fn pick_is_the_chain_it_replaces() {
    let cases = [
        (
            "k = floor(px/32)\nframe { hh = hour/24; m = minute/60 }\ngray(pick(k, 0.1, hh, m, 1))",
            "k = floor(px/32)\nframe { hh = hour/24; m = minute/60 }\ngray(k < 0 ? 0 : k < 1 ? 0.1 : k < 2 ? hh : k < 3 ? m : k < 4 ? 1 : 0)",
        ),
        (
            "gray(pick(x*6 - 1, 0.2, 0.4, 0.6, 0.8))",
            "k = x*6 - 1\ngray(k < 0 ? 0 : k < 1 ? 0.2 : k < 2 ? 0.4 : k < 3 ? 0.6 : k < 4 ? 0.8 : 0)",
        ),
        (
            "gray(pick(px/32, x, y, 1 - x))",
            "k = px/32\ngray(k < 0 ? 0 : k < 1 ? x : k < 2 ? y : k < 3 ? 1 - x : 0)",
        ),
        (
            "gray(pick(py/8 - 0.5, 0.25, y, 0.75) + x*0.1)",
            "k = py/8 - 0.5\ngray((k < 0 ? 0 : k < 1 ? 0.25 : k < 2 ? y : k < 3 ? 0.75 : 0) + x*0.1)",
        ),
        (
            "frame { s = second % 4 }\ngray(pick(s, 0.25, 0.5, 0.75, 1) * x)",
            "frame { s = second % 4 }\ngray((s < 1 ? 0.25 : s < 2 ? 0.5 : s < 3 ? 0.75 : 1) * x)",
        ),
        (
            "u = px - 39; v = 22 - py\nch = pick(floor(u/10), 'H', 'E', 'L', 'L', 'O')\ngray(glyph(ch, floor(u/2) % 5, floor(v/2)))",
            "u = px - 39; v = 22 - py\ncell = floor(u/10)\nch = cell < 0 ? 0 : cell < 1 ? 'H' : cell < 2 ? 'E' : cell < 4 ? 'L' : cell < 5 ? 'O' : 0\ngray(glyph(ch, floor(u/2) % 5, floor(v/2)))",
        ),
    ];
    for (a, b) in cases {
        for ms in [0, 1500] {
            assert!(over(a, None, ms) == over(b, None, ms), "{a}\nvs\n{b}");
        }
    }
    // One pick step a pixel where the chain took ten.
    let px = |s: &str| compile(s).unwrap().counts().2;
    assert_eq!(px(cases[0].0) + 9, px(cases[0].1));
    assert_eq!(px(cases[5].0) + 9, px(cases[5].1));
    // In a colour expression (read three times) a pick is one step (and
    // all of it once a column: the column run's marker is one more).
    assert_eq!(
        px("#f80 * pick(px/32, 0.5, 1)"),
        5,
        "{}",
        compile("#f80 * pick(px/32, 0.5, 1)").unwrap().listing()
    );
    // Picks of the same choices share their table, and the listing shows it.
    let q =
        compile("u = px/32\nrgb(pick(u, 0.1, 0.2), pick(u + 1, 0.1, 0.2), pick(u + 2, 0.1, 0.2))")
            .unwrap();
    assert_eq!(
        q.values(),
        compile("u = px/32\nrgb(pick(u, 0.1, 0.2), 1, 2)")
            .unwrap()
            .values()
    );
    assert!(q.listing().contains("= pick(L"), "{}", q.listing());
    assert!(q.listing().contains("; 0.1"), "{}", q.listing());
    // A constant k is its choice; outside, 0.
    let p = |s: &str| compile(s).unwrap();
    assert_eq!(p("gray(pick(2.5, x, y, 0.5))"), p("gray(0.5)"));
    assert_eq!(p("gray(pick(3, x, y, 0.5))"), p("gray(0)"));
    assert_eq!(p("gray(pick(-0.5, x, y))"), p("gray(0)"));
    // Sliders read through a pick still get their sliders.
    assert_eq!(p("gray(pick(px/64, a, c))").sliders(), 0b101);
}

/// A colour expression is read once a channel: `#f80` is that channel's
/// value (a byte `b` is `b/255` exactly: `rgb` gives the byte back),
/// `rgb(r, g, b)` that value, the rest is arithmetic as usual.
#[test]
fn colours_are_their_channels() {
    let px = over("rgb(#ff8001)", None, 0);
    assert!(px.chunks(3).all(|c| c == [255, 128, 1]));
    let p = |s: &str| compile(s).unwrap_or_else(|e| panic!("{s}: {}", e.render(s)));
    assert_eq!(p("#f80"), p("rgb(#ff8800)"));
    assert_eq!(p("#F80"), p("#ff8800"));
    assert_eq!(p("#ff9933 * x"), p("rgb(1 * x, 0.6 * x, 0.2 * x)"));
    for (a, b) in [
        (
            "mix(#336699, #fff, x)",
            "rgb(mix(0.2, 1, x), mix(0.4, 1, x), mix(0.6, 1, x))",
        ),
        (
            "mix(rgb(x, y, 0), mix(#000, #fff, y), 0.5)",
            "rgb(mix(x, mix(0, 1, y), 0.5), mix(y, mix(0, 1, y), 0.5), mix(0, mix(0, 1, y), 0.5))",
        ),
        (
            "pick(px/32, #f00, #0f0, #00f) * (y > 0.5 ? #fff : #888)",
            "k = px/32\nm = y > 0.5 ? 1 : 0.533333\nrgb(pick(k, 1, 0, 0)*m, pick(k, 0, 1, 0)*m, pick(k, 0, 0, 1)*m)",
        ),
    ] {
        assert!(over(a, None, 0) == over(b, None, 0), "{a} vs {b}");
        assert_eq!(p(a).counts(), p(b).counts(), "{a} vs {b}");
    }
    // With coverage, over a picture: the same bytes as the decimals that
    // round to the same bytes.
    let u = pic();
    assert_eq!(
        over("layer { rgb(#ffbf4d, x) }", Some(&u), 0),
        over("layer { rgb(1, 0.75, 0.3, x) }", Some(&u), 0)
    );
    // Anywhere a colour line goes: a layer, a part, a part drawn elsewhere.
    for s in [
        "layer { #102040 }\nlayer multiply { mix(#fff, #000, x) }",
        "part p { mix(#000, #fff, x) }\np(1 - x, y)",
        "-- #nope\nloop 4\nk = sin(t/4)\nrgb(mix(#f00, #00f, k*0.5 + 0.5), 0.5)",
    ] {
        p(s);
    }
    assert!(p("mix(rgb(ur, ug, ub), #b08040, a)").over());
    // rgb(r, g, b) in a sum is a colour; rgb(colour) inside is that colour.
    assert!(over("rgb(x, y, 0.5) * 0.5", None, 0) == over("rgb(x*0.5, y*0.5, 0.25)", None, 0));
    assert!(over("mix(rgb(#f80), #000, x)", None, 0) == over("mix(#f80, #000, x)", None, 0));
    // Old photo's form keeps its registers: each channel reads only its value.
    assert_eq!(
        p("mix(rgb(ur, ug, ub), rgb(ul*1.1 + 0.08, ul*0.92 + 0.04, ul*0.65), a)").regs(),
        p("rgb(mix(ur, ul*1.1 + 0.08, a), mix(ug, ul*0.92 + 0.04, a), mix(ub, ul*0.65, a))").regs()
    );
}

/// The new forms' mistakes, in plain words, where they are.
#[test]
fn colour_and_pick_errors() {
    for (src, want, line, col) in [
        ("gray(#ff88)", "a colour is # and 3 or 6 hex digits", 1, 6),
        ("gray(#ggg)", "a colour is # and 3 or 6 hex digits", 1, 6),
        ("gray(x # 2)", "a colour is # and 3 or 6 hex digits", 1, 8),
        ("gray(#fff)", "a colour can't be a value", 1, 6),
        ("hsv(#f00)", "a colour can't be a value", 1, 5),
        (
            "v = mix(#000, #fff, x)\ngray(v)",
            "a colour can't be a value",
            1,
            9,
        ),
        ("mix(#000, #fff)", "`mix` takes 3 values, but got 2", 1, 1),
        (
            "rgb(1, #fff, 0)",
            "inside `rgb(r, g, b)` the three are values",
            1,
            8,
        ),
        (
            "mix(rgb(1, #fff, 0), #000, x)",
            "inside `rgb(r, g, b)` the three are values",
            1,
            12,
        ),
        (
            "mix(rgb(#f80, 0.5), #000, x)",
            "how much a colour covers goes on the whole line",
            1,
            13,
        ),
        (
            "mix(hsv(x, 1, 1), #000, x)",
            "in a sum with colours only `#…` and `rgb(r, g, b)` are colours",
            1,
            5,
        ),
        ("pal(fire, x) * #fff", "`pal(…)` can't go into a sum", 1, 14),
        ("gray(x) * #fff", "`gray(…)` can't go into a sum", 1, 9),
        (
            "x + 1 * rgb(1, 2)",
            "`,` (`rgb(r, g, b)` takes three values)",
            1,
            17,
        ),
        (
            "x + 1\ngray(#fff)",
            "only the last line can be a bare value",
            1,
            1,
        ),
        (
            "rgb(#f00, 1, 2)",
            "`rgb` of a colour takes the colour and",
            1,
            12,
        ),
        ("rgb(#f00, x\ngray(x)", "this `(` is never closed", 1, 4),
        ("#f80\ngray(x)", "the colour has to be the last line", 1, 1),
        (
            "layer { #f80 }\n#000",
            "with layers, every colour goes inside one",
            2,
            1,
        ),
        (
            "gray(pick(x))",
            "`pick` takes a number, then the values to pick from",
            1,
            6,
        ),
        ("gray(pick(x, 1, tme))", "unknown name `tme`", 1, 17),
        ("gray(pick)", "`pick` is a function", 1, 6),
        ("gray(cx(1))", "`cx` is a value, not a function", 1, 6),
        (
            "gray(angel)",
            "unknown name `angel` — did you mean `angle`?",
            1,
            6,
        ),
    ] {
        match compile(src) {
            Ok(_) => panic!("{src:?}: compiled"),
            Err(e) => assert!(
                e.msg.contains(want) && e.line_col(src) == (line, col),
                "{src:?}: got {:?} at {:?}\n{}",
                e.msg,
                e.line_col(src),
                e.render(src)
            ),
        }
    }
}

// --- the draw stage -----------------------------------------------------------

/// The draw stage's errors (E701-E710 of docs/psi-drawing-design.md §2.3;
/// the codes are not yet a field of `Error`): (source, words in the
/// sentence, line, column, the code).
const DRAW_ERRORS: &[(&str, &str, usize, usize, &str)] = &[
    ("layer { draw { dot(1, 1) }\ngray(ink) }", "goes at the top of the formula", 1, 9, "E701"),
    ("layer { table T = 1\ngray(1) }", "a table goes at the top", 1, 9, "E701"),
    ("draw { dot(1, 1) }\ndraw { dot(2, 2) }\ngray(ink)", "there can be one `draw { }`", 2, 1, "E702"),
    ("part p { draw { dot(1, 1) }\ngray(ink) }\ndraw { dot(2, 2) }\np", "there can be one `draw { }`", 3, 1, "E702"),
    ("draw { for k 12 { dot(k, 1) } }\ngray(ink)", "`for` takes a name and a count", 1, 8, "E703"),
    ("draw { for", "`for` takes a name and a count", 1, 8, "E703"),
    ("draw { for k", "`for` takes a name and a count", 1, 8, "E703"),
    ("draw { for k <", "`for` takes a name and a count", 1, 8, "E703"),
    ("draw { for k < 0 { dot(k, 1) } }\ngray(ink)", "a whole number from 1 to 1024", 1, 8, "E703"),
    ("draw { for k < 1025 { dot(k, 1) } }\ngray(ink)", "a whole number from 1 to 1024", 1, 8, "E703"),
    ("draw { for k < 2.5 { dot(k, 1) } }\ngray(ink)", "a whole number from 1 to 1024", 1, 8, "E703"),
    ("draw { for k < 1024 { dot(k, 1) }\nfor j < 1025 { dot(j, 2) } }\ngray(ink)", "a whole number", 2, 1, "E703"),
    ("draw { for k < 1024 { dot(k, 1) }\nfor j < 1024 { dot(j, 2) }\nfor q < 1 { dot(q, 3) } }\ngray(ink)", "2049 loop turns in all; the limit is 2048", 3, 1, "E704"),
    ("draw { for k < 32 { for j < 64 { dot(j, k) } } }\ngray(ink)", "2080 loop turns", 1, 21, "E704"),
    ("draw { for a1 < 2 { for b1 < 2 { for c1 < 2 { dot(a1, b1, c1) } } } }\ngray(ink)", "two deep at most", 1, 34, "E705"),
    ("line(0, 0, 9, 9)\ngray(1)", "`line(…)` draws: it goes on a line of its own inside `draw { }`", 1, 1, "E706"),
    ("draw { v = dot(1, 2) }\ngray(ink)", "`dot(…)` draws", 1, 12, "E706"),
    ("gray(ink)", "`ink` needs a `draw { }` block", 1, 6, "E706"),
    ("table T = 1, x\ngray(T[0])", "a table holds numbers only", 1, 14, "E707"),
    ("table T = 1, 2\ntable U = 3,\n4\ngray(T[0] + U[1] + V[0])", "`[ ]` reads a table", 4, 20, "E710"),
    ("draw { dot(x, 1) }\ngray(ink)", "inside draw { } only values that are the same for every pixel", 1, 12, "E708"),
    ("draw { v = py*2\ndot(v, 1) }\ngray(ink)", "but this uses `py`", 1, 12, "E708"),
    ("draw { line(0, 0, 9, ink) }\ngray(ink)", "but this uses `ink`", 1, 22, "E708"),
    ("draw { for k < 3 { k = 2\ndot(k, 1) } }\ngray(ink)", "`k` is a loop counter: it can't be set", 1, 20, "E709"),
    ("draw { for k < 3 { for k < 2 { dot(k, 1) } } }\ngray(ink)", "`k` is already a loop counter here", 1, 24, "E709"),
    ("v = 1\ngray(v[0])", "`[ ]` reads a table: `NAME[k]` — `v` is not a table", 2, 6, "E710"),
    ("table T = 1, 2\ngray(T)", "`T` is a table: read one of its numbers with `T[k]`", 2, 6, "E710"),
    ("table T = 1, 2\ngray(T[0)", "expected `]` here", 2, 9, "—"),
    ("draw { frame { v = 1 } }\ngray(ink)", "inside draw { } go `name = value`", 1, 8, "—"),
    ("draw { dot(1) }\ngray(ink)", "`dot` takes 2 values", 1, 8, "—"),
    ("draw { line(1, 2, 3, 4, 5, 6, 7) }\ngray(ink)", "`line` takes 4 values", 1, 8, "—"),
    ("for k < 3 { }\ngray(1)", "`for` goes inside `draw { }`", 1, 1, "—"),
    ("draw { dot(1, 1)\ngray(ink)", "this `draw {` is never closed", 1, 1, "—"),
    ("table T = 1\ntable T = 2\ngray(T[0])", "`T` is already set on line 1", 2, 7, "—"),
];

#[test]
fn draw_error_messages() {
    let mut bad = Vec::new();
    for &(src, want, line, col, code) in DRAW_ERRORS {
        match compile(src) {
            Ok(_) => bad.push(format!("{code} {src:?}: compiled")),
            Err(e) => {
                let lc = e.line_col(src);
                if !e.msg.contains(want) || lc != (line, col) {
                    bad.push(format!(
                        "{code} {src:?}: got {:?} at {lc:?}, want {want:?} at {:?}\n{}",
                        e.msg,
                        (line, col),
                        e.render(src)
                    ));
                }
            }
        }
    }
    let big: alloc::vec::Vec<alloc::string::String> =
        (0..257).map(|k| alloc::format!("{k}")).collect();
    let e = compile(&alloc::format!("table T = {}\ngray(T[0])", big.join(", "))).unwrap_err();
    if !e.msg.contains("up to 256 in all") {
        bad.push(alloc::format!("E707 257 numbers: {}", e.msg));
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

/// Words that became part of the draw stage stay names a formula may set:
/// `line`, `dot` and `ink` (a set name is that name).
#[test]
fn drawing_words_stay_names() {
    let a = compile("line = x\ndot = y\nink = 0.5\nrgb(line, dot, ink)").unwrap();
    assert_eq!(a, compile("rgb(x, y, 0.5)").unwrap());
    let b = compile("line = 3\ndraw { line(0, 0, line, 9) }\ngray(ink)").unwrap();
    assert!(b.draws());
}

/// A value inside a loop that doesn't depend on its counter runs once a
/// frame; one that depends on the outer counter only, once a turn of the
/// outer loop; the canvas isn't drawn when nothing reads `ink`.
#[test]
fn draw_values_run_as_rarely_as_they_can() {
    let p = compile(
        "loop 4\ndraw {\n  for s < 3 {\n    q = sin(t/4)*10\n    u = s*q\n    for r < 4 { dot(u + r, r*2 + q, s/3) }\n  }\n}\ngray(ink)",
    )
    .unwrap();
    let l = p.listing();
    // sin(t/4)·10 per frame; s·q in the outer body; r+u, r·2+q, in the inner.
    assert_eq!(p.counts().0, 3, "{l}");
    let draw: alloc::vec::Vec<&str> = l.lines().filter(|s| s.contains("draw")).collect();
    let pos = |w: &str| draw.iter().position(|s| s.contains(w)).unwrap_or(99);
    assert!(
        pos("= for(") < pos("= mul(")
            && pos("= mul(") < draw.iter().rposition(|s| s.contains("= for(")).unwrap(),
        "{l}"
    );
    assert_eq!(p.primitives(), 12, "{l}");
    assert!(p.inputs() & 8 != 0);
    // A draw block nothing shows is left out.
    let q = compile("draw { dot(1, 1) }\ngray(x)").unwrap();
    assert!(!q.draws());
    assert_eq!(q, compile("gray(x)").unwrap());
    // Two sibling loops may use the same counter's name and values' names.
    let r = compile("draw {\n for k < 2 { v = k*3\n dot(v, 1) }\n for k < 3 { v = k\n dot(v, 2) }\n}\ngray(ink)").unwrap();
    assert_eq!(r.primitives(), 5);
}

/// Tables: a constant index is folded; per frame, per row, per pixel and
/// in a loop, entry floor(k), 0 outside; the same numbers stored once.
#[test]
fn tables_read_floor_k() {
    let c = compile("table T = 1, 0.5, 0.25\ngray(T[1.9] + T[3] + T[-0.5])").unwrap();
    assert_eq!(c.listing(), compile("gray(0.5)").unwrap().listing());
    let p = compile("table T = 0.1, 0.2, 0.3, 0.4\ntable U = 0.2, 0.3\nrgb(T[px % 5], U[py % 3], T[second % 5])").unwrap();
    // Two tables, the second inside the first: 4 numbers held.
    assert_eq!(&p.tables[..5], &[q(0.1), q(0.2), q(0.3), q(0.4), 0]);
    let mut out = [0u8; W * H * 3];
    let inputs = Inputs {
        second: 3,
        ..Default::default()
    };
    render(&p, &mut Scratch::new(), &inputs, &OWN, &mut out);
    for (k, px) in out.chunks(3).enumerate() {
        let (col, row) = (k % W, k / W);
        let py = H - 1 - row;
        let t = [0.1, 0.2, 0.3, 0.4, 0.0][col % 5];
        let u = [0.2, 0.3, 0.0][py % 3];
        assert_eq!(
            px,
            [fixed::chan(q(t)), fixed::chan(q(u)), fixed::chan(q(0.4))],
            "{col}, {row}"
        );
    }
}

/// The canvas: lines' ends lit, as many pixels as the longer side (+1),
/// each step one pixel on; ink from v0 to v1; max where they cross; dots
/// at the nearest pixel; `y` up from the bottom.
#[test]
fn lines_and_dots_land_where_they_should() {
    let ink_of = |src: &str| {
        let p = compile(src).unwrap_or_else(|e| panic!("{}", e.render(src)));
        let mut ink = Ink::boxed();
        let mut out = [0u8; W * H * 3];
        render_rows_ink::<128>(
            &p,
            &mut Scratch::new(),
            Some(&mut ink),
            &Inputs::default(),
            &OWN,
            None,
            u32::MAX,
            &mut out,
        );
        // gray(ink) draws the byte back.
        for (k, px) in out.chunks(3).enumerate() {
            assert_eq!(px[0], ink.0[k]);
        }
        ink
    };
    let at = |ink: &Ink, x: i32, y: i32| ink.at(x as usize, H - 1 - y as usize);
    let mut r = 0x1234_5678u32;
    for _ in 0..300 {
        let mut n = |m: u32| {
            r ^= r << 13;
            r ^= r >> 17;
            r ^= r << 5;
            (r % m) as i32
        };
        let (x0, y0, x1, y1) = (n(128), n(32), n(128), n(32));
        let ink = ink_of(&alloc::format!(
            "draw {{ line({x0}, {y0}, {x1}, {y1}, 1, 0.2) }}\ngray(ink)"
        ));
        assert_eq!(at(&ink, x0, y0), 255);
        assert_eq!(at(&ink, x1, y1), 51, "{x0},{y0} {x1},{y1}");
        let lit = ink.0.iter().filter(|&&b| b > 0).count() as i32;
        assert_eq!(
            lit,
            (x1 - x0).abs().max((y1 - y0).abs()) + 1,
            "{x0},{y0} {x1},{y1}"
        );
        // Every lit pixel touches another (8-connected), but for a dot.
        if lit > 1 {
            for y in 0..H as i32 {
                for x in 0..W as i32 {
                    if at(&ink, x, y) > 0 {
                        let near = (-1..=1)
                            .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                            .any(|(dx, dy)| {
                                (dx, dy) != (0, 0)
                                    && (0..W as i32).contains(&(x + dx))
                                    && (0..H as i32).contains(&(y + dy))
                                    && at(&ink, x + dx, y + dy) > 0
                            });
                        assert!(near, "{x0},{y0} {x1},{y1}: {x},{y} alone");
                    }
                }
            }
        }
    }
    let ink = ink_of("draw { line(0, 0, 127, 31, 0.3); line(0, 31, 127, 0, 0.8); dot(5.5, 2.49); dot(-0.5, 0, 0.5) }\ngray(ink)");
    assert_eq!(at(&ink, 1, 0), fixed::chan(q(0.3)));
    assert_eq!(at(&ink, 0, 31), fixed::chan(q(0.8)));
    // y up: (6, 2) is row 29 from the top.
    assert_eq!(ink.at(6, 29), 255);
    // -0.5 rounds up to 0, onto (0, 0), lighter than the line there.
    assert_eq!(at(&ink, 0, 0), fixed::chan(q(0.5)));
}

/// Over the caps the VM stops the same way everywhere, and a program that
/// draws without a canvas draws nothing.
#[test]
fn draw_caps_and_no_canvas() {
    // Each line passes the panel's corner by (812 steps, none shown).
    let src = "draw { for k < 1024 { line(-512, 300, 300, -512); dot(k % 128, 0) }\nfor j < 1024 { dot(j % 128, 31) } }\ngray(ink)";
    let p = compile(src).unwrap();
    assert_eq!(p.primitives(), 3072);
    let mut ink = Ink::boxed();
    let mut out = [0u8; W * H * 3];
    render_rows_ink::<128>(
        &p,
        &mut Scratch::new(),
        Some(&mut ink),
        &Inputs::default(),
        &OWN,
        None,
        u32::MAX,
        &mut out,
    );
    // 21 lines use the frame's 16,384 steps: the dots of turns 0..19
    // along the bottom, none after, none at the top.
    let bottom = (0..W).filter(|&x| ink.at(x, H - 1) > 0).count();
    assert_eq!(bottom, 20);
    assert!((0..W).all(|x| ink.at(x, 0) == 0));
    let mut out2 = [1u8; W * H * 3];
    render(&p, &mut Scratch::new(), &Inputs::default(), &OWN, &mut out2);
    assert!(out2.iter().all(|&b| b == 0));
}

/// The estimate counts a loop's body as often as it runs (a turn at a
/// time), or a loop run as lanes once a lane of 128 turns (its lines and
/// dots each turn).
#[test]
fn draw_estimate_scales_with_turns() {
    let e = |n: u32| {
        let p = compile(&alloc::format!(
            "draw {{ for k < {n} {{ dot(hash(k, 1)*128, hash(k, 2)*32) }} }}\ngray(ink)"
        ))
        .unwrap();
        (p.draw_us(), p.lane_loops())
    };
    let (a, b) = (e(3), e(30));
    assert!(
        a.1 == 0 && b.1 == 0 && a.0 > 10 && b.0 > 9 * (a.0 - 10),
        "{a:?} {b:?}"
    );
    let (a, b) = (e(128), e(1024));
    assert!(
        a.1 == 1 && b.1 == 1 && a.0 > 10 && b.0 > 7 * (a.0 - 10),
        "{a:?} {b:?}"
    );
    // As lanes, a big loop is cheaper than a turn at a time.
    let p =
        compile("draw { for k < 1024 { dot(hash(k, 1)*128, hash(k, 2)*32) } }\ngray(ink)").unwrap();
    assert!(
        p.esp32_us() < p.esp32_us_turns(),
        "{} {}",
        p.esp32_us(),
        p.esp32_us_turns()
    );
    // A constant line's length is known: 100 px costs more than 12.
    let l = |x1: u32| {
        compile(&alloc::format!("draw {{ line(0, 0, {x1}, 0) }}\ngray(ink)"))
            .unwrap()
            .draw_us()
    };
    assert!(l(100) > l(11));
}

/// The draw stage runs on a full render only: a render of some rows reads
/// the canvas the last full one left (a look redrawing the rows its GIF
/// changed doesn't draw its lines again).
#[test]
fn only_a_full_render_draws() {
    let p = compile("draw { for k < 8 { line(k*16, 0, k*16 + 8, 31) } }\ngray(ink)").unwrap();
    let mut ink = Ink::boxed();
    let mut s = Scratch::<128>::new();
    let mut full = [0u8; W * H * 3];
    let i = Inputs::default();
    render_rows_ink::<128>(
        &p,
        &mut s,
        Some(&mut ink),
        &i,
        &OWN,
        None,
        u32::MAX,
        &mut full,
    );
    let drawn = ink.0;
    assert!(drawn.iter().any(|&b| b > 0));
    // Rows 3 and 7 alone: the same bytes, the canvas untouched.
    let mut part = [0u8; W * H * 3];
    render_rows_ink::<128>(
        &p,
        &mut s,
        Some(&mut ink),
        &i,
        &OWN,
        None,
        (1 << 3) | (1 << 7),
        &mut part,
    );
    assert_eq!(ink.0, drawn);
    for r in [3, 7] {
        assert_eq!(
            part[r * W * 3..(r + 1) * W * 3],
            full[r * W * 3..(r + 1) * W * 3]
        );
    }
    // Proof it isn't drawn again: a canvas changed by hand is what shows.
    ink.0.fill(200);
    render_rows_ink::<128>(&p, &mut s, Some(&mut ink), &i, &OWN, None, 1, &mut part);
    assert!(part[..W * 3].iter().all(|&b| b == 200));
    render_rows_ink::<128>(
        &p,
        &mut s,
        Some(&mut ink),
        &i,
        &OWN,
        None,
        u32::MAX,
        &mut part,
    );
    assert_eq!(ink.0, drawn);
}

#[test]
fn mul_and_the_narrowed_steps_are_exact() {
    // `mul` is sat(a·b >> 16) (floor), and each narrowed step is its
    // checked step wherever its range condition holds: random values and
    // the edges.
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let edges = [
        0,
        1,
        -1,
        ONE,
        -ONE,
        i32::MAX,
        i32::MIN,
        i32::MAX - 1,
        i32::MIN + 1,
        1 << 23,
        -(1 << 23),
        181 << 16,
        -(181 << 16),
        46_340,
        -46_341,
    ];
    for k in 0..400_000 {
        let (a, b) = if k < edges.len() * edges.len() {
            (edges[k % edges.len()], edges[k / edges.len()])
        } else {
            let r = next();
            let sh = (r >> 59) as u32; // spread the magnitudes
            ((r as i32) >> sh, ((r >> 32) as i32) >> (sh / 2))
        };
        let p = a as i64 * b as i64;
        assert_eq!(fixed::mul(a, b), fixed::sat(p >> 16), "{a} {b}");
        if (p >> 16) == ((p >> 16) as i32) as i64 {
            assert_eq!(fixed::mul_w(a, b), fixed::mul(a, b), "{a} {b}");
        }
        if p == (p as i32) as i64 {
            assert_eq!(fixed::mul_n(a, b), fixed::mul(a, b), "{a} {b}");
        }
        if (a as i64 + b as i64) == (a.wrapping_add(b)) as i64 {
            assert_eq!(fixed::add_w(a, b), fixed::add(a, b));
        }
        if (a as i64 - b as i64) == (a.wrapping_sub(b)) as i64 {
            assert_eq!(fixed::sub_w(a, b), fixed::sub(a, b));
        }
        for n in [0, 1, 3, 4, 11, 16] {
            assert_eq!(fixed::mul(a, ONE >> n), a >> n, "{a} >> {n}");
        }
    }
}

#[test]
fn the_sine_table_is_within_one() {
    // `range` takes sin and cos to be within ±1: the table's entries within
    // ±16,384 (interpolated between two, then × 4).
    assert!(crate::tables::SIN
        .iter()
        .all(|&v| (v as i32).abs() <= 16_384));
}

// --- loop forever: wide values ------------------------------------------------

/// Each wide operation is its Q16.16 operation wherever the values fit one
/// (and past that carries on, exactly, instead of saturating).
#[test]
fn wide_maths_is_the_q16_maths_where_it_fits() {
    let mut r: u64 = 0x5eed_0ff0_e7e7;
    let mut next = || {
        r ^= r >> 12;
        r ^= r << 25;
        r ^= r >> 27;
        r.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    let mut checked = 0;
    for k in 0..200_000 {
        // Values across the range, small ones and edges more often.
        let pick = |v: u64| -> i32 {
            match v % 5 {
                0 => (v >> 8) as i32,
                1 => ((v >> 8) as i32) >> 12,
                2 => ((v >> 8) as i32) >> 20,
                3 => [0, ONE, -ONE, i32::MAX, i32::MIN, 1, -1, ONE / 2][(v >> 8) as usize % 8],
                _ => ((v >> 8) as i32) >> 4,
            }
        };
        let (a, b) = (pick(next()), pick(next()));
        let w = a as i64;
        let m = fixed::mul(a, b);
        if m != i32::MAX && m != i32::MIN {
            assert_eq!(fixed::w_mul(w, b), m as i64, "mul {a} {b}");
            checked += 1;
        }
        let d = fixed::div(a, b);
        if d != i32::MAX && d != i32::MIN && b != 0 {
            assert_eq!(fixed::w_div(w, b), d as i64, "div {a} {b}");
        }
        assert_eq!(fixed::w_mod(w, b), fixed::modulo(a, b), "mod {a} {b}");
        assert_eq!(fixed::w_frac(w), fixed::frac(a));
        assert_eq!(fixed::w_floor(w), fixed::floor(a) as i64);
        assert_eq!(fixed::w_reduce(w), a, "a value that fits hashes as itself");
        for c in [code::LT, code::LE, code::GT, code::GE, code::EQ, code::NE] {
            let want = crate::program::eval(c, a, b, 0, None);
            assert_eq!(fixed::w_cmp(c, w, b), want);
        }
        assert_eq!(fixed::w_minmax(code::MIN, w, b), fixed::min(a, b));
        assert_eq!(fixed::w_minmax(code::MAX, w, b), fixed::max(a, b));
        // noise of a wide value split into its whole part (the offset) and
        // fraction + a small part: the noise of the sum.
        if k % 8 == 0 {
            let s = pick(next()) >> 8;
            let y = pick(next()) >> 6;
            if let Some(sum) = a.checked_add(s) {
                let off = [fixed::w_lat(w, 0), 0, 0];
                let small = fixed::frac(a) + s;
                assert_eq!(
                    fixed::noise_off(small, y, 0, off),
                    fixed::noise(sum, y, 0),
                    "noise of {a} + {s}"
                );
            }
        }
    }
    assert!(checked > 50_000);
    // Past the Q16.16 range: exact, not saturated.
    let big = 3_000_000i64 << 16; // 3,000,000
    assert_eq!(fixed::w_mul(big, 10 << 16), 30_000_000i64 << 16);
    assert_eq!(fixed::w_div(big, 3 << 16), 1_000_000i64 << 16);
    assert_eq!(
        fixed::w_frac(fixed::w_mul(big + ONE as i64 / 4, 10 << 16)),
        ONE / 2
    );
    assert_eq!(
        fixed::w_mod(big + 5 * ONE as i64, 7 << 16),
        (3_000_005 % 7) << 16
    );
    assert_eq!(
        fixed::w_mod(-big, 7 << 16),
        ((-3_000_000i64).rem_euclid(7) as i32) << 16
    );
    assert_eq!(fixed::w_minmax(code::MIN, big, ONE), ONE);
    assert_eq!(fixed::w_minmax(code::MAX, big, ONE), i32::MAX);
    assert_eq!(fixed::w_cmp(code::GT, big, i32::MAX), ONE);
    // Hashes past 65,536 s don't come round again.
    assert_ne!(
        fixed::w_reduce(big),
        fixed::w_reduce(big + (65_536i64 << 16))
    );
    assert_ne!(fixed::w_reduce(big), (big as u32) as i32);
    // w_mul's whole part wraps at 2^47 (millions of years of `t`), its
    // fraction exact whatever the size.
    let huge = 1i64 << 60;
    assert_eq!(fixed::w_frac(fixed::w_mul(huge + 3, 3 << 16)), 9);
}

/// `loop forever`: no loop, `t` wide (frame steps only), `seed` in its
/// slot; the pixel steps those values reach are the usual ones.
#[test]
fn loop_forever_programs() {
    let p = compile("loop forever\ngray(frac(t*0.15))").unwrap();
    assert!(p.forever());
    assert_eq!(p.loop_ms(), 0);
    assert_eq!(p.inputs() & 8, 8, "it reads t");
    let l = p.listing();
    assert!(l.contains("w_muls(w0"), "{l}");
    assert!(l.contains("w_frac("), "{l}");
    // A still one (it never reads t).
    let s = compile("loop forever\ngray(x)").unwrap();
    assert!(s.forever());
    assert_eq!(s.inputs() & 8, 0);
    // seed: its slot, and the inputs' bit 6.
    let p = compile("loop forever\ngray(frac(seed*100 + t))").unwrap();
    assert_eq!(p.inputs() & 64, 64);
    assert!(p.listing().contains("mul(seed, 100)"), "{}", p.listing());
    // A formula that sets its own `seed` keeps it, looped or not.
    let a = compile("seed = 0.5\ngray(seed*x)").unwrap();
    assert_eq!(a, compile("gray(0.5*x)").unwrap());
    // `forever` is still a name a formula may set.
    compile("forever = 2\ngray(x*forever)").unwrap();
    // noise of t once a frame: one step.
    let p = compile("loop forever\ngray(noise(t*0.5, 5, 0) + x)").unwrap();
    assert!(
        p.listing().contains("w_noisex(w1, 5, 0)"),
        "{}",
        p.listing()
    );
    // noise of t: a wide noise per pixel and its offsets once a frame.
    let p = compile("loop forever\ngray(noise(x*8 + t, y*4, t*0.3))").unwrap();
    let l = p.listing();
    assert!(
        l.contains("noise_o0(") && l.contains("noise0.x = w_lat") && l.contains("noise0.z = w_lat"),
        "{l}"
    );
    assert!(!l.contains("noise0.y"), "{l}");
    // A part with its own loop inside one that never repeats: t % its loop.
    let p = compile("loop forever\npart p {\n  loop 4\n  gray(t/4)\n}\np").unwrap();
    assert!(p.listing().contains("w_mod(w0, 4)"), "{}", p.listing());
    // One that never repeats inside a looped formula: the host's t.
    let p = compile("loop 4\npart p {\n  loop forever\n  gray(frac(t))\n}\np").unwrap();
    assert!(
        !p.forever() && !p.listing().contains("w_"),
        "{}",
        p.listing()
    );
}

/// The wide steps' errors (E901-E906; `Error.code` is not a field yet).
const FOREVER_ERRORS: &[(&str, &str, usize, usize, &str)] = &[
    ("loop forever\ngray(t*x)", "grows without end", 2, 7, "E901"),
    ("loop forever\ngray(sqrt(t))", "grows without end", 2, 6, "E901"),
    ("loop forever\ngray(frac(t*t))", "grows without end", 2, 12, "E901"),
    ("loop forever\ngray(floor(t + x))", "grows without end", 2, 6, "E901"),
    ("loop forever\nv = t + px\ngray(v < 3)", "grows without end", 3, 8, "E901"),
    ("loop forever\ntable T = 1, 2\ngray(T[t])", "grows without end", 3, 6, "E901"),
    ("loop forever\ngray(pick(px/32, t, 1))", "grows without end", 2, 6, "E901"),
    ("loop forever\ngray(x / t)", "grows without end", 2, 8, "E901"),
    ("loop forever\ngray(frac((t + x)/3))", "add the pixel's part there", 2, 18, "E901"),
    ("loop forever\ngray((t + x) > 5)", "add the pixel's part there", 2, 14, "E901"),
    ("loop forever\nv = t + x\ngray(v)", "a colour can't take a value that grows", 3, 1, "E905"),
    ("loop forever\nrgb(1, 1, 1, t + x)", "a colour can't take a value that grows", 2, 1, "E905"),
    ("loop forever\npal(fire, x) * t", "a colour can't take a value that grows", 2, 1, "E905"),
    ("loop forever\ngray(noise(t + x, 0, 0) + noise(t*2 + x, 0, 0) + noise(t*3 + x, 0, 0) + noise(t*4 + x, 0, 0) + noise(t*5 + x, 0, 0))", "four different ones at most", 2, 96, "E902"),
    ("gray(seed)", "`seed` is for formulas that go on without repeating", 1, 6, "E903"),
    ("loop forever\ndraw { dot(t, 3) }\ngray(ink)", "can't be drawn at", 2, 12, "E906"),
    ("loop never\ngray(x)", "or `forever` for one that goes on without repeating", 1, 6, "—"),
];

#[test]
fn forever_error_messages() {
    let mut bad = Vec::new();
    for &(src, want, line, col, code) in FOREVER_ERRORS {
        match compile(src) {
            Ok(_) => bad.push(format!("{code} {src:?}: compiled")),
            Err(e) => {
                let lc = e.line_col(src);
                if !e.msg.contains(want) || lc != (line, col) {
                    bad.push(format!(
                        "{code} {src:?}: got {:?} at {lc:?}, want {want:?} at {:?}",
                        e.msg,
                        (line, col)
                    ));
                }
            }
        }
    }
    // Sixteen wide values: one more than the slots.
    let many: String = (1..=16).map(|k| format!("w{k} = t*{k}.5\n")).collect();
    let sum: String = (1..=16)
        .map(|k| format!("frac(w{k})"))
        .collect::<Vec<_>>()
        .join(" + ");
    let e = compile(&format!("loop forever\n{many}gray({sum})")).unwrap_err();
    if !e.msg.contains("too many values that grow without end") {
        bad.push(format!("E904: {}", e.msg));
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

// --- sprites ------------------------------------------------------------------

/// A frame of `src` at the golden inputs (no canvas, nothing under).
fn spr_frame(src: &str) -> alloc::vec::Vec<u8> {
    let p = compile(src).unwrap_or_else(|e| panic!("{}", e.render(src)));
    let mut out = alloc::vec![0u8; W * H * 3];
    render(&p, &mut Scratch::new(), &golden::inputs(0), &OWN, &mut out);
    out
}

/// `NAME[u, v]` reads the key's number at column floor(u), row floor(v)
/// from the top left, 0 outside; at 1, 2 and 4 bits a pixel, across word
/// boundaries, once a frame, a row or a pixel.
#[test]
fn sprites_read_their_keys() {
    // A random picture per key size and width: each pixel against the
    // characters it was written with.
    let mut r: u64 = 0x5b1e_c0de_1234_5678;
    let mut next = |n: usize| {
        r ^= r >> 12;
        r ^= r << 25;
        r ^= r >> 27;
        (r.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33) as usize % n
    };
    let alphabet = b".abcdefghijklmnopqrstuvwxyz0123456789#*";
    for &(keys, w, h) in &[
        (1, 7, 3),
        (2, 33, 5),
        (3, 17, 2),
        (4, 64, 4),
        (5, 9, 9),
        (16, 31, 32),
        (16, 64, 3),
        (9, 1, 1),
    ] {
        let chars = &alphabet[..keys];
        let vals: alloc::vec::Vec<i32> = (0..keys)
            .map(|k| (k as i32 * 9_973 + 77) % 65_536)
            .collect();
        let key: alloc::vec::Vec<alloc::string::String> = (0..keys)
            .map(|k| alloc::format!("{} {}", chars[k] as char, vals[k] as f64 / 65_536.0))
            .collect();
        let rows: alloc::vec::Vec<alloc::vec::Vec<u8>> = (0..h)
            .map(|_| (0..w).map(|_| chars[next(keys)]).collect())
            .collect();
        let src = alloc::format!(
            "sprite pic {{\n  key {}\n{}}}\ngray(pic[px - 3, ty - 2])",
            key.join(", "),
            rows.iter()
                .map(|r| alloc::format!("  {}\n", core::str::from_utf8(r).unwrap()))
                .collect::<alloc::string::String>()
        );
        let f = spr_frame(&src);
        // The same values as a table read, pixel by pixel.
        for py in 0..H {
            let ty = py as i32;
            for px in 0..W {
                let (u, v) = (px as i32 - 3, ty - 2);
                let want = if u < 0 || v < 0 || u >= w || v >= h {
                    0
                } else {
                    let c = rows[v as usize][u as usize];
                    vals[chars.iter().position(|&k| k == c).unwrap()]
                };
                let got = f[(py * W + px) * 3];
                assert_eq!(got, fixed::chan(want), "{keys} keys, {w} x {h}: ({u}, {v})");
            }
        }
    }
    // Once a frame (constant), once a row, floors of fractions, negatives.
    let src = "sprite s {\n  key . 0, a 0.25, b 0.5, c 1\n  abc.\n  .cba\n}\nframe { k = s[2, 0] }\nrgb(s[1.9, 1.99] + k*0, s[px - 1, ty*0], s[px*0.5 - 0.5, 1])";
    let f = spr_frame(src);
    let at = |px: usize, ty: usize, c: usize| f[(ty * W + px) * 3 + c];
    assert_eq!(at(0, 0, 0), 255, "s[1, 1] is c");
    assert_eq!(
        (
            at(1, 5, 1),
            at(2, 5, 1),
            at(3, 5, 1),
            at(4, 5, 1),
            at(0, 5, 1)
        ),
        (fixed::chan(q(0.25)), fixed::chan(q(0.5)), 255, 0, 0)
    );
    // px*0.5 - 0.5 at px 0 is -0.5: floored, -1: outside.
    assert_eq!((at(0, 9, 2), at(1, 9, 2), at(3, 9, 2)), (0, 0, 255));
    let p = compile(src).unwrap();
    let l = p.listing();
    assert!(l.contains("frame") && l.contains("spr("), "{l}");
    assert!(l.contains("sprite 4 x 2 at 0"), "{l}");
    // Two sprites alike are stored once (and the same read of them is
    // one step).
    let a = compile("sprite s {\nkey . 0, X 1\n.X.\n}\nsprite u {\nkey . 0, X 1\n.X.\n}\ngray(s[px, ty] + u[px, ty])").unwrap();
    assert!(a.listing().matches("spr(").count() == 1, "{}", a.listing());
}

/// The source sent to the panel (`compact`: comments out, lines joined by
/// `; `) is the same sprite.
#[test]
fn a_sprite_compacts() {
    let src = "-- a car\nsprite car {   -- its key first\n  key . 0, R 0.5, # 1   -- R red, # dark\n  ..RR..\n  .R##R.   -- the glass\n\n  RRRRRR\n}\ngray(car[px - 61, ty - 12])";
    let c = compact(src);
    assert_eq!(
        c,
        "sprite car {; key . 0, R 0.5, # 1; ..RR..; .R##R.; RRRRRR; }; gray(car[px - 61, ty - 12])"
    );
    assert_eq!(compile(src).unwrap(), compile(&c).unwrap());
    // `sprite` and its words stay names a formula may set.
    compile("sprite = 3\nkey = 2\ngray(sprite*key)").unwrap();
}

/// The sprite's errors (E911-E919; `Error.code` is not a field yet).
const SPRITE_ERRORS: &[(&str, &str, usize, usize, &str)] = &[
    (
        "layer {\nsprite s {\nkey . 0\n.\n}\ngray(1)\n}",
        "a sprite goes at the top",
        2,
        1,
        "E911",
    ),
    (
        "sprite s {\n}\ngray(s[0, 0])",
        "a sprite starts with its key",
        1,
        1,
        "E912",
    ),
    (
        "sprite s {\n.X.\n}\ngray(s[0, 0])",
        "a sprite starts with its key",
        2,
        1,
        "E912",
    ),
    (
        "sprite s {\nkey .0, X 1\n.X.\n}\ngray(s[0, 0])",
        "a key is a character and its number",
        2,
        5,
        "E913",
    ),
    (
        "sprite s {\nkey . 0, X one\n.X.\n}\ngray(s[0, 0])",
        "a key is a character and its number",
        2,
        10,
        "E913",
    ),
    (
        "sprite s {\nkey . 0, - 1\n.X.\n}\ngray(s[0, 0])",
        "a key is a character and its number",
        2,
        10,
        "E913",
    ),
    (
        "sprite s {\nkey . 0, . 1\n...\n}\ngray(s[0, 0])",
        "`.` is in the key twice",
        2,
        10,
        "E913",
    ),
    (
        "sprite s {\nkey . 0, X 1\n.Y.\n}\ngray(s[0, 0])",
        "`Y` is not in the sprite's key",
        3,
        2,
        "E914",
    ),
    (
        "sprite s {\nkey . 0, X 1\n.★\n}\ngray(s[0, 0])",
        "`★` is not in the sprite's key",
        3,
        2,
        "E914",
    ),
    (
        "sprite s {\nkey . 0, X 1\n.X.\n.X\n}\ngray(s[0, 0])",
        "as wide as its first (3 characters)",
        4,
        1,
        "E915",
    ),
    (
        "sprite s {\nkey . 0, X 1\n.X.\n}\ngray(s[0])",
        "a sprite is read at a column and a row",
        5,
        9,
        "E918",
    ),
    (
        "sprite s {\nkey . 0, X 1\n.X.\n}\ngray(s)",
        "`s` is a sprite: read its pixels with `s[u, v]`",
        5,
        6,
        "E918",
    ),
    (
        "sprite s {\nkey . 0, X 1\n.X.\ngray(s[0, 0])",
        "this `sprite {` is never closed",
        1,
        1,
        "E919",
    ),
    (
        "sprite s {\nkey . 0, X 1\n.X.\n}\nsprite s {\nkey . 0\n.\n}\ngray(1)",
        "`s` is already set on line 1",
        5,
        8,
        "—",
    ),
    (
        "loop forever\nsprite s {\nkey . 0, X 1\n.X.\n}\ngray(s[t, 0])",
        "grows without end",
        6,
        6,
        "E901",
    ),
];

#[test]
fn sprite_error_messages() {
    let mut bad = alloc::vec::Vec::new();
    for &(src, want, line, col, code) in SPRITE_ERRORS {
        match compile(src) {
            Ok(_) => bad.push(alloc::format!("{code} {src:?}: compiled")),
            Err(e) => {
                let lc = e.line_col(src);
                if !e.msg.contains(want) || lc != (line, col) {
                    bad.push(alloc::format!(
                        "{code} {src:?}: got {:?} at {lc:?}, want {want:?} at {:?}",
                        e.msg,
                        (line, col)
                    ));
                }
            }
        }
    }
    // 33 rows; 65 columns; more than the tables hold.
    let tall: alloc::string::String = (0..33).map(|_| ".X\n").collect();
    let e = compile(&alloc::format!(
        "sprite s {{\nkey . 0, X 1\n{tall}}}\ngray(s[0, 0])"
    ))
    .unwrap_err();
    if !e.msg.contains("1 to 32 rows") {
        bad.push(alloc::format!("E916 33 rows: {}", e.msg));
    }
    let wide = ".".repeat(65);
    let e = compile(&alloc::format!(
        "sprite s {{\nkey . 0\n{wide}\n}}\ngray(s[0, 0])"
    ))
    .unwrap_err();
    if !e.msg.contains("at most 64") {
        bad.push(alloc::format!("E915 65 wide: {}", e.msg));
    }
    let big: alloc::string::String = (0..32)
        .map(|k| {
            alloc::format!(
                "{}\n",
                "abcdefghijklmnop"
                    .repeat(4)
                    .chars()
                    .cycle()
                    .skip(k)
                    .take(64)
                    .collect::<alloc::string::String>()
            )
        })
        .collect();
    let keys: alloc::vec::Vec<alloc::string::String> = "abcdefghijklmnop"
        .chars()
        .enumerate()
        .map(|(k, c)| alloc::format!("{c} {k}"))
        .collect();
    let e = compile(&alloc::format!(
        "table T = 1, 2\nsprite s {{\nkey {}\n{big}}}\ngray(s[0, 0] + T[0])",
        keys.join(", ")
    ))
    .unwrap_err();
    if !e
        .msg
        .contains("tables, sprites and palettes hold 256 numbers in all")
    {
        bad.push(alloc::format!("E917: {}", e.msg));
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

// --- palettes of a formula's own -------------------------------------------------

/// `palette NAME = #…, …`: `pal(NAME, i/(n − 1))` is colour i exactly, for
/// every n from 1 to 32; scaled and covered like the built-ins.
#[test]
fn own_palettes_hit_their_colours() {
    let mut r: u64 = 0x9a1e_77e5_0000_0001;
    let mut next = || {
        r ^= r >> 12;
        r ^= r << 25;
        r ^= r >> 27;
        (r.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as u32 & 0xff_ffff
    };
    for n in 1..=32usize {
        let cols: alloc::vec::Vec<u32> = (0..n).map(|_| next()).collect();
        let list: alloc::vec::Vec<alloc::string::String> =
            cols.iter().map(|c| alloc::format!("#{c:06x}")).collect();
        // Column i shows colour i at k = i/(n - 1) (0 for n = 1).
        let k = if n == 1 {
            "0".into()
        } else {
            alloc::format!("min(px, {})/{}", n - 1, n - 1)
        };
        let src = alloc::format!("palette mine = {}\npal(mine, {k})", list.join(",\n  "));
        let f = spr_frame(&src);
        for (i, &c) in cols.iter().enumerate() {
            let p = &f[i * 3..i * 3 + 3];
            assert_eq!(
                p,
                [(c >> 16) as u8, (c >> 8) as u8, c as u8],
                "{n} colours: colour {i}\n{src}"
            );
        }
        // The ends: below 0 the first, past 1 the last.
        let g = spr_frame(&alloc::format!(
            "palette mine = {}\npal(mine, x*3 - 1)",
            list.join(", ")
        ));
        assert_eq!(
            &g[..3],
            &[(cols[0] >> 16) as u8, (cols[0] >> 8) as u8, cols[0] as u8]
        );
        let l = cols[n - 1];
        assert_eq!(
            &g[127 * 3..128 * 3],
            &[(l >> 16) as u8, (l >> 8) as u8, l as u8]
        );
    }
    // Scaled (`pal(P, v) * k`) and covered, as `rgb` of its bytes would be.
    let a = spr_frame("palette sea = #003366, #3399cc, #ffffff\npal(sea, 0.5) * y");
    let b = spr_frame("rgb(0.2*y, 0.6*y, 0.8*y)");
    assert_eq!(a, b);
    let c = spr_frame("palette sea = #003366, #3399cc, #ffffff\nlayer { gray(0.25) }\nlayer add { pal(sea, 1, x) }");
    let d = spr_frame("layer { gray(0.25) }\nlayer add { rgb(1, 1, 1, x) }");
    assert_eq!(c, d);
    // Three of a formula's own, one stored once, in parts too; a name a
    // formula sets is still that name.
    let p = compile("palette a1 = #f00, #0f0\npalette a2 = #00f, #fff\npart q {\n  palette a3 = #f00, #0f0\n  palette a4 = #123, #456\n  pal(a4, x)\n}\nlayer { pal(a1, x) }\nlayer add { pal(a2, y, 0.5) }\nlayer add { q(x, y, 0.5) }").unwrap();
    assert_eq!(p.layers(), 3);
    compile("palette = 3\ngray(palette*x)").unwrap();
    assert_eq!(
        spr_frame("palette p1 = #ff0000\npal(p1, x)")[..3],
        [255, 0, 0]
    );
}

/// The palettes' errors (E921-E924).
const PALETTE_ERRORS: &[(&str, &str, usize, usize, &str)] = &[
    (
        "layer {\npalette p = #fff\npal(p, x)\n}",
        "a palette goes at the top",
        2,
        1,
        "E921",
    ),
    (
        "palette p = #fff, 3\npal(p, x)",
        "a palette is 1 to 32 colours",
        1,
        19,
        "E922",
    ),
    (
        "palette p = red\npal(p, x)",
        "a palette is 1 to 32 colours",
        1,
        13,
        "E922",
    ),
    (
        "palette p1 = #f00\npalette p2 = #0f0\npalette p3 = #00f\npalette p4 = #fff\npal(p4, x)",
        "3 palettes of its own at most",
        4,
        1,
        "E923",
    ),
    (
        "palette fire = #f00\npal(fire, x)",
        "a palette's name",
        1,
        9,
        "—",
    ),
    (
        "palette p = #f00\npalette p = #0f0\npal(p, x)",
        "`p` is already set on line 1",
        2,
        9,
        "—",
    ),
    ("palette p = #f00\npal(q, x)", "and yours: p", 2, 5, "—"),
];

#[test]
fn palette_error_messages() {
    let mut bad = alloc::vec::Vec::new();
    for &(src, want, line, col, code) in PALETTE_ERRORS {
        match compile(src) {
            Ok(_) => bad.push(alloc::format!("{code} {src:?}: compiled")),
            Err(e) => {
                let lc = e.line_col(src);
                if !e.msg.contains(want) || lc != (line, col) {
                    bad.push(alloc::format!(
                        "{code} {src:?}: got {:?} at {lc:?}, want {want:?} at {:?}",
                        e.msg,
                        (line, col)
                    ));
                }
            }
        }
    }
    let many: alloc::vec::Vec<alloc::string::String> =
        (0..33).map(|k| alloc::format!("#{k:02x}0000")).collect();
    let e = compile(&alloc::format!(
        "palette p = {}\npal(p, x)",
        many.join(", ")
    ))
    .unwrap_err();
    if !e.msg.contains("1 to 32 colours") {
        bad.push(alloc::format!("E922 33: {}", e.msg));
    }
    // 200 table numbers and two palettes: past 256.
    let nums: alloc::vec::Vec<alloc::string::String> =
        (0..200).map(|k| alloc::format!("{k}")).collect();
    let e = compile(&alloc::format!(
        "table T = {}\npalette p = #f00\npalette q = #0f0\npal(q, T[x])",
        nums.join(", ")
    ))
    .unwrap_err();
    if !e
        .msg
        .contains("tables, sprites and palettes hold 256 numbers")
    {
        bad.push(alloc::format!("E924: {}", e.msg));
    }
    let e = compile(&alloc::format!(
        "palette p = #f00\npalette q = #0f0\ntable T = {}\npal(q, T[x])",
        nums.join(", ")
    ))
    .unwrap_err();
    if !e.msg.contains("256") {
        bad.push(alloc::format!("E924 (table after): {}", e.msg));
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

