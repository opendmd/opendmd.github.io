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
//! The language (v0) and the VM's design are in `opendmd-notes docs/research/psi-spike.md`.

#![cfg_attr(not(test), no_std)]
// `fixed::lz32`'s one `nsau` (the esp toolchain is a nightly; the host and
// wasm builds never see the gate).
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
// Code that runs once a compile or once a frame, optimised for size on the
// ESP32 (`#[optimize(size)]`: the app's flash is the scarce thing there).
#![cfg_attr(target_arch = "xtensa", feature(optimize_attribute))]
// Without the compiler (the player) some of its helpers go unused.
#![cfg_attr(not(feature = "compiler"), allow(dead_code, unused_imports))]

extern crate alloc;

#[doc(hidden)]
pub mod builtins;
#[cfg(feature = "compiler")]
mod compile;
mod draw;
mod error;
#[cfg(feature = "docs")]
#[doc(hidden)]
pub mod errors;
pub mod fixed;
mod fuse;
#[cfg(feature = "docs")]
#[doc(hidden)]
pub mod reference;
// (With `docs` on the host too: `psi check` says whether the panel's JIT
// takes a formula, and why not.)
#[cfg(any(
    test,
    feature = "docs",
    all(feature = "jit", target_arch = "xtensa", target_os = "espidf")
))]
pub mod jit;
mod jitplan;
#[cfg(feature = "compiler")]
pub mod look;
mod meta;
mod program;
pub mod psb;
pub use error::{Error, NO_MEMORY};
mod range;
mod rate;
mod still;
#[cfg(feature = "syntax")]
pub mod syntax;
mod tables;
mod vm;
pub mod world;

#[cfg(feature = "compiler")]
pub use compile::{
    compact, compile_declared, compile_heap_estimate, compile_heap_for_tokens, compile_into,
    compile_layout, compile_look, compile_look_with, compile_measured, compile_opts,
    compile_portable, compile_portable_with, compile_unfused, compile_with, squeeze, Declared,
    DeclaredControl, Info, MAX_ABOUT_TEXT, MAX_DEPTH, MAX_LABEL_TEXT, MAX_NAME_TEXT, MAX_NODES,
    MAX_SOURCE,
};
pub use draw::{Ink, Plane, FRAME_STEPS, LINE_STEPS, PRIMITIVES, TURNS};
pub use meta::{Control, Meta, BLENDS};
pub use program::{
    Op, Prog, Program, ProgramOf, SceneFrame, H, MAX_INS, MAX_OPS, MAX_STEPS, MAX_PALETTES, MAX_SCALARS, MAX_TABLE,
    MAX_TABLE_LEN, MIN_STORE, REGS, USER_PALETTES, W, store_words,
};
pub use world::World;
pub use rate::{draw_slow, render_fast, render_held, Held, MAX_RATE};
pub use still::{Still, StillLane, STILL_LANES};
/// (psi-probe's references for `fixed::sqrt` and `len`.)
#[doc(hidden)]
pub use tables::SQRT;
pub use tables::{Glyphs, Tables, ATAN};
#[cfg(feature = "tables")]
pub use tables::{FONT_4X6, FONT_5X7, FONT_BOLD, OWN, PALETTES, SIN};
#[cfg(feature = "stats")]
pub use vm::stats;
pub use vm::{
    pair_row, q16, scene_rows, render_rows, render_rows_beam, render_rows_changed, render_rows_ink,
    render_rows_ink_turns, render_rows_still, render_with, Inputs, PairSink, Scratch, BEAM_START,
    Q16_ONE,
};

/// A decimal number (digits, an optional `.` and more digits; no sign) as
/// Q16.16, rounded to the nearest 1/65,536: how the compiler reads numbers,
/// for the sliders' `a=` options too.
pub fn number(s: &str) -> Option<i32> {
    if s.is_empty()
        || s == "."
        || s.matches('.').count() > 1
        || !s.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return None;
    }
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    let mut v: i64 = 0;
    for b in int.bytes() {
        v = v * 10 + (b - b'0') as i64;
        if v > 32767 {
            return None;
        }
    }
    // The fraction to 9 digits, rounded once.
    let mut num: i64 = 0;
    let mut den: i64 = 1;
    for b in frac.bytes().take(9) {
        num = num * 10 + (b - b'0') as i64;
        den *= 10;
    }
    let f = (num * 65536 + den / 2) / den;
    let r = (v << 16) + f;
    (r <= i32::MAX as i64).then_some(r as i32)
}

/// The lane width the effect uses: one panel row.
pub const LANES: usize = 128;

#[cfg(feature = "compiler")]
/// Compile `src` (a new [`Program`] by value: ~2.8 KB; on a small stack use
/// [`compile_into`] on a zeroed allocation instead).
pub fn compile(src: &str) -> Result<Program, Error> {
    let mut p = Program::default();
    compile_into(src, &mut p)?;
    Ok(p)
}

