//! Kernels against the VM, on the host, through the emulator (`emu`): the
//! goldens (their hashes), the bundled formulas at several times with and
//! without the still cache, and random formulas. A kernel that draws one
//! byte differently from the VM fails here.

use super::emu::{Emu, EmuTarget};
use super::lower::{lower, Lowered};
use super::{check_rows, frame, CHECK_ROWS};
use crate::program::{Program, H, W};
use crate::still::Still;
use crate::tables::OWN;
use crate::vm::{Inputs, Scratch};
use crate::{compile, golden, render_rows_changed, render_rows_still, Ink};
use std::path::Path;

/// What a frame is drawn with besides the program: the picture under it
/// and whether it has a canvas (each frame its own, drawn on a full call).
#[derive(Clone, Copy)]
struct With<'a> {
    under: Option<&'a [u8]>,
    ink: bool,
}

const PLAIN: With<'static> = With {
    under: None,
    ink: false,
};

/// The picture the tests draw over (`golden::picture`).
fn picture() -> Vec<u8> {
    let mut u = vec![0u8; W * H * 3];
    golden::picture(&mut u);
    u
}

/// A frame through the kernel (`still` as the player passes it).
fn kernel_frame(
    prog: &Program,
    low: &Lowered,
    inputs: &Inputs,
    still: Option<&mut Still>,
    with: With,
) -> (Vec<u8>, u64) {
    let (out, steps, _) = kernel_frame_changed(prog, low, inputs, still, with, None);
    (out, steps)
}

/// [`kernel_frame`] over `before` (the picture held before; zeros if
/// `None`), and what changed when `before` is given.
fn kernel_frame_changed(
    prog: &Program,
    low: &Lowered,
    inputs: &Inputs,
    still: Option<&mut Still>,
    with: With,
    before: Option<&[u8]>,
) -> (Vec<u8>, u64, [u32; H]) {
    let mut t = EmuTarget {
        emu: Emu::new(&low.code),
    };
    let mut ctx = vec![0u32; low.words.len()];
    let mut s = Scratch::<128>::new();
    // (Word-aligned, as the player's canvas.)
    let mut words = vec![0u32; W * H * 3 / 4];
    let out: &mut [u8] = bytemuck_bytes(&mut words);
    if let Some(b) = before {
        out.copy_from_slice(b);
    }
    let base = out.as_mut_ptr();
    // SAFETY: row < 32: within `out`.
    let dest = |row: usize| unsafe { base.add(row * W * 3) };
    let mut ink = with.ink.then(Ink::boxed);
    let mut changed = [0u32; H];
    frame(
        &mut t,
        low,
        &mut ctx,
        prog,
        &mut s,
        ink.as_deref_mut(),
        still,
        inputs,
        &OWN,
        with.under,
        u32::MAX,
        &dest,
        before.is_some().then_some(&mut changed),
    );
    (out.to_vec(), t.emu.steps, changed)
}

fn bytemuck_bytes(w: &mut [u32]) -> &mut [u8] {
    // SAFETY: u32s as their bytes, the same memory.
    unsafe { core::slice::from_raw_parts_mut(w.as_mut_ptr().cast::<u8>(), w.len() * 4) }
}

fn vm_frame(prog: &Program, inputs: &Inputs, still: Option<&mut Still>, with: With) -> Vec<u8> {
    let mut s = Scratch::<128>::new();
    let mut out = vec![0u8; W * H * 3];
    let mut ink = with.ink.then(Ink::boxed);
    render_rows_still::<128>(
        prog,
        &mut s,
        ink.as_deref_mut(),
        still,
        inputs,
        &OWN,
        with.under,
        u32::MAX,
        &mut out,
    );
    out
}

/// The first pixel two frames differ at, for a message.
fn first_diff(a: &[u8], b: &[u8]) -> Option<(usize, usize, [u8; 3], [u8; 3])> {
    (0..W * H)
        .find(|&p| a[p * 3..p * 3 + 3] != b[p * 3..p * 3 + 3])
        .map(|p| {
            let c = |v: &[u8]| [v[p * 3], v[p * 3 + 1], v[p * 3 + 2]];
            (p % W, p / W, c(a), c(b))
        })
}

