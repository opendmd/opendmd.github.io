//! The declarations (docs/research/formula-apps.md §2.1, built in F1):
//! `psi 1`, `name "…"`, `about "…"`, `control …`, `look …` — what they
//! compile to, their errors (each with its stable code, listed in
//! docs/research/psi-tooling-plan.md §4), `compact`, the controls' ranges
//! (sound for any value a path gives) and the `look` line as the
//! composition `look::compose` makes.

use opendmd_psi::{
    compact, compile, compile_declared, compile_with, golden, look, render_rows_ink, Control,
    Ink, Program, Scratch, OWN,
};

const ONE: i32 = 1 << 16;

fn frame(p: &Program, ms: u32, abc: [i32; 3]) -> Vec<u8> {
    let mut i = golden::inputs(ms);
    (i.a, i.b, i.c) = (abc[0], abc[1], abc[2]);
    i.set_ms(p, u64::from(ms));
    let mut out = vec![0u8; 128 * 32 * 3];
    let mut sc = Box::new(Scratch::<128>::new());
    let mut ink = Box::new(Ink::new());
    render_rows_ink(p, &mut sc, Some(&mut ink), &i, &OWN, None, u32::MAX, &mut out);
    out
}

/// Every new error: the source, a piece of the sentence, line, column and
/// the code (docs/research/psi-tooling-plan.md §4).
const ERRORS: &[(&str, &str, usize, usize, &str)] = &[
    ("psi 2\ngray(x)", "it is written for Psi 2, this one has Psi 1", 1, 5, "E601"),
    ("psi 12\ngray(x)", "written for Psi 12", 1, 5, "E601"),
    ("psi 0\ngray(x)", "`psi` takes the language's version, a whole number: `psi 1`", 1, 1, "E603"),
    ("psi 1.5\ngray(x)", "a whole number", 1, 1, "E603"),
    ("psi\ngray(x)", "a whole number", 1, 1, "E603"),
    ("psi one\ngray(x)", "a whole number", 1, 1, "E603"),
    ("name \"x\"\npsi 1\ngray(x)", "`psi 1` goes on the formula's first line", 2, 1, "E604"),
    ("loop 4\npsi 1\ngray(x)", "`psi 1` goes on the formula's first line", 2, 1, "E604"),
    ("loop 4\nname \"Late\"\ngray(x)", "`name` goes at the top of the formula, before its other lines", 2, 1, "E951"),
    ("v = 1\ncontrol k = 0.5\ngray(v)", "`control` goes at the top", 2, 1, "E951"),
    ("loop 4\nabout \"x\"\ngray(x)", "`about` goes at the top", 2, 1, "E951"),
    ("loop 4\nlook screen 40\ngray(x)", "`look` goes at the top", 2, 1, "E951"),
    ("name \"a\"\nname \"b\"\ngray(x)", "`name` is already set on line 1", 2, 1, "E952"),
    ("look\nlook over\ngray(x)", "`look` is already set on line 1", 2, 1, "E952"),
    ("name \"Unclosed\ngray(x)", "text in quotes ends with `\"` on the same line", 1, 6, "E953"),
    ("name \"a<b>\"\ngray(x)", "text in quotes can't have `<`, `>`, `\\` or a control character (a tab too) in it", 1, 8, "E954"),
    ("name \"a\\b\"\ngray(x)", "can't have", 1, 8, "E954"),
    ("name \"a\tb\"\ngray(x)", "can't have", 1, 8, "E954"),
    ("control k = 1 \"0123456789012345678901234\"\ngray(k)", "this text can be 24 characters at most", 1, 15, "E955"),
    ("name \"0123456789012345678901234567890123456789012345678901234567890\"\ngray(x)", "60 characters at most", 1, 6, "E955"),
    ("name\ngray(x)", "this takes its text in quotes", 1, 5, "E956"),
    ("about 3\ngray(x)", "this takes its text in quotes", 1, 7, "E956"),
    ("control k 1 = 2\ngray(k)", "a control is `control NAME lo..hi = default", 1, 11, "E957"),
    ("control k 0..1 \"L\" = 0.5\ngray(k)", "a control is", 1, 20, "E957"),
    ("control k 0..1 = 0.5 \"L\" \"M\"\ngray(k)", "a control is", 1, 26, "E957"),
    ("control k = calm, 3\ngray(k)", "a choice is words (names) between commas", 1, 19, "E966"),
    ("control k = calm,\ngray(k)", "a choice is words", 1, 18, "E966"),
    ("name \"x\" \"y\"\ngray(x)", "expected the end of the `name` line, found `\"y\"` (one declaration a line)", 1, 10, "E967"),
    ("psi 1 2\ngray(x)", "expected the end of the `psi` line", 1, 7, "E967"),
    ("look screen 40 x\ngray(x)", "expected the end of the `look` line, found `x`", 1, 16, "E967"),
    ("name \"a\u{202e}b\"\ngray(x)", "characters that hide or reorder text", 1, 8, "E969"),
    ("name \"a\u{200b}b\"\ngray(x)", "characters that hide or reorder text", 1, 8, "E969"),
    ("about \"a\u{2028}b\"\ngray(x)", "characters that hide or reorder text", 1, 9, "E969"),
    ("name \"a\u{2066}b\"\ngray(x)", "characters that hide or reorder text", 1, 8, "E969"),
    ("name \"a\u{85}b\"\ngray(x)", "or a control character", 1, 8, "E954"),
    ("psi 01\ngray(x)", "a whole number", 1, 1, "E603"),
    ("psi 1.0\ngray(x)", "a whole number", 1, 1, "E603"),
    ("control k 0..1 = calm, wild\ngray(k)", "a control is `control NAME lo..hi = default", 1, 18, "E957"),
    ("control k = 0.5 x\ngray(k)", "a control is", 1, 17, "E957"),
    ("control k = \ngray(k)", "a control is", 1, 13, "E957"),
    ("control p = 0\ncontrol q = 0\ncontrol r = 0\ncontrol s = 0\ngray(p)", "three controls at most", 4, 1, "E958"),
    ("control k 4..1\ngray(k)", "a range goes from the lower number to the higher", 1, 11, "E959"),
    ("control k 1..1\ngray(k)", "a range goes from the lower", 1, 11, "E959"),
    ("control k 1..4 = 5\ngray(k)", "the default is outside the control's range", 1, 9, "E960"),
    ("control k = -1\ngray(k)", "the default is outside", 1, 9, "E960"),
    ("control k = calm\ngray(k)", "a choice has 2 to 6 words", 1, 13, "E961"),
    ("control k = a1, a2, a3, a4, a5, a6, a7\ngray(k)", "a choice has 2 to 6 words", 1, 13, "E961"),
    ("control k = calm, wild, calm\ngray(k)", "`calm` is a choice already", 1, 25, "E962"),
    ("control b = 0.5\ngray(b)", "`b` is control 2's letter", 1, 9, "E963"),
    ("control k = 0\ncontrol a = 0\ngray(k)", "`a` is control 1's letter", 2, 9, "E963"),
    ("look shiny\ngray(x)", "`look` takes a blend (over, screen, add, multiply, max or min) and an amount from 0 to 100", 1, 6, "E964"),
    ("look screen 140\ngray(x)", "`look` takes a blend", 1, 13, "E964"),
    ("look 40.5\ngray(x)", "`look` takes a blend", 1, 6, "E964"),
    ("look screen 40\nlayer { gray(x) }\nlayer add { gray(y) }", "a formula with layers goes over the GIF as a look", 1, 1, "E965"),
    ("control x = 0.5\ngray(x)", "`x` is built in", 1, 9, "E204"),
    ("control k = 0.5\nk = 2\ngray(k)", "`k` is already set on line 1", 2, 1, "E203"),
    ("gray(\"x\")", "there are no strings but on `name`, `about` and `control` lines", 1, 6, "E001"),
    ("loop 4 \"x\"\ngray(x)", "there are no strings", 1, 8, "E001"),
];

