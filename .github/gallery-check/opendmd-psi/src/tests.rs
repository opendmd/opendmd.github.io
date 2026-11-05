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
    let full = compile(
        "z = 3 + t*0 + floor(a*0.1)\ngray(noise(x*8, y*4, z) + noise(px, py, -1 + z*0)*0.25)",
    )
    .unwrap();
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
        let yv = fixed::div(((row) as i32) << 16, ((H - 1) as i32) << 16);
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
    assert!(e.msg.contains("only be scaled"), "{}", e.msg);
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
    // Division by a constant stays a division (the same value at every
    // level); % by a power of two is a mask. (Narrowed, the division by a
    // scalar is `div_s`: no division instruction.)
    let mut p = Program::default();
    crate::compile_with("gray(x / 4 + x % 0.5)", &mut p, true, false).unwrap();
    let codes: Vec<u8> = p.ops[..p.len()].iter().map(|o| o.code).collect();
    assert!(codes.contains(&code::DIV) && codes.contains(&code::MASK));
    assert!(!codes.contains(&code::MUL) && !codes.contains(&code::MOD));
    let p = compile("gray(x / 4 + x % 0.5)").unwrap();
    let codes: Vec<u8> = p.ops[..p.len()].iter().map(|o| o.code).collect();
    assert!(codes.contains(&code::DIV_S) && codes.contains(&code::ADD_W));
}

#[test]
fn levels_run_as_rarely_as_they_can() {
    // t-only: per frame; y-only: per row; x: per pixel.
    let p = compile("a1 = sin(t/4)\nb1 = y * a1\nrgb(x + b1, 0, 0)").unwrap();
    assert_eq!(p.counts(), (2, 1, 2), "{}", p.listing());
    // Division by a per-row value: the divisor a row, the division a pixel
    // (`div_s`: the divisor's multiplier worked out once a lane).
    let p = compile("gray(x / (y + 1))").unwrap();
    assert_eq!(p.counts(), (0, 1, 2), "{}", p.listing());
    assert!(p.listing().contains("div_s"), "{}", p.listing());
    // One over a pixel value: `recip` (the same bits as the division).
    let p = compile("gray(1 / (px + 1))").unwrap();
    assert!(p.listing().contains("recip"), "{}", p.listing());
    assert!(!p.listing().contains("div"), "{}", p.listing());
    let plasma = compile(sketches::PLASMA).unwrap();
    // (its `len(x-0.5, y-0.5)*6` is worked out once, and its `sin(x*3 +
    // t/8)` once a column: the still cache's and the column run's markers
    // are not counted as steps)
    assert_eq!(plasma.counts(), (2, 5, 13));
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
    assert!(e.msg.contains("values at once"), "{}", e.msg);
}

