//! The language service's notes (feature `syntax`; `syntax::Sink::note`,
//! printed by `psi check`): where a Formula costs more than it needs to,
//! and why — a `pick` worked out per pixel, a sum or product that brackets
//! better, what the still cache and the column run took (and the heaviest
//! value they couldn't), draw loops run as lanes. A child of `compile`:
//! it reads the parser's and the DAG's own fields. Never in the firmware.

use super::*;
use crate::syntax::Kept;

/// A whole number with thousands' commas.
fn group(v: i64) -> String {
    let d = v.to_string();
    let mut o = String::new();
    for (k, ch) in d.chars().enumerate() {
        if k > 0 && (d.len() - k).is_multiple_of(3) {
            o.push(',');
        }
        o.push(ch);
    }
    o
}

/// Source text for a note, cut to 40 characters.
fn short(s: &str) -> String {
    match s.char_indices().nth(40) {
        Some((k, _)) => format!("{}…", &s[..k]),
        None => s.to_string(),
    }
}

impl<'s> Parser<'s> {
    /// The nearest of the formula's own values that makes `id` vary as
    /// it does, by its name, else the built-in it reads.
    fn reads_word(&self, id: Id) -> String {
        let named = |m: Id| {
            self.locals
                .iter()
                .rposition(|l| l.id == m && !l.counter && self.visible(l.scope))
        };
        match cause(&self.dag, id, &named) {
            (Some(j), _) => format!("`{}`", self.locals[j].name),
            (None, r) => format!("`{}`", input_word(&self.dag.nodes[r as usize]).0),
        }
    }

    /// The pick note: a per-pixel `pick` with a per-pixel arm is a chain
    /// of compares and choices a pixel, not one step (`Dag::pick`).
    pub(super) fn pick_note(
        &self,
        t: Token,
        k: Id,
        v: &[Id],
        spans: &[(usize, usize)],
    ) -> Option<(usize, usize, String)> {
        if self.dag.konst(k).is_some() || self.dag.level(k) != LV_PIXEL {
            return None;
        }
        let i = v.iter().position(|&o| self.dag.level(o) == LV_PIXEL)?;
        let (a, b) = spans[i];
        let text = format!(
            "arm {} of this `pick` (`{}`) is per pixel (it reads {}), so the whole `pick` is \
             worked out per pixel: about {} steps a pixel instead of 1. Keep the arms the same \
             for every pixel and add the per-pixel part outside: `pick(…) + …`",
            i + 1,
            short(sub(self.src, a, b).trim()),
            self.reads_word(v[i]),
            2 * v.len() + 2
        );
        Some((t.start, t.end, text))
    }

    /// The split note: `A op B op C` with `A` per pixel and `B`, `C` not
    /// works `op C` out for every pixel; `A op (B op C)` would not.
    pub(super) fn split_note(
        &self,
        start: usize,
        (pop, bs, a, b): (Token, (usize, usize), Id, Id),
        (op, cs, lhs, c): (Token, (usize, usize), Id, Id),
    ) -> Option<(usize, usize, String)> {
        let fam = |t: Tok| match t {
            Tok::Plus | Tok::Minus => 1,
            Tok::Star => 2,
            _ => 0,
        };
        let f = fam(op.tok);
        if f == 0 || fam(pop.tok) != f {
            return None;
        }
        let d = &self.dag;
        let n = d.nodes[lhs as usize];
        if n.code >= K_CONST || n.a != a || n.b != b {
            return None;
        }
        if d.level(a) != LV_PIXEL || d.level(b) == LV_PIXEL || d.level(c) == LV_PIXEL {
            return None;
        }
        let once = if d.level(b).max(d.level(c)) == LV_ROW {
            "row"
        } else {
            "frame"
        };
        let txt = |x: usize, y: usize| sub(self.src, x, y).trim();
        let (ta, tb, tc) = (txt(start, pop.start), txt(bs.0, bs.1), txt(cs.0, cs.1));
        let (o1, o2) = (self.text(pop), self.text(op));
        let o2b = match (o1, o2) {
            ("-", "+") => "-",
            ("-", _) => "+",
            _ => o2,
        };
        let mut text = format!(
            "`{} {o1} {} {o2} {}` works out `{o2} {}` for every pixel; bracketed, \
             `{} {o1} ({} {o2b} {})`, the brackets are worked out once a {once} instead",
            short(ta),
            short(tb),
            short(tc),
            short(tc),
            short(ta),
            short(tb),
            short(tc)
        );
        if f == 2 {
            text.push_str(
                " (a product regrouped can differ in its last bit: check it with `psi check --same-as`)",
            );
        }
        Some((start, cs.1, text))
    }

