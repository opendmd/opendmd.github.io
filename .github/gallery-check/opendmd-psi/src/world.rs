//! The built-in inputs (`opendmd-notes docs/research/psi-inputs.md` §3): values the
//! panel keeps about the world, which a Formula reads by name without a
//! line of its own (`daylight`, `sun.alt`, `replay.game` …), and the two
//! functions over them, `since(KEY)` (seconds since the key last changed)
//! and `was(KEY)` (its value before that).
//!
//! Each read is a frame value: the compiler gives it a scalar slot of its
//! own after the fixed inputs ([`crate::Program`]'s input list, in the
//! psb), and the player writes the slot once a frame from a [`World`],
//! held to the key's range, so anything made from it is hoisted and costs
//! nothing per pixel.
//!
//! **Defaults are the contract.** A key with no live value (no location
//! set, the clock not set, RePlayOS off) reads its default; `since` reads
//! [`SINCE_NEVER`] and `was` the default. Previews, thumbnails and the
//! gallery render at the defaults ([`World::default`]), so they are
//! reproducible.

use crate::fixed::ONE;

/// A built-in key: its number in the psb, its name, range and default
/// (Q16.16), and whether it is a whole number (what it means: [`about`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub id: u8,
    pub name: &'static str,
    pub lo: i32,
    pub hi: i32,
    pub default: i32,
    pub whole: bool,
}

/// Key numbers (the psb's; never reused: a new key takes the next).
pub mod id {
    pub const DAYLIGHT: u8 = 1;
    pub const SUN_ALT: u8 = 2;
    pub const MOON_PHASE: u8 = 3;
    pub const SEASON: u8 = 4;
    pub const YEARDAY: u8 = 5;
    pub const NIGHT: u8 = 6;
    pub const BRIGHT: u8 = 7;
    pub const REPLAY_PLAYING: u8 = 8;
    pub const REPLAY_PLATFORM: u8 = 9;
    pub const REPLAY_GAME: u8 = 10;
}

/// Room for this many keys (numbers 1 to `MAX_KEYS − 1`; 0 is none): the
/// built-ins and room to grow.
pub const MAX_KEYS: usize = 16;

/// `since` of a key that never changed: 32,767 s (Q16.16's reach, ~9 h).
pub const SINCE_NEVER: i32 = 32767 << 16;
/// A real `since` is held at 32,766 s (language review A5), so 32,767
/// means "never changed" and nothing else, however long a change is past.
pub const SINCE_HELD: i32 = 32766 << 16;

/// What a read of a key is: its value, `since(KEY)` or `was(KEY)` (the
/// top two bits of an input's byte in the program, its key the low six).
pub const VALUE: u8 = 0;
pub const SINCE: u8 = 1;
pub const WAS: u8 = 2;

/// An input's byte: key `id`, read as `what`.
#[inline]
pub const fn input_byte(id: u8, what: u8) -> u8 {
    id | what << 6
}

/// The highest platform number `replay.platform` reads (the closed
/// platform list's places, 1 up; 0 none).
pub const MAX_PLATFORM: i32 = 127;

/// The built-in keys.
pub const KEYS: [Key; 10] = [
    Key {
        id: id::DAYLIGHT,
        name: "daylight",
        lo: 0,
        hi: ONE,
        default: ONE,
        whole: false,
    },
    Key {
        id: id::SUN_ALT,
        name: "sun.alt",
        lo: -ONE / 4,
        hi: ONE / 4,
        default: 6554,
        whole: false,
    },
    Key {
        id: id::MOON_PHASE,
        name: "moon.phase",
        lo: 0,
        hi: ONE,
        default: ONE / 2,
        whole: false,
    },
    Key {
        id: id::SEASON,
        name: "season",
        lo: 0,
        hi: ONE,
        default: ONE / 2,
        whole: false,
    },
    Key {
        id: id::YEARDAY,
        name: "yearday",
        lo: 0,
        hi: 365 << 16,
        default: 172 << 16,
        whole: true,
    },
    Key {
        id: id::NIGHT,
        name: "night",
        lo: 0,
        hi: ONE,
        default: 0,
        whole: true,
    },
    Key {
        id: id::BRIGHT,
        name: "bright",
        lo: 0,
        hi: ONE,
        default: ONE,
        whole: false,
    },
    Key {
        id: id::REPLAY_PLAYING,
        name: "replay.playing",
        lo: 0,
        hi: ONE,
        default: 0,
        whole: true,
    },
    Key {
        id: id::REPLAY_PLATFORM,
        name: "replay.platform",
        lo: 0,
        hi: MAX_PLATFORM << 16,
        default: 0,
        whole: true,
    },
    Key {
        id: id::REPLAY_GAME,
        name: "replay.game",
        lo: 0,
        hi: ONE,
        default: 0,
        whole: false,
    },
];

