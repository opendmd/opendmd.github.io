//! The built-in table (`opendmd_psi::builtins`) is the one list of names:
//! the web app's highlighter words (`js/psikit.js`) and the editor's cheat
//! sheet and completions (`js/fhint.js`) are generated from it into marked blocks, and
//! this test fails when a block is not what the table gives.
//! `PSI_BLESS=1 cargo test --features docs --test builtins` rewrites them.
//! It also checks the cheat sheet against the compiler's tables: every
//! function, colour, value and declaration has a row, and every function a
//! row names is one the compiler knows.
#![cfg(not(target_arch = "wasm32"))]

use opendmd_psi::builtins::{self, cheat};
use std::path::PathBuf;

fn www(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../opendmd-web/www")
        .join(rel)
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
    let draws: Vec<&str> = builtins::PRIMITIVES
        .iter()
        .copied()
        .chain(["ink"])
        .collect();
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
    s.push_str(&names());
    s.push_str("// psi-builtins end\n");
    s
}

/// The rows in the order `js/fhint.js` flattens them (`ROWS`).
const ORDER: [&[cheat::Row]; 5] = [
    cheat::VARS,
    cheat::FUNCS,
    cheat::COLS,
    cheat::DRAW,
    cheat::DECLS,
];

/// Names the editor's own forms add to the tables' (special forms, block words).
const FORMS: [&str; 10] = [
    "since", "was", "under", "tween", "seed", "scene", "when", "if", "else", "key",
];

/// `name(…)`'s arguments in a cheat-sheet label (`rectfill(…)` takes the
/// row's first call's), or None.
fn sig_in(label: &str, name: &str) -> Option<String> {
    let args = |k: usize| -> String {
        let rest = &label[k..];
        let mut d = 0;
        for (j, c) in rest.char_indices() {
            match c {
                '(' => d += 1,
                ')' => {
                    d -= 1;
                    if d == 0 {
                        return rest[1..j].to_string();
                    }
                }
                _ => {}
            }
        }
        rest[1..].to_string()
    };
    let at = label.match_indices(&format!("{name}(")).find(|&(k, _)| {
        !label[..k].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    })?;
    let a = args(at.0 + name.len());
    Some(if a == "…" {
        args(label.find('(')?)
    } else {
        a
    })
}

/// The completions: every built-in name once, `[name or name(args), row]`
/// (its row in `ROWS`, -1 for none), functions with their arguments.
fn names() -> String {
    let rows: Vec<cheat::Row> = ORDER.iter().flat_map(|r| r.iter().copied()).collect();
    let calls: Vec<&str> = builtins::FUNCS
        .iter()
        .map(|f| f.0)
        .chain(builtins::COLOURS.iter().map(|f| f.0))
        .chain(builtins::PRIMITIVES)
        .chain(["since", "was", "under", "tween"])
        .collect();
    let arity = |n: &str| {
        builtins::FUNCS
            .iter()
            .map(|f| (f.0, f.1))
            .chain(builtins::COLOURS.iter().map(|f| (f.0, f.1)))
            .find(|f| f.0 == n)
            .map_or(1, |f| f.1)
    };
    let mut all: Vec<&str> = Vec::new();
    for n in builtins::INPUTS
        .iter()
        .map(|e| e.0)
        .chain(builtins::LATER)
        .chain(builtins::CONSTS.iter().map(|e| e.0))
        .chain(opendmd_psi::world::KEYS.iter().map(|k| k.name))
        .chain(builtins::FUNCS.iter().map(|e| e.0))
        .chain(builtins::COLOURS.iter().map(|e| e.0))
        .chain(builtins::PRIMITIVES)
        .chain(FORMS)
        .chain(builtins::KEYWORDS)
        .chain(builtins::CONTEXT_WORDS)
        .chain(
            builtins::DECLARATIONS
                .iter()
                .copied()
                .filter(|&d| d != "psi"),
        )
        .chain(builtins::PALETTES.iter().map(|e| e.0))
        .chain(builtins::BLENDS.iter().map(|e| e.0))
        .chain(builtins::FONTS.iter().map(|e| e.0))
    {
        if !all.contains(&n) {
            all.push(n);
        }
    }
    let mut s = String::from("const NAMES = [\n");
    for (k, n) in all.iter().enumerate() {
        // (Its row: the first whose label names it, else whose sentence does,
        // past the values';
        // `fract` is `frac`'s other spelling.)
        let w = if *n == "fract" { "frac" } else { n };
        let row = rows
            .iter()
            .position(|r| {
                if w.contains('.') {
                    r.0.contains(w)
                } else {
                    named(r.0).iter().any(|x| x == w)
                }
            })
            .or_else(|| {
                rows.iter()
                    .skip(cheat::VARS.len())
                    .position(|r| named(r.1).iter().any(|x| x == w))
                    .map(|k| k + cheat::VARS.len())
            });
        let sig = if calls.contains(n) {
            // (Not a row that defines a function of its own, `fn ring(…) = abs(…)`.)
            let a = rows
                .iter()
                .filter(|r| !r.0.starts_with("fn "))
                .find_map(|r| sig_in(r.0, n))
                .unwrap_or_else(|| ["v", "a, b", "a, b, c"][arity(n).clamp(1, 3) - 1].to_string());
            format!("{n}({a})")
        } else {
            n.to_string()
        };
        let sep = if k + 1 == all.len() { "" } else { "," };
        s.push_str(&format!(
            "  [{}, {}]{sep}\n",
            js(&sig),
            row.map_or(-1, |r| r as i64)
        ));
    }
    s.push_str("];\n");
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

// --- the language reference (docs/psi-spec.md, the site's /dev/psi/) -----------

const MD_HOW: &str = "generated from opendmd-psi/src/builtins.rs: PSI_BLESS=1 cargo test --features docs --test builtins";

/// A Markdown table cell: `|` escaped.
fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// A live example as a table cell: inline HTML `<code class="psi-ex">`
/// (the reference page draws it), with every character Markdown or a table
/// would read escaped and the lines as `<br>`.
fn example_cell(src: &str) -> String {
    let mut o = String::from("<code class=\"psi-ex\">");
    for c in src.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '|' => o.push_str("&#124;"),
            '*' => o.push_str("&#42;"),
            '_' => o.push_str("&#95;"),
            '[' => o.push_str("&#91;"),
            ']' => o.push_str("&#93;"),
            '\\' => o.push_str("&#92;"),
            '#' => o.push_str("&#35;"),
            '\n' => o.push_str("<br>"),
            c => o.push(c),
        }
    }
    o.push_str("</code>");
    o
}

