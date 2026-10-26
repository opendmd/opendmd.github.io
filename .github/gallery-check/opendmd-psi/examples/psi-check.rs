//! Check a candidate formula the way a person (or a model) writing one would
//! want: does it compile — and if not, the sentence, the line and carets —,
//! how heavy it is for the panel, what it reads, and a PNG contact sheet of
//! its frames. The feedback loop `docs/psi-for-models.md` describes.
//!
//!     make psi-check F=my.psi [ARGS="--frames 8 --out my.png --a 0.5"]
//!     cargo run --release --example psi-check -- my.psi --out my.png
//!     echo 'gray(x)' | cargo run --release --example psi-check -- -
//!
//! Options: `--out FILE.png` (default: the formula's name with .png, or
//! `formula.png` from stdin; `--out -` for none), `--frames N` (8, spread
//! over one loop; a still formula gets 1), `--ms 0,250,…` (exact times
//! instead), `--scale K` (4), `--a`/`--b`/`--c V` (the sliders, 0 to 1;
//! default 0.5), `--clock HH:MM:SS` (10:09:30), `--over` (draw over the
//! sample picture even if the formula doesn't read it; it is used anyway
//! whenever it does), `--hash` (each frame's FNV-1a, as the golden tests
//! print them), `--listing` (the compiled steps). Exit status 1 when it
//! doesn't compile.

use opendmd_psi::golden::fnv;
use opendmd_psi::{compile, q16, render_rows, Inputs, Scratch, H, MAX_OPS, OWN, REGS, W};
use std::io::Read;

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut path, mut out, mut frames, mut scale) =
        (None::<String>, None::<String>, 8usize, 4usize);
    let (mut ms_list, mut over, mut hash, mut listing) = (None::<Vec<u32>>, false, false, false);
    let mut sl = [0.5f64; 3];
    let mut clock = (10u8, 9u8, 30u8);
    let num = |v: Option<String>, what: &str| -> f64 {
        v.and_then(|s| s.parse().ok())
            .unwrap_or_else(|| die(&format!("{what} needs a number")))
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = args.next(),
            "--frames" => frames = num(args.next(), "--frames").max(1.0) as usize,
            "--scale" => scale = num(args.next(), "--scale").clamp(1.0, 16.0) as usize,
            "--a" => sl[0] = num(args.next(), "--a"),
            "--b" => sl[1] = num(args.next(), "--b"),
            "--c" => sl[2] = num(args.next(), "--c"),
            "--ms" => {
                let v = args.next().unwrap_or_default();
                ms_list = Some(
                    v.split(',')
                        .map(|s| {
                            s.trim()
                                .parse()
                                .unwrap_or_else(|_| die("--ms takes 0,250,…"))
                        })
                        .collect(),
                );
            }
            "--clock" => {
                let v = args.next().unwrap_or_default();
                let p: Vec<u8> = v.split(':').filter_map(|s| s.parse().ok()).collect();
                if p.len() != 3 || p[0] > 23 || p[1] > 59 || p[2] > 59 {
                    die("--clock takes HH:MM:SS");
                }
                clock = (p[0], p[1], p[2]);
            }
            "--over" => over = true,
            "--hash" => hash = true,
            "--listing" => listing = true,
            "-h" | "--help" => {
                println!("psi-check FILE.psi|- [--out FILE.png|-] [--frames N] [--ms 0,250,…] [--scale K] [--a V] [--b V] [--c V] [--clock HH:MM:SS] [--over] [--hash] [--listing]");
                return;
            }
            _ if path.is_none() => path = Some(a),
            _ => die(&format!("unknown option {a}")),
        }
    }
    let path = path.unwrap_or_else(|| die("which formula? (a .psi file, or - for stdin)"));
    let src = if path == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .unwrap_or_else(|e| die(&e.to_string()));
        s
    } else {
        std::fs::read_to_string(&path).unwrap_or_else(|e| die(&format!("{path}: {e}")))
    };

    let prog = match compile(&src) {
        Ok(p) => p,
        Err(e) => {
            let (line, col) = e.line_col(&src);
            let r = e.render(&src);
            // render() starts "line N: msg"; say the column too.
            let rest = r.split_once('\n').map_or("", |x| x.1);
            println!("error: line {line}, column {col}: {}\n{rest}", e.msg);
            std::process::exit(1);
        }
    };

    // What it is.
    let (f, r, x) = prog.counts();
    let inputs = prog.inputs();
    let us = prog.esp32_us();
    let rating = match us {
        0..=5000 => "light",
        5001..=8000 => "medium",
        _ => "heavy",
    };
    let reads: Vec<&str> = [
        (1, "a"),
        (2, "b"),
        (4, "c"),
        (8, "t"),
        (16, "the clock"),
        (32, "the picture under it"),
    ]
    .iter()
    .filter(|e| inputs & e.0 != 0)
    .map(|e| e.1)
    .collect();
    println!(
        "ok: {} of {MAX_OPS} steps (per frame {f}, per row {r}, per pixel {x}), {} layer{}, {} of {REGS} registers",
        prog.len(),
        prog.layers(),
        if prog.layers() == 1 { "" } else { "s" },
        prog.regs()
    );
    println!(
        "loop {} s; reads {}",
        prog.loop_ms() as f64 / 1000.0,
        if reads.is_empty() {
            "nothing (a still picture)".into()
        } else {
            reads.join(", ")
        }
    );
    println!(
        "panel: ~{:.1} ms a frame: {rating} (light ≤ 5 ms, medium ≤ 8 ms; the gallery and GIF looks take ≤ 8 ms)",
        us as f64 / 1000.0
    );
    if listing {
        print!("{}", prog.listing());
    }

    // The frames: over one loop; a clock-only formula over a day; a still one once.
    let moves = inputs & 8 != 0;
    let clocked = inputs & 16 != 0 && !moves;
    let times: Vec<(u32, (u8, u8, u8))> = match ms_list {
        Some(v) => v.into_iter().map(|ms| (ms, clock)).collect(),
        None if moves => (0..frames)
            .map(|k| {
                // Whole 20 ms frames, as the panel plays them.
                let ms = (prog.loop_ms() as u64 * k as u64 / frames as u64) as u32 / 20 * 20;
                (ms, clock)
            })
            .collect(),
        None if clocked => (0..frames)
            .map(|k| {
                // Across the day, the minutes moving too.
                let m = (k * 24 * 60 / frames + k * 13) as u32 % (24 * 60);
                (0, ((m / 60) as u8, (m % 60) as u8, ((k * 17) % 60) as u8))
            })
            .collect(),
        None => vec![(0, clock)],
    };
    let under = (over || prog.over()).then(sample);
    let mut scratch = Scratch::<128>::new();
    let mut shots = Vec::new();
    for &(ms, (hh, mm, ss)) in &times {
        let inp = Inputs {
            t: Inputs::at_ms(ms),
            hour: hh,
            minute: mm,
            second: ss,
            a: q(sl[0]),
            b: q(sl[1]),
            c: q(sl[2]),
        };
        let mut frame = vec![0u8; W * H * 3];
        render_rows(
            &prog,
            &mut scratch,
            &inp,
            &OWN,
            under.as_deref(),
            u32::MAX,
            &mut frame,
        );
        shots.push(frame);
    }
    let label: Vec<String> = times
        .iter()
        .map(|&(ms, (hh, mm, ss))| {
            if clocked {
                format!("{hh:02}:{mm:02}:{ss:02}")
            } else {
                format!("{ms} ms")
            }
        })
        .collect();
    if hash {
        for (l, s) in label.iter().zip(&shots) {
            println!("frame {l}: {:#x}", fnv(s));
        }
    }
    let out = out.unwrap_or_else(|| {
        if path == "-" {
            "formula.png".into()
        } else {
            std::path::Path::new(&path)
                .with_extension("png")
                .to_string_lossy()
                .into_owned()
        }
    });
    if out != "-" {
        let png = sheet(&shots, scale);
        std::fs::write(&out, png).unwrap_or_else(|e| die(&format!("{out}: {e}")));
        println!(
            "frames: {} → {out} ({} a row, left to right, {scale}x{})",
            label.join(", "),
            COLS.min(shots.len()),
            if under.is_some() {
                ", over a sample picture"
            } else {
                ""
            }
        );
    }
}

