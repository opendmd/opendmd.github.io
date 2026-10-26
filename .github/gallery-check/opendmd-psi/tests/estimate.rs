//! `Program::esp32_us` against what the device measured (`psi-probe` on
//! the spare, the VM's frame p50 over a picture: tests/measured.rs).
//!
//! - `STEPS` are the fit set: the per-step and blend costs were read off
//!   them, so they sit near 0 % by construction; this keeps a later change
//!   to the table from drifting off them. Every row is held to ±15 %.
//! - `LOOKS` are the hold-out: examples composed as looks, never fitted;
//!   they are what says the model generalises (all within ±12 % when
//!   written). Held to ±15 %.
//! - The ×1.15 for the real paths (the effect's copy, the look's rows) is
//!   applied to the measurements here exactly as the estimate applies it,
//!   so it is **not** tested by this file: it rests on two device readings
//!   (plasma as an effect 8,217 vs 7,254 µs; Plasma screened at 40 % as a
//!   look 8,552 vs 7,474), in `Program::esp32_us`'s comments.

mod measured;
use opendmd_psi::compile;
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
    println!("{}", rows.join("\n"));
    assert!(
        bad.is_empty(),
        "outside ±15 %: {bad:?}\n{}",
        rows.join("\n")
    );
}