#[test]
fn every_declaration_error_says_what_and_where() {
    let mut bad = Vec::new();
    for &(src, want, line, col, code) in ERRORS {
        match compile(src) {
            Ok(_) => bad.push(format!("{code} {src:?}: compiled")),
            Err(e) => {
                let lc = e.line_col(src);
                if !e.msg.contains(want) || lc != (line, col) {
                    bad.push(format!(
                        "{code} {src:?}: got {:?} at {lc:?}, want {want:?} at {:?}\n{}",
                        e.msg,
                        (line, col),
                        e.render(src)
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

/// The new codes are all in the tooling plan's index.
#[test]
fn the_codes_are_in_the_tooling_plan() {
    let plan = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/psi-tooling-plan.md"
    ))
    .unwrap();
    for &(_, _, _, _, code) in ERRORS {
        assert!(plan.contains(code), "{code} is not in psi-tooling-plan.md §4");
    }
}

const FULL: &str = "psi 1
name \"Rotozoom -- the; one\"   -- a comment
about \"Mode 7:  a checker\"
control zoom 1..4 = 2 \"Zoom\"
control wobble = gentle, strong, off
control c = 0.25
look screen 40
loop 4
gray(zoom/4 + pick(wobble, 0, 0.25, 0.5) + c)
";

#[test]
fn what_the_declarations_compile_to() {
    let mut p = Program::default();
    let d = compile_declared(FULL, &mut p).unwrap();
    let m = p.meta();
    assert_eq!(
        m.control(0),
        Some(Control::Slider { lo: ONE, hi: 4 * ONE, default: 2 * ONE })
    );
    assert_eq!(m.control(1), Some(Control::Choice { n: 3 }));
    assert_eq!(
        m.control(2),
        Some(Control::Slider { lo: 0, hi: ONE, default: ONE / 4 })
    );
    assert_eq!(m.declared(), 7);
    assert_eq!(m.look(), Some(("screen", 40)));
    assert_eq!(m.values([None, Some(ONE), None]), [2 * ONE, ONE, ONE / 4]);
    let at = |r: (u32, u32)| &FULL[r.0 as usize..r.1 as usize];
    assert_eq!(at(d.name.unwrap()), "Rotozoom -- the; one");
    assert_eq!(at(d.about.unwrap()), "Mode 7:  a checker");
    let c0 = d.controls[0].unwrap();
    assert_eq!((at(c0.name), c0.label.map(at), c0.words), ("zoom", Some("Zoom"), None));
    let c1 = d.controls[1].unwrap();
    assert_eq!((at(c1.name), c1.words.map(at)), ("wobble", Some("gentle, strong, off")));
    assert_eq!(at(d.controls[2].unwrap().name), "c");
    // The program is the same as with the values written in.
    let lit = compile("loop 4\ngray(clamp(a, 1, 4)/4 + pick(floor(clamp(b, 0, 2)), 0, 0.25, 0.5) + clamp(c, 0, 1))").unwrap();
    for abc in [[2 * ONE, 0, ONE / 4], [ONE / 2, 7 * ONE, -ONE], [i32::MAX, i32::MIN, 3 * ONE]] {
        assert_eq!(frame(&p, 500, abc), frame(&lit, 500, abc), "{abc:?}");
    }
    // No declarations: nothing declared, the defaults 0.
    let q = compile("gray(a + b)").unwrap();
    assert_eq!(q.meta().declared(), 0);
    assert_eq!(q.meta().values([None; 3]), [0; 3]);
    assert_eq!(q.meta().look(), None);
}

#[test]
fn compact_keeps_the_declarations_as_written() {
    let c = compact(FULL);
    assert_eq!(
        c,
        "psi 1; name \"Rotozoom -- the; one\"; about \"Mode 7:  a checker\"; control zoom 1..4 = 2 \"Zoom\"; control wobble = gentle, strong, off; control c = 0.25; look screen 40; loop 4; gray(zoom/4 + pick(wobble, 0, 0.25, 0.5) + c)"
    );
    let (mut a, mut b) = (Program::default(), Program::default());
    let da = compile_declared(FULL, &mut a).unwrap();
    let db = compile_declared(&c, &mut b).unwrap();
    assert_eq!(a, b);
    let text = |s: &str, r: Option<(u32, u32)>| r.map(|r| s[r.0 as usize..r.1 as usize].to_string());
    assert_eq!(text(FULL, da.name), text(&c, db.name));
    assert_eq!(text(FULL, da.about), text(&c, db.about));
    // `'"'` is a character, on a line that starts with `name` but sets it
    // (review, formula-f1): the comment after it goes.
    let src = "name = '\"' -- a quote\ngray(name/100)";
    assert_eq!(compact(src), "name = '\"'; gray(name/100)");
    assert_eq!(compile(&compact(src)).unwrap(), compile(src).unwrap());
    // A declaration line written on one line with others.
    assert!(compile("name \"x; y\"; control k = 1; gray(k)").is_ok());
}

#[test]
fn the_words_stay_names_where_they_are_not_declarations() {
    // Values called `name`, `about`, `control`, `look` and `psi`, set and read.
    let p = compile("name = x\nabout = y\ncontrol = 0.5\nlook = 0.25\npsi = 1\nrgb(name, about*control, look*psi)").unwrap();
    assert_eq!(p.meta().declared(), 0);
    assert_eq!(p, compile("rgb(x, y*0.5, 0.25)").unwrap());
    // A part called `look` drawn by itself as the last line (a composed look).
    assert!(compile("part look {\n  gray(x)\n}\nlook").is_ok());
    assert!(compile("part look {\n  gray(x)\n}\nlook(x, y, 0.4)").is_ok());
    // `psi 1`: accepted, the same program.
    assert_eq!(compile("psi 1\ngray(x)").unwrap(), compile("gray(x)").unwrap());
}

/// The slot letters read as the control: `a` is the first control's value
/// (held to its range), in a part too.
#[test]
fn the_letters_read_as_the_controls() {
    let named = compile("control size 1..3 = 2\npart p {\n  gray(a/3)\n}\nlayer { gray(size/3) }\nlayer add { p(x, y, 0.5) }").unwrap();
    let lit = compile("part p {\n  gray(clamp(a, 1, 3)/3)\n}\nlayer { gray(clamp(a, 1, 3)/3) }\nlayer add { p(x, y, 0.5) }").unwrap();
    for v in [0, ONE, 2 * ONE, 9 * ONE] {
        assert_eq!(frame(&named, 0, [v, 0, 0]), frame(&lit, 0, [v, 0, 0]));
    }
}

/// A formula pasted into a part keeps its controls: their names read in the
/// part, and when the formula's own lines declare none the part's describe
/// the slots (a look composed as `part look { … }` keeps its defaults).
#[test]
fn a_parts_controls() {
    let inner = "control dim 0..1 = 0.4 \"Dim rows\"\ngray(py % 2 ? 1 : 0.3 + dim*0.6)";
    let composed = look::compose(inner, "screen", 40).unwrap();
    let p = compile(&composed).unwrap();
    assert_eq!(p.meta().control(0), compile(inner).unwrap().meta().control(0));
    assert_eq!(p.meta().declared(), 1);
    // The formula's own control wins; the part's names still read.
    let own = compile(&format!("control k = 0.9\npart q {{\n{inner}\n}}\nlayer {{ gray(k) }}\nlayer {{ q(x, y, 0.5) }}")).unwrap();
    assert_eq!(own.meta().default_value(0), compile("control k = 0.9\ngray(k)").unwrap().meta().default_value(0));
    // A part describes a slot the formula's own lines leave undeclared, even
    // when they declare others: its range and default reach `Meta`.
    let mixed = compile("control q 0.5..1 = 0.75\npart p {\n  control r 2..8 = 4\n  control s 0..100 = 50\n  gray(frac(s*x + r))\n}\nlayer { gray(q) }\nlayer add { p(x, y, 0.5) }").unwrap();
    assert_eq!(mixed.meta().declared(), 3);
    assert_eq!(mixed.meta().control(0), Some(Control::Slider { lo: ONE / 2, hi: ONE, default: 3 * ONE / 4 }));
    assert_eq!(mixed.meta().control(1), Some(Control::Slider { lo: 0, hi: 100 * ONE, default: 50 * ONE }));
    // A part's declarations other than control do nothing.
    let q = compile("part p {\n  psi 1\n  name \"P\"\n  look screen 40\n  gray(x)\n}\np").unwrap();
    assert_eq!(q.meta().look(), None);
}

/// The narrowed steps (`range`) assume each declared control is within its
/// range — true for any value a path gives, since the formula reads it only
/// through the clamp its line made. Narrowed and checked frames agree for
/// values far outside every range.
#[test]
fn control_ranges_are_sound_for_any_value() {
    let forms = [
        "control z 1..4 = 2\nloop 4\nv = x*z*37.25 + z*z*900\ngray(frac(v/97) + (z*8000)*(px*2))",
        "control z -2..2 = 0\ncontrol s 0..100 = 50\nloop 4\nrgb(frac(z*px*s*3.5), frac((s*s*s)/7 + py*z), sin(z*x*s))",
        "control m = a1, b1, c1, d1\ncontrol wd 0..0.01 = 0.005\nloop 4\ngray(frac(pick(m, x, y, px*wd*1000, py) + m*px*py*wd*65.5))",
        "control big 0..30000 = 1000\nloop 4\nk = big*big\nrgb(frac(k*x/1000), frac(big*px*py/3), frac(big*7.5 + x))",
        // (The review's, formula-f1: loop forever; sprites and palettes; the
        // formula reading `a` before a part declares it, both orders; a
        // formula with fewer controls than its part.)
        "control z 1..4 = 2\ncontrol s 0..100 = 50\nloop forever\nv = frac(t*z/7 + seed)\ngray(frac(v*s*px + noise(x*z, y*s, t)))",
        "control m = red, green, blue\ncontrol k 0..10 = 3\npalette p = #f00, #0f0, #00f\nsprite s {\n  key . 0, X 1\n  .X.\n  X.X\n}\nloop 4\nv = s[floor(x*3*k), floor(y*2)]\nlayer { pal(p, frac(m/2 + v*k)) }\nlayer add { gray(frac(k*x*37 + m*px)) }",
        "loop 4\nk = a*a*900\npart p {\n  control r 2..8 = 4\n  gray(frac(r*x*py*a))\n}\nlayer { gray(frac(k*x)) }\nlayer add { p(x, y, 0.5) }",
        "loop 4\npart p {\n  control r 2..8 = 4\n  gray(frac(r*x*py))\n}\nk = a*a*900\nlayer { gray(frac(k*x)) }\nlayer add { p(x, y, 0.5) }",
        "control q 0.5..1 = 0.75\nloop 4\npart p {\n  control r 2..8 = 4\n  control s 0..100 = 50\n  gray(frac(s*x*py*31 + r))\n}\nlayer { gray(frac(q*px*py*31 + b*c)) }\nlayer add { p(x, y, 0.5) }",
        "control q 0.5..1 = 0.75\ncontrol k = on, off\npart p {\n  control r 2..8 = 4\n  gray(frac(r*x*py*a))\n}\nlayer { gray(frac(q*px*py*31 + k)) }\nlayer add { p(x, y, 0.5) }",
    ];
    let vals = [
        i32::MIN,
        -32767 * ONE,
        -5 * ONE,
        -1,
        0,
        ONE / 3,
        ONE,
        3 * ONE + 7,
        250 * ONE,
        32767 * ONE,
        i32::MAX,
    ];
    let mut seed = 0x1234_5678u32;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for src in forms {
        let narrowed = compile(src).unwrap();
        let mut checked = Program::default();
        compile_with(src, &mut checked, true, false).unwrap();
        for _ in 0..40 {
            let abc = [0, 1, 2].map(|_| {
                let r = rnd();
                if r & 1 == 0 {
                    vals[(r >> 1) as usize % vals.len()]
                } else {
                    (r >> 1) as i32 - (1 << 30)
                }
            });
            let ms = rnd() % 20000;
            assert_eq!(
                frame(&narrowed, ms, abc),
                frame(&checked, ms, abc),
                "{src:?} at {abc:?}"
            );
        }
    }
}

/// `look [blend] [amount]` as the composition: the program the look line
/// asks for is the one `look::compose` gives for the formula without it.
#[test]
fn the_look_line_is_the_composition() {
    let bodies = [
        golden::OVER[2].1,
        "control k 0..2 = 1 \"K\"\nloop 4.8\nhsv(x + t/4.8, 1, k/2)",
        "control dim = 0.4\nlayer multiply { gray(py % 2 ? 1 : 0.3 + dim*0.6) }",
        opendmd_psi::sketches::PLASMA,
    ];
    for body in bodies {
        let layered = compile(body).unwrap().layers() > 0;
        for blend in look::BLENDS {
            for amount in [0u8, 40, 100] {
                let src = format!("look {blend} {amount}\n{body}");
                let p = match compile(&src) {
                    Ok(p) => p,
                    Err(e) => {
                        assert!(layered && blend != "over", "{src:?}: {}", e.msg);
                        assert!(e.msg.contains("with layers goes over"));
                        continue;
                    }
                };
                let want = look::compose(body, blend, amount).unwrap();
                match look::declared(&src, &p) {
                    None => {
                        // `look` (over, 100) is the formula itself: played
                        // as itself, the line changes nothing drawn.
                        assert!(blend == "over" && amount == 100);
                        assert_eq!(want, body);
                        let q = compile(body).unwrap();
                        assert_eq!(frame(&p, 700, [ONE / 2; 3]), frame(&q, 700, [ONE / 2; 3]));
                    }
                    Some(text) => {
                        let text = text.unwrap();
                        assert_eq!(text, want, "{src:?}");
                        assert_eq!(compile(&text).unwrap(), compile(&want).unwrap());
                    }
                }
            }
        }
    }
}