/// (source, message, line, column)
const ERRORS: &[(&str, &str, usize, usize)] = &[
    (
        "hsv(tme, 1, 1)",
        "unknown name `tme` — did you mean `t`?",
        1,
        5,
    ),
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
    ("gray(digit)", "`digit` is a function: write `digit(…)`", 1, 6),
    ("fire = 2\ngray(fire)", "is a palette's name", 1, 1),
    (
        "gray(((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((x)))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))",
        "nested too deeply",
        1,
        54,
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
    assert!(core::mem::size_of::<crate::ProgramOf<[i32; 0]>>() <= 6_500);
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
        "a1 = x +\n y\n q = t\nrgb(a1, q, - -x)",
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

/// One frame's cost counts each scene by the rows its `when` is on in that
/// frame, and the draw stage's arm it takes: the docs' environment cycle,
/// its towers on rows 0-19 while the city is on, the sea all rows while it
/// is off.
#[test]
fn a_frame_costs_the_scenes_and_rows_it_runs() {
    let src = "loop 8\ncity = frac(t/8) > 0.5\nscene towers when city * (py < 20) {\n  h = hash(floor(px/6), 3)\n  lit = (frac(px/6) > 0.3) * (frac(py/3) > 0.4) * (h > 0.3)\n}\nscene sea when 1 - city {\n  wave = sin(x*12 + t*2 + y*6)*0.5 + 0.5\n}\ndraw {\n  if city { for k < 40 { line(k*3, 0, k*3, 31, 0.5) } } else { dot(1, 1, 0.5) }\n}\nrgb(lit, wave*0.3 + lit*0.5 + ink, wave)";
    let p = compile(src).unwrap();
    let mut s = Scratch::<128>::new();
    let at = |ms: u64, s: &mut Scratch<128>| {
        let mut inp = Inputs::default();
        inp.set_ms(&p, ms);
        scene_rows(&p, s, &inp, &OWN)
    };
    let (sea, city) = (at(1000, &mut s), at(5000, &mut s));
    assert_eq!(&sea.rows[1..3], &[0, 32]);
    assert_eq!(&city.rows[1..3], &[20, 0]);
    assert_eq!((sea.known & 1, sea.taken & 1, city.taken & 1), (1, 0, 1));
    // Each frame no more than the two costliest scenes on every row and
    // the costlier arm; the city's frame, with its lines, more than the
    // sea's.
    let (a, b) = (p.tier_us_frame(false, &sea), p.tier_us_frame(false, &city));
    assert!(a <= p.tier_us(false) && b <= p.tier_us(false), "{a} {b} {}", p.tier_us(false));
    assert!(b > a, "{a} {b}");
}

/// Palettes of 16 colours or fewer that only tweens' halves read take half
/// a store place each: ten of them fit the store's eight places.
#[test]
fn half_palettes_take_half_a_place() {
    let mut src = String::from("loop 8\nw = frac(t/8)\n");
    for k in 0..10 {
        src += &format!("palette h{k} = #{k}00, #0{k}0, #00{k}, #{k}{k}{k}\n");
    }
    src += "palette S = tween(tween(tween(h0, h1, w), tween(h2, h3, w), w), tween(h4, h5, w), w), tween(tween(h6, h7, w), tween(h8, h9, w), w)\n";
    src += "pal(S, floor(x*32)/32)";
    compile(&src).unwrap_or_else(|e| panic!("{}", e.render(&src)));
}

#[test]
fn sizes() {
    // What the docs say (Program, the scratch at 128 and 64 lanes, the
    // draw stage's canvas).
    // (768 steps and the two markers, 241 values, the palette store: 3,388
    // B before P0, 6,156 at 512 steps.)
    // (ABI 3: the store sized by the formula, after the fixed part: a
    // whole `Program`'s 1,024 numbers, a player's `Prog` its own.)
    assert_eq!(core::mem::size_of::<Program>(), 10_536);
    assert_eq!(core::mem::size_of::<crate::ProgramOf<[i32; 0]>>(), 6_440);
    assert_eq!(crate::Prog::layout(256).size(), 6_440 + 1_024);
    // The ink plane and the paint plane's pointer (the plane itself, 4 KB
    // more, only for a formula that paints).
    assert_eq!(
        core::mem::size_of::<crate::Ink>(),
        4096 + core::mem::size_of::<usize>()
    );
    // (+ racing the beam's order, places and sink: 12 B on the ESP32.)
    let beam = 3 * core::mem::size_of::<usize>();
    // (The scalar slots one for every operand byte: 256, 128 before.)
    assert_eq!(core::mem::size_of::<Scratch<128>>(), 8704 + beam);
    assert_eq!(core::mem::size_of::<Scratch<64>>(), 5376 + beam);
}

#[test]
fn sliders_and_the_esp32_estimate() {
    let s = |src: &str| compile(src).unwrap().sliders();
    assert_eq!(s("gray(x)"), 0);
    assert_eq!(s("gray(a)"), 1);
    assert_eq!(s("k = c * 2\nrgb(x, k, b)"), 0b110);
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
                "functions"
                    | "nested"
                    | "nested digit"
                    | "nested mix"
                    | "nested pick"
                    | "functions in layers"
                    | "draw"
                    | "nested sprite"
                    | "nested forever"
                    | "nested palette"
                    | "declared"
            ) && n <= 2048
                || (1900..=2048).contains(&n),
            "{name}: {n} B"
        );
        match compile(&src) {
            Ok(p) => assert!(p.len() <= crate::MAX_OPS, "{name}"),
            Err(e) => assert!(
                e.msg.contains("too long for the panel")
                    || e.msg.contains("too many times")
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
        let py = row;
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
    assert!(compile("s = t\nlayer { u = s*2\ngray(u) }\nlayer add { u = s\ngray(u) }").is_ok());
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
    ("layer { v = x }", "this layer ends without a colour", 1, 15),
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
    (
        "layer = 3\ngray(x)",
        "`layer` is a word of the language",
        1,
        1,
    ),
    (
        "fn p(a) = a\nlayer { p = 2\ngray(p) }",
        "`p` is already the name of a function (line 1)",
        2,
        9,
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

// --- Functions -------------------------------------------------------------------

#[test]
fn functions_are_their_bodies_written_out() {
    let same = |a: &str, b: &str| {
        assert_eq!(compile(a).unwrap(), compile(b).unwrap(), "{a}\nvs\n{b}");
    };
    // A value; a body of lines; a colour by channel; a whole colour.
    same("fn f(a, b) = a*2 + b\ngray(f(x, y))", "gray(x*2 + y)");
    same(
        "fn f(a) {\n  k = a*a\n  k + 0.5\n}\ngray(f(x))",
        "gray(x*x + 0.5)",
    );
    same(
        "fn sky(v) = mix(#102040, #ff8800, v)\nsky(y)",
        "mix(#102040, #ff8800, y)",
    );
    same("fn tint(c, k) = c * k\ntint(#f80, x)", "#f80 * x");
    same(
        "fn p(v) {\n  w = v*0.5\n  hsv(w, 1, 1)\n}\np(x)",
        "hsv(x*0.5, 1, 1)",
    );
    same("fn p(v) = pal(fire, v)\np(x)", "pal(fire, x)");
    // A function inside another; called twice; unused costs nothing.
    same(
        "fn sq(a) = a*a\nfn d(a, b) = sq(a) + sq(b)\ngray(d(x, y))",
        "gray(x*x + y*y)",
    );
    same("fn u(a) = noise(a, y, t)\ngray(x)", "gray(x)");
    // Its values hide the formula's names of the same name; others it
    // sees as they are where it is called.
    same("k = 0.25\nfn f(x) = x + k\ngray(f(y))", "gray(y + 0.25)");
    // Its own names stay inside it.
    same(
        "v = y\nfn f(a) {\n  v = a\n  v*2\n}\ngray(f(x) + v)",
        "gray(x*2 + y)",
    );
    // Drawing: lines called on a line of their own in draw.
    let same = |a: &str, b: &str| assert!(over(a, None, 0) == over(b, None, 0), "{a}\nvs\n{b}");
    same(
        "fn palm(x0, s) {\n  line(x0, 31, x0, 31 - 10*s, #a62)\n  circfill(x0, 31 - 10*s, 3*s, #2a4)\n}\ndraw { for k < 3 { palm(10 + k*30, 1) } }\ngray(0)",
        "draw { for k < 3 {\n  line(10 + k*30, 31, 10 + k*30, 21, #a62)\n  circfill(10 + k*30, 21, 3, #2a4)\n} }\ngray(0)",
    );
}

/// A function's values named `x, y`: `px py i cx cy r angle` inside it are
/// those of that place (the picture drawn elsewhere); set at the top, the
/// same for what follows.
#[test]
fn x_and_y_move_what_comes_from_them() {
    let same = |a: &str, b: &str| {
        assert!(over(a, None, 0) == over(b, None, 0), "{a}\nvs\n{b}");
    };
    same(
        "fn p(x, y) = rgb(x, y, 0)\np(1 - x, y*0.5)",
        "rgb(1 - x, y*0.5, 0)",
    );
    same(
        "fn p(x, y) = rgb(px % 3 / 2, py % 4 / 3, i % 7 / 6)\np(x*2, 1 - y)",
        "qx = floor(x*2*127 + 0.5); qy = floor((1 - y)*31 + 0.5)\nrgb(qx % 3 / 2, qy % 4 / 3, (qy*128 + qx) % 7 / 6)",
    );
    same(
        "fn p(x, y) = hsv(angle, 1, r)\np(1 - x, y)",
        "hsv(atan2(y - 0.5, (1 - x - 0.5)*4), 1, len((1 - x - 0.5)*4, y - 0.5))",
    );
    same("y = y*0.5\ngray(py/31)", "gray(floor(y*0.5*31 + 0.5)/31)");
}

#[test]
fn functions_stay_within_the_limits() {
    // Called again and again, nested: bounded.
    let mut s = String::from("fn f0(a) = a\n");
    for k in 1..7 {
        let j = k - 1;
        s += &alloc::format!("fn f{k}(a) = f{j}(a) + f{j}(a*0.5) + f{j}(a*0.25) + f{j}(a*0.125) + f{j}(a*3) + f{j}(a*5) + f{j}(a*7) + f{j}(a*9)\n");
    }
    s += "gray(f6(x))";
    let e = compile(&s).unwrap_err();
    assert!(e.msg.contains("too long for the panel"), "{}", e.msg);
    // Long bodies read again and again, each adding nothing new: the
    // reading itself is bounded.
    let mut s = String::from("fn f0(a) = a\n");
    let pad = " + 0".repeat(60);
    for k in 1..12 {
        let j = k - 1;
        s += &alloc::format!("fn f{k}(a) = max(f{j}(a), f{j}(a)){pad}\n");
    }
    s += "gray(f11(x))";
    let e = compile(&s).unwrap_err();
    assert!(
        e.msg.contains("calls its functions too many times"),
        "{}",
        e.msg
    );
    // Itself: refused.
    let e = compile("fn f(a) = a + f(a)\ngray(f(x))").unwrap_err();
    assert!(e.msg.contains("calls itself"), "{}", e.msg);
}

/// (source, message, line, column)
const FN_ERRORS: &[(&str, &str, usize, usize)] = &[
    (
        "fn f(a) = a\ngray(f(x, y))",
        "`f` takes 1 values, but got 2",
        2,
        6,
    ),
    (
        "fn f(a) = a\nf = 2\ngray(x)",
        "`f` is already the name of a function (line 1)",
        2,
        1,
    ),
    (
        "fn f(a) {\n  gray(a)\n  v = 1\n}\nf(x)",
        "only a function's last line is its value",
        2,
        3,
    ),
    (
        "fn p(v) = hsv(v, 1, 1)\ngray(p(x))",
        "`p(…)` is a whole colour",
        2,
        6,
    ),
    (
        "fn p(v) = hsv(v, 1, 1)\np(x) * 0.5",
        "`p(…)` is a whole colour: it can't go into a sum",
        2,
        6,
    ),
    (
        "fn f(a) = gray(tme)\nf(x)",
        "unknown name `tme` — did you mean `t`?",
        1,
        16,
    ),
    (
        "part p { gray(x) }\np",
        "there are no parts: write a function",
        1,
        1,
    ),
    (
        "use Plasma as p\np",
        "`use` puts a formula in by its name",
        1,
        1,
    ),
    (
        "part = 2\ngray(x)",
        "`part` is a word of the language",
        1,
        1,
    ),
    ("fn = 2\ngray(x)", "`fn` is a word of the language", 1, 1),
];

#[test]
fn function_error_messages() {
    let mut bad = alloc::vec::Vec::new();
    for &(src, want, line, col) in FN_ERRORS {
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
fn compact_keeps_functions() {
    let src = "loop 8\nfn p(x, y) {\n  -- Plasma\n  v = sin(x*2 + t/4)\n  hsv(v, 1, 1)\n}\nlayer { p(x, y) }\nlayer add { hsv(sin((1 - x)*2 + t/4), 1, 1, 0.5) }";
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
    same("gray(r)", "gray(len((x - 0.5)*4, y - 0.5))");
    same("gray(angle)", "gray(atan2(y - 0.5, (x - 0.5)*4))");
    // Several: the same steps (maybe in another order), the same bytes.
    let (a, b) = (
        "fn p(x, y) = hsv(angle, 1, r + py/31)\np(1 - x, y*0.5)",
        "fn p(x, y) = hsv(atan2(y - 0.5, (x - 0.5)*4), 1, len((x - 0.5)*4, y - 0.5) + py/31)\np(1 - x, y*0.5)",
    );
    assert_eq!(compile(a).unwrap().counts(), compile(b).unwrap().counts());
    assert!(over(a, None, 0) == over(b, None, 0));
    same(
        "r = x\ncx = y\npy = 0.5\nangle = 1\ncy = 0\npick = 0.25\nrgb(r, cx, py + angle + cy + pick)",
        "rgb(x, y, 0.5 + 1 + 0 + 0.25)",
    );
    // A `fn` takes no built-in function's name (language review A2: a
    // later built-in can't be shadowed by a function, only by a value).
    let e = compile("fn pick(a, b) = a*b\ngray(pick(x, y))").unwrap_err();
    assert!(e.msg.contains("is a built-in function"), "{}", e.msg);
}

/// `pick(k, …)` against the chain of `?:` it replaces, byte for byte; one
/// step a pixel when its choices are the same for every pixel.
#[test]
fn pick_is_the_chain_it_replaces() {
    let cases = [
        (
            "k = floor(px/32)\nhh = hour/24; m = minute/60\ngray(pick(k, 0.1, hh, m, 1))",
            "k = floor(px/32)\nhh = hour/24; m = minute/60\ngray(k < 0 ? 0 : k < 1 ? 0.1 : k < 2 ? hh : k < 3 ? m : k < 4 ? 1 : 0)",
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
            "s = second % 4\ngray(pick(s, 0.25, 0.5, 0.75, 1) * x)",
            "s = second % 4\ngray((s < 1 ? 0.25 : s < 2 ? 0.5 : s < 3 ? 0.75 : 1) * x)",
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
    // (Some of the chain's compares fused with their selects: P0b-4.)
    assert_eq!(px(cases[0].0) + 8, px(cases[0].1));
    assert_eq!(px(cases[5].0) + 7, px(cases[5].1));
    // In a colour expression (read three times) a pick is one step (and
    // all of it once a column; the column run's marker is not counted).
    assert_eq!(
        px("#f80 * pick(px/32, 0.5, 1)"),
        4,
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
    // Anywhere a colour line goes: a layer, a function.
    for s in [
        "layer { #102040 }\nlayer multiply { mix(#fff, #000, x) }",
        "fn p(x, y) = mix(#000, #fff, x)\np(1 - x, y)",
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
        ("gray(#fff)", "is a colour, not a number", 1, 6),
        ("hsv(#f00)", "is a colour, not a number", 1, 5),
        (
            "v = mix(#000, #fff, x)\ngray(v)",
            "is a colour, not a number",
            1,
            9,
        ),
        ("mix(#000, #fff)", "`mix` takes 3 values, but got 2", 1, 1),
        ("rgb(1, #fff, 0)", "takes three numbers, not colours", 1, 8),
        (
            "mix(rgb(1, #fff, 0), #000, x)",
            "takes three numbers, not colours",
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
            "is a whole colour: it can't be combined with others",
            1,
            5,
        ),
        (
            "pal(fire, x) * #fff",
            "`pal(…)` is a whole colour: it can't go into a sum",
            1,
            14,
        ),
        (
            "gray(x) * #fff",
            "`gray(…)` is a whole colour: it can't go into a sum",
            1,
            9,
        ),
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
    ("draw { dot(1, 1) }\ndraw { dot(2, 2) }\ngray(ink)", "there can be one `draw { }`", 2, 1, "E702"),
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
    ("table T = 1, 2\ntable U = 3,\n4\ngray(T[0] + U[1] + V[0])", "reads a table, and `V` is not one", 4, 20, "E710"),
    ("draw { dot(x, 1) }\ngray(ink)", "`draw { }` runs once a frame", 1, 12, "E708"),
    ("draw { v = py*2\ndot(v, 1) }\ngray(ink)", "but this uses `py`", 1, 12, "E708"),
    ("draw { line(0, 0, 9, ink) }\ngray(ink)", "but this uses `ink`", 1, 22, "E708"),
    ("draw { for k < 3 { k = 2\ndot(k, 1) } }\ngray(ink)", "`k` is a loop counter: it can't be set", 1, 20, "E709"),
    ("draw { for k < 3 { for k < 2 { dot(k, 1) } } }\ngray(ink)", "`k` is already a loop counter here", 1, 24, "E709"),
    ("v = 1\ngray(v[0])", "`v[…]` reads a table, and `v` is not one", 2, 6, "E710"),
    ("table T = 1, 2\ngray(T)", "`T` is a table: read one of its numbers, `T[k]`", 2, 6, "E710"),
    ("table T = 1, 2\ngray(T[0)", "expected `]` here", 2, 9, "—"),
    ("draw { dot(1) }\ngray(ink)", "`dot` takes x, y", 1, 8, "—"),
    ("draw { line(1, 2, 3, 4, 5, 6, 7) }\ngray(ink)", "`line` takes x0, y0, x1, y1", 1, 8, "—"),
    ("for k < 3 { }\ngray(1)", "`for` goes inside `draw { }`", 1, 1, "—"),
    ("draw { dot(1, 1)\ngray(ink)", "this `draw {` is never closed", 1, 1, "—"),
    ("table T = 1\ntable T = 2\ngray(T[0])", "`T` is already the name of a table (line 1)", 2, 7, "—"),
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
    if !e.msg.contains("a table holds 256 numbers at most") {
        bad.push(alloc::format!("E974 257 numbers: {}", e.msg));
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
    let p = compile("table T = 0.1, 0.2, 0.3, 0.4\ntable U = 0.2, 0.3\ntable V = 0.1, 0.2\nrgb(T[px % 5], U[py % 3] + V[3], T[second % 5])").unwrap();
    // Three tables: the second at the next place in fours (a step names
    // a table's place in fours), the third inside the first: 6 numbers
    // held.
    assert_eq!(&p.tables[..7], &[q(0.1), q(0.2), q(0.3), q(0.4), q(0.2), q(0.3), 0]);
    let mut out = [0u8; W * H * 3];
    let inputs = Inputs {
        second: 3,
        ..Default::default()
    };
    render(&p, &mut Scratch::new(), &inputs, &OWN, &mut out);
    for (k, px) in out.chunks(3).enumerate() {
        let (col, row) = (k % W, k / W);
        let py = row;
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
    let at = |ink: &Ink, x: i32, y: i32| ink.at(x as usize, y as usize);
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
    // Rows count down from the top: (6, 2) is row 2.
    assert_eq!(ink.at(6, 2), 255);
    // -0.5 rounds up to 0, onto (0, 0), lighter than the line there.
    assert_eq!(at(&ink, 0, 0), fixed::chan(q(0.5)));
}

/// `ink(n)` is the canvas byte in `n` levels, round(b·n/255), exactly:
/// every byte and level, and an ink written `k/n` reads back as `k` for
/// every k ≤ n (the byte is `chan(k/n)`, the constant `k/n` folded as the
/// compiler folds it).
#[test]
fn ink_levels_are_exact() {
    for n in 1..=255i32 {
        for b in 0..=255u8 {
            assert_eq!(
                fixed::ink_levels(b, n) >> 16,
                (2 * b as i32 * n + 255) / 510,
                "b {b} n {n}"
            );
        }
        for k in 0..=n {
            let v = crate::program::eval(code::DIV, k << 16, n << 16, 0, None);
            assert_eq!(fixed::ink_levels(fixed::chan(v), n), k << 16, "{k}/{n}");
        }
    }
    // Through the compiler and the draw stage: lines of ink k/n at column
    // k read back as k.
    for n in [1, 2, 3, 7, 16, 100, 127] {
        let src = format!("draw {{ for k < {} {{ line(k, 0, k, 31, k/{n}) }} }}\ngray(px > {n} or ink({n}) == px)", n + 1);
        let p = compile(&src).unwrap_or_else(|e| panic!("{}", e.render(&src)));
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
        assert!(out.iter().all(|&c| c == 255), "ink({n})");
    }
    // Only a whole number from 1 to 255.
    for bad in ["ink(0)", "ink(256)", "ink(1.5)", "ink(x)", "ink(16, 2)"] {
        let src = format!("draw {{ dot(1, 1) }}\ngray({bad})");
        let e = compile(&src).unwrap_err();
        assert!(
            e.msg
                .contains("`ink(n)` takes a whole number from 1 to 255"),
            "{bad}: {}",
            e.msg
        );
    }
    // A name set `ink` is that name.
    assert!(compile("draw { dot(1, 1) }\nink = 3\ngray(ink/4)").is_ok());
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
    // along the top (y 0), none after, none at the bottom.
    let top = (0..W).filter(|&x| ink.at(x, 0) > 0).count();
    assert_eq!(top, 20);
    assert!((0..W).all(|x| ink.at(x, H - 1) == 0));
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
}

/// The wide steps' errors (E901-E906; `Error.code` is not a field yet).
const FOREVER_ERRORS: &[(&str, &str, usize, usize, &str)] = &[
    ("loop forever\ngray(t*x)", "grows without end", 2, 7, "E901"),
    ("loop forever\ngray(sqrt(t))", "grows without end", 2, 6, "E901"),
    ("loop forever\ngray(frac(t*t))", "grows without end", 2, 12, "E901"),
    ("loop forever\ngray(floor(t + x))", "a growing value plus a per-pixel", 2, 6, "E907"),
    ("loop forever\nv = t + px\ngray(v < 3)", "a growing value plus a per-pixel", 3, 8, "E907"),
    ("loop forever\ntable T = 1, 2\ngray(T[t])", "grows without end", 3, 6, "E901"),
    ("loop forever\ngray(pick(px/32, t, 1))", "grows without end", 2, 6, "E901"),
    ("loop forever\ngray(x / t)", "grows without end", 2, 8, "E901"),
    ("loop forever\ngray(frac((t + x)/3))", "a growing value plus a per-pixel or per-row part", 2, 18, "E907"),
    ("loop forever\ngray((t + x) > 5)", "a growing value plus a per-pixel or per-row part", 2, 14, "E907"),
    ("loop forever\nv = t + x\ngray(v)", "a colour can't take a value that grows", 2, 1, "E905"),
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
            "sprite pic {{\n  key {}\n{}}}\ngray(pic[px - 3, py - 2])",
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
    let src = "sprite s {\n  key . 0, a 0.25, b 0.5, c 1\n  abc.\n  .cba\n}\nk = s[2, 0]\nrgb(s[1.9, 1.99] + k*0, s[px - 1, py*0], s[px*0.5 - 0.5, 1])";
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
    let a = compile("sprite s {\nkey . 0, X 1\n.X.\n}\nsprite u {\nkey . 0, X 1\n.X.\n}\ngray(s[px, py] + u[px, py])").unwrap();
    assert!(a.listing().matches("spr(").count() == 1, "{}", a.listing());
}

/// The source sent to the panel (`compact`: comments out, lines joined by
/// `; `) is the same sprite.
#[test]
fn a_sprite_compacts() {
    let src = "-- a car\nsprite car {   -- its key first\n  key . 0, R 0.5, # 1   -- R red, # dark\n  ..RR..\n  .R##R.   -- the glass\n\n  RRRRRR\n}\ngray(car[px - 61, py - 12])";
    let c = compact(src);
    assert_eq!(
        c,
        "sprite car {; key . 0, R 0.5, # 1; ..RR..; .R##R.; RRRRRR; }; gray(car[px - 61, py - 12])"
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
        "a key entry is a character",
        2,
        5,
        "E913",
    ),
    (
        "sprite s {\nkey . 0, X one\n.X.\n}\ngray(s[0, 0])",
        "a key entry is a character",
        2,
        10,
        "E913",
    ),
    (
        "sprite s {\nkey . 0, - 1\n.X.\n}\ngray(s[0, 0])",
        "a key entry is a character",
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
        "`s` is a sprite: read its pixels, `s[u, v]`",
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
        "`s` is already the name of a sprite (line 1)",
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
    // Four of them (each 273 numbers, a row apart so none is stored once
    // for another) pass the 1,024.
    let mut src = alloc::string::String::from("table T = 1, 2\n");
    let rows: alloc::vec::Vec<&str> = big.lines().collect();
    for s in 0..4 {
        let mine: alloc::string::String = (0..32).map(|k| alloc::format!("{}\n", rows[(k + s) % 32])).collect();
        src += &alloc::format!("sprite s{s} {{\nkey {}\n{mine}}}\n", keys.join(", "));
    }
    src += "gray(s0[0, 0] + s1[0, 0] + s2[0, 0] + s3[0, 0] + T[0])";
    let e = compile(&src).unwrap_err();
    if !e.msg.contains("tables and sprites hold 1024 numbers in all") {
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
    // Three of a formula's own, one stored once; a name a formula sets is
    // still that name.
    let p = compile("palette a1 = #f00, #0f0\npalette a2 = #00f, #fff\npalette a3 = #f00, #0f0\npalette a4 = #123, #456\nlayer { pal(a1, x) }\nlayer add { pal(a2, y, 0.5) }\nlayer add { pal(a4, x, 0.5) }").unwrap();
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
        "palette p1 = #111, #001, #002, #003, #004, #005, #006, #007, #008, #009, #00a, #00b, #00c, #00d, #00e, #00f, #010\npalette p2 = #222, #001, #002, #003, #004, #005, #006, #007, #008, #009, #00a, #00b, #00c, #00d, #00e, #00f, #010\npalette p3 = #333, #001, #002, #003, #004, #005, #006, #007, #008, #009, #00a, #00b, #00c, #00d, #00e, #00f, #010\npalette p4 = #444, #001, #002, #003, #004, #005, #006, #007, #008, #009, #00a, #00b, #00c, #00d, #00e, #00f, #010\npalette p5 = #555, #001, #002, #003, #004, #005, #006, #007, #008, #009, #00a, #00b, #00c, #00d, #00e, #00f, #010\npalette p6 = #666, #001, #002, #003, #004, #005, #006, #007, #008, #009, #00a, #00b, #00c, #00d, #00e, #00f, #010\npalette p7 = #777, #001, #002, #003, #004, #005, #006, #007, #008, #009, #00a, #00b, #00c, #00d, #00e, #00f, #010\npalette p8 = #888, #001, #002, #003, #004, #005, #006, #007, #008, #009, #00a, #00b, #00c, #00d, #00e, #00f, #010\npalette p9 = #999, #001, #002, #003, #004, #005, #006, #007, #008, #009, #00a, #00b, #00c, #00d, #00e, #00f, #010\npal(p9, x)",
        "8 palettes of its own at most",
        9,
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
        "`p` is already the name of a palette (line 1)",
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
    // Palettes have a store of their own: 200 table numbers and two
    // palettes fit; 200 and 60 more table numbers don't.
    let nums: alloc::vec::Vec<alloc::string::String> =
        (0..200).map(|k| alloc::format!("{k}")).collect();
    assert!(compile(&alloc::format!(
        "table T = {}\npalette p = #f00\npalette q = #0f0\npal(q, T[x])",
        nums.join(", ")
    ))
    .is_ok());
    let e = compile(&alloc::format!(
        "table T = {}\ntable U = -{}\ntable V = 1{}\ntable X = 2{}\ntable Y = 3{}\ngray(T[x] + U[x] + V[x] + X[x] + Y[x])",
        nums.join(", "),
        nums[..60].join(", -"),
        ", 9".repeat(255),
        ", 8".repeat(255),
        ", 7".repeat(255),
    ))
    .unwrap_err();
    if !e.msg.contains("1024") {
        bad.push(alloc::format!("E924 (table after): {}", e.msg));
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

// --- `cycle`: palette cycling (console-lessons Phase B) -------------------------

/// `cycle(P, v, s)` is entry (floor(v·32) + floor(s·32)) mod 32 of P: the
/// same bytes as `pal` of the folded index wherever that sum can't
/// saturate, the turn per frame or per row, scaled and covered.
#[test]
fn cycle_is_the_palette_turned() {
    let pairs = [
        (
            "cycle(fire, x*3 - 1, t/4)",
            "pal(fire, frac(frac(x*3 - 1) + floor(frac(t/4)*32)/32))",
        ),
        (
            "cycle(rainbow, len(cx, cy), -t/3 + py/40)",
            "pal(rainbow, frac(frac(len(cx, cy)) + floor(frac(-t/3 + py/40)*32)/32))",
        ),
        (
            "palette p = #f00, #0f0, #00f\ncycle(p, x - y*2.5, t*0.37) * (0.5 + x/2)",
            "palette p = #f00, #0f0, #00f\npal(p, frac(frac(x - y*2.5) + floor(frac(t*0.37)*32)/32)) * (0.5 + x/2)",
        ),
        (
            "layer { gray(x) }\nlayer screen { cycle(ice, y*7, t, x) }",
            "layer { gray(x) }\nlayer screen { pal(ice, frac(frac(y*7) + floor(frac(t)*32)/32), x) }",
        ),
        (
            "cycle(chrono, x, 0.5)",
            "pal(chrono, frac(x + 0.5))",
        ),
    ];
    for (a, b) in pairs {
        for ms in [0, 130, 1000, 2470, 5555, 7990] {
            assert_eq!(over(a, None, ms), over(b, None, ms), "{a} at {ms} ms");
        }
    }
}

#[test]
fn cycle_turns_once_a_row_and_takes_the_still_cache() {
    let p = compile("cycle(fire, sin(x*2) + sin(y*1.5) + sin(x*1.5 + y), t/8)").unwrap();
    let l = p.listing();
    assert!(l.contains("turn("), "{l}");
    assert_eq!(p.still_lanes(), 1, "{l}");
    // A turn per row: in the row section, the last step there.
    let (de, re) = (p.draw_end as usize, p.row_end as usize);
    assert_eq!(p.ops[re - 1].code, code::TURN);
    assert!(de < re);
    // The same palette and turn twice: one turn.
    let p = compile("layer { cycle(fire, x, t) }\nlayer add { cycle(fire, y, t, 0.5) }").unwrap();
    assert_eq!(p.listing().matches("turn(").count(), 1);
    // `cycle` stays a name a saved formula may set.
    assert!(compile("cycle = x\ngray(cycle)").is_ok());
}

/// The errors (E931-E934).
#[test]
fn cycle_error_messages() {
    let cases: &[(&str, &str, &str)] = &[
        ("cycle(fire, x, x)", "turns a whole row at once", "E931"),
        ("cycle(gray, x, t)", "gray is not one", "E932"),
        ("cycle(nope, x, t)", "unknown palette", "E932"),
        (
            "palette pa = #f00\npalette pb = #0f0\nlayer { cycle(pa, x, t) }\nlayer add { cycle(pb, x, t/2) }",
            "holds 3 palettes",
            "E933",
        ),
        ("cycle(fire, x)", "and the turn", "E934"),
        ("cycle(fire, x, t) + 1", "can't go into a sum", "E934"),
    ];
    let mut bad = alloc::vec::Vec::new();
    for &(src, want, code) in cases {
        match compile(src) {
            Ok(_) => bad.push(alloc::format!("{code} {src:?}: compiled")),
            Err(e) if !e.msg.contains(want) => {
                bad.push(alloc::format!("{code} {src:?}: {}", e.msg))
            }
            Err(_) => {}
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

// --- `layer rate N { }`: slow layers (console-lessons Phase B) ------------------

fn frame_of(p: &Prog, i: &Inputs) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec![0u8; W * H * 3];
    render_rows::<128>(p, &mut Scratch::new(), i, &OWN, None, u32::MAX, &mut out);
    out
}

/// The time each row of the slow layers is drawn at, from the definition
/// (`rate`'s module doc), written out again here.
fn slow_ms(n: i64, f: i64, lp: Option<i64>, ms: i64, r: usize) -> i64 {
    let k = ms / f;
    let b = r as i64 * n / 32;
    let t = (k - (k - b).rem_euclid(n)) * f;
    match lp {
        Some(l) => t.rem_euclid(l),
        None => t.max(0),
    }
}

/// Slow layers that read no `t` draw what the formula without `rate`
/// draws, frame for frame (Big clock's way).
#[test]
fn a_still_slow_layer_is_the_formula_without_rate() {
    let srcs = [
        "layer rate 50 { gray(digit(floor(px/8) % 10, px % 8, py % 8, bold)) }\nlayer multiply { hsv(x + t/10, 1, 1) }",
        "hh = hour/24\nlayer rate 4 { pal(fire, x*hh + y*a) }\nlayer add { rgb(0, 0, frac(x + t), 0.5) }",
        "layer rate 3 { gray(x) }\nlayer multiply rate 3 { pal(ice, y) }\nlayer screen { hsv(t, 1, x) }",
    ];
    for src in srcs {
        let a = compile(src).unwrap();
        assert!(a.rate().is_some(), "{src}\n{}", a.listing());
        let b = compile(
            &src.replace(" rate 50", "")
                .replace(" rate 4", "")
                .replace(" rate 3", ""),
        )
        .unwrap();
        assert!(b.rate().is_none());
        for ms in [0, 20, 40, 980, 1000, 4321, 7980] {
            let i = golden::inputs(ms);
            assert_eq!(frame_of(&a, &i), frame_of(&b, &i), "{src} at {ms}");
        }
    }
}

/// A slow layer that reads `t`: each row as the bottom layer alone draws
/// it at its band's time (`slow_ms`), under a top layer that changes
/// nothing; looping and forever, fps 50, 100 and 25, rates within and past
/// the 32 rows.
#[test]
fn a_moving_slow_layer_is_drawn_band_by_band() {
    for (head, n, f, lp) in [
        ("loop 2\n", 4i64, 20i64, Some(2000i64)),
        ("loop 3\nfps 100\n", 7, 10, Some(3000)),
        ("loop 1.01\nfps 25\n", 3, 40, Some(1010)),
        ("loop 4\n", 50, 20, Some(4000)),
        ("loop forever\n", 5, 20, None),
    ] {
        let bottom = "hsv(x + t/3, 1, frac(y + t))";
        let a = compile(&alloc::format!(
            "{head}layer rate {n} {{ {bottom} }}\nlayer max {{ gray(0*t) }}"
        ))
        .unwrap();
        let s = compile(&alloc::format!("{head}{bottom}")).unwrap();
        for ms in [0u64, 20, 60, 140, 990, 1000, 1010, 1990, 2020, 2990, 61_000] {
            let mut i = golden::inputs(0);
            i.set_ms(&a, ms);
            let got = frame_of(&a, &i);
            let lms = match lp {
                Some(l) => ms as i64 % l,
                None => ms as i64,
            };
            for r in 0..H {
                let mut j = i;
                j.set_ms(&s, slow_ms(n, f, lp, lms, r) as u64);
                let want = frame_of(&s, &j);
                assert_eq!(
                    &got[r * W * 3..(r + 1) * W * 3],
                    &want[r * W * 3..(r + 1) * W * 3],
                    "{head} rate {n} at {ms} ms, row {r}"
                );
            }
        }
    }
}

/// The held picture gives the frames drawn from nothing, whatever frames
/// the player skips, the clock and sliders moving, a loop's end.
#[test]
fn the_held_picture_draws_the_frames_drawn_from_nothing() {
    let srcs = [
        "loop 2\nlayer rate 4 { hsv(x + t/3, 1, frac(y + t)) }\nlayer add { gray(x*a*0.2) }",
        "loop 1.03\ns = second/60\nlayer rate 6 { pal(fire, frac(x + s + t)) }\nlayer screen { hsv(y + t, 1, 0.3) }",
        "loop 60\nlayer rate 50 { gray(digit(minute % 10, floor(px/8) % 8, py % 8, bold) + b*0) }\nlayer min { hsv(x + t/10, 1, 1) }",
        "loop forever\nlayer rate 3 { gray(frac(x + t/5 + seed)) }\nlayer multiply { pal(ice, y + frac(t)) }",
        "fps 100\nloop 2\nlayer rate 40 { cycle(fire, x, t/2) }\nlayer max { cycle(ice, y, t, 0.5) }",
    ];
    let mut held = Held::try_boxed().unwrap();
    for src in srcs {
        let p = compile(src).unwrap_or_else(|e| panic!("{}", e.render(src)));
        held.forget();
        let mut sc = Scratch::<128>::new();
        let mut out = alloc::vec![0u8; W * H * 3];
        let mut ms = 0u64;
        let mut i = golden::inputs(0);
        for step in 0..160u64 {
            // Mostly the next frame; now and then 2-9 frames on (the
            // governor), the clock and a slider moving.
            ms += 20 * (1 + (step * 7 % 11 == 0) as u64 * (step % 9));
            i.set_ms(&p, ms);
            i.second = ((ms / 1000) % 60) as u8;
            i.minute = ((ms / 3000) % 60) as u8;
            if step == 90 {
                i.a = 3 << 14;
                i.b = 1 << 15;
            }
            let mut changed = [0u32; H];
            render_held::<128>(
                &p,
                &mut sc,
                None,
                None,
                &mut held,
                &i,
                &OWN,
                &mut out,
                Some(&mut changed),
            );
            assert_eq!(out, frame_of(&p, &i), "{src} at {ms} ms (step {step})");
        }
    }
}

/// Racing the beam through a `rate` formula: the slow layers held, the
/// fast part raced over them in pair order — the bytes and changed groups
/// of [`render_held`]'s frame, each pair told once, in order, while the
/// rows still to come hold the frame before.
#[test]
fn a_rate_formula_races_its_fast_part_over_the_held_picture() {
    let mut srcs: alloc::vec::Vec<(String, String)> = [
        "loop 2\nlayer rate 4 { hsv(x + t/3, 1, frac(y + t)) }\nlayer add { gray(x*a*0.2) }",
        "loop 60\nlayer rate 50 { gray(digit(minute % 10, floor(px/8) % 8, py % 8, bold) + b*0) }\nlayer min { hsv(x + t/10, 1, 1) }",
        "fps 100\nloop 2\nlayer rate 40 { cycle(fire, x, t/2) }\nlayer max { cycle(ice, y, t, 0.5) }",
    ]
    .iter()
    .map(|s| (String::from("sketch"), String::from(*s)))
    .collect();
    srcs.extend(
        every_formula()
            .into_iter()
            .filter(|(_, src)| compile(src).is_ok_and(|p| p.rate().is_some())),
    );
    let mut n = 0;
    for (name, src) in srcs {
        let p = compile(&src).unwrap_or_else(|e| panic!("{}", e.render(&src)));
        let (mut held_a, mut held_b) = (Held::try_boxed().unwrap(), Held::try_boxed().unwrap());
        let (mut sa, mut sb) = (Scratch::<128>::new(), Scratch::<128>::new());
        let mut want_w = alloc::vec![0u32; W * H * 3 / 4];
        let mut got_w = alloc::vec![0u32; W * H * 3 / 4];
        let mut i = golden::inputs(0);
        for step in 0..24u64 {
            let ms = step * 20 + (step % 5) * 40;
            i.set_ms(&p, ms);
            i.second = ((ms / 1000) % 60) as u8;
            let want = words_as_bytes(&mut want_w);
            let before = want.to_vec();
            let mut want_ch = [0u32; H];
            render_held::<128>(
                &p,
                &mut sa,
                None,
                None,
                &mut held_a,
                &i,
                &OWN,
                want,
                Some(&mut want_ch),
            );
            let want = want.to_vec();
            let got = words_as_bytes(&mut got_w);
            assert_eq!(&got[..], &before[..]);
            held_b.update::<128>(&p, &mut sb, &i, &OWN);
            let mut got_ch = [0u32; H];
            let mut told = alloc::vec::Vec::new();
            let mut sink = |q: usize, out: &[u8], a: u32, b: u32| {
                let row = |r: usize, pic: &[u8]| pic[r * W * 3..(r + 1) * W * 3].to_vec();
                for r in 0..H / 2 {
                    let pic = if q != BEAM_START && r <= q {
                        &want[..]
                    } else {
                        &before[..]
                    };
                    assert!(
                        row(r, out) == row(r, pic) && row(r + H / 2, out) == row(r + H / 2, pic),
                        "{name} at {ms} ms: pair {r} when pair {q} was told"
                    );
                }
                told.push((q, a, b));
            };
            render_rows_beam::<128>(
                &p,
                &mut sb,
                None,
                None,
                &i,
                &OWN,
                Some(held_b.picture()),
                got,
                Some(&mut got_ch),
                &mut sink,
            );
            assert_eq!(&got[..], &want[..], "{name} at {ms} ms");
            assert_eq!(got_ch, want_ch, "{name} at {ms} ms: what changed");
            let want_told: alloc::vec::Vec<_> = [(BEAM_START, 0, 0)]
                .into_iter()
                .chain((0..H / 2).map(|q| (q, want_ch[q], want_ch[q + H / 2])))
                .collect();
            assert_eq!(told, want_told, "{name} at {ms} ms: the pairs told");
            n += 1;
        }
    }
    assert!(n >= 72, "only {n} frames raced");
}

/// The errors (E941-E947).
#[test]
fn rate_error_messages() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "layer { gray(x) }\nlayer rate 4 add { gray(y) }",
            "write the blend first",
            "—",
        ),
        (
            "layer { gray(x) }\nlayer add rate 4 { gray(y) }",
            "bottom layers only",
            "E941",
        ),
        (
            "layer rate 1 { gray(x) }\nlayer add { gray(y) }",
            "2 to 200",
            "E942",
        ),
        (
            "layer rate 2.5 { gray(x) }\nlayer add { gray(y) }",
            "2 to 200",
            "E942",
        ),
        (
            "layer rate 4 { gray(x) }\nlayer multiply rate 5 { gray(y) }\nlayer add { gray(t) }",
            "share one",
            "E943",
        ),
        (
            "layer rate 4 { gray(x, 0.5) }\nlayer add { gray(y) }",
            "must cover the whole panel",
            "E944",
        ),
        (
            "layer rate 4 { gray(ul) }\nlayer add { gray(y) }",
            "can't read",
            "E944",
        ),
        (
            "layer rate 4 { gray(x) }\nlayer add { gray(ur) }",
            "can't read",
            "E944",
        ),
        (
            "draw { dot(3, 3) }\nlayer rate 4 { gray(ink) }\nlayer add { gray(y) }",
            "can't read",
            "E944",
        ),
        ("layer rate 4 { gray(x) }", "add a layer above", "E947"),
    ];
    let mut bad = alloc::vec::Vec::new();
    for &(src, want, code) in cases {
        match compile(src) {
            Ok(_) => bad.push(alloc::format!("{code} {src:?}: compiled")),
            Err(e) if !e.msg.contains(want) => {
                bad.push(alloc::format!("{code} {src:?}: {}", e.msg))
            }
            Err(_) => {}
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

/// In steady play the held picture redraws a band a frame (`t`), or
/// nothing until the clock it reads ticks.
#[test]
fn the_held_picture_redraws_a_band_a_frame() {
    let mut held = Held::try_boxed().unwrap();
    let mut sc = Scratch::<128>::new();
    let p = compile("loop 2\nlayer rate 4 { hsv(x + t/3, 1, 1) }\nlayer add { gray(x) }").unwrap();
    let mut i = golden::inputs(0);
    i.set_ms(&p, 0);
    assert_eq!(held.update::<128>(&p, &mut sc, &i, &OWN), 32);
    for f in 1..20u64 {
        i.set_ms(&p, f * 20);
        assert_eq!(held.update::<128>(&p, &mut sc, &i, &OWN), 8, "frame {f}");
    }
    let p = compile("layer rate 50 { gray(second/60) }\nlayer add { gray(x*t) }").unwrap();
    held.forget();
    assert_eq!(held.update::<128>(&p, &mut sc, &i, &OWN), 32);
    for f in 1..20u64 {
        i.set_ms(&p, f * 20);
        assert_eq!(held.update::<128>(&p, &mut sc, &i, &OWN), 0);
    }
    i.second += 1;
    assert_eq!(held.update::<128>(&p, &mut sc, &i, &OWN), 32);
}

/// The slow layers' marker is the first pixel step (`slow_marker` looks
/// only there), whatever the fused runs and the runs before the fast part.
#[test]
fn the_slow_marker_is_the_first_pixel_step() {
    let srcs = [
        "layer rate 4 { pal(fire, (floor(x*4) + floor(y*2)) % 2 ? 0.2 : 0.8) }\nlayer add { gray(x*a) }",
        "layer rate 3 { gray(x) }\nlayer multiply rate 3 { pal(ice, y > 0.5 ? x : y) }\nlayer screen { cycle(fire, len(cx, cy), t) }",
        "loop forever\nlayer rate 5 { gray(frac(x + t/5 + seed)) }\nlayer multiply { pal(ice, y + frac(t)) }",
    ];
    for src in srcs {
        let p = compile(src).unwrap();
        let re = p.row_end as usize;
        assert_eq!(p.ops[re].code, code::SLOW, "{src}\n{}", p.listing());
        assert!(!p.ops[re + 1..p.len()].iter().any(|o| o.code == code::SLOW));
    }
}

// --- rebinding ----------------------------------------------------------------

/// A name may be set again (`v = v*2` reads the one before), a layer, a
/// part or a loop may set a name from outside (its own until it closes),
/// and `p.v` is the last `v` on the part's own lines: each the same program as written
/// with fresh names. Parts, tables, sprites, palettes and controls keep
/// their names (E203); a loop counter can't be set (E709).
#[test]
fn names_may_be_set_again() {
    let same = |a: &str, b: &str| {
        let pa = compile(a).unwrap_or_else(|e| panic!("{}", e.render(a)));
        let pb = compile(b).unwrap_or_else(|e| panic!("{}", e.render(b)));
        assert!(pa == pb, "{a:?} is not {b:?}");
    };
    same("v = x\nv = v*2\ngray(v)", "v = x*2\ngray(v)");
    same(
        "v = t\nv = v + x\nv = sin(v)\ngray(v)",
        "q = sin(t + x)\ngray(q)",
    );
    same(
        "v = x\nlayer { v = y\ngray(v) }\nlayer add { gray(v*0.5) }",
        "v = x\nlayer { q = y\ngray(q) }\nlayer add { gray(v*0.5) }",
    );
    same(
        "v = x\nlayer { v = v*y\ngray(v) }\nlayer add { gray(v) }",
        "v = x\nlayer { q = v*y\ngray(q) }\nlayer add { gray(v) }",
    );
    same(
        "fn p(a) {\n  v = a\n  v = v*0.5\n  v\n}\ngray(p(x))",
        "gray(x*0.5)",
    );
    same(
        "k = 3\ndraw { for k < 4 { dot(k, 1) } }\ngray(ink*k/4)",
        "k = 3\ndraw { for j < 4 { dot(j, 1) } }\ngray(ink*k/4)",
    );
    same(
        "draw { for k < 4 { v = k\nv = v*2\ndot(v, 1) } }\ngray(ink)",
        "draw { for k < 4 { dot(k*2, 1) } }\ngray(ink)",
    );
    same(
        " s = t\ns = s*2 \ngray(frac(s + x))",
        "s = t*2\ngray(frac(s + x))",
    );
    same("cycle = 2\ncycle = cycle*x\ngray(cycle)", "gray(2*x)");
    same(
        "loop forever\nseed = 0.25\nseed = seed*2\ngray(seed)",
        "loop forever\ngray(0.5)",
    );
    // Kinds that keep their names, and counters.
    for (src, want) in [
        (
            "table T = 1\nT = 2\ngray(T)",
            "`T` is already the name of a table (line 1)",
        ),
        (
            "v = 1\ntable v = 2\ngray(v)",
            "`v` is already the name of a value (line 1)",
        ),
        (
            "fn p(a) = a\nlayer { p = 2\ngray(p) }",
            "`p` is already the name of a function (line 1)",
        ),
        (
            "control s 0..1\ncontrol s 0..2\ngray(s)",
            "`s` is already the name of a control (line 1)",
        ),
        (
            "control s 0..1 = 0.5\ns = s*2\ngray(s)",
            "`s` is already the name of a control (line 1)",
        ),
        (
            "control s 0..1\ndraw { for s < 4 { dot(s, 1) } }\ngray(ink*s)",
            "`s` is already the name of a control (line 1)",
        ),
        (
            "draw { for k < 4 { k = 2\ndot(k, 1) } }\ngray(ink)",
            "`k` is a loop counter: it can't be set",
        ),
        (
            "draw { for k < 4 { for k < 2 { dot(k, 1) } } }\ngray(ink)",
            "`k` is already a loop counter here",
        ),
        ("v = v*2\ngray(v)", "unknown name `v`"),
    ] {
        let e = compile(src).unwrap_err();
        assert!(e.msg.contains(want), "{src:?}: {}", e.msg);
    }
}

// --- named palette entries, lines hidden by ink 0 ----------------------------

/// `P.name` is the fraction `pal` maps back to that colour, i/(n − 1) as
/// written (the same constant: the same program); `P.step` 1/(n − 1),
/// `P.n` the count; names are optional per colour.
#[test]
fn palette_entries_have_names() {
    let same = |a: &str, b: &str| {
        let pa = compile(a).unwrap_or_else(|e| panic!("{}", e.render(a)));
        let pb = compile(b).unwrap_or_else(|e| panic!("{}", e.render(b)));
        assert!(pa == pb, "{a:?} is not {b:?}");
    };
    let p = "palette road = black #000, #403, kerb #806, #ec0, sky #f48\n";
    same(
        &format!("{p}pal(road, road.kerb)"),
        &format!("{p}pal(road, 2/4)"),
    );
    same(
        &format!("{p}pal(road, road.sky - road.step*x)"),
        &format!("{p}pal(road, 4/4 - 1/4*x)"),
    );
    same(
        &format!("{p}gray(road.n/8 + road.black)"),
        &format!("{p}gray(5/8)"),
    );
    same(
        "palette p = a #000, b #fff\npal(p, p.b)",
        "palette p = #000, #fff\npal(p, 1)",
    );
    // In a function.
    same(
        "palette sea = deep #003, foam #fff\nfn q(x) = pal(sea, sea.foam*x)\nq(x)",
        "palette sea = #003, #fff\npal(sea, 1*x)",
    );
}

/// A line or dot of ink 0 draws nothing and walks nothing: lines hidden
/// that way leave the frame's steps to the lines after them.
#[test]
fn ink_zero_walks_nothing() {
    let draw = |src: &str| {
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
        out
    };
    // 40 hidden lines of 812 steps each (32,480: twice the frame's 16,384),
    // then a row of dots: all drawn.
    let hidden = draw("draw { for k < 40 { line(-512, 300, 300, -512, 0) }\nfor j < 128 { dot(j, 31) } }\ngray(ink)");
    let none = draw("draw { for j < 128 { dot(j, 31) } }\ngray(ink)");
    assert!(hidden == none);
    // Shown at one end: walked, as ever (the same picture as before).
    let a =
        draw("draw { line(0, 0, 127, 31, 0, 1); line(0, 31, 127, 0, 0); dot(3, 3, 0) }\ngray(ink)");
    let b = draw("draw { line(0, 0, 127, 31, 0, 1) }\ngray(ink)");
    assert!(a == b);
}

/// Every formula the tests know: the goldens' sources, the formulas here
/// and the web app's (bundled and gallery).
fn every_formula() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = golden::FRAMES
        .iter()
        .map(|&(n, s, _, _)| (n.to_string(), s.to_string()))
        .collect();
    v.dedup();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in [root.join("formulas"), root.join("../opendmd-web/formulas")] {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();
        for f in files
            .into_iter()
            .filter(|f| f.extension().is_some_and(|e| e == "psi"))
        {
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            v.push((name, std::fs::read_to_string(&f).unwrap()));
        }
    }
    v
}

/// Racing the beam (console-lessons §2.6): a frame drawn in the panel's
/// pair order is the frame drawn at once, byte for byte, with the same
/// changed groups; each pair is told once, in order, as soon as its two
/// rows hold the new frame and while the rows still to come hold the old
/// one; with the still cache kept and used, with the draw stage's canvas,
/// and without a compare.
#[test]
fn a_frame_raced_in_pair_order_is_the_frame_drawn_at_once() {
    let mut frames = 0;
    for (name, src) in every_formula() {
        let Ok(prog) = compile(&src) else { continue };
        let lp = prog.loop_ms().max(1000);
        for ms in [0, 777, lp / 2 + 13] {
            let before_in = golden::inputs(ms);
            let mut inputs = golden::inputs(ms + 40);
            inputs.set_ms(&prog, u64::from(ms + 40));
            // The picture before (word-aligned, as the player's canvas).
            let mut before_w = vec![0u32; W * H * 3 / 4];
            let before = words_as_bytes(&mut before_w);
            render_rows_ink::<LANES>(
                &prog,
                &mut Scratch::new(),
                Some(&mut Ink::boxed()),
                &before_in,
                &OWN,
                None,
                u32::MAX,
                before,
            );
            let before = before.to_vec();
            for still_on in [false, true] {
                // At once.
                let mut st = Still::new();
                let mut want_w = vec![0u32; W * H * 3 / 4];
                let want = words_as_bytes(&mut want_w);
                want.copy_from_slice(&before);
                let mut want_ch = [0u32; H];
                {
                    let s = if still_on && st.take(&prog) {
                        Some(&mut st)
                    } else {
                        None
                    };
                    render_rows_changed::<LANES>(
                        &prog,
                        &mut Scratch::new(),
                        Some(&mut Ink::boxed()),
                        s,
                        &inputs,
                        &OWN,
                        None,
                        u32::MAX,
                        want,
                        &mut want_ch,
                    );
                }
                // Raced: twice with the still cache (kept, then used).
                let mut st2 = Still::new();
                let mut s2 = if still_on && st2.take(&prog) {
                    Some(&mut st2)
                } else {
                    None
                };
                for pass in 0..1 + usize::from(s2.is_some()) {
                    let mut got_w = vec![0u32; W * H * 3 / 4];
                    let got = words_as_bytes(&mut got_w);
                    got.copy_from_slice(&before);
                    let mut got_ch = [0u32; H];
                    let mut told = Vec::new();
                    let mut sink = |p: usize, out: &[u8], a: u32, b: u32| {
                        let row = |r: usize, pic: &[u8]| pic[r * W * 3..(r + 1) * W * 3].to_vec();
                        // (A `rate` formula is drawn whole, then told; at the
                        // start, `BEAM_START`, every row still holds the old
                        // frame.)
                        let drawn = |q: usize| p != BEAM_START && q <= p;
                        for q in (0..H / 2).filter(|&q| drawn(q) || prog.rate().is_none()) {
                            let (top, bottom) = if drawn(q) {
                                (&want[..], &want[..])
                            } else {
                                (&before[..], &before[..])
                            };
                            assert!(
                                row(q, out) == row(q, top)
                                    && row(q + H / 2, out) == row(q + H / 2, bottom),
                                "{name} at {ms} ms: pair {q} when pair {p} was told"
                            );
                        }
                        told.push((p, a, b));
                    };
                    render_rows_beam::<LANES>(
                        &prog,
                        &mut Scratch::new(),
                        Some(&mut Ink::boxed()),
                        s2.as_deref_mut(),
                        &inputs,
                        &OWN,
                        None,
                        got,
                        Some(&mut got_ch),
                        &mut sink,
                    );
                    assert_eq!(
                        &got[..],
                        &want[..],
                        "{name} at {ms} ms, still {still_on} pass {pass}"
                    );
                    assert_eq!(got_ch, want_ch, "{name} at {ms} ms: what changed");
                    let want_told: Vec<_> = [(BEAM_START, 0, 0)]
                        .into_iter()
                        .chain((0..H / 2).map(|p| (p, want_ch[p], want_ch[p + H / 2])))
                        .collect();
                    assert_eq!(told, want_told, "{name} at {ms} ms: the pairs told");
                    frames += 1;
                }
            }
            // Without a compare: the bytes, every group said to change.
            let mut want = vec![0u8; W * H * 3];
            render_rows_still::<LANES>(
                &prog,
                &mut Scratch::new(),
                Some(&mut Ink::boxed()),
                None,
                &inputs,
                &OWN,
                None,
                u32::MAX,
                &mut want,
            );
            let mut got = vec![0u8; W * H * 3];
            let mut told = 0;
            let mut started = false;
            render_rows_beam::<LANES>(
                &prog,
                &mut Scratch::new(),
                Some(&mut Ink::boxed()),
                None,
                &inputs,
                &OWN,
                None,
                &mut got,
                None,
                &mut |p, _, a, b| {
                    if p == BEAM_START {
                        assert!(!started && told == 0);
                        started = true;
                        return;
                    }
                    assert_eq!((p, a, b), (told, u32::MAX, u32::MAX));
                    told += 1;
                },
            );
            assert_eq!(got, want, "{name} at {ms} ms, no compare");
            assert!(started);
            assert_eq!(told, H / 2);
        }
    }
    assert!(frames >= 200, "only {frames} frames raced");
}

fn words_as_bytes(w: &mut [u32]) -> &mut [u8] {
    // SAFETY: u32s as their bytes, the same memory.
    unsafe { core::slice::from_raw_parts_mut(w.as_mut_ptr().cast::<u8>(), w.len() * 4) }
}

#[test]
fn the_pair_order_is_the_panels() {
    let order: Vec<usize> = (0..H).map(pair_row).collect();
    assert_eq!(&order[..6], &[0, 16, 1, 17, 2, 18]);
    assert_eq!(&order[30..], &[15, 31]);
}

/// The fused compares' formulas (P0b-4): every comparison (`<`, `<=`, `>`,
/// `>=`, `step`) in every pattern the compiler fuses (`cmp * v`, `v *
/// cmp`, `cmp ? v : 0`, `cmp ? 0 : v`, `cmp + cmp` of one value against
/// two, the shared value on either side), per pixel (lanes and values for
/// the lane, ties included: whole numbers against whole numbers), per row
/// and per frame.
/// Each with whether it must fuse (two different comparisons of one value
/// need not count).
pub(crate) fn fused_compare_cases() -> alloc::vec::Vec<(alloc::string::String, bool)> {
    use alloc::format;
    let ops = ["<", "<=", ">", ">="];
    // (a, b, v): lanes, values for the lane, ties (floor(px/8) against py/2).
    let sides = [
        ("floor(px/8)", "py/2", "x*3 - 1"),
        ("x", "0.5", "y + 0.25"),
        ("t/4", "frac(px*0.07)", "2"),
        ("frac(px*y*0.3)", "frac(py*x*0.2)", "x - y"),
    ];
    let mut v = alloc::vec::Vec::new();
    for op in ops {
        for (a, b, w) in sides {
            let c = format!("({a} {op} {b})");
            for e in [
                format!("{c} * ({w})"),
                format!("({w}) * {c}"),
                format!("{c} ? {w} : 0"),
                format!("{c} ? 0 : {w}"),
            ] {
                v.push((format!("loop 8\nk = {e}\ngray(frac(k*0.37 + 0.5))"), true));
            }
            for op2 in ops {
                // The shared value first in both, second in both, and mixed.
                let flip = |o: &str| match o {
                    "<" => ">",
                    "<=" => ">=",
                    ">" => "<",
                    _ => "<=",
                };
                for (p, q, must) in [
                    (
                        format!("({a} {op} {b})"),
                        format!("({a} {op2} {w})"),
                        op == op2,
                    ),
                    (
                        format!("({b} {op} {a})"),
                        format!("({w} {op2} {a})"),
                        op == op2,
                    ),
                    (
                        format!("({a} {op} {b})"),
                        format!("({w} {op2} {a})"),
                        flip(op2) == op,
                    ),
                ] {
                    // (`t/4 op 2` is a frame's comparison: not fused into a
                    // pixel's count.)
                    let must = must && w != "2";
                    v.push((format!("loop 8\nk = {p} + {q}\ngray(k*0.4 + 0.1)"), must));
                }
            }
        }
        v.push((
            format!(
                "loop 8\nf = (t {op} 4) * 3\nr = (py {op} 12) * 2 + (py {op} 20) + (y {op} 0.5)\ngray(frac((f + r)*0.1 + x*(t {op} 2)))"
            ),
            true,
        ));
    }
    for (a, b) in [("floor(px/8)", "py/2"), ("x", "y")] {
        v.push((
            format!("loop 8\nk = step({a}, {b}) * (x + 1) + step({b}, {a})\ngray(k*0.3)"),
            true,
        ));
        v.push((
            format!("loop 8\nk = step({a}, {b}) + step({a}, x*0.5)\ngray(k*0.4)"),
            true,
        ));
    }
    v
}

#[test]
fn fused_compares_are_the_steps_they_replace() {
    use alloc::vec::Vec;
    let draw = |p: &Program, ms: u32, a: i32| {
        let mut out = alloc::vec![0u8; W * H * 3];
        let mut inp = golden::inputs(ms);
        inp.a = a;
        render_rows::<128>(
            p,
            &mut Scratch::new(),
            &inp,
            &OWN,
            None,
            u32::MAX,
            &mut out,
        );
        out
    };
    let mut kinds = [0usize; 6];
    let (mut musts, mut fused) = (0, 0);
    let cases = fused_compare_cases();
    for (src, must) in &cases {
        let mut with = Program::default();
        compile_into(src, &mut with).unwrap_or_else(|e| panic!("{}", e.render(src)));
        let mut without = Program::default();
        compile_measured(src, &mut without, true, true, false).unwrap();
        // The checked steps too (a frame out of range: `t` past the loop).
        let mut checked = Program::default();
        compile_with(src, &mut checked, true, false).unwrap();
        let n: Vec<u8> = with.ops[..with.len()]
            .iter()
            .map(|o| o.code)
            .filter(|&c| (code::CMP_FIRST..=code::CMP_LAST).contains(&c))
            .collect();
        for &c in &n {
            kinds[(c - code::CMP_FIRST) as usize] += 1;
        }
        // (One the column run or the still cache would take apart from the
        // step that reads it stays apart: once a column against a compare
        // a pixel.)
        if *must {
            musts += 1;
            fused += usize::from(!n.is_empty());
        }
        assert!(
            with.steps() + n.len() <= without.steps(),
            "{src}: {} steps fused, {} without\n{}",
            with.steps(),
            without.steps(),
            with.listing()
        );
        for ms in [0, 1999, 2000, 4000, 7321] {
            let want = draw(&without, ms, 1 << 15);
            assert!(
                draw(&with, ms, 1 << 15) == want,
                "{src} at {ms} ms\n{}",
                with.listing()
            );
            assert!(
                draw(&checked, ms, 1 << 15) == want,
                "{src} at {ms} ms (checked)"
            );
        }
    }
    assert!(kinds.iter().all(|&k| k > 0), "each kind made: {kinds:?}");
    assert!(fused * 2 >= musts, "only {fused} of {musts} fused");
}

/// The fused compares' values are the steps' they stand for, at the ends
/// of the range and at ties.
#[test]
fn fused_compares_are_exact() {
    let vals = [i32::MIN, -ONE, -1, 0, 1, ONE / 2, ONE, ONE + 1, i32::MAX];
    for &a in &vals {
        for &b in &vals {
            for &c in &vals {
                assert_eq!(fixed::sel_lt(a, b, c), fixed::mul(fixed::lt(a, b), c));
                assert_eq!(fixed::sel_le(a, b, c), fixed::mul(fixed::le(a, b), c));
                assert_eq!(
                    fixed::cnt_lt(a, b, c),
                    fixed::add(fixed::lt(a, b), fixed::lt(a, c))
                );
                assert_eq!(
                    fixed::cnt_le(a, b, c),
                    fixed::add(fixed::le(a, b), fixed::le(a, c))
                );
                assert_eq!(
                    fixed::cnt_gt(a, b, c),
                    fixed::add(fixed::gt(a, b), fixed::gt(a, c))
                );
                assert_eq!(
                    fixed::cnt_ge(a, b, c),
                    fixed::add(fixed::ge(a, b), fixed::ge(a, c))
                );
            }
        }
    }
}

// --- built-in inputs and the store (psb ABI 3) -----------------------------------

/// A built-in reads its default without a live value (what previews
/// show), a live value when the world has one, held to its range; a frame
/// value (no step per pixel); the slots it takes are the player's.
#[test]
fn built_in_inputs_read_their_defaults_and_live_values() {
    let p = compile("gray(daylight * x + replay.game * y)").unwrap();
    assert_eq!(p.ins.n, 2);
    assert!(p.listing().contains("daylight") && p.listing().contains("replay.game"));
    let at = |w: crate::World| frame_of(&p, &Inputs { world: w, ..Default::default() });
    // Defaults: daylight 1, replay.game 0.
    let d = at(crate::World::default());
    assert_eq!(d, frame_of(&compile("gray(1 * x + 0 * y)").unwrap(), &Inputs::default()));
    let mut w = crate::World::default();
    w.value[world::id::DAYLIGHT as usize] = q(0.25);
    w.value[world::id::REPLAY_GAME as usize] = q(0.5);
    assert_eq!(at(w), frame_of(&compile("gray(0.25 * x + 0.5 * y)").unwrap(), &Inputs::default()));
    // Out of range: held to it.
    w.value[world::id::DAYLIGHT as usize] = q(7.0);
    w.value[world::id::REPLAY_GAME as usize] = q(-3.0);
    assert_eq!(at(w), d);
    // Every key reads, by its name; one set as the formula's own is its.
    for k in &world::KEYS {
        let p = compile(&alloc::format!("gray({})", k.name)).unwrap_or_else(|e| panic!("{}: {}", k.name, e.msg));
        assert_eq!(p.builtins().next().map(|(key, what)| (key.id, what)), Some((k.id, world::VALUE)));
    }
    let own = compile("night = 0.5\ngray(night)").unwrap();
    assert_eq!(own.ins.n, 0);
    // A dotted input's group is no name of the formula's (language review
    // A2): not a value's, a palette's, a table's or a function's.
    for src in [
        "sun = 1\ngray(sun.alt)",
        "palette sun = alt #f80, #fff\npal(sun, 0.5)",
        "table moon = 1, 2\ngray(moon[0])",
        "fn replay(a) = a\ngray(replay(x))",
    ] {
        let e = compile(src).unwrap_err();
        assert!(e.msg.contains("is the built-in inputs' name"), "{src}: {}", e.msg);
    }
    // `inputs()`: bit 7, and a `since` moves with time (bit 3).
    assert_eq!(p.inputs() & 0x88, 0x80);
    assert_eq!(compile("gray(since(night))").unwrap().inputs() & 0x88, 0x88);
}

/// `since(KEY)` and `was(KEY)`: frame values the player fills, 32767 and
/// the default when the key never changed; `World::change` is a change.
#[test]
fn since_and_was_are_frame_values() {
    let p = compile("gray(min(since(replay.game), 1) * x + was(replay.game) * y)").unwrap();
    let mut w = crate::World::default();
    let at = |w: crate::World| frame_of(&p, &Inputs { world: w, ..Default::default() });
    assert_eq!(at(w), frame_of(&compile("gray(x)").unwrap(), &Inputs::default()));
    w.change(world::id::REPLAY_GAME, q(0.5));
    assert_eq!(w.since[world::id::REPLAY_GAME as usize], 0);
    w.change(world::id::REPLAY_GAME, q(0.75));
    w.since[world::id::REPLAY_GAME as usize] = q(0.5);
    assert_eq!(at(w), frame_of(&compile("gray(0.5 * x + 0.5 * y)").unwrap(), &Inputs::default()));
    // The same value again is no change.
    w.change(world::id::REPLAY_GAME, q(0.75));
    assert_eq!(w.since[world::id::REPLAY_GAME as usize], q(0.5));
    // A change's seconds are held at 32,766: 32,767 is "never changed"
    // (language review A5), however long ago the change was.
    let k = world::input_byte(world::id::REPLAY_GAME, world::SINCE);
    assert_eq!(crate::World::default().read(k), world::SINCE_NEVER);
    w.since[world::id::REPLAY_GAME as usize] = world::SINCE_NEVER - 1;
    assert_eq!(w.read(k), world::SINCE_HELD);
    w.since[world::id::REPLAY_GAME as usize] = i32::MAX;
    assert_eq!(w.read(k), world::SINCE_HELD);
    // One slot each: value, since and was of a key are three reads.
    let p = compile("gray(sun.alt + since(sun.alt) + was(sun.alt))").unwrap();
    assert_eq!(p.ins.n, 3);
    assert_eq!(compile("gray(since(daylight) + since(daylight))").unwrap().ins.n, 1);
}

/// The inputs travel in the psb (flag bit 2) and the player checks them:
/// a known key, each once, in order, in slots of their own no step writes.
#[test]
fn input_slots_survive_the_psb_and_the_verifier() {
    let src = "loop forever\nv = noise(t*0.1, y, 0)\ngray(v * daylight + seed * was(night) + since(replay.game) * 0)";
    let full = compile(src).unwrap();
    let b = crate::build(src).unwrap();
    let mut p = crate::psb::load_boxed(&b, crate::psb::Which::Play).unwrap();
    assert_eq!(p.ins, full.ins);
    assert!(p.ins.base as usize > crate::program::S_FIRST_FREE as usize);
    let bad = |f: &dyn Fn(&mut Prog)| {
        let mut q = crate::psb::load_boxed(&b, crate::psb::Which::Play).unwrap();
        // (Its portable form: the verifier's.)
        crate::psb::load_portable(&b, crate::psb::Which::Play, &mut q).unwrap();
        f(&mut q);
        assert!(crate::psb::verify(&q).is_err());
    };
    bad(&|q| q.ins.keys[0] = 63);
    bad(&|q| q.ins.keys[0] |= 3 << 6);
    bad(&|q| q.ins.base = 3);
    bad(&|q| q.ins.n = 0);
    bad(&|q| q.ins.keys.swap(0, 1));
    bad(&|q| {
        let s = q.ins.base;
        let k = (0..q.len()).find(|&k| crate::program::code::writes_scalar(q.ops[k].code) && !crate::program::is_lane(q.ops[k].d)).unwrap();
        q.ops[k].d = s;
    });
    // Frames are the same through the psb.
    let mut w = crate::World::default();
    w.change(world::id::NIGHT, ONE);
    let inputs = Inputs { world: w, seed: q(0.3), ..golden::inputs(1000) };
    assert_eq!(frame_of(&p, &inputs), frame_of(&full, &inputs));
    p.clear();
}

/// The player's program holds the store its formula needs, a power of two
/// (a formula with no tables 4 numbers: ~1 KB less than before ABI 3), and
/// draws what the whole program draws; a store too small is refused.
#[test]
fn a_prog_holds_the_store_its_formula_needs() {
    for (name, src) in crate::psb::fuzz::sources() {
        let b = crate::build(&src).unwrap();
        let p = crate::psb::load_boxed(&b, crate::psb::Which::Play).unwrap_or_else(|e| panic!("{name}: {e}"));
        let full = compile(&src).unwrap();
        assert_eq!(p.store_len(), store_words(full.used()), "{name}");
        assert!(*p == *full, "{name}");
        let inputs = golden::inputs(1500);
        assert!(frame_of(&p, &inputs) == frame_of(&full, &inputs), "{name}");
    }
    assert_eq!(store_words(0), 4);
    assert_eq!(store_words(5), 8);
    assert_eq!(store_words(1024), 1024);
    let b = crate::build("table T = 0.1, 0.2, 0.3, 0.4, 0.5\ngray(T[x*5])").unwrap();
    let mut small = Prog::try_boxed(4).unwrap();
    assert_eq!(
        crate::psb::load_into(&b, crate::psb::Which::Play, &mut small),
        Err(crate::psb::SMALL_STORE)
    );
    assert_eq!(crate::psb::store_needed(&b, crate::psb::Which::Play), Ok(5));
    assert!(core::mem::size_of_val(&*small) < core::mem::size_of::<Program>() - 4000);
}

/// The store past 256 numbers: tables and sprites placed after 900
/// numbers read right per frame, per row and per pixel, through the psb's
/// smaller store too.
#[test]
fn the_store_reaches_past_256_numbers() {
    let filler: alloc::vec::Vec<alloc::string::String> = (0..250).map(|k| alloc::format!("{}", k % 7)).collect();
    let mut src = alloc::string::String::new();
    for k in 0..3 {
        src += &alloc::format!("table F{k} = {}, {k}.5\n", filler.join(", "));
    }
    src += "table T = 0.1, 0.5, 0.9\nsprite s {\nkey . 0, X 1, Y 0.5\nXY.\n.YX\n}\n";
    src += "gray(T[px % 3] * 0.5 + s[px % 3, py % 2] * 0.5 + F2[250] * 0.01 + F0[second] * 0.01 + F1[py % 3] * 0.01)";
    let p = compile(&src).unwrap();
    assert!(p.used() > 760, "{}", p.used());
    let want = "gray(pick(px % 3, 0.1, 0.5, 0.9) * 0.5 + pick(px % 3 + (py % 2)*3, 1, 0.5, 0, 0, 0.5, 1) * 0.5 + 2.5 * 0.01 + 2 * 0.01 + pick(py % 3, 0, 1, 2) * 0.01)";
    let inputs = golden::inputs(0);
    assert_eq!(frame_of(&p, &inputs), frame_of(&compile(want).unwrap(), &inputs));
    let b = crate::build(&src).unwrap();
    let q = crate::psb::load_boxed(&b, crate::psb::Which::Play).unwrap();
    assert_eq!(q.store_len(), 1024);
    assert_eq!(frame_of(&q, &inputs), frame_of(&p, &inputs));
}

/// Language review A1: `/` gives the same value at every level (the spec:
/// "levels change only the cost, never the result"). Each pair below is
/// one value worked out once a frame (or a row) and the same value worked
/// out per pixel; their frames are the same bytes all through the loop.
#[test]
fn division_is_the_same_at_every_level() {
    let pairs = [
        // The review's pair: `k/3` once a frame against per pixel.
        (
            "loop 4\nk = floor(t*50) % 128\nv = k/3\ngray(frac(v))",
            "loop 4\nk = floor(t*50) % 128 + (px > 200)\nv = k/3\ngray(frac(v))",
        ),
        // A negative dividend (truncation towards zero, not a floor).
        (
            "loop 2\nv = (t - 1 - 0.0000153)/2\ngray(0.5 + v * 1000)",
            "loop 2\nv = (t - 1 - 0.0000153 + (px > 200))/2\ngray(0.5 + v * 1000)",
        ),
        // Large and fractional constant divisors, and a slider's.
        (
            "loop 2\nk = t*1000\ngray(frac(k/21846*1000) + frac(k/3000) + frac(k/2.5) + frac(k/(a*3+0.7)))",
            "loop 2\nk = t*1000 + (px > 200)\ngray(frac(k/21846*1000) + frac(k/3000) + frac(k/2.5) + frac(k/(a*3+0.7)))",
        ),
        // By a per-row value that is 0 on some rows: the ends of the
        // numerator's sign, as once a row.
        (
            "loop 2\ns = floor(y*2)\nv = (t*0.001 - 0.0005)/s\ngray(frac(v))",
            "loop 2\ns = floor(y*2)\nv = (t*0.001 - 0.0005 + (px > 200))/s\ngray(frac(v))",
        ),
    ];
    for (once, per_pixel) in pairs {
        let (p, q) = (compile(once).unwrap(), compile(per_pixel).unwrap());
        assert!(q.listing().contains("div_s"), "{}", q.listing());
        for ms in (0..2000).step_by(20) {
            let i = Inputs { t: Inputs::at_ms(ms), a: q16(1, 3), ..Default::default() };
            let (a, b) = (frame_of(&p, &i), frame_of(&q, &i));
            // (The per-pixel one differs only past column 200: none here.)
            assert!(a == b, "{once:?} at {ms} ms");
        }
    }
    // `px / 21846` is `px / 21846`, not `px / 32768` (it was a shift).
    let p = compile("gray(px/21846*1000)").unwrap();
    let f = frame_of(&p, &Inputs::default());
    let want = |px: i32| fixed::chan(fixed::mul(fixed::div(px << 16, 21846 << 16), 1000 << 16));
    for px in [0usize, 63, 127] {
        assert_eq!(f[px * 3], want(px as i32), "px {px}");
    }
    assert!(f[127 * 3] > 0);
}

/// Language review A2: one name rule. A formula's own value wins wherever
/// the name is read as a value; a call `name(…)` is always the built-in
/// function (`since`, `was`, `under`, `ink` as `sin`); a `fn` takes no
/// built-in function's name nor a value's; `since(k)` of a name the formula
/// set is refused, not read two ways.
#[test]
fn one_name_rule() {
    let mut pic = alloc::vec![0u8; W * H * 3];
    golden::picture(&mut pic);
    let draw = |p: &Prog| {
        let mut out = alloc::vec![0u8; W * H * 3];
        render_rows::<128>(p, &mut Scratch::new(), &golden::inputs(500), &OWN, Some(&pic), u32::MAX, &mut out);
        out
    };
    let same = |a: &str, b: &str| {
        let (p, q) = (compile(a).unwrap(), compile(b).unwrap());
        assert!(p.counts() == q.counts() && draw(&p) == draw(&q), "{a} vs {b}");
    };
    same("sin = 0.5\ngray(sin(y) + sin)", "gray(sin(y) + 0.5)");
    same("since = 1\ngray(since(replay.game) * 0 + since)", "gray(since(replay.game) * 0 + 1)");
    same("was = 0.25\ngray(was(night) + was)", "gray(was(night) + 0.25)");
    let (a, b) = (
        compile("ink = 0.5\ndraw { dot(1, 1, 1) }\ngray(ink(4) + ink)").unwrap(),
        compile("draw { dot(1, 1, 1) }\ngray(ink(4) + 0.5)").unwrap(),
    );
    assert_eq!(a.counts(), b.counts());
    assert!(a.listing().contains("inkn") && frame_of(&a, &Inputs::default()) == frame_of(&b, &Inputs::default()));
    same("under = 0.5\nrgb(under(px + 1, py) * under)", "rgb(under(px + 1, py) * 0.5)");
    for (src, says) in [
        ("fn sin(a) = a*2\ngray(sin(x))", "is a built-in function"),
        ("fn since(a) = a\ngray(since(x))", "is a built-in function"),
        ("fn under(a, b) = a\ngray(under(x, y))", "is a built-in function"),
        ("fn line(a) = a\ngray(line(x))", "is a built-in function"),
        ("f = 3\nfn f(a) = a\ngray(f(x))", "is already the name of a value"),
        ("fn f(a) = a\nf = 3\ngray(f(x))", "is already the name of a function"),
        ("daylight = 0.5\ngray(since(daylight))", "is this formula's own value here"),
        ("night = 1\ngray(was(night))", "is this formula's own value here"),
        ("moon = 1\ngray(moon)", "is the built-in inputs' name"),
        ("fire = 0.5\ngray(fire)", "is a palette's name"),
    ] {
        let e = compile(src).unwrap_err();
        assert!(e.msg.contains(says), "{src}: {}", e.msg);
    }
    // The built-ins are still read where the formula has not set the name.
    same("gray(since(daylight) * 0 + daylight)", "gray(since(daylight) * 0 + daylight)");
}

/// Language review A4: `a < b < c` is refused, with the `and` to write.
#[test]
fn chained_comparisons_are_refused() {
    for (src, says) in [
        ("gray(0 < x < 0.5)", "write `0 < x and x < 0.5`"),
        ("gray(x >= 0.2 <= y)", "write `x >= 0.2 and 0.2 <= y`"),
        ("v = px + 1 == py == 3\ngray(v)", "write `px + 1 == py and py == 3`"),
    ] {
        let e = compile(src).unwrap_err();
        assert!(e.msg.contains(says), "{src}: {}", e.msg);
    }
    // Brackets say what is meant, and `and` joins two.
    compile("gray((0 < x) < 0.5)").unwrap();
    compile("gray(0 < x and x < 0.5)").unwrap();
}

/// Language review B6: the misleading E304 and `under` sentences.
#[test]
fn colour_errors_say_what_went_wrong() {
    for (src, says) in [
        ("gray(under(px, py))", "`under(x, y)` is a colour, not a number"),
        ("x > 0.5 ? pal(fire, x) : #000", "can't be combined with others by `+`, `*`, `? :`"),
        ("mix(#a00, #00b, x) * pal(fire, x)", "can't be combined with others"),
    ] {
        let e = compile(src).unwrap_err();
        assert!(e.msg.contains(says), "{src}: {}", e.msg);
    }
}

/// Language review B1: a note where a sum or product can pass the number
/// range and goes into something that cares; none where it only meets a
/// clamp, a compare or a colour.
#[cfg(feature = "syntax")]
#[test]
fn saturation_is_noted_where_it_matters() {
    struct N(Vec<String>);
    impl crate::syntax::Sink for N {
        fn note(&mut self, _: (usize, usize), text: &str) {
            self.0.push(text.into());
        }
    }
    let notes = |src: &str| -> Vec<String> {
        let mut n = N(Vec::new());
        let mut p = Program::default();
        crate::syntax::compile_observed(src, &mut p, &mut n).unwrap();
        n.0
    };
    let has = |src: &str| notes(src).iter().any(|m| m.contains("can pass ±32,768"));
    assert!(has("j = px\ngray(frac(127.5*j*j/2500/1000))"));
    assert!(has("loop 3600\nv = t*t\ngray(frac(v))"));
    assert!(!has("gray(x*30000)"));
    assert!(!has("gray(px*px*px > 5)"));
    assert!(!has("gray(frac(px*py*0.1))"));
}

/// A slot read without a `control` line plays held to 0..1 (`&a=30` from a
/// script left the JIT's kernel waiting forever for inputs in range); a
/// declared one is the formula's to clamp.
#[test]
fn an_undeclared_slot_is_held_to_0_1() {
    let one = crate::fixed::ONE;
    let p = crate::compile("gray(a)").unwrap();
    let m = p.meta();
    assert_eq!(m.values([Some(30 * one), Some(-one), None]), [one, 0, 0]);
    let p = crate::compile("control k 0..40 = 2\ngray(k / 40)").unwrap();
    assert_eq!(p.meta().value(0, Some(30 * one)), 30 * one);
}