    /// The notes only the laid-out program can give (what the still cache
    /// and the column run took, and the heaviest value they could not; the
    /// draw loops run as lanes), then every note to the sink, in order.
    pub(super) fn layout_notes(
        &mut self,
        prog: &mut Program,
        (cached, col, col_lost, took): &(Vec<usize>, Vec<usize>, Option<&'static str>, Vec<usize>),
    ) {
        let dag = &self.dag;
        let n = dag.nodes.len();
        let local = |i: usize| {
            self.locals
                .iter()
                .rposition(|l| l.id as usize == i && !l.counter)
        };
        let place = |i: usize| -> Option<(usize, usize, String)> {
            if let Some(j) = local(i) {
                let l = &self.locals[j];
                return Some((l.at, l.at + l.name.len(), format!("`{}`", l.name)));
            }
            let &(_, a, b) = dag.made.iter().find(|m| m.0 as usize == i)?;
            let line = self.src[..a].bytes().filter(|&c| c == b'\n').count() + 1;
            Some((a, b, format!("a value on line {line}")))
        };
        let list = |v: &[usize], named_only: bool| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for &i in v {
                let w = if named_only {
                    local(i).map(|j| format!("`{}`", self.locals[j].name))
                } else {
                    place(i).map(|p| p.2)
                };
                if let Some(w) = w.filter(|w| !out.contains(w)) {
                    out.push(w);
                }
            }
            out
        };
        let mut notes: Vec<(usize, usize, String)> = Vec::new();
        let first = cached
            .iter()
            .chain(col.iter())
            .find_map(|&i| {
                local(i).map(|j| {
                    (
                        self.locals[j].at,
                        self.locals[j].at + self.locals[j].name.len(),
                    )
                })
            })
            .or_else(|| cached.first().and_then(|&i| place(i)).map(|p| (p.0, p.1)));
        let (kept, cols) = (list(cached, false), list(col, true));
        if !kept.is_empty() || !cols.is_empty() {
            let mut t = String::new();
            if !kept.is_empty() {
                t.push_str(&format!(
                    "still cache: {} worked out once and kept (the same every frame)",
                    kept.join(", ")
                ));
            }
            if !cols.is_empty() {
                if !t.is_empty() {
                    t.push_str("; ");
                }
                t.push_str(&format!(
                    "column run: {} worked out once a column, not for every pixel",
                    cols.join(", ")
                ));
            }
            let (a, b) = first.unwrap_or((0, 0));
            notes.push((a, b, t));
        }
        // A column run the formula has, left out (then every pixel works
        // its values out: the frame can cost several times as much).
        if let Some(why) = col_lost {
            notes.push((
                0,
                0,
                format!("column run: left out ({why}): its values are worked out for every pixel, not once a column"),
            ));
        }
        // The heaviest per-pixel value that would be still but for a value
        // that moves (a control, `t`): what keeps it out.
        let still = still_nodes(dag);
        let colnode = column_nodes(dag);
        let mut almost = alloc::vec![false; n];
        let mut sub_ns = alloc::vec![0u32; n];
        for i in 0..n {
            let m = &dag.nodes[i];
            if m.level != LV_PIXEL || m.code >= K_CONST {
                continue;
            }
            sub_ns[i] = value_ns(m.code).saturating_add(
                reads(dag, i)
                    .filter(|&x| dag.level(x) == LV_PIXEL)
                    .map(|x| sub_ns[x as usize])
                    .fold(0u32, u32::saturating_add),
            );
            almost[i] = !still[i]
                && !(code::U_R..=code::INKN).contains(&m.code)
                && !is_noise_o(m.code)
                && reads(dag, i).all(|x| {
                    let x = x as usize;
                    still[x] || almost[x] || dag.nodes[x].level <= LV_FRAME
                });
        }
        let best = (0..n)
            .filter(|&i| {
                almost[i] && !colnode[i] && sub_ns[i] >= STILL_GAIN_NS && local(i).is_some()
            })
            .max_by_key(|&i| sub_ns[i]);
        if let Some(i) = best {
            // The first value once a frame (not a constant) on the way down.
            let mut q = alloc::vec![i as Id];
            let mut k = 0;
            let mut mover = None;
            while k < q.len() && k < 256 && mover.is_none() {
                let m = q[k];
                k += 1;
                for x in reads(dag, m as usize) {
                    let lv = dag.level(x);
                    if lv == LV_FRAME {
                        mover = Some(x);
                        break;
                    }
                    if lv == LV_PIXEL && almost[x as usize] && !q.contains(&x) {
                        q.push(x);
                    }
                }
            }
            if let (Some(mv), Some((a, b, w))) = (mover, place(i)) {
                let named = |m: Id| self.locals.iter().rposition(|l| l.id == m && !l.counter);
                let by = match named(mv) {
                    Some(j) => format!("`{}`", self.locals[j].name),
                    None => match cause(dag, mv, &named) {
                        (Some(j), _) => format!("`{}`", self.locals[j].name),
                        (None, r) => format!("`{}`", input_word(&dag.nodes[r as usize]).0),
                    },
                };
                notes.push((
                    a,
                    b,
                    format!(
                        "{w} is worked out for every pixel each frame (~{} ns a pixel): it would be \
                         kept in the still cache but it reads {by}, which changes from frame to frame",
                        sub_ns[i]
                    ),
                ));
            }
        }
        notes.extend(self.saturation_notes(prog));
        notes.extend(self.loop_note(prog));
        // Draw loops run as lanes: costed for 128 turns a lane.
        prog.mark_lane_loops();
        for (k, (count, lanes)) in prog.draw_loops().into_iter().enumerate() {
            let (Some((all, turned)), Some(&(a, b))) = (lanes, self.loop_at.get(k)) else {
                continue;
            };
            notes.push((
                a,
                b,
                format!(
                    "this loop of {count} turns runs as lanes (32 turns or more): each of its steps \
                     is costed as for {} turns; the draw stage ~{all} µs a frame, ~{turned} µs with \
                     this loop a turn at a time",
                    count.div_ceil(W as u32) * W as u32
                ),
            ));
        }
        // (A pick or a sum in the column run or kept in the still cache is
        // not worked out for every pixel: no note.)
        for (a, b, t, id) in core::mem::take(&mut self.notes) {
            let i = id as usize;
            if !col.contains(&i) && !cached.contains(&i) {
                notes.push((a, b, t));
            }
        }
        notes.sort_by_key(|x| (x.0, x.1));
        // One note a place, and one split note a line.
        let mut lines: Vec<usize> = Vec::new();
        let mut last: Option<(usize, usize)> = None;
        if let Some(sink) = self.sink.as_mut() {
            // The steps the still cache and the column run took out of the
            // pixel loop, as [`Sink::op`] told them.
            for &(id, a, e) in &self.dag.made {
                let i = id as usize;
                let how = if took.contains(&i) {
                    Kept::Once
                } else if col.contains(&i) {
                    Kept::Column
                } else {
                    continue;
                };
                if self.dag.nodes[i].code < K_CONST {
                    sink.kept((a, e), self.dag.nodes[i].code, how);
                }
            }
            for (a, b, t) in &notes {
                if last == Some((*a, *b)) {
                    continue;
                }
                last = Some((*a, *b));
                if t.contains("for every pixel; bracketed") {
                    let line = self.src[..*a].bytes().filter(|&c| c == b'\n').count();
                    if lines.contains(&line) {
                        continue;
                    }
                    lines.push(line);
                }
                sink.note((*a, *b), t);
            }
        }
    }