#[cfg(feature = "compiler")]
/// A zeroed [`Program`] on the heap, never on the stack (`compile_into`
/// and its kin take a zeroed one: `program_is_plain_data`); an error, not
/// an abort, when the heap can't give it (the panel's probe builds at a
/// test panel's heap).
fn zeroed_program() -> Result<alloc::boxed::Box<Program>, Error> {
    let layout = core::alloc::Layout::new::<Program>();
    // SAFETY: `Program` is not zero-sized; a non-null block of `layout`
    // from the global allocator is what `Box` frees, and a Program is
    // integers and arrays of integers all the way down: all-zero is valid.
    let p = unsafe { alloc::alloc::alloc_zeroed(layout) } as *mut Program;
    if p.is_null() {
        return Err(Error {
            start: 0,
            end: 0,
            msg: alloc::string::String::from("not enough memory to compile this formula right now"),
        });
    }
    Ok(unsafe { alloc::boxed::Box::from_raw(p) })
}

#[cfg(feature = "compiler")]
/// A Formula compiled for the player ([`psb`]): its program, and when its
/// `look` line composes a blend or an amount, its program as a look.
pub fn build(src: &str) -> Result<alloc::vec::Vec<u8>, Error> {
    build_with(src, true)
}

#[cfg(feature = "compiler")]
/// [`build`] with (`cmp`) or without the fused compares (P0b-4; the same
/// frames): the paint probe's before and after. A look program, when the
/// Formula has one, is [`build`]'s.
#[doc(hidden)]
pub fn build_with(src: &str, cmp: bool) -> Result<alloc::vec::Vec<u8>, Error> {
    let mut d = Declared::default();
    // (One program on the heap, zeroed in place, for the declarations and
    // then the player's (each compile clears it): a Program is 7.4 KB, and
    // the panel's probe builds on its main task's 16 KB stack, or on a
    // thread whose stack is taken from the same heap.)
    let mut play = zeroed_program()?;
    compile_declared(src, &mut play).map(|x| d = x)?;
    let name = d
        .name
        .and_then(|(a, b)| src.get(a as usize..b as usize))
        .unwrap_or("");
    compile_portable_with(src, &mut play, cmp)?;
    let look = match play.meta().look() {
        Some((blend, amount)) if blend != "over" || amount < 100 => {
            let mut l = zeroed_program()?;
            compile_look(src, &mut l)?;
            Some(l)
        }
        _ => None,
    };
    Ok(psb::encode(name, &play, look.as_deref().map(|l| -> &Prog { l })))
}

