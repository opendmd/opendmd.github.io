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
// `fixed::lz32`'s one `nsau` (the esp toolchain is a nightly; the host and
// wasm builds never see the gate).
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]

extern crate alloc;

#[doc(hidden)]
pub mod builtins;
mod compile;
mod draw;
pub mod fixed;
mod fuse;
#[cfg(any(test, all(feature = "jit", target_arch = "xtensa", target_os = "espidf")))]
pub mod jit;
pub mod look;
mod meta;
mod program;
mod range;
mod still;
#[cfg(feature = "syntax")]
pub mod syntax;
mod tables;
mod vm;

pub use compile::{
    compact, compile_declared, compile_heap_estimate, compile_into, compile_layout, compile_opts, compile_unfused, compile_with, number,
    Declared, DeclaredControl, Error, Info, MAX_ABOUT_TEXT, MAX_DEPTH, MAX_LABEL_TEXT,
    MAX_NAME_TEXT, MAX_NODES, MAX_SOURCE, NO_MEMORY,
};
pub use meta::{Control, Meta};
pub use draw::{Ink, FRAME_STEPS, LINE_STEPS, PRIMITIVES, TURNS};
pub use program::{Op, Program, H, MAX_OPS, MAX_SCALARS, MAX_TABLE, REGS, W};
pub use still::{Still, StillLane, STILL_LANES};
pub use tables::{Glyphs, Tables, ATAN};
/// (psi-probe's references for `fixed::sqrt` and `len`.)
#[doc(hidden)]
pub use tables::SQRT;
#[cfg(feature = "tables")]
pub use tables::{FONT_4X6, FONT_5X7, FONT_BOLD, OWN, PALETTES, SIN};
#[cfg(feature = "stats")]
pub use vm::stats;
pub use vm::{
    q16, render_rows, render_rows_changed, render_rows_ink, render_rows_ink_turns,
    render_rows_still, render_with, Inputs, Scratch, Q16_ONE,
};

/// The lane width the effect uses: one panel row.
pub const LANES: usize = 128;

/// Compile `src` (a new [`Program`] by value: ~2.8 KB; on a small stack use
/// [`compile_into`] on a zeroed allocation instead).
pub fn compile(src: &str) -> Result<Program, Error> {
    let mut p = Program::default();
    compile_into(src, &mut p)?;
    Ok(p)
}