    /// Language review B1: a `+ − *` whose exact result can pass the ends
    /// of the number range for the inputs in theirs (so it is held there,
    /// silently) and whose value goes into something that cares about it
    /// (`frac`, `%`, a wave, `noise`, `hash`, a later `*` or `/`); not one
    /// that goes only into a clamp, a compare or a colour, where a held
    /// value is the point (`gray(x*30000)`). Intervals over the DAG, as
    /// `range` has them over the laid-out program. At most three notes.
    fn saturation_notes(&self, prog: &Program) -> Vec<(usize, usize, String)> {
        use crate::range::{add_iv, mul_iv, sub_iv, value, Iv, FULL_IV};
        let dag = &self.dag;
        let mut out = Vec::new();
        if dag.forever {
            return out;
        }
        let n = dag.nodes.len();
        let unit = Iv(0, ONE as i64);
        let declared = prog.meta.declared();
        let input = |v: i32| -> Iv {
            let v = v as u8;
            if is_lane(v) {
                return match lane_no(v) {
                    L_X => unit,
                    L_PX => Iv(0, 127 * ONE as i64),
                    L_I => Iv(0, 4095 * ONE as i64),
                    _ => FULL_IV,
                };
            }
            match v {
                S_Y => unit,
                S_PY => Iv(0, 31 * ONE as i64),
                S_T => Iv(0, i64::from(prog.loop_ms) * ONE as i64 / 1000),
                S_HOUR => Iv(0, 23 * ONE as i64),
                S_MINUTE | S_SECOND => Iv(0, 59 * ONE as i64),
                S_A | S_B | S_C if declared & (1 << (v - S_A)) == 0 => unit,
                S_SEED => unit,
                _ => FULL_IV,
            }
        };
        // (An interval reaching an end is one `range` could not bound,
        // `len`'s or `sqrt`'s: no note from it.)
        let known = |v: Iv| v.0 > i64::from(i32::MIN) && v.1 < i64::from(i32::MAX);
        let mut iv = alloc::vec![FULL_IV; n];
        let mut raw = alloc::vec![false; n];
        // Whether each value is made from `t` (P1.5: a product of it that
        // passes the end within the loop holds still from there, whatever
        // it goes into).
        let mut of_t = alloc::vec![false; n];
        for i in 0..n {
            let m = &dag.nodes[i];
            of_t[i] = match m.code {
                K_INPUT => key_byte(m).is_none() && m.val == i32::from(S_T),
                c if c >= K_CONST => false,
                c => {
                    let ar = code::arity(c);
                    [m.a, m.b, m.c].iter().take(ar).any(|&x| of_t[x as usize])
                }
            };
            iv[i] = match m.code {
                K_CONST => Iv::point(m.val),
                K_INPUT => match key_byte(m) {
                    Some(b) => {
                        let (lo, hi) = crate::world::range(b & 63, b >> 6);
                        Iv(i64::from(lo), i64::from(hi))
                    }
                    None => input(m.val),
                },
                c if c >= K_CONST => FULL_IV,
                code::TAB | code::SPR | code::PICK => FULL_IV,
                c => {
                    let ar = code::arity(c);
                    let g = |k: usize, x: Id| if k < ar { iv[x as usize] } else { Iv::point(0) };
                    let (a, b, cc) = (g(0, m.a), g(1, m.b), g(2, m.c));
                    // A loop's counter or a value of one: not known here.
                    if m.level == LV_LOOP1 || m.level == LV_LOOP2 {
                        FULL_IV
                    } else {
                        raw[i] = match c {
                            code::ADD => !add_iv(a, b).fits(),
                            code::SUB => !sub_iv(a, b).fits(),
                            code::MUL => !mul_iv(a, b).fits(),
                            _ => false,
                        } && known(a)
                            && known(b);
                        let op = Op { code: c, d: 0, a: 0, b: 0, c: 0 };
                        value(&op, a, b, cc, &[0])
                    }
                }
            };
        }
        let cares = |c: u8| {
            matches!(
                c,
                code::FRAC
                    | code::MOD
                    | code::MASK
                    | code::SIN
                    | code::COS
                    | code::TRI
                    | code::SAW
                    | code::SQR
                    | code::NOISE
                    | code::HASH
                    | code::MUL
                    | code::DIV
            )
        };
        for (i, _) in raw.iter().enumerate().filter(|r| *r.1) {
            if out.len() >= 3 {
                break;
            }
            // (A product of `t` that only a `min` or a `clamp` reads is a
            // ramp held at its end on purpose.)
            let held = {
                let mut by = (0..n).filter(|&j| dag.nodes[j].code < K_CONST && reads(dag, j).any(|x| x as usize == i));
                let first = by.next();
                first.is_some() && first.into_iter().chain(by).all(|j| matches!(dag.nodes[j].code, code::MIN | code::CLAMP))
            };
            let timed = of_t[i] && dag.nodes[i].code == code::MUL && !held;
            let user = (0..n).find(|&j| {
                let m = &dag.nodes[j];
                m.code < K_CONST && cares(m.code) && reads(dag, j).any(|x| x as usize == i)
            });
            if user.is_none() && !timed {
                continue;
            }
            // (A product of a product of `t`: the first one said it.)
            if timed && reads(dag, i).any(|x| raw[x as usize] && of_t[x as usize]) {
                continue;
            }
            let Some(&(_, a, b)) = dag.made.iter().find(|m| m.0 as usize == i) else {
                continue;
            };
            let m = &dag.nodes[i];
            let (x, y) = (iv[m.a as usize], iv[m.b as usize]);
            let exact = match m.code {
                code::ADD => add_iv(x, y),
                code::SUB => sub_iv(x, y),
                _ => mul_iv(x, y),
            };
            let reach = (exact.1.max(-exact.0) >> 16).max(32768);
            if timed {
                let named = self
                    .locals
                    .iter()
                    .rposition(|l| l.id as usize == i && !l.counter)
                    .map_or_else(|| "this product of `t`".to_string(), |k| format!("`{}`, a product of `t`,", self.locals[k].name));
                let s = |v: u32| {
                    let t = format!("{}.{:02}", v / 1000, v % 1000 / 10);
                    t.trim_end_matches('0').trim_end_matches('.').to_string()
                };
                out.push((
                    a,
                    b,
                    format!(
                        "{named} may pass 32,767 before the loop ends (up to about {} by t = {} s, reckoned from \
                         each factor's range): past it, it is held at the end and stops moving. Scale it down \
                         (divide first, or a smaller factor), or wrap `t` first, as frac(t/k)",
                        group(reach),
                        s(prog.loop_ms)
                    ),
                ));
                continue;
            }
            let Some(user) = user else { continue };
            let what = match dag.nodes[user].code {
                code::MUL => "*",
                code::DIV => "/",
                code::MOD | code::MASK => "%",
                c => code::name(c),
            };
            out.push((
                a,
                b,
                {
                    let kind = match m.code {
                        code::MUL => "this product",
                        _ => "this sum",
                    };
                    let named = self
                        .locals
                        .iter()
                        .rposition(|l| l.id as usize == i && !l.counter)
                        .map(|k| format!("`{}`", self.locals[k].name));
                    let ls = self.src[..a].rfind('\n').map_or(0, |k| k + 1);
                    let le = self.src[b..].find('\n').map_or(self.src.len(), |k| b + k);
                    let line = short(sub(self.src, ls, le).trim());
                    format!(
                        "{} can pass ±32,768 (to about {} here, in `{line}`) and is then held at the end: the `{what}` it goes into sees a flat value there. Scale it down before it grows (divide first, or use smaller units)",
                        named.unwrap_or_else(|| kind.to_string()),
                        group(reach)
                    )
                },
            ));
        }
        out
    }