fn code_list(v: &[&str]) -> String {
    v.iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every name the language knows, by kind.
fn spec_names() -> String {
    let funcs: Vec<String> = builtins::FUNCS
        .iter()
        .map(|(n, arity, f)| match f {
            builtins::F::Pick => format!("`{n}(k, a, b, …)`"),
            builtins::F::Glyph(_) => format!("`{n}(…)` (3 values and a font)"),
            _ => format!("`{n}` ({arity})"),
        })
        .collect();
    let colours: Vec<String> = builtins::COLOURS
        .iter()
        .map(|(n, k, _)| format!("`{n}` ({k})"))
        .collect();
    let values: Vec<&str> = builtins::INPUTS
        .iter()
        .map(|e| e.0)
        .chain(["seed"])
        .collect();
    let consts: Vec<&str> = builtins::CONSTS.iter().map(|e| e.0).collect();
    let pals: Vec<&str> = builtins::PALETTES.iter().map(|e| e.0).collect();
    let fonts: Vec<&str> = builtins::FONTS.iter().map(|e| e.0).collect();
    let blends: Vec<&str> = builtins::BLENDS.iter().map(|e| e.0).collect();
    let keys: Vec<&str> = opendmd_psi::world::KEYS.iter().map(|k| k.name).collect();
    let mut s = format!("<!-- psi-builtins: names ({MD_HOW}) -->\n");
    s.push_str("| kind | names |\n| --- | --- |\n");
    for (kind, names) in [
        ("values", code_list(&values)),
        ("values about the middle", code_list(&builtins::LATER)),
        ("built-in inputs (values from the panel)", code_list(&keys)),
        ("their events", code_list(&["since(KEY)", "was(KEY)"])),
        ("constants", code_list(&consts)),
        ("functions (how many values)", funcs.join(", ")),
        (
            "colours (how many values, before the coverage)",
            colours.join(", "),
        ),
        ("palettes", code_list(&pals)),
        ("fonts", code_list(&fonts)),
        ("blends", code_list(&blends)),
        ("keywords", code_list(&builtins::KEYWORDS)),
        ("drawing", code_list(&builtins::PRIMITIVES)),
        ("declarations", code_list(&builtins::DECLARATIONS)),
        (
            "words that are keywords in one place only",
            code_list(&builtins::CONTEXT_WORDS),
        ),
        (
            "words of the draw stage and scenes (names elsewhere)",
            code_list(&["if", "else", "when", "scene", "tween"]),
        ),
        ("the picture under", code_list(&["under", "under(x, y)"])),
        (
            "the built-in inputs' groups (not names)",
            code_list(&["sun", "moon", "replay"]),
        ),
    ] {
        s.push_str(&format!("| {kind} | {} |\n", cell(&names)));
    }
    s.push_str(
        "\n**The name rule.** A formula's names win over the built-ins.\n\n- Any built-in value's, function's or drawing call's name may be set as a value of the formula's (`t = 0.5`, `sin = 0.25`, `line = 3`). From the line that sets it, the name reads as the formula's value wherever a value is read (setting `x` or `y` moves `px py i cx cy r angle` with it, as in a function). A call `name(…)` is still the built-in function, and `since`, `was`, `under` and `ink(n)` work the same way as `sin`.\n- The keywords (E204), the palettes (E208) and `sun`, `moon` and `replay` (E216) are not names.\n- A `fn`, table, sprite or palette of the formula's needs a name of its own. None may take the name of a value the formula has set (E203), and a `fn` may not take a built-in function's name either (E215).\n- `since(k)` and `was(k)` read the built-in inputs only. On a name the formula has set they are refused (E217), so a name is not read two ways on one line.\n- A built-in added later is read only where a formula has not set its name, so a formula saved before it means what it meant.\n",
    );
    s.push_str("<!-- psi-builtins end -->\n");
    s
}

/// The cheat sheet's rows (the editor's words) as tables.
fn spec_reference() -> String {
    let mut s = format!("<!-- psi-builtins: reference ({MD_HOW}) -->\n");
    for (title, rows) in [
        ("Values", cheat::VARS),
        ("Functions and statements", cheat::FUNCS),
        ("Drawing", cheat::DRAW),
        ("Colours", cheat::COLS),
        ("Declarations", cheat::DECLS),
    ] {
        s.push_str(&format!(
            "\n#### {title}\n\n| write | what it does | example |\n| --- | --- | --- |\n"
        ));
        for (what, does, _) in rows {
            let ex = opendmd_psi::reference::example(what)
                .expect("every row has an example (tests/reference.rs)");
            s.push_str(&format!(
                "| `{}` | {} | {} |\n",
                cell(what),
                cell(does),
                example_cell(ex)
            ));
        }
    }
    s.push_str("<!-- psi-builtins end -->\n");
    s
}

/// The limits, from the compiler's constants.
fn spec_limits() -> String {
    use opendmd_psi as p;
    let n = |v: usize| {
        let s = v.to_string();
        let mut o = String::new();
        for (k, c) in s.chars().enumerate() {
            if k > 0 && (s.len() - k).is_multiple_of(3) {
                o.push(',');
            }
            o.push(c);
        }
        o
    };
    let rows = [
        (
            "a formula's text, comments included",
            format!("{} bytes", n(p::MAX_SOURCE)),
        ),
        (
            "steps (per frame, draw, per row, per pixel, and one per layer)",
            n(p::MAX_STEPS),
        ),
        ("values held a pixel at once", n(p::REGS)),
        (
            "constants and per-frame and per-row values held at once",
            n(p::MAX_SCALARS),
        ),
        (
            "nesting of brackets, operators and function calls",
            n(p::MAX_DEPTH),
        ),
        ("`table` and `sprite` numbers, in all", n(p::MAX_TABLE)),
        ("numbers in one `table`", n(p::MAX_TABLE_LEN)),
        (
            "built-in inputs read (each `since` and `was` one more)",
            n(p::MAX_INS),
        ),
        ("palettes of your own (of colours)", n(p::MAX_PALETTES)),
        ("palettes `pal` and `cycle` read", n(p::USER_PALETTES)),
        ("loop turns a frame in `draw { }`", n(p::TURNS as usize)),
        ("lines and dots a frame", n(p::PRIMITIVES as usize)),
        ("line steps a frame", n(p::FRAME_STEPS as usize)),
        ("`layer rate N`", format!("N from 2 to {}", p::MAX_RATE)),
        (
            "`name` / `about` / a control's label",
            format!(
                "{} / {} / {} characters",
                p::MAX_NAME_TEXT,
                p::MAX_ABOUT_TEXT,
                p::MAX_LABEL_TEXT
            ),
        ),
    ];
    let mut s =
        format!("<!-- psi-builtins: limits ({MD_HOW}) -->\n| limit | at most |\n| --- | --- |\n");
    for (what, v) in rows {
        s.push_str(&format!("| {what} | {v} |\n"));
    }
    s.push_str("<!-- psi-builtins end -->\n");
    s
}

/// The Markdown block in `doc` (from the repository's root) that starts
/// with `<!-- psi-builtins: <what> (` and ends after `<!-- psi-builtins
/// end -->`'s line, replaced by `want` under PSI_BLESS.
fn check_md_block(doc: &str, what: &str, want: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(doc);
    let text = std::fs::read_to_string(&path).unwrap();
    let head = format!("<!-- psi-builtins: {what} (");
    let start = text
        .find(&head)
        .unwrap_or_else(|| panic!("{doc}: no `{head}` block"));
    let end_mark = "<!-- psi-builtins end -->\n";
    let end = start + text[start..].find(end_mark).expect("block end") + end_mark.len();
    if &text[start..end] == want {
        return;
    }
    if std::env::var_os("PSI_BLESS").is_some() {
        std::fs::write(&path, format!("{}{want}{}", &text[..start], &text[end..])).unwrap();
        return;
    }
    panic!("{doc}'s {what} block is not what builtins.rs gives (PSI_BLESS=1 to rewrite):\n--- in the file\n{}\n--- generated\n{want}", &text[start..end]);
}

#[test]
fn the_reference_pages_tables_are_the_tables() {
    check_md_block("docs/psi-spec.md", "names", &spec_names());
    check_md_block("docs/psi-spec.md", "reference", &spec_reference());
    check_md_block("docs/psi-spec.md", "limits", &spec_limits());
}

/// The reference's grammar is psi-for-models.md's, word for word.
#[test]
fn the_reference_pages_grammar_is_the_models() {
    let read = |doc: &str| {
        std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join(doc),
        )
        .unwrap()
    };
    let grammar = |text: &str| -> String {
        let a = text
            .find("```text\nformula   =")
            .expect("the grammar block");
        let e = a + text[a..].find("\n```\n").expect("its end");
        text[a..e].to_string()
    };
    assert_eq!(
        grammar(&read("docs/psi-spec.md")),
        grammar(&read("docs/psi-for-models.md"))
    );
}