/// [`compile`] without the column run and the still cache's run (every
/// pixel step on every pixel, the same bytes): `psi-probe`'s step costs
/// and the estimate's tests.
#[doc(hidden)]
pub fn compile_plain(src: &str) -> Result<Program, Error> {
    let mut p = Program::default();
    compile_layout(src, &mut p, false)?;
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
    /// ¼, ¾; a `loop forever` formula's `t` the same `ms`, its `seed`
    /// 0.3).
    pub fn inputs(ms: u32) -> Inputs {
        Inputs {
            t: Inputs::at_ms(ms),
            hour: 10,
            minute: 9,
            second: 30,
            a: 1 << 15,
            b: 1 << 14,
            c: 3 << 14,
            tw: Inputs::wide_ms(ms as u64),
            seed: SEED,
        }
    }

    /// The golden frames' `seed`: 0.3.
    pub const SEED: i32 = 19_661;

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
        // The fonts and bits: every glyph of both fonts, digits out of
        // range (-1, 10, 11), coordinates outside the cell, the steps once
        // a frame and once a row, bit() of negative and fractional values.
        ("glyphs", "ch = 32 + floor(px/5) + floor((31 - py)/8)*25\nhsv(ch/95, 1, glyph(ch, px % 5, (31 - py) % 8))", 0, 0xe1d356e86e222963),
        ("glyphs-small", "ch = 32 + floor(px/4) + floor((31 - py)/6)*32\nhsv(ch/95, 0.5, glyph(ch, px % 4 - 0.5, (31 - py) % 6 + 0.75, small))", 0, 0x34cfed23cec69500),
        ("digits", "rgb(digit(floor(px/5) % 12 - 1, px % 5, 31 - py), digit(floor(px/4) % 12 - 1, px % 4 - 1, 24 - py, small), bit(px, py % 8))", 0, 0xa7bad8a015931c40),
        ("digit-levels", "frame { d = second % 10 }\nr = 31 - py\nrgb(digit(d, 2, r), glyph('A', 1, r - 8)*0.5 + digit(7, 1, 1)*0.25, glyph(':', px % 5, r - 16, small))", 0, 0x1f18467e4e514c89),
        ("digits-bold", "rgb(digit(floor(px/5) % 12 - 1, px % 5 - 1, (31 - py) % 7 - 1, bold), digit(second % 10, floor(px/8) % 4, floor((31 - py)/6), bold), 0.2)", 0, 0x19904e2c8f6ffcab),
        ("bits", "gray(bit(px*258 - 16000, floor(py/2) - 1) + bit(-px - 0.5, py)*0.5 + bit(t*1000, 3)*0.25)", 1500, 0xab556034d27ca745),
        // About the middle and from the top; pick of per-frame values per
        // pixel (one step), of per-row values per row, per frame, of
        // per-pixel values (compares and selects), k out of range both
        // ways and fractional; colour literals, mixes of them, nested.
        ("centred", "loop 4\nhsv(angle + t/4, frac(r*2), 1 - ty/40 + cx*cy*0.1)", 1000, 0xa8e6660c580785fc),
        ("pick", "frame { s = second/60; m = minute/60 }\nk = px/16 - 1.5\nrgb(pick(k, s, m, 0.25, 1), pick(py/8, y, 1 - y, 0.5) + pick(minute % 3, 0.1, 0.2), pick(px/40, x, y, ur + 0.5))", 0, 0x6ee236d2d1be0235),
        ("colours", "loop 2\nk = sin(t/2)*0.5 + 0.5\nmix(mix(#000, #f80, x), mix(#123456, rgb(y, k, 1 - y), x), smoothstep(0.2, 0.8, y))", 500, 0xb2b7af7010efe215),
        ("colour-rgb", "rgb(mix(#fff, #08c, cy*2 + 1), r*0.6)", 0, 0x4f171c37bb61e199),
        // A palette times a value (`pal(P, v) * k`, a layer's with coverage
        // too); noise at a whole z (its four-corner path); values for the
        // still cache (`psi-probe` draws these with it as well).
        ("pal-lit", "loop 2\nlayer { pal(chrono, frac(x + t/2)) * y }\nlayer screen { pal(fire, y, 0.5) * (1 - x) * 1.5 }", 700, 0x29dfb6dd6baeba44),
        ("noise-flat", "gray(noise(x*8, y*4, 0)*0.5 + noise(px/9, py/5, 3)*0.5 - noise(x, y, -2)*0.1)", 0, 0xfa6f54daaa8319d2),
        // `loop forever`: `t` growing without end (past the Q16.16 range at
        // 9.1 hours: frames at 11 hours and 46 days), its sums and
        // products, fractions, waves, noises (per pixel, per row, in the
        // draw stage), hashes, `%`, compares, smoothstep, `seed`.
        ("forever", FOREVER, 0, 0x1104e195c98cef2a),
        ("forever", FOREVER, 125_000, 0x2ea22e7deac5650d),
        ("forever", FOREVER, 40_000_000, 0x34ff604690d2e7e1),
        ("forever", FOREVER, 4_000_000_000, 0xea02ad83dede366c),
        // Sprites: 1, 2 and 4 bits a pixel, read per pixel (scaled, moving),
        // per row and once a frame, outside them, and the road's car.
        ("sprites", SPRITES, 1500, 0xb4426b7968cc9336),
        ("sprite-car", SPRITE_CAR, 0, 0x3cdaf547126eb095),
        // Palettes of a formula's own: plain, scaled, covered, in a part,
        // indexed by a sprite.
        ("palettes", PALETTES_OWN, 700, 0x9b6bdb382e0435d6),
        ("still", "loop 4.8\nd = len(px - 63.5, (15.5 - py)*2)\nk = 64/(d + 0.5)\nu = frac(atan2(15.5 - py, px - 63.5)*2 + t/4.8 + k/64)\nv = frac(k/16 + t/2.4)\ncell = (floor(u*8) + floor(v*4)) % 2\npal(chrono, frac(mix(0.28, 0.78, cell) + t/4.8)) * clamp((d - 6)/12, 0, 1)", 1900, 0x79d9337fc1b74897),
    ];

    /// A `loop forever` golden formula: per frame and per pixel.
    pub const FOREVER: &str = "loop forever\nframe { u = t*0.15 + seed*1000 }\nv = noise(x*8 + t*0.5, y*4, t*0.2)*0.5 + sin(x*3 + t/8)*0.2\nhsv(u + v + noise(u, 5, 0)*0.25, 1 - (t % 7)/14, smoothstep(0, 20, t)*0.5 + 0.5)";

    /// Another: per row, a draw stage, hashes, compares, min and max.
    pub const FOREVER_ROWS: &str = "loop forever\ndraw { for k < 24 { dot(frac(t*0.1 + k/24)*128, noise(t*0.3 + k, 5, 0)*31, hash(floor(t*4), k)) } }\nq = noise(t*0.15 + y*2, 5, 0)\nrgb(q, ink + frac(t*10 + x)*0.25, (t > 60) * 0.5 + min(t/300, x)*0.5)";

    /// A sprites golden formula: three sprites, every level.
    pub const SPRITES: &str = "loop 3\nsprite one {\n  key . 0, # 1\n  .##..##.\n  #..##..#\n}\nsprite two {\n  key . 0, r 0.3, g 0.6, b 1\n  rgb.bgr.rgb.bgr.rgb.bgr.rgb.bgr.rgb.bgr.r\n  .r.g.b.r.g.b.r.g.b.r.g.b.r.g.b.r.g.b.r.g.\n  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n}\nsprite many {\n  key 0 0, 1 0.1, 2 0.2, 3 0.3, 4 0.4, 5 0.5, 6 0.6, 7 0.7, 8 0.8, 9 0.9, A 1, B -1, C 2, D 0.05, E 0.15, F 0.25\n  0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF\n  FEDCBA9876543210FEDCBA9876543210FEDCBA9876543210FEDCBA9876543210\n  02468ACE13579BDF02468ACE13579BDF02468ACE13579BDF02468ACE13579BDF\n}\nframe { f = many[floor(t*5), 1] }\nrw = two[ty, 1]\nrgb(one[px/8 - t, ty/16], many[px/2, ty/4 - 2] + f*0.25, two[px - 40, ty - 13] + rw*0.5)";

    /// The road's car (`sprite`), its colours from a pick.
    pub const SPRITE_CAR: &str = "sprite car {\n  key . 0, R 1, D 2, L 3\n  ........RRRRRRRRRR........\n  .......RDDDDDDDDDDR.......\n  ......RDDDDDDDDDDDDR......\n  ...RRRRRRRRRRRRRRRRRRRR...\n  .RLLLLRDDDDDDDDDDDDRLLLLR.\n  .RLLLLRDDDDDDDDDDDDRLLLLR.\n  .RRRRRRRRRRRRRRRRRRRRRRRR.\n  .RRRRRRRRRRRRRRRRRRRRRRRR.\n  DDDDDDDDDDDDDDDDDDDDDDDDDD\n}\nk = car[px - 50.5, ty - 23]\npick(k, #203040, #e02020, #101010, #ffa020)";

    /// A golden formula with palettes of its own.
    pub const PALETTES_OWN: &str = "loop 2\npalette sea = #001020, #0060a0, #40c0e0, #ffffff\npalette ember = #200000, #c02000,\n  #ffa020\nsprite dot {\n  key . 0, o 1, O 0.5\n  .oOo.\n  oOOOo\n  .oOo.\n}\npart glow {\n  palette ember = #200000, #c02000, #ffa020\n  pal(ember, frac(x*4 - t/2))\n}\nlayer { pal(sea, y + sin(x*2 + t/2)*0.2) * (0.6 + x*0.4) }\nlayer screen { glow(x, y, 0.5) }\nlayer add { pal(ember, dot[px % 6, ty % 4]) }";

    /// The picture the [`OVER`] frames are drawn over: every byte value in
    /// every channel somewhere (a hash per pixel), with smooth ramps across
    /// the top half.
    pub fn picture(out: &mut [u8]) {
        for (i, p) in out.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            let (x, y) = ((i % crate::W) as u32, (i / crate::W) as u32);
            let h = crate::fixed::mix32(i as u32 ^ 0x9e37_79b9);
            if y < 16 {
                p.copy_from_slice(&[(x * 2) as u8, (y * 16 + x / 8) as u8, (255 - x * 2) as u8]);
            } else {
                p.copy_from_slice(&[h as u8, (h >> 8) as u8, (h >> 16) as u8]);
            }
        }
    }

    /// Frames of formulas with layers, and of formulas drawn over
    /// [`picture`] (`over`: true) — what is under, blends, coverage.
    pub const OVER: &[(&str, &str, u32, bool, u64)] = &[
        ("identity", "rgb(ur, ug, ub)", 0, true, 0x2c7a29e300e4c8cf),
        ("luma", "gray(ul)", 0, true, 0xc7f57bec83c88cfe),
        ("grade", "rgb(ur*1.2 - 0.05, ug, ub*0.7 + ul*0.2)", 0, true, 0x56bbda81836b00e),
        ("scanlines", "layer multiply { gray(py % 2 ? 1 : 0.55) }", 0, true, 0x8925595265a58f1c),
        ("vignette", "d = len((x - 0.5)*2, (y - 0.5)*1.2)\nlayer multiply { gray(1 - smoothstep(0.6, 1.3, d)) }", 0, true, 0x83fc31d9d8ceb1e3),
        ("blends", "loop 4\nlayer add { hsv(x + t/4, 1, 0.5) }\nlayer screen { gray(y, 0.5) }\nlayer max { rgb(0, step(0.5, x), 0) }\nlayer min { gray(0.9) }", 1000, true, 0x7160afc8affc1391),
        ("cover", "layer { rgb(1, 0, 0, x) }\nlayer over { pal(ice, y, 0.25) }\nlayer { gray(0.5, step(px % 8, 1)) }", 0, true, 0x4c437de4180ecfbe),
        ("hidden", "layer add { noise(x*8, y*4, t) }\nlayer { hsv(x, 1, 1) }\nlayer multiply { gray(1 - ul*0.5) }", 0, true, 0x6e7731edba40d20b),
        ("alone", "layer { hsv(x, 1, 1) }\nlayer add { gray(ur + 0.2, 0.5) }", 0, false, 0xb62345c4247f9a5),
        ("cover-colour", "layer multiply { mix(#fff, #400000, r*0.7) }\nlayer { rgb(#ffbf4d, step(abs(cy), 0.1)) }\nlayer screen { mix(rgb(ur, ug, ub), #304050, x) }", 0, true, 0x7679d76ab8dfdd68),
        ("scoped", "frame { s = sin(t) }\nk = x*4\nlayer { v = frac(k + s)\nrgb(v, ub, 0) }\nlayer screen { v = frac(k*2 - y)\nframe { q = cos(t) }\npal(fire, v*q, 0.75) }", 2500, true, 0xe01c5295366f8613),
    ];

    /// Frames of formulas that draw (`draw { }`, `for`, `table`, `line`,
    /// `dot`, `ink`), rendered with a canvas ([`crate::render_rows_ink`]),
    /// `over`: over [`picture`]. Lines in all eight octants and both
    /// directions, ties at ½, clipping on every edge, the step caps, the max
    /// rule, dots, tables at every level, loops of 1 and 1,024 turns, loops
    /// nested, `ink` through a part and through a look.
    pub const DRAWN: &[(&str, &str, u32, bool, u64)] = &[
    ("octants", "draw {\n  for k < 16 {\n    an = k/16 + 1/64\n    ex = 64 + cos(an)*60; ey = 16 + sin(an)*15\n    rev = k % 2\n    line(mix(64, ex, rev), mix(16, ey, rev), mix(ex, 64, rev), mix(ey, 16, rev), 1, k/16)\n  }\n}\npal(rainbow, ink)", 0, false, 0x75ab1043ab15722a),
    ("ties", "draw {\n  dot(10.5, 3.5); dot(20.49, 3.5, 0.5); dot(-0.5, 0); dot(127.5, 31.5); dot(127.49, 31.49, 0.25)\n  line(0.5, 10.5, 20.5, 17.5); line(30.5, 20.5, 30.5, 20.5, 0.5); line(40, 5, 52, 5.5, 0, 1)\n}\ngray(ink)", 0, false, 0xdd474907fb393651),
    ("clipping", "draw {\n  line(-20, 16, 140, 20); line(64, -10, 70, 40, 0.5)\n  line(-30000, -30000, 30000, 30000, 0.75); line(-5, -5, -1, 40); line(130, 0, 200, 31)\n  line(0, 31.6, 127, 31.4); line(10, -0.6, 120, -0.4)\n}\ngray(ink)", 0, false, 0xfcc5dd4b13859523),
    ("caps", "draw {\n  for k < 64 { line(-512, 10 - k, 639, 52 - k, 1 - k/64, 0.5) }\n  for j < 4 { dot(j*10, 30) }\n}\nrgb(ink, ink*0.5, 0.1)", 0, false, 0x7bf1c544177ae246),
    ("max-rule", "loop 2\ndraw {\n  line(0, 0, 127, 31, 0.3); line(0, 31, 127, 0, 0.8); line(64, 0, 64, 31, 0, 1)\n  for k < 8 { dot(64, k*4, k/8) }\n}\ngray(ink)", 0, false, 0x9da79f25fe888543),
    ("tables", "table T = 0, 0.25, 0.5, 1, -1\ntable P = 3, 9, 27\nframe { f = T[floor(t*2)] }\ndraw {\n  for k < 5 { dot(P[k % 3]*4 + k, T[k]*20 + 5, T[k] + 0.5) }\n  dot(100, 10, f + 0.5)\n}\nrgb(T[px % 6] + ink, T[py % 4], f + T[-1] + T[2.9])", 1000, false, 0xd9e70fec96826bc7),
    ("loops", "loop 4\ndraw {\n  for k < 1 { line(0, 0, 127, 0) }\n  for j < 1024 { dot(j % 128, floor(j/128)*4 + 2, hash(j, floor(t))) }\n}\npal(fire, ink)", 2000, false, 0xe0b34a62ddc5a9cd),
    ("nested", "loop 4.8\ndraw {\n  for s < 12 {\n    th = s/12 + t/4.8\n    for r < 6 {\n      ph = r/6\n      rad = 30 + 10*cos(ph)\n      x0 = 64 + rad*cos(th); y0 = 16 + 10*sin(ph) + rad*sin(th)*0.2\n      x1 = 64 + rad*cos(th + 1/12); y1 = 16 + 10*sin(ph) + rad*sin(th + 1/12)*0.2\n      line(x0, y0, x1, y1, 0.3 + r/9, 1 - r/9)\n    }\n  }\n}\nhsv(x + t/4.8, 0.7, ink)", 1200, false, 0x7f399d8d5bf3b553),
    ("ink-part", "part p {\n  loop 2\n  draw { for k < 24 { dot(k*5 + t*2, 16 + sin(k/24 + t/2)*12) } }\n  gray(ink)\n}\nlayer { p(x*0.5, y) }\nlayer add { p(1 - x, y, 0.5) }", 500, false, 0x20b3d33cb32de631),
    ("forever-rows", super::golden::FOREVER_ROWS, 3_500, false, 0x847d8d42f450cf91),
    ("forever-rows", super::golden::FOREVER_ROWS, 40_000_000, true, 0xf4f2f6627225ad38),
    ("ink-look", "part look {\n  loop 4.8\n  draw { for e < 6 { line(64 + 30*cos(e/6 + t/4.8), 16 + 12*sin(e/6 + t/4.8), 64 + 30*cos(e/6 + 1/6 + t/4.8), 16 + 12*sin(e/6 + 1/6 + t/4.8)) } }\n  #40c0ff * ink\n}\nloop 4.8\nlayer screen { look(x, y, 0.4) }", 600, true, 0x6c519b2a47772c08),
    ];
}

