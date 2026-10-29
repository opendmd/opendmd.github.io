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
/// measurements predate the narrowed ones, `range`).
fn compile(src: &str) -> Result<opendmd_psi::Program, opendmd_psi::Error> {
    let mut p = opendmd_psi::Program::default();
    opendmd_psi::compile_with(src, &mut p, false, false)?;
    Ok(p)
}
use opendmd_psi::look::compose;

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
        let p = compile(&compose(src, blend, k).unwrap()).unwrap();
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
    let est_of = |src: &str| {
        let mut p = opendmd_psi::Program::default();
        opendmd_psi::compile_into(src, &mut p).unwrap();
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
            if opendmd_psi::compile_into(&std::fs::read_to_string(&p).unwrap(), &mut prog).is_err() {
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
        rows.push(format!("{name:<30} measured {us:>5} est {est:>5} ({:+.0} %)", (r - 1.0) * 100.0));
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
        let mut p = opendmd_psi::Program::default();
        opendmd_psi::compile_into(&compose(src, blend, k).unwrap(), &mut p).unwrap();
        check(format!("look {name} {blend} {k}"), p.esp32_us_jit(), us);
    }
    println!("{}", rows.join("\n"));
    assert!(bad.is_empty(), "{bad:?}\n{}", rows.join("\n"));
}
