//! The language reference's own material (`opendmd_psi::reference`,
//! feature `docs`) and the reference page's recipes (docs/psi-spec.md,
//! the site's /dev/psi/), against the compiler: every cheat-sheet row has a
//! live example and every example compiles and lights the panel; every
//! error's `before` fails with its code and its `after` compiles; every
//! ```psi block of the recipes compiles and lights the panel, and a block
//! marked `<!-- psi-gallery: <stem> -->` is that gallery Formula, word for
//! word (`PSI_BLESS=1 cargo test --features docs --test reference`
//! rewrites those blocks). The site draws every ```psi block of its Psi
//! pages (tools/site.py PSI_PAGES, psiref.js): each one compiles and
//! lights something, and each ```psi-bad block (shown, never drawn) fails; every
//! ```psi block of the guide's Psi pages (Make a Formula, How Psi thinks)
//! is a whole, named Formula that lights the panel well.
#![cfg(not(target_arch = "wasm32"))]

use opendmd_psi::builtins::cheat;
use opendmd_psi::errors::{self, code_of};
use opendmd_psi::reference::{EXAMPLES, HELP, INTERNAL};
use opendmd_psi::{compile, render_rows_ink, Ink, Inputs, Program, Scratch, H, OWN, W};
use std::path::PathBuf;

fn root(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(rel)
}

/// The web app's sample picture (js/psikit.js `sample`, as psi-check's):
/// what the reference page draws a formula that reads the picture over.
fn picture() -> Vec<u8> {
    let mut p = vec![0u8; W * H * 3];
    for i in 0..W * H {
        let (x, y) = ((i % W) as f64, (i / W) as f64);
        let (xi, yi) = (i % W, i / W);
        let c = if y > 23.0 - 4.0 * (x / 13.0).sin() - 2.0 * (x / 5.0).sin() {
            [30, (110 + (xi * 7 + yi * 13) % 50) as u8, 40]
        } else if (x - 98.0).powi(2) + 1.5 * (y - 10.0).powi(2) < 50.0 {
            [255, 215, 80]
        } else {
            [
                (40 + yi * 6) as u8,
                (60 + yi * 4) as u8,
                (170 - yi * 3) as u8,
            ]
        };
        p[i * 3..i * 3 + 3].copy_from_slice(&c);
    }
    p
}

/// The most pixels lit (r + g + b > 90, as site-smoke counts) in any of a
/// few frames through the loop, at its controls' defaults, over the picture
/// when it reads it.
fn lit(p: &Program) -> usize {
    let under = picture();
    let reads_picture = p.inputs() & 32 != 0;
    let [a, b, c] = p.meta().values([None, None, None]);
    let mut best = 0;
    for ms in [0u64, 400, 1000, 1700, 2600, 3900] {
        let mut inputs = Inputs {
            hour: 10,
            minute: 30,
            second: 45,
            a,
            b,
            c,
            ..Inputs::default()
        };
        inputs.set_ms(p, ms);
        let mut s = Scratch::<128>::new();
        let mut ink = Ink::boxed();
        let mut out = vec![0u8; W * H * 3];
        render_rows_ink::<128>(
            p,
            &mut s,
            Some(&mut ink),
            &inputs,
            &OWN,
            reads_picture.then_some(under.as_slice()),
            u32::MAX,
            &mut out,
        );
        let n = out
            .chunks(3)
            .filter(|px| px[0] as u32 + px[1] as u32 + px[2] as u32 > 90)
            .count();
        best = best.max(n);
    }
    best
}

#[test]
fn every_cheat_sheet_row_has_an_example_and_every_example_a_row() {
    let rows: Vec<&str> = [
        cheat::VARS,
        cheat::FUNCS,
        cheat::DRAW,
        cheat::COLS,
        cheat::DECLS,
    ]
    .iter()
    .flat_map(|r| r.iter().map(|e| e.0))
    .collect();
    for row in &rows {
        assert_eq!(
            EXAMPLES.iter().filter(|e| e.0 == *row).count(),
            1,
            "the cheat-sheet row `{row}` needs one example"
        );
    }
    for (row, _) in EXAMPLES {
        assert!(
            rows.contains(row),
            "the example for `{row}` is for no cheat-sheet row"
        );
    }
}