/// Every built-in is named in the models' own words too (their tables are
/// written by hand; the guide teaches and links to the reference instead).
#[test]
fn the_models_name_every_built_in() {
    for doc in [
        "docs/psi-for-models.md",
        "docs/psi-card.md",
    ] {
        let text = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join(doc),
        )
        .unwrap();
        let words: std::collections::HashSet<&str> = text
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .collect();
        let names = builtins::FUNCS
            .iter()
            .map(|e| e.0)
            .chain(builtins::COLOURS.iter().map(|e| e.0))
            .chain(builtins::INPUTS.iter().map(|e| e.0))
            .chain(builtins::LATER)
            .chain(builtins::PALETTES.iter().map(|e| e.0))
            .chain(builtins::FONTS.iter().map(|e| e.0))
            .chain(builtins::BLENDS.iter().map(|e| e.0))
            .chain(builtins::PRIMITIVES);
        // (`fract` is `frac`'s other spelling.)
        for n in names.filter(|&n| n != "fract") {
            assert!(words.contains(n), "{doc} never names `{n}`");
        }
        // The built-in inputs, dotted names whole, and their events.
        for k in opendmd_psi::world::KEYS.iter().map(|k| k.name).chain(["since(", "was("]) {
            assert!(text.contains(k), "{doc} never names `{k}`");
        }
    }
}

