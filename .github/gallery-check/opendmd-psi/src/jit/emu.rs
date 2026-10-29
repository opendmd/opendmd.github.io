//! An emulator of the Xtensa instructions the kernels use, for the host
//! tests: a kernel runs here against the VM over the goldens and the
//! bundled formulas before it ever runs on the device. Its own 32-bit
//! address space maps onto the host memory the driver's words point at
//! (each object a region of its own, registered as its address is asked
//! for); a helper call runs the helper (`eval_helper`, the device shims'
//! one definition) and then scribbles over everything a `callx8` does not
//! keep (a8, a9, a11..a15, SAR), so a kernel that relied on them fails
//! here. Anything outside the subset, a misaligned or out-of-region
//! access, a write to a0 or a1 after `entry`, panics.

use super::lower::hid;
use super::{eval_helper, Target};

const CODE: u32 = 0x4000_0000;
const STACK: u32 = 0x3000_0000;
const STACK_LEN: usize = 4096;
const HELPERS: u32 = 0x5000_0000;
const RETURN: u32 = 0x6000_0000;
const B4CONST: [i32; 16] = [-1, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 16, 32, 64, 128, 256];
const B4CONSTU: [u32; 16] = [
    32768, 65536, 2, 3, 4, 5, 6, 7, 8, 10, 12, 16, 32, 64, 128, 256,
];
const JUNK: u32 = 0xdead_beef;

pub(crate) struct Emu {
    code: Vec<u8>,
    /// The running function's reserved top of frame, `[top - 32, top)`
    /// (its window's save areas): a kernel store there panics.
    reserved: Option<(u32, u32)>,
    /// Host regions: (address, length); region k is at `(k + 1) << 20`.
    regions: Vec<(usize, usize)>,
    stack: Vec<u8>,
    /// Instructions run (all rows so far).
    pub steps: u64,
    /// Helper calls made.
    pub calls: u64,
}

fn sext(v: u32, bits: u32) -> i32 {
    ((v << (32 - bits)) as i32) >> (32 - bits)
}

impl Emu {
    pub fn new(code: &[u8]) -> Emu {
        Emu {
            code: code.to_vec(),
            reserved: None,
            regions: Vec::new(),
            stack: vec![0; STACK_LEN],
            steps: 0,
            calls: 0,
        }
    }

    fn map(&mut self, p: *const u8, len: usize) -> u32 {
        let a = p as usize;
        for (k, &(b, n)) in self.regions.iter().enumerate() {
            if a >= b && a + len <= b + n {
                return ((k as u32 + 1) << 20) + (a - b) as u32;
            }
        }
        assert!(len < 1 << 20);
        self.regions.push((a, len));
        (self.regions.len() as u32) << 20
    }

    /// The host address of `n` bytes at emulator address `a`.
    fn host(&mut self, a: u32, n: usize) -> *mut u8 {
        if (STACK..STACK + STACK_LEN as u32).contains(&a) {
            let o = (a - STACK) as usize;
            assert!(o + n <= STACK_LEN, "stack access out of range at {a:#x}");
            return self.stack[o..].as_mut_ptr();
        }
        let k = (a >> 20) as usize;
        assert!(
            k >= 1 && k <= self.regions.len(),
            "access to an unmapped address {a:#x}"
        );
        let (b, len) = self.regions[k - 1];
        let o = (a & 0xfffff) as usize;
        assert!(
            o + n <= len,
            "access past a region's end at {a:#x} ({n} bytes, region {len})"
        );
        (b + o) as *mut u8
    }

    fn ld(&mut self, a: u32, n: usize) -> u32 {
        assert!(
            (a as usize).is_multiple_of(n),
            "misaligned {n}-byte load at {a:#x}"
        );
        let p = self.host(a, n);
        // SAFETY: inside a live region (checked by `host`).
        unsafe {
            match n {
                1 => u32::from(*p),
                2 => u32::from(u16::from_le_bytes([*p, *p.add(1)])),
                _ => u32::from_le_bytes([*p, *p.add(1), *p.add(2), *p.add(3)]),
            }
        }
    }

