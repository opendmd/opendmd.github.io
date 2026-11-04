//! `psi fmt` (`opendmd_psi::syntax::format`) never changes what a formula
//! is: the same tokens in the same order, so the same program (every
//! example, experiment and doc block, the gallery's contributions when
//! `PSI_GALLERY` names their folder, and random formulas written every
//! which way); the comments kept word for word; and formatting twice is
//! formatting once.

use opendmd_psi::syntax::{format, tokens};
use opendmd_psi::{compact, compile, compile_declared, squeeze, Program};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn psi_files(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = rd.map(|e| e.unwrap().path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            psi_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "psi") {
            out.push((
                p.display().to_string(),
                std::fs::read_to_string(&p).unwrap(),
            ));
        }
    }
}

/// Every formula there is to hand.
fn corpus() -> Vec<(String, String)> {
    let r = root();
    let mut out = Vec::new();
    for d in [
        "formulas",
        "../opendmd-web/formulas",
        "../opendmd-web/experiments",
    ] {
        psi_files(&r.join(d), &mut out);
    }
    if let Some(g) = std::env::var_os("PSI_GALLERY") {
        psi_files(Path::new(&g), &mut out);
    }
    // The docs' ```psi blocks (errors too: those must keep their error).
    for doc in [
        "../docs/psi-for-models.md",
        "../docs/guide/formulas.md",
        "../docs/psi-spec.md",
    ] {
        let Ok(text) = std::fs::read_to_string(r.join(doc)) else {
            continue;
        };
        let mut cur: Option<(usize, String)> = None;
        for (k, line) in text.lines().enumerate() {
            match &mut cur {
                None if line.starts_with("```psi") => cur = Some((k + 1, String::new())),
                None => {}
                Some((at, s)) => {
                    if line.starts_with("```") {
                        out.push((format!("{doc}:{at}"), std::mem::take(s)));
                        cur = None;
                    } else {
                        s.push_str(line);
                        s.push('\n');
                    }
                }
            }
        }
    }
    assert!(out.len() > 100, "{} formulas", out.len());
    out
}

/// The tokens as the compiler reads them: kind and text; a line end just
/// inside a `{ }` left out (the formatter opens a block out onto lines of
/// its own, and the compiler skips those).
fn toks(src: &str) -> Vec<String> {
    let t: Vec<(String, &str)> = tokens(src)
        .unwrap()
        .into_iter()
        .map(|(k, a, b)| (format!("{k:?}"), &src[a..b]))
        .collect();
    (0..t.len())
        .filter(|&k| {
            !(t[k].0 == "End"
                && ((k > 0 && t[k - 1].1 == "{") || t.get(k + 1).is_some_and(|n| n.1 == "}")))
        })
        .map(|k| format!("{} {}", t[k].0, t[k].1))
        .collect()
}

/// The comments, as written (trailing spaces aside).
fn comments(src: &str) -> Vec<String> {
    let mut all = Vec::new();
    let b = src.as_bytes();
    let spans: Vec<(usize, usize)> = tokens(src).unwrap().iter().map(|t| (t.1, t.2)).collect();
    let mut from = 0;
    for (a, e) in spans.iter().copied().chain([(src.len(), src.len())]) {
        if a < from {
            continue;
        }
        let gap = &src[from..a];
        let mut j = 0;
        while j < gap.len() {
            if matches!(b[from + j], b'-' | b'/') && gap.as_bytes().get(j + 1) == Some(&b[from + j])
            {
                let end = gap[j..].find('\n').map_or(gap.len(), |k| j + k);
                all.push(gap[j..end].trim_end().to_string());
                j = end;
            } else {
                j += 1;
            }
        }
        from = e.max(from);
    }
    all
}

fn program(src: &str) -> Result<Box<Program>, String> {
    compile(src).map(Box::new).map_err(|e| e.msg)
}

fn name(src: &str) -> Option<String> {
    compile_declared(src, &mut Box::<Program>::default())
        .ok()
        .and_then(|d| d.name.map(|(a, b)| src[a as usize..b as usize].to_string()))
}

/// Everything `fmt` must keep, for `src`.
fn holds(what: &str, src: &str) {
    holds_as(what, src, true)
}