#[cfg(feature = "compiler")]
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
    prog: &Prog,
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
hr = frac(hour/12 + minute/720)       -- the hour hand as a turn
glow = 0.6 + 0.4*sin(t/2)
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
            world: Default::default(),
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
        ("plasma", super::sketches::PLASMA, 0, 0xab24cddf5c7967cb),
        ("plasma", super::sketches::PLASMA, 2500, 0xa9bfa837e59c89c8),
        ("plasma", super::sketches::PLASMA, 7980, 0x4dbc961b217bc066),
        ("fire", super::sketches::FIRE, 0, 0x26f4a06a29ff088d),
        ("fire", super::sketches::FIRE, 1230, 0xd8e523beb45f8c37),
        ("ring-clock", super::sketches::RING_CLOCK, 0, 0xdef90f39b4011ef1),
        ("ring-clock", super::sketches::RING_CLOCK, 41_250, 0x1f8cc823ba028d9),
        // Edge cases: saturation, division by zero, floored %, the quadrants
        // of atan2, a constant colour, `i`, hash, sliders, comparisons.
        ("saturate", "gray(t*t*t*1000 - 5 + x*30000)", 3000, 0xfe4ec469d4f39325),
        ("div0", "v = x / (y - y)\nw2 = (x - 0.5) / 0\nrgb(v, w2 + 0.5, 0 / 0 + 0.25)", 0, 0xb8aafd09b9ba0b25),
        ("mod", "rgb((x*10 - 5) % 3 / 3, (px - 64) % -7 / -7, (y - 0.5) % 0.25 * 4)", 0, 0xd970023ebb7c3b7d),
        ("atan2", "hsv(atan2(y - 0.5, x - 0.5) + 0.5, 1, len(x - 0.5, y - 0.5)*2)", 0, 0x194326ee958e751a),
        ("const", "rgb(1, 0.5, 0.25)", 0, 0x2433c930830b3325),
        ("index", "hsv(i / 4096, hash(i, 7), 1)", 0, 0xdf6cffa94510c228),
        ("sliders", "rgb(a, b * x, c ? y : 0)", 0, 0xa7934e0e5b269a5),
        ("logic", "gray(x < 0.5 and y > 0.5 or not (px % 8 < 4) ? 1 : 0.2)", 0, 0x678e1542bdd2ef25),
        ("waves", "rgb(tri(x*2 + t), saw(y + t) * 0.5 + 0.5, sqr(x*4) * 0.5 + 0.5)", 1000, 0x145ebcf8439f78b5),
        ("misc", "v = mix(0.2, 0.8, x) + floor(y * 4) / 4 + sqrt(x) - min(x, y) + max(0, cos(t))\ngray(frac(v))", 500, 0xf1790aa4ab192ee2),
        ("smooth", "pal(ocean, smoothstep(0.2, 0.8, x) * smoothstep(1, 0, y))", 0, 0xab603ffbbe0aec0a),
        ("palettes", "pal(rainbow, x)", 0, 0xd2ee745daaab3f25),
        ("clock", "s = second / 60\npal(amber, step(x, s) + 0.3 * step(y, minute / 60))", 0, 0x99e86cb021c87f25),
        // The fonts and bits: every glyph of both fonts, digits out of
        // range (-1, 10, 11), coordinates outside the cell, the steps once
        // a frame and once a row, bit() of negative and fractional values.
        ("glyphs", "ch = 32 + floor(px/5) + floor((31 - py)/8)*25\nhsv(ch/95, 1, glyph(ch, px % 5, (31 - py) % 8))", 0, 0x2b656aa1e30a75fa),
        ("glyphs-small", "ch = 32 + floor(px/4) + floor((31 - py)/6)*32\nhsv(ch/95, 0.5, glyph(ch, px % 4 - 0.5, (31 - py) % 6 + 0.75, small))", 0, 0x13f6d322dfff033),
        ("digits", "rgb(digit(floor(px/5) % 12 - 1, px % 5, 31 - py), digit(floor(px/4) % 12 - 1, px % 4 - 1, 24 - py, small), bit(px, py % 8))", 0, 0x5d8dd69817c2ad40),
        ("digit-levels", "d = second % 10\nr = 31 - py\nrgb(digit(d, 2, r), glyph('A', 1, r - 8)*0.5 + digit(7, 1, 1)*0.25, glyph(':', px % 5, r - 16, small))", 0, 0x284487414908fc89),
        ("digits-bold", "rgb(digit(floor(px/5) % 12 - 1, px % 5 - 1, (31 - py) % 7 - 1, bold), digit(second % 10, floor(px/8) % 4, floor((31 - py)/6), bold), 0.2)", 0, 0xf75ce1f25201139b),
        ("bits", "gray(bit(px*258 - 16000, floor(py/2) - 1) + bit(-px - 0.5, py)*0.5 + bit(t*1000, 3)*0.25)", 1500, 0x8a5d3b643b0aed45),
        // About the middle and from the top; pick of per-frame values per
        // pixel (one step), of per-row values per row, per frame, of
        // per-pixel values (compares and selects), k out of range both
        // ways and fractional; colour literals, mixes of them, nested.
        ("centred", "loop 4\nhsv(angle + t/4, frac(r*2), 1 - py/40 + cx*cy*0.1)", 1000, 0x6eddca84c430ae2b),
        ("pick", "s = second/60; m = minute/60\nk = px/16 - 1.5\nrgb(pick(k, s, m, 0.25, 1), pick(py/8, y, 1 - y, 0.5) + pick(minute % 3, 0.1, 0.2), pick(px/40, x, y, ur + 0.5))", 0, 0x75163221dd870125),
        ("colours", "loop 2\nk = sin(t/2)*0.5 + 0.5\nmix(mix(#000, #f80, x), mix(#123456, rgb(y, k, 1 - y), x), smoothstep(0.2, 0.8, y))", 500, 0x1c2b208300d57219),
        ("colour-rgb", "rgb(mix(#fff, #08c, cy*2 + 1), r*0.6)", 0, 0x872e0a76ebdba8f5),
        // A palette times a value (`pal(P, v) * k`, a layer's with coverage
        // too); noise at a whole z (its four-corner path); values for the
        // still cache (`psi-probe` draws these with it as well).
        ("pal-lit", "loop 2\nlayer { pal(chrono, frac(x + t/2)) * y }\nlayer screen { pal(fire, y, 0.5) * (1 - x) * 1.5 }", 700, 0xd9de37c92532fb64),
        ("noise-flat", "gray(noise(x*8, y*4, 0)*0.5 + noise(px/9, py/5, 3)*0.5 - noise(x, y, -2)*0.1)", 0, 0xf2b3fce19507bdf7),
        // `loop forever`: `t` growing without end (past the Q16.16 range at
        // 9.1 hours: frames at 11 hours and 46 days), its sums and
        // products, fractions, waves, noises (per pixel, per row, in the
        // draw stage), hashes, `%`, compares, smoothstep, `seed`.
        ("forever", FOREVER, 0, 0x9d16f4c044497c86),
        ("forever", FOREVER, 125_000, 0x602a1447e87dbc8d),
        ("forever", FOREVER, 40_000_000, 0xe08248386b41b58d),
        ("forever", FOREVER, 4_000_000_000, 0xf6bae5793f9f4e64),
        // Sprites: 1, 2 and 4 bits a pixel, read per pixel (scaled, moving),
        // per row and once a frame, outside them, and the road's car.
        ("sprites", SPRITES, 1500, 0xb4426b7968cc9336),
        ("sprite-car", SPRITE_CAR, 0, 0x3cdaf547126eb095),
        // Palettes of a formula's own: plain, scaled, covered, in a part,
        // indexed by a sprite.
        ("palettes", PALETTES_OWN, 700, 0x185a3fdd072f78f),
        ("still", "loop 4.8\nd = len(px - 63.5, (15.5 - py)*2)\nk = 64/(d + 0.5)\nu = frac(atan2(15.5 - py, px - 63.5)*2 + t/4.8 + k/64)\nv = frac(k/16 + t/2.4)\ncell = (floor(u*8) + floor(v*4)) % 2\npal(chrono, frac(mix(0.28, 0.78, cell) + t/4.8)) * clamp((d - 6)/12, 0, 1)", 1900, 0x220dbed231f24409),
        // `cycle` (console-b): turned once a frame (the still cache's
        // index), once a row, a palette of its own, scaled and covered.
        ("cycle", CYCLE, 0, 0x29f9a2953849a61f),
        ("cycle", CYCLE, 3330, 0x5f8c2dc99de13d0b),
        ("cycle-rows", CYCLE_ROWS, 1250, 0x4c938ea3065f7d5f),
        // `layer rate N` (console-b): bands at their times (looping, fps
        // 100, past the 32 rows, forever), a slow layer of the clock only.
        ("rate", RATE, 0, 0x83459eac91909c48),
        ("rate", RATE, 1730, 0x902026dda280551c),
        ("rate-100", RATE_100, 2990, 0xa1c2d3a39d0f7014),
        ("rate-clock", RATE_CLOCK, 41_000, 0x5c3d0e0186886076),
        ("rate-forever", RATE_FOREVER, 7_777, 0x1a156c950305d3a6),
        // Tweens of palettes (psi-p0): nested, in halves, at three times.
        ("tween", TWEEN, 0, 0x1bfdf360e2732725),
        ("tween", TWEEN, 2_470, 0xdd00436c9886c525),
        ("tween", TWEEN, 7_990, 0x1c16efa901f24d25),
        ("tween-one", TWEEN_ONE, 1_234, 0x712551f14dd475b9),
    ];

    /// Tween golden formulas (psi-p0): two halves, one of them a tween of
    /// tweens, worked out once a frame; one with a built-in palette.
    pub const TWEEN: &str = "loop 8\npalette dD = #f80, #fc0, #08f, #fff\npalette dN = #400, #320, #002, #224\npalette cD = #f0f, #0ff, #888, #ccc\npalette cN = #202, #022, #111, #333\npalette tun = #000, #444, #f90, #222\nnight = 0.5 + 0.5*sin(t*0.8)\np = frac(t/8)\npalette scene = tween(tween(dD, dN, night), tween(cD, cN, night), p), tween(tun, dN, p)\npal(scene, floor(x*4)/32 + (y > 0.5)*0.5)";
    pub const TWEEN_ONE: &str = "loop 4\npalette sea = #002, #06c, #4cf, #fff\npalette s = tween(sea, rainbow, 0.5 + 0.5*sin(t*1.57))\nlayer { pal(s, x) }\nlayer multiply { cycle(s, y, t/4 + py/64, 0.5) }";
    /// More than 256 steps and 200 values at once (psi-p0): 351 once a
    /// frame, so the draw stage's loop and s go on at steps past 255.
    pub const BIG: &str = "loop 8\nk1 = sin(t*1.0010 + 0.0076)\nk2 = sin(t*1.0020 + 0.0153)\nk3 = sin(t*1.0030 + 0.0229)\nk4 = sin(t*1.0040 + 0.0305)\nk5 = sin(t*1.0050 + 0.0382)\nk6 = sin(t*1.0060 + 0.0458)\nk7 = sin(t*1.0070 + 0.0534)\nk8 = sin(t*1.0080 + 0.0611)\nk9 = sin(t*1.0090 + 0.0687)\nk10 = sin(t*1.0100 + 0.0763)\nk11 = sin(t*1.0110 + 0.0840)\nk12 = sin(t*1.0120 + 0.0916)\nk13 = sin(t*1.0130 + 0.0992)\nk14 = sin(t*1.0140 + 0.1069)\nk15 = sin(t*1.0150 + 0.1145)\nk16 = sin(t*1.0160 + 0.1221)\nk17 = sin(t*1.0171 + 0.1298)\nk18 = sin(t*1.0181 + 0.1374)\nk19 = sin(t*1.0191 + 0.1450)\nk20 = sin(t*1.0201 + 0.1527)\nk21 = sin(t*1.0211 + 0.1603)\nk22 = sin(t*1.0221 + 0.1679)\nk23 = sin(t*1.0231 + 0.1756)\nk24 = sin(t*1.0241 + 0.1832)\nk25 = sin(t*1.0251 + 0.1908)\nk26 = sin(t*1.0261 + 0.1985)\nk27 = sin(t*1.0271 + 0.2061)\nk28 = sin(t*1.0281 + 0.2137)\nk29 = sin(t*1.0291 + 0.2214)\nk30 = sin(t*1.0301 + 0.2290)\nk31 = sin(t*1.0311 + 0.2366)\nk32 = sin(t*1.0321 + 0.2443)\nk33 = sin(t*1.0331 + 0.2519)\nk34 = sin(t*1.0341 + 0.2595)\nk35 = sin(t*1.0351 + 0.2672)\nk36 = sin(t*1.0361 + 0.2748)\nk37 = sin(t*1.0371 + 0.2824)\nk38 = sin(t*1.0381 + 0.2901)\nk39 = sin(t*1.0391 + 0.2977)\nk40 = sin(t*1.0401 + 0.3053)\nk41 = sin(t*1.0411 + 0.3130)\nk42 = sin(t*1.0421 + 0.3206)\nk43 = sin(t*1.0431 + 0.3282)\nk44 = sin(t*1.0441 + 0.3359)\nk45 = sin(t*1.0451 + 0.3435)\nk46 = sin(t*1.0461 + 0.3511)\nk47 = sin(t*1.0471 + 0.3588)\nk48 = sin(t*1.0481 + 0.3664)\nk49 = sin(t*1.0491 + 0.3740)\nk50 = sin(t*1.0502 + 0.3817)\nk51 = sin(t*1.0512 + 0.3893)\nk52 = sin(t*1.0522 + 0.3969)\nk53 = sin(t*1.0532 + 0.4046)\nk54 = sin(t*1.0542 + 0.4122)\nk55 = sin(t*1.0552 + 0.4198)\nk56 = sin(t*1.0562 + 0.4275)\nk57 = sin(t*1.0572 + 0.4351)\nk58 = sin(t*1.0582 + 0.4427)\nk59 = sin(t*1.0592 + 0.4504)\nk60 = sin(t*1.0602 + 0.4580)\nk61 = sin(t*1.0612 + 0.4656)\nk62 = sin(t*1.0622 + 0.4733)\nk63 = sin(t*1.0632 + 0.4809)\nk64 = sin(t*1.0642 + 0.4885)\nk65 = sin(t*1.0652 + 0.4962)\nk66 = sin(t*1.0662 + 0.5038)\nk67 = sin(t*1.0672 + 0.5115)\nk68 = sin(t*1.0682 + 0.5191)\nk69 = sin(t*1.0692 + 0.5267)\nk70 = sin(t*1.0702 + 0.5344)\ns = k1*0.00014 + k2*0.00029 + k3*0.00043 + k4*0.00057 + k5*0.00071 + k6*0.00086 + k7*0.00100 + k8*0.00114 + k9*0.00129 + k10*0.00143 + k11*0.00157 + k12*0.00171 + k13*0.00186 + k14*0.00200 + k15*0.00214 + k16*0.00229 + k17*0.00243 + k18*0.00257 + k19*0.00271 + k20*0.00286 + k21*0.00300 + k22*0.00314 + k23*0.00329 + k24*0.00343 + k25*0.00357 + k26*0.00371 + k27*0.00386 + k28*0.00400 + k29*0.00414 + k30*0.00429 + k31*0.00443 + k32*0.00457 + k33*0.00471 + k34*0.00486 + k35*0.00500 + k36*0.00514 + k37*0.00528 + k38*0.00543 + k39*0.00557 + k40*0.00571 + k41*0.00586 + k42*0.00600 + k43*0.00614 + k44*0.00628 + k45*0.00643 + k46*0.00657 + k47*0.00671 + k48*0.00686 + k49*0.00700 + k50*0.00714 + k51*0.00728 + k52*0.00743 + k53*0.00757 + k54*0.00771 + k55*0.00786 + k56*0.00800 + k57*0.00814 + k58*0.00828 + k59*0.00843 + k60*0.00857 + k61*0.00871 + k62*0.00886 + k63*0.00900 + k64*0.00914 + k65*0.00928 + k66*0.00943 + k67*0.00957 + k68*0.00971 + k69*0.00986 + k70*0.01000\ndraw {\n  for j < 40 {\n    if j % 3 == 0 { dot(j*3 + s, 5, 1) } else { line(j*3, 10 + s, j*3 + 2, 24, 0.5, 1) }\n  }\n  if s > 0 { line(0, 30, 127, 30) } else { line(0, 1, 127, 1) }\n}\ngray(frac(x*3.1 + y*1.7 + s)*0.5 + ink*0.5)";

    /// `cycle` golden formulas: a still index turned once a frame; per-row
    /// turns of a palette of its own, scaled and as a covered layer.
    pub const CYCLE: &str =
        "loop 8\nwave = sin(x*2) + sin(y*1.5) + sin(x*1.5 + y)\ncycle(rainbow, wave/6, t/8)";
    pub const CYCLE_ROWS: &str = "loop 5\npalette sky = #000, #024, #08c, #fc8, #fff\nlayer { cycle(sky, x*0.3 - y*2.5, t/5 + py/64) * (0.6 + x*0.4) }\nlayer screen { cycle(fire, len(cx, cy), -t + py/32, 0.5) }";
    /// `layer rate N` golden formulas.
    pub const RATE: &str = "loop 2\nlayer rate 4 { hsv(x + t/3, 1, frac(y + t)) }\nlayer add { pal(ice, frac(x - t), 0.5) }";
    pub const RATE_100: &str = "loop 3\nfps 100\nlayer rate 40 { cycle(fire, len(cx, cy), t/2) }\nlayer multiply rate 40 { gray(0.5 + y/2) }\nlayer max { gray(px == floor(t*40) % 128) }";
    pub const RATE_CLOCK: &str = "loop 60\nm2 = minute % 10; s = second/60\nlayer rate 50 { gray(px < s*128 or digit(m2, floor(px/8) % 8, py % 8, bold)) }\nlayer min { hsv(x + py/64 + t/10, 1, 1) }";
    pub const RATE_FOREVER: &str = "loop forever\nlayer rate 5 { gray(frac(x + t/5 + seed)) }\nlayer multiply { pal(ice, y + frac(t)) }";

    /// A `loop forever` golden formula: per frame and per pixel.
    pub const FOREVER: &str = "loop forever\nu = t*0.15 + seed*1000\nv = noise(x*8 + t*0.5, y*4, t*0.2)*0.5 + sin(x*3 + t/8)*0.2\nhsv(u + v + noise(u, 5, 0)*0.25, 1 - (t % 7)/14, smoothstep(0, 20, t)*0.5 + 0.5)";

    /// Another: per row, a draw stage, hashes, compares, min and max.
    pub const FOREVER_ROWS: &str = "loop forever\ndraw { for k < 24 { dot(frac(t*0.1 + k/24)*128, noise(t*0.3 + k, 5, 0)*31, hash(floor(t*4), k)) } }\nq = noise(t*0.15 + y*2, 5, 0)\nrgb(q, ink + frac(t*10 + x)*0.25, (t > 60) * 0.5 + min(t/300, x)*0.5)";

    /// A sprites golden formula: three sprites, every level.
    pub const SPRITES: &str = "loop 3\nsprite one {\n  key . 0, # 1\n  .##..##.\n  #..##..#\n}\nsprite two {\n  key . 0, r 0.3, g 0.6, b 1\n  rgb.bgr.rgb.bgr.rgb.bgr.rgb.bgr.rgb.bgr.r\n  .r.g.b.r.g.b.r.g.b.r.g.b.r.g.b.r.g.b.r.g.\n  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n}\nsprite many {\n  key 0 0, 1 0.1, 2 0.2, 3 0.3, 4 0.4, 5 0.5, 6 0.6, 7 0.7, 8 0.8, 9 0.9, A 1, B -1, C 2, D 0.05, E 0.15, F 0.25\n  0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF\n  FEDCBA9876543210FEDCBA9876543210FEDCBA9876543210FEDCBA9876543210\n  02468ACE13579BDF02468ACE13579BDF02468ACE13579BDF02468ACE13579BDF\n}\nf = many[floor(t*5), 1]\nrw = two[py, 1]\nrgb(one[px/8 - t, py/16], many[px/2, py/4 - 2] + f*0.25, two[px - 40, py - 13] + rw*0.5)";

    /// The road's car (`sprite`), its colours from a pick.
    pub const SPRITE_CAR: &str = "sprite car {\n  key . 0, R 1, D 2, L 3\n  ........RRRRRRRRRR........\n  .......RDDDDDDDDDDR.......\n  ......RDDDDDDDDDDDDR......\n  ...RRRRRRRRRRRRRRRRRRRR...\n  .RLLLLRDDDDDDDDDDDDRLLLLR.\n  .RLLLLRDDDDDDDDDDDDRLLLLR.\n  .RRRRRRRRRRRRRRRRRRRRRRRR.\n  .RRRRRRRRRRRRRRRRRRRRRRRR.\n  DDDDDDDDDDDDDDDDDDDDDDDDDD\n}\nk = car[px - 50.5, py - 23]\npick(k, #203040, #e02020, #101010, #ffa020)";

    /// A golden formula with palettes of its own.
    pub const PALETTES_OWN: &str = "loop 2\npalette sea = #001020, #0060a0, #40c0e0, #ffffff\npalette ember = #200000, #c02000,\n  #ffa020\nsprite dot {\n  key . 0, o 1, O 0.5\n  .oOo.\n  oOOOo\n  .oOo.\n}\nfn glow(x, y) = pal(ember, frac(x*4 - t/2), 0.5)\nlayer { pal(sea, y + sin(x*2 + t/2)*0.2) * (0.6 + x*0.4) }\nlayer screen { glow(x, y) }\nlayer add { pal(ember, dot[px % 6, py % 4]) }";

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
        // `under(x, y)`: the picture under at another pixel (pixelated,
        // shifted, held to the panel's edges), in a colour expression.
        ("under-at", "loop 2\ndx = floor(sin(t/2)*3)\nmix(under(floor(px/4)*4 + 2 + dx, floor(py/4)*4 + 2), under(px - 40, py + 3), x < 0.3) * 0.9", 500, true, 0x78453c5a54159a10),
        ("identity", "rgb(ur, ug, ub)", 0, true, 0x2c7a29e300e4c8cf),
        ("luma", "gray(ul)", 0, true, 0xc7f57bec83c88cfe),
        ("grade", "rgb(ur*1.2 - 0.05, ug, ub*0.7 + ul*0.2)", 0, true, 0x56bbda81836b00e),
        ("scanlines", "layer multiply { gray(py % 2 ? 1 : 0.55) }", 0, true, 0x5aaeedc2566b0254),
        ("vignette", "d = len((x - 0.5)*2, (y - 0.5)*1.2)\nlayer multiply { gray(1 - smoothstep(0.6, 1.3, d)) }", 0, true, 0x9efd01faa9a30409),
        ("blends", "loop 4\nlayer add { hsv(x + t/4, 1, 0.5) }\nlayer screen { gray(y, 0.5) }\nlayer max { rgb(0, step(0.5, x), 0) }\nlayer min { gray(0.9) }", 1000, true, 0x3e1322d7c586ced4),
        ("cover", "layer { rgb(1, 0, 0, x) }\nlayer over { pal(ice, y, 0.25) }\nlayer { gray(0.5, step(px % 8, 1)) }", 0, true, 0x726c06c1ae1f5c48),
        ("hidden", "layer add { noise(x*8, y*4, t) }\nlayer { hsv(x, 1, 1) }\nlayer multiply { gray(1 - ul*0.5) }", 0, true, 0x6e7731edba40d20b),
        ("alone", "layer { hsv(x, 1, 1) }\nlayer add { gray(ur + 0.2, 0.5) }", 0, false, 0xb62345c4247f9a5),
        ("cover-colour", "layer multiply { mix(#fff, #400000, r*0.7) }\nlayer { rgb(#ffbf4d, step(abs(cy), 0.1)) }\nlayer screen { mix(rgb(ur, ug, ub), #304050, x) }", 0, true, 0x1e65516f4765655a),
        ("scoped", "s = sin(t)\nk = x*4\nlayer { v = frac(k + s)\nrgb(v, ub, 0) }\nlayer screen { v = frac(k*2 - y)\nq = cos(t)\npal(fire, v*q, 0.75) }", 2500, true, 0xe01c5295366f8613),
    ];

    /// Frames of formulas that draw (`draw { }`, `for`, `table`, `line`,
    /// `dot`, `ink`), rendered with a canvas ([`crate::render_rows_ink`]),
    /// `over`: over [`picture`]. Lines in all eight octants and both
    /// directions, ties at ½, clipping on every edge, the step caps, the max
    /// rule, dots, tables at every level, loops of 1 and 1,024 turns, loops
    /// nested, `ink` through a part and through a look.
    pub const DRAWN: &[(&str, &str, u32, bool, u64)] = &[
        // 379 steps, 233 values; the draw stage from step 351 (psi-p0).
        ("big", BIG, 0, false, 0x62c00cf899951f3f),
        ("big", BIG, 5_150, true, 0x623ea41cb2d93df0),
        // Paint (the scene stage): every primitive, the palettes, a
        // sprite in colours flipped, text and a number, an `if`/`else` in a
        // loop, painted over a colour line and alone (over the picture).
        ("paint", "loop 4\nsprite s {\n  key . 0, R #f00, W #fff\n  .RR.\n  RWWR\n}\ndraw {\n  cls(#102)\n  rectfill(10, 4, 40, 20, #f80)\n  rect(12, 6, 30, 18, pal(ice, 0.5))\n  circfill(64 + 30*sin(t/4), 16, 8, pal(fire, 0.7))\n  circ(64, 16, 12.5, #0f0)\n  trifill(90, 28, 110, 4, 126, 28, gray(0.4))\n  line(0, 31, 127, 0, #fff)\n  print(\"Hi! 42\", 4, 24, #0ff)\n  print(second - 40, 100, 2, #ff0, small, 3)\n  print(minute, 80, 2, #f0f, bold, 2)\n  spr(s, 50, 26)\n  spr(s, 56, 26, 3)\n  for k < 8 {\n    if k % 2 == 0 { pset(k*4, 2, #f0f) } else { pset(k*4, 3, gray(k/8)) }\n  }\n}\ngray(x)", 1000, false, 0x17f4f937f0881e04),
        ("paint-alone", "draw { circfill(64, 16, 10, #f00); line(0, 0, 127, 31, #0f0); dot(5, 5, #fff) }", 0, true, 0x423ea6c55f0f8e58),
    ("octants", "draw {\n  for k < 16 {\n    an = k/16 + 1/64\n    ex = 64 + cos(an)*60; ey = 16 + sin(an)*15\n    rev = k % 2\n    line(mix(64, ex, rev), mix(16, ey, rev), mix(ex, 64, rev), mix(ey, 16, rev), 1, k/16)\n  }\n}\npal(rainbow, ink)", 0, false, 0xbca6efe0672e7e),
    ("ties", "draw {\n  dot(10.5, 3.5); dot(20.49, 3.5, 0.5); dot(-0.5, 0); dot(127.5, 31.5); dot(127.49, 31.49, 0.25)\n  line(0.5, 10.5, 20.5, 17.5); line(30.5, 20.5, 30.5, 20.5, 0.5); line(40, 5, 52, 5.5, 0, 1)\n}\ngray(ink)", 0, false, 0xadf5a3b2dcd16f49),
    ("clipping", "draw {\n  line(-20, 16, 140, 20); line(64, -10, 70, 40, 0.5)\n  line(-30000, -30000, 30000, 30000, 0.75); line(-5, -5, -1, 40); line(130, 0, 200, 31)\n  line(0, 31.6, 127, 31.4); line(10, -0.6, 120, -0.4)\n}\ngray(ink)", 0, false, 0x69688b0d838cfe57),
    ("caps", "draw {\n  for k < 64 { line(-512, 10 - k, 639, 52 - k, 1 - k/64, 0.5) }\n  for j < 4 { dot(j*10, 30) }\n}\nrgb(ink, ink*0.5, 0.1)", 0, false, 0x3cb2d30c3ee2915e),
    ("max-rule", "loop 2\ndraw {\n  line(0, 0, 127, 31, 0.3); line(0, 31, 127, 0, 0.8); line(64, 0, 64, 31, 0, 1)\n  for k < 8 { dot(64, k*4, k/8) }\n}\ngray(ink)", 0, false, 0x62ee8a1c01d7a1df),
    ("tables", "table T = 0, 0.25, 0.5, 1, -1\ntable P = 3, 9, 27\nf = T[floor(t*2)]\ndraw {\n  for k < 5 { dot(P[k % 3]*4 + k, T[k]*20 + 5, T[k] + 0.5) }\n  dot(100, 10, f + 0.5)\n}\nrgb(T[px % 6] + ink, T[py % 4], f + T[-1] + T[2.9])", 1000, false, 0x1feb9c1298243e5f),
    ("loops", "loop 4\ndraw {\n  for k < 1 { line(0, 0, 127, 0) }\n  for j < 1024 { dot(j % 128, floor(j/128)*4 + 2, hash(j, floor(t))) }\n}\npal(fire, ink)", 2000, false, 0xb8a6a36408cde385),
    ("nested", "loop 4.8\ndraw {\n  for s < 12 {\n    th = s/12 + t/4.8\n    for r < 6 {\n      ph = r/6\n      rad = 30 + 10*cos(ph)\n      x0 = 64 + rad*cos(th); y0 = 16 + 10*sin(ph) + rad*sin(th)*0.2\n      x1 = 64 + rad*cos(th + 1/12); y1 = 16 + 10*sin(ph) + rad*sin(th + 1/12)*0.2\n      line(x0, y0, x1, y1, 0.3 + r/9, 1 - r/9)\n    }\n  }\n}\nhsv(x + t/4.8, 0.7, ink)", 1200, false, 0xdcef36f6fa52496b),
    ("forever-rows", super::golden::FOREVER_ROWS, 3_500, false, 0xb103becafa6ec425),
    ("forever-rows", super::golden::FOREVER_ROWS, 40_000_000, true, 0x19e63a938a8b8dc0),
    ("ink-levels", "loop 2\ndraw {\n  for k < 16 { line(k*8, 0, k*8 + 7, 31, k/16) }\n  for j < 3 { dot(t*60 + j, 16, j/3) }\n}\nn = ink(16)\nrgb(pick(n, 0, 0.1, 0.3, 0.5, 0.7, 0.9, 1, 0.8, 0.6, 0.4, 0.2, 0.15, 0.25, 0.35, 0.45, 0.55), ink(3)/3, ink(255)/255)", 700, false, 0x1c96c08131bb6ba0),
    ("ink-look", "loop 4.8\ndraw { for e < 6 { line(64 + 30*cos(e/6 + t/4.8), 16 + 12*sin(e/6 + t/4.8), 64 + 30*cos(e/6 + 1/6 + t/4.8), 16 + 12*sin(e/6 + 1/6 + t/4.8)) } }\nlayer screen { rgb(#40c0ff * ink, 0.4) }", 600, true, 0x7712d62a80b1dbca),
    ];
}