#[test]
fn the_highlighters_words_are_the_tables() {
    check_block("js/psikit.js", "highlighter", &highlighter());
}

#[test]
fn the_cheat_sheet_is_the_tables() {
    check_block("js/fhint.js", "cheat sheet", &cheat_sheet());
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
    let words: Vec<String> = rows.iter().flat_map(|l| named(l)).collect();
    let has = |n: &str| words.iter().any(|w| w == n);
    for (name, _, _) in builtins::FUNCS {
        // (`fract` is `frac`'s other spelling.)
        assert!(
            name == "fract" || has(name),
            "no cheat-sheet row for `{name}`"
        );
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
        assert!(
            name == "psi" || has(name),
            "no cheat-sheet row for `{name}`"
        );
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
                    || head == "p"
                    // `ink(n)`: the canvas in n levels (a special form)
                    || head == "ink"
                    // `tween(P, Q, w)`: a palette line's (a special form)
                    || head == "tween"
                    // `since(KEY)`, `was(KEY)`: a built-in input's events
                    || head == "since"
                    || head == "was"
                    // a function the row itself defines (`fn ring(cx, r) = …`)
                    || label.starts_with(&format!("fn {head}("));
                assert!(
                    known,
                    "`{head}(…)` in the cheat sheet is not a built-in ({label})"
                );
            }
        }
    }
}

#[test]
fn the_palettes_the_cheat_sheet_lists_are_the_compilers() {
    let row = cheat::COLS
        .iter()
        .find(|r| r.0 == "pal(fire, v)")
        .unwrap()
        .1;
    for (name, _) in builtins::PALETTES {
        assert!(
            row.contains(name),
            "the cheat sheet's palettes miss `{name}`"
        );
    }
}
