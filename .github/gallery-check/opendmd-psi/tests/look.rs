//! Any Formula as a look (`opendmd_psi::look`): the composed source, that
//! it compiles to what it should, and every blend at 0, 50 and 100 % over
//! the golden picture — checked pixel by pixel against the layer formulas
//! (`fixed::blend`, `fixed::cover`) applied to the formula's own frame, and
//! as golden hashes, here and in wasm (`make test-wasm`).

use opendmd_psi::fixed::{blend, cover};
use opendmd_psi::golden::{fnv, inputs, picture};
use opendmd_psi::look::{compose, BLENDS};
use opendmd_psi::{compile, render_rows, Scratch, H, OWN, W};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// The web app's Plasma: covers every pixel, reads nothing under it.
const PLASMA: &str = "loop 8\n-- three waves added together\nwave = sin(x*2 + t/8) + sin(y*1.5 - t/4) + sin(x*1.5 + y + t/8)\nhsv(wave/6 + t/8, 0.9, 1)";
/// Old photo: reads the picture under it (draws over a GIF already).
const OLD_PHOTO: &str = "-- a: how much (0: the GIF as it is)\nmix(rgb(ur, ug, ub), rgb(ul*1.1 + 0.08, ul*0.92 + 0.04, ul*0.65), a)";
/// Scanlines: a layer of its own (goes over only).
const SCANLINES: &str =
    "-- a: how bright the dim rows are\nlayer multiply { gray(py % 2 ? 1 : 0.3 + a*0.6) }";
/// A formula with a loop of its own (not 8 s).
const FIRE: &str =
    "loop 4\nheat = (1 - y)*1.6 - 0.5 + sin(x*6 + t)*0.3\npal(fire, clamp(heat, 0, 0.95))";

fn frame(src: &str, ms: u32, under: &[u8]) -> Vec<u8> {
    let p = compile(src).unwrap_or_else(|e| panic!("{}", e.render(src)));
    let mut out = vec![0x5a; W * H * 3];
    render_rows::<128>(
        &p,
        &mut Scratch::new(),
        &inputs(ms),
        &OWN,
        Some(under),
        u32::MAX,
        &mut out,
    );
    let mut b = vec![0xa5; W * H * 3];
    render_rows::<64>(
        &p,
        &mut Scratch::new(),
        &inputs(ms),
        &OWN,
        Some(under),
        u32::MAX,
        &mut b,
    );
    assert!(out == b, "lane widths differ for {src:?}");
    out
}

fn under() -> Vec<u8> {
    let mut u = vec![0u8; W * H * 3];
    picture(&mut u);
    u
}

#[test]
fn composes_as_written() {
    assert_eq!(
        compose(PLASMA, "screen", 40).unwrap(),
        "part look {\n  loop 8\n  -- three waves added together\n  wave = sin(x*2 + t/8) + sin(y*1.5 - t/4) + sin(x*1.5 + y + t/8)\n  hsv(wave/6 + t/8, 0.9, 1)\n}\nlayer screen { look(x, y, 0.4) }"
    );
    assert_eq!(
        compose(FIRE, "over", 30).unwrap(),
        "loop 4\npart look {\n  loop 4\n  heat = (1 - y)*1.6 - 0.5 + sin(x*6 + t)*0.3\n  pal(fire, clamp(heat, 0, 0.95))\n}\nlook(x, y, 0.3)"
    );
    assert!(compose(PLASMA, "multiply", 100)
        .unwrap()
        .ends_with("}\nlayer multiply { look }"));
    assert!(compose(PLASMA, "add", 0)
        .unwrap()
        .ends_with("}\nlayer add { look(x, y, 0) }"));
    assert!(compose(PLASMA, "max", 5)
        .unwrap()
        .ends_with("}\nlayer max { look(x, y, 0.05) }"));
    assert!(
        compose(PLASMA, "min", 250)
            .unwrap()
            .ends_with("}\nlayer min { look }"),
        "clamped to 100"
    );
    assert!(compose(SCANLINES, "over", 50)
        .unwrap()
        .ends_with("}\nlook(x, y, 0.5)"));
    assert!(compose("loop 0.5\ngray(t)", "screen", 60)
        .unwrap()
        .starts_with("loop 0.5\npart look {"));
    // One that never repeats: so does its look, the same picture.
    let src = "loop forever\ngray(frac(t*0.15 + x))";
    let look = compose(src, "screen", 40).unwrap();
    assert!(look.starts_with("loop forever\npart look {"), "{look}");
    let p = opendmd_psi::compile(&look).unwrap();
    assert!(p.forever());
    assert!(compose(PLASMA, "lighten", 40).is_err(), "not a blend");
    assert!(compose("gray(", "screen", 40).is_err(), "does not compile");
}

#[test]
fn over_at_full_is_the_formula_itself() {
    // Today's looks are unchanged: the same text, so the same program.
    for src in [OLD_PHOTO, SCANLINES, PLASMA] {
        assert_eq!(compose(src, "over", 100).unwrap(), src);
    }
    // And the formula as a part draws the same picture as the formula.
    let u = under();
    let wrapped = format!("part look {{\n{OLD_PHOTO}\n}}\nlook");
    assert_eq!(frame(&wrapped, 0, &u), frame(OLD_PHOTO, 0, &u));
}