/// [`holds`]; `same_error`: a formula that doesn't compile fails with the
/// same sentence (else only fails: a cut-off formula's unclosed bracket is
/// told by whether the end is on its line, and the formatter ends the
/// text with a line end).
fn holds_as(what: &str, src: &str, same_error: bool) {
    let Ok(out) = format(src) else {
        assert!(tokens(src).is_err(), "{what}: format refused what lexes");
        return;
    };
    assert_eq!(toks(&out), toks(src), "{what}: the tokens changed:\n{out}");
    let (a, b) = (program(src), program(&out));
    match (&a, &b) {
        (Ok(a), Ok(b)) => assert!(a == b, "{what}: another program:\n{out}"),
        _ if same_error => assert_eq!(
            a.as_ref().err(),
            b.as_ref().err(),
            "{what}: another error:\n{out}"
        ),
        _ => assert!(
            a.is_err() && b.is_err(),
            "{what}: compiles one way only:\n{out}"
        ),
    }
    assert_eq!(name(&out), name(src), "{what}");
    assert_eq!(
        comments(&out),
        comments(src),
        "{what}: the comments changed:\n{out}"
    );
    let again = format(&out).unwrap();
    assert_eq!(
        again, out,
        "{what}: formatting twice is not formatting once"
    );
    assert!(out.ends_with('\n') || out.is_empty(), "{what}");
    for l in out.lines() {
        assert_eq!(l, l.trim_end(), "{what}: a line ends in spaces:\n{out}");
    }
    assert!(!out.contains("\n\n\n"), "{what}: two blank lines:\n{out}");
}

#[test]
fn every_formula_formats_to_the_same_program() {
    let all = corpus();
    for (what, src) in &all {
        holds(what, src);
        // The ways a formula travels: on one line, and squeezed. (An error
        // there need only stay an error: on one line, an unclosed bracket
        // reads on into the next statement, as the reference's E102 shows.)
        let ok = compile(src).is_ok();
        holds_as(&format!("{what} (compact)"), &compact(src), ok);
        holds_as(&format!("{what} (squeezed)"), &squeeze(src), ok);
    }
    eprintln!("{} formulas", all.len());
}

/// Every prefix of every formula (most don't compile; some don't lex):
/// never a panic, the same tokens, once is twice.
#[test]
fn every_prefix_formats_or_is_refused() {
    for (what, src) in corpus().iter().take(60) {
        let cut: Vec<usize> = (0..=src.len())
            .filter(|&k| src.is_char_boundary(k))
            .step_by(7)
            .collect();
        for k in cut {
            holds_as(&format!("{what} [..{k}]"), &src[..k], false);
        }
    }
}

/// The house style, as the examples are written.
#[test]
fn the_layout() {
    let cases: &[(&str, &str)] = &[
        ("v=sin(x*3+t/8)\nhsv(v,1,1)", "v = sin(x*3 + t/8)\nhsv(v, 1, 1)\n"),
        ("gray( frac(t/8)*96 )", "gray(frac(t/8)*96)\n"),
        ("c = pal(ice,v*0.8+y)*(0.35+v*0.5)\nc", "c = pal(ice, v*0.8 + y) * (0.35 + v*0.5)\nc\n"),
        ("a = 1; b = 2;gray(a)", "a = 1\nb = 2\ngray(a)\n"),
        ("v = a - b * -19\ngray(v)", "v = a - b*-19\ngray(v)\n"),
        ("v = - -x\nw = 1 - -x\ngray(v)", "v = - -x\nw = 1 - -x\ngray(v)\n"),
        ("control k -1..1 = -0.5 \"K\"\ngray(k)", "control k -1..1 = -0.5 \"K\"\ngray(k)\n"),
        ("control m = calm,wild\ngray(m)", "control m = calm, wild\ngray(m)\n"),
        ("layer add {\n\n      v=x   -- a note\n\n\n   gray(v)\n\n}", "layer add {\n  v = x -- a note\n\n  gray(v)\n}\n"),
        ("h = hour%12; m = minute\nlayer { gray(h) }", "h = hour%12\nm = minute\nlayer {\n  gray(h)\n}\n"),
        ("q = px -- the column\nkk = floor(q/32)    -- which digit\n\nn = 1  -- one\ngray(n)", "q = px           -- the column\nkk = floor(q/32) -- which digit\n\nn = 1 -- one\ngray(n)\n"),
        ("-- top\n\n\nloop 4\n  -- own line\ngray(x)\n\n", "-- top\n\nloop 4\n-- own line\ngray(x)\n"),
        ("v = not x < 0.5 and !y\ngray(v)", "v = not x < 0.5 and !y\ngray(v)\n"),
        ("v = c ? a+1 : b*2\ngray(v)", "v = c ? a + 1 : b*2\ngray(v)\n"),
        ("v = c ? a+1 : f(b + 1)*2\ngray(v)", "v = c ? a + 1 : f(b + 1) * 2\ngray(v)\n"),
        ("v = pick(k,\n#f00,\n  #0f0)\nv", "v = pick(k,\n  #f00,\n  #0f0)\nv\n"),
        ("v = 1 +\n2\ngray(v)", "v = 1 +\n  2\ngray(v)\n"),
        ("table t = 1, -2,\n3\ngray(t[0])", "table t = 1, -2,\n  3\ngray(t[0])\n"),
        ("part p {\nloop 2\ngray( x )\n}\np(x , y).v", "part p {\n  loop 2\n  gray(x)\n}\np(x, y).v\n"),
        ("sprite s {   -- a sprite\n  key . 0, X 1\n.X.\n    X.X ; .X.\n}\ngray(s[px, py])", "sprite s { -- a sprite\n  key . 0, X 1\n  .X.\n  X.X\n  .X.\n}\ngray(s[px, py])\n"),
        ("draw {\nfor k < 4 {\ndot(k,k)\n}\n}\ngray(ink)", "draw {\n  for k < 4 {\n    dot(k, k)\n  }\n}\ngray(ink)\n"),
        ("g = glyph('A',px%6,py)\ngray(g)", "g = glyph('A', px%6, py)\ngray(g)\n"),
        ("name \"A  ;  b -- c\"  -- d\ngray(x)", "name \"A  ;  b -- c\" -- d\ngray(x)\n"),
    ];
    for (src, want) in cases {
        let got = format(src).unwrap();
        assert_eq!(&got, want, "for {src:?}");
        holds(src, src);
    }
}

