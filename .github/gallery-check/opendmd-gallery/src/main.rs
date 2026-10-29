//! The formula gallery's checker, on the command line.
//!
//! ```text
//! opendmd-gallery check [--json] [--trusted] [--thumbs DIR] [--previews DIR] FILE...
//! opendmd-gallery pr --repo DIR [--base REV] [--head REV] [--previews DIR] [--summary FILE]
//! ```
//!
//! `check` validates gallery files on disk (the site build: the merged
//! gallery and the examples); `--json` prints one line per file, the entry
//! or its problems; `--trusted` (OpenDMD's own examples only) lets the
//! name or author be OpenDMD's; `--thumbs` writes each one's 128 × 32 thumbnail and
//! `--previews` a sheet of eight frames, as `<slug>.png`. Only regular files
//! are read (a symlink is refused, not followed).
//!
//! `pr` validates what a pull request changes: the commits `--base`
//! (default `HEAD^1`) and `--head` (default `HEAD`) of the repository at
//! `--repo`, read from git's objects — nothing is checked out or run. Every
//! change must add or change one regular file `gallery/<slug>.psi` that
//! passes `check`. `--summary` appends a Markdown report (the workflow's job
//! summary). Untrusted text is printed escaped (`{:?}`) on stdout and as
//! entities in the summary.
//!
//! Exit status: 0 all good, 1 a problem with the contribution, 2 a usage or
//! environment error.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use opendmd_gallery::{
    check_file, check_file_as, check_path, json, md, sheet, thumbnail, Entry, MAX_FILE,
};

/// The most files one pull request may add or change.
const MAX_FILES: usize = 20;

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  opendmd-gallery check [--json] [--trusted] [--thumbs DIR] [--previews DIR] FILE...\n  \
         opendmd-gallery pr --repo DIR [--base REV] [--head REV] [--previews DIR] [--summary FILE]"
    );
    ExitCode::from(2)
}

struct Opts {
    json: bool,
    trusted: bool,
    thumbs: Option<PathBuf>,
    previews: Option<PathBuf>,
    repo: Option<PathBuf>,
    base: String,
    head: String,
    summary: Option<PathBuf>,
    files: Vec<PathBuf>,
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(cmd) = args.next() else {
        return usage();
    };
    let mut o = Opts {
        json: false,
        trusted: false,
        thumbs: None,
        previews: None,
        repo: None,
        base: "HEAD^1".into(),
        head: "HEAD".into(),
        summary: None,
        files: Vec::new(),
    };
    while let Some(a) = args.next() {
        let mut val = |what: &str| args.next().ok_or_else(|| format!("{what} needs a value"));
        let r = match a.as_str() {
            "--json" => {
                o.json = true;
                Ok(())
            }
            "--trusted" => {
                o.trusted = true;
                Ok(())
            }
            "--thumbs" => val("--thumbs").map(|v| o.thumbs = Some(v.into())),
            "--previews" => val("--previews").map(|v| o.previews = Some(v.into())),
            "--repo" => val("--repo").map(|v| o.repo = Some(v.into())),
            "--base" => val("--base").map(|v| o.base = v),
            "--head" => val("--head").map(|v| o.head = v),
            "--summary" => val("--summary").map(|v| o.summary = Some(v.into())),
            s if s.starts_with('-') => Err(format!("unknown option {s}")),
            s => {
                o.files.push(s.into());
                Ok(())
            }
        };
        if let Err(e) = r {
            eprintln!("{e}");
            return usage();
        }
    }
    for d in [&o.thumbs, &o.previews].into_iter().flatten() {
        if let Err(e) = fs::create_dir_all(d) {
            eprintln!("{}: {e}", d.display());
            return ExitCode::from(2);
        }
    }
    match cmd.as_str() {
        "check" if !o.files.is_empty() => check(&o),
        "pr" if o.repo.is_some() && o.files.is_empty() => pr(&o),
        _ => usage(),
    }
}