/// The kernel's frames against the VM's at `times` (ms), plain and with
/// the still cache (kept, then used): `None` when the formula is refused
/// (the reason), else the instructions a pixel the kernel took.
fn same_as_vm(name: &str, src: &str, prog: &Program, times: &[u32]) -> Result<f64, &'static str> {
    let u = picture();
    // Over the picture (what a look draws over), with a canvas if it draws.
    same_as_vm_with(name, src, prog, times, With { under: Some(&u), ink: prog.draws() })?;
    // ...and alone (nothing under: black; ink 0 without a canvas).
    same_as_vm_with(name, src, prog, times, PLAIN)
}

/// [`same_as_vm`] drawn with `with`.
fn same_as_vm_with(
    name: &str,
    src: &str,
    prog: &Program,
    times: &[u32],
    with: With,
) -> Result<f64, &'static str> {
    let low = lower(prog)?;
    let mut steps = 0;
    let mut frames = 0;
    for &ms in times {
        let mut inputs = golden::inputs(ms);
        inputs.set_ms(prog, u64::from(ms));
        if !crate::range::inputs_in_range(prog, &inputs) {
            continue;
        }
        let want = vm_frame(prog, &inputs, None, with);
        let (got, n) = kernel_frame(prog, &low, &inputs, None, with);
        if let Some(d) = first_diff(&got, &want) {
            panic!(
                "{name} at {ms} ms: the kernel's pixel ({}, {}) is {:?}, the VM's {:?}\n{src}\n{}",
                d.0,
                d.1,
                d.2,
                d.3,
                prog.listing()
            );
        }
        steps += n;
        frames += 1;
        if prog.still_lanes() > 0 {
            let mut st = Still::new();
            assert!(st.take(prog));
            let (kept, _) = kernel_frame(prog, &low, &inputs, Some(&mut st), with);
            assert!(
                st.holds(prog),
                "{name}: the kernel's frame kept no still lanes"
            );
            let (used, _) = kernel_frame(prog, &low, &inputs, Some(&mut st), with);
            assert!(
                first_diff(&kept, &want).is_none(),
                "{name} at {ms} ms: the kernel's frame keeping the still cache differs"
            );
            assert!(
                first_diff(&used, &want).is_none(),
                "{name} at {ms} ms: the kernel's frame from the still cache differs"
            );
        }
    }
    Ok(if frames == 0 {
        0.0
    } else {
        steps as f64 / frames as f64 / (W * H) as f64
    })
}

#[test]
fn kernels_draw_the_goldens() {
    let (mut ok, mut refused) = (0, Vec::new());
    for &(name, src, ms, want) in golden::FRAMES {
        let prog = compile(src).unwrap();
        let low = match lower(&prog) {
            Ok(l) => l,
            Err(why) => {
                refused.push(format!("{name}: {why}"));
                continue;
            }
        };
        let inputs = golden::inputs(ms);
        if !crate::range::inputs_in_range(&prog, &inputs) {
            refused.push(format!("{name}: a checked frame"));
            continue;
        }
        let (got, _) = kernel_frame(&prog, &low, &inputs, None, PLAIN);
        let h = golden::fnv(&got);
        if h != want {
            let vm = vm_frame(&prog, &inputs, None, PLAIN);
            panic!(
                "golden {name} at {ms} ms through the kernel: {h:#018x}, want {want:#018x}; first difference {:?}\n{}",
                first_diff(&got, &vm),
                prog.listing()
            );
        }
        ok += 1;
    }
    println!(
        "goldens through kernels: {ok} the same, {} on the VM: {refused:#?}",
        refused.len()
    );
    assert!(ok >= golden::FRAMES.len() / 2, "only {ok} goldens lowered");
}

