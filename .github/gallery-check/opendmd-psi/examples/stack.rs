//! The compiler's stack high-water on the host for the worst formulas
//! (`opendmd_psi::worst`), the way the review measured it: a thread of its
//! own, the stack below it filled with a pattern, a compile, then the
//! deepest byte changed. Relative numbers (the ESP32's frames differ; its
//! own figure comes from `psi-probe`).
//!
//!     CARGO_PROFILE_RELEASE_OPT_LEVEL=2 cargo run --release --example stack

use opendmd_psi::{compile_into, worst, Program};

const SPAN: usize = 512 * 1024;

#[inline(never)]
fn here() -> usize {
    let x = 0u8;
    core::hint::black_box(&x) as *const u8 as usize
}

#[inline(never)]
fn fill(sp: usize) {
    for a in (sp - SPAN)..(sp - 512) {
        // SAFETY: below the stack pointer, inside this thread's 4 MB stack.
        unsafe { core::ptr::write_volatile(a as *mut u8, 0xA5) };
    }
}

#[inline(never)]
fn used(sp: usize) -> usize {
    let mut a = sp - SPAN;
    // SAFETY: as `fill`.
    while a < sp && unsafe { core::ptr::read_volatile(a as *const u8) } == 0xA5 {
        a += 1;
    }
    sp - a
}

fn main() {
    let mut cases: Vec<(String, String)> = worst::ALL
        .iter()
        .map(|(n, f)| (n.to_string(), f()))
        .collect();
    cases.insert(0, ("gray(x)".into(), "gray(x)".into()));
    for (name, src) in cases {
        let r = std::thread::Builder::new()
            .stack_size(4 << 20)
            .spawn(move || {
                let mut prog = Box::new(Program::default());
                let sp = here();
                fill(sp);
                let res = compile_into(core::hint::black_box(&src), &mut prog);
                let n = used(sp);
                (n, res.err().map(|e| e.msg))
            })
            .unwrap()
            .join()
            .unwrap();
        println!(
            "{name:<16} {:>7} B  {}",
            r.0,
            r.1.unwrap_or_else(|| "ok".into())
        );
    }
}
