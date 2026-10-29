//! `squeeze` (what the firmware keeps of its built-in bundled Formulas,
//! build.rs) compiles every example and Psi formula to the very same
//! program, with the same name, and is shorter.

use opendmd_psi::{compile, compile_declared, squeeze};

fn sources() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for dir in [root.join("formulas"), root.join("../opendmd-web/formulas")] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "psi") {
                out.push((p.display().to_string(), std::fs::read_to_string(&p).unwrap()));
            }
        }
    }
    assert!(out.len() > 30, "{} formulas", out.len());
    out
}

#[test]
fn squeezed_is_the_same_program() {
    let listing = |s: &str| compile(s).map(|p| p.listing()).map_err(|e| e.msg);
    let name = |s: &str| compile_declared(s, &mut Box::<opendmd_psi::Program>::default()).ok().and_then(|d| d.name.map(|(a, b)| s[a as usize..b as usize].to_string()));
    let (mut before, mut after) = (0, 0);
    for (path, text) in sources() {
        let sq = squeeze(&text);
        assert_eq!(listing(&sq), listing(&text), "{path}:\n{sq}");
        assert_eq!(name(&sq), name(&text), "{path}");
        assert!(!sq.contains("--") || sq.lines().any(|l| l.contains('"')), "{path}: a comment kept:\n{sq}");
        before += text.len();
        after += sq.len();
    }
    eprintln!("squeezed: {before} -> {after} B");
    assert!(after * 3 < before * 2);
}

#[test]
fn squeeze_keeps_what_spaces_mean() {
    assert_eq!(squeeze("  x = a * -3   -- a comment\n\n-- only a comment\ny = x - -1\n"), "x=a*-3\ny=x- -1\n");
    assert_eq!(squeeze("a = b / /c\n"), "a=b/ /c\n");
    assert_eq!(squeeze("name \"A  -- b\"  -- c\nk = 'x' -- d\n"), "name \"A  -- b\"\nk='x'\n");
    assert_eq!(squeeze("v = noise(u, 0, 0) > .45 or d > 11 ? -99 : 1\n"), "v=noise(u,0,0)>.45 or d>11?-99:1\n");
}