/// Write the entry's pictures where asked.
fn pictures(o: &Opts, e: &Entry) -> Result<(), String> {
    if let Some(d) = &o.thumbs {
        let p = d.join(format!("{}.png", e.slug));
        fs::write(&p, thumbnail(e)).map_err(|err| format!("{}: {err}", p.display()))?;
    }
    if let Some(d) = &o.previews {
        let p = d.join(format!("{}.png", e.slug));
        fs::write(&p, sheet(e)).map_err(|err| format!("{}: {err}", p.display()))?;
    }
    Ok(())
}

fn describe(e: &Entry) -> String {
    format!(
        "{:?} by {:?} ({}): {} steps, about {:.1} ms a frame on the panel",
        e.name,
        e.author,
        e.licence,
        e.steps,
        e.us as f64 / 1000.0
    )
}

/// A regular file's bytes (no symlinks, nothing over the limit).
fn read_file(p: &Path) -> Result<Vec<u8>, String> {
    let m = fs::symlink_metadata(p).map_err(|e| e.to_string())?;
    if !m.file_type().is_file() {
        return Err("not a regular file (a link or a folder)".into());
    }
    if m.len() > MAX_FILE as u64 {
        return Err(format!("the file is {} bytes: {MAX_FILE} at most", m.len()));
    }
    fs::read(p).map_err(|e| e.to_string())
}

fn check(o: &Opts) -> ExitCode {
    let mut failed = 0;
    let out = std::io::stdout();
    let mut out = out.lock();
    for f in &o.files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let r = check_path(&format!("gallery/{name}"))
            .map_err(|e| vec![e])
            .and_then(|slug| {
                read_file(f)
                    .map_err(|e| vec![e])
                    .and_then(|b| check_file_as(&slug, &b, o.trusted))
            });
        match r {
            Ok(e) => {
                if let Err(err) = pictures(o, &e) {
                    eprintln!("{err}");
                    return ExitCode::from(2);
                }
                eprintln!("ok   {name:?}: {}", describe(&e));
                if o.json {
                    let _ = writeln!(out, "{}", json(&e));
                }
            }
            Err(problems) => {
                failed += 1;
                for p in &problems {
                    eprintln!("FAIL {name:?}: {p}");
                }
                if o.json {
                    let list: Vec<String> =
                        problems.iter().map(|p| opendmd_gallery::js(p)).collect();
                    let _ = writeln!(
                        out,
                        "{{\"file\":{},\"problems\":[{}]}}",
                        opendmd_gallery::js(&name),
                        list.join(",")
                    );
                }
            }
        }
    }
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `git -C repo <args>`, its stdout (an error if it fails).
fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// A changed file: its path as printed, the entry or its problems, and notes.
type Checked = (String, Result<Entry, Vec<String>>, Vec<String>);

/// One change in the pull request.
struct Change {
    path: Vec<u8>,
    status: u8,
    mode: String,
    blob: String,
}

/// `git diff-tree -r -z --raw`: `:old new oldsha newsha S\0path\0`...
fn changes(repo: &Path, base: &str, head: &str) -> Result<Vec<Change>, String> {
    let raw = git(
        repo,
        &[
            "diff-tree",
            "-r",
            "-z",
            "--no-renames",
            "--raw",
            "--no-commit-id",
            base,
            head,
        ],
    )?;
    let mut parts = raw.split(|&b| b == 0).filter(|p| !p.is_empty());
    let mut out = Vec::new();
    while let Some(meta) = parts.next() {
        let meta = String::from_utf8_lossy(meta);
        let f: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
        let path = parts
            .next()
            .ok_or("diff-tree: a change without a path")?
            .to_vec();
        if f.len() != 5 {
            return Err(format!("diff-tree: unexpected line {meta:?}"));
        }
        out.push(Change {
            path,
            status: f[4].as_bytes()[0],
            mode: f[1].to_string(),
            blob: f[3].to_string(),
        });
    }
    Ok(out)
}

