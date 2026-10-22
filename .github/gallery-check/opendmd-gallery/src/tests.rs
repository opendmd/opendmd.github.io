use super::*;

const GOOD: &str = "-- name: Rainbow rings\n-- description: Rings rippling out from the middle\n-- author: @someone\n-- licence: CC0-1.0\nloop 4\nd = len((x - 0.5)*4, y - 0.5)\npal(rainbow, frac(d*1.5 - t/2))\n";

fn problems(slug: &str, text: &str) -> Vec<String> {
    check_file(slug, text.as_bytes()).expect_err("should fail")
}

fn has(ps: &[String], word: &str) -> bool {
    ps.iter().any(|p| p.contains(word))
}

#[test]
fn a_good_file() {
    let e = check_file("rainbow-rings", GOOD.as_bytes()).unwrap();
    assert_eq!(e.name, "Rainbow rings");
    assert_eq!(e.author, "@someone");
    assert_eq!(e.licence, "CC0-1.0");
    assert_eq!(
        e.src,
        "loop 4\nd = len((x - 0.5)*4, y - 0.5)\npal(rainbow, frac(d*1.5 - t/2))\n"
    );
    assert_eq!(e.loop_ms, 4000);
    assert!(e.us > 0 && e.us <= MAX_US);
    assert_eq!(e.values, [None; 3]);
    let png = thumbnail(&e);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(sheet(&e).len() > png.len());
}

#[test]
fn values_and_license_spelling() {
    let t = GOOD.replace(
        "-- licence: CC0-1.0\n",
        "-- license: CC-BY-4.0\n-- values: a=0.3 c=1\n",
    );
    let e = check_file("x1", t.as_bytes()).unwrap();
    assert_eq!(e.licence, "CC-BY-4.0");
    assert_eq!(e.values, [Some(30), None, Some(100)]);
    assert!(e.src.starts_with("loop 4"));
    for bad in [
        "a=1.5",
        "a=0.333",
        "d=0.1",
        "a=0.1 a=0.2",
        "a=.5",
        "a=",
        "a=0.1  b=0.2",
    ] {
        let t = GOOD.replace(
            "-- licence: CC0-1.0\n",
            &format!("-- licence: CC0-1.0\n-- values: {bad}\n"),
        );
        assert!(has(&problems("x", &t), "values are like"), "{bad}");
    }
    assert_eq!(hundredths(30), "0.3");
    assert_eq!(hundredths(5), "0.05");
    assert_eq!(hundredths(100), "1");
    assert_eq!(hundredths(0), "0");
}

#[test]
fn paths() {
    assert_eq!(
        check_path("gallery/rainbow-rings.psi").unwrap(),
        "rainbow-rings"
    );
    for bad in [
        "gallery/../x.psi",
        "gallery/sub/x.psi",
        "x.psi",
        "gallery/X.psi",
        "gallery/a_b.psi",
        "gallery/-a.psi",
        "gallery/a-.psi",
        "gallery/a--b.psi",
        "gallery/.psi",
        "gallery/readme.psi",
        "gallery/x.txt",
        "gallery/README.md",
        ".github/workflows/x.yml",
        "gallery/ünï.psi",
        "gallery/a b.psi",
    ] {
        assert!(check_path(bad).is_err(), "{bad}");
    }
    assert!(check_path(&format!("gallery/{}.psi", "a".repeat(49))).is_err());
    assert!(check_path(&format!("gallery/{}.psi", "a".repeat(48))).is_ok());
}