#[test]
fn every_composition_compiles_to_a_look() {
    for src in [PLASMA, OLD_PHOTO, FIRE] {
        for b in BLENDS {
            for k in [0, 1, 40, 50, 99, 100] {
                let s = compose(src, b, k).unwrap();
                let p = compile(&s).unwrap_or_else(|e| panic!("{b} {k}: {}", e.render(&s)));
                // Each one lets the GIF show through — except over at 100 %
                // of a formula that covers it (the player refuses that one).
                let covers = b == "over" && k == 100 && src != OLD_PHOTO;
                assert_eq!(p.over(), !covers, "{b} {k} {src:.20}");
                assert_eq!(
                    p.loop_ms(),
                    compile(src).unwrap().loop_ms(),
                    "the formula's loop"
                );
            }
        }
    }
    // Plasma, screened at 40 %: what the panel is asked to draw.
    let p = compile(&compose(PLASMA, "screen", 40).unwrap()).unwrap();
    assert_eq!(p.layers(), 1);
    // (~8.2 ms: the spare measured it at 8.55 ms as a look, a 12 ms limit;
    // ~6.7 ms once its waves of x are worked out once a column; ~5.9 with
    // the narrowed steps.)
    assert!(
        (5_000..12_000).contains(&p.esp32_us()),
        "{} µs",
        p.esp32_us()
    );
    // A formula with layers goes over only.
    let e = compile(&compose(SCANLINES, "screen", 40).unwrap()).unwrap_err();
    assert!(e.msg.contains("can only go over"), "{}", e.msg);
}

/// Every blend at 0, 50 and 100 % over the golden picture: Plasma (covers
/// it) and Old photo (reads it), each pixel the layer formulas applied to
/// the formula's own frame — and the frames' hashes.
#[test]
fn blends_over_a_picture() {
    const MODES: [(&str, u8); 6] = [
        ("over", 0),
        ("add", 1),
        ("multiply", 2),
        ("screen", 3),
        ("max", 4),
        ("min", 5),
    ];
    // (formula, blend, amount, hash) at 2,500 ms, sliders as `golden::inputs`.
    const GOLDEN: &[(&str, &str, u8, u64)] = &[
        ("plasma", "over", 0, 0x2c7a29e300e4c8cf),
        ("plasma", "over", 50, 0xc6a9ef044e60eb1a),
        ("plasma", "over", 100, 0xbbc2773807384e51),
        ("plasma", "screen", 0, 0x2c7a29e300e4c8cf),
        ("plasma", "screen", 50, 0x87649c42479ef570),
        ("plasma", "screen", 100, 0xc7e158d09c1b1f7c),
        ("plasma", "add", 0, 0x2c7a29e300e4c8cf),
        ("plasma", "add", 50, 0x35d23d5328797d87),
        ("plasma", "add", 100, 0xd2fbd0005f839515),
        ("plasma", "multiply", 0, 0x2c7a29e300e4c8cf),
        ("plasma", "multiply", 50, 0xdbaebd6cb3bfc64a),
        ("plasma", "multiply", 100, 0x2f0469545486a40c),
        ("plasma", "max", 0, 0x2c7a29e300e4c8cf),
        ("plasma", "max", 50, 0xac07343ad7d1e460),
        ("plasma", "max", 100, 0xc5f5f4ecd2716a1a),
        ("plasma", "min", 0, 0x2c7a29e300e4c8cf),
        ("plasma", "min", 50, 0xe5a81052fd2d3ce5),
        ("plasma", "min", 100, 0xfce047abebe9d6e8),
        ("old photo", "over", 0, 0x2c7a29e300e4c8cf),
        ("old photo", "over", 50, 0xbfd560f644101946),
        ("old photo", "over", 100, 0x59e09958fe87f58c),
        ("old photo", "screen", 0, 0x2c7a29e300e4c8cf),
        ("old photo", "screen", 50, 0x6646e179f6e0d9bc),
        ("old photo", "screen", 100, 0xcea0655c7fd210cb),
        ("old photo", "add", 0, 0x2c7a29e300e4c8cf),
        ("old photo", "add", 50, 0xbf5fa92a0d132489),
        ("old photo", "add", 100, 0x7984f6123f67c512),
        ("old photo", "multiply", 0, 0x2c7a29e300e4c8cf),
        ("old photo", "multiply", 50, 0x4a88503289445e71),
        ("old photo", "multiply", 100, 0x1e99d66ed7d75f3c),
        ("old photo", "max", 0, 0x2c7a29e300e4c8cf),
        ("old photo", "max", 50, 0x6414fccae0b69b47),
        ("old photo", "max", 100, 0x75e2ddcef6b57272),
        ("old photo", "min", 0, 0x2c7a29e300e4c8cf),
        ("old photo", "min", 50, 0xeb66f928ee69747e),
        ("old photo", "min", 100, 0x3532f289c5b671a1),
    ];
    let u = under();
    let mut bad = Vec::new();
    for &(name, b, k, want) in GOLDEN {
        let src = if name == "plasma" { PLASMA } else { OLD_PHOTO };
        let own = frame(src, 2500, &u);
        let got = frame(&compose(src, b, k).unwrap(), 2500, &u);
        let mode = MODES.iter().find(|m| m.0 == b).unwrap().1;
        let a = i32::from(k) * 65536 / 100;
        for (i, (&g, (&d, &s))) in got.iter().zip(u.iter().zip(&own)).enumerate() {
            let r = cover(d, blend(mode, d, s), a);
            assert!(
                g == r,
                "{name} {b} {k}%: byte {i} is {g}, the blend says {r} (d {d}, s {s})"
            );
        }
        if k == 0 {
            assert!(got == u, "{name} {b} at 0 %: the picture as it is");
        }
        let h = fnv(&got);
        if h != want {
            bad.push(format!("        (\"{name}\", \"{b}\", {k}, {h:#x}),"));
        }
    }
    assert!(
        bad.is_empty(),
        "look frames differ (got):\n{}",
        bad.join("\n")
    );
}
