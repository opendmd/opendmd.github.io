//! The formula gallery's checker.
//!
//! The public gallery is a folder of plain-text files in the public site
//! repo (`gallery/<slug>.psi`), each a header of two comment lines and then
//! a formula in Psi (`opendmd-psi`) that says its own name and what it is:
//!
//! ```text
//! -- author: OpenDMD
//! -- licence: CC0-1.0
//! name "Plasma"
//! about "The demoscene classic: three waves, every colour"
//! control speed 0..1 = 0.3 "Speed"
//! loop 8
//! wave = sin(x*2 + t/8) + sin(y*1.5 - t/4)
//! hsv(wave/4 + t/8 + speed, 0.9, 1)
//! ```
//!
//! The two header lines are required, in that order (`license` is accepted
//! for `licence`); OpenDMD's own examples (`opendmd-web/formulas/`, checked
//! `trusted`) may add a third, `-- bundle`: the web app ships that one. The
//! formula's `name "…"` and `about "…"` lines are required too; they, its
//! `control` lines (where the sliders start: their defaults) and its `look`
//! line are read from the compiler ([`compile_declared`]), so the gallery,
//! the web app and the panel read them the same way. (Until formula-apps
//! F3 the header carried `-- name:`, `-- description:` and `-- values:`.)
//! The header lines are Psi comments, so the whole file is a formula too.
//!
//! [`check_path`] and [`check_file`] are everything a contribution must
//! pass: the path (one flat `gallery/<slug>.psi`), the size, the text (UTF-8
//! without control, invisible or direction-changing characters; no `<` or
//! `>` in the header), the header's fields and limits, the licence, that it
//! compiles, and its cost on the panel (`Program::esp32_us_still`). The checker
//! never runs anything but the Psi VM, which has no I/O: a formula is
//! arithmetic over the pixel and the time.
//!
//! The same checks run in three places: the public repo's pull-request
//! workflow (on the contribution, as data), `tools/site.py` at every site
//! build (on the merged files, before anything is published), and the
//! tests here. `main.rs` is the command line.

use opendmd_psi::{compile, compile_declared, render_rows_ink, Inputs, Program, Scratch, H, OWN, W};

pub mod png;

/// A gallery file's size limit, bytes.
pub const MAX_FILE: usize = 4096;
/// The header's limits, in characters.
pub const MAX_NAME: usize = 40;
pub const MAX_DESCRIPTION: usize = 120;
pub const MAX_AUTHOR: usize = 40;
/// A slug's length, characters (the file name without `.psi`).
pub const MAX_SLUG: usize = 48;
/// The licences a contribution may carry (SPDX ids). CC0 is the default.
pub const LICENCES: [&str; 2] = ["CC0-1.0", "CC-BY-4.0"];
/// The most a formula may cost on the panel, estimated µs a frame. 8,000
/// until the estimate was calibrated on the device (2026-10-26, any-look),
/// which reads the same formulas ~10-25 % higher: the heaviest examples
/// (Matrix, Plasma letters) are 8.9-9.7 ms now, as they always ran.
pub const MAX_US: u32 = 10_000;
/// The clock a preview is drawn at (formulas that follow it show 10:09:30).
pub const CLOCK: (u8, u8, u8) = (10, 9, 30);
/// The panel's frame time: `t` is a whole number of these.
pub const FRAME_MS: u32 = 20;

/// A gallery entry that passed every check.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub slug: String,
    /// The formula's `name "…"` and `about "…"` lines' text.
    pub name: String,
    pub about: String,
    pub author: String,
    pub licence: String,
    /// `-- bundle` (OpenDMD's examples only): the web app ships it.
    pub bundle: bool,
    /// Its `look` line: the blend and amount (`look` alone is over 100).
    pub look: Option<(&'static str, u8)>,
    /// Its controls' labels, slot a, b, c in order (a slot read without a
    /// `control` line: its letter).
    pub controls: Vec<String>,
    /// The formula without the header (what a share link carries).
    pub src: String,
    /// Steps, loop length (ms), the inputs it reads (bits 0-2 the sliders
    /// a b c, 3 t, 4 the clock) and the estimated µs a frame on the panel.
    pub steps: usize,
    pub loop_ms: u32,
    pub inputs: u8,
    pub us: u32,
}

