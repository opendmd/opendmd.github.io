//! Endless road V2's sweep (docs/research/endless-road-v2.md §12.1, §13):
//! each frame's own cost (`psi check --ms`'s figure) at 24 points of the
//! cycle × 3 seeds × by day and by night (the sun's phase picked by the
//! cycle the point is taken from), summed by place.
//!
//!     cargo run --release --example road_sweep -- ../opendmd-web/experiments/road-v2/road-v2.psi [-v]
//!
//! `OFF=0.5` takes each point half a step on (the points (k + ½)/24 of the
//! cycle; 0 by default: k/24); `SEEDS=0.3,0.5` other seeds; `-v` a line a
//! frame (its ms, place and scenes' rows).
use opendmd_psi::{compile, fixed, scene_rows, Inputs, Scratch, OWN};

fn q(v: f64) -> i32 {
    (v * 65536.0).round() as i32
}

/// `hash(a, b)` as the Formula has it, 0..1.
fn h(a: f64, b: f64) -> f64 {
    f64::from(fixed::hash(q(a), q(b))) / 65536.0
}

fn main() {
    let path = std::env::args().nth(1).expect("road_sweep FILE.psi [-v]");
    let verbose = std::env::args().any(|a| a == "-v");
    let src = std::fs::read_to_string(&path).expect("reading the Formula");
    let prog = compile(&src).unwrap_or_else(|e| panic!("{}", e.render(&src)));
    let mut scratch = Scratch::<128>::new();
    let seeds: Vec<f64> = match std::env::var("SEEDS") {
        Ok(v) => v.split(',').map(|x| x.parse().expect("SEEDS")).collect(),
        _ => vec![0.0, 1.0 / 3.0, 2.0 / 3.0],
    };
    let off: f64 = std::env::var("OFF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let mut rows: Vec<(&str, f64, f64)> = Vec::new();
    for &seed in &seeds {
        let hs = h(seed, 7.0);
        for i in 0..24 {
            let p = (f64::from(i) + off) / 24.0;
            for night in [false, true] {
                // t = (p - seed - 0.125 + k)·324: the k whose `light` is
                // the most day (or night).
                let mut best = (0.0, f64::MAX);
                for k in 1..80 {
                    let t = (p - seed - 0.125 + f64::from(k)) * 324.0;
                    if t < 0.0 {
                        continue;
                    }
                    let sp = (std::f64::consts::TAU * (t / 240.0 + hs)).sin();
                    let light = (sp * sp * sp * -3.0).clamp(-1.0, 1.0);
                    let score = if night { light } else { -light };
                    if score < best.1 {
                        best = (t, score);
                    }
                }
                let t = best.0;
                let ms = (t * 1000.0).round() as u64;
                // The Formula's own clock and boundaries.
                let cyc = t / 324.0 + seed + 0.125;
                let cn = cyc.floor();
                let tc = cyc.fract() * 324.0 - 40.0;
                let tdc = tc - h(cn, seed + 1.0) * 20.0;
                let tct = tc - h(cn, seed + 2.0) * 20.0;
                let ttf = tc - h(cn, seed + 3.0) * 20.0;
                let tfd = tc - h(cn, seed + 4.0) * 20.0;
                let place = if (170.0..178.0).contains(&tct) {
                    "mouth"
                } else if (200.0..206.0).contains(&ttf) {
                    "exit"
                } else if tct >= 170.0 && ttf < 206.0 {
                    "tunnel"
                } else if (65.0..95.0).contains(&tdc) && tct < 178.0 {
                    "desert -> city"
                } else if (-40.0..-10.0).contains(&tfd) {
                    "forest -> desert"
                } else if tdc >= 95.0 && tct < 170.0 {
                    "city"
                } else if ttf >= 206.0 || tfd < -40.0 {
                    "forest"
                } else {
                    "desert"
                };
                let mut inp = Inputs {
                    seed: q(seed),
                    ..Inputs::default()
                };
                inp.set_ms(&prog, ms);
                let ran = scene_rows(&prog, &mut scratch, &inp, &OWN);
                let jit = f64::from(prog.tier_us_frame(false, &ran)) / 1000.0;
                let vm = f64::from(prog.vm_us_frame(false, &ran)) / 1000.0;
                if verbose {
                    println!(
                        "seed {seed:.4} point {p:.3} {} ms {ms} {place}: {jit:.2} / {vm:.2} ms, rows {:?}",
                        if night { "night" } else { "day" },
                        &ran.rows[..8]
                    );
                }
                rows.push((place, jit, vm));
            }
        }
    }
    let over = rows.iter().filter(|r| r.1 > 6.5).count();
    let worst = rows
        .iter()
        .copied()
        .fold(("", 0.0, 0.0), |a, r| if r.1 > a.1 { r } else { a });
    println!(
        "static: ~{:.1} ms JIT / ~{:.1} VM; {} steps",
        f64::from(prog.tier_us(false)) / 1000.0,
        f64::from(prog.esp32_us_still()) / 1000.0,
        prog.steps()
    );
    println!(
        "sweep: {} frames, {over} over 6.5 ms JIT; the worst {:.2} ms JIT ({:.2} VM) at the {}",
        rows.len(),
        worst.1,
        worst.2,
        worst.0
    );
    let mut places: Vec<&str> = rows.iter().map(|r| r.0).collect();
    places.sort_unstable();
    places.dedup();
    println!("| place | frames | JIT | VM | over 6.5 |");
    for pl in places {
        let rs: Vec<_> = rows.iter().filter(|r| r.0 == pl).collect();
        let lo = |f: fn(&(&str, f64, f64)) -> f64| rs.iter().map(|r| f(r)).fold(f64::MAX, f64::min);
        let hi = |f: fn(&(&str, f64, f64)) -> f64| rs.iter().map(|r| f(r)).fold(0.0, f64::max);
        println!(
            "| {pl} | {} | {:.1}-{:.1} | {:.1}-{:.1} | {} |",
            rs.len(),
            lo(|r| r.1),
            hi(|r| r.1),
            lo(|r| r.2),
            hi(|r| r.2),
            rs.iter().filter(|r| r.1 > 6.5).count()
        );
    }
}