/// The goldens over a picture and the drawn ones, through kernels: their
/// hashes.
#[test]
fn kernels_draw_the_goldens_over_and_drawn() {
    let u = picture();
    let (mut ok, mut refused) = (0, Vec::new());
    for &(name, src, ms, over, want) in golden::OVER.iter().chain(golden::DRAWN) {
        let prog = compile(src).unwrap();
        let low = match lower(&prog) {
            Ok(l) => l,
            Err(why) => {
                refused.push(format!("{name}: {why}"));
                continue;
            }
        };
        let inputs = golden::inputs(ms);
        if !crate::range::inputs_in_range(&prog, &inputs) {
            refused.push(format!("{name}: a checked frame"));
            continue;
        }
        let with = With { under: over.then_some(&u[..]), ink: true };
        let (got, _) = kernel_frame(&prog, &low, &inputs, None, with);
        let h = golden::fnv(&got);
        if h != want {
            let vm = vm_frame(&prog, &inputs, None, with);
            panic!(
                "golden {name} at {ms} ms through the kernel: {h:#018x}, want {want:#018x}; first difference {:?}\n{}",
                first_diff(&got, &vm),
                prog.listing()
            );
        }
        ok += 1;
    }
    println!("over/drawn goldens through kernels: {ok} the same, on the VM: {refused:#?}");
    assert!(refused.len() <= 2, "{refused:?}");
}

/// Random formulas with layers (every blend, with and without coverage),
/// `ur ug ub ul` and a draw stage's `ink`, against the VM over the picture.
#[test]
fn random_layers_under_and_ink_draw_the_same_through_kernels() {
    let mut r = Rng(0x1d17_0b5e_55ed_0002);
    let (mut ok, mut why) = (0, std::collections::BTreeMap::<&str, u32>::new());
    let blends = ["", "over", "add", "multiply", "screen", "max", "min"];
    let pals = ["fire", "ice", "chrono", "amber", "rainbow"];
    let leaf = |r: &mut Rng| ["ur", "ug", "ub", "ul", "ink", "x", "y"][r.below(7)].to_string();
    for k in 0..400 {
        let mut src = format!("loop {}\n", 1 + k % 5);
        if k % 3 != 0 {
            src.push_str("draw {\n  for j < 24 {\n    line(hash(j, 3)*128, hash(j, 5)*32, hash(j, 7)*128 + t*9, hash(j, 9)*32, hash(j, 1), 0.5)\n  }\n  dot(px0, 7, 0.75)\n}\npx0 = 0\n".replace("px0 = 0\n", "").replace("px0", "64").as_str());
        }
        src.push_str(&format!("v = {}\n", expr(&mut r, 2).replace("hour", &leaf(&mut r))));
        src.push_str(&format!("u = {} * {}\n", leaf(&mut r), expr(&mut r, 2)));
        let n = 1 + r.below(3);
        for j in 0..n {
            let cov = ["", ", 0.4", ", v", ", x", ", a", ", 1", ", u*2 - 0.5"][r.below(7)];
            let colour = match r.below(6) {
                0 => format!("gray({}{cov})", leaf(&mut r)),
                1 => format!("rgb({}, v, {}{cov})", leaf(&mut r), leaf(&mut r)),
                2 => format!("hsv(v + {}, 1, u{cov})", leaf(&mut r)),
                3 => format!("pal({}, {}{cov})", pals[r.below(5)], leaf(&mut r)),
                4 => format!("pal({}, {}{cov}) * {}", pals[r.below(5)], leaf(&mut r), ["0.5", "y", "u", "1.5", "v"][r.below(5)]),
                _ => format!("rgb(ur*1.2 - 0.05, ug, ub*0.7 + ul*0.2{cov})"),
            };
            let b = if j == 0 && k % 2 == 0 { "" } else { blends[r.below(blends.len())] };
            src.push_str(&format!("layer {b} {{ {colour} }}\n"));
        }
        let Ok(prog) = compile(&src) else { continue };
        match same_as_vm("random layers", &src, &prog, &[0, 1234, 3210]) {
            Ok(_) => ok += 1,
            Err(w) => *why.entry(w).or_default() += 1,
        }
    }
    println!("random layers/under/ink through kernels: {ok}; on the VM: {why:?}");
    assert!(ok >= 250, "only {ok} lowered: {why:?}");
}

/// The bundled formulas (the web app's examples and psi's own).
fn bundled() -> Vec<(String, String)> {
    let mut v = Vec::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in [root.join("formulas"), root.join("../opendmd-web/formulas")] {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();
        for f in files {
            if f.extension().is_some_and(|e| e == "psi") {
                let name = f.file_name().unwrap().to_string_lossy().into_owned();
                v.push((name, std::fs::read_to_string(&f).unwrap()));
            }
        }
    }
    v
}