fn die(msg: &str) -> ! {
    eprintln!("psi-check: {msg}");
    std::process::exit(2)
}

fn q(v: f64) -> i32 {
    q16((v.clamp(-32767.0, 32767.0) * 10000.0).round() as i32, 10000)
}

/// The web app's sample picture (js/psikit.js: a sky, a sun and hills),
/// what the editor draws an over-the-GIF formula over: 128 × 32 RGB, top
/// row first.
fn sample() -> Vec<u8> {
    let mut p = vec![0u8; W * H * 3];
    for i in 0..W * H {
        let (x, y) = ((i % W) as f64, (i / W) as f64);
        let (xi, yi) = (i % W, i / W);
        let c = if y > 23.0 - 4.0 * (x / 13.0).sin() - 2.0 * (x / 5.0).sin() {
            [30, (110 + (xi * 7 + yi * 13) % 50) as u8, 40]
        } else if (x - 98.0).powi(2) + 1.5 * (y - 10.0).powi(2) < 50.0 {
            [255, 215, 80]
        } else {
            [
                (40 + yi * 6) as u8,
                (60 + yi * 4) as u8,
                (170 - yi * 3) as u8,
            ]
        };
        p[i * 3..i * 3 + 3].copy_from_slice(&c);
    }
    p
}

const COLS: usize = 4;
const GAP: usize = 4;

/// The frames in rows of [`COLS`], each scaled `s` times, as a PNG.
fn sheet(shots: &[Vec<u8>], s: usize) -> Vec<u8> {
    let cols = COLS.min(shots.len());
    let rows = shots.len().div_ceil(COLS);
    let (fw, fh) = (W * s, H * s);
    let (iw, ih) = (GAP + cols * (fw + GAP), GAP + rows * (fh + GAP));
    let mut img = vec![48u8; iw * ih * 3];
    for (k, f) in shots.iter().enumerate() {
        let (ox, oy) = (GAP + (k % COLS) * (fw + GAP), GAP + (k / COLS) * (fh + GAP));
        for y in 0..fh {
            for x in 0..fw {
                let src = ((y / s) * W + x / s) * 3;
                let dst = ((oy + y) * iw + ox + x) * 3;
                img[dst..dst + 3].copy_from_slice(&f[src..src + 3]);
            }
        }
    }
    png(iw, ih, &img)
}

/// An RGB PNG, uncompressed (stored deflate blocks): no dependencies.
fn png(w: usize, h: usize, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(h * (w * 3 + 1));
    for row in rgb.chunks(w * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (k, c) in chunks.iter().enumerate() {
        z.push((k + 1 == chunks.len()) as u8);
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    for (ty, data) in [(b"IHDR", &ihdr[..]), (b"IDAT", &z[..]), (b"IEND", &[][..])] {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = ty.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    }
    out
}

fn crc32(d: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}
