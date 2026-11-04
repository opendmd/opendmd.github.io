//! The language service's notes (feature `syntax`; `syntax::Sink::note`,
//! printed by `psi check`): where a Formula costs more than it needs to,
//! and why — a `pick` worked out per pixel, a sum or product that brackets
//! better, what the still cache and the column run took (and the heaviest
//! value they couldn't), draw loops run as lanes. A child of `compile`:
//! it reads the parser's and the DAG's own fields. Never in the firmware.

use super::*;

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
        (cached, col, col_lost): &(Vec<usize>, Vec<usize>, Option<&'static str>),
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
