//! The error index (`opendmd_psi::errors`, feature `docs`) against the
//! compiler. `tests/diagnostics.txt` holds the golden cases: a source, the
//! place and the message it fails with, under the code it must be
//! classified as:
//!
//! ```text
//! === E203
//! v = 1
//! v = 2
//! gray(v)
//! --- 2:1 `v` is already set on line 1 — each name can be set once
//! ```
//!
//! In a source line, `\u{…}` is that character, `<<N*TEXT>>` is TEXT N
//! times and `<<N;SEP;TEMPLATE>>` is TEMPLATE for k = 1..N (`{k}`), joined
//! by SEP (`\n` for a newline). The test also checks that every code has a
//! case; that every `// E…` code in `src/compile.rs` is indexed; that every
//! sentence `src/compile.rs` can say (found by a scan of its string
//! literals: see `unclassified_sentences`) has a code; and that the error
//! index in `docs/psi-spec.md` is what `errors::ERRORS` gives.
//! `PSI_BLESS=1 cargo test --features docs --test diagnostics` rewrites the
//! `---` lines and the index.
#![cfg(not(target_arch = "wasm32"))]

use opendmd_psi::errors::{self, ERRORS};
use opendmd_psi::reference;
use opendmd_psi::{compile, look};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn bless() -> bool {
    std::env::var_os("PSI_BLESS").is_some()
}

const COMPILE_RS: &str = include_str!("../src/compile.rs");

// --- the golden cases -------------------------------------------------------

struct Case {
    code: u16,
    src: String,
    /// The index of the `---` line in the file.
    at: usize,
    want: String,
}