fn pr(o: &Opts) -> ExitCode {
    let repo = o.repo.as_deref().expect("checked");
    for r in [&o.base, &o.head] {
        if r.starts_with('-') {
            eprintln!("a revision can't start with -");
            return ExitCode::from(2);
        }
    }
    let list = match changes(repo, &o.base, &o.head) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut results: Vec<Checked> = Vec::new();
    let mut general: Vec<String> = Vec::new();
    if list.is_empty() {
        general.push("the pull request changes nothing".into());
    }
    if list.len() > MAX_FILES {
        general.push(format!(
            "the pull request changes {} files: {MAX_FILES} at most",
            list.len()
        ));
    }
    for c in list.iter().take(MAX_FILES) {
        let shown = String::from_utf8_lossy(&c.path).into_owned();
        let mut notes = Vec::new();
        let r = (|| -> Result<Entry, Vec<String>> {
            let path = std::str::from_utf8(&c.path)
                .map_err(|_| vec!["the file name is not plain text".to_string()])?;
            match c.status {
                b'A' => {}
                b'M' => {}
                b'D' => {
                    return Err(vec![
                        "removes a file: a pull request can only add or change gallery files"
                            .into(),
                    ])
                }
                _ => {
                    return Err(vec![
                        "changes a file's type: only plain gallery files can be added".into(),
                    ])
                }
            }
            match c.mode.as_str() {
                "100644" => {}
                "120000" => {
                    return Err(vec![
                        "is a symbolic link: only plain files can be added".into()
                    ])
                }
                "160000" => {
                    return Err(vec!["is a submodule: only plain files can be added".into()])
                }
                "100755" => {
                    return Err(vec![
                        "is marked executable: gallery files are plain text".into()
                    ])
                }
                m => return Err(vec![format!("has an unexpected file mode {m}")]),
            }
            let slug = check_path(path).map_err(|e| vec![e])?;
            if c.status == b'M' {
                notes.push(
                    "changes a formula already in the gallery: check its author agrees".to_string(),
                );
            }
            let size: u64 = git(repo, &["cat-file", "-s", &c.blob])
                .map_err(|e| vec![e])
                .and_then(|s| {
                    String::from_utf8_lossy(&s)
                        .trim()
                        .parse()
                        .map_err(|_| vec!["cat-file -s".to_string()])
                })?;
            if size > MAX_FILE as u64 {
                return Err(vec![format!(
                    "the file is {size} bytes: {MAX_FILE} at most"
                )]);
            }
            let bytes = git(repo, &["cat-file", "blob", &c.blob]).map_err(|e| vec![e])?;
            check_file(&slug, &bytes)
        })();
        results.push((shown, r, notes));
    }
    let mut failed = !general.is_empty();
    for g in &general {
        println!("FAIL {g}");
    }
    for (path, r, notes) in &results {
        match r {
            Ok(e) => {
                println!("ok   {path:?}: {}", describe(e));
                if let Err(err) = pictures(o, e) {
                    eprintln!("{err}");
                    return ExitCode::from(2);
                }
            }
            Err(ps) => {
                failed = true;
                for p in ps {
                    println!("FAIL {path:?}: {p}");
                }
            }
        }
        for n in notes {
            println!("note {path:?}: {n}");
        }
    }
    if let Some(s) = &o.summary {
        let mut t = String::from("## Formula gallery check\n\n");
        t.push_str(if failed {
            "**Not ready:** fix the problems below and push again.\n\n"
        } else {
            "**Every file passed.** A maintainer still reviews each formula (the preview pictures are in this run's artifacts).\n\n"
        });
        for g in &general {
            t.push_str(&format!("- {}\n", md(g)));
        }
        for (path, r, notes) in &results {
            match r {
                Ok(e) => t.push_str(&format!(
                    "- ✅ {}: <b>{}</b> by {} ({}) — {}; {} steps, about {:.1} ms a frame\n",
                    md(path),
                    md(&e.name),
                    md(&e.author),
                    md(&e.licence),
                    md(&e.about),
                    e.steps,
                    e.us as f64 / 1000.0
                )),
                Err(ps) => {
                    for p in ps {
                        t.push_str(&format!("- ❌ {}: {}\n", md(path), md(p)));
                    }
                }
            }
            for n in notes {
                t.push_str(&format!("- ⚠️ {}: {}\n", md(path), md(n)));
            }
        }
        let w = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(s)
            .and_then(|mut f| f.write_all(t.as_bytes()));
        if let Err(e) = w {
            eprintln!("{}: {e}", s.display());
            return ExitCode::from(2);
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
