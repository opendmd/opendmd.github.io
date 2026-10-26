//! The compiler's stack at its worst (`opendmd_psi::worst`), pinned below
//! a budget so a change can't make the ESP32's compile stack overflow
//! unseen. Measured on the host the way `examples/stack.rs` does (a thread
//! of its own, the stack below it filled with a pattern, a compile, the
//! deepest byte changed), so only meaningful optimised as the ESP32 build
//! is: `make test` runs it with `CARGO_PROFILE_RELEASE_OPT_LEVEL=2 cargo
//! test --release --test stack`; unoptimised it is ignored.
//!
//! **The calibration** (`psi-probe` on the spare, 2026-10-26, depth cap 24):
//! the deepest case used 11,808 B of a thread's stack on the ESP32 where
//! the host used 15,895 B (`nested digit`) and 15,527 B (`nested`): the
//! ESP32 takes ~0.76 of the host's bytes. The player compiles on 12,288 B
//! (`PSI_COMPILE_STACK`), ~800 B of it the thread's entry frames and
//! interrupts; [`HOST_BUDGET`] at 0.76 is ~9.9 KB on the ESP32, which leaves
//! ~1.6 KB never touched.
#![cfg(not(target_arch = "wasm32"))]

use opendmd_psi::{compile_into, worst, Program};

/// The most a worst case may use on the host, optimised at opt-level 2.
const HOST_BUDGET: usize = 13_000;

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

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "frame sizes only mean something optimised: make test runs it at opt-level 2"
)]
fn the_worst_compile_fits_the_esp32s_compile_stack() {
    let mut over = Vec::new();
    for (name, f) in worst::ALL {
        let src = f();
        let n = std::thread::Builder::new()
            .stack_size(4 << 20)
            .spawn(move || {
                let mut prog = Box::new(Program::default());
                let sp = here();
                fill(sp);
                let _ = compile_into(core::hint::black_box(&src), &mut prog);
                used(sp)
            })
            .unwrap()
            .join()
            .unwrap();
        if n > HOST_BUDGET {
            over.push(format!("{name}: {n} B"));
        }
    }
    assert!(
        over.is_empty(),
        "over the {HOST_BUDGET} B host budget (~0.76 of it on the ESP32's 12 KB compile stack): {over:?}"
    );
}
