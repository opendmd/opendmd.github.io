//! Host timing of the sketches: `cargo run --release --example bench [--
//! FRAMES] [--list] [--ppm DIR]`. Prints ns per frame at lane widths 64 and
//! 128, the step counts (frame / row / pixel) and the registers used;
//! `--list` the step listings, `--ppm DIR` frame 0 and a mid-loop frame of
//! each as PPM images.

use opendmd_psi::{compile, render_with, sketches, Inputs, Program, Scratch, H, OWN, W};
use std::hint::black_box;
use std::time::Instant;

fn bench<const N: usize>(p: &Program, frames: u32) -> (f64, f64) {
    let mut s = Scratch::<N>::new();
    let mut out = vec![0u8; W * H * 3];
    let loop_ms = p.loop_ms();
    let mut times = Vec::with_capacity(frames as usize);
    for f in 0..frames {
        let inputs = Inputs {
            t: Inputs::at_ms(f * 20 % loop_ms),
            hour: 10,
            minute: 9,
            second: 30,
            ..Default::default()
        };
        let t0 = Instant::now();
        render_with::<N>(black_box(p), &mut s, &inputs, &OWN, &mut out);
        times.push(t0.elapsed().as_nanos() as f64);
        black_box(&out);
    }
    times.sort_by(f64::total_cmp);
    (times[times.len() / 2], times[times.len() * 9 / 10])
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let frames: u32 = args.iter().find_map(|a| a.parse().ok()).unwrap_or(2000);
    let list = args.iter().any(|a| a == "--list");
    let ppm = args
        .iter()
        .position(|a| a == "--ppm")
        .and_then(|i| args.get(i + 1));
    let extra: Vec<(String, String)> = args
        .iter()
        .filter(|a| a.ends_with(".psi"))
        .map(|f| (f.clone(), std::fs::read_to_string(f).expect("read")))
        .collect();
    let all = sketches::ALL
        .iter()
        .map(|(n, s)| (n.to_string(), s.to_string()))
        .chain(extra);
    println!(
        "{:<12} {:>5} {:>14} {:>5} {:>12} {:>12} {:>12} {:>12}",
        "sketch", "steps", "frame/row/px", "regs", "N=64 p50", "N=64 p90", "N=128 p50", "N=128 p90"
    );
    for (name, src) in all {
        let t0 = Instant::now();
        let p = match compile(&src) {
            Ok(p) => p,
            Err(e) => {
                println!("{name}: {}", e.render(&src));
                continue;
            }
        };
        let ct = t0.elapsed();
        let (f, r, px) = p.counts();
        let (a50, a90) = bench::<64>(&p, frames);
        let (b50, b90) = bench::<128>(&p, frames);
        println!(
            "{name:<12} {:>5} {:>14} {:>5} {:>9.1} µs {:>9.1} µs {:>9.1} µs {:>9.1} µs   (compile {:.1} µs)",
            p.len(),
            format!("{f}/{r}/{px}"),
            p.regs(),
            a50 / 1e3,
            a90 / 1e3,
            b50 / 1e3,
            b90 / 1e3,
            ct.as_secs_f64() * 1e6
        );
        if list {
            print!("{}", p.listing());
        }
        if let Some(dir) = ppm {
            let mut s = Scratch::<128>::new();
            let mut out = vec![0u8; W * H * 3];
            for (k, ms) in [0, p.loop_ms() / 3].into_iter().enumerate() {
                let inputs = Inputs {
                    t: Inputs::at_ms(ms),
                    hour: 10,
                    minute: 9,
                    ..Default::default()
                };
                render_with::<128>(&p, &mut s, &inputs, &OWN, &mut out);
                let mut f = format!("P6\n{W} {H}\n255\n").into_bytes();
                f.extend_from_slice(&out);
                let base = std::path::Path::new(&name)
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                std::fs::write(format!("{dir}/{base}-{k}.ppm"), f).expect("write ppm");
            }
        }
    }
}
