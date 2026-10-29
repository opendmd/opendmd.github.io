//! The compiler's heap: its peak per bundled and example Formula (and the
//! `worst` cases), held to a budget, and a compile the heap can't hold
//! refused with [`NO_MEMORY`] instead of aborting.
//!
//! Endless road aborted the spare on 2026-10-29 (`memory allocation of
//! 4096 bytes failed` in the layout's slot list, 9,648 B free): its compile
//! peaked at 80,225 B on the host (64-bit), the token list (1,064 tokens at
//! 24 B, grown by doubling: the 24 KB and the 48 KB blocks at once) the
//! most of it, and held through the layout. Now ~30 KB: the tokens counted
//! first and kept at 12 B, let go before the layout with the names and the
//! sharing index; the nodes and the slots reserved once.
//!
//! A counting allocator (this thread's allocations only, so the harness's
//! other tests don't count) around `compile_into` into a `Program` made
//! beforehand (the player owns that box). A `realloc` counts as the new
//! block taken before the old one is let go, as on the ESP32 when it can't
//! grow in place.
//!
//! `cargo test -p opendmd-psi --release --test compile_heap -- --nocapture`
//! prints the table.
// (No file system in the wasm run.)
#![cfg(not(target_arch = "wasm32"))]

use opendmd_psi::{compact, compile_heap_estimate, compile_into, Program, NO_MEMORY};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// What any bundled Formula may peak at, on the host. The spare has ~86-90
/// KB free before a Formula opens (the outgoing GIF is let go before the
/// compile: `Engine::advance` drops it, then `open_next`), less the
/// Formula's 10 KB box and the compile's 12.5 KB stack: ~63 KB. 36 KB
/// leaves ~27 KB for a look, the glow and the network meanwhile. The
/// device's lists are no bigger than the host's (32-bit), so these numbers
/// bound it.
const BUNDLED_BUDGET: usize = 36 * 1024;

struct Counting;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static NOW: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
    /// Refuse a block of at least [`BIG`] bytes that would take the count
    /// over this (the short heap's model: what fails on the device is a
    /// big block; the small ones are what `psi_stack`'s reserve is for).
    static CAP: Cell<usize> = const { Cell::new(usize::MAX) };
}

const BIG: usize = 256;

/// Count `n` more, or refuse it (`false`).
fn take(n: usize) -> bool {
    ON.try_with(|on| {
        if !on.get() {
            return true;
        }
        let v = NOW.with(|c| c.get()) + n;
        if n >= BIG && v > CAP.with(|c| c.get()) {
            return false;
        }
        NOW.with(|c| c.set(v));
        PEAK.with(|p| p.set(p.get().max(v)));
        COUNT.with(|c| c.set(c.get() + 1));
        true
    })
    .unwrap_or(true)
}

fn give(n: usize) {
    let _ = ON.try_with(|on| {
        if on.get() {
            NOW.with(|c| c.set(c.get().wrapping_sub(n)));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if !take(l.size()) {
            return std::ptr::null_mut();
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        give(l.size());
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if !take(n) {
            return std::ptr::null_mut();
        }
        give(l.size());
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// One compile of `src` with blocks of [`BIG`] or more refused over `cap`:
/// (peak bytes held at once, allocations, the result).
fn run(src: &str, prog: &mut Program, cap: usize) -> (usize, usize, Result<(), String>) {
    NOW.with(|c| c.set(0));
    PEAK.with(|c| c.set(0));
    COUNT.with(|c| c.set(0));
    CAP.with(|c| c.set(cap));
    ON.with(|c| c.set(true));
    let r = compile_into(src, prog).map(|_| ()).map_err(|e| e.msg);
    ON.with(|c| c.set(false));
    (PEAK.with(|c| c.get()), COUNT.with(|c| c.get()), r)
}

fn strip_header(text: &str) -> String {
    text.split_inclusive('\n')
        .skip_while(|l| {
            ["-- author:", "-- licence:", "-- license:", "-- bundle"]
                .iter()
                .any(|h| l.starts_with(h))
        })
        .collect()
}

/// Every Formula in `opendmd-web/formulas` (name, source as the firmware
/// compiles it: compacted, as the path's `src=` carries it; bundled), then
/// the `worst` cases.
fn formulas() -> Vec<(String, String, bool)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../opendmd-web/formulas");
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "psi"))
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            let bundled = text.lines().any(|l| l.starts_with("-- bundle"));
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            (name, compact(&strip_header(&text)), bundled)
        })
        .collect();
    v.sort();
    assert!(v.len() >= 30, "only {} formulas found in {dir}", v.len());
    use opendmd_psi::worst;
    for (n, s) in [
        ("worst::many_locals", worst::many_locals()),
        ("worst::chain", worst::chain()),
        ("worst::distinct", worst::distinct()),
        ("worst::parts", worst::parts()),
        ("worst::nested", worst::nested()),
        ("worst::nested_digit", worst::nested_digit()),
    ] {
        v.push((n.to_string(), s, false));
    }
    v
}

/// Every compile peaks under `compile_heap_estimate` (what the firmware
/// asks the heap for before it compiles), and every bundled Formula under
/// [`BUNDLED_BUDGET`].
///
/// Not with `syntax`: the language service's records come on top (the
/// editor's, never on the device); `make test` runs this file without it.
#[test]
#[cfg_attr(feature = "syntax", ignore = "the language service's records come on top")]
fn compile_peak_heap_is_in_budget() {
    let mut prog = Box::<Program>::default();
    let mut bad = Vec::new();
    for (name, src, bundled) in formulas() {
        let (peak, n, r) = run(&src, &mut prog, usize::MAX);
        let est = compile_heap_estimate(&src);
        eprintln!(
            "{name:<24} {:<7} {:>5} B source {peak:>7} B peak {n:>4} allocs (estimate {est} B){}",
            if bundled { "bundled" } else { "" },
            src.len(),
            if r.is_ok() { "" } else { "  (an error)" }
        );
        if bundled {
            assert!(r.is_ok(), "{name}: {r:?}");
        }
        if peak > est {
            bad.push(format!("{name}: {peak} B over its estimate {est} B"));
        }
        if bundled && peak > BUNDLED_BUDGET {
            bad.push(format!("{name}: {peak} B over the budget {BUNDLED_BUDGET} B"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// A heap that can't give the compile a big block is a refusal with
/// [`NO_MEMORY`] wherever it runs short, never an abort (the test would
/// die with the process) and never another error; with room, the same
/// program as always.
///
/// Not with `syntax` (the language service's records of what it read grow
/// infallibly: the editor's, never on the device).
#[test]
#[cfg_attr(feature = "syntax", ignore = "the language service's records grow infallibly")]
fn a_short_heap_refuses_the_formula() {
    let mut prog = Box::<Program>::default();
    let mut want = Box::<Program>::default();
    let empty = Box::<Program>::default().listing();
    for (name, src, _) in formulas() {
        let (peak, _, full) = run(&src, &mut want, usize::MAX);
        let mut refused = 0;
        for cap in (0..peak).step_by(160) {
            match run(&src, &mut prog, cap).2 {
                Ok(()) => assert_eq!(prog.listing(), want.listing(), "{name} at {cap} B"),
                Err(e) if e == NO_MEMORY => {
                    refused += 1;
                    assert_eq!(prog.listing(), empty, "{name}: a program left at {cap} B");
                }
                Err(e) => assert_eq!(Err(e), full, "{name} at {cap} B"),
            }
        }
        assert!(refused > 0, "{name}: never refused");
    }
}