    /// Language review P1.4: a `loop` that is not a whole number of the
    /// frames it is drawn in (20 ms, or 40 with `fps 25`) in a formula that
    /// moves. Every player draws whole frames of its
    /// grid (10, 20 or 40 ms: every refresh, the 2nd or the 4th; a saved
    /// GIF 20 or 40), and the loop is cut to them (`fx_psi`'s
    /// `loop_frames`, the web app's preview and GIF), so its length, and
    /// where `t` jumps back, differ by the rate it gets.
    fn loop_note(&self, prog: &Program) -> Option<(usize, usize, String)> {
        let ms = prog.loop_ms;
        // The frames it is drawn in: 20 ms (every 2nd refresh, the default,
        // and where the panel holds a `smooth` one that is too heavy), 40
        // with `fps 25`. (A multiple of 20 ms only cut when a heavy one is
        // held to every 4th refresh: no note.)
        let grid = prog.frame_ms().unwrap_or(20).max(20);
        if ms == 0 || ms.is_multiple_of(grid) || prog.inputs() & 8 == 0 {
            return None;
        }
        // The `loop` line's number: a statement that starts `loop` and a
        // number (the tokens are gone by now; comments skipped).
        let b = self.src.as_bytes();
        let (mut i, mut start, mut at) = (0, true, None);
        while i < b.len() {
            let c = b[i];
            if (c == b'-' || c == b'/') && b.get(i + 1) == Some(&c) {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if start && b[i..].starts_with(b"loop") && b.get(i + 4).is_some_and(|c| *c == b' ' || *c == b'\t') {
                let a = i + 4 + b[i + 4..].iter().take_while(|c| **c == b' ' || **c == b'\t').count();
                let e = a + b[a..].iter().take_while(|c| c.is_ascii_digit() || **c == b'.').count();
                if e > a {
                    at = Some((a, e));
                    break;
                }
            }
            if !c.is_ascii_whitespace() || c == b'\n' {
                start = matches!(c, b'\n' | b';');
            }
            i += 1;
        }
        let (a, e) = at?;
        let s = |v: u32| {
            let t = format!("{}.{:03}", v / 1000, v % 1000);
            t.trim_end_matches('0').trim_end_matches('.').to_string()
        };
        Some((
            a,
            e,
            format!(
                "`loop {}` is not a whole number of {} ms frames: a GIF saved from it is cut to {} s, as is the \
                 panel's loop when it can't follow its scan (a `rate` layer, the glow), and `t` jumps back there; \
                 {} loops without a jump everywhere",
                sub(self.src, a, e),
                grid,
                s(ms / grid * grid),
                // The 40 ms multiples either side: whole frames at every rate.
                match ms / 40 * 40 {
                    0 => format!("`loop {}`", s(40)),
                    lo => format!("`loop {}` or `loop {}`", s(lo), s(lo + 40)),
                }
            ),
        ))
    }

    /// Radians in a wave (`sin(x*6.28)`, a habit from other languages):
    /// a note at the number.
    pub(super) fn radians_notes(&self, sink: &mut dyn crate::syntax::Sink) {
        for k in 0..self.toks.len().saturating_sub(2) {
            let t = self.toks[k].get();
            if t.tok != Tok::Name
                || !matches!(self.text(t), "sin" | "cos")
                || self.toks[k + 1].get().tok != Tok::LParen
            {
                continue;
            }
            let (mut depth, mut j) = (0, k + 1);
            while j < self.toks.len() {
                let u = self.toks[j].get();
                match u.tok {
                    Tok::LParen => depth += 1,
                    Tok::RParen => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Tok::Num(v)
                        if [(3, 1416), (6, 2832), (1, 5708)].iter().any(|&(w, f)| {
                            let pi = (w << 16) + (f << 16) / 10000;
                            (v - pi).abs() < ONE / 50
                        }) =>
                    {
                        sink.note(
                                (u.start, u.end),
                                &format!(
                                    "`{}` inside {}: Psi's waves take turns, not radians — {}(v) goes round once as v goes from 0 to 1, so there is no pi to multiply by (`sin(x*6.28)` goes round six times; write `sin(x)` for once)",
                                    self.text(u),
                                    self.text(t),
                                    self.text(t)
                                ),
                            );
                    }
                    Tok::Eof => break,
                    _ => {}
                }
                j += 1;
            }
        }
    }
}