/// The slug of a gallery path, `gallery/<slug>.psi`: lower-case letters and
/// digits in groups joined by single hyphens, at most [`MAX_SLUG`]; nothing
/// in a subfolder; `readme` is taken.
pub fn check_path(path: &str) -> Result<String, String> {
    let name = path
        .strip_prefix("gallery/")
        .ok_or_else(|| "only files in gallery/ can be added".to_string())?;
    let slug = name
        .strip_suffix(".psi")
        .ok_or_else(|| "a gallery file's name ends in .psi".to_string())?;
    if slug.contains('/') {
        return Err("gallery files go straight in gallery/, not in a folder".into());
    }
    check_slug(slug)?;
    Ok(slug.to_string())
}

/// A slug on its own (see [`check_path`]).
pub fn check_slug(slug: &str) -> Result<(), String> {
    let ok_chars = slug
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if slug.is_empty()
        || slug.len() > MAX_SLUG
        || !ok_chars
        || slug.starts_with('-')
        || slug.ends_with('-')
        || slug.contains("--")
    {
        return Err(format!(
            "the file name must be lower-case letters, digits and single hyphens, at most {MAX_SLUG} long, then .psi (like rainbow-rings.psi)"
        ));
    }
    if slug == "readme" || slug == "index" {
        return Err("that file name is taken".into());
    }
    Ok(())
}

/// Why a character may not appear: controls (but a tab in the formula),
/// and the invisible, joining and direction-changing ones a reviewer could
/// not see (the "Trojan Source" characters among them), private-use and
/// non-characters. `None` = allowed.
pub fn bad_char(c: char, in_formula: bool) -> Option<&'static str> {
    let u = c as u32;
    match u {
        0x09 if in_formula => None,
        0x00..=0x1F | 0x7F..=0x9F => Some("a control character"),
        0x202A..=0x202E | 0x2066..=0x2069 | 0x200E | 0x200F | 0x061C => {
            Some("a character that changes the text's direction")
        }
        0x00AD
        | 0x034F
        | 0x115F
        | 0x1160
        | 0x17B4
        | 0x17B5
        | 0x180B..=0x180E
        | 0x200B..=0x200D
        | 0x2060..=0x2065
        | 0x206A..=0x206F
        | 0x2800
        | 0x3164
        | 0xFE00..=0xFE0F
        | 0xFEFF
        | 0xFFA0
        | 0xE0000..=0xE0FFF => Some("an invisible character"),
        0x2000..=0x200A | 0x2028..=0x2029 | 0x202F | 0x205F | 0x3000 | 0x00A0 => {
            Some("an unusual space or line break")
        }
        0xFFF0..=0xFFFF => Some("a special character"),
        0xE000..=0xF8FF | 0xF0000..=0x10FFFF => Some("a private-use character"),
        _ => None,
    }
}

/// Combining marks (the common blocks of Unicode's Mn): more than
/// [`MAX_MARKS`] on one letter is "Zalgo" text, drawn over the lines around it.
pub fn is_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5
        | 0x05C7 | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4
        | 0x06E7..=0x06E8 | 0x06EA..=0x06ED | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0E31 | 0x0E34..=0x0E3A
        | 0x0E47..=0x0E4E | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0x302A..=0x302F
        | 0x3099..=0x309A | 0xFE20..=0xFE2F)
}
/// The most combining marks one character may carry.
pub const MAX_MARKS: usize = 2;

/// Names a contribution's name or author may not pass for: the project and
/// its maintainers (keep in step with site/public-repo/.github/CODEOWNERS).
pub const RESERVED: [&str; 2] = ["opendmd", "duggan"];

/// A rough "looks like" form of a name, for [`RESERVED`]: fullwidth letters
/// as ASCII, lower case, accents dropped, common Cyrillic and Greek
/// lookalikes as their Latin letters, 0 as o and 1 as l, and nothing but
/// letters and digits (an approximation of NFKC + casefold + confusables,
/// without the tables).
pub fn skeleton(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let c = match c as u32 {
            0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        };
        if is_mark(c) {
            continue;
        }
        for c in c.to_lowercase() {
            let c = match c {
                'а' => 'a',
                'в' => 'b',
                'с' => 'c',
                'ԁ' => 'd',
                'е' | 'ё' => 'e',
                'һ' => 'h',
                'і' | 'ї' => 'i',
                'ј' => 'j',
                'к' => 'k',
                'м' => 'm',
                'н' => 'h',
                'о' => 'o',
                'р' => 'p',
                'ԛ' => 'q',
                'ѕ' => 's',
                'т' => 't',
                'ц' => 'u',
                'ѵ' => 'v',
                'ԝ' => 'w',
                'х' => 'x',
                'у' => 'y',
                'α' => 'a',
                'β' => 'b',
                'ε' => 'e',
                'η' => 'n',
                'ι' => 'i',
                'κ' => 'k',
                'μ' => 'm',
                'ν' => 'v',
                'ο' => 'o',
                'ρ' => 'p',
                'τ' => 't',
                'υ' => 'u',
                'χ' => 'x',
                'ɡ' => 'g',
                'ı' => 'i',
                'ℓ' => 'l',
                '0' => 'o',
                '1' => 'l',
                'à'..='å' => 'a',
                'è'..='ë' => 'e',
                'ì'..='ï' => 'i',
                'ò'..='ö' => 'o',
                'ù'..='ü' => 'u',
                'ñ' => 'n',
                'ç' => 'c',
                c => c,
            };
            if c.is_ascii_alphanumeric() {
                out.push(c);
            }
        }
    }
    out
}

