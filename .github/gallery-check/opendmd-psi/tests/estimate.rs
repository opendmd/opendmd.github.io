//! `Program::esp32_us` against what the device measured (`psi-probe` on
//! the spare, the VM's frame p50 over a picture: tests/measured.rs). Every
//! row here is compiled without the column run and the still cache's run
//! (`compile_plain`) and without the narrowed steps, as it was when
//! measured.
//!
//! - `STEPS` are the fit set: the per-step and blend costs were read off
//!   them, so they sit near 0 % by construction; this keeps a later change
//!   to the table from drifting off them. Every row is held to ±15 %.
//! - `DRAW` are the draw stage's rows (the fit set for `Program::draw_us`);
//!   `EFFECTS` the two drawing examples played as effects, a hold-out
//!   measured on the real path (so compared without the ×1.15).
//! - `LOOKS` are the hold-out: examples composed as looks, never fitted;
//!   they are what says the model generalises (all within ±12 % when
//!   written). Held to ±15 %.
//! - The ×1.15 for the real paths (the effect's copy, the look's rows) is
//!   applied to the measurements here exactly as the estimate applies it,
//!   so it is **not** tested by this file: it rests on two device readings
//!   (plasma as an effect 8,217 vs 7,254 µs; Plasma screened at 40 % as a
//!   look 8,552 vs 7,474), in `Program::esp32_us`'s comments.

mod measured;

/// Compiled as measured: no runs, and the checked steps only (the
/// measurements predate the narrowed ones, `range`), each comparison a
/// step (they predate the fused compares, P0b-4).
fn compile(src: &str) -> Result<opendmd_psi::Program, opendmd_psi::Error> {
    let mut p = opendmd_psi::Program::default();
    opendmd_psi::compile_measured(src, &mut p, false, false, false)?;
    Ok(p)
}
/// `src` as a look, `blend` at `k` %, compiled as measured.
fn compose(src: &str, blend: &str, k: u8) -> opendmd_psi::Program {
    let mut p = opendmd_psi::Program::default();
    opendmd_psi::compile_look_with(
        &opendmd_psi::look::set(src, blend, k).unwrap(),
        &mut p,
        false,
        false,
    )
    .unwrap();
    p
}

#[test]
fn the_estimate_is_within_15_percent_of_the_device() {
    let mut bad = Vec::new();
    let mut rows = Vec::new();
    for &(name, src, us) in measured::STEPS {
        let p = compile(src).unwrap();
        let est = p.esp32_us();
        let r = est as f64 / (us as f64 * 1.15);
        rows.push(format!(
            "{name:<26} measured {us:>5} est {est:>5} ({:+.0} %)",
            (r - 1.0) * 100.0
        ));
        if !(0.85..=1.15).contains(&r) {
            bad.push(name.to_string());
        }
    }
    for &(name, src, blend, k, us) in measured::LOOKS {
        let p = compose(src, blend, k);
        let est = p.esp32_us();
        let r = est as f64 / (us as f64 * 1.15);
        rows.push(format!(
            "look {name} {blend} {k:<3} measured {us:>5} est {est:>5} ({:+.0} %)",
            (r - 1.0) * 100.0
        ));
        if !(0.85..=1.15).contains(&r) {
            bad.push(format!("{name} {blend} {k}"));
        }
    }
    // (Measured with every loop a turn at a time: against that estimate.
    // Loops of 32 turns and more run as lanes since psi-speed2.)
    for &(name, src, us) in measured::DRAW {
        let p = compile(src).unwrap();
        let est = p.esp32_us_turns();
        let r = est as f64 / (us as f64 * 1.15);
        rows.push(format!(
            "{name:<30} measured {us:>5} est {est:>5} ({:+.0} %)",
            (r - 1.0) * 100.0
        ));
        if !(0.85..=1.15).contains(&r) {
            bad.push(name.to_string());
        }
    }
    // Draw loops as lanes: what each loop adds over the one-dot base.
    // (Without the fused compares: the rows predate them.)
    let est_of = |src: &str| {
        let mut p = opendmd_psi::Program::default();
        opendmd_psi::compile_measured(src, &mut p, true, true, false).unwrap();
        p.esp32_us_still() as f64
    };
    let base = est_of("draw { dot(-1, -1, t) }\ngray(ink)");
    for &(name, src, us) in measured::DRAW_LANES {
        let est = est_of(src) - base;
        let r = est / ((us - measured::DRAW_LANES_BASE) as f64 * 1.15);
        rows.push(format!(
            "{name:<30} measured {:>5} est {est:>5} over the base ({:+.0} %)",
            us - measured::DRAW_LANES_BASE,
            (r - 1.0) * 100.0
        ));
        if !(0.85..=1.15).contains(&r) {
            bad.push(name.to_string());
        }
    }
    // The examples as compiled now (VM frames, s2probe-a's speed2 rows).
    for &(name, src, us) in measured::SPEED2 {
        let est = est_of(src) as u32;
        let r = est as f64 / (us as f64 * 1.15);
        rows.push(format!(
            "{name:<30} measured {us:>5} est {est:>5} ({:+.0} %)",
            (r - 1.0) * 100.0
        ));
        if !(0.85..=1.15).contains(&r) {
            bad.push(name.to_string());
        }
    }
    // As effects: measured on the real path, so against the estimate as is.
    for &(name, src, us) in measured::EFFECTS {
        let p = compile(src).unwrap();
        let est = p.esp32_us_turns();
        let r = est as f64 / us as f64;
        rows.push(format!(
            "effect {name:<23} measured {us:>5} est {est:>5} ({:+.0} %)",
            (r - 1.0) * 100.0
        ));
        if !(0.85..=1.15).contains(&r) {
            bad.push(format!("effect {name}"));
        }
    }
    println!("{}", rows.join("\n"));
    assert!(
        bad.is_empty(),
        "outside ±15 %: {bad:?}\n{}",
        rows.join("\n")
    );
}