#[test]
fn every_example_compiles_and_lights_the_panel() {
    for (row, src) in EXAMPLES {
        let p = compile(src)
            .unwrap_or_else(|e| panic!("the example for `{row}` fails: {}\n{src}", e.msg));
        let n = lit(&p);
        assert!(
            n >= 24,
            "the example for `{row}` lights only {n} pixels\n{src}"
        );
        assert!(
            !src.contains('`') && !src.contains('\t'),
            "the example for `{row}`: no backticks or tabs"
        );
    }
}

#[test]
fn every_errors_help_gives_its_code_and_its_fix_works() {
    let mut last = 0;
    for h in HELP {
        assert!(h.code > last, "HELP is ascending ({})", h.code);
        last = h.code;
        assert!(
            errors::doc(h.code).is_some(),
            "help for {} which is not a code",
            errors::label(h.code)
        );
        assert!(!INTERNAL.contains(&h.code));
        assert_eq!(
            h.before.is_empty(),
            h.after.is_empty(),
            "{}: a before needs an after",
            errors::label(h.code)
        );
        if h.before.is_empty() {
            continue;
        }
        let e = match compile(h.before) {
            Ok(_) => panic!("{}'s before compiles:\n{}", errors::label(h.code), h.before),
            Err(e) => e,
        };
        assert_eq!(
            code_of(&e.msg),
            Some(h.code),
            "{}'s before fails with another error: {}",
            errors::label(h.code),
            e.msg
        );
        // (The same with the line end the page's copy has.)
        let e2 = compile(&format!("{}\n", h.before)).err().map(|e| e.msg);
        assert_eq!(
            e2.as_deref().and_then(code_of),
            Some(h.code),
            "{}'s before, with a line end",
            errors::label(h.code)
        );
        let p = compile(h.after).unwrap_or_else(|e| {
            panic!(
                "{}'s after fails: {}\n{}",
                errors::label(h.code),
                e.msg,
                h.after
            )
        });
        assert!(
            lit(&p) > 0,
            "{}'s after draws nothing",
            errors::label(h.code)
        );
    }
    for c in INTERNAL {
        assert!(errors::doc(*c).is_some(), "internal code {c} is not a code");
    }
}

/// The ```psi blocks between `## Recipes` and the next `## ` heading:
/// (the line the block starts on, its text, the gallery stem it is marked
/// with).
fn cookbook(text: &str) -> Vec<(usize, String, Option<String>)> {
    let lines: Vec<&str> = text.lines().collect();
    let a = lines
        .iter()
        .position(|l| *l == "## Recipes")
        .expect("docs/psi-spec.md has a `## Recipes`");
    let mut out = Vec::new();
    let mut mark: Option<String> = None;
    let mut k = a + 1;
    while k < lines.len() && !lines[k].starts_with("## ") {
        let l = lines[k];
        if let Some(stem) = l
            .strip_prefix("<!-- psi-gallery: ")
            .and_then(|r| r.strip_suffix(" -->"))
        {
            mark = Some(stem.to_string());
        } else if l == "```psi" {
            let start = k + 1;
            let mut end = start;
            while lines[end] != "```" {
                end += 1;
            }
            out.push((start, lines[start..end].join("\n"), mark.take()));
            k = end;
        }
        k += 1;
    }
    out
}

/// A gallery file's formula: its header lines (`-- author:`, `-- licence:`,
/// `-- bundle`) left out.
fn gallery(stem: &str) -> String {
    let text = std::fs::read_to_string(root(&format!("opendmd-web/formulas/{stem}.psi"))).unwrap();
    let body: Vec<&str> = text
        .lines()
        .skip_while(|l| {
            l.starts_with("-- author:")
                || l.starts_with("-- licence:")
                || l.starts_with("-- bundle")
        })
        .collect();
    body.join("\n")
}