// --- random formulas, written every which way -------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Spaces, or a line break where the language allows one inside brackets,
/// or a comment and a line break.
fn ws(r: &mut Rng, inside: bool) -> String {
    match r.below(12) {
        0..=4 => String::new(),
        5..=8 => " ".repeat(1 + r.below(3)),
        9 if inside => "\n   ".into(),
        10 if inside => " -- note\n".into(),
        11 if inside => ";".into(),
        _ => " ".into(),
    }
}

/// An expression of depth ≤ `d`, brackets only where needed or at random.
fn expr(r: &mut Rng, d: u32, inside: bool) -> String {
    const LEAVES: &[&str] = &[
        "x", "y", "px", "t", "a", "0.5", "3", ".25", "w", "cx", "r", "'A'",
    ];
    if d == 0 || r.below(4) == 0 {
        return LEAVES[r.below(LEAVES.len())].into();
    }
    let s = |r: &mut Rng| ws(r, inside);
    match r.below(9) {
        0..=3 => {
            let op = [
                "+", "-", "*", "/", "%", "<", ">=", "==", "!=", "and", "or", "&&",
            ][r.below(12)];
            let (a, b) = (expr(r, d - 1, inside), expr(r, d - 1, inside));
            let (l, rr) = (s(r), s(r));
            // Words need a space; `- -` and `/ /` would be a comment.
            let gap = |g: String| {
                if g.is_empty() && (op.len() > 1 || op == "-" || op == "/") {
                    " ".into()
                } else {
                    g
                }
            };
            if r.below(2) == 0 {
                format!("({a}{}{op}{}{b})", gap(l), gap(rr))
            } else {
                format!("({}{a}{}{op}{}{b}{})", s(r), gap(l), gap(rr), s(r))
            }
        }
        4 => format!("-{}", expr(r, d - 1, inside).replace("--", "- -")),
        5 => format!(
            "({} ? {} : {})",
            expr(r, d - 1, true),
            expr(r, d - 1, true),
            expr(r, d - 1, true)
        ),
        6 => format!("sin({}{})", s(r), expr(r, d - 1, true)),
        7 => format!(
            "min({},{}{})",
            expr(r, d - 1, true),
            s(r),
            expr(r, d - 1, true)
        ),
        _ => format!(
            "mix({}, {}, {})",
            expr(r, d - 1, true),
            expr(r, d - 1, true),
            expr(r, d - 1, true)
        ),
    }
}

#[test]
fn random_formulas_format_to_the_same_program() {
    let mut r = Rng(0x5eed_f0f0_1234_abcd);
    let mut compiled = 0;
    for k in 0..800 {
        let mut src = String::new();
        if r.below(3) == 0 {
            src.push_str("-- a random one\n");
        }
        src.push_str(&format!(
            "loop {}{}",
            1 + k % 9,
            if r.below(2) == 0 { "; " } else { "\n" }
        ));
        let n = 1 + r.below(3);
        for j in 0..n {
            let e = expr(&mut r, 4, false);
            src.push_str(&format!(
                "{}v{j}{}={}{e}",
                " ".repeat(r.below(3)),
                ws(&mut r, false),
                ws(&mut r, false)
            ));
            src.push_str(if r.below(4) == 0 {
                "   -- after\n"
            } else if r.below(3) == 0 {
                ";"
            } else {
                "\n"
            });
            if r.below(5) == 0 {
                src.push('\n');
            }
        }
        if r.below(2) == 0 {
            src.push_str(&format!(
                "layer add {{\n   u = v0*{}\n gray(u)\n}}\n",
                expr(&mut r, 2, false)
            ));
        } else {
            src.push_str(&format!("hsv(v0,1,{})\n", expr(&mut r, 2, true)));
        }
        if compile(&src).is_ok() {
            compiled += 1;
        }
        holds(&format!("random {k}:\n{src}"), &src);
    }
    assert!(compiled > 400, "only {compiled} of 800 compiled");
}