#[test]
fn header_rules() {
    // Missing or out of order.
    assert!(has(
        &problems("x", "loop 4\ngray(x)\n"),
        "must start with four lines"
    ));
    let swapped = GOOD.replace(
        "-- name: Rainbow rings\n-- description: Rings rippling out from the middle\n",
        "-- description: Rings rippling out from the middle\n-- name: Rainbow rings\n",
    );
    assert!(has(&problems("x", &swapped), "must start with four lines"));
    // Limits.
    let long = GOOD.replace("Rainbow rings", &"n".repeat(41));
    assert!(has(&problems("x", &long), "the name is 41 characters"));
    let long = GOOD.replace("Rings rippling out from the middle", &"d".repeat(121));
    assert!(has(&problems("x", &long), "the description is 121"));
    let long = GOOD.replace("@someone", &"a".repeat(41));
    assert!(has(&problems("x", &long), "the author is 41"));
    let empty = GOOD.replace("-- author: @someone", "-- author:");
    assert!(has(&problems("x", &empty), "the author is empty"));
    // Accents are fine in a name (and counted as characters).
    let e = check_file(
        "moire",
        GOOD.replace("Rainbow rings", "Moiré ✨ ".trim()).as_bytes(),
    )
    .unwrap();
    assert_eq!(e.name, "Moiré ✨");
    // HTML-ish text.
    for (k, v) in [
        ("Rainbow rings", "<img src=x onerror=alert(1)>"),
        ("@someone", "a>b"),
        ("Rings rippling out from the middle", "<b>"),
    ] {
        let ps = problems("x", &GOOD.replace(k, v));
        assert!(has(&ps, "may not contain < or >"), "{v}: {ps:?}");
    }
    // Licence allow-list.
    for l in ["MIT", "GPL-3.0", "cc0-1.0", "CC0", "CC-BY-SA-4.0", ""] {
        let ps = problems("x", &GOOD.replace("CC0-1.0", l));
        assert!(
            has(&ps, "licence must be one of") || has(&ps, "must start"),
            "{l}: {ps:?}"
        );
    }
    let spaces = GOOD.replace("Rainbow rings", "Rainbow  rings");
    assert!(has(&problems("x", &spaces), "extra spaces"));
}

#[test]
fn hostile_characters() {
    // Bidi overrides (Trojan Source), in the header and in the formula.
    let rlo = GOOD.replace("Rainbow rings", "Rainbow \u{202E}sgnir");
    assert!(has(&problems("x", &rlo), "direction"));
    let in_code = GOOD.replace("loop 4\n", "loop 4\n-- \u{2067}hidden\u{2069}\n");
    assert!(has(&problems("x", &in_code), "direction"));
    for c in [
        '\u{200B}',
        '\u{FEFF}',
        '\u{00AD}',
        '\u{2060}',
        '\u{E0041}',
        '\u{E000}',
        '\u{0007}',
        '\u{001B}',
        '\u{0085}',
        '\r',
        '\u{2028}',
    ] {
        let t = GOOD.replace("Rainbow rings", &format!("Rainbow{c}rings"));
        let ps = check_file("x", t.as_bytes()).expect_err(&format!("U+{:04X}", c as u32));
        assert!(has(&ps, "line 1"), "U+{:04X}: {ps:?}", c as u32);
    }
    // A tab is fine in the formula, not in the header.
    assert!(check_file("x", GOOD.replace("d = len", "d =\tlen").as_bytes()).is_ok());
    assert!(has(
        &problems("x", &GOOD.replace("Rainbow rings", "Rainbow\trings")),
        "control"
    ));
    // Not text.
    assert!(has(
        &check_file("x", b"\xff\xfe-- name").unwrap_err(),
        "not plain text"
    ));
    assert!(has(
        &check_file("x", b"-- name: a\0b").unwrap_err(),
        "not plain text"
    ));
}

#[test]
fn size_compile_and_cost() {
    let big = format!("{GOOD}{}", "-- padding\n".repeat(400));
    assert!(big.len() > MAX_FILE);
    assert!(has(&check_file("x", big.as_bytes()).unwrap_err(), "bytes"));
    let broken = GOOD.replace("pal(rainbow, frac(d*1.5 - t/2))", "pal(rainbow, frac(tme))");
    let ps = problems("x", &broken);
    assert!(has(&ps, "line 7: the formula doesn't compile"), "{ps:?}");
    // Heavy: noise and divisions per pixel.
    let heavy = GOOD.replace(
        "pal(rainbow, frac(d*1.5 - t/2))",
        "n = noise(x*8, y*4, t) + noise(x*4, y*8, t) + noise(x*2, y*2, t)\nm = 1/(d + 0.1) + 1/(x + 0.2) + 1/(y + 0.3)\npal(fire, frac(n + m))",
    );
    assert!(has(&problems("x", &heavy), "too heavy"));
    let none =
        "-- name: a\n-- description: b\n-- author: c\n-- licence: CC0-1.0\n-- only a comment\n";
    assert!(has(&problems("x", none), "no formula"));
}