#[test]
fn every_recipe_compiles_and_lights_the_panel() {
    let path = root("docs/psi-spec.md");
    let mut text = std::fs::read_to_string(&path).unwrap();
    let blocks = cookbook(&text);
    assert!(
        blocks.len() >= 7,
        "the reference has {} recipes",
        blocks.len()
    );
    let mut stale = Vec::new();
    for (line, src, mark) in &blocks {
        if let Some(stem) = mark {
            let want = gallery(stem);
            if *src != want {
                stale.push((src.clone(), want, stem.clone()));
                continue;
            }
        }
        let p = compile(src).unwrap_or_else(|e| {
            panic!("docs/psi-spec.md line {line}: the recipe fails: {}", e.msg)
        });
        assert!(
            lit(&p) >= 24,
            "docs/psi-spec.md line {line}: the recipe lights almost nothing"
        );
        assert!(
            src.lines().any(|l| l.starts_with("name \"")),
            "docs/psi-spec.md line {line}: give the recipe a `name` line"
        );
    }
    if stale.is_empty() {
        return;
    }
    if std::env::var_os("PSI_BLESS").is_none() {
        panic!("the recipes' copy of {} is not the gallery's: PSI_BLESS=1 cargo test --features docs --test reference", stale[0].2);
    }
    for (have, want, stem) in stale {
        let at = text
            .find(&format!("<!-- psi-gallery: {stem} -->\n```psi\n"))
            .unwrap();
        text = format!("{}{}", &text[..at], text[at..].replacen(&have, &want, 1));
    }
    std::fs::write(&path, text).unwrap();
}

/// The docs behind the site's Psi pages (tools/site.py PSI_PAGES; the
/// errors page's blocks are the errors' help, tested above). A guide page
/// not written yet is skipped.
const PSI_DOCS: &[&str] = &[
    "docs/psi-spec.md",
    "docs/guide/psi.md",
    "docs/guide/psi-1.md",
    "docs/guide/psi-2.md",
    "docs/guide/psi-3.md",
];

/// A doc's fenced blocks of one kind (```psi or ```psi-bad exactly): (the
/// line the block starts on, its text).
fn fenced(text: &str, kind: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let fence = format!("```{kind}");
    let mut out = Vec::new();
    let mut k = 0;
    while k < lines.len() {
        if lines[k].trim_end() == fence {
            let start = k + 1;
            let mut end = start;
            while end < lines.len() && !lines[end].starts_with("```") {
                end += 1;
            }
            out.push((start + 1, lines[start..end].join("\n")));
            k = end;
        }
        k += 1;
    }
    out
}

#[test]
fn every_block_the_psi_pages_draw_compiles_and_lights_and_every_bad_one_fails() {
    for doc in PSI_DOCS {
        let Ok(text) = std::fs::read_to_string(root(doc)) else {
            continue;
        };
        for (line, src) in fenced(&text, "psi") {
            let p = compile(&src).unwrap_or_else(|e| {
                panic!("{doc} line {line}: the site draws this block, but it fails: {} (a block meant to fail is ```psi-bad)", e.msg)
            });
            assert!(lit(&p) > 0, "{doc} line {line}: the site draws this block, but it lights nothing");
        }
        for (line, src) in fenced(&text, "psi-bad") {
            assert!(compile(&src).is_err(), "{doc} line {line}: a ```psi-bad block compiles (make it ```psi, and it is drawn)");
        }
    }
}

/// The guide's pages that teach Psi: every ```psi block on them is a whole
/// Formula a reader opens in the editor.
const GUIDE: [&str; 5] = [
    "docs/guide/psi-1.md",
    "docs/guide/psi-2.md",
    "docs/guide/psi-3.md",
    "docs/guide/psi.md",
    "docs/guide/editor.md",
];

/// Every ```psi block of a page: (the line it starts on, its text).
fn blocks(page: &str, text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut k = 0;
    while k < lines.len() {
        if lines[k] == "```psi" {
            let start = k + 1;
            let end = start
                + lines[start..]
                    .iter()
                    .position(|l| *l == "```")
                    .unwrap_or_else(|| panic!("{page} line {}: a ```psi block with no end", start));
            out.push((start + 1, lines[start..end].join("\n")));
            k = end;
        }
        k += 1;
    }
    out
}

#[test]
fn every_guide_example_compiles_and_lights_the_panel() {
    let mut n = 0;
    for page in GUIDE {
        let text = std::fs::read_to_string(root(page)).unwrap();
        for (line, src) in blocks(page, &text) {
            n += 1;
            let p = compile(&src)
                .unwrap_or_else(|e| panic!("{page} line {line}: the example fails: {}", e.msg));
            let lit = lit(&p);
            assert!(lit >= 24, "{page} line {line}: the example lights only {lit} pixels");
            assert!(
                src.lines().any(|l| l.starts_with("name \"")),
                "{page} line {line}: give the example a `name` line"
            );
        }
    }
    assert!(n >= 25, "the guide's Psi pages have only {n} examples");
}