/// `\u{…}` and the `<<…>>` macros of a source line.
fn expand(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("<<") {
            let end = r.find(">>").expect("an unclosed <<");
            let body = &r[..end];
            rest = &r[end + 2..];
            let join = body
                .split_once(';')
                .and_then(|(n, _)| n.parse::<usize>().ok());
            if let Some(n) = join {
                let mut p = body.splitn(3, ';').skip(1);
                let sep = p.next().unwrap().replace("\\n", "\n");
                let tpl = p.next().unwrap();
                let items: Vec<String> = (1..=n)
                    .map(|k| tpl.replace("{k}", &k.to_string()))
                    .collect();
                out += &items.join(&sep);
            } else {
                let (n, text) = body
                    .split_once('*')
                    .expect("<<N*TEXT>> or <<N;SEP;TEMPLATE>>");
                out += &text.repeat(n.parse().unwrap());
            }
        } else if let Some(r) = rest.strip_prefix("\\u{") {
            let end = r.find('}').expect("an unclosed \\u{");
            out.push(char::from_u32(u32::from_str_radix(&r[..end], 16).unwrap()).unwrap());
            rest = &r[end + 1..];
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

fn cases(text: &str) -> Vec<Case> {
    let mut out = Vec::new();
    let mut cur: Option<(u16, Vec<String>)> = None;
    for (n, line) in text.lines().enumerate() {
        if let Some(c) = line.strip_prefix("=== E") {
            assert!(
                cur.is_none(),
                "line {}: a case without its `---` line",
                n + 1
            );
            cur = Some((c.trim().parse().expect("a code"), Vec::new()));
        } else if let Some(want) = line.strip_prefix("---") {
            let (code, lines) = cur
                .take()
                .unwrap_or_else(|| panic!("line {}: `---` outside a case", n + 1));
            out.push(Case {
                code,
                src: lines.join("\n"),
                at: n,
                want: want.trim().to_string(),
            });
        } else if let Some((_, lines)) = &mut cur {
            lines.push(expand(line));
        }
    }
    assert!(cur.is_none(), "the last case has no `---` line");
    out
}

/// What a case fails with: "line:col message".
fn outcome(src: &str) -> Result<String, String> {
    match compile(src) {
        Ok(_) => Err("it compiled".to_string()),
        Err(e) => {
            let (l, c) = e.line_col(src);
            Ok(format!("{l}:{c} {}", e.msg))
        }
    }
}

#[test]
fn every_case_fails_with_its_code_and_sentence() {
    let file = path("tests/diagnostics.txt");
    let text = std::fs::read_to_string(&file).unwrap();
    let cs = cases(&text);
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut bad = Vec::new();
    for c in &cs {
        let shown = c.src.chars().take(80).collect::<String>();
        match outcome(&c.src) {
            Err(why) => bad.push(format!("E{:03} {shown:?}: {why}", c.code)),
            Ok(got) => {
                let msg = got.split_once(' ').unwrap().1;
                match errors::code_of(msg) {
                    Some(code) if code == c.code => {}
                    other => bad.push(format!(
                        "E{:03} {shown:?}: classified as {other:?}: {msg}",
                        c.code
                    )),
                }
                if bless() {
                    lines[c.at] = format!("--- {got}");
                } else if got != c.want {
                    bad.push(format!(
                        "E{:03} {shown:?}:\n  got  {got}\n  want {}",
                        c.code, c.want
                    ));
                }
            }
        }
    }
    if bless() && bad.is_empty() {
        lines.push(String::new());
        while lines.len() > 1 && lines[lines.len() - 2].is_empty() {
            lines.pop();
        }
        std::fs::write(&file, lines.join("\n")).unwrap();
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

/// Sentences no source can provoke: the code is checked on the text.
const NO_CASE: &[(u16, &str, &str)] = &[
    (
        407,
        "this formula could not be laid out (a bug in Psi) — please report it",
        "a compiler bug",
    ),
    (408, opendmd_psi::NO_MEMORY, "needs the allocator to fail"),
];

#[test]
fn every_code_has_a_case_and_every_case_a_code() {
    let cs = cases(&std::fs::read_to_string(path("tests/diagnostics.txt")).unwrap());
    let have: BTreeSet<u16> = cs.iter().map(|c| c.code).collect();
    let mut bad = Vec::new();
    for d in ERRORS {
        if !have.contains(&d.code) && !NO_CASE.iter().any(|n| n.0 == d.code) && d.code != 968 {
            bad.push(format!("{} has no case", errors::label(d.code)));
        }
    }
    for c in have {
        if errors::doc(c).is_none() {
            bad.push(format!(
                "a case names {}, which is not in ERRORS",
                errors::label(c)
            ));
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

#[test]
fn the_sentences_nothing_can_provoke_are_classified() {
    for &(code, msg, _why) in NO_CASE {
        assert_eq!(errors::code_of(msg), Some(code), "{msg}");
    }
    // A look composed with a blend it has not got (not a compile error: a
    // host's call).
    let e = look::compose("gray(x)", "nope", 50).unwrap_err();
    assert_eq!(errors::code_of(&e.msg), Some(968), "{}", e.msg);
}

// --- the index itself ---------------------------------------------------------

#[test]
fn the_index_is_well_formed() {
    assert!(!ERRORS.is_empty());
    for w in ERRORS.windows(2) {
        assert!(
            w[0].code < w[1].code,
            "E{:03} is out of order or twice",
            w[1].code
        );
    }
    for d in ERRORS {
        let l = errors::label(d.code);
        assert!((1..=999).contains(&d.code), "{l}");
        assert!(!d.matches.is_empty(), "{l} has no fragments");
        for m in d.matches {
            assert!(!m.is_empty(), "{l} has an empty fragment");
        }
        for (what, s) in [("title", d.title), ("sentence", d.sentence), ("fix", d.fix)] {
            assert!(!s.is_empty(), "{l} has no {what}");
            assert!(!s.contains("color"), "{l} {what}: British spelling");
            assert!(
                !s.contains(".rs") && !s.contains("cache"),
                "{l} {what} names an internal"
            );
        }
        for m in d.matches {
            assert!(
                d.sentence.contains(m),
                "{l}: the fragment {m:?} is not in its sentence"
            );
        }
        assert_eq!(errors::doc(d.code).map(|x| x.code), Some(d.code));
    }
    assert_eq!(errors::label(203), "E203");
    assert_eq!(errors::label(1), "E001");
    assert!(errors::doc(999).is_none());
    assert_eq!(errors::code_of("a message no entry knows"), None);
}

// --- a scan of compile.rs ---------------------------------------------------

const OPEN: char = '\u{1}';
const CLOSE: char = '\u{2}';

struct Scan {
    /// The source with comments cut out and each string literal replaced by
    /// `OPEN index CLOSE`.
    code: String,
    lits: Vec<String>,
    comments: Vec<String>,
}

fn scan(src: &str) -> Scan {
    let b: Vec<char> = src.chars().collect();
    let (mut i, mut code, mut lits, mut comments) = (0, String::new(), Vec::new(), Vec::new());
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            let s = i;
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            comments.push(b[s..i].iter().collect());
        } else if c == '/' && next == Some('*') {
            let (s, mut depth) = (i, 0);
            loop {
                if b[i] == '/' && b.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            comments.push(b[s..i].iter().collect());
        } else if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                match b[i] {
                    '\\' => {
                        i += 1;
                        match b[i] {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            '0' => s.push('\0'),
                            '\n' => {
                                while b[i + 1].is_whitespace() {
                                    i += 1;
                                }
                            }
                            'u' => {
                                let end = (i..).find(|&k| b[k] == '}').unwrap();
                                let hex: String = b[i + 2..end].iter().collect();
                                s.push(
                                    char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap(),
                                );
                                i = end;
                            }
                            other => s.push(other),
                        }
                        i += 1;
                    }
                    '"' => {
                        i += 1;
                        break;
                    }
                    ch => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            code.push(OPEN);
            code += &lits.len().to_string();
            code.push(CLOSE);
            lits.push(s);
        } else if c == '\'' {
            // A character literal, or a lifetime.
            if next == Some('\\') {
                i += 2;
                while b[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if b.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                code.push(c);
                i += 1;
            }
            code.push(' ');
        } else {
            code.push(c);
            i += 1;
        }
    }
    Scan {
        code,
        lits,
        comments,
    }
}

/// The literals (indices) between the markers in `s`.
fn lits_in(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(a) = rest.find(OPEN) {
        let r = &rest[a + 1..];
        let z = r.find(CLOSE).unwrap();
        out.push(r[..z].parse().unwrap());
        rest = &r[z + 1..];
    }
    out
}

/// The text inside the brackets that open at `open` (a `(`, `[` or `{`).
fn inside(code: &str, open: usize) -> &str {
    let mut depth = 0;
    for (k, ch) in code[open..].char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return &code[open + 1..open + k];
                }
            }
            _ => {}
        }
    }
    &code[open + 1..]
}

/// A `format!` literal as the message it makes: `{{` is `{`, and a
/// placeholder is a stand-in word.
fn formatted(s: &str) -> String {
    let (mut out, mut it) = (String::new(), s.chars().peekable());
    while let Some(c) = it.next() {
        match (c, it.peek()) {
            ('{', Some('{')) | ('}', Some('}')) => {
                out.push(c);
                it.next();
            }
            ('{', _) => {
                for d in it.by_ref() {
                    if d == '}' {
                        break;
                    }
                }
                out.push('N');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Literals that have a space but are not an error's sentence.
const NOT_SENTENCES: &[(&str, &str)] = &[
    ("line {line}: {}\n  {text}\n  {}{}", "Error::render's layout, not a message"),
    (" — did you mean `{s}`?", "the hint appended to unknown names, which keep their own sentence's code"),
    (", the picture and the turn", "a piece of E934's sentence, an argument of its format!"),
    (" and one value", "a piece of E934's sentence, an argument of its format!"),
    (" ", "a separator in a format! argument"),
    ("`{}` (line {}) is per {per}: it reads `{w}`", "the end of E301's, E708's and E931's sentences (`Parser::varies`), an argument of their format!"),
    ("this uses `{w}` ({what})", "the end of E301's, E708's and E931's sentences (`Parser::varies`), an argument of their format!"),
    ("too long", "the lexer's own cap, for the language service's 64 KB sources: `compile` stops at E401 first"),
];

/// The message of every literal in `compile.rs` that is, or feeds, an
/// error's sentence. The heuristic: every string literal (with a space in
/// it) inside the brackets of `Error::new(`, `.err(`, `.fail(`,
/// `.fail_at(`, `rate_err(`, `.expect(`/`.expected(` (the sentence is
/// `expected WHAT here, found …`), every `format!(` literal, every
/// `const NAME: &str = "…"` and every literal in a `let msg = …;`, where a
/// `format!` literal's `{…}` are placeholders.
fn sentences() -> Vec<(String, String)> {
    let sc = scan(COMPILE_RS);
    let mut found: Vec<(usize, bool)> = Vec::new();
    let mut expected: Vec<usize> = Vec::new();
    let mut templates = BTreeSet::new();
    for (pat, exp) in [
        ("Error::new(", false),
        (".err(", false),
        (".fail(", false),
        (".fail_at(", false),
        ("rate_err(", false),
        (".expect(", true),
        (".expected(", true),
        ("format!(", false),
        ("let msg = ", false),
    ] {
        for (at, _) in sc.code.match_indices(pat) {
            let open = at + pat.len() - 1;
            let body = if pat.ends_with('(') {
                inside(&sc.code, open).to_string()
            } else {
                // To the `;` outside any brackets.
                let (mut depth, mut end) = (0, sc.code.len());
                for (k, ch) in sc.code[at + pat.len()..].char_indices() {
                    match ch {
                        '(' | '[' | '{' => depth += 1,
                        ')' | ']' | '}' => depth -= 1,
                        ';' if depth == 0 => {
                            end = at + pat.len() + k;
                            break;
                        }
                        _ => {}
                    }
                }
                sc.code[at + pat.len()..end].to_string()
            };
            for l in lits_in(&body) {
                if exp {
                    expected.push(l);
                } else {
                    found.push((l, false));
                }
            }
            // (A `format!`'s first literal is its template.)
            if pat == "format!(" {
                if let Some(&l) = lits_in(&body)
                    .first()
                    .filter(|_| body.trim_start().starts_with(OPEN))
                {
                    templates.insert(l);
                }
            }
        }
    }
    // Message constants.
    for (at, _) in sc.code.match_indices(": &str =") {
        let rest = sc.code[at + 8..].trim_start();
        if rest.starts_with(OPEN) {
            let upto = &rest[..rest.find(CLOSE).unwrap() + 1];
            found.push((lits_in(upto)[0], false));
        }
    }
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (l, _) in found {
        let fmt = templates.contains(&l);
        let raw = &sc.lits[l];
        if !raw.contains(' ') || !seen.insert((l, fmt)) {
            continue;
        }
        if NOT_SENTENCES.iter().any(|n| n.0 == raw) {
            continue;
        }
        out.push((raw.clone(), if fmt { formatted(raw) } else { raw.clone() }));
    }
    for l in expected {
        let raw = &sc.lits[l];
        out.push((raw.clone(), format!("expected {raw} here, found N")));
    }
    out
}

#[test]
fn unclassified_sentences() {
    let all = sentences();
    assert!(
        all.len() > 150,
        "the scan found only {} sentences",
        all.len()
    );
    let bad: Vec<String> = all
        .iter()
        .filter(|(_, msg)| errors::code_of(msg).is_none())
        .map(|(raw, msg)| format!("{raw:?} (as {msg:?})"))
        .collect();
    assert!(bad.is_empty(), "no code for:\n{}", bad.join("\n"));
}

/// Codes that appear in comments but are not an entry, and why.
const COMMENT_ONLY: &[(u16, &str)] = &[
    (
        907,
        "none: `loop` takes its own sentence at the line, which is E110",
    ),
    (
        917,
        "a table that the sprite's rows overflow says E924's sentence: the budget is one",
    ),
];

#[test]
fn comment_codes_are_indexed() {
    let sc = scan(COMPILE_RS);
    let mut seen = BTreeSet::new();
    for c in &sc.comments {
        let b = c.as_bytes();
        for k in 0..b.len().saturating_sub(3) {
            if b[k] == b'E'
                && b[k + 1..k + 4].iter().all(u8::is_ascii_digit)
                && (k == 0 || !b[k - 1].is_ascii_alphanumeric())
            {
                seen.insert(c[k + 1..k + 4].parse::<u16>().unwrap());
            }
        }
    }
    assert!(seen.len() > 30, "found {} codes in comments", seen.len());
    let bad: Vec<String> = seen
        .into_iter()
        .filter(|&c| errors::doc(c).is_none() && !COMMENT_ONLY.iter().any(|o| o.0 == c))
        .map(errors::label)
        .collect();
    assert!(
        bad.is_empty(),
        "codes in compile.rs comments but not in ERRORS: {bad:?}"
    );
}

// --- the language reference's index -------------------------------------------

/// Plain text for Markdown: `<`, `>`, `&`, `*`, `_`, `[`, `]` escaped outside
/// the `code spans`.
fn md(s: &str) -> String {
    let (mut out, mut code) = (String::new(), false);
    for c in s.chars() {
        match c {
            '`' => {
                code = !code;
                out.push(c);
            }
            _ if code => out.push(c),
            '<' => out += "&lt;",
            '>' => out += "&gt;",
            '&' => out += "&amp;",
            '*' | '_' | '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// A title as HTML: backticks make `<code>`.
fn html(s: &str) -> String {
    let (mut out, mut code) = (String::new(), false);
    for c in s.chars() {
        match c {
            '`' => {
                out += if code { "</code>" } else { "<code>" };
                code = !code;
            }
            '<' => out += "&lt;",
            '>' => out += "&gt;",
            '&' => out += "&amp;",
            _ => out.push(c),
        }
    }
    out
}

const GROUPS: &[(u16, &str)] = &[
    (0, "Characters and numbers"),
    (1, "Syntax"),
    (2, "Names"),
    (3, "Placement and levels"),
    (4, "Limits"),
    (5, "Layers"),
    (6, "Version"),
    (7, "The draw stage, paint and functions"),
    (8, "Frame rate"),
    (
        9,
        "Growing values, sprites, palettes, slow layers and declarations",
    ),
];

fn index() -> String {
    let mut s = String::new();
    for &(h, title) in GROUPS {
        let ds: Vec<_> = ERRORS.iter().filter(|d| d.code / 100 == h).collect();
        if ds.is_empty() {
            continue;
        }
        s += &format!("### {title} (E{h}xx)\n\n");
        for d in ds {
            let l = errors::label(d.code);
            let internal = reference::INTERNAL.contains(&d.code);
            let badge = if internal {
                " <span class=\"badge\">internal</span>"
            } else {
                ""
            };
            s += &format!(
                "<h4 id=\"{l}\">{l} · {}{badge}</h4>\n\n> {}\n\n",
                html(d.title),
                md(d.sentence)
            );
            if internal {
                s += "**Internal:** a bug in Psi: please report it, with the formula. It is never the formula's fault.\n\n";
            }
            if let Some(h) = reference::help(d.code) {
                s += &format!("{}\n\n", md(h.why));
            }
            s += &format!("{}\n\n", md(d.fix));
            if let Some(h) = reference::help(d.code).filter(|h| !h.before.is_empty()) {
                s += &format!(
                    "Before:\n\n```psi\n{}\n```\n\nAfter:\n\n```psi\n{}\n```\n\n",
                    h.before, h.after
                );
            }
        }
    }
    s
}

#[test]
fn the_error_index_in_the_language_reference_is_current() {
    let file = path("../docs/psi-spec.md");
    let text = std::fs::read_to_string(&file).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let a = lines
        .iter()
        .position(|l| l.starts_with("<!-- psi-errors (generated"))
        .expect("the start marker");
    let z = lines
        .iter()
        .position(|l| *l == "<!-- psi-errors end -->")
        .expect("the end marker");
    assert!(a < z);
    let want = index();
    let have = lines[a + 1..z].join("\n");
    if have.trim() == want.trim() {
        return;
    }
    if bless() {
        let mut out: Vec<String> = lines[..=a].iter().map(|l| l.to_string()).collect();
        out.push(want.trim_end().to_string());
        out.push(String::new());
        out.extend(lines[z..].iter().map(|l| l.to_string()));
        std::fs::write(&file, out.join("\n") + "\n").unwrap();
        return;
    }
    panic!("the error index in docs/psi-spec.md is stale: PSI_BLESS=1 cargo test --features docs --test diagnostics");
}

// --- the web app's code table (js/psierr.js) ---------------------------------

/// A JS string literal.
fn js(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o += "\\\\",
            '"' => o += "\\\"",
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `C`: each code, the bytes of its fragments in all (what `code_of`
/// weighs: UTF-8, as JS's lengths are not), its fragments.
fn js_codes() -> String {
    let mut s = String::from(
        "// psi-errors: codes (generated from opendmd-psi/src/errors.rs: PSI_BLESS=1 cargo test --features docs --test diagnostics)\nconst C = [\n",
    );
    for (k, d) in ERRORS.iter().enumerate() {
        let w: usize = d.matches.iter().map(|m| m.len()).sum();
        let f: Vec<String> = d.matches.iter().map(|m| js(m)).collect();
        let sep = if k + 1 == ERRORS.len() { "" } else { "," };
        s += &format!("  [{}, {w}, {}]{sep}\n", d.code, f.join(", "));
    }
    s += "];\n// psi-errors end\n";
    s
}

#[test]
fn the_web_apps_code_table_is_the_index() {
    let file = path("../opendmd-web/www/js/psierr.js");
    let text = std::fs::read_to_string(&file).unwrap();
    let a = text
        .find("// psi-errors: codes")
        .expect("js/psierr.js: the block's start");
    let end = "// psi-errors end\n";
    let z = a + text[a..].find(end).expect("its end") + end.len();
    let want = js_codes();
    if text[a..z] == want {
        return;
    }
    if bless() {
        std::fs::write(&file, format!("{}{want}{}", &text[..a], &text[z..])).unwrap();
        return;
    }
    panic!("js/psierr.js's code table is stale: PSI_BLESS=1 cargo test --features docs --test diagnostics");
}