#[test]
fn json_and_markdown_escape() {
    let e = check_file("rainbow-rings", GOOD.as_bytes()).unwrap();
    let j = json(&e);
    assert!(j.starts_with("{\"slug\":\"rainbow-rings\",\"name\":\"Rainbow rings\""));
    assert!(j.contains("\\n"));
    assert_eq!(js("a<b>&\"\\"), "\"a\\u003cb\\u003e\\u0026\\\"\\\\\"");
    assert_eq!(
        md("*x* [y](z) <b>"),
        "&#42;x&#42; &#91;y&#93;&#40;z&#41; &#60;b&#62;"
    );
    assert!(!md("a\u{202E}b\n").contains('\u{202E}'));
}

/// Every example in the web app's formulas.json passes, as the site build
/// writes it (author OpenDMD, CC0).
#[test]
fn the_examples_pass() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../opendmd-web/www/formulas.json"
    ))
    .unwrap();
    let mut n = 0;
    for line in text.lines().filter(|l| l.starts_with('{')) {
        let name = string_field(line, "name");
        let desc = string_field(line, "desc");
        let src = string_field(line, "src");
        let file = format!("-- name: {name}\n-- description: {desc}\n-- author: OpenDMD\n-- licence: CC0-1.0\n{src}\n");
        let e = check_file_as("example", file.as_bytes(), true)
            .unwrap_or_else(|p| panic!("{name}: {p:?}"));
        assert_eq!(e.src, format!("{src}\n"));
        n += 1;
    }
    assert_eq!(n, 16);
}

/// A JSON string field, the simple way (the file is one object a line).
fn string_field(line: &str, key: &str) -> String {
    let start = line.find(&format!("\"{key}\": \"")).unwrap() + key.len() + 5;
    let mut out = String::new();
    let mut chars = line[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next().unwrap() {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
    out
}

#[test]
fn png_crc() {
    assert_eq!(png::crc32(b"IEND"), 0xAE42_6082);
}

#[test]
fn reserved_names_zalgo_braille_crlf() {
    for (k, v) in [
        ("Rainbow rings", "OpenDMD"),
        ("@someone", "@duggan"),
        ("@someone", "Ｏｐｅｎ ＤＭＤ"),
        ("@someone", "0pen-DMD"),
        ("Rainbow rings", "Opеndmd"),
        ("@someone", "dúggan"),
    ] {
        let ps = problems("x", &GOOD.replace(k, v));
        assert!(has(&ps, "OpenDMD's or a maintainer's"), "{v}: {ps:?}");
    }
    let mine = GOOD.replace("@someone", "OpenDMD");
    assert!(check_file_as("x", mine.as_bytes(), true).is_ok());
    assert!(check_file("x", GOOD.replace("@someone", "OpenDMD fan").as_bytes()).is_ok());
    // Two accents on a letter are fine, three are not.
    assert!(check_file(
        "x",
        GOOD.replace("Rainbow", "Rai\u{301}\u{308}nbow").as_bytes()
    )
    .is_ok());
    assert!(has(
        &problems(
            "x",
            &GOOD.replace("Rainbow", "Rai\u{301}\u{308}\u{30A}nbow")
        ),
        "accents on one letter"
    ));
    assert!(has(
        &problems("x", &GOOD.replace("Rainbow rings", "Rainbow\u{2800}rings")),
        "invisible"
    ));
    let crlf = GOOD.replace('\n', "\r\n");
    let ps = problems("x", &crlf);
    assert!(
        has(&ps, "Windows line endings") && !has(&ps, "control"),
        "{ps:?}"
    );
    assert_eq!(skeleton("Ｏｐｅｎ ＤＭＤ!"), "opendmd");
}
