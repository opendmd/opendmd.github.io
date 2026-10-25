//! `docs/psi-for-models.md` (the reference for writing formulas with a
//! model) checked against the language: every ```psi block compiles, every
//! ```psi error block fails with the sentence in its first comment, and every
//! worked example (```psi example=<slug>) compiles, is light enough for the
//! panel, and draws the frames in `tests/models.golden`. After a deliberate
//! change: `PSI_BLESS=1 cargo test --test models` rewrites the goldens.

use opendmd_psi::golden::{fnv, inputs, picture};
use opendmd_psi::{compile, render_rows, Scratch, H, OWN, W};

const DOC: &str = include_str!("../../docs/psi-for-models.md");
const GOLDEN: &str = "tests/models.golden";

/// The doc's ```psi blocks: (info after `psi`, source, line).
fn blocks() -> Vec<(String, String, usize)> {
    let mut out = Vec::new();
    let mut cur: Option<(String, String, usize)> = None;
    for (n, line) in DOC.lines().enumerate() {
        match &mut cur {
            None => {
                if let Some(info) = line.strip_prefix("```psi") {
                    cur = Some((info.trim().to_string(), String::new(), n + 1));
                }
            }
            Some(b) => {
                if line.starts_with("```") {
                    out.extend(cur.take());
                } else {
                    b.1 += line;
                    b.1 += "\n";
                }
            }
        }
    }
    assert!(cur.is_none(), "an unclosed ```psi block");
    out
}

/// The frames a worked example is checked at: its start and 37 % into its
/// loop (whole 20 ms frames), at the golden inputs, over the golden picture
/// when it draws over what is under it.
fn frames(slug: &str, src: &str) -> Vec<(u32, u64)> {
    let p = compile(src).unwrap_or_else(|e| panic!("example {slug}: {}", e.render(src)));
    assert!(
        p.esp32_us() <= 8000,
        "example {slug}: ~{} µs a frame on the panel (keep examples ≤ 8 ms)",
        p.esp32_us()
    );
    let mut under = vec![0u8; W * H * 3];
    picture(&mut under);
    let mut s = Scratch::<128>::new();
    [0, p.loop_ms() * 37 / 100 / 20 * 20]
        .into_iter()
        .map(|ms| {
            let mut out = vec![0u8; W * H * 3];
            let u = p.over().then_some(&under[..]);
            render_rows(&p, &mut s, &inputs(ms), &OWN, u, u32::MAX, &mut out);
            (ms, fnv(&out))
        })
        .collect()
}

#[test]
fn the_reference_matches_the_language() {
    let mut examples = Vec::new();
    let mut errors = 0;
    for (info, src, line) in blocks() {
        if let Some(slug) = info.strip_prefix("example=") {
            examples.push((slug.to_string(), src));
        } else if info == "error" {
            let want = src
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("-- error: "))
                .unwrap_or_else(|| {
                    panic!("line {line}: an error block starts `-- error: <sentence>`")
                });
            match compile(&src) {
                Ok(_) => panic!("line {line}: compiled, but should fail with {want:?}"),
                Err(e) => assert!(
                    e.msg.contains(want),
                    "line {line}: got {:?}, want {want:?}",
                    e.msg
                ),
            }
            errors += 1;
        } else {
            assert!(info.is_empty(), "line {line}: unknown block kind {info:?}");
            compile(&src).unwrap_or_else(|e| panic!("line {line}: {}", e.render(&src)));
        }
    }
    assert!(
        (20..=30).contains(&examples.len()),
        "{} worked examples",
        examples.len()
    );
    assert!(errors >= 10, "{errors} common mistakes");

    let mut got = String::new();
    for (slug, src) in &examples {
        assert!(
            examples.iter().filter(|e| &e.0 == slug).count() == 1,
            "example {slug} twice"
        );
        for (ms, h) in frames(slug, src) {
            got += &format!("{slug} {ms} {h:#018x}\n");
        }
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/", "tests/models.golden");
    if std::env::var_os("PSI_BLESS").is_some() {
        std::fs::write(path, &got).unwrap();
        return;
    }
    let want =
        std::fs::read_to_string(path).unwrap_or_else(|_| panic!("{GOLDEN} missing: PSI_BLESS=1"));
    let diff: Vec<String> = got
        .lines()
        .zip(want.lines())
        .filter(|(a, b)| a != b)
        .map(|(a, b)| format!("got {a}, want {b}"))
        .collect();
    assert!(
        diff.is_empty() && got.lines().count() == want.lines().count(),
        "the worked examples' frames differ from {GOLDEN} (a deliberate change: PSI_BLESS=1):\n{}",
        diff.join("\n")
    );
}

