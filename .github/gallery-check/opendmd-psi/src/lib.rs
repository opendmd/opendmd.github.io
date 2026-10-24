//! **Psi** (Ψ): OpenDMD's per-pixel effects language. Users call a script a
//! *Formula*; this crate is the language, its compiler and its VM.
//!
//! A formula is a few lines of arithmetic over the pixel's position and the
//! time, ending in a colour:
//!
//! ```text
//! loop 8
//! v = sin(x*3 + t/8) + sin((y*4 + t/8)*1.3) + sin(len(x-0.5, y-0.5)*6 - t/4)
//! hsv(v/3 + t/8, 1, 1)
//! ```
//!
//! [`compile`] turns the text into a [`Program`] — a fixed-size value of at
//! most [`MAX_OPS`] steps — and [`render`] draws a 128 × 32 frame of it
//! into RGB888 bytes, a row at a time, in Q16.16 integer arithmetic, so the
//! same frame is the same bytes on the ESP32, the host and in wasm. `no_std`;
//! the compiler uses `alloc` (and frees it all before returning), the VM
//! never allocates: its working memory is a caller's [`Scratch`].
//!
//! The language (v0) and the VM's design are in `docs/research/psi-spike.md`.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod compile;
pub mod fixed;
mod program;
mod tables;
mod vm;

pub use compile::{compact, compile_into, number, Error, Info, MAX_DEPTH, MAX_NODES, MAX_SOURCE};
pub use program::{Op, Program, H, MAX_OPS, MAX_SCALARS, REGS, W};
pub use tables::{Tables, ATAN};
#[cfg(feature = "tables")]
pub use tables::{OWN, PALETTES, SIN};
pub use vm::{q16, render_with, Inputs, Scratch, Q16_ONE};

/// The lane width the effect uses: one panel row.
pub const LANES: usize = 128;

/// Compile `src` (a new [`Program`] by value: ~1.8 KB; on a small stack use
/// [`compile_into`] on a zeroed allocation instead).
pub fn compile(src: &str) -> Result<Program, Error> {
    let mut p = Program::default();
    compile_into(src, &mut p)?;
    Ok(p)
}

/// [`render_with`] at the default lane width, [`LANES`].
pub fn render(
    prog: &Program,
    scratch: &mut Scratch<LANES>,
    inputs: &Inputs,
    tables: &Tables,
    out: &mut [u8],
) {
    render_with::<LANES>(prog, scratch, inputs, tables, out)
}

/// The three sketches from the research report, as shipped examples and
/// the golden-frame and benchmark set (`--` comments, the report's `//`
/// also accepted).
pub mod sketches {
    pub const PLASMA: &str = "\
loop 8
v = sin(x*3 + t/8) + sin((y*4 + t/8)*1.3) + sin(len(x-0.5, y-0.5)*6 - t/4)
hsv(v/3 + t/8, 1, 1)
";

    pub const FIRE: &str = "\
loop 4
heat = noise(x*8, y*4 + t*2, t) * (1.3 - y)      -- hotter at the bottom (y = 0)
pal(fire, clamp(heat, 0, 1))
";

    pub const RING_CLOCK: &str = "\
loop 60
frame {
  hr = frac(hour/12 + minute/720)       -- the hour hand as a turn
  glow = 0.6 + 0.4*sin(t/2)
}
d = len((x-0.5)*4, (y-0.5))            -- the ring, centred, aspect-corrected
ang = frac(atan2(y-0.5, (x-0.5)*4) + 0.25)
ring = smoothstep(0.08, 0.0, abs(d - 0.8))
hand = step(abs(ang - hr), 0.01) * step(d, 0.8)
rgb(ring*glow*0.3 + hand, ring*glow*0.5, ring*glow)
";

    pub const ALL: [(&str, &str); 3] = [
        ("plasma", PLASMA),
        ("fire", FIRE),
        ("ring-clock", RING_CLOCK),
    ];
}

/// The golden frames: (name, source, ms into the loop, FNV-1a 64 of the
/// RGB888 frame) under [`golden::inputs`]. Checked by `tests/golden.rs` on
/// the host and in wasm, and by the firmware's `psi-probe` on the ESP32:
/// the same hashes everywhere is bit-identical output.
pub mod golden {
    use crate::Inputs;

    /// The inputs a golden frame is rendered with (10:09:30, sliders ½,
    /// ¼, ¾).
    pub fn inputs(ms: u32) -> Inputs {
        Inputs {
            t: Inputs::at_ms(ms),
            hour: 10,
            minute: 9,
            second: 30,
            a: 1 << 15,
            b: 1 << 14,
            c: 3 << 14,
        }
    }

