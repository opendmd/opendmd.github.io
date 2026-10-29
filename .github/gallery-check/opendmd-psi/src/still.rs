//! The still cache: values that depend on the pixel's position only (`x y
//! px py i`, constants, and anything worked out from them alone) are the
//! same every frame, so a player may work them out once and keep them.
//!
//! The compiler chooses up to [`STILL_LANES`] such values per formula — the
//! ones that save the most per frame (a tunnel's `1/len(cx, cy*2)` and
//! `atan2(cy, cx)`), only when each saves at least 0.4 µs a pixel — and lays
//! out the steps that only feed them as one run after a `STILL` marker
//! ([`crate::Program::still_lanes`]). A player that has a [`Still`] (16 KB a
//! value, [`StillLane`]) gives it to [`crate::render_rows_still`]: the first
//! full frame works the values out as usual and keeps them; every frame
//! after skips those steps and copies the kept lanes into their registers.
//! A player without one (short of heap, a look, the web app's previews)
//! runs every step every frame. **The frame's bytes are the same either
//! way**: the kept values are the same steps' results (the same `fixed`
//! arithmetic), so the cache is only an optimisation a player may decline
//! or let go at any moment.

use crate::program::*;
use alloc::boxed::Box;

/// Values a formula may have cached (the box holds a lane for each).
pub const STILL_LANES: usize = 2;

/// One cached value for every pixel: 4,096 Q16.16 values, 16 KB, row-major
/// from the top as the frame is.
#[repr(C)]
pub struct StillLane(pub(crate) [i32; W * H]);

impl StillLane {
    const LAYOUT: core::alloc::Layout = core::alloc::Layout::new::<StillLane>();

    /// A zeroed lane (psi-probe's placement test), `None` without 16 KB.
    #[doc(hidden)]
    pub fn try_new() -> Option<Box<StillLane>> {
        Self::try_boxed()
    }

    /// A lane on the heap made zeroed in place, or `None` when the heap
    /// hasn't 16 KB.
    fn try_boxed() -> Option<Box<StillLane>> {
        // SAFETY: a non-zero-sized layout; `StillLane` is integers, so the
        // zeroed memory is a valid value; the box frees it with the same
        // layout.
        unsafe {
            let p = alloc::alloc::alloc_zeroed(Self::LAYOUT).cast::<StillLane>();
            (!p.is_null()).then(|| Box::from_raw(p))
        }
    }
}

/// A player's still cache: the lanes (taken from the heap only when asked,
/// [`take`](Still::take), fallibly) and which program's values they hold.
/// All-zero is a valid value (no lanes, nothing kept), so it can sit in a
/// zeroed allocation.
#[derive(Default)]
pub struct Still {
    pub(crate) lanes: [Option<Box<StillLane>>; STILL_LANES],
    /// The fingerprint of the steps whose values the lanes hold
    /// ([`fingerprint`]), 0: nothing kept.
    pub(crate) kept: u32,
}

impl Still {
    /// psi-probe's placement test: put `lane` in place of lane `k` (which
    /// is handed back), and forget what was kept (the next full frame keeps
    /// it again).
    #[doc(hidden)]
    pub fn swap_lane(&mut self, k: usize, lane: Box<StillLane>) -> Option<Box<StillLane>> {
        self.kept = 0;
        self.lanes.get_mut(k)?.replace(lane)
    }

    /// Lane `k`'s address, 0 for none.
    #[doc(hidden)]
    pub fn lane_addr(&self, k: usize) -> usize {
        self.lanes
            .get(k)
            .and_then(|l| l.as_deref())
            .map_or(0, |l| l.0.as_ptr() as usize)
    }

    pub const fn new() -> Still {
        Still {
            lanes: [None, None],
            kept: 0,
        }
    }

    /// The heap `prog`'s cache takes (16 KB a value; 0 for a formula
    /// without cached values).
    pub fn bytes_for(prog: &Program) -> usize {
        prog.still_lanes() * core::mem::size_of::<StillLane>()
    }

    /// Take the lanes `prog` needs (any it doesn't are let go): `false`,
    /// and none held, when the heap hasn't them or the formula has no
    /// cached values. Nothing is kept until the next full frame.
    pub fn take(&mut self, prog: &Program) -> bool {
        let n = prog.still_lanes();
        self.kept = 0;
        for (k, l) in self.lanes.iter_mut().enumerate() {
            if k >= n {
                *l = None;
            } else if l.is_none() {
                *l = StillLane::try_boxed();
                if l.is_none() {
                    self.release();
                    return false;
                }
            }
        }
        n > 0
    }

    /// Let the lanes go (a frame after works everything out as usual).
    pub fn release(&mut self) {
        self.lanes = [None, None];
        self.kept = 0;
    }

    /// Whether lanes are held (kept values or not).
    pub fn is_held(&self) -> bool {
        self.lanes[0].is_some()
    }

    /// The heap the lanes hold, bytes.
    pub fn held_bytes(&self) -> usize {
        self.lanes.iter().filter(|l| l.is_some()).count() * core::mem::size_of::<StillLane>()
    }

    /// Whether the lanes hold `prog`'s values (a full frame of it has been
    /// drawn since they were taken).
    pub fn holds(&self, prog: &Program) -> bool {
        self.kept != 0 && self.kept == fingerprint(prog)
    }

    /// The lanes, if there are enough for `prog`.
    pub(crate) fn lanes_for(&self, prog: &Program) -> bool {
        let n = prog.still_lanes();
        n > 0 && self.lanes[..n].iter().all(Option::is_some)
    }
}

/// What the kept values depend on: the steps up to the end of the cached
/// run (the per-row ones included: `y`-based values the cached ones read),
/// the entries of the tables they read, and the constants. Never 0. A program changed in place (a live edit)
/// that the player forgot to tell the cache about is noticed here, so a
/// frame is never drawn from another formula's values.
pub(crate) fn fingerprint(prog: &Program) -> u32 {
    let Some(k) = prog.still_marker() else {
        return 0;
    };
    let end = (k + 1 + prog.ops[k].a as usize).min(MAX_OPS);
    // FNV-1a, 32-bit (a 64-bit multiply is several on the ESP32): ~1 KB a
    // frame, ~20 µs there.
    let mut h: u32 = 0x811c_9dc5;
    let mut eat = |b: u8| {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    };
    for op in &prog.ops[prog.draw_end as usize..end] {
        for b in [op.code, op.d, op.a, op.b, op.c] {
            eat(b);
        }
        // A table the steps read: its entries (a live edit may change only
        // them).
        // (A sprite's words, its header first.)
        let (a, n) = match op.code {
            code::TAB => (op.b as usize, op.c as usize + 1),
            code::SPR => {
                let h = crate::program::SprHead::new(&prog.tables, op.c);
                (op.c as usize, h.words())
            }
            _ => (0, 0),
        };
        if n > 0 {
            for v in &prog.tables[a.min(MAX_TABLE)..(a + n).min(MAX_TABLE)] {
                for b in v.to_le_bytes() {
                    eat(b);
                }
            }
        }
    }
    for v in &prog.init[..(prog.n_scalars as usize).min(MAX_SCALARS)] {
        for b in v.to_le_bytes() {
            eat(b);
        }
    }
    eat(prog.uses);
    h | 1
}