/// Check a field of the header.
fn field(problems: &mut Vec<String>, what: &str, v: &str, max: usize) {
    let n = v.chars().count();
    if n == 0 {
        problems.push(format!("the {what} is empty"));
    } else if n > max {
        problems.push(format!("the {what} is {n} characters: {max} at most"));
    }
    if v.contains('<') || v.contains('>') {
        problems.push(format!("the {what} may not contain < or >"));
    }
    if v != v.trim() || v.contains("  ") {
        problems.push(format!("the {what} has extra spaces"));
    }
}

/// A header line `-- <key>: <value>`, for any of `keys`.
fn header<'a>(line: Option<&'a str>, keys: &[&str]) -> Option<&'a str> {
    let rest = line?.strip_prefix("-- ")?;
    keys.iter()
        .find_map(|k| rest.strip_prefix(k)?.strip_prefix(':'))
        .map(|v| v.trim())
}

/// Check a whole gallery file (its slug from [`check_path`]): every
/// problem, or the entry.
pub fn check_file(slug: &str, bytes: &[u8]) -> Result<Entry, Vec<String>> {
    check_file_as(slug, bytes, false)
}

/// [`check_file`]; `trusted` (OpenDMD's own examples, at the site build)
/// lets the name or author be one of the [`RESERVED`], and the header carry
/// `-- bundle`.
pub fn check_file_as(slug: &str, bytes: &[u8], trusted: bool) -> Result<Entry, Vec<String>> {
    if bytes.len() > MAX_FILE {
        return Err(vec![format!(
            "the file is {} bytes: {MAX_FILE} at most",
            bytes.len()
        )]);
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => return Err(vec!["the file is not plain text (UTF-8)".into()]),
    };
    if text.contains('\0') {
        return Err(vec!["the file is not plain text".into()]);
    }
    let mut problems = Vec::new();
    let lines: Vec<&str> = text.split('\n').collect();
    let line = |i: usize| lines.get(i).copied();
    let bundle = line(2).is_some_and(|l| l.trim_end() == "-- bundle");
    let skip = if bundle { 3 } else { 2 };
    // Characters: where and what (each kind once); a tab only in the formula.
    let mut seen: Vec<&str> = Vec::new();
    let crlf = text.contains("\r\n");
    if crlf {
        problems.push(
            "the file has Windows line endings (CRLF): save it with plain (LF) line endings".into(),
        );
    }
    for (i, l) in lines.iter().enumerate() {
        let mut marks = 0;
        for c in l.chars() {
            marks = if is_mark(c) { marks + 1 } else { 0 };
            if marks == MAX_MARKS + 1 {
                problems.push(format!(
                    "line {}: more than {MAX_MARKS} accents on one letter",
                    i + 1
                ));
            }
            if crlf && c == '\r' {
                continue;
            }
            if let Some(why) = bad_char(c, i >= skip) {
                if !seen.contains(&why) {
                    seen.push(why);
                    problems.push(format!("line {}: {why} (U+{:04X})", i + 1, c as u32));
                }
            }
        }
    }
    if let Err(e) = check_slug(slug) {
        problems.push(e);
    }
    let (Some(author), Some(licence)) = (
        header(line(0), &["author"]),
        header(line(1), &["licence", "license"]),
    ) else {
        problems.push(if header(line(0), &["name"]).is_some() {
            "the header is two lines now, `-- author: …` and `-- licence: CC0-1.0`: the name and the description are the formula's own first lines, `name \"…\"` and `about \"…\"` (and `-- values:` is the `control` lines' defaults)".into()
        } else {
            "the file must start with two lines: `-- author: …`, `-- licence: CC0-1.0`, then the formula, starting `name \"…\"` and `about \"…\"`".into()
        });
        return Err(problems);
    };
    if bundle && !trusted {
        problems.push("`-- bundle` is for the examples OpenDMD ships: leave it out".into());
    }
    field(&mut problems, "author", author, MAX_AUTHOR);
    if !LICENCES.contains(&licence) {
        problems.push(format!(
            "the licence must be one of {}",
            LICENCES.join(", ")
        ));
    }
    // The formula: everything after the header, byte for byte.
    let offset: usize = text.split_inclusive('\n').take(skip).map(str::len).sum();
    let src = &text[offset.min(text.len())..];
    if src.lines().all(|l| {
        let t = l.trim();
        t.is_empty() || t.starts_with("--") || t.starts_with("//")
    }) {
        problems.push("there is no formula after the header".into());
        return Err(problems);
    }
    let mut prog = Box::<Program>::default();
    let declared = match compile_declared(src, &mut prog) {
        Ok(d) => d,
        Err(e) => {
            let line = skip + src[..e.start.min(src.len())].matches('\n').count() + 1;
            problems.push(format!(
                "line {line}: the formula doesn't compile: {}",
                e.msg
            ));
            return Err(problems);
        }
    };
    // (compile_declared skips the optimiser: the program as it plays.)
    let prog = compile(src).expect("it compiled");
    let text_of = |r: Option<(u32, u32)>| r.map(|(a, b)| &src[a as usize..b as usize]);
    // Its own name and line about it, as the web app shows them.
    let (name, about) = (text_of(declared.name), text_of(declared.about));
    match name {
        Some(n) => field(&mut problems, "name", n, MAX_NAME),
        None => problems.push("the formula needs a name: a first line like `name \"Rainbow rings\"`".into()),
    }
    match about {
        Some(d) => field(&mut problems, "about line", d, MAX_DESCRIPTION),
        None => problems.push("the formula needs a line about it, after its name: `about \"Rings rippling out from the middle\"`".into()),
    }
    if !trusted {
        for (what, v) in [("name", name.unwrap_or("")), ("author", author)] {
            if RESERVED.contains(&skeleton(v).as_str()) {
                problems.push(format!("the {what} can't be OpenDMD's or a maintainer's"));
            }
        }
    }
    // As the panel plays it: with its still cache when it has values for
    // one (a gallery formula is played as itself; a look is checked by the
    // player when it is chosen).
    let us = prog.esp32_us_still();
    if us > MAX_US {
        problems.push(format!(
            "the formula is too heavy for the panel: about {:.1} ms a frame, {:.0} ms at most",
            us as f64 / 1000.0,
            MAX_US as f64 / 1000.0
        ));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let controls = (0..3)
        .filter(|&k| declared.controls[k].is_some() || prog.sliders() & (1 << k) != 0)
        .map(|k| match declared.controls[k] {
            Some(c) => text_of(c.label).or(text_of(Some(c.name))).unwrap_or("").to_string(),
            None => ["a", "b", "c"][k].to_string(),
        })
        .collect();
    Ok(Entry {
        slug: slug.to_string(),
        name: name.unwrap_or("").to_string(),
        about: about.unwrap_or("").to_string(),
        author: author.to_string(),
        licence: licence.to_string(),
        bundle,
        look: prog.meta().look(),
        controls,
        src: src.to_string(),
        steps: prog.len(),
        loop_ms: prog.loop_ms(),
        inputs: prog.inputs(),
        us,
    })
}

/// The picture a formula that draws over a GIF (`Program::over`: it reads
/// `ur ug ub ul`, or its layers let what is under show through) is previewed
/// over: the web app's sample (js/psikit.js `sample()`, a sky, a sun and
/// hills; psi-check has a copy too), 128 × 32 RGB888, top row first.
pub fn sample() -> Vec<u8> {
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

/// One 128 × 32 RGB888 frame at `ms` into the loop, the controls at their
/// defaults (its `control` lines') and the clock at [`CLOCK`]; a formula
/// that draws over a GIF is drawn over [`sample`], as the web app's editor
/// shows it.
pub fn frame(prog: &Program, ms: u32) -> Vec<u8> {
    let mut inputs = Inputs {
        hour: CLOCK.0,
        minute: CLOCK.1,
        second: CLOCK.2,
        ..Inputs::default()
    };
    [inputs.a, inputs.b, inputs.c] = prog.meta().values([None; 3]);
    // (A formula that never repeats: `ms` since it started, seed 0.)
    inputs.set_ms(prog, u64::from(ms));
    let mut scratch: Box<Scratch<128>> = Box::new(Scratch::new());
    let mut out = vec![0u8; W * H * 3];
    let under = prog.over().then(sample);
    render_rows_ink(
        prog,
        &mut scratch,
        Some(&mut opendmd_psi::Ink::boxed()),
        &inputs,
        &OWN,
        under.as_deref(),
        u32::MAX,
        &mut out,
    );
    out
}

/// The thumbnail's moment: 30% into the loop, on a frame (as the web app's
/// tiles); for a formula that never repeats (`loop_ms` 0), of its first
/// [`FOREVER_MS`].
pub fn thumb_ms(loop_ms: u32) -> u32 {
    (shown(loop_ms) * 3 / 10) / FRAME_MS * FRAME_MS
}

/// What a formula that never repeats is shown over (its thumbnail and its
/// sheet): its first minute.
pub const FOREVER_MS: u32 = 60_000;

/// The time a formula's frames are shown across: its loop, or a minute.
fn shown(loop_ms: u32) -> u32 {
    if loop_ms == 0 {
        FOREVER_MS
    } else {
        loop_ms
    }
}

/// The entry's thumbnail: one frame, 128 × 32, as a PNG.
pub fn thumbnail(e: &Entry) -> Vec<u8> {
    let prog = compile(&e.src).expect("checked");
    png::encode(
        W as u32,
        H as u32,
        &frame(&prog, thumb_ms(e.loop_ms)),
    )
}

/// A reviewer's preview: eight frames across the loop, each pixel a 3 × 3
/// dot with a dark gap (roughly how the panel looks), stacked with a gap.
pub fn sheet(e: &Entry) -> Vec<u8> {
    const S: usize = 4;
    const N: usize = 8;
    const GAP: usize = 6;
    let prog = compile(&e.src).expect("checked");
    let (w, fh) = (W * S, H * S);
    let h = N * fh + (N - 1) * GAP;
    let mut img = vec![0x16u8; w * h * 3];
    for k in 0..N {
        let ms = (shown(e.loop_ms) * k as u32 / N as u32) / FRAME_MS * FRAME_MS;
        let f = frame(&prog, ms);
        let top = k * (fh + GAP);
        for y in 0..fh {
            for x in 0..w {
                let (px, py) = (x / S, y / S);
                let lit = x % S < S - 1 && y % S < S - 1;
                let o = ((top + y) * w + x) * 3;
                let s = (py * W + px) * 3;
                let rgb = if lit {
                    [f[s], f[s + 1], f[s + 2]]
                } else {
                    [0, 0, 0]
                };
                img[o..o + 3].copy_from_slice(&rgb);
            }
        }
    }
    png::encode(w as u32, h as u32, &img)
}

/// JSON for an entry (one line; `<`, `>` and `&` escaped too, so it is
/// safe inside HTML as well). `look`: `[blend, amount]` or null.
pub fn json(e: &Entry) -> String {
    let look = match e.look {
        Some((b, k)) => format!("[{},{k}]", js(b)),
        None => "null".into(),
    };
    let controls: Vec<String> = e.controls.iter().map(|c| js(c)).collect();
    format!(
        "{{\"slug\":{},\"name\":{},\"about\":{},\"author\":{},\"licence\":{},\"bundle\":{},\"look\":{look},\"controls\":[{}],\"src\":{},\"steps\":{},\"loop_ms\":{},\"inputs\":{},\"us\":{}}}",
        js(&e.slug),
        js(&e.name),
        js(&e.about),
        js(&e.author),
        js(&e.licence),
        e.bundle,
        controls.join(","),
        js(&e.src),
        e.steps,
        e.loop_ms,
        e.inputs,
        e.us
    )
}

/// A JSON string.
pub fn js(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '<' | '>' | '&' => o.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                o.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Text from a contribution made safe for a Markdown/HTML page (the
/// workflow's summary): every character that means something there as an
/// entity, controls dropped.
pub fn md(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '&' | '<' | '>' | '"' | '\'' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '|' | '~'
            | '#' | '!' | '\\' | '{' | '}' | ':' | '@' => o.push_str(&format!("&#{};", c as u32)),
            c if c.is_control() || bad_char(c, false).is_some() => o.push('\u{FFFD}'),
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests;
