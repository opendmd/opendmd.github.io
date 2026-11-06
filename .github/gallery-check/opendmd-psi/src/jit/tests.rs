//! Kernels against the VM, on the host, through the emulator (`emu`): the
//! goldens (their hashes), the bundled formulas at several times with and
//! without the still cache, and random formulas. A kernel that draws one
//! byte differently from the VM fails here.

use super::emu::EmuTarget;
use super::lower::{lower, Lowered};
use super::{check_rows, frame, CHECK_ROWS};
use crate::program::{Prog, Program, H, W};
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
    prog: &Prog,
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
    prog: &Prog,
    low: &Lowered,
    inputs: &Inputs,
    still: Option<&mut Still>,
    with: With,
    before: Option<&[u8]>,
) -> (Vec<u8>, u64, [u32; H]) {
    kernel_frame_beam(prog, low, inputs, still, with, before, None)
}

/// [`kernel_frame_changed`], racing the beam when `sink` is given.
fn kernel_frame_beam(
    prog: &Prog,
    low: &Lowered,
    inputs: &Inputs,
    still: Option<&mut Still>,
    with: With,
    before: Option<&[u8]>,
    sink: Option<crate::PairSink<'_>>,
) -> (Vec<u8>, u64, [u32; H]) {
    let mut t = EmuTarget::new(&low.code);
    let mut ctx = vec![0u32; low.words.len()];
    let mut s = Scratch::<128>::new();
    // (Word-aligned, as the player's canvas.)
    let mut words = vec![0u32; W * H * 3 / 4];
    let out: &mut [u8] = bytemuck_bytes(&mut words);
    if let Some(b) = before {
        out.copy_from_slice(b);
    }
    // (As `Kernel::render`: a `rate` formula without its held picture has
    // its slow rows drawn first.)
    if low.rate && with.under.is_none() {
        crate::draw_slow::<128>(prog, &mut Scratch::new(), inputs, &OWN, u32::MAX, out);
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
        sink,
    );
    (out.to_vec(), t.emu.steps, changed)
}

fn bytemuck_bytes(w: &mut [u32]) -> &mut [u8] {
    // SAFETY: u32s as their bytes, the same memory.
    unsafe { core::slice::from_raw_parts_mut(w.as_mut_ptr().cast::<u8>(), w.len() * 4) }
}

fn vm_frame(prog: &Prog, inputs: &Inputs, still: Option<&mut Still>, with: With) -> Vec<u8> {
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
fn same_as_vm(name: &str, src: &str, prog: &Prog, times: &[u32]) -> Result<f64, &'static str> {
    let u = picture();
    // Over the picture (what a look draws over), with a canvas if it draws.
    same_as_vm_with(
        name,
        src,
        prog,
        times,
        With {
            under: Some(&u),
            ink: prog.draws(),
        },
    )?;
    // ...and alone (nothing under: black; ink 0 without a canvas).
    same_as_vm_with(name, src, prog, times, PLAIN)
}