/// What each of [`KEYS`] means, in its order (the compiler's and the
/// tools': the player carries none of it).
#[cfg(feature = "compiler")]
pub const ABOUT: [&str; 10] = [
    "how light it is outside, 0 at night to 1 by day (civil twilight between, the sun 6° under the horizon to 6° over); 1 without a location",
    "the sun's height in turns: 0 on the horizon, 0.25 straight up, below 0 at night; 0.1 without a location",
    "the moon's phase in turns: 0 new, 0.5 full; 0.5 before the clock is set",
    "the time of year in turns for the panel's hemisphere: 0 midwinter, 0.5 midsummer; 0.5 without a location",
    "the day of the year, 0 on 1 January to 365; 172 (midsummer) before the clock is set",
    "1 while the panel's night mode has it dimmed, else 0",
    "the panel's brightness, 0 to 1 of its maximum",
    "1 while RePlayOS plays a game, else 0 (and 0 when it isn't followed)",
    "the playing game's platform as a number (its place in the panel's platform list, from 1); 0 when none",
    "a number for the playing game, 0 to 1: different for each game, the same each time it plays (a hue, a seed); 0 when none",
];

/// What key `id` means ("" for none).
#[cfg(feature = "compiler")]
pub fn about(id: u8) -> &'static str {
    KEYS.iter().position(|k| k.id == id).map_or("", |i| ABOUT[i])
}

/// The key named `name` (`sun.alt`).
pub fn key(name: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.name == name)
}

/// The key numbered `id`.
pub fn key_by_id(id: u8) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.id == id)
}

/// Whether `name` is a group of dotted keys (`sun`, `moon`, `replay`).
pub fn is_group(name: &str) -> bool {
    KEYS.iter()
        .any(|k| k.name.split_once('.').is_some_and(|(g, _)| g == name))
}

/// The range a read of key `id` as `what` holds to (Q16.16).
pub fn range(id: u8, what: u8) -> (i32, i32) {
    match (key_by_id(id), what) {
        (_, SINCE) => (0, SINCE_NEVER),
        (Some(k), _) => (k.lo, k.hi),
        (None, _) => (0, 0),
    }
}

/// What the built-in keys read for a frame: each key's value, its value
/// before its last change, and the seconds since that change (Q16.16; the
/// default's [`SINCE_NEVER`] when it never changed). Indexed by key
/// number. [`Default`] is every key's default: what previews show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct World {
    pub value: [i32; MAX_KEYS],
    pub was: [i32; MAX_KEYS],
    pub since: [i32; MAX_KEYS],
}

impl Default for World {
    fn default() -> Self {
        let mut w = World {
            value: [0; MAX_KEYS],
            was: [0; MAX_KEYS],
            since: [SINCE_NEVER; MAX_KEYS],
        };
        for k in &KEYS {
            w.value[k.id as usize] = k.default;
            w.was[k.id as usize] = k.default;
        }
        w
    }
}

impl World {
    /// The read of an input byte ([`input_byte`]), held to its range.
    #[inline]
    pub fn read(&self, byte: u8) -> i32 {
        let (id, what) = (byte & 63, byte >> 6);
        let i = id as usize % MAX_KEYS;
        let v = match what {
            // (A change's seconds are held at SINCE_HELD; only a key that
            // never changed reads SINCE_NEVER.)
            SINCE if self.since[i] == SINCE_NEVER => SINCE_NEVER,
            SINCE => self.since[i].min(SINCE_HELD),
            WAS => self.was[i],
            _ => self.value[i],
        };
        let (lo, hi) = range(id, what);
        v.clamp(lo, hi)
    }

    /// Set key `id` to `v` at once as a live change (the preview's "make it
    /// change"): `was` the value it had, `since` 0. The same value is no
    /// change.
    pub fn change(&mut self, id: u8, v: i32) {
        let i = id as usize % MAX_KEYS;
        if self.value[i] != v {
            self.was[i] = self.value[i];
            self.value[i] = v;
            self.since[i] = 0;
        }
    }
}
