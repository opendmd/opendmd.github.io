//! What a Formula says about itself that the player needs: its controls'
//! kinds, ranges and defaults, and whether it is offered as a look (its
//! `control` and `look` lines, docs/research/formula-apps.md §2.1-2.2).
//! The rest of the declarations (`name`, `about`, a control's label and
//! words) is text the browser reads back from the source
//! ([`crate::compile_declared`]): it is never stored in a [`Program`].
//!
//! [`Program`]: crate::Program

/// A slot's control, as declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// `control NAME lo..hi = default`: a value from `lo` to `hi` (Q16.16),
    /// clamped to that range wherever the formula reads it.
    Slider { lo: i32, hi: i32, default: i32 },
    /// `control NAME = w1, w2, …`: the chosen word's number, 0 to `n − 1`
    /// (whole; the first word, 0, is the default).
    Choice { n: u8 },
}

const NONE: u8 = 0;
const SLIDER: u8 = 1;
const CHOICE: u8 = 2;

/// A compiled formula's declarations (44 bytes in [`Program`]).
///
/// [`Program`]: crate::Program
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    /// Each slot's (`a b c`) kind: none, a slider, a choice.
    kind: [u8; 3],
    /// `look`: 0 none, else 1 + the blend's place in [`crate::look::BLENDS`].
    look: u8,
    /// `look`'s amount, 0 to 100.
    amount: u8,
    /// Each slot's range and default (Q16.16; a choice's: 0, n − 1, 0).
    lo: [i32; 3],
    hi: [i32; 3],
    def: [i32; 3],
}

impl Meta {
    /// Slot `k`'s control (0 `a`, 1 `b`, 2 `c`), `None` when undeclared (a
    /// formula that reads `a` without a `control` line: a slider 0 to 1,
    /// default 0, as before the declarations).
    pub fn control(&self, k: usize) -> Option<Control> {
        match *self.kind.get(k)? {
            SLIDER => Some(Control::Slider {
                lo: self.lo[k],
                hi: self.hi[k],
                default: self.def[k],
            }),
            CHOICE => Some(Control::Choice {
                n: (self.hi[k] >> 16) as u8 + 1,
            }),
            _ => None,
        }
    }

    /// Bit `k` set for each declared slot.
    pub fn declared(&self) -> u8 {
        (0..3).filter(|&k| self.kind[k] != NONE).fold(0, |m, k| m | 1 << k)
    }

    /// Slot `k`'s default (Q16.16): the declared one, else 0.
    pub fn default_value(&self, k: usize) -> i32 {
        self.def.get(k).copied().unwrap_or(0)
    }

    /// The value a slot plays at: the one given (a path's `&a=`, a
    /// slider's), else the declared default. The formula clamps it to the
    /// declared range itself; nothing here needs to.
    pub fn value(&self, k: usize, given: Option<i32>) -> i32 {
        given.unwrap_or_else(|| self.default_value(k))
    }

    /// [`value`](Self::value) for the three slots.
    pub fn values(&self, given: [Option<i32>; 3]) -> [i32; 3] {
        [
            self.value(0, given[0]),
            self.value(1, given[1]),
            self.value(2, given[2]),
        ]
    }

    /// `look [blend] [amount]`: the blend (one of [`crate::look::BLENDS`])
    /// and the amount (0 to 100), when the formula says it may be a look.
    pub fn look(&self) -> Option<(&'static str, u8)> {
        let b = crate::look::BLENDS.get(usize::from(self.look).checked_sub(1)?)?;
        Some((b, self.amount))
    }

    pub(crate) fn set_slider(&mut self, k: usize, lo: i32, hi: i32, default: i32) {
        self.kind[k] = SLIDER;
        (self.lo[k], self.hi[k], self.def[k]) = (lo, hi, default);
    }

    pub(crate) fn set_choice(&mut self, k: usize, n: usize) {
        self.kind[k] = CHOICE;
        (self.lo[k], self.hi[k], self.def[k]) = (0, ((n as i32) - 1) << 16, 0);
    }

    pub(crate) fn set_look(&mut self, blend: usize, amount: u8) {
        self.look = blend as u8 + 1;
        self.amount = amount;
    }
}

const _: () = assert!(core::mem::size_of::<Meta>() == 44);
