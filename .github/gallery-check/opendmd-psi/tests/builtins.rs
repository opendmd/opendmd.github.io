//! The built-in table (`opendmd_psi::builtins`) is the one list of names:
//! the web app's highlighter words (`js/psikit.js`) and the editor's cheat
//! sheet (`js/formulas.js`) are generated from it into marked blocks, and
//! this test fails when a block is not what the table gives.
//! `PSI_BLESS=1 cargo test --features docs --test builtins` rewrites them.
//! It also checks the cheat sheet against the compiler's tables: every
//! function, colour, value and declaration has a row, and every function a
//! row names is one the compiler knows.
#![cfg(not(target_arch = "wasm32"))]

use opendmd_psi::builtins::{self, cheat};
use std::path::PathBuf;

fn www(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../opendmd-web/www").join(rel)
}

const HOW: &str = "generated from opendmd-psi/src/builtins.rs: PSI_BLESS=1 cargo test --features docs --test builtins";

/// The highlighter's word lists.
fn highlighter() -> String {
    let words: Vec<&str> = builtins::KEYWORDS
        .iter()
        .copied()
        // (`use` is the editor's: it never reaches the compiler.)
        .filter(|&w| w != "use")
        .chain(builtins::PRIMITIVES)
        .chain(["ink"])
        .collect();
    let draws: Vec<&str> = builtins::PRIMITIVES.iter().copied().chain(["ink"]).collect();
    format!(
        "// psi-builtins: highlighter ({HOW})\nconst WORDS = /^({})$/, DRAWS = /^({})$/, DECLS = /^({})$/;\n// psi-builtins end\n",
        words.join("|"),
        draws.join("|"),
        builtins::DECLARATIONS.join("|")
    )
}

fn js(s: &str) -> String {
    let mut o = String::from("'");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\'' => o.push_str("\\'"),
            '\n' => o.push_str("\\n"),
            c => o.push(c),
        }
    }
    o.push('\'');
    o
}

/// The cheat sheet's rows as JS constants.
fn cheat_sheet() -> String {
    let mut s = format!("// psi-builtins: cheat sheet ({HOW})\n");
    for (name, rows) in [
        ("VARS", cheat::VARS),
        ("FUNCS", cheat::FUNCS),
        ("DRAW", cheat::DRAW),
        ("COLS", cheat::COLS),
        ("DECLS", cheat::DECLS),
    ] {
        s.push_str(&format!("const {name} = [\n"));
        for (k, &(a, b, c)) in rows.iter().enumerate() {
            let sep = if k + 1 == rows.len() { "" } else { "," };
            if c.is_empty() {
                s.push_str(&format!("  [{}, {}]{sep}\n", js(a), js(b)));
            } else {
                s.push_str(&format!("  [{}, {}, {}]{sep}\n", js(a), js(b), js(c)));
            }
        }
        s.push_str("];\n");
    }
    s.push_str("// psi-builtins end\n");
    s
}

/// The block in `file` that starts with `// psi-builtins: <what>` and ends
/// after `// psi-builtins end`'s line, replaced by `want` under PSI_BLESS.
fn check_block(file: &str, what: &str, want: &str) {
    let path = www(file);
    let text = std::fs::read_to_string(&path).unwrap();
    let head = format!("// psi-builtins: {what} (");
    let start = text
        .find(&head)
        .unwrap_or_else(|| panic!("{file}: no `{head}` block"));
    let end_mark = "// psi-builtins end\n";
    let end = start + text[start..].find(end_mark).expect("block end") + end_mark.len();
    if &text[start..end] == want {
        return;
    }
    if std::env::var_os("PSI_BLESS").is_some() {
        let out = format!("{}{want}{}", &text[..start], &text[end..]);
        std::fs::write(&path, out).unwrap();
        return;
    }
    panic!(
        "{file}'s {what} block is not what builtins.rs gives (PSI_BLESS=1 to rewrite):\n--- in the file\n{}\n--- generated\n{want}",
        &text[start..end]
    );
}

#[test]
fn the_highlighters_words_are_the_tables() {
    check_block("js/psikit.js", "highlighter", &highlighter());
}

#[test]
fn the_cheat_sheet_is_the_tables() {
    check_block("js/formulas.js", "cheat sheet", &cheat_sheet());
}

/// The words in a cheat-sheet label that look like a call or a bare name.
fn named(label: &str) -> Vec<String> {
    label
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| w.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
        .map(String::from)
        .collect()
}

#[test]
fn every_built_in_has_a_cheat_sheet_row() {
    let rows: Vec<&str> = [cheat::VARS, cheat::FUNCS, cheat::DRAW, cheat::COLS, cheat::DECLS]
        .iter()
        .flat_map(|r| r.iter().map(|e| e.0))
        .collect();
    let words: Vec<String> = rows.iter().flat_map(|l| named(l)).collect();
    let has = |n: &str| words.iter().any(|w| w == n);
    for (name, _, _) in builtins::FUNCS {
        // (`fract` is `frac`'s other spelling.)
        assert!(name == "fract" || has(name), "no cheat-sheet row for `{name}`");
    }
    for (name, _, _) in builtins::COLOURS {
        assert!(has(name), "no cheat-sheet row for `{name}`");
    }
    for (name, _, _) in builtins::INPUTS {
        assert!(has(name), "no cheat-sheet row for `{name}`");
    }
    for name in builtins::LATER.iter().chain(&builtins::PRIMITIVES) {
        assert!(has(name), "no cheat-sheet row for `{name}`");
    }
    for name in builtins::DECLARATIONS {
        // (`psi 1` is never written by the editor: unversioned is 1.)
        assert!(name == "psi" || has(name), "no cheat-sheet row for `{name}`");
    }
    for (name, _) in builtins::CONSTS {
        assert!(has(name), "no cheat-sheet row for `{name}`");
    }
}

#[test]
fn every_call_in_the_cheat_sheet_is_a_built_in() {
    for rows in [cheat::FUNCS, cheat::DRAW, cheat::COLS] {
        for (label, _, _) in rows {
            for (k, _) in label.match_indices('(') {
                let head: String = label[..k]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                if head.is_empty() {
                    continue;
                }
                let known = builtins::FUNCS.iter().any(|e| e.0 == head)
                    || builtins::COLOURS.iter().any(|e| e.0 == head)
                    || builtins::PRIMITIVES.contains(&head.as_str())
                    // a part drawn at a place, as the row's example says
                    || head == "p";
                assert!(known, "`{head}(…)` in the cheat sheet is not a built-in ({label})");
            }
        }
    }
}

#[test]
fn the_palettes_the_cheat_sheet_lists_are_the_compilers() {
    let row = cheat::COLS.iter().find(|r| r.0 == "pal(fire, v)").unwrap().1;
    for (name, _) in builtins::PALETTES {
        assert!(row.contains(name), "the cheat sheet's palettes miss `{name}`");
    }
}
