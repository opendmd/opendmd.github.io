//! Host timing of the sketches: `cargo run --release --example bench [--
//! FRAMES] [--list] [--ppm DIR]`. Prints ns per frame at lane widths 64 and
//! 128, the step counts (frame / row / pixel) and the registers used;
//! `--list` the step listings, `--ppm DIR` frame 0 and a mid-loop frame of
//! each as PPM images. Then the compile time of the worst formulas a
//! panel accepts (`opendmd_psi::worst`), 200 compiles each, and the cost
//! of layers and of reading what is under (`golden::picture`), per pixel
//! over `gray(x)` and as a multiple of one `add`.
//! A formula that reads what is under it is drawn over `golden::picture`.

use opendmd_psi::{
    compile, compile_into, golden, render_rows_ink, sketches, worst, Inputs, Program, Scratch, H,
    OWN, W,
};
use std::hint::black_box;
use std::time::Instant;

fn bench<const N: usize>(p: &Program, frames: u32) -> (f64, f64) {
    let mut s = Scratch::<N>::new();
    let mut ink = opendmd_psi::Ink::boxed();
    let mut out = vec![0u8; W * H * 3];
    let mut under = vec![0u8; W * H * 3];
    golden::picture(&mut under);
    let under = p.over().then_some(under.as_slice());
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
        render_rows_ink::<N>(
            black_box(p),
            &mut s,
            Some(&mut ink),
            &inputs,
            &OWN,
            under,
            u32::MAX,
            &mut out,
        );
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
            "{name:<12} {:>5} {:>14} {:>5} {:>9.1} µs {:>9.1} µs {:>9.1} µs {:>9.1} µs   (compile {:.1} µs, ESP32 est {} µs)",
            p.len(),
            format!("{f}/{r}/{px}"),
            p.regs(),
            a50 / 1e3,
            a90 / 1e3,
            b50 / 1e3,
            b90 / 1e3,
            ct.as_secs_f64() * 1e6,
            p.esp32_us()
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
                let mut under = vec![0u8; W * H * 3];
                golden::picture(&mut under);
                let u = p.over().then_some(under.as_slice());
                render_rows_ink::<128>(
                    &p,
                    &mut s,
                    Some(&mut opendmd_psi::Ink::boxed()),
                    &inputs,
                    &OWN,
                    u,
                    u32::MAX,
                    &mut out,
                );
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
    // Layers and what is under: ns a pixel over `gray(x)` (p50 of the
    // frame), and that as a multiple of one per-pixel `add`.
    let base = bench::<128>(&compile("gray(x)").unwrap(), frames).0;
    let add = bench::<128>(&compile("gray(x + px)").unwrap(), frames).0 - base;
    let px = (W * H) as f64;
    println!(
        "\nper pixel over gray(x) ({:.2} ns): one add = {:.2} ns",
        base / px,
        add / px
    );
    for (what, src) in [
        ("add", "gray(x + px)"),
        ("ur (one channel)", "gray(ur)"),
        ("ul (luminance)", "gray(ul)"),
        ("rgb(ur, ug, ub)", "rgb(ur, ug, ub)"),
        ("bottom shows through", "gray(x, 0.5)"),
        ("+ layer over, cover 0.5", "layer { gray(x) }\nlayer { gray(px, 0.5) }"),
        ("+ layer add", "layer { gray(x) }\nlayer add { gray(px) }"),
        ("+ layer multiply", "layer { gray(x) }\nlayer multiply { gray(px) }"),
        ("+ layer screen", "layer { gray(x) }\nlayer screen { gray(px) }"),
        ("+ layer max", "layer { gray(x) }\nlayer max { gray(px) }"),
        ("+ layer add, hsv", "layer { gray(x) }\nlayer add { hsv(px, 1, 1) }"),
        ("+ layer add, cover lane", "layer { gray(x) }\nlayer add { gray(px, x) }"),
        ("4 layers over the GIF", "layer multiply { gray(y) }\nlayer add { gray(x) }\nlayer screen { gray(px) }\nlayer max { gray(py) }"),
    ] {
        let p = compile(src).unwrap();
        let t = bench::<128>(&p, frames).0 - base;
        // The `px` lanes and the second gray: the layer's own cost is
        // what is left after the bottom `gray(x)` (in `base`).
        println!(
            "  {what:<26} {:>6.2} ns/px  = {:>4.1} adds  ({} steps, {} layers{})",
            t / px,
            t / add,
            p.len(),
            p.layers(),
            if p.over() { ", over" } else { "" }
        );
    }
    let mut p = Program::default();
    for (name, f) in worst::ALL {
        let src = f();
        let t0 = Instant::now();
        let mut r = Ok(Default::default());
        for _ in 0..200 {
            r = compile_into(black_box(&src), &mut p);
        }
        let us = t0.elapsed().as_secs_f64() * 1e6 / 200.0;
        let what = match r {
            Ok(_) => format!("{} steps, est {} µs", p.len(), p.esp32_us()),
            Err(e) => e.msg,
        };
        println!(
            "compile {name:<15} {} B source, {} B path: {us:.1} µs ({what})",
            src.len(),
            worst::path_len(&src)
        );
    }
}