/// The JIT's estimate is never above the VM's (the player keeps whichever
/// draws a frame faster), with and without the still cache, for every
/// bundled formula; and it is the VM's for none of the ones the device
/// measured faster compiled.
#[test]
fn the_jit_estimate_is_never_above_the_vms() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut n = 0;
    for dir in [root.join("formulas"), root.join("../opendmd-web/formulas")] {
        for f in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()) {
            let p = f.path();
            if p.extension().is_none_or(|e| e != "psi") {
                continue;
            }
            let mut prog = opendmd_psi::Program::default();
            if opendmd_psi::compile_into(&std::fs::read_to_string(&p).unwrap(), &mut prog).is_err()
            {
                continue;
            }
            assert!(prog.esp32_us_jit() <= prog.esp32_us(), "{p:?}");
            assert!(prog.esp32_us_jit_still() <= prog.esp32_us_still(), "{p:?}");
            n += 1;
        }
    }
    assert!(n > 30);
}

/// The JIT's estimate against the spare's JIT frames (jit2-c): within
/// ±20 % of each (the governor measures anyway; the estimate is the
/// editor's label and the governor's first frame).
#[test]
fn the_jit_estimate_is_within_20_percent_of_the_device() {
    let mut rows = Vec::new();
    let mut bad = Vec::new();
    let mut check = |name: String, est: u32, us: u32| {
        let r = est as f64 / us as f64;
        rows.push(format!(
            "{name:<30} measured {us:>5} est {est:>5} ({:+.0} %)",
            (r - 1.0) * 100.0
        ));
        if !(0.8..=1.2).contains(&r) {
            bad.push(name);
        }
    };
    for &(name, src, us) in measured::JIT_EFFECTS {
        let mut p = opendmd_psi::Program::default();
        opendmd_psi::compile_into(src, &mut p).unwrap();
        check(name.to_string(), p.esp32_us_jit_still(), us);
    }
    for &(name, src, blend, k, us) in measured::JIT_LOOKS {
        let p = opendmd_psi::look::compose(src, blend, k).unwrap();
        check(format!("look {name} {blend} {k}"), p.esp32_us_jit(), us);
    }
    println!("{}", rows.join("\n"));
    assert!(bad.is_empty(), "{bad:?}\n{}", rows.join("\n"));
}