/// Every built-in the reference's tables name is one the compiler knows.
#[test]
fn the_reference_names_every_built_in() {
    let values = [
        "x", "y", "px", "py", "i", "t", "hour", "minute", "second", "a", "b", "c", "w", "h", "ur",
        "ug", "ub", "ul",
    ];
    let funcs = [
        ("sin", 1),
        ("cos", 1),
        ("tri", 1),
        ("saw", 1),
        ("sqr", 1),
        ("abs", 1),
        ("floor", 1),
        ("frac", 1),
        ("fract", 1),
        ("sqrt", 1),
        ("min", 2),
        ("max", 2),
        ("clamp", 3),
        ("mix", 3),
        ("step", 2),
        ("smoothstep", 3),
        ("len", 2),
        ("atan2", 2),
        ("noise", 3),
        ("hash", 2),
        ("select", 3),
    ];
    for v in values {
        assert!(
            DOC.contains(&format!("`{v}`")),
            "the reference doesn't name `{v}`"
        );
        compile(&format!("gray({v})")).unwrap();
    }
    for (f, n) in funcs {
        assert!(
            DOC.contains(&format!("`{f}(")),
            "the reference doesn't name `{f}(…)`"
        );
        let args = vec!["x"; n].join(", ");
        compile(&format!("gray({f}({args}))")).unwrap_or_else(|e| panic!("{f}: {}", e.msg));
    }
    for p in ["fire", "ice", "ocean", "amber", "rainbow", "chrono", "gray"] {
        assert!(DOC.contains(&format!("`{p}`")), "palette {p}");
        compile(&format!("pal({p}, x)")).unwrap();
    }
    for b in ["over", "add", "multiply", "screen", "max", "min"] {
        compile(&format!("layer {b} {{ gray(x) }}")).unwrap();
    }
}

/// The reference's cost table is the estimate's (`Program::esp32_us`): each
/// row's nanoseconds, a pixel, are what one more such step adds to it.
#[test]
fn the_cost_table_is_the_estimates() {
    let us = |src: &str| {
        compile(src)
            .unwrap_or_else(|e| panic!("{src}: {}", e.msg))
            .esp32_us() as i64
    };
    // (the row as the table prints it, ns, a formula, the same with one more step)
    let rows: &[(&str, i64, &str, &str)] = &[
        (
            "| + − compare min max step and or `%`2ⁿ | 50 |",
            50,
            "gray(x*px)",
            "gray(x*px + x)",
        ),
        ("| `*` | 86 |", 86, "gray(x)", "gray(x*px)"),
        ("| `%` (other) | 113 |", 113, "gray(px)", "gray(px % 3)"),
        (
            "| neg abs floor frac not | 40 |",
            40,
            "gray(x*px)",
            "gray(abs(x*px))",
        ),
        (
            "| `/` by a varying value, `1/v` | 685 |",
            685,
            "gray(x)",
            "gray(x / px)",
        ),
        ("| `sqrt` | 539 |", 539, "gray(x)", "gray(sqrt(x))"),
        ("| `len` | 642 |", 642, "gray(x)", "gray(len(x, px))"),
        ("| `sin` `cos` | 110 |", 110, "gray(x)", "gray(sin(x))"),
        (
            "| `tri` `saw` `sqr` `select` `clamp` | 60 |",
            60,
            "gray(x)",
            "gray(tri(x))",
        ),
        ("| `mix` | 130 |", 130, "gray(x)", "gray(mix(x, px, x))"),
        (
            "| `smoothstep` | 214 ",
            214,
            "gray(x)",
            "gray(smoothstep(0.2, 0.8, x))",
        ),
        ("| `hash` | 220 |", 220, "gray(x)", "gray(hash(px, x))"),
        ("| `atan2` | 529 |", 529, "gray(x)", "gray(atan2(x, px))"),
        ("| `noise` | 855 |", 855, "gray(x)", "gray(noise(x, px, x))"),
        ("| colour `rgb` | 120 |", 120, "gray(x)", "rgb(x, x, x)"),
        ("| colour `hsv` | 195 |", 195, "gray(x)", "hsv(x, x, x)"),
        ("| `ur` `ug` `ub` | 35 |", 35, "gray(x)", "gray(ur)"),
        ("| `ul` | 125 |", 125, "gray(x)", "gray(ul)"),
        (
            "| each extra layer | 150 (multiply 210, screen 290; +75 with coverage) |",
            150,
            "gray(x)",
            "layer { gray(x) }\nlayer add { gray(px) }",
        ),
        (
            "multiply 210",
            210,
            "gray(x)",
            "layer { gray(x) }\nlayer multiply { gray(px) }",
        ),
        (
            "screen 290",
            290,
            "gray(x)",
            "layer { gray(x) }\nlayer screen { gray(px) }",
        ),
        (
            "+75 with coverage",
            225,
            "gray(x)",
            "layer { gray(x) }\nlayer add { gray(px, x) }",
        ),
    ];
    for &(row, ns, base, more) in rows {
        assert!(
            DOC.contains(row),
            "the reference's cost table has no {row:?}"
        );
        let got = us(more) - us(base);
        let want = ns * 4096 / 1000;
        assert!(
            (got - want).abs() <= 1,
            "{row}: one more step adds {got} µs, the table says {ns} ns a pixel ({want} µs)"
        );
    }
}