#[test]
fn bundled_formulas_draw_the_same_through_kernels() {
    let mut report = Vec::new();
    let mut lowered = 0;
    for (name, src) in bundled() {
        let Ok(prog) = compile(&src) else { continue };
        let lp = prog.loop_ms().max(1);
        let times = [0, 777, lp / 3, lp / 2 + 13, lp.saturating_sub(20), 61_234];
        match same_as_vm(&name, &src, &prog, &times) {
            Ok(ipp) => {
                lowered += 1;
                let low = lower(&prog).unwrap();
                report.push(format!(
                    "{name}: {} B, {} words, {ipp:.1} instructions a pixel{}",
                    low.code.len(),
                    low.words.len(),
                    if low.rows_at.is_some() { ", the row stage too" } else { "" }
                ));
            }
            Err(why) => report.push(format!("{name}: VM ({why})")),
        }
    }
    println!("{}", report.join("\n"));
    assert!(lowered >= 10, "only {lowered} bundled formulas lowered");
}

/// A random well-formed expression of depth ≤ `d` (the golden fuzz's, with
/// the steps a kernel has).
fn expr(r: &mut Rng, d: u32) -> String {
    const LEAVES: &[&str] = &[
        "x", "y", "px", "py", "i", "t", "hour", "a", "0.5", "3", "-2.25", "100", "w", "32000",
        "-0.001",
    ];
    if d == 0 || r.below(4) == 0 {
        return LEAVES[r.below(LEAVES.len())].to_string();
    }
    let e = |r: &mut Rng| expr(r, d - 1);
    match r.below(12) {
        0..=3 => {
            let op = [
                "+", "-", "*", "/", "%", "<", ">=", "==", "and", "or", "<=", ">", "!=",
            ][r.below(13)];
            format!("({} {op} {})", e(r), e(r))
        }
        4 => format!("-({})", e(r)),
        5 => format!("({} ? {} : {})", e(r), e(r), e(r)),
        6 | 7 => {
            let f = [
                "sin", "cos", "tri", "saw", "sqr", "abs", "floor", "frac", "sqrt", "not ",
            ][r.below(10)];
            format!("{f}({})", e(r))
        }
        8 => {
            let f = ["min", "max", "step", "hash", "len", "atan2", "bit"][r.below(7)];
            format!("{f}({}, {})", e(r), e(r))
        }
        9 => format!(
            "({} * {})",
            e(r),
            ["0.5", "0.25", "1/8", "3", "x", "a"][r.below(6)]
        ),
        10 => format!("(floor({}) + floor({})) % 2", e(r), e(r)),
        _ => {
            let f = ["clamp", "mix", "smoothstep", "noise"][r.below(4)];
            format!("{f}({}, {}, {})", e(r), e(r), e(r))
        }
    }
}

/// xorshift64*.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn random_formulas_draw_the_same_through_kernels() {
    let mut r = Rng(0x1d17_0b5e_55ed_0001);
    let (mut ok, mut why) = (0, std::collections::BTreeMap::<&str, u32>::new());
    let pals = ["fire", "ice", "chrono", "amber", "rainbow", "gray", "ocean"];
    for k in 0..500 {
        let colour = match k % 7 {
            0 => format!("hsv({}, 1, {})", expr(&mut r, 3), expr(&mut r, 2)),
            1 => format!(
                "rgb({}, {}, {})",
                expr(&mut r, 3),
                expr(&mut r, 2),
                expr(&mut r, 2)
            ),
            2 => format!("gray({})", expr(&mut r, 3)),
            3 => format!("pal({}, {})", pals[r.below(pals.len())], expr(&mut r, 3)),
            4 => format!(
                "pal({}, {}) * {}",
                pals[r.below(5)],
                expr(&mut r, 3),
                expr(&mut r, 2)
            ),
            5 => format!(
                "pal({}, {} ? {} : {}) * {}",
                pals[r.below(5)],
                expr(&mut r, 2),
                ["0.3", "a", "t/4"][r.below(3)],
                ["0.7", "b", "frac(t)"][r.below(3)],
                ["y", "0.5", "x", "floor(x*8)/8"][r.below(4)]
            ),
            _ => format!(
                "pal({}, pick(floor({}), 0.1, a, 0.6, t/8)) * {}",
                pals[r.below(5)],
                expr(&mut r, 2),
                ["1", "0.5", "y", "a"][r.below(4)]
            ),
        };
        let src = format!(
            "loop {}\nv = {}\nu = {}\n{colour}",
            1 + k % 9,
            expr(&mut r, 3),
            expr(&mut r, 3)
        );
        let Ok(prog) = compile(&src) else { continue };
        match same_as_vm("random", &src, &prog, &[0, 1234, 4321]) {
            Ok(_) => ok += 1,
            Err(w) => *why.entry(w).or_default() += 1,
        }
    }
    println!("random formulas through kernels: {ok}; on the VM: {why:?}");
    assert!(ok >= 300, "only {ok} random formulas lowered");
}

