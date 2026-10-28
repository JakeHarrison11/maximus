//! Textbook layout engine. Turns a math tree into positioned drawing operations.
//! It is independent of egui: text measurement is supplied through [`Measure`].

use maximus_math::words::*;
use maximus_math::{BigOp, Item};

/// Cursor path: for each nesting level, (item index in the row, slot inside that item).
pub type Path = Vec<(usize, usize)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontKind {
    Roman,
    Italic,
}

pub trait Measure {
    fn width(&self, text: &str, size: f32, font: FontKind) -> f32;
}

#[derive(Clone, Debug)]
pub enum Op {
    /// `y` is the baseline.
    Text { x: f32, y: f32, text: String, size: f32, font: FontKind },
    Line { x0: f32, y0: f32, x1: f32, y1: f32, width: f32 },
    Poly { pts: Vec<(f32, f32)>, width: f32 },
    /// Dotted empty-slot marker.
    Placeholder { x: f32, y: f32, w: f32, h: f32 },
}

/// A caret position inside the layout (editor mode).
#[derive(Clone, Debug)]
pub struct Mark {
    pub path: Path,
    pub pos: usize,
    pub x: f32,
    pub y0: f32,
    pub y1: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Boxed {
    pub w: f32,
    pub asc: f32,
    pub desc: f32,
    pub ops: Vec<Op>,
    pub marks: Vec<Mark>,
}

impl Boxed {
    pub fn height(&self) -> f32 {
        self.asc + self.desc
    }

    /// Adds `child` with its origin at (`dx`, `dy`) relative to this box's baseline origin.
    pub fn place(&mut self, child: Boxed, dx: f32, dy: f32) {
        for mut op in child.ops {
            match &mut op {
                Op::Text { x, y, .. } => {
                    *x += dx;
                    *y += dy;
                }
                Op::Line { x0, y0, x1, y1, .. } => {
                    *x0 += dx;
                    *x1 += dx;
                    *y0 += dy;
                    *y1 += dy;
                }
                Op::Poly { pts, .. } => {
                    for p in pts {
                        p.0 += dx;
                        p.1 += dy;
                    }
                }
                Op::Placeholder { x, y, .. } => {
                    *x += dx;
                    *y += dy;
                }
            }
            self.ops.push(op);
        }
        for mut m in child.marks {
            m.x += dx;
            m.y0 += dy;
            m.y1 += dy;
            self.marks.push(m);
        }
        self.asc = self.asc.max(child.asc - dy);
        self.desc = self.desc.max(child.desc + dy);
    }
}

pub struct Layout<'a> {
    pub m: &'a dyn Measure,
    /// Editor mode: record caret marks and draw placeholders for empty slots.
    pub editing: bool,
    pub root_size: f32,
}

const AXIS: f32 = 0.3;
const ASC: f32 = 0.78;
const DESC: f32 = 0.22;

#[derive(Clone, Copy, PartialEq)]
enum Prev {
    Start,
    Operand,
    Operator,
    Open,
}

fn is_binary(c: char) -> bool {
    matches!(c, '+' | '−' | '=' | '<' | '>' | '≤' | '≥' | '≠' | '→' | '≈' | '±' | '×' | '÷' | '∧' | '∨' | '∈')
}

struct State {
    out: Boxed,
    prev: Prev,
    pending_gap: f32,
    /// (start x, width, asc, desc) of the last operand piece, for attaching scripts.
    last: Option<(f32, f32, f32, f32)>,
}

impl<'a> Layout<'a> {
    pub fn new(m: &'a dyn Measure, size: f32, editing: bool) -> Self {
        Layout { m, editing, root_size: size }
    }

    pub fn layout(&self, row: &[Item]) -> Boxed {
        let mut path = Vec::new();
        self.row(row, self.root_size, &mut path)
    }

    fn min_size(&self) -> f32 {
        self.root_size * 0.5
    }