    /// FNV-1a, 64-bit.
    pub fn fnv(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    pub const FRAMES: &[(&str, &str, u32, u64)] = &[
        ("plasma", super::sketches::PLASMA, 0, 0xba0025be8e93c42d),
        ("plasma", super::sketches::PLASMA, 2500, 0xf0b0ca78ce47c0de),
        ("plasma", super::sketches::PLASMA, 7980, 0x40cf5c5d3b3bfc5),
        ("fire", super::sketches::FIRE, 0, 0xbd8f0354e44e62e1),
        ("fire", super::sketches::FIRE, 1230, 0xc4a40f51b506735f),
        ("ring-clock", super::sketches::RING_CLOCK, 0, 0x9ecdc4f671bb2db5),
        ("ring-clock", super::sketches::RING_CLOCK, 41_250, 0xac60627909815c81),
        // Edge cases: saturation, division by zero, floored %, the quadrants
        // of atan2, a constant colour, `i`, hash, sliders, comparisons.
        ("saturate", "gray(t*t*t*1000 - 5 + x*30000)", 3000, 0xfe4ec469d4f39325),
        ("div0", "v = x / (y - y)\nw2 = (x - 0.5) / 0\nrgb(v, w2 + 0.5, 0 / 0 + 0.25)", 0, 0xb8aafd09b9ba0b25),
        ("mod", "rgb((x*10 - 5) % 3 / 3, (px - 64) % -7 / -7, (y - 0.5) % 0.25 * 4)", 0, 0x897f48091e041b41),
        ("atan2", "hsv(atan2(y - 0.5, x - 0.5) + 0.5, 1, len(x - 0.5, y - 0.5)*2)", 0, 0xbfeee2097b9f6042),
        ("const", "rgb(1, 0.5, 0.25)", 0, 0x2433c930830b3325),
        ("index", "hsv(i / 4096, hash(i, 7), 1)", 0, 0xdf6cffa94510c228),
        ("sliders", "rgb(a, b * x, c ? y : 0)", 0, 0x333771e31b603aa5),
        ("logic", "gray(x < 0.5 and y > 0.5 or not (px % 8 < 4) ? 1 : 0.2)", 0, 0x2836493ef252ef25),
        ("waves", "rgb(tri(x*2 + t), saw(y + t) * 0.5 + 0.5, sqr(x*4) * 0.5 + 0.5)", 1000, 0xe2f3ccaeb3a87095),
        ("misc", "v = mix(0.2, 0.8, x) + floor(y * 4) / 4 + sqrt(x) - min(x, y) + max(0, cos(t))\ngray(frac(v))", 500, 0x4e537773ef7d12fe),
        ("smooth", "pal(ocean, smoothstep(0.2, 0.8, x) * smoothstep(1, 0, y))", 0, 0x7d51aaf40701e136),
        ("palettes", "pal(rainbow, x)", 0, 0xd2ee745daaab3f25),
        ("clock", "frame { s = second / 60 }\npal(amber, step(x, s) + 0.3 * step(y, minute / 60))", 0, 0x2fa28273c4507f25),
    ];
}

/// Formulas that make the compiler work hardest within what the panel
/// accepts (a path of at most 2,048 B, as `fx_psi::path` writes it: the
/// source compacted, `+` as `%2B`): compile-time tests and the bench.
#[cfg(feature = "tables")]
#[doc(hidden)]
pub mod worst {
    use alloc::format;
    use alloc::string::String;

    /// The path's length for `src` (`@fx:psi?src=` + compacted, `+` 3 B).
    pub fn path_len(src: &str) -> usize {
        12 + crate::compact(src).bytes().map(|b| if b == b'+' { 3 } else { 1 }).sum::<usize>()
    }

    fn fill(mut line: impl FnMut(usize) -> String, end: impl Fn(usize) -> String) -> String {
        let mut s = String::from("loop 8\n");
        let mut k = 0;
        loop {
            let next = format!("{s}{}\n", line(k));
            if path_len(&format!("{next}{}", end(k))) > 2048 {
                return format!("{s}{}", end(k.saturating_sub(1)));
            }
            s = next;
            k += 1;
        }
    }

    /// As many lines (locals) as fit, each a new value: the most names and
    /// nodes.
    pub fn many_locals() -> String {
        fill(|k| format!("q{k} = x*{k}.5 + y*{k}.25"), |k| format!("gray(q{k})"))
    }

    /// A chain, each local built on the one before, up to the step limit.
    pub fn chain() -> String {
        fill(
            |k| if k == 0 { String::from("q0 = x") } else { format!("q{k} = q{}*0.99 + {k}.5", k - 1) },
            |k| format!("gray(frac(q{k}))"),
        )
    }

    /// One long sum of distinct terms (no sharing to find).
    pub fn distinct() -> String {
        let mut s = String::from("v = 0");
        let mut k = 0;
        while path_len(&format!("{s} + x*1.{k:03}\ngray(v)")) <= 2048 {
            s = format!("{s} + x*1.{k:03}");
            k += 1;
        }
        format!("{s}\ngray(v)")
    }

    /// A named generator.
    pub type Case = (&'static str, fn() -> String);

    pub const ALL: [Case; 3] =
        [("many locals", many_locals), ("chain", chain), ("distinct terms", distinct)];
}

#[cfg(all(test, feature = "tables"))]
mod tests;