/// Random row stages (values of `y`, `py`, `t` and the sliders only,
/// helpers among them) through the row kernel, against the VM: a row value
/// a pixel then shows.
#[test]
fn random_row_stages_draw_the_same_through_kernels() {
    let mut r = Rng(0x1d17_0b5e_55ed_0003);
    let (mut ok, mut rows, mut why) = (0, 0, std::collections::BTreeMap::<&str, u32>::new());
    let leaf = ["y", "py", "t", "a", "0.5", "3", "-2.25", "100", "seed"];
    fn row(r: &mut Rng, d: u32, leaf: &[&str]) -> String {
        if d == 0 || r.below(4) == 0 {
            return leaf[r.below(leaf.len())].to_string();
        }
        let e = |r: &mut Rng| row(r, d - 1, leaf);
        match r.below(9) {
            0..=2 => {
                let op = ["+", "-", "*", "/", "%", "<", ">=", "==", "and", "or"][r.below(10)];
                format!("({} {op} {})", e(r), e(r))
            }
            3 => format!("({} ? {} : {})", e(r), e(r), e(r)),
            4 => format!("{}({})", ["sin", "cos", "tri", "saw", "sqr", "abs", "floor", "frac", "sqrt", "-"][r.below(10)], e(r)),
            5 => format!("{}({}, {})", ["min", "max", "step", "hash", "len", "atan2", "bit"][r.below(7)], e(r), e(r)),
            6 => format!("{}({}, {}, {})", ["clamp", "mix", "smoothstep", "noise"][r.below(4)], e(r), e(r), e(r)),
            7 => format!("T[{}]", e(r)),
            _ => format!("digit(floor({}) % 10, {}, {})", e(r), e(r), e(r)),
        }
    }
    for k in 0..300 {
        let forever = k % 4 == 0;
        let lf: Vec<&str> = leaf.iter().copied().filter(|&l| forever || l != "seed").collect();
        let src = format!(
            "{}table T = 0.1, -3, 0.9, 7, 0.3\nq = {}\nr = {}\npal(fire, frac(x * q + r))",
            if forever { "loop forever\n" } else { "loop 3\n" },
            row(&mut r, 4, &lf),
            row(&mut r, 3, &lf)
        );
        let prog = match compile(&src) {
            Ok(p) => p,
            Err(e) => {
                if k < 3 {
                    println!("{src}: {}", e.msg);
                }
                continue;
            }
        };
        if lower(&prog).is_ok_and(|l| l.rows_at.is_some()) {
            rows += 1;
        }
        match same_as_vm("random rows", &src, &prog, &[0, 1234, 2999]) {
            Ok(_) => ok += 1,
            Err(w) => *why.entry(w).or_default() += 1,
        }
    }
    println!("random row stages through kernels: {ok} ({rows} with a row kernel); on the VM: {why:?}");
    assert!(rows >= 200, "only {rows} row kernels");
}