/// `cycle` and `layer rate N` (console-b): the estimate the editor and the
/// governor start from against the spare's frames, within ±20 %.
#[test]
fn the_cycled_and_slow_formulas_estimate_their_frames() {
    let mut rows = Vec::new();
    let mut bad = Vec::new();
    for &(name, src, us) in measured::CONSOLEB {
        let mut p = opendmd_psi::Program::default();
        opendmd_psi::compile_into(src, &mut p).unwrap();
        let est = p.esp32_us_jit_still();
        let r = est as f64 / us as f64;
        rows.push(format!(
            "{name:<34} measured {us:>5} est {est:>5} ({:+.0} %)",
            (r - 1.0) * 100.0
        ));
        if !(0.8..=1.2).contains(&r) {
            bad.push(name);
        }
    }
    println!("{}", rows.join("\n"));
    assert!(bad.is_empty(), "{bad:?}\n{}", rows.join("\n"));
}

/// The paint primitives' draw stage, the overlay, `under(x, y)` and the
/// Formulas that use them against paint-probe's rows on the spare
/// (2026-11-04): each primitive's draw stage within 15 % of `draw_us`
/// (the fit set, three sizes a kind), the `UAT` looks and the Formulas
/// within 15 % (the VM; the JIT's within 20 %).
#[test]
fn the_paint_and_under_estimates_fit_the_spare() {
    // (Compiled as the player has them: with the runs and the narrowed
    // steps, as `psi build` made the probe's cases.)
    let mut bad = Vec::new();
    let mut rows = Vec::new();
    let mut row = |what: String, us: u32, est: u32, tol: f64| {
        let r = est as f64 / us as f64;
        rows.push(format!("{what:<30} measured {us:>6} est {est:>6} ({:+.0} %)", (r - 1.0) * 100.0));
        if !(1.0 - tol..=1.0 + tol).contains(&r) {
            bad.push(what);
        }
    };
    for &(name, body, us) in measured::PAINT {
        let src = format!("{}draw {{\n  for k < 16 {{\n    {body}\n  }}\n}}\ngray(x)\n", measured::PAINT_SPRITE);
        let p = opendmd_psi::compile(&src).unwrap();
        row(format!("paint {name}"), us, p.draw_us(), 0.15);
    }
    // The overlay: what the plane adds to the rows, empty and full.
    let [none, empty, full] = measured::OVERLAY_ROWS;
    let painter = opendmd_psi::compile("draw {\n  pset(-9, -9, #f00)\n}\ngray(x)\n").unwrap();
    let clearer = opendmd_psi::compile("draw {\n  cls(#f00)\n}\ngray(x)\n").unwrap();
    // (Half-way for a painter without `cls`: its plane is partly painted.)
    row("overlay, a painter".into(), (empty + full) / 2 - none, painter.overlay_us(), 0.15);
    row("overlay, cls".into(), full - none, clearer.overlay_us(), 0.15);
    for &(name, src, us) in measured::UNDER {
        let p = opendmd_psi::compile(src).unwrap();
        // (Measured directly: a look's real path adds ~5 %, as pixelate's
        // and under-shake's rows show.)
        row(format!("under {name}"), us * 105 / 100, p.esp32_us(), 0.15);
    }
    for &(name, src, vm, jit) in measured::PAINT_FORMULAS {
        let p = opendmd_psi::compile(src).unwrap();
        row(format!("formula {name} (vm)"), vm, p.esp32_us(), 0.15);
        row(format!("formula {name} (jit)"), jit, p.esp32_us_jit(), 0.20);
    }
    println!("{}", rows.join("\n"));
    assert!(bad.is_empty(), "outside: {bad:?}\n{}", rows.join("\n"));
}