    fn st(&mut self, a: u32, n: usize, v: u32) {
        if let Some((lo, hi)) = self.reserved {
            assert!(
                a + n as u32 <= lo || a >= hi,
                "a store into the frame's save areas at {a:#x} ([{lo:#x}, {hi:#x}): a window spill keeps the caller's caller's registers there)"
            );
        }
        assert!(
            (a as usize).is_multiple_of(n),
            "misaligned {n}-byte store at {a:#x}"
        );
        let p = self.host(a, n);
        // SAFETY: as `ld`.
        unsafe {
            for k in 0..n {
                *p.add(k) = (v >> (8 * k)) as u8;
            }
        }
    }

    /// Run the kernel once: `(ctx, out)` in a2, a3.
    pub fn call(&mut self, ctx: u32, out: u32) {
        self.call_at(0, ctx, out)
    }

    /// Run the function at `start` once, (a2, a3) its arguments.
    pub fn call_at(&mut self, start: usize, ctx: u32, out: u32) {
        let mut a = [0u32; 16];
        a[0] = RETURN;
        a[1] = STACK + STACK_LEN as u32;
        a[2] = ctx;
        a[3] = out;
        let mut sar: u32 = JUNK;
        let mut pc: usize = start;
        let mut sp_set = false;
        let mut sp = 0;
        let mut frame_top = 0u32;
        loop {
            self.steps += 1;
            assert!(self.steps < 2_000_000_000, "the kernel ran away");
            assert!(pc + 2 <= self.code.len(), "pc {pc:#x} outside the code");
            let b0 = self.code[pc] as u32;
            let op0 = b0 & 15;
            let w16 = b0 | (self.code[pc + 1] as u32) << 8;
            let (t, s, r) = ((w16 >> 4) & 15, (w16 >> 8) & 15, (w16 >> 12) & 15);
            let (t_, s_, r_) = (t as usize, s as usize, r as usize);
            if op0 >= 8 {
                // Density.
                let mut wr: Option<(usize, u32)> = None;
                match op0 {
                    8 => wr = Some((t_, self.ld(a[s_].wrapping_add(r * 4), 4))),
                    9 => self.st(a[s_].wrapping_add(r * 4), 4, a[t_]),
                    10 => wr = Some((r_, a[s_].wrapping_add(a[t_]))),
                    11 => wr = Some((r_, a[s_].wrapping_add(if t == 0 { u32::MAX } else { t }))),
                    12 if t & 8 == 0 => {
                        let v = (t & 7) << 4 | r;
                        let v = if v >= 96 { v as i32 - 128 } else { v as i32 };
                        wr = Some((s_, v as u32));
                    }
                    13 if r == 0 => wr = Some((t_, a[s_])),
                    13 if r == 15 && s == 0 && t == 1 => return, // retw.n
                    _ => panic!("unhandled density instruction {w16:04x} at {pc:#x}"),
                }
                if let Some((d, v)) = wr {
                    assert!(d >= 2, "a write to a{d} at {pc:#x}");
                    a[d] = v;
                }
                pc += 2;
                continue;
            }
            assert!(pc + 3 <= self.code.len());
            let w = w16 | (self.code[pc + 2] as u32) << 16;
            let (op1, op2) = ((w >> 16) & 15, (w >> 20) & 15);
            let mut next = pc + 3;
            let mut wr: Option<(usize, u32)> = None;
            let target = |off: i32| (pc as i32 + 4 + off) as usize;
            let (av, at_, as__) = (a[r_], a[t_], a[s_]);
            let _ = av;
            match op0 {
                0 => match (op1, op2) {
                    (0, 0) => match (r, t >> 2, t & 3) {
                        (0, 2, 1) => return, // retw
                        (0, 3, 2) => {
                            // callx8
                            let f = as__;
                            assert!(
                                (HELPERS..HELPERS + 16 * u32::from(hid::COUNT)).contains(&f),
                                "callx8 to {f:#x}"
                            );
                            let id = ((f - HELPERS) / 16) as u8;
                            let args = [a[10], a[11], a[12], a[13], a[14]];
                            self.calls += 1;
                            let me = self as *mut Emu;
                            // SAFETY: the helper's pointer arguments are its
                            // words' addresses (mapped above); `me` is only
                            // used to translate them.
                            let v = unsafe { eval_helper(id, args, &|p| (*me).host(p, 1)) };
                            for k in [8, 9, 11, 12, 13, 14, 15] {
                                a[k] = JUNK ^ (k as u32);
                            }
                            // A window overflow while the helper runs may
                            // spill this frame's a4..a7 into its top
                            // [frame-32, frame-16) and its caller's caller's
                            // a0..a3 into [frame-16, frame): a kernel must
                            // keep nothing there (jit2-a on the emulator: a
                            // slot at 64 of an 80 B frame took Kernel::check's
                            // return address).
                            self.reserved = None;
                            for o in (frame_top - 32..frame_top).step_by(4) {
                                self.st(o, 4, JUNK ^ o);
                            }
                            self.reserved = Some((frame_top - 32, frame_top));
                            a[10] = v;
                            sar = JUNK;
                        }
                        (2, _, _) if t == 0xc => {} // memw
                        _ => panic!("unhandled ST0 {w:06x} at {pc:#x}"),
                    },
                    (0, 1) => wr = Some((r_, as__ & at_)),
                    (0, 2) => wr = Some((r_, as__ | at_)),
                    (0, 3) => wr = Some((r_, as__ ^ at_)),
                    (0, 4) if r == 4 => sar = s | (t & 1) << 4,
                    (0, 6) if s == 0 => wr = Some((r_, (at_ as i32).wrapping_neg() as u32)),
                    (0, 6) if s == 1 => wr = Some((r_, (at_ as i32).wrapping_abs() as u32)),
                    (0, 8) => wr = Some((r_, as__.wrapping_add(at_))),
                    (0, 9) => wr = Some((r_, (as__ << 1).wrapping_add(at_))),
                    (0, 10) => wr = Some((r_, (as__ << 2).wrapping_add(at_))),
                    (0, 12) => wr = Some((r_, as__.wrapping_sub(at_))),
                    (1, 0 | 1) => {
                        let sa = 32 - (t | (op2 & 1) << 4);
                        wr = Some((r_, as__ << sa));
                    }
                    (1, 2 | 3) => wr = Some((r_, ((at_ as i32) >> (s | (op2 & 1) << 4)) as u32)),
                    (1, 4) => wr = Some((r_, at_ >> s)),
                    (1, 8) => {
                        assert!(sar < 32, "src with SAR unset at {pc:#x}");
                        let v = ((as__ as u64) << 32 | at_ as u64) >> sar;
                        wr = Some((r_, v as u32));
                    }
                    (2, 8) => wr = Some((r_, as__.wrapping_mul(at_))),
                    (2, 10) => wr = Some((r_, ((as__ as u64 * at_ as u64) >> 32) as u32)),
                    (2, 11) => {
                        wr = Some((r_, ((as__ as i32 as i64 * at_ as i32 as i64) >> 32) as u32))
                    }
                    (3, 4) => wr = Some((r_, (as__ as i32).min(at_ as i32) as u32)),
                    (3, 5) => wr = Some((r_, (as__ as i32).max(at_ as i32) as u32)),
                    (3, 6) => wr = Some((r_, as__.min(at_))),
                    (3, 7) => wr = Some((r_, as__.max(at_))),
                    (3, 8) if at_ == 0 => wr = Some((r_, as__)),
                    (3, 9) if at_ != 0 => wr = Some((r_, as__)),
                    (3, 10) if (at_ as i32) < 0 => wr = Some((r_, as__)),
                    (3, 11) if (at_ as i32) >= 0 => wr = Some((r_, as__)),
                    (3, 8..=11) => {}
                    (4 | 5, _) => {
                        let shift = s | (op1 & 1) << 4;
                        let bits = op2 + 1;
                        wr = Some((r_, (at_ >> shift) & ((1u32 << bits) - 1)));
                    }
                    _ => panic!("unhandled RRR {w:06x} at {pc:#x}"),
                },
                2 => {
                    let imm8 = w >> 16;
                    match r {
                        0 => wr = Some((t_, self.ld(as__.wrapping_add(imm8), 1))),
                        2 => wr = Some((t_, self.ld(as__.wrapping_add(imm8 * 4), 4))),
                        4 => self.st(as__.wrapping_add(imm8), 1, at_),
                        6 => self.st(as__.wrapping_add(imm8 * 4), 4, at_),
                        9 => {
                            wr = Some((
                                t_,
                                self.ld(as__.wrapping_add(imm8 * 2), 2) as u16 as i16 as i32 as u32,
                            ))
                        }
                        10 => wr = Some((t_, sext(s << 8 | imm8, 12) as u32)),
                        12 => wr = Some((t_, as__.wrapping_add(sext(imm8, 8) as u32))),
                        13 => wr = Some((t_, as__.wrapping_add((sext(imm8, 8) << 8) as u32))),
                        _ => panic!("unhandled LSAI {w:06x} at {pc:#x}"),
                    }
                }
                6 => {
                    let (n, m) = ((w >> 4) & 3, (w >> 6) & 3);
                    match n {
                        0 => next = target(sext(w >> 6, 18)),
                        1 => {
                            let v = as__ as i32;
                            let take = match m {
                                0 => v == 0,
                                1 => v != 0,
                                2 => v < 0,
                                _ => v >= 0,
                            };
                            if take {
                                next = target(sext(w >> 12, 12));
                            }
                        }
                        2 => {
                            let (v, c) = (as__ as i32, B4CONST[r_]);
                            let take = match m {
                                0 => v == c,
                                1 => v != c,
                                2 => v < c,
                                _ => v >= c,
                            };
                            if take {
                                next = target(sext(w >> 16, 8));
                            }
                        }
                        _ => match m {
                            0 => {
                                assert!(!sp_set && pc == start && s == 1, "entry not first");
                                let frame = (w >> 12) << 3;
                                assert!(frame >= 16 && frame as usize <= STACK_LEN);
                                frame_top = a[1];
                                a[1] -= frame;
                                sp = a[1];
                                sp_set = true;
                                self.reserved = Some((frame_top - 32, frame_top));
                            }
                            2 | 3 => {
                                let c = B4CONSTU[r_];
                                let take = if m == 2 { as__ < c } else { as__ >= c };
                                if take {
                                    next = target(sext(w >> 16, 8));
                                }
                            }
                            _ => panic!("unhandled BI1 {w:06x} at {pc:#x}"),
                        },
                    }
                }
                7 => {
                    let off = sext(w >> 16, 8);
                    let take = match r {
                        1 => as__ == at_,
                        2 => (as__ as i32) < at_ as i32,
                        3 => as__ < at_,
                        9 => as__ != at_,
                        10 => as__ as i32 >= at_ as i32,
                        11 => as__ >= at_,
                        6 | 7 => as__ & (1 << (t | (r & 1) << 4)) == 0,
                        _ => panic!("unhandled branch {w:06x} at {pc:#x}"),
                    };
                    if take {
                        next = target(off);
                    }
                }
                _ => panic!("unhandled instruction {w:06x} at {pc:#x}"),
            }
            if let Some((d, v)) = wr {
                assert!(d >= 2, "a write to a{d} at {pc:#x}");
                a[d] = v;
            }
            assert!(sp_set, "the first instruction must be entry");
            assert_eq!(a[1], sp, "a1 moved at {pc:#x}");
            pc = next;
        }
    }
}

/// The emulator as a kernel's target (the host tests).
pub(crate) struct EmuTarget {
    pub emu: Emu,
}

impl Target for EmuTarget {
    fn begin(&mut self) {
        self.emu.regions.clear();
    }

    fn addr(&mut self, p: *const u8, len: usize) -> u32 {
        self.emu.map(p, len)
    }

    fn helper(&mut self, id: u8) -> u32 {
        HELPERS + 16 * u32::from(id)
    }

    fn run(&mut self, ctx: &mut [u32], out: *mut u8) {
        let c = self.emu.map(ctx.as_mut_ptr() as *const u8, ctx.len() * 4);
        let o = self.emu.map(out, 128 * 3);
        self.emu.call(c, o);
    }

    fn run_rows(&mut self, at: usize, ctx: &mut [u32], sc: *mut i32) {
        let c = self.emu.map(ctx.as_mut_ptr() as *const u8, ctx.len() * 4);
        let s = self.emu.map(sc as *const u8, crate::program::MAX_SCALARS * 4);
        self.emu.call_at(at, c, s);
    }
}

#[allow(dead_code)]
const _: u32 = CODE;