/// The rows the player checks a kernel on, drawn alone, are the full
/// frame's rows (the check renders only those; the canvas is the full
/// frame's, not drawn again).
#[test]
fn the_check_rows_alone_are_the_frames_rows() {
    let u = picture();
    for (name, src) in bundled() {
        let Ok(prog) = compile(&src) else { continue };
        let Ok(low) = lower(&prog) else { continue };
        let inputs = golden::inputs(500);
        if !crate::range::inputs_in_range(&prog, &inputs) {
            continue;
        }
        let mut s = Scratch::<128>::new();
        let mut want = vec![0u8; W * H * 3];
        let mut ink = Ink::boxed();
        render_rows_still::<128>(&prog, &mut s, Some(&mut ink), None, &inputs, &OWN, Some(&u), u32::MAX, &mut want);
        let mut t = EmuTarget {
            emu: Emu::new(&low.code),
        };
        let mut ctx = vec![0u32; low.words.len()];
        let mut buf = [[0u8; W * 3]; CHECK_ROWS.len()];
        let base = buf.as_mut_ptr() as *mut u8;
        let dest = |row: usize| {
            let j = CHECK_ROWS.iter().position(|&r| r == row).unwrap();
            // SAFETY: j < 3.
            unsafe { base.add(j * W * 3) }
        };
        frame(
            &mut t,
            &low,
            &mut ctx,
            &prog,
            &mut s,
            Some(&mut ink),
            None,
            &inputs,
            &OWN,
            Some(&u),
            check_rows(),
            &dest,
            None,
        );
        for (j, &row) in CHECK_ROWS.iter().enumerate() {
            assert_eq!(
                &buf[j][..],
                &want[row * W * 3..(row + 1) * W * 3],
                "{name}: row {row}"
            );
        }
    }
}

/// What a kernel frame says changed is what the VM's says, over a frame
/// before it (the bundled formulas, a frame 40 ms on).
#[test]
fn kernels_say_what_changed_as_the_vm_does() {
    let u = picture();
    let mut n = 0;
    for (name, src) in bundled() {
        let Ok(prog) = compile(&src) else { continue };
        let Ok(low) = lower(&prog) else { continue };
        let lp = prog.loop_ms().max(1000);
        for ms in [0, lp / 2] {
            let before_in = golden::inputs(ms);
            let mut inputs = golden::inputs(ms + 40);
            inputs.set_ms(&prog, u64::from(ms + 40));
            if !crate::range::inputs_in_range(&prog, &inputs) {
                continue;
            }
            let with = With { under: Some(&u), ink: prog.draws() };
            let before = vm_frame(&prog, &before_in, None, with);
            let mut s = Scratch::<128>::new();
            let mut ink = Ink::boxed();
            let mut words = vec![0u32; W * H * 3 / 4];
            let want = bytemuck_bytes(&mut words);
            want.copy_from_slice(&before);
            let mut want_ch = [0u32; H];
            render_rows_changed::<128>(&prog, &mut s, Some(&mut ink), None, &inputs, &OWN, Some(&u), u32::MAX, want, &mut want_ch);
            let (got, _, got_ch) = kernel_frame_changed(&prog, &low, &inputs, None, with, Some(&before));
            assert_eq!(&got[..], &want[..], "{name} at {ms} ms");
            assert_eq!(got_ch, want_ch, "{name} at {ms} ms: what changed");
            n += 1;
        }
    }
    assert!(n >= 20, "only {n} frames compared");
}

/// What the kernel still refuses: nothing of phase 2's (layers, under,
/// ink lower now).
#[test]
fn what_the_kernel_lowers_now() {
    for src in [
        "layer { gray(y) }\nlayer add { gray(x) }",
        "rgb(ur, ug, ub)",
        "draw { line(0, 0, 127, 31, 1) }\ngray(ink)",
        "layer screen { pal(fire, x, 0.4) }",
    ] {
        let prog = compile(src).unwrap();
        assert!(lower(&prog).is_ok(), "{src}: {:?}", lower(&prog).err());
    }
}

