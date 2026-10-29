//! The encoders against the GNU assembler (`xtensa-esp32-elf-as`, from
//! ESP-IDF's tools): every form, registers and immediates across their
//! ranges, assembled as text in one file and compared byte for byte. Skipped
//! (with a line saying so) where the assembler is not installed.

use super::enc::*;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

/// The assembler's directory: `XTENSA_BIN`, else the newest ESP-IDF
/// toolchain under ~/.espressif.
pub(crate) fn binutils() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("XTENSA_BIN") {
        let b = PathBuf::from(b);
        return b.join("xtensa-esp32-elf-as").exists().then_some(b);
    }
    let home = std::env::var_os("HOME")?;
    let root = PathBuf::from(home).join(".espressif/tools/xtensa-esp-elf");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("xtensa-esp-elf/bin"))
        .filter(|b| b.join("xtensa-esp32-elf-as").exists())
        .collect();
    found.sort();
    found.pop()
}

/// Assemble `text` (one instruction a line) without the assembler's own
/// transformations; the raw bytes of `.text`.
pub(crate) fn assemble(bin: &std::path::Path, text: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!(
        "psi-jit-as-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let (s, o, b) = (dir.join("t.s"), dir.join("t.o"), dir.join("t.bin"));
    std::fs::write(
        &s,
        format!("\t.text\n\t.begin no-transform\n{text}\t.end no-transform\n"),
    )
    .unwrap();
    let st = Command::new(bin.join("xtensa-esp32-elf-as"))
        .arg("-o")
        .arg(&o)
        .arg(&s)
        .status()
        .unwrap();
    assert!(st.success(), "the assembler refused the test's text");
    let st = Command::new(bin.join("xtensa-esp32-elf-objcopy"))
        .args(["-O", "binary", "-j", ".text"])
        .arg(&o)
        .arg(&b)
        .status()
        .unwrap();
    assert!(st.success());
    let out = std::fs::read(&b).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// Lines of text beside the code, each with where its bytes start.
struct Pair {
    code: Code,
    text: String,
    lines: Vec<(usize, String)>,
}

impl Pair {
    fn new() -> Pair {
        Pair {
            code: Code::new(),
            text: String::new(),
            lines: Vec::new(),
        }
    }

    fn at(&mut self, line: String, f: impl FnOnce(&mut Code)) {
        let at = self.code.len();
        f(&mut self.code);
        assert!(!self.code.bad, "encoder refused `{line}`");
        let _ = writeln!(self.text, "\t{line}");
        self.lines.push((at, line));
    }

    /// `n` zero bytes (room behind the branches that reach back).
    fn pad(&mut self, n: usize) {
        self.code.bytes.resize(self.code.len() + n, 0);
        let _ = writeln!(self.text, "\t.fill {n}, 1, 0");
    }

    fn check(self, bin: &std::path::Path) {
        let want = assemble(bin, &self.text);
        let got = &self.code.bytes;
        if want != *got {
            for (k, (at, line)) in self.lines.iter().enumerate() {
                let end = self.lines.get(k + 1).map_or(got.len(), |l| l.0);
                let (g, w) = (
                    &got[*at..end],
                    want.get(*at..end.min(want.len())).unwrap_or(&[]),
                );
                assert_eq!(
                    g, w,
                    "`{line}` at {at}: ours {g:02x?}, the assembler's {w:02x?}"
                );
            }
            panic!(
                "lengths differ: ours {} B, the assembler's {} B",
                got.len(),
                want.len()
            );
        }
    }
}

/// `. + off` / `. - off`, as the assembler reads a relative target.
fn rel(off: i32) -> String {
    if off < 0 {
        format!(". - {}", -off)
    } else {
        format!(". + {off}")
    }
}

fn a(r: Reg) -> String {
    format!("a{r}")
}

#[test]
fn encoders_match_the_gnu_assembler() {
    let Some(bin) = binutils() else {
        println!("SKIP: encoders_match_the_gnu_assembler: no xtensa-esp32-elf-as (set XTENSA_BIN or install ESP-IDF's tools)");
        return;
    };
    let mut p = Pair::new();
    // (Room for the longest backward branch, `j . - 131072`.)
    p.pad(132 * 1024);
    p.at("entry a1, 32".into(), |c| c.entry(32));
    p.at("entry a1, 4088".into(), |c| c.entry(4088));
    // Every register in every position of the three-register forms.
    type Rrr = fn(&mut Code, Reg, Reg, Reg);
    let rrr: [(&str, Rrr); 16] = [
        ("and", Code::and),
        ("or", Code::or),
        ("xor", Code::xor),
        ("add", Code::add),
        ("addx2", Code::addx2),
        ("addx4", Code::addx4),
        ("sub", Code::sub),
        ("mull", Code::mull),
        ("muluh", Code::muluh),
        ("mulsh", Code::mulsh),
        ("min", Code::min),
        ("max", Code::max),
        ("minu", Code::minu),
        ("maxu", Code::maxu),
        ("src", Code::src),
        ("moveqz", Code::moveqz),
    ];
    let more: [(&str, Rrr); 3] = [
        ("movnez", Code::movnez),
        ("movltz", Code::movltz),
        ("movgez", Code::movgez),
    ];
    for (name, f) in rrr.iter().chain(more.iter()) {
        for (r, s, t) in [(2, 3, 4), (15, 0, 7), (5, 14, 15), (8, 9, 10), (13, 13, 1)] {
            p.at(format!("{name} {}, {}, {}", a(r), a(s), a(t)), |c| {
                f(c, r, s, t)
            });
        }
    }
    for (r, t) in [(2, 3), (15, 14), (9, 0)] {
        p.at(format!("neg {}, {}", a(r), a(t)), |c| c.neg(r, t));
        p.at(format!("abs {}, {}", a(r), a(t)), |c| c.abs(r, t));
    }
    for sa in 1..32 {
        p.at(format!("slli a5, a12, {sa}"), |c| c.slli(5, 12, sa));
    }
    for sa in 0..32 {
        p.at(format!("srai a14, a3, {sa}"), |c| c.srai(14, 3, sa));
        p.at(format!("ssai {sa}"), |c| c.ssai(sa));
    }
    for sa in 0..16 {
        p.at(format!("srli a6, a9, {sa}"), |c| c.srli(6, 9, sa));
    }
    for shift in [0, 1, 7, 8, 11, 14, 15, 16, 17, 24, 31] {
        for bits in [1, 2, 5, 8, 15, 16] {
            if shift + bits <= 32 {
                p.at(format!("extui a7, a13, {shift}, {bits}"), |c| {
                    c.extui(7, 13, shift, bits)
                });
            }
        }
    }
    for off in [0, 1, 2, 3, 100, 255] {
        p.at(format!("l8ui a4, a11, {off}"), |c| c.l8ui(4, 11, off));
        p.at(format!("s8i a4, a11, {off}"), |c| c.s8i(4, 11, off));
    }
    for off in [0, 2, 4, 254, 510] {
        p.at(format!("l16si a10, a2, {off}"), |c| c.l16si(10, 2, off));
    }
    for off in [0, 4, 56, 60] {
        p.at(format!("l32i.n a3, a2, {off}"), |c| c.l32i(3, 2, off));
        p.at(format!("s32i.n a3, a1, {off}"), |c| c.s32i(3, 1, off));
    }
    for off in [0, 4, 64, 512, 1020] {
        p.at(format!("l32i a15, a2, {off}"), |c| c.l32i_wide(15, 2, off));
        p.at(format!("s32i a8, a1, {off}"), |c| c.s32i_wide(8, 1, off));
    }
    for off in [64, 1020] {
        p.at(format!("l32i a12, a2, {off}"), |c| c.l32i(12, 2, off));
        p.at(format!("s32i a12, a2, {off}"), |c| c.s32i(12, 2, off));
    }
    for imm in [-32, -1, 0, 1, 31, 95] {
        p.at(format!("movi.n a9, {imm}"), |c| c.movi(9, imm));
    }
    for imm in [-2048, -33, 96, 255, 256, 2047] {
        p.at(format!("movi a9, {imm}"), |c| c.movi(9, imm));
    }
    for imm in [-128, -2, 16, 127] {
        p.at(format!("addi a3, a4, {imm}"), |c| c.addi(3, 4, imm));
    }
    for imm in [-1, 1, 15] {
        p.at(format!("addi.n a3, a4, {imm}"), |c| c.addi(3, 4, imm));
    }
    for imm in [-32768, -256, 256, 16384, 32512] {
        p.at(format!("addmi a6, a7, {imm}"), |c| c.addmi(6, 7, imm));
    }
    p.at("add.n a2, a3, a4".into(), |c| c.add_n(2, 3, 4));
    p.at("add.n a15, a14, a13".into(), |c| c.add_n(15, 14, 13));
    p.at("mov.n a5, a6".into(), |c| c.mov(5, 6));
    p.at("mov.n a15, a0".into(), |c| c.mov_n(15, 0));
    p.at("retw.n".into(), |c| c.retw_n());
    p.at("callx8 a8".into(), |c| c.callx8(8));
    p.at("callx8 a15".into(), |c| c.callx8(15));
    p.at("memw".into(), |c| c.memw());
    // Branches, both ways, at the ends of their reach.
    let conds = [
        ("beq", Cond::Eq),
        ("bne", Cond::Ne),
        ("blt", Cond::Lt),
        ("bge", Cond::Ge),
        ("bltu", Cond::Ltu),
        ("bgeu", Cond::Geu),
    ];
    for (name, cc) in conds {
        for off in [-128, -4, 4, 8, 127] {
            p.at(format!("{name} a3, a12, {}", rel(off + 4)), |c| {
                let to = (c.here() as i32 + off + 4) as usize;
                c.b(cc, 3, 12, to)
            });
        }
    }
    for bit in [0, 7, 15, 16, 31] {
        p.at(format!("bbci a6, {bit}, . + 24"), |c| {
            let to = c.here() + 24;
            c.bbci(6, bit, to)
        });
    }
    let zeros = [
        ("beqz", Zero::Eqz),
        ("bnez", Zero::Nez),
        ("bltz", Zero::Ltz),
        ("bgez", Zero::Gez),
    ];
    for (name, z) in zeros {
        for off in [-2048, -8, 8, 2047] {
            p.at(format!("{name} a10, {}", rel(off + 4)), |c| {
                let to = (c.here() as i32 + off + 4) as usize;
                c.bz(z, 10, to)
            });
        }
    }
    let imms = [
        ("beqi", Imm::Eqi),
        ("bnei", Imm::Nei),
        ("blti", Imm::Lti),
        ("bgei", Imm::Gei),
    ];
    for (name, m) in imms {
        for imm in [-1, 1, 3, 12, 128, 256] {
            p.at(format!("{name} a5, {imm}, . + 16"), |c| {
                let to = c.here() + 16;
                c.bi(m, 5, imm, to)
            });
        }
        p.at(format!("{name} a5, 64, . - 96"), |c| {
            let to = c.here() - 96;
            c.bi(m, 5, 64, to)
        });
    }
    for (name, ge) in [("bltui", false), ("bgeui", true)] {
        for imm in [32768, 65536, 2, 16, 256] {
            p.at(format!("{name} a7, {imm}, . + 34"), |c| {
                let to = c.here() + 34;
                c.bui(ge, 7, imm, to)
            });
        }
    }
    for off in [-131072, -300, 0, 300, 131071] {
        p.at(format!("j {}", rel(off + 4)), |c| {
            let to = (c.here() as i32 + off + 4) as usize;
            c.j(to)
        });
    }
    p.check(&bin);
}

#[test]
fn forward_branches_patch_to_what_backward_ones_encode() {
    // A fixup bound later is the same bytes as the branch written to its
    // target directly.
    let mut a = Code::new();
    let f = a.b_fwd(Cond::Ne, 4, 5);
    let g = a.bz_fwd(Zero::Ltz, 6);
    let h = a.bi_fwd(Imm::Eqi, 7, 3);
    let k = a.j_fwd();
    let m = a.bbci_fwd(8, 15);
    let u = a.bui_fwd(false, 9, 2);
    a.movi(2, 1);
    a.bind(f);
    a.bind(g);
    a.bind(h);
    a.bind(k);
    a.bind(m);
    a.bind(u);
    let end = a.here();
    let mut b = Code::new();
    b.b(Cond::Ne, 4, 5, end);
    b.bz(Zero::Ltz, 6, end);
    b.bi(Imm::Eqi, 7, 3, end);
    b.j(end);
    b.bbci(8, 15, end);
    b.bui(false, 9, 2, end);
    b.movi(2, 1);
    assert_eq!(a.bytes, b.bytes);
    assert!(!a.bad && !b.bad);
}

#[test]
fn out_of_range_operands_are_refused_not_wrapped() {
    for f in [
        (|c: &mut Code| c.movi(2, 2048)) as fn(&mut Code),
        |c| c.addi(2, 3, 128),
        |c| c.addmi(2, 3, 32768),
        |c| c.addmi(2, 3, 100),
        |c| c.l32i(2, 3, 1024),
        |c| c.l32i(2, 3, 2),
        |c| c.slli(2, 3, 0),
        |c| c.srli(2, 3, 16),
        |c| c.extui(2, 3, 0, 17),
        |c| c.b(Cond::Eq, 2, 3, 200),
        |c| c.bi(Imm::Eqi, 2, 9, 8),
        |c| c.entry(12),
    ] {
        let mut c = Code::new();
        f(&mut c);
        assert!(c.bad);
    }
}
