//! The formula gallery's checker.
//!
//! The public gallery is a folder of plain-text files in the public site
//! repo (`gallery/<slug>.psi`), each a header of comment lines and then a
//! formula in Psi (`opendmd-psi`):
//!
//! ```text
//! -- name: Plasma
//! -- description: The demoscene classic: three waves, every colour
//! -- author: OpenDMD
//! -- licence: CC0-1.0
//! -- values: a=0.3
//! loop 8
//! wave = sin(x*2 + t/8) + sin(y*1.5 - t/4)
//! hsv(wave/4 + t/8, 0.9, 1)
//! ```
//!
//! The first four lines are required, in that order (`license` is accepted
//! for `licence`); `values` (the sliders' starting positions) is optional.
//! The header lines are Psi comments, so the whole file is a formula too.
//!
//! [`check_path`] and [`check_file`] are everything a contribution must
//! pass: the path (one flat `gallery/<slug>.psi`), the size, the text (UTF-8
//! without control, invisible or direction-changing characters; no `<` or
//! `>` in the header), the header's fields and limits, the licence, that it
//! compiles, and its cost on the panel (`Program::esp32_us`). The checker
//! never runs anything but the Psi VM, which has no I/O: a formula is
//! arithmetic over the pixel and the time.
//!
//! The same checks run in three places: the public repo's pull-request
//! workflow (on the contribution, as data), `tools/site.py` at every site
//! build (on the merged files, before anything is published), and the
//! tests here. `main.rs` is the command line.

use opendmd_psi::{compile, render_rows, Inputs, Program, Scratch, H, OWN, W};

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
    pub name: String,
    pub description: String,
    pub author: String,
    pub licence: String,
    /// The sliders' starting positions in hundredths (`values: a=0.3` is
    /// `Some(30)`).
    pub values: [Option<u8>; 3],
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

/// `a=0.3 c=1`: each slider at most once, 0 to 1 in at most two decimals.
fn values(v: &str) -> Result<[Option<u8>; 3], String> {
    let mut out = [None; 3];
    let bad = || {
        "values are like `a=0.3 b=1`: a, b or c, each 0 to 1 with at most two decimals".to_string()
    };
    for part in v.split(' ') {
        let (k, num) = part.split_once('=').ok_or_else(bad)?;
        let i = match k {
            "a" => 0,
            "b" => 1,
            "c" => 2,
            _ => return Err(bad()),
        };
        let (whole, frac) = num.split_once('.').unwrap_or((num, ""));
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if whole.is_empty()
            || whole.len() > 1
            || frac.len() > 2
            || !digits(whole)
            || !digits(frac)
            || out[i].is_some()
        {
            return Err(bad());
        }
        let h = whole.parse::<u32>().map_err(|_| bad())? * 100
            + format!("{frac:0<2}").parse::<u32>().map_err(|_| bad())?;
        if h > 100 {
            return Err(bad());
        }
        out[i] = Some(h as u8);
    }
    Ok(out)
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
/// lets the name or author be one of the [`RESERVED`].
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
    let skip = if header(line(4), &["values"]).is_some() {
        5
    } else {
        4
    };
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
    let (Some(name), Some(description), Some(author), Some(licence)) = (
        header(line(0), &["name"]),
        header(line(1), &["description"]),
        header(line(2), &["author"]),
        header(line(3), &["licence", "license"]),
    ) else {
        problems.push(
            "the file must start with four lines: `-- name: …`, `-- description: …`, `-- author: …`, `-- licence: CC0-1.0`"
                .into(),
        );
        return Err(problems);
    };
    field(&mut problems, "name", name, MAX_NAME);
    field(&mut problems, "description", description, MAX_DESCRIPTION);
    field(&mut problems, "author", author, MAX_AUTHOR);
    if !trusted {
        for (what, v) in [("name", name), ("author", author)] {
            if RESERVED.contains(&skeleton(v).as_str()) {
                problems.push(format!("the {what} can't be OpenDMD's or a maintainer's"));
            }
        }
    }
    if !LICENCES.contains(&licence) {
        problems.push(format!(
            "the licence must be one of {}",
            LICENCES.join(", ")
        ));
    }
    let mut vals = [None; 3];
    if let Some(v) = header(line(4), &["values"]) {
        match values(v) {
            Ok(v) => vals = v,
            Err(e) => problems.push(e),
        }
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
    let prog = match compile(src) {
        Ok(p) => p,
        Err(e) => {
            let line = skip + src[..e.start.min(src.len())].matches('\n').count() + 1;
            problems.push(format!(
                "line {line}: the formula doesn't compile: {}",
                e.msg
            ));
            return Err(problems);
        }
    };
    let us = prog.esp32_us();
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
    Ok(Entry {
        slug: slug.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        author: author.to_string(),
        licence: licence.to_string(),
        values: vals,
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

/// One 128 × 32 RGB888 frame at `ms` into the loop, the sliders where the
/// entry starts them and the clock at [`CLOCK`]; a formula that draws over
/// a GIF is drawn over [`sample`], as the web app's editor shows it.
pub fn frame(prog: &Program, values: &[Option<u8>; 3], ms: u32) -> Vec<u8> {
    // As the web app's Math.round(v * 65536) (never a tie for hundredths).
    let q = |v: Option<u8>| ((i32::from(v.unwrap_or(0)) << 16) + 50) / 100;
    let inputs = Inputs {
        t: Inputs::at_ms(ms),
        hour: CLOCK.0,
        minute: CLOCK.1,
        second: CLOCK.2,
        a: q(values[0]),
        b: q(values[1]),
        c: q(values[2]),
    };
    let mut scratch: Box<Scratch<128>> = Box::new(Scratch::new());
    let mut out = vec![0u8; W * H * 3];
    let under = prog.over().then(sample);
    render_rows(
        prog,
        &mut scratch,
        &inputs,
        &OWN,
        under.as_deref(),
        u32::MAX,
        &mut out,
    );
    out
}

/// The thumbnail's moment: 30% into the loop, on a frame (as the web app's
/// tiles).
pub fn thumb_ms(loop_ms: u32) -> u32 {
    (loop_ms * 3 / 10) / FRAME_MS * FRAME_MS
}

/// The entry's thumbnail: one frame, 128 × 32, as a PNG.
pub fn thumbnail(e: &Entry) -> Vec<u8> {
    let prog = compile(&e.src).expect("checked");
    png::encode(
        W as u32,
        H as u32,
        &frame(&prog, &e.values, thumb_ms(e.loop_ms)),
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
        let ms = (e.loop_ms * k as u32 / N as u32) / FRAME_MS * FRAME_MS;
        let f = frame(&prog, &e.values, ms);
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
/// safe inside HTML as well).
pub fn json(e: &Entry) -> String {
    let mut vals = String::new();
    for (k, v) in ["a", "b", "c"].iter().zip(e.values) {
        if let Some(v) = v {
            vals.push_str(&format!(",\"{k}\":{}", hundredths(v)));
        }
    }
    format!(
        "{{\"slug\":{},\"name\":{},\"description\":{},\"author\":{},\"licence\":{},\"src\":{},\"steps\":{},\"loop_ms\":{},\"inputs\":{},\"us\":{}{vals}}}",
        js(&e.slug),
        js(&e.name),
        js(&e.description),
        js(&e.author),
        js(&e.licence),
        js(&e.src),
        e.steps,
        e.loop_ms,
        e.inputs,
        e.us
    )
}

/// `30` -> `0.3`, `100` -> `1`, `5` -> `0.05`.
pub fn hundredths(v: u8) -> String {
    let s = format!("{}.{:02}", v / 100, v % 100);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
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