/// A formula's kernel disassembled (`PSI_JIT_DUMP=file.psi cargo test
/// --lib dump_kernel -- --nocapture`), with its listing: for reading the
/// generated code.
#[test]
fn dump_kernel() {
    let Some(f) = std::env::var_os("PSI_JIT_DUMP") else {
        return;
    };
    let Some(bin) = super::enc_tests::binutils() else {
        return;
    };
    let src = std::fs::read_to_string(&f).unwrap();
    let prog = compile(&src).unwrap();
    println!("{}", prog.listing());
    let low = match lower(&prog) {
        Ok(l) => l,
        Err(w) => return println!("refused: {w}"),
    };
    println!("words: {:?}", low.words);
    let dir = std::env::temp_dir().join(format!("psi-jit-dump-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("k.bin"), &low.code).unwrap();
    let out = std::process::Command::new(bin.join("xtensa-esp32-elf-objdump"))
        .args(["-D", "-b", "binary", "-m", "xtensa"])
        .arg(dir.join("k.bin"))
        .output()
        .unwrap();
    println!("{}", String::from_utf8_lossy(&out.stdout));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The helpers the fuzz rarely reaches (tables, glyphs, sprites, picks of
/// lanes, a wide noise and a split's multiply in `loop forever`, `bit`,
/// checked arithmetic at its ends), each against the VM.
#[test]
fn helpers_draw_the_same_through_kernels() {
    let cases = [
        "table T = 0.1, 0.5, 0.9, 0.3\ngray(T[x * 4 + t] + y)",
        "rgb(digit(floor(px/5) % 12 - 1, px % 5, 31 - py), glyph(32 + px % 64, px % 5, py % 8, small), bit(px*258 - 16000, py - 1))",
        "gray(digit(px % 10, px % 4, py % 6, bold) * 0.5 + digit(floor(t*3) % 10, px % 5, py % 7))",
        "sprite s {\nkey . 0, X 1, Y 0.5\nXY.XY.XY.XY.XY.XY.\n.XYXY.XYXY.XYXY.XY\n}\ngray(s[x * 18 + t, y * 3])",
        "loop forever\ngray(noise(x * 8 + t, y * 4, t * 0.5))",
        "loop forever\nv = frac((t + x) * 0.37)\ngray(v)",
        "loop forever\nhsv(x + t * 0.1, 1, noise(px / 9 + t * 3, py / 5, 0))",
        "gray(pick(floor(x * 5 + t), 0.1, y, 0.6, x * x))",
        "gray((x * 30000 + y * 30000) * 3 - t * 20000)",
        "gray(-(x * 30000 * 3 - 32767 - 1) + abs(y * 30000 * 3 - 32000) / 30000)",
        "v = (px - 64) * (py - 16) * 9 * t\ngray(v * v)",
        "gray(sqrt(x + t) * (1 / (y + 0.1)) % 1 + x / (x + t) + atan2(y - 0.5, x - 0.5))",
        "gray(smoothstep(0.2, 0.8, x + sin(t)) * mix(x, y, t))",
        "palette p = #102030, #f08000, #ffffff\npal(p, x + t/8) * y",
        "palette p = #102030, #f08000, #ffffff\npal(p, floor(x * 3 + t) % 3 / 2)",
        "pal(chrono, (floor(px*0.1 + t) + floor(py*0.3)) % 2 ? frac(0.711 + t/4.8) : frac(0.336 + t/4.8)) * (floor(px*py*0.002 + t) % 8 / 8)",
        "pal(rainbow, pick(floor(px*0.07 + py*0.3) % 9, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, t/8, 0.8, 0.9)) * (py / 31)",
        "gray(tri(x * 3 + t) * sqr(y * 2 + t) * saw(x + y))",
        "gray(x > t or not (y < 0.5) and px != 7)",
        // Fable's review: a saturating mul's operand read again later (a
        // released operand register was a live value's).
        "v = (x + y) * 30000 + y + t\nu = v * v\ngray(min(u, 0.1) + frac(v))",
        "v = (x + y + t) * 300 + y\nu = v * v\ngray(frac(v) + min(u, 0.1))",
        "v = x * 3 + t\nu = sin(v) + cos(v * 2)\ngray(u + frac(v))",
        // ...and a table past 8 entries, a lane pick of 9 (two immediates).
        "table T = 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9\ngray(T[x*9 + y*3 + t] + y)",
        "gray(pick(floor(x * 9 + t) % 9, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, y, 0.8, x) + y)",
        // Fable's phase-2 review: a palette hoisted in a10 read after a
        // memo miss wrote a10 (an unscaled and a scaled layer of one palette).
        "u = x * 0.5\nv = u + y\nq = v * x + u\nlayer { pal(fire, q) }\nlayer max { gray(y, 0.3) }\nlayer multiply { pal(fire, x) * 0.7 }",
        "layer { pal(ice, x + t) }\nlayer screen { pal(ice, y) * (x * 0.9) }\nlayer add { pal(ice, x * y) * y }",
    ];
    let mut lowered = 0;
    for src in cases {
        let prog = compile(src).unwrap_or_else(|e| panic!("{src}: {}", e.msg));
        match same_as_vm("helpers", src, &prog, &[0, 1234, 4321, 99_999]) {
            Ok(_) => lowered += 1,
            Err(w) => println!("{src}: VM ({w})"),
        }
    }
    assert_eq!(lowered, cases.len());
}