/// Formulas that make the compiler work hardest within what the panel
/// accepts (a path of at most 2,048 B, as `fx_psi::path` writes it: the
/// source compacted, `+` as `%2B`): compile-time tests, the bench, the
/// host's stack probe (`examples/stack.rs`) and `psi-probe`'s stack report.
#[doc(hidden)]
pub mod worst {
    use alloc::format;
    use alloc::string::String;

    /// The path's length for `src` (`@formula?src=` + compacted, `+` 3 B).
    pub fn path_len(src: &str) -> usize {
        13 + crate::compact(src)
            .bytes()
            .map(|b| if b == b'+' { 3 } else { 1 })
            .sum::<usize>()
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
        fill(
            |k| format!("q{k} = x*{k}.5 + y*{k}.25"),
            |k| format!("gray(q{k})"),
        )
    }

    /// A chain, each local built on the one before, up to the step limit.
    pub fn chain() -> String {
        fill(
            |k| {
                if k == 0 {
                    String::from("q0 = x")
                } else {
                    format!("q{k} = q{}*0.99 + {k}.5", k - 1)
                }
            },
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

    /// Parts inside parts, each drawn at several places: the most reading
    /// again (stopped by the compiler's bound on it).
    pub fn parts() -> String {
        let mut s = String::from("part p0 { v = sin(x*3 + t)*y\ngray(v) }\n");
        for k in 1.. {
            let j = k - 1;
            let next = format!(
                "part p{k} {{\n{s}v = p{j}(x*0.5, y).v + p{j}(y, x).v + p{j}(x + 0.1, y).v + p{j}(x, y*2).v + p{j}(1 - x, y).v\ngray(v)\n}}\n"
            );
            if path_len(&format!("{next}p{k}")) > 2048
                || crate::compile(&format!("{next}p{k}")).is_err_and(|e| e.msg.contains("too deep"))
            {
                return format!("{s}p{j}");
            }
            s = next;
        }
        unreachable!()
    }

    /// The deepest plain nesting: calls inside calls to the depth cap, and
    /// a misspelt name at the bottom (the error and its suggestions are
    /// built at the deepest point): the parser's stack at its highest.
    pub fn nested() -> String {
        let mut s = String::from("tme");
        let mut k = 0;
        while crate::compile(&format!("gray({s})"))
            .is_err_and(|e| e.msg.starts_with("unknown name"))
        {
            s = format!("sin({s})");
            k += 1;
            if k > 64 {
                break;
            }
        }
        // One less: the deepest that still reaches the name.
        s = s.replacen("sin(", "", 1);
        s.pop();
        format!("gray({s})")
    }

    /// [`nested`] through `digit(…, 0, 0)` (a three-value call with its own
    /// parser, `glyph_args`) instead of `sin(…)`, to the depth cap.
    pub fn nested_digit() -> String {
        let mut s = String::from("tme");
        let mut k = 0;
        while crate::compile(&format!("gray({s})"))
            .is_err_and(|e| e.msg.starts_with("unknown name"))
        {
            s = format!("digit({s}, 0, 0)");
            k += 1;
            if k > 64 {
                break;
            }
        }
        // One less: the deepest that still reaches the name.
        s = s.replacen("digit(", "", 1);
        s.truncate(s.len() - ", 0, 0)".len());
        format!("gray({s})")
    }

    /// Colours mixed inside colours (`mix(mix(…), #fff, 0.5)`, the colour
    /// parser's own recursion) to the depth cap, a misspelt name at the
    /// bottom.
    pub fn nested_mix() -> String {
        deepest("mix(#000, #fff, tme)", |s| format!("mix({s}, #fff, 0.5)"))
    }

    /// `pick(pick(…), 1)` (a list of values, read out of line) to the depth
    /// cap, a misspelt name at the bottom.
    pub fn nested_pick() -> String {
        deepest("pick(tme, 1)", |s| format!("pick({s}, 1)"))
    }

    /// The deepest `wrap` of `leaf` (as a whole line) that still reaches the
    /// misspelt name at its bottom.
    fn deepest(leaf: &str, wrap: impl Fn(&str) -> String) -> String {
        let mut s = String::from(leaf);
        for _ in 0..64 {
            let next = wrap(&s);
            if !crate::compile(&next).is_err_and(|e| e.msg.starts_with("unknown name")) {
                break;
            }
            s = next;
        }
        s
    }

    /// Parts inside parts, each drawn in a layer at another place, as deep
    /// as the cap allows, with a misspelt name at the bottom.
    pub fn parts_in_layers() -> String {
        let mut best = String::new();
        for depth in 1..12 {
            let mut s = String::from("part p0 {\nv = sin(cos(x*3 + tme))\ngray(v)\n}\n");
            for k in 1..depth {
                let j = k - 1;
                s = format!("part p{k} {{\n{s}layer {{ p{j}(x*0.5, sin(y)) }}\nlayer {{ p{j}(1 - x, y, 0.5) }}\n}}\n");
            }
            let src = format!("{s}layer {{ p{}(x, y) }}", depth - 1);
            match crate::compile(&src) {
                Err(e) if e.msg.starts_with("unknown name") => best = src,
                _ => break,
            }
        }
        best
    }

    /// [`nested`] inside two loops of the draw stage (each a level of the
    /// parser's recursion), as the line's x: the draw stage's deepest.
    pub fn draw() -> String {
        let mut best = String::new();
        let mut s = String::from("tme");
        for _ in 0..64 {
            let src =
                format!("draw {{\nfor k < 2 {{\nfor j < 2 {{\ndot({s}, j)\n}}\n}}\n}}\ngray(ink)");
            if !crate::compile(&src).is_err_and(|e| e.msg.starts_with("unknown name")) {
                break;
            }
            best = src;
            s = format!("sin({s})");
        }
        best
    }

    /// The deepest `wrap` of `tme` inside `head` + `(…)` + `tail` that still
    /// reaches the misspelt name.
    fn deepest_in(head: &str, tail: &str, wrap: impl Fn(&str) -> String) -> String {
        let mut s = String::from("tme");
        let mut best = format!("{head}{s}{tail}");
        for _ in 0..64 {
            let next = wrap(&s);
            let src = format!("{head}{next}{tail}");
            if !crate::compile(&src).is_err_and(|e| e.msg.starts_with("unknown name")) {
                break;
            }
            (s, best) = (next, src);
        }
        best
    }

    /// Sprite reads inside sprite reads (`s[s[…], y]`) to the depth cap.
    pub fn nested_sprite() -> String {
        deepest_in("sprite s {\n key . 0, X 1\n .X..\n}\ngray(", ")", |s| {
            format!("s[{s}, y]")
        })
    }

    /// A `loop forever` formula's growing values nested (`sin(t*0.1 + …)`):
    /// the parser's and the wide rewriting's frames together.
    pub fn nested_forever() -> String {
        deepest_in("loop forever\ngray(", ")", |s| format!("sin(t*0.1 + {s})"))
    }

    /// Nesting inside a palette of the formula's own.
    pub fn nested_palette() -> String {
        deepest_in("palette p = #000, #f80, #fff\npal(p, ", ")", |s| {
            format!("sin({s})")
        })
    }

    /// Every declaration a formula may have (formula-apps F1), the longest
    /// text each may hold, then [`nested_digit`]'s nesting with the
    /// controls' names at the bottom: the header's reading before the
    /// deepest parse.
    pub fn declared() -> String {
        let head = format!(
            "psi 1\nname \"{}\"\nabout \"{}\"\ncontrol zoom 1..4 = 2 \"{}\"\ncontrol mode = calm, wild, odd, four, five, six \"{}\"\ncontrol c -1..1 = 0.5 \"{}\"\nlook screen 40\ngray(",
            "N".repeat(crate::MAX_NAME_TEXT),
            "A".repeat(crate::MAX_ABOUT_TEXT),
            "L".repeat(crate::MAX_LABEL_TEXT),
            "L".repeat(crate::MAX_LABEL_TEXT),
            "L".repeat(crate::MAX_LABEL_TEXT),
        );
        deepest_in(&head, ")", |s| format!("digit({s}, zoom, mode)"))
    }

    /// A part whose own lines declare three controls (a formula pasted in),
    /// inside a part, nested as deep as it goes inside.
    pub fn declared_part() -> String {
        deepest_in(
            "part q {\npart p {\nname \"P\"\ncontrol k 0..2 = 1\ncontrol m = on, off\ncontrol n = 0.5\nlook\ngray(",
            ")\n}\np\n}\nq",
            |s| format!("sin({s} + k*m*n)"),
        )
    }

    /// A named generator.
    pub type Case = (&'static str, fn() -> String);

    pub const ALL: [Case; 15] = [
        ("many locals", many_locals),
        ("chain", chain),
        ("distinct terms", distinct),
        ("parts", parts),
        ("nested", nested),
        ("nested digit", nested_digit),
        ("nested mix", nested_mix),
        ("nested pick", nested_pick),
        ("parts in layers", parts_in_layers),
        ("draw", draw),
        ("nested sprite", nested_sprite),
        ("nested forever", nested_forever),
        ("nested palette", nested_palette),
        ("declared", declared),
        ("declared part", declared_part),
    ];
}

#[cfg(all(test, feature = "tables"))]
mod tests;

#[cfg(test)]
mod fixed_tests;