/// Scenes (psi-p0, P0b-1): the estimate is the steps outside scenes and the
/// two costliest scenes — a third as dear changes nothing, a cheaper one
/// nothing either, and two scenes cost about two of their steps' worth.
#[test]
fn the_estimate_counts_two_scenes() {
    let scene = |k: u32| format!("scene s{k} when frac(t/8) > {k}/8 {{\n  v{k} = sin(x*{k}.3 + y)*noise(x*7, y*{k}, t)\n}}\n");
    let est = |n: u32, extra: &str| {
        let mut src = String::from("loop 8\n");
        for k in 1..=n {
            src += &scene(k);
        }
        src += extra;
        let sum: Vec<String> = (1..=n).map(|k| format!("v{k}")).collect();
        src += &format!("gray({})", sum.join(" + "));
        let p = opendmd_psi::compile(&src).unwrap_or_else(|e| panic!("{}", e.render(&src)));
        (p.esp32_us(), p.esp32_us_jit())
    };
    let (two, two_jit) = est(2, "");
    let (three, three_jit) = est(3, "");
    let (four, _) = est(4, "");
    // (The extra scenes' sums are steps outside them: a step a scene.)
    assert!(three <= two + two / 20 && four <= two + two / 10, "{two} {three} {four}");
    assert!(three_jit <= two_jit + two_jit / 10, "{two_jit} {three_jit}");
    let (one, _) = est(1, "");
    assert!(two > one + one / 2, "{one} {two}");
}

/// P0b-4's fused compares and the guards a row against the spare
/// (`measured::P0B4`): each frame's own estimate (`tier_us_frame` with the
/// JIT, against the direct kernel frame; `vm_us_frame` without, against
/// the direct VM frame ×1.15 as the rows above) within ±15 % for the
/// guards and Endless road V2; for the fused compares' rows (a dozen
/// steps, where the fixed costs weigh), what fusing changes, est against
/// measured, within 0.6-1.6×.
#[test]
fn the_fused_compares_and_guards_as_measured() {
    use opendmd_psi::{golden, scene_rows, Inputs, Scratch, OWN};
    let road = include_str!("../../opendmd-web/experiments/road-v2/road-v2.psi");
    let mut bad = Vec::new();
    let mut rows = Vec::new();
    let mut ests = Vec::new();
    for &(name, body, fused, start, vm_us, jit_us) in measured::P0B4 {
        let src = if body == "road" {
            road.to_string()
        } else if name.starts_with("guards") && name != "guards base" {
            measured::p0b4_guards(body)
        } else {
            format!("{}{body}\n", measured::P0B4_U)
        };
        let mut p = opendmd_psi::Program::default();
        opendmd_psi::compile_measured(&src, &mut p, true, true, fused).unwrap();
        let mut inp: Inputs = golden::inputs(0);
        inp.set_ms(&p, u64::from(start));
        let ran = scene_rows(&p, &mut Scratch::<128>::new(), &inp, &OWN);
        let (ej, ev) = (p.tier_us_frame(false, &ran), p.vm_us_frame(false, &ran));
        let (rj, rv) = (ej as f64 / jit_us as f64, ev as f64 / (vm_us as f64 * 1.15));
        rows.push(format!(
            "{name:<30} jit {jit_us:>5} est {ej:>5} ({:+.0} %)  vm {vm_us:>5} est {ev:>5} ({:+.0} %)",
            (rj - 1.0) * 100.0,
            (rv - 1.0) * 100.0
        ));
        ests.push((ej as f64 / 1.15, ev as f64 / 1.15));
        let whole = name.starts_with("road") || (name.starts_with("guards") && name != "guards base");
        if whole && (!(0.85..=1.15).contains(&rj) || !(0.85..=1.15).contains(&rv)) {
            bad.push(name.to_string());
        }
    }
    // The fused rows against their unfused twins: what fusing saves.
    for k in [0, 2] {
        let (f, u) = (measured::P0B4[k], measured::P0B4[k + 1]);
        for (tier, m, e) in [
            ("jit", f.5 as f64 - u.5 as f64, ests[k].0 - ests[k + 1].0),
            ("vm", f.4 as f64 - u.4 as f64, ests[k].1 - ests[k + 1].1),
        ] {
            let r = e / m;
            rows.push(format!("{} fusing, {tier}: measured {m:+.0} µs, est {e:+.0}", f.0));
            if !(0.6..=1.6).contains(&r) {
                bad.push(format!("{} {tier}", f.0));
            }
        }
    }
    println!("{}", rows.join("\n"));
    assert!(bad.is_empty(), "outside: {bad:?}\n{}", rows.join("\n"));
}