    fn text_box(&self, text: &str, size: f32, font: FontKind) -> Boxed {
        let w = self.m.width(text, size, font);
        Boxed {
            w,
            asc: ASC * size,
            desc: DESC * size,
            ops: vec![Op::Text { x: 0.0, y: 0.0, text: text.to_string(), size, font }],
            marks: vec![],
        }
    }

    fn row(&self, row: &[Item], size: f32, path: &mut Path) -> Boxed {
        self.row_from(row, 0, size, path, Prev::Start)
    }

    fn mark(&self, st: &mut State, path: &Path, pos: usize, size: f32) {
        if self.editing {
            st.out.marks.push(Mark { path: path.clone(), pos, x: st.out.w, y0: -ASC * size, y1: DESC * size });
        }
    }

    fn row_from(&self, row: &[Item], start: usize, size: f32, path: &mut Path, prev: Prev) -> Boxed {
        let mut st = State {
            out: Boxed { w: 0.0, asc: ASC * size, desc: DESC * size, ops: vec![], marks: vec![] },
            prev,
            pending_gap: 0.0,
            last: None,
        };
        let mut i = start;
        while i < row.len() {
            self.mark(&mut st, path, i, size);
            let next_is_script_or_paren =
                matches!(row.get(i), Some(Item::Sup(_) | Item::Sub(_) | Item::Group('(', _, _)));
            if !next_is_script_or_paren {
                st.out.w += st.pending_gap;
            }
            st.pending_gap = 0.0;
            match &row[i] {
                Item::Char(c) if c.is_ascii_digit() || *c == '.' => {
                    let mut s = String::new();
                    let mut j = i;
                    while let Some(Item::Char(d)) = row.get(j) {
                        if d.is_ascii_digit() || *d == '.' {
                            s.push(*d);
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    let x0 = st.out.w;
                    let b = self.text_box(&s, size, FontKind::Roman);
                    self.push_piece(&mut st, b, true);
                    // marks inside the digit run
                    if self.editing {
                        let n = j - i;
                        for k in 1..n {
                            let x = x0 + self.m.width(&s[..k], size, FontKind::Roman);
                            st.out.marks.push(Mark { path: path.clone(), pos: i + k, x, y0: -ASC * size, y1: DESC * size });
                        }
                    }
                    st.prev = Prev::Operand;
                    i = j;
                    continue;
                }
                Item::Char(c) if c.is_ascii_alphabetic() => {
                    let (run, j) = collect_run(row, i);
                    let words = split_letters(&run);
                    let nwords = words.len();
                    for (wi, (a, b, w)) in words.into_iter().enumerate() {
                        let (txt, font, is_func) = match &w {
                            Word::Func(f) => (display_name(f), FontKind::Roman, true),
                            Word::Greek(g) => (g.to_string(), if g.is_uppercase() { FontKind::Roman } else { FontKind::Italic }, false),
                            Word::Inf => ("∞".to_string(), FontKind::Roman, false),
                            Word::Letter(c) => (c.to_string(), FontKind::Italic, false),
                            Word::Num(n) => (n.clone(), FontKind::Roman, false),
                        };
                        if is_func && st.prev == Prev::Operand && wi == 0 {
                            st.out.w += 0.12 * size;
                        }
                        let x0 = st.out.w;
                        let bx = self.text_box(&txt, size, font);
                        let width = bx.w;
                        self.push_piece(&mut st, bx, true);
                        if self.editing {
                            for k in 1..(b - a) {
                                let x = x0 + width * (k as f32) / ((b - a) as f32);
                                st.out.marks.push(Mark { path: path.clone(), pos: i + a + k, x, y0: -ASC * size, y1: DESC * size });
                            }
                            if wi + 1 < nwords {
                                st.out.marks.push(Mark { path: path.clone(), pos: i + b, x: st.out.w, y0: -ASC * size, y1: DESC * size });
                            }
                        }
                        if is_func {
                            st.pending_gap = 0.14 * size;
                        }
                    }
                    st.prev = Prev::Operand;
                    i = j;
                    continue;
                }
                Item::Char(c) => {
                    let c = *c;
                    // ":=" is shown as one token.
                    if c == ':' && matches!(row.get(i + 1), Some(Item::Char('='))) {
                        st.out.w += 0.25 * size;
                        let b = self.text_box(":=", size, FontKind::Roman);
                        self.push_piece(&mut st, b, false);
                        st.out.w += 0.25 * size;
                        st.prev = Prev::Operator;
                        i += 2;
                        continue;
                    }
                    self.char_item(&mut st, c, size);
                }
                Item::Func(name) => {
                    if st.prev == Prev::Operand {
                        st.out.w += 0.12 * size;
                    }
                    let b = self.text_box(&display_name(name), size, FontKind::Roman);
                    self.push_piece(&mut st, b, true);
                    st.pending_gap = 0.14 * size;
                    st.prev = Prev::Operand;
                }
                Item::Text(t) => {
                    let b = self.text_box(t, size, FontKind::Roman);
                    self.push_piece(&mut st, b, true);
                    st.prev = Prev::Operand;
                }
                Item::Sup(r) => {
                    // Stack with an adjacent subscript.
                    let sub = match row.get(i + 1) {
                        Some(Item::Sub(s)) => Some((i + 1, s)),
                        _ => None,
                    };
                    self.scripts(&mut st, size, path, Some((i, r)), sub);
                    i += if sub.is_some() { 2 } else { 1 };
                    continue;
                }
                Item::Sub(r) => {
                    let sup = match row.get(i + 1) {
                        Some(Item::Sup(s)) => Some((i + 1, s)),
                        _ => None,
                    };
                    self.scripts(&mut st, size, path, sup, Some((i, r)));
                    i += if sup.is_some() { 2 } else { 1 };
                    continue;
                }
                Item::Frac(n, d) => {
                    let s2 = (size * 0.9).max(self.min_size());
                    path.push((i, 0));
                    let nb = self.slot(n, s2, path);
                    path.pop();
                    path.push((i, 1));
                    let db = self.slot(d, s2, path);
                    path.pop();
                    let b = self.frac(nb, db, size);
                    self.push_piece(&mut st, b, true);
                    st.prev = Prev::Operand;
                }
                Item::Sqrt(idx, body) => {
                    path.push((i, 0));
                    let bb = self.slot(body, size, path);
                    path.pop();
                    let ib = idx.as_ref().map(|r| {
                        path.push((i, 1));
                        let b = self.slot(r, (size * 0.6).max(self.min_size()), path);
                        path.pop();
                        b
                    });
                    let b = self.sqrt(bb, ib, size);
                    self.push_piece(&mut st, b, true);
                    st.prev = Prev::Operand;
                }
                Item::Group(o, c, body) => {
                    path.push((i, 0));
                    let bb = self.slot_group(body, size, path);
                    path.pop();
                    let b = self.group(*o, *c, bb, size);
                    self.push_piece(&mut st, b, true);
                    st.prev = Prev::Operand;
                }
                Item::Matrix(rows) => {
                    let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
                    let mut cells: Vec<Vec<Boxed>> = Vec::new();
                    for (ri, r) in rows.iter().enumerate() {
                        let mut line = Vec::new();
                        for (ci, c) in r.iter().enumerate() {
                            path.push((i, ri * ncols + ci));
                            line.push(self.slot(c, size, path));
                            path.pop();
                        }
                        cells.push(line);
                    }
                    let b = self.matrix(cells, size);
                    self.push_piece(&mut st, b, true);
                    st.prev = Prev::Operand;
                }
                Item::Big(op, lo, hi) => {
                    path.push((i, 0));
                    let lob = if (lo.is_empty() && !self.editing) || *op == BigOp::IntNoLimits { None } else { Some(self.slot(lo, (size * 0.7).max(self.min_size()), path)) };
                    path.pop();
                    path.push((i, 1));
                    let hib = if (hi.is_empty() && !self.editing) || *op == BigOp::IntNoLimits { None } else if *op == BigOp::Lim { None } else { Some(self.slot(hi, (size * 0.7).max(self.min_size()), path)) };
                    path.pop();
                    // Lay out the operand first so the sign can adapt to its height.
                    let rest = self.row_from(row, i + 1, size, path, Prev::Open);
                    let sign = self.big(*op, lob, hib, size, rest.height());
                    let gap = 0.12 * size;
                    let sw = sign.w + gap;
                    let mut sign = sign;
                    sign.w = sw;
                    self.push_piece(&mut st, sign, false);
                    let rw = rest.w;
                    let x = st.out.w;
                    st.out.place(rest, x, 0.0);
                    st.out.w = x + rw;
                    st.last = None;
                    if self.editing {
                        st.out.marks.push(Mark { path: path.clone(), pos: row.len(), x: st.out.w, y0: -ASC * size, y1: DESC * size });
                    }
                    return st.out;
                }
            }
            i += 1;
        }
        self.mark(&mut st, path, row.len(), size);
        st.out
    }

    fn push_piece(&self, st: &mut State, b: Boxed, operand: bool) {
        let x = st.out.w;
        let (w, asc, desc) = (b.w, b.asc, b.desc);
        st.out.place(b, x, 0.0);
        st.out.w = x + w;
        if operand {
            st.last = Some((x, w, asc, desc));
        }
    }

    fn char_item(&self, st: &mut State, c: char, size: f32) {
        let binary_space = 0.24 * size;
        match c {
            c if is_binary(c) => {
                let unary = matches!(c, '+' | '−' | '±') && st.prev != Prev::Operand;
                let glyph = c.to_string();
                if unary {
                    let b = self.text_box(&glyph, size, FontKind::Roman);
                    self.push_piece(st, b, false);
                    st.prev = Prev::Open;
                } else {
                    st.out.w += binary_space;
                    let b = self.text_box(&glyph, size, FontKind::Roman);
                    self.push_piece(st, b, false);
                    st.out.w += binary_space;
                    st.prev = Prev::Operator;
                }
            }
            '·' => {
                st.out.w += 0.12 * size;
                let b = self.text_box("·", size, FontKind::Roman);
                self.push_piece(st, b, false);
                st.out.w += 0.12 * size;
                st.prev = Prev::Operator;
            }
            ',' | ';' => {
                let b = self.text_box(&c.to_string(), size, FontKind::Roman);
                self.push_piece(st, b, false);
                st.out.w += 0.22 * size;
                st.prev = Prev::Operator;
            }
            ':' => {
                st.out.w += 0.2 * size;
                let b = self.text_box(":", size, FontKind::Roman);
                self.push_piece(st, b, false);
                st.out.w += 0.2 * size;
                st.prev = Prev::Operator;
            }
            '/' => {
                let b = self.text_box("/", size, FontKind::Roman);
                self.push_piece(st, b, false);
                st.prev = Prev::Operator;
            }
            'ⅈ' | 'ⅇ' => {
                let s = if c == 'ⅈ' { "i" } else { "e" };
                let b = self.text_box(s, size, FontKind::Roman);
                self.push_piece(st, b, true);
                st.prev = Prev::Operand;
            }
            c => {
                let font = if c.is_alphabetic() && !c.is_uppercase() { FontKind::Italic } else { FontKind::Roman };
                let b = self.text_box(&c.to_string(), size, font);
                self.push_piece(st, b, true);
                st.prev = if matches!(c, '(' | '[' | '{') { Prev::Open } else { Prev::Operand };
            }
        }
    }

    /// Lays out a nested row; empty slots show a placeholder when editing.
    fn slot(&self, row: &[Item], size: f32, path: &mut Path) -> Boxed {
        if row.is_empty() && self.editing {
            let w = 0.6 * size;
            let mut b = Boxed {
                w,
                asc: ASC * size,
                desc: DESC * size,
                ops: vec![Op::Placeholder { x: 0.0, y: -ASC * size, w, h: (ASC + DESC) * size }],
                marks: vec![],
            };
            b.marks.push(Mark { path: path.clone(), pos: 0, x: w * 0.5, y0: -ASC * size, y1: DESC * size });
            return b;
        }
        self.row(row, size, path)
    }

    fn slot_group(&self, row: &[Item], size: f32, path: &mut Path) -> Boxed {
        if row.is_empty() && self.editing {
            return self.slot(row, size, path);
        }
        self.row(row, size, path)
    }

    fn scripts(&self, st: &mut State, size: f32, path: &mut Path, sup: Option<(usize, &Vec<Item>)>, sub: Option<(usize, &Vec<Item>)>) {
        let s2 = (size * 0.7).max(self.min_size());
        let (bx, bw, basc, bdesc) = st.last.unwrap_or((st.out.w, 0.0, ASC * size, DESC * size));
        let _ = (bx, bw);
        let supb = sup.map(|(i, r)| {
            path.push((i, 0));
            let b = self.slot(r, s2, path);
            path.pop();
            b
        });
        let subb = sub.map(|(i, r)| {
            path.push((i, 0));
            let b = self.slot(r, s2, path);
            path.pop();
            b
        });
        let x = st.out.w;
        let mut width: f32 = 0.0;
        if let Some(b) = supb {
            let up = (basc - 0.55 * s2).max(0.42 * size).max(b.desc + 0.18 * size);
            width = width.max(b.w);
            st.out.place(b, x, -up);
        }
        if let Some(b) = subb {
            let down = (bdesc + b.asc * 0.4).max(0.2 * size);
            width = width.max(b.w);
            st.out.place(b, x, down);
        }
        st.out.w = x + width + 0.04 * size;
        st.last = None;
        st.prev = Prev::Operand;
    }

    fn frac(&self, n: Boxed, d: Boxed, size: f32) -> Boxed {
        let axis = AXIS * size;
        let gap = 0.16 * size;
        let pad = 0.12 * size;
        let w = n.w.max(d.w) + 2.0 * pad;
        let mut b = Boxed { w, asc: 0.0, desc: 0.0, ops: vec![], marks: vec![] };
        let (nw, dw) = (n.w, d.w);
        let (nd, da) = (n.desc, d.asc);
        let (nasc, ddesc) = (n.asc, d.desc);
        b.place(n, (w - nw) / 2.0, -(axis + gap + nd));
        b.place(d, (w - dw) / 2.0, -axis + gap + da);
        b.ops.push(Op::Line { x0: 0.0, y0: -axis, x1: w, y1: -axis, width: (size * 0.06).max(1.0) });
        b.asc = b.asc.max(axis + gap + nd + nasc);
        b.desc = b.desc.max(-axis + gap + da + ddesc);
        b
    }

    fn sqrt(&self, body: Boxed, idx: Option<Boxed>, size: f32) -> Boxed {
        let stroke = (size * 0.06).max(1.0);
        let top = -(body.asc + 0.2 * size);
        let bottom = body.desc + 0.06 * size;
        let h = bottom - top;
        let sw = 0.55 * size;
        let pad = 0.1 * size;
        let idx_w = idx.as_ref().map_or(0.0, |i| (i.w - 0.3 * sw).max(0.0));
        let x0 = idx_w;
        let ymid = bottom - 0.45 * h;
        let bw = body.w;
        let (basc, bdesc) = (body.asc, body.desc);
        let mut b = Boxed { w: x0 + sw + pad + bw + pad, asc: -top, desc: bottom, ops: vec![], marks: vec![] };
        b.ops.push(Op::Poly {
            pts: vec![
                (x0, ymid),
                (x0 + 0.14 * sw, ymid - 0.03 * h),
                (x0 + 0.45 * sw, bottom),
                (x0 + sw, top),
                (x0 + sw + pad + bw + pad, top),
            ],
            width: stroke,
        });
        b.place(body, x0 + sw + pad, 0.0);
        let _ = (basc, bdesc);
        if let Some(i) = idx {
            let iw = i.w;
            let id = i.desc;
            b.place(i, (x0 - iw + 0.35 * sw).max(0.0), ymid - 0.2 * size - id);
        }
        b
    }

    fn group(&self, open: char, close: char, body: Boxed, size: f32) -> Boxed {
        let mut asc = body.asc.max(ASC * size);
        let mut desc = body.desc.max(DESC * size);
        let tall = (asc + desc) > 1.45 * size;
        if tall {
            asc += 0.06 * size;
            desc += 0.06 * size;
        }
        let (lw, rw) = (self.delim_width(open, tall, size), self.delim_width(close, tall, size));
        let bw = body.w;
        let mut b = Boxed { w: lw + bw + rw, asc, desc, ops: vec![], marks: vec![] };
        self.draw_delim(&mut b, open, true, 0.0, asc, desc, lw, tall, size);
        b.place(body, lw, 0.0);
        self.draw_delim(&mut b, close, false, lw + bw, asc, desc, rw, tall, size);
        b.asc = asc;
        b.desc = desc;
        b
    }

    fn delim_width(&self, c: char, tall: bool, size: f32) -> f32 {
        if !tall {
            return self.m.width(&c.to_string(), size, FontKind::Roman);
        }
        match c {
            '(' | ')' => 0.34 * size,
            '[' | ']' => 0.3 * size,
            '{' | '}' => 0.4 * size,
            _ => 0.22 * size,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_delim(&self, b: &mut Boxed, c: char, left: bool, x: f32, asc: f32, desc: f32, w: f32, tall: bool, size: f32) {
        if !tall {
            b.ops.push(Op::Text { x, y: 0.0, text: c.to_string(), size, font: FontKind::Roman });
            return;
        }
        let stroke = (size * 0.06).max(1.0);
        let (top, bot) = (-asc, desc);
        let h = bot - top;
        let pad = stroke;
        let inner = w - 2.0 * pad;
        // Mirror helper: x offset from the outer edge (left=true) or inner edge.
        let px = |t: f32| -> f32 { if left { x + pad + t } else { x + pad + inner - t } };
        match c {
            '(' | ')' => {
                let n = 12;
                let pts = (0..=n)
                    .map(|k| {
                        let t = k as f32 / n as f32;
                        let u = 2.0 * t - 1.0;
                        let off = inner * u * u;
                        (px(off), top + t * h)
                    })
                    .collect();
                b.ops.push(Op::Poly { pts, width: stroke });
            }
            '[' | ']' => {
                b.ops.push(Op::Poly { pts: vec![(px(inner), top), (px(0.0), top), (px(0.0), bot), (px(inner), bot)], width: stroke });
            }
            '{' | '}' => {
                let pts = vec![
                    (px(inner), top),
                    (px(inner * 0.45), top + 0.06 * h),
                    (px(inner * 0.45), top + 0.42 * h),
                    (px(0.0), top + 0.5 * h),
                    (px(inner * 0.45), top + 0.58 * h),
                    (px(inner * 0.45), top + 0.94 * h),
                    (px(inner), bot),
                ];
                b.ops.push(Op::Poly { pts, width: stroke });
            }
            _ => {
                let xm = x + w / 2.0;
                b.ops.push(Op::Line { x0: xm, y0: top, x1: xm, y1: bot, width: stroke });
            }
        }
    }

    fn matrix(&self, cells: Vec<Vec<Boxed>>, size: f32) -> Boxed {
        let ncols = cells.iter().map(|r| r.len()).max().unwrap_or(0);
        let col_gap = 0.9 * size;
        let row_gap = 0.3 * size;
        let mut colw = vec![0.0f32; ncols];
        let mut rows_h: Vec<(f32, f32)> = Vec::new();
        for r in &cells {
            let mut a: f32 = 0.0;
            let mut d: f32 = 0.0;
            for (ci, c) in r.iter().enumerate() {
                colw[ci] = colw[ci].max(c.w);
                a = a.max(c.asc);
                d = d.max(c.desc);
            }
            rows_h.push((a, d));
        }
        let total_h: f32 = rows_h.iter().map(|(a, d)| a + d).sum::<f32>() + row_gap * (cells.len().saturating_sub(1)) as f32;
        let inner_w: f32 = colw.iter().sum::<f32>() + col_gap * ncols.saturating_sub(1) as f32;
        let axis = AXIS * size;
        let asc = total_h / 2.0 + axis;
        let desc = total_h / 2.0 - axis;
        let mut grid = Boxed { w: inner_w, asc, desc, ops: vec![], marks: vec![] };
        let mut y = -asc;
        for (r, (ra, rd)) in cells.into_iter().zip(rows_h.iter()) {
            let base = y + ra;
            let mut x = 0.0;
            for (ci, c) in r.into_iter().enumerate() {
                let cw = c.w;
                grid.place(c, x + (colw[ci] - cw) / 2.0, base);
                x += colw[ci] + col_gap;
            }
            y += ra + rd + row_gap;
        }
        grid.asc = asc;
        grid.desc = desc;
        self.group('(', ')', grid, size)
    }

    fn big(&self, op: BigOp, lo: Option<Boxed>, hi: Option<Boxed>, size: f32, rest_h: f32) -> Boxed {
        let axis = AXIS * size;
        match op {
            BigOp::Int | BigOp::IntNoLimits => {
                let ss = (rest_h * 0.95).clamp(1.5 * size, 3.4 * size);
                let sw = self.m.width("∫", ss, FontKind::Roman);
                let half = 0.55 * ss;
                let mut b = Boxed { w: sw, asc: axis + half, desc: half - axis, ops: vec![], marks: vec![] };
                b.ops.push(Op::Text { x: 0.0, y: -axis + 0.28 * ss, text: "∫".into(), size: ss, font: FontKind::Roman });
                let mut lim_w: f32 = 0.0;
                if let Some(h) = hi {
                    lim_w = lim_w.max(h.w);
                    let y = -(axis + half) + h.asc * 0.9;
                    b.place(h, sw * 0.95, y);
                }
                if let Some(l) = lo {
                    lim_w = lim_w.max(l.w);
                    let y = (half - axis) - l.desc * 0.9;
                    b.place(l, sw * 0.45, y);
                }
                b.w = if lim_w > 0.0 { sw * 0.95 + lim_w } else { sw };
                b
            }
            BigOp::Sum | BigOp::Prod => {
                let ss = (rest_h * 0.9).clamp(1.5 * size, 3.0 * size);
                let glyph = if op == BigOp::Sum { "∑" } else { "∏" };
                let sw = self.m.width(glyph, ss, FontKind::Roman);
                let half = 0.5 * ss;
                let gap = 0.1 * size;
                let w = sw.max(hi.as_ref().map_or(0.0, |h| h.w)).max(lo.as_ref().map_or(0.0, |l| l.w));
                let mut b = Boxed { w, asc: axis + half, desc: half - axis, ops: vec![], marks: vec![] };
                b.ops.push(Op::Text { x: (w - sw) / 2.0, y: -axis + 0.3 * ss, text: glyph.into(), size: ss, font: FontKind::Roman });
                if let Some(h) = hi {
                    let hw = h.w;
                    let y = -(axis + half) - gap - h.desc;
                    b.place(h, (w - hw) / 2.0, y);
                }
                if let Some(l) = lo {
                    let lw = l.w;
                    let y = (half - axis) + gap + l.asc;
                    b.place(l, (w - lw) / 2.0, y);
                }
                b
            }
            BigOp::Lim => {
                let t = self.text_box("lim", size, FontKind::Roman);
                let tw = t.w;
                let w = tw.max(lo.as_ref().map_or(0.0, |l| l.w));
                let mut b = Boxed { w, asc: t.asc, desc: t.desc, ops: vec![], marks: vec![] };
                b.place(t, (w - tw) / 2.0, 0.0);
                if let Some(l) = lo {
                    let lw = l.w;
                    let y = DESC * size + 0.06 * size + l.asc;
                    b.place(l, (w - lw) / 2.0, y);
                }
                b
            }
        }
    }
}

fn display_name(n: &str) -> String {
    match n {
        "asin" => "arcsin".into(),
        "acos" => "arccos".into(),
        "atan" => "arctan".into(),
        "log" => "ln".into(),
        n => n.to_string(),
    }
}