#[cfg(feature = "compiler")]
/// Formulas that make the compiler work hardest within what the panel
/// accepted inline (a path of at most [`worst::PATH_MAX`], as `fx_psi::path` writes it: the
/// source compacted, `+` as `%2B`): compile-time tests, the bench, the
/// host's stack probe (`examples/stack.rs`) and `psi-probe`'s stack report.
#[doc(hidden)]
pub mod worst {
    use alloc::format;
    use alloc::string::String;

    /// The size the worst cases are made to: a path of 2,048 B, the inline
    /// cap before Formulas were played by reference (~700-800 tokens). What
    /// compiles on the device is bounded by its heap check instead
    /// (`compile_heap_estimate`: about 1,300 tokens with ~63 KB free;
    /// Endless road is ~1,100); nesting, not length, bounds the stack.
    pub const PATH_MAX: usize = 2048;

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
            if path_len(&format!("{next}{}", end(k))) > PATH_MAX {
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
        while path_len(&format!("{s} + x*1.{k:03}\ngray(v)")) <= PATH_MAX {
            s = format!("{s} + x*1.{k:03}");
            k += 1;
        }
        format!("{s}\ngray(v)")
    }

    /// Functions calling functions, each several times: the most reading
    /// again (stopped by the compiler's bounds on it).
    pub fn functions() -> String {
        let mut s = String::from("fn f0(a, b) = sin(a*3 + t)*b\n");
        for k in 1.. {
            let j = k - 1;
            let next = format!(
                "{s}fn f{k}(a, b) = f{j}(a*0.5, b) + f{j}(b, a) + f{j}(a + 0.1, b) + f{j}(a, b*2) + f{j}(1 - a, b)\n"
            );
            let src = format!("{next}gray(f{k}(x, y))");
            if path_len(&src) > PATH_MAX || crate::compile(&src).is_err() {
                return format!("{s}gray(f{j}(x, y))");
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

    /// Functions inside functions, each called in a layer at another place,
    /// as deep as the cap allows, with a misspelt name at the bottom.
    pub fn functions_in_layers() -> String {
        let mut best = String::new();
        for depth in 1..12 {
            let mut s = String::from("fn p0(x, y) {\nv = sin(cos(x*3 + tme))\nv*y\n}\n");
            for k in 1..depth {
                let j = k - 1;
                s = format!("{s}fn p{k}(x, y) = p{j}(x*0.5, sin(y)) + p{j}(1 - x, y)*0.5\n");
            }
            let src = format!(
                "{s}layer {{ gray(p{}(x, y)) }}\nlayer add {{ gray(p{}(y, x), 0.5) }}",
                depth - 1,
                depth - 1
            );
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

    /// A named generator.
    pub type Case = (&'static str, fn() -> String);

    pub const ALL: [Case; 14] = [
        ("many locals", many_locals),
        ("chain", chain),
        ("distinct terms", distinct),
        ("functions", functions),
        ("nested", nested),
        ("nested digit", nested_digit),
        ("nested mix", nested_mix),
        ("nested pick", nested_pick),
        ("functions in layers", functions_in_layers),
        ("draw", draw),
        ("nested sprite", nested_sprite),
        ("nested forever", nested_forever),
        ("nested palette", nested_palette),
        ("declared", declared),
    ];
}

#[cfg(all(test, feature = "tables"))]
mod tests;

#[cfg(test)]
mod fixed_tests;