/// [`same_as_vm`] drawn with `with`.
fn same_as_vm_with(
    name: &str,
    src: &str,
    prog: &Prog,
    times: &[u32],
    with: With,
) -> Result<f64, &'static str> {
    let low = lower(prog)?;
    let mut steps = 0;
    let mut frames = 0;
    // A `rate` formula: the kernel draws its fast part over the held slow
    // layers (the VM's), as the player has it.
    let mut held = crate::Held::try_boxed().unwrap();
    let mut pic: Vec<u8>;
    for &ms in times {
        let mut inputs = golden::inputs(ms);
        inputs.set_ms(prog, u64::from(ms));
        if !crate::range::inputs_in_range(prog, &inputs) {
            continue;
        }
        let want = vm_frame(prog, &inputs, None, with);
        let with = if prog.rate().is_some() {
            held.update::<128>(prog, &mut Scratch::new(), &inputs, &OWN);
            pic = held.picture().to_vec();
            With {
                under: Some(&pic),
                ink: with.ink,
            }
        } else {
            with
        };
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
        let with = With {
            under: over.then_some(&u[..]),
            ink: true,
        };
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
        src.push_str(&format!(
            "v = {}\n",
            expr(&mut r, 2).replace("hour", &leaf(&mut r))
        ));
        src.push_str(&format!("u = {} * {}\n", leaf(&mut r), expr(&mut r, 2)));
        let n = 1 + r.below(3);
        for j in 0..n {
            let cov = ["", ", 0.4", ", v", ", x", ", a", ", 1", ", u*2 - 0.5"][r.below(7)];
            let colour = match r.below(6) {
                0 => format!("gray({}{cov})", leaf(&mut r)),
                1 => format!("rgb({}, v, {}{cov})", leaf(&mut r), leaf(&mut r)),
                2 => format!("hsv(v + {}, 1, u{cov})", leaf(&mut r)),
                3 => format!("pal({}, {}{cov})", pals[r.below(5)], leaf(&mut r)),
                4 => format!(
                    "pal({}, {}{cov}) * {}",
                    pals[r.below(5)],
                    leaf(&mut r),
                    ["0.5", "y", "u", "1.5", "v"][r.below(5)]
                ),
                _ => format!("rgb(ur*1.2 - 0.05, ug, ub*0.7 + ul*0.2{cov})"),
            };
            let b = if j == 0 && k % 2 == 0 {
                ""
            } else {
                blends[r.below(blends.len())]
            };
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
                    if low.rows_at.is_some() {
                        ", the row stage too"
                    } else {
                        ""
                    }
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
            4 => format!(
                "{}({})",
                ["sin", "cos", "tri", "saw", "sqr", "abs", "floor", "frac", "sqrt", "-"]
                    [r.below(10)],
                e(r)
            ),
            5 => format!(
                "{}({}, {})",
                ["min", "max", "step", "hash", "len", "atan2", "bit"][r.below(7)],
                e(r),
                e(r)
            ),
            6 => format!(
                "{}({}, {}, {})",
                ["clamp", "mix", "smoothstep", "noise"][r.below(4)],
                e(r),
                e(r),
                e(r)
            ),
            7 => format!("T[{}]", e(r)),
            _ => format!("digit(floor({}) % 10, {}, {})", e(r), e(r), e(r)),
        }
    }
    for k in 0..300 {
        let forever = k % 4 == 0;
        let lf: Vec<&str> = leaf
            .iter()
            .copied()
            .filter(|&l| forever || l != "seed")
            .collect();
        let src = format!(
            "{}table T = 0.1, -3, 0.9, 7, 0.3\nq = {}\nr = {}\npal(fire, frac(x * q + r))",
            if forever {
                "loop forever\n"
            } else {
                "loop 3\n"
            },
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
    println!(
        "random row stages through kernels: {ok} ({rows} with a row kernel); on the VM: {why:?}"
    );
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
        // (A `rate` formula's kernel draws over its held picture: its own
        // test, `rate_formulas_draw_the_same_through_kernels`.)
        if prog.rate().is_some() {
            continue;
        }
        let Ok(low) = lower(&prog) else { continue };
        let inputs = golden::inputs(500);
        if !crate::range::inputs_in_range(&prog, &inputs) {
            continue;
        }
        let mut s = Scratch::<128>::new();
        let mut want = vec![0u8; W * H * 3];
        let mut ink = Ink::boxed();
        render_rows_still::<128>(
            &prog,
            &mut s,
            Some(&mut ink),
            None,
            &inputs,
            &OWN,
            Some(&u),
            u32::MAX,
            &mut want,
        );
        let mut t = EmuTarget::new(&low.code);
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
        // (A `rate` formula's kernel draws over its held picture: its own
        // test, `rate_formulas_draw_the_same_through_kernels`.)
        if prog.rate().is_some() {
            continue;
        }
        let Ok(low) = lower(&prog) else { continue };
        let lp = prog.loop_ms().max(1000);
        for ms in [0, lp / 2] {
            let before_in = golden::inputs(ms);
            let mut inputs = golden::inputs(ms + 40);
            inputs.set_ms(&prog, u64::from(ms + 40));
            if !crate::range::inputs_in_range(&prog, &inputs) {
                continue;
            }
            let with = With {
                under: Some(&u),
                ink: prog.draws(),
            };
            let before = vm_frame(&prog, &before_in, None, with);
            let mut s = Scratch::<128>::new();
            let mut ink = Ink::boxed();
            let mut words = vec![0u32; W * H * 3 / 4];
            let want = bytemuck_bytes(&mut words);
            want.copy_from_slice(&before);
            let mut want_ch = [0u32; H];
            render_rows_changed::<128>(
                &prog,
                &mut s,
                Some(&mut ink),
                None,
                &inputs,
                &OWN,
                Some(&u),
                u32::MAX,
                want,
                &mut want_ch,
            );
            let (got, _, got_ch) =
                kernel_frame_changed(&prog, &low, &inputs, None, with, Some(&before));
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

/// `under(x, y)` (`UAT`) through kernels, over the picture and without
/// one: in the panel and past each edge, rounded both ways, a scalar
/// coordinate (a row value), values near the top of the range (the
/// saturating `add` of ½), mixed and scaled, beside a layer.
#[test]
fn under_at_draws_the_same_through_kernels() {
    let cases = [
        "look\nunder(px + a, py)",
        "look\nunder(floor(px/4)*4 + 2, floor(py/4)*4 + 2)",
        "look\nunder(px + floor(sin(t*2)*2), py)",
        "look\nunder(127 - px, py - 0.5)",
        "look\nunder(px*3 - 200, py*2 - 20)",
        "look\nunder(px - 0.5, py + 0.4999)",
        "look\nunder(px*1000 + t*30000, py*-1000)",
        "look\nunder(x*32767 + 32760, y*32767 - 32767)",
        "look\nmix(under, under(px, py - 2), 0.5)",
        "look\nunder(px + a, py)*0.25 + under(px - a, py)*0.25 + under(px, py + a)*0.25 + under(px, py - a)*0.25",
        "look\nlayer { gray(x) }\nlayer screen { under(px + 3, py + 1) * 0.6 }",
        "look\nfn sh(d) = under(px + d, py)\nsh(2)*0.5 + sh(-2)*0.5",
    ];
    for src in cases {
        let prog = compile(src).unwrap_or_else(|e| panic!("{src}: {}", e.msg));
        assert!(prog.uat_steps() > 0, "{src}: no UAT step");
        if let Err(w) = same_as_vm("under", src, &prog, &[0, 777, 1234, 4321, 7999]) {
            panic!("{src}: the VM draws it ({w})");
        }
    }
}

/// `cycle`: the turned palettes through kernels, turned once a frame and
/// once a row (the row's colour words and memos filled again each row),
/// scaled, fused and as layers.
#[test]
fn cycled_palettes_draw_the_same_through_kernels() {
    let cases = [
        "cycle(fire, sin(x*2) + sin(y*1.5) + sin(x*1.5 + y), t/8)",
        "palette sky = #000, #024, #08c, #fc8, #fff\ncycle(sky, x*0.3 + y*0.5, t/4 + py/64)",
        "cycle(rainbow, len(cx, cy), -t/3 + py/40) * (0.5 + x/2)",
        "cycle(chrono, floor(px/8)/16, t + py/32) * (floor(px/16)/8)",
        "cycle(ice, x > 0.5 ? y : 0.25, t/2 + py/20)",
        "cycle(amber, pick(floor(px/32), 0.1, 0.4, 0.7, t), t/3)",
        "layer { gray(x) }\nlayer screen { cycle(ice, y*7, t + py/9, x) }",
        "palette p = #f00, #0f0, #00f\nlayer { cycle(p, x - y*2.5, t*0.37) }\nlayer multiply { cycle(fire, y, py/32 - t) * 0.75 }",
    ];
    for src in cases {
        let prog = compile(src).unwrap();
        let times = [0, 130, 1000, 2470, 5555, 7990];
        if let Err(why) = same_as_vm("cycle", src, &prog, &times) {
            panic!("{src}: on the VM ({why})");
        }
    }
}

/// Tweens of palettes (worked out once a frame, before the kernel's words):
/// the bytes of the VM's frame, as the palette changes from frame to frame.
#[test]
fn tweened_palettes_draw_the_same_through_kernels() {
    let cases = [
        "loop 8\npalette d = #048, #8cf, #fff\npalette n = #002, #124, #468\npalette s = tween(d, n, 0.5 + 0.5*sin(t*0.8))\npal(s, y) * (0.5 + x/2)",
        "loop 8\npalette a = #f00, #0f0\npalette b = #00f, #ff0, #0ff\npalette s = tween(tween(a, b, t/8), fire, frac(t/3))\ncycle(s, x + y, t/4 + py/40)",
        "loop 8\npalette a = #f80, #fc0, #08f, #fff\npalette b = #400, #320, #002, #224\npalette c = #f0f, #0ff, #888, #ccc\nw = frac(t/8)\npalette s = tween(a, b, w), tween(c, b, 1 - w)\npal(s, floor(px/8)/16) * (py > 15 ? 1 : 0.5)",
    ];
    for src in cases {
        let prog = compile(src).unwrap();
        let times = [0, 130, 1000, 2470, 5555, 7990];
        if let Err(why) = same_as_vm("tween", src, &prog, &times) {
            panic!("{src}: on the VM ({why})");
        }
    }
}

/// Scenes (`scene NAME when c { … }`): the kernel's branch a pixel and its
/// zeros, and the row function's, the bytes of the VM's frame, the scene on
/// and off within a frame and from frame to frame.
#[test]
fn scenes_draw_the_same_through_kernels() {
    let cases = [
        "loop 8\non = frac(t/8) > 0.5\nscene a when on * (py < 20) {\n  h = hash(floor(px/6), 3)\n  w = (frac(px/6) > 0.3) * (frac(py/3) > 0.4) * (h > 0.3)\n  lit = w * (hash(floor(px/2), floor(py/3)) > 0.5)\n}\nscene b when 1 - on {\n  wave = sin(x*12 + t*2 + y*6) * 0.5 + 0.5\n}\nrgb(lit, wave*0.3 + lit*0.5, wave + h*0.2*(py < 20))",
        "loop 4\nscene s when py > 10 + 8*sin(t*1.57) {\n  r = y*2\n  g = sin(x*8 + r)\n  n = noise(x*6, y*3, t)\n}\npal(fire, g*0.5 + n*0.5 + r*0.1)",
        "loop 4\nk = x*3\nscene s when t > 1 {\n  d = len(x - 0.5, y - 0.5)\n  e = k + d\n}\nrgb(frac(e), d, k/3)",
        "look\nloop 4\nscene s when t > 2 {\n  m = atan2(x - 0.5, y - 0.5) + len(x, y)\n}\nunder * 0.5 + rgb(m, m, m) * 0.5",
    ];
    for src in cases {
        let prog = compile(src).unwrap();
        let low = lower(&prog).unwrap_or_else(|w| panic!("{src}: the JIT refuses: {w}"));
        assert!(low.rows_at.is_some(), "{src}: the row function");
        // Each pixel guard a site the driver patches (no test a pixel).
        let n = prog.ops[prog.row_end as usize..prog.len()]
            .iter()
            .filter(|o| o.code == crate::program::code::GUARD)
            .count();
        assert_eq!(low.guards.len(), n, "{src}: a site for each guard");
        for g in &low.guards {
            assert_eq!(g.on.len(), g.off.len());
            assert_eq!(&low.code[g.at..g.at + g.on.len()], &g.on[..]);
        }
        let times = [0, 130, 1000, 1500, 2470, 3300, 5555, 7990];
        if let Err(why) = same_as_vm("scene", src, &prog, &times) {
            panic!("{src}: on the VM ({why})");
        }
    }
}

/// `layer rate N`: the kernel draws the fast part over the held slow
/// layers, the bytes of the VM's frame.
#[test]
fn rate_formulas_draw_the_same_through_kernels() {
    let cases = [
        "loop 60\nh1 = floor(hour/10); m2 = minute % 10\nlayer rate 50 { gray(digit(pick(floor(px/32), h1, m2, 3, 4), floor(px % 32 / 8), floor(py/6), bold)) }\nlayer min { hsv(x + py/64 + t/10, py < 1 ? 0 : 1, 1) }",
        "loop 2\nlayer rate 4 { hsv(x + t/3, 1, frac(y + t)) }\nlayer add { gray(x*a*0.2) }",
        "fps 100\nloop 2\nlayer rate 40 { cycle(fire, x, t/2) }\nlayer max { cycle(ice, len(cx, cy), t + py/32, 0.5) }",
        "layer rate 3 { gray(x) }\nlayer multiply rate 3 { pal(ice, y) }\nlayer screen { hsv(t, 1, x) }",
    ];
    for src in cases {
        let prog = compile(src).unwrap();
        assert!(prog.rate().is_some());
        let times = [0, 20, 40, 1000, 1500, 1980];
        if let Err(why) = same_as_vm("rate", src, &prog, &times) {
            panic!("{src}: on the VM ({why})");
        }
    }
}

/// Racing the beam through a kernel: the same bytes and changed groups as
/// the VM's frame at once, each pair told once, in order, as soon as its
/// rows hold the new frame and while the rows to come hold the old.
#[test]
fn kernels_race_the_beam_as_the_vm_draws_at_once() {
    let u = picture();
    let mut n = 0;
    for (name, src) in bundled() {
        let Ok(prog) = compile(&src) else { continue };
        if prog.rate().is_some() {
            continue;
        }
        let Ok(low) = lower(&prog) else { continue };
        let lp = prog.loop_ms().max(1000);
        for ms in [0, lp / 2] {
            let before_in = golden::inputs(ms);
            let mut inputs = golden::inputs(ms + 40);
            inputs.set_ms(&prog, u64::from(ms + 40));
            if !crate::range::inputs_in_range(&prog, &inputs) {
                continue;
            }
            for with in [
                With {
                    under: Some(&u),
                    ink: prog.draws(),
                },
                With {
                    under: None,
                    ink: prog.draws(),
                },
            ] {
                let before = vm_frame(&prog, &before_in, None, with);
                let mut s = Scratch::<128>::new();
                let mut ink = Ink::boxed();
                let mut words = vec![0u32; W * H * 3 / 4];
                let want = bytemuck_bytes(&mut words);
                want.copy_from_slice(&before);
                let mut want_ch = [0u32; H];
                render_rows_changed::<128>(
                    &prog,
                    &mut s,
                    Some(&mut ink),
                    None,
                    &inputs,
                    &OWN,
                    with.under,
                    u32::MAX,
                    want,
                    &mut want_ch,
                );
                let mut told = Vec::new();
                let mut sink = |p: usize, out: &[u8], a: u32, b: u32| {
                    let row = |r: usize, pic: &[u8]| pic[r * W * 3..(r + 1) * W * 3].to_vec();
                    for q in 0..H / 2 {
                        let pic = if p != crate::BEAM_START && q <= p {
                            &want[..]
                        } else {
                            &before[..]
                        };
                        assert!(
                            row(q, out) == row(q, pic)
                                && row(q + H / 2, out) == row(q + H / 2, pic),
                            "{name} at {ms} ms: pair {q} when pair {p} was told"
                        );
                    }
                    told.push((p, a, b));
                };
                let (got, _, got_ch) = kernel_frame_beam(
                    &prog,
                    &low,
                    &inputs,
                    None,
                    with,
                    Some(&before),
                    Some(&mut sink),
                );
                assert_eq!(&got[..], &want[..], "{name} at {ms} ms");
                assert_eq!(got_ch, want_ch, "{name} at {ms} ms: what changed");
                let want_told: Vec<_> = [(crate::BEAM_START, 0, 0)]
                    .into_iter()
                    .chain((0..H / 2).map(|p| (p, want_ch[p], want_ch[p + H / 2])))
                    .collect();
                assert_eq!(told, want_told, "{name} at {ms} ms: the pairs told");
                n += 1;
            }
        }
    }
    assert!(n >= 40, "only {n} frames raced");
}

/// A `rate` formula raced through its kernel: the fast part in pair order
/// over the held slow layers, the bytes and changed groups of the VM's
/// fast part over them at once, each pair told once, in order.
#[test]
fn rate_kernels_race_their_fast_part_over_the_held_picture() {
    let mut srcs: Vec<(String, String)> = bundled()
        .into_iter()
        .filter(|(_, src)| compile(src).is_ok_and(|p| p.rate().is_some()))
        .collect();
    srcs.push((
        "sketch".into(),
        "loop 2\nlayer rate 4 { hsv(x + t/3, 1, frac(y + t)) }\nlayer add { gray(x*t) }".into(),
    ));
    let mut n = 0;
    for (name, src) in srcs {
        let prog = compile(&src).unwrap();
        let Ok(low) = lower(&prog) else { continue };
        assert!(low.rate, "{name}");
        let mut held = crate::Held::try_boxed().unwrap();
        let mut hs = Scratch::<128>::new();
        let mut before = vec![0u8; W * H * 3];
        for step in 0..6u32 {
            let ms = step * 20 + 40;
            let mut inputs = golden::inputs(ms);
            inputs.set_ms(&prog, u64::from(ms));
            if !crate::range::inputs_in_range(&prog, &inputs) {
                continue;
            }
            held.update::<128>(&prog, &mut hs, &inputs, &OWN);
            let with = With {
                under: Some(held.picture()),
                ink: prog.draws(),
            };
            let mut words = vec![0u32; W * H * 3 / 4];
            let want = bytemuck_bytes(&mut words);
            want.copy_from_slice(&before);
            let mut want_ch = [0u32; H];
            let mut ink = Ink::boxed();
            crate::render_fast::<128>(
                &prog,
                &mut Scratch::new(),
                Some(&mut ink),
                None,
                &inputs,
                &OWN,
                with.under,
                u32::MAX,
                want,
                Some(&mut want_ch),
            );
            let mut told = Vec::new();
            let mut sink = |p: usize, out: &[u8], a: u32, b: u32| {
                let row = |r: usize, pic: &[u8]| pic[r * W * 3..(r + 1) * W * 3].to_vec();
                for q in 0..H / 2 {
                    let pic = if p != crate::BEAM_START && q <= p {
                        &want[..]
                    } else {
                        &before[..]
                    };
                    assert!(
                        row(q, out) == row(q, pic) && row(q + H / 2, out) == row(q + H / 2, pic),
                        "{name} at {ms} ms: pair {q} when pair {p} was told"
                    );
                }
                told.push((p, a, b));
            };
            let (got, _, got_ch) = kernel_frame_beam(
                &prog,
                &low,
                &inputs,
                None,
                with,
                Some(&before),
                Some(&mut sink),
            );
            assert_eq!(&got[..], &want[..], "{name} at {ms} ms");
            assert_eq!(got_ch, want_ch, "{name} at {ms} ms: what changed");
            let want_told: Vec<_> = [(crate::BEAM_START, 0, 0)]
                .into_iter()
                .chain((0..H / 2).map(|p| (p, want_ch[p], want_ch[p + H / 2])))
                .collect();
            assert_eq!(told, want_told, "{name} at {ms} ms: the pairs told");
            before = got;
            n += 1;
        }
    }
    assert!(n >= 6, "only {n} frames raced");
}

/// Damaged programs the player's check takes ([`crate::psb::verify`]) are
/// lowered and run in the emulator without a stray access (it panics on
/// one). A kernel that draws differently from the VM is counted, not
/// failed: the player checks rows of every kernel against the VM and
/// keeps the VM then.
#[test]
fn damaged_programs_lower_safely() {
    use crate::psb::fuzz::{mutate, sources, Rng};
    use crate::psb::{load_into, Which};
    let mut r = Rng(0x2545_f491_4f6c_dd1d);
    let u = picture();
    let (mut lowered, mut differ) = (0, 0);
    for (_, src) in sources() {
        let b = crate::build(&src).unwrap();
        for _ in 0..150 {
            let m = mutate(&mut r, &b);
            let mut p = Program::default();
            if load_into(&m, Which::Play, &mut p).is_err() {
                continue;
            }
            let low = match std::panic::catch_unwind(|| lower(&p)) {
                Ok(Ok(l)) => l,
                Ok(Err(_)) => continue,
                Err(_) => panic!("lowering panicked on a verified program:\n{}", p.listing()),
            };
            lowered += 1;
            let mut inputs = golden::inputs((r.next() % 8000) as u32);
            inputs.set_ms(&p, 1000);
            let with = With {
                under: Some(&u),
                ink: p.draws(),
            };
            let (got, _) = kernel_frame(&p, &low, &inputs, None, with);
            if crate::range::inputs_in_range(&p, &inputs)
                && got != vm_frame(&p, &inputs, None, with)
            {
                differ += 1;
            }
        }
    }
    println!("{lowered} damaged programs lowered, {differ} drew otherwise than the VM");
}

/// P0b-2: the editor's "does the panel's JIT take it" (`Program::jit_takes`,
/// the lowering's rules without emitting) is the lowering's answer — over
/// the bundled Formulas, the goldens, the sketches, the device cases and
/// random formulas, wide ones (many values alive at once) among them.
#[test]
fn jit_takes_is_lower() {
    let mut srcs: Vec<String> = bundled().into_iter().map(|(_, s)| s).collect();
    for &(_, s, _, _) in crate::golden::FRAMES {
        srcs.push(s.into());
    }
    for &(_, s, _, _, _) in crate::golden::DRAWN.iter().chain(crate::golden::OVER) {
        srcs.push(s.into());
    }
    for (_, s) in crate::sketches::ALL {
        srcs.push(s.into());
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/research/psi-rebuild-device");
    for e in std::fs::read_dir(dev).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "psi") {
            srcs.push(std::fs::read_to_string(e.path()).unwrap());
        }
    }
    let mut r = Rng(0x1d17_0b5e_55ed_0b2b);
    for k in 0..600usize {
        // 1 to 12 values alive at once into the colour.
        let n = 4 + k % 9;
        let mut src = String::from("loop 4\n");
        for j in 0..n {
            let f = ["sin", "frac", "abs", "noise", "hash", "floor"][(j + k) % 6];
            let a = match f {
                "noise" => format!("noise(x*{j}.3, y*{k}, t)"),
                "hash" => format!("hash(px*{}, py)", j + 1),
                _ => format!("{f}(x*{j}.7 + y*{}.1 + {})", k % 7, expr(&mut r, 1)),
            };
            src += &format!("v{j} = {a}\n");
        }
        let sum: Vec<String> = (0..n).map(|j| format!("v{j}*{}.{j}", j % 3)).collect();
        let v = sum.join(" + ");
        src += &match k % 3 {
            0 => format!("gray({v})"),
            1 => format!("hsv(x, 1, {v})"),
            _ => format!("pal(fire, {v})"),
        };
        srcs.push(src);
    }
    let (mut yes, mut no, mut bad) = (0, 0, Vec::new());
    for src in &srcs {
        let Ok(prog) = compile(src) else { continue };
        let real = crate::jit::refusal(&prog);
        if prog.jit_takes() != real.is_none() {
            bad.push(format!("{real:?}: {src}"));
        }
        if real.is_none() {
            yes += 1;
        } else {
            no += 1;
        }
    }
    println!("jit_takes: {yes} taken, {no} refused, {} disagree", bad.len());
    assert!(bad.is_empty(), "{}", bad.join("\n---\n"));
    assert!(yes > 200 && no > 20, "{yes} {no}");
}

/// The fused compares (P0b-4) through the kernel: every case of
/// `crate::tests::fused_compare_cases`, the bytes of the VM's frame (the
/// pixel templates and the row function's).
#[test]
fn fused_compares_draw_the_same_through_kernels() {
    let mut lowered = 0;
    for (src, _) in crate::tests::fused_compare_cases() {
        let prog = compile(&src).unwrap();
        let times = [0, 1999, 2000, 4000, 7321];
        if same_as_vm("fused", &src, &prog, &times).is_ok() {
            lowered += 1;
        }
    }
    assert!(lowered >= 250, "only {lowered} lowered");
}

/// Endless road V2 (its scenes, the fused compares, the guards) through
/// the kernel at points of its cycle: each place and transition, by day
/// and by night, three seeds.
#[test]
fn road_v2_draws_the_same_through_kernels() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(root.join("../opendmd-web/experiments/road-v2/road-v2.psi"))
        .unwrap();
    let prog = compile(&src).unwrap();
    let low = lower(&prog).unwrap_or_else(|w| panic!("the JIT refuses road-v2: {w}"));
    let u = picture();
    for seed in [0.0, 1.0 / 3.0, 2.0 / 3.0] {
        for k in 0..24 {
            // Through the cycle (324 s), and a day later (the sun moved).
            for day in [0u64, 1] {
                let p = (k as f64 + 0.5) / 24.0;
                let t = (p - seed - 0.125 + 2.0 + day as f64 * 3.0) * 324.0;
                let mut inputs = golden::inputs(0);
                inputs.seed = (seed * 65536.0) as i32;
                inputs.set_ms(&prog, (t * 1000.0) as u64);
                let want = vm_frame(&prog, &inputs, None, PLAIN);
                let (got, _) = kernel_frame(&prog, &low, &inputs, None, PLAIN);
                if let Some(d) = first_diff(&got, &want) {
                    panic!("road-v2 seed {seed} at {t} s: the kernel's pixel {d:?}");
                }
                let with = With {
                    under: Some(&u),
                    ink: prog.draws(),
                };
                let want = vm_frame(&prog, &inputs, None, with);
                let (got, _) = kernel_frame(&prog, &low, &inputs, None, with);
                assert!(
                    first_diff(&got, &want).is_none(),
                    "road-v2 over a picture, {t} s"
                );
            }
        }
    }
}

/// The built-in inputs (frame slots the driver fills) and the store past
/// 256 numbers, in a whole program's store and in the player's smaller one
/// (the helpers mask by its length), through kernels.
#[test]
fn built_ins_and_the_big_store_draw_the_same_through_kernels() {
    let filler: Vec<String> = (0..250).map(|k| format!("{}", k % 9)).collect();
    let big = format!(
        "table F0 = {f}, 0.5\ntable F1 = {f}, 0.25\ntable F2 = {f}, 0.75\n",
        f = filler.join(", ")
    );
    let cases = [
        String::from("hsv(replay.game + x*0.3, 1 - night*0.5, daylight * y + since(replay.game)*0.01)"),
        String::from("v = sun.alt*4\ngray(clamp(v + y - 0.5, 0, 1) * bright + was(night)*x)"),
        format!("{big}table T = 0.1, 0.5, 0.9, 0.3\ngray(T[x * 4 + t] * daylight + F2[px % 251] * 0.05)"),
        format!("{big}sprite s {{\nkey . 0, X 1, Y 0.5\nXY.XY.XY.XY.XY.XY.\n.XYXY.XYXY.XYXY.XY\n}}\ngray(s[x * 18 + t, y * 3] * (0.5 + moon.phase) + F1[py])"),
    ];
    let mut worlds = vec![crate::World::default()];
    let mut w = crate::World::default();
    for (k, key) in crate::world::KEYS.iter().enumerate() {
        w.change(key.id, key.lo + (key.hi - key.lo) / (k as i32 + 2));
        w.since[key.id as usize] = (k as i32) << 14;
    }
    worlds.push(w);
    for src in &cases {
        let full = compile(src).unwrap_or_else(|e| panic!("{src}: {}", e.msg));
        let b = crate::build(src).unwrap();
        let small = crate::psb::load_boxed(&b, crate::psb::Which::Play).unwrap();
        for prog in [&*full as &Prog, &*small] {
            let low = lower(prog).unwrap_or_else(|e| panic!("{src}: {e}"));
            for w in &worlds {
                for ms in [0, 1234, 5000] {
                    let mut inputs = golden::inputs(ms);
                    inputs.set_ms(prog, u64::from(ms));
                    inputs.world = *w;
                    let want = vm_frame(prog, &inputs, None, PLAIN);
                    let (got, _) = kernel_frame(prog, &low, &inputs, None, PLAIN);
                    assert!(first_diff(&got, &want).is_none(), "{src} at {ms} ms (store {})", prog.store_len());
                }
            }
        }
    }
}

/// The kernel's inline table and sprite reads (no helper call): sprites
/// and tables at the store's start and past 750 numbers, read at a row's
/// value, a constant and per pixel, out of range both ways (below 0, past
/// the end), with a multiply after them in the same pixel loop (the sprite
/// read moves SAR, which the multiplies hoist).
#[test]
fn inline_table_and_sprite_reads_draw_as_the_vm() {
    let filler: Vec<String> = (0..250).map(|k| format!("{}", k % 9)).collect();
    for pre in [0usize, 1, 3] {
        let mut big = String::new();
        for j in 0..pre {
            big += &format!("table F{j} = {}, {j}.5\n", filler.join(", "));
        }
        for v in ["y * 3", "py % 2", "0", "y * 2.5 - 0.5"] {
            let src = format!(
                "{big}table T = 0.1, 0.5, 0.9, 0.3\nsprite s {{\nkey . 0, X 1, Y 0.5\nXY.XY.XY.XY.XY.XY.\n.XYXY.XYXY.XYXY.XY\n}}\n\
                 gray(s[x * 20 - 1, {v}] * 0.5 + T[x * 6 - 1] * 0.3 + (x * 0.7) * (y * 0.3))"
            );
            let p = compile(&src).unwrap();
            let low = lower(&p).unwrap_or_else(|e| panic!("{src}: {e}"));
            for ms in [0, 2500] {
                let mut inputs = golden::inputs(ms);
                inputs.set_ms(&p, u64::from(ms));
                let want = vm_frame(&p, &inputs, None, PLAIN);
                let (got, _) = kernel_frame(&p, &low, &inputs, None, PLAIN);
                assert!(first_diff(&got, &want).is_none(), "{src} at {ms} ms: {:?}", first_diff(&got, &want));
            }
        }
    }
}

/// `div_s` (a lane divided by a scalar, language review A1) through the
/// kernel's every form against the VM's `fixed::DivBy`: a shift, a magic
/// number, a constant 0, a constant with neither, and a frame's or a row's
/// divisor known past ±1 or not at all (0, tiny, negative), with the
/// sliders swept.
#[test]
fn divisions_by_a_scalar_draw_as_the_vm() {
    let srcs = [
        "gray(frac(px/3) + frac(px/7) + frac((px - 64)/10) + frac((px - 64)/-3))",
        "gray(frac((px - 64)/8) + frac((px - 64)/-4) + frac(px/16384) + frac((px - 64)/-16384))",
        "gray(frac((px - 64)/2.5) + frac((px - 64)/0.3) + frac((px - 64)/0.0003) + frac((px - 64)/-0.7))",
        "gray(frac((px - 64)/1.7) + frac((px - 64)/127.3) + frac((px - 64)/255.9) + frac(x/1.0001))",
        "gray((px - 64)/0 + 0.5)",
        "gray(frac(px/8 + px/-4 + (px + py)/7 + px/-10 + x/0.5 + py*px/2.5))",
        "s = a*4 + 1.5\ngray(frac((px - 64)/s))",
        "s = a - 0.5\ngray(frac((px - 64)*0.01/s) + (px - 64)/s/10000)",
        "s = -a*3 - 1.25\ngray(frac((px - 64)/s))",
        "s = floor(y*4) - 2\ngray(frac((px - 64)/s) + (px - 64)/s/10000)",
        "s = (y - 0.5)*0.001\ngray(frac(x/s) + x/s/10000)",
        "s = (y - 0.5)*0.00002\ngray(frac(x/s) + x/s/10000)",
        "s = t - 2\ngray(frac((px - 64)/s) + (px - 64)/s/10000)",
    ];
    for src in srcs {
        let p = compile(src).unwrap();
        assert!(p.listing().contains("div_s"), "{src}\n{}", p.listing());
        let low = lower(&p).unwrap_or_else(|e| panic!("{src}: {e}"));
        for (ms, a) in [(0, 0), (700, 1 << 14), (2000, 1 << 15), (2600, 3 << 14), (3999, 1 << 16)] {
            let mut inputs = golden::inputs(ms);
            inputs.set_ms(&p, u64::from(ms));
            inputs.a = a;
            if !crate::range::inputs_in_range(&p, &inputs) {
                continue;
            }
            let want = vm_frame(&p, &inputs, None, PLAIN);
            let (got, _) = kernel_frame(&p, &low, &inputs, None, PLAIN);
            assert!(first_diff(&got, &want).is_none(), "{src} at {ms} ms, a {a}: {:?}\n{}", first_diff(&got, &want), p.listing());
        }
    }
}

/// A heap that runs out mid-lowering (the code buffer refused past each
/// of many lengths): the lowering says `NO_MEMORY`, never panics or spins
/// (a guard's padding, a branch bound past the end), and with room again
/// gives the same code as before.
#[test]
fn a_short_heap_mid_lowering_refuses_and_recovers() {
    use super::enc::FAIL_PAST;
    let mut tried = 0;
    for &(name, src, _, _) in golden::FRAMES {
        let prog = compile(src).unwrap();
        let Ok(whole) = lower(&prog) else { continue };
        let len = whole.code.len();
        let mut cut = 0;
        while cut < len {
            FAIL_PAST.with(|f| f.set(cut));
            let r = lower(&prog);
            FAIL_PAST.with(|f| f.set(usize::MAX));
            match r {
                Err(why) => assert_eq!(why, super::NO_MEMORY, "{name} cut at {cut} of {len} B"),
                // (Each buffer is cut on its own: the pixel and row
                // stages may each fit when the two joined would not.)
                Ok(l) => assert!(l.code == whole.code, "{name} cut at {cut}: lowered, but not the same code"),
            }
            tried += 1;
            cut += 1 + len / 23;
        }
        let again = lower(&prog).unwrap();
        assert_eq!(again.code, whole.code, "{name}: different code after the refusals");
    }
    assert!(tried > 100, "only {tried} cuts tried");
}
