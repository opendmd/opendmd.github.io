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

const SRC: &str = "name \"Spin\"\ncontrol speed 0..4 = 1\nloop 4\nk = sin(t/4*speed)\nv = x*k + y\nfn p(a) = a*0.5\nlayer { gray(v) }\nlayer add { gray(p(x)) }";

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
    assert_eq!(def("p").2, Defined::Function);
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
    assert!(seen
        .ops
        .iter()
        .any(|o| o.1 == Level::Frame && at(o.0).contains("sin")));
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
    assert!(!kinds
        .iter()
        .any(|k| k.1.contains('c') && k.0 == Kind::Name && k.1 == "c"));
}

/// A value's place, the place of the value that makes it vary, the input.
type Why = ((usize, usize), Option<(usize, usize)>, String);

/// What `why` and `note` tell, collected.
#[derive(Default)]
struct Notes {
    defined: Vec<(String, (usize, usize), Level)>,
    why: Vec<Why>,
    notes: Vec<((usize, usize), String)>,
}

impl Sink for Notes {
    fn defined(&mut self, name: &str, at: (usize, usize), _: Defined, _: u16, level: Level) {
        self.defined.push((name.into(), at, level));
    }
    fn why(&mut self, at: (usize, usize), via: Option<(usize, usize)>, input: &str) {
        self.why.push((at, via, input.into()));
    }
    fn note(&mut self, at: (usize, usize), text: &str) {
        self.notes.push((at, text.into()));
    }
}

fn notes(src: &str) -> Notes {
    let mut n = Notes::default();
    let mut p = Program::default();
    compile_observed(src, &mut p, &mut n).unwrap_or_else(|e| panic!("{}", e.render(src)));
    assert_eq!(
        p,
        compile(src).unwrap(),
        "a sink changed the program of {src:?}"
    );
    n
}

/// `why`: the nearest value of the formula's own that makes a value vary
/// as it does (the chain `psi check --levels` prints), else the built-in.
#[test]
fn why_names_the_chain_to_the_input() {
    let src = "off = px - 64\nbeam = abs(off)*2\nk = py/31\ngray(beam/64 + k)";
    let n = notes(src);
    let why = |name: &str| {
        let d = n.defined.iter().find(|d| d.0 == name).unwrap();
        let w = n.why.iter().find(|w| w.0 == d.1).unwrap();
        (w.1.map(|v| &src[v.0..v.1]), w.2.clone(), d.2)
    };
    assert_eq!(why("off"), (None, "px".into(), Level::Pixel));
    assert_eq!(why("beam"), (Some("off"), "px".into(), Level::Pixel));
    assert_eq!(why("k"), (None, "py".into(), Level::Row));
}

/// The notes: each says where, and none for what a run already works out
/// once (a pick or a sum in the column run).
#[test]
fn notes_say_where_a_formula_costs_more() {
    // A per-pixel arm makes the whole pick per pixel.
    let n = notes("k = px/8 + py\nv = pick(k, t, sin(t), y*x)\ngray(v)");
    assert!(
        n.notes
            .iter()
            .any(|x| x.1.starts_with("arm 3 of this `pick` (`y*x`) is per pixel")),
        "{:?}",
        n.notes
    );
    // ... but not when the pick is in the column run (k and x by column).
    let n = notes("k = px/8\nv = pick(k, t, sin(t), x)\ngray(v)");
    assert!(
        !n.notes.iter().any(|x| x.1.contains("of this `pick`")),
        "{:?}",
        n.notes
    );
    assert!(
        n.notes
            .iter()
            .any(|x| x.1.starts_with("column run: `k`, `v`")),
        "{:?}",
        n.notes
    );
    // A column run left out (here: its ten values and the sum's would
    // need more registers than the panel has) is said, with why.
    let mut src = String::from("loop 8\n");
    for k in 0..10 {
        src += &format!("c{k} = hash(floor(px/{}), {k}) * sin(t/8)\n", k + 2);
    }
    src += "v = (y < c0) + (y < c1)*2 + (y < c2)*3 + (y < c3)*4 + (y < c4)*5 + (y < c5)*6 + (y < c6)*7 + (y < c7)*8 + (y < c8)*9 + (y < c9)*10\ngray(v/80)";
    let n = notes(&src);
    assert!(
        n.notes.iter().any(|x| x.1.starts_with(
            "column run: left out (it holds more values at once than the registers)"
        )),
        "{:?}",
        n.notes
    );
    // A sum worked out per pixel that brackets better.
    let n = notes("X = sin(t)*20\nd = px*py - 64 - X\ngray(d/64)");
    assert!(
        n.notes.iter().any(|x| x.1.starts_with(
            "`px*py - 64 - X` works out `- X` for every pixel; bracketed, `px*py - (64 + X)`"
        )),
        "{:?}",
        n.notes
    );
    // What the still cache kept, and what a moving value kept out.
    let n = notes("d = sqrt(cx*cx + cy*cy)\nq = d*a + y\ngray(frac(q + t))");
    assert!(
        n.notes
            .iter()
            .any(|x| x.1.contains("it reads `a`, which changes")),
        "{:?}",
        n.notes
    );
    let src = "near = 8/max(sqrt(cx*cx + cy*cy), 0.1)\ngray(frac(near + t))";
    let n = notes(src);
    assert!(
        n.notes
            .iter()
            .any(|x| x.1.starts_with("still cache: `near`") && &src[x.0 .0..x.0 .1] == "near"),
        "{:?}",
        n.notes
    );
    // A draw loop of 32 turns or more runs as lanes.
    let n = notes("draw { for k < 48 { dot(hash(k, 1)*128, hash(k, 2)*32) } }\ngray(ink)");
    assert!(
        n.notes
            .iter()
            .any(|x| x.1.starts_with("this loop of 48 turns runs as lanes")),
        "{:?}",
        n.notes
    );
}

/// A sink changes no program: every bundled Formula and experiment.
#[test]
fn a_sink_changes_no_program() {
    for dir in ["../opendmd-web/formulas", "../opendmd-web/experiments"] {
        let mut stack = vec![std::path::PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "psi") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    let _ = notes(&src);
                }
            }
        }
    }
}
