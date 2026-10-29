use super::*;

const GOOD: &str = "-- author: @someone\n-- licence: CC0-1.0\nname \"Rainbow rings\"\nabout \"Rings rippling out from the middle\"\nloop 4\nd = len((x - 0.5)*4, y - 0.5)\npal(rainbow, frac(d*1.5 - t/2))\n";

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
    assert_eq!(e.about, "Rings rippling out from the middle");
    assert_eq!(
        e.src,
        "name \"Rainbow rings\"\nabout \"Rings rippling out from the middle\"\nloop 4\nd = len((x - 0.5)*4, y - 0.5)\npal(rainbow, frac(d*1.5 - t/2))\n"
    );
    assert_eq!(e.loop_ms, 4000);
    assert!(e.us > 0 && e.us <= MAX_US);
    assert!(!e.bundle && e.look.is_none() && e.controls.is_empty());
    let png = thumbnail(&e);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(sheet(&e).len() > png.len());
}

#[test]
fn declarations_and_license_spelling() {
    // Controls (their defaults are where the sliders start), a look line.
    let t = GOOD.replace(
        "-- licence: CC0-1.0\n",
        "-- license: CC-BY-4.0\n",
    ).replace(
        "loop 4\n",
        "control width 0..1 = 0.3 \"Ring width\"\ncontrol mode = soft, hard\nlook screen 40\nloop 4\n",
    ).replace("frac(d*1.5 - t/2)", "frac(d*1.5 - t/2 + width + mode + c)");
    let e = check_file("x1", t.as_bytes()).unwrap();
    assert_eq!(e.licence, "CC-BY-4.0");
    assert_eq!(e.controls, ["Ring width", "mode", "c"]);
    assert_eq!(e.look, Some(("screen", 40)));
    assert!(e.src.starts_with("name \"Rainbow rings\"\n"));
    let j = json(&e);
    assert!(j.contains("\"look\":[\"screen\",40],\"controls\":[\"Ring width\",\"mode\",\"c\"]"), "{j}");
    // At their defaults: as a slider at 0.3 draws it.
    let p = compile(&e.src).unwrap();
    let at = compile(&e.src.replace("control width 0..1 = 0.3", "control width 0..1 = 0")).unwrap();
    assert_ne!(frame(&p, 0), frame(&at, 0));
    // The old header (name, description, values) says what changed.
    let old = "-- name: Rainbow rings\n-- description: Rings\n-- author: @someone\n-- licence: CC0-1.0\ngray(x)\n";
    assert!(has(&problems("x", old), "the header is two lines now"));
    // `-- bundle`: OpenDMD's examples only.
    let b = GOOD.replace("-- licence: CC0-1.0\n", "-- licence: CC0-1.0\n-- bundle\n");
    assert!(has(&problems("x", &b), "is for the examples OpenDMD ships"));
    let e = check_file_as("x", b.as_bytes(), true).unwrap();
    assert!(e.bundle && e.src.starts_with("name"));
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
        "must start with two lines"
    ));
    let swapped = GOOD.replace(
        "-- author: @someone\n-- licence: CC0-1.0\n",
        "-- licence: CC0-1.0\n-- author: @someone\n",
    );
    assert!(has(&problems("x", &swapped), "must start with two lines"));
    // The formula's own name and about lines.
    let nameless = GOOD.replace("name \"Rainbow rings\"\n", "");
    assert!(has(&problems("x", &nameless), "needs a name"));
    let aboutless = GOOD.replace("about \"Rings rippling out from the middle\"\n", "");
    assert!(has(&problems("x", &aboutless), "needs a line about it"));
    // Limits (the language's own: 60 and 120; the gallery's name: 40).
    let long = GOOD.replace("Rainbow rings", &"n".repeat(41));
    assert!(has(&problems("x", &long), "the name is 41 characters"));
    let long = GOOD.replace("Rings rippling out from the middle", &"d".repeat(121));
    assert!(has(&problems("x", &long), "doesn't compile"));
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
    // HTML-ish text: the header's own check, the language's in quotes.
    let ps = problems("x", &GOOD.replace("@someone", "a>b"));
    assert!(has(&ps, "may not contain < or >"), "{ps:?}");
    for (k, v) in [
        ("Rainbow rings", "<img src=x onerror=alert(1)>"),
        ("Rings rippling out from the middle", "<b>"),
    ] {
        let ps = problems("x", &GOOD.replace(k, v));
        assert!(has(&ps, "doesn't compile"), "{v}: {ps:?}");
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
        let t = GOOD.replace("@someone", &format!("@some{c}one"));
        let ps = check_file("x", t.as_bytes()).expect_err(&format!("U+{:04X}", c as u32));
        assert!(has(&ps, "line 1"), "U+{:04X}: {ps:?}", c as u32);
        // In the name: refused (by the checker, or the language's quotes).
        let t = GOOD.replace("Rainbow rings", &format!("Rainbow{c}rings"));
        assert!(check_file("x", t.as_bytes()).is_err(), "U+{:04X}", c as u32);
    }
    // A tab is fine in the formula, not in the header.
    assert!(check_file("x", GOOD.replace("d = len", "d =\tlen").as_bytes()).is_ok());
    assert!(has(
        &problems("x", &GOOD.replace("@someone", "@some\tone")),
        "control"
    ));
    // Not text.
    assert!(has(
        &check_file("x", b"\xff\xfe-- name").unwrap_err(),
        "not plain text"
    ));
    assert!(has(
        &check_file("x", b"-- author: a\0b").unwrap_err(),
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
    // (Its own lines are counted from the file's top, header included.)
    // Heavy: noise and divisions per pixel.
    let heavy = GOOD.replace(
        "pal(rainbow, frac(d*1.5 - t/2))",
        "n = noise(x*8, y*4, t) + noise(x*4, y*8, t) + noise(x*2, y*2, t)\nm = 1/(d + 0.1) + 1/(x + 0.2) + 1/(y + 0.3)\npal(fire, frac(n + m))",
    );
    assert!(has(&problems("x", &heavy), "too heavy"));
    let none = "-- author: c\n-- licence: CC0-1.0\n-- only a comment\n";
    assert!(has(&problems("x", none), "no formula"));
}

#[test]
fn json_and_markdown_escape() {
    let e = check_file("rainbow-rings", GOOD.as_bytes()).unwrap();
    let j = json(&e);
    assert!(j.starts_with("{\"slug\":\"rainbow-rings\",\"name\":\"Rainbow rings\",\"about\":\"Rings rippling out from the middle\""));
    assert!(j.contains("\"bundle\":false,\"look\":null,\"controls\":[]"), "{j}");
    assert!(j.contains("\\n"));
    assert_eq!(js("a<b>&\"\\"), "\"a\\u003cb\\u003e\\u0026\\\"\\\\\"");
    assert_eq!(
        md("*x* [y](z) <b>"),
        "&#42;x&#42; &#91;y&#93;&#40;z&#41; &#60;b&#62;"
    );
    assert!(!md("a\u{202E}b\n").contains('\u{202E}'));
}

/// Every example (`opendmd-web/formulas/*.psi`) passes as the site build
/// checks it (trusted: author OpenDMD), ten of them `-- bundle`, and every
/// one that draws over a GIF says it can be the look.
#[test]
fn the_examples_pass() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../opendmd-web/formulas");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "psi"))
        .collect();
    files.sort();
    let (mut n, mut over, mut bundled) = (0, 0, 0);
    for p in &files {
        let slug = p.file_stem().unwrap().to_str().unwrap();
        check_slug(slug).unwrap();
        let e = check_file_as(slug, &std::fs::read(p).unwrap(), true)
            .unwrap_or_else(|ps| panic!("{slug}: {ps:?}"));
        assert_eq!(e.author, "OpenDMD");
        // Its thumbnail is lit (as site-smoke checks the gallery page: some
        // pixel's r + g + b over 90); one that draws over a GIF is drawn
        // over the sample picture, not black.
        let prog = compile(&e.src).unwrap();
        let f = frame(&prog, thumb_ms(e.loop_ms));
        assert!(
            f.chunks(3)
                .any(|p| p.iter().map(|&v| u32::from(v)).sum::<u32>() > 90),
            "{slug}: its thumbnail is dark"
        );
        assert_eq!(prog.over(), e.look.is_some(), "{slug}: over a GIF, and a look line");
        over += usize::from(prog.over());
        bundled += usize::from(e.bundle);
        n += 1;
    }
    assert_eq!((n, bundled), (33, 10));
    assert!(over >= 8, "the examples that draw over a GIF: {over}");
}

/// A formula that draws over a GIF is previewed over the sample picture
/// (the web app's, byte for byte as its js/psikit.js draws it); one that
/// doesn't, over nothing.
#[test]
fn over_the_sample() {
    let s = sample();
    assert_eq!(s.len(), W * H * 3);
    // A pixel of each part: the sky (top left), the sun, the hills.
    assert_eq!(&s[..3], &[40, 60, 170]);
    assert_eq!(&s[(10 * W + 98) * 3..][..3], &[255, 215, 80]);
    assert_eq!(s[(31 * W) * 3], 30);
    let shown = compile("rgb(ur, ug, ub)").unwrap();
    assert!(shown.over());
    let f = frame(&shown, 0);
    let close = f.iter().zip(&s).all(|(a, b)| a.abs_diff(*b) <= 1);
    assert!(close, "rgb(ur, ug, ub) is the sample");
    let own = compile("rgb(0, 0, 0)").unwrap();
    assert!(!own.over());
    assert!(frame(&own, 0).iter().all(|&v| v == 0));
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
