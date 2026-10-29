//! The language service's hook (`syntax`): the tokens, and what the
//! compiler tells a `Sink` — definitions, reads and their definitions,
//! ops with their costs, declarations — on a formula that compiles and up
//! to an error on one that doesn't.

use opendmd_psi::syntax::{compile_observed, tokens, Defined, Kind, Level, Sink};
use opendmd_psi::{compile, Program};

type Def = (String, (usize, usize), Defined, u16, Level);
type Read = ((usize, usize), Option<(usize, usize)>);

#[derive(Default)]
struct Seen {
    defined: Vec<Def>,
    reads: Vec<Read>,
    ops: Vec<((usize, usize), Level, u32)>,
    decls: Vec<String>,
}

impl Sink for Seen {
    fn defined(&mut self, name: &str, at: (usize, usize), what: Defined, scope: u16, level: Level) {
        self.defined.push((name.into(), at, what, scope, level));
    }
    fn read(&mut self, at: (usize, usize), defined: Option<(usize, usize)>) {
        self.reads.push((at, defined));
    }
    fn op(&mut self, at: (usize, usize), level: Level, _code: u8, cost_ns: u32) {
        self.ops.push((at, level, cost_ns));
    }
    fn declaration(&mut self, word: &str, _at: (usize, usize)) {
        self.decls.push(word.into());
    }
}

const SRC: &str = "name \"Spin\"
control speed 0..4 = 1
loop 4
frame { k = sin(t/4*speed) }
v = x*k + y
part p {
  gray(x)
}
layer { gray(v) }
layer add { p(x, y, 0.5) }";

#[test]
fn the_sink_hears_what_the_compiler_saw() {
    let mut seen = Seen::default();
    let mut p = Program::default();
    compile_observed(SRC, &mut p, &mut seen).unwrap();
    // The same program as without a sink.
    assert_eq!(p, compile(SRC).unwrap());
    let at = |r: (usize, usize)| &SRC[r.0..r.1];
    let def = |n: &str| seen.defined.iter().find(|d| d.0 == n).unwrap().clone();
    assert_eq!(at(def("speed").1), "speed");
    assert_eq!(def("k").4, Level::Frame);
    assert_eq!(def("v").4, Level::Pixel);
    assert_eq!(def("p").2, Defined::Part);
    assert_eq!(def("v").3, 0);
    // Every read of a defined name points at its definition.
    for (r, d) in &seen.reads {
        if let Some(d) = d {
            assert_eq!(at(*r), at(*d), "read at {r:?}");
        }
    }
    let v_reads = seen.reads.iter().filter(|(r, _)| at(*r) == "v").count();
    assert_eq!(v_reads, 1);
    assert!(seen.reads.iter().any(|(r, d)| at(*r) == "x" && d.is_none()));
    // Ops: per pixel ones, each with a cost.
    assert!(seen.ops.iter().any(|o| o.1 == Level::Pixel && o.2 > 0));
    assert!(seen.ops.iter().any(|o| o.1 == Level::Frame && at(o.0).contains("sin")));
    assert_eq!(seen.decls, ["name", "control"]);
}

#[test]
fn up_to_an_error() {
    let src = "a1 = x\nb1 = a1*2\ngray(b1 + nope)";
    let mut seen = Seen::default();
    let mut p = Program::default();
    assert!(compile_observed(src, &mut p, &mut seen).is_err());
    let names: Vec<&str> = seen.defined.iter().map(|d| d.0.as_str()).collect();
    assert_eq!(names, ["a1", "b1"]);
}

#[test]
fn tokens_say_what_each_is() {
    let src = "name \"A b\"\ncontrol k 1..4 = 2\ngray(#f80 * k) -- c";
    let t = tokens(src).unwrap();
    let kinds: Vec<(Kind, &str)> = t.iter().map(|&(k, a, e)| (k, &src[a..e])).collect();
    assert!(kinds.contains(&(Kind::Text, "\"A b\"")));
    assert!(kinds.contains(&(Kind::Punctuation, "..")));
    assert!(kinds.contains(&(Kind::Colour, "#f80")));
    assert!(kinds.contains(&(Kind::Operator, "*")));
    assert!(!kinds.iter().any(|k| k.1.contains('c') && k.0 == Kind::Name && k.1 == "c"));
}
