//! Structured 2D math editor: model (cursor + edit operations) and the egui widget.

use crate::layout::{Boxed, Mark, Path};
use maximus_math::{closer_of, expr_to_tree, from_latex, parse_maxima, row_to_latex, BigOp, Item, Row};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Cursor {
    pub path: Path,
    pub pos: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Template {
    Frac,
    Sup,
    Sub,
    Sqrt,
    NthRoot,
    Paren,
    Abs,
    Integral,
    DefIntegral,
    Sum,
    Product,
    Limit,
    Derivative,
    Matrix(usize, usize),
}

#[derive(Clone, Debug, Default)]
pub struct EditorState {
    pub row: Row,
    pub cur: Cursor,
    pub undo: Vec<(Row, Cursor)>,
}

fn item_slot(item: &mut Item, slot: usize) -> Option<&mut Row> {
    match item {
        Item::Frac(a, b) => match slot {
            0 => Some(a),
            1 => Some(b),
            _ => None,
        },
        Item::Sup(a) | Item::Sub(a) => (slot == 0).then_some(a),
        Item::Sqrt(idx, body) => match slot {
            0 => Some(body),
            1 => idx.as_mut(),
            _ => None,
        },
        Item::Big(_, lo, hi) => match slot {
            0 => Some(lo),
            1 => Some(hi),
            _ => None,
        },
        Item::Group(_, _, b) => (slot == 0).then_some(b),
        Item::Matrix(rows) => {
            let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
            rows.get_mut(slot / ncols).and_then(|r| r.get_mut(slot % ncols))
        }
        _ => None,
    }
}

fn item_slot_count(item: &Item) -> usize {
    match item {
        Item::Frac(..) | Item::Big(..) => 2,
        Item::Sqrt(idx, _) => 1 + idx.is_some() as usize,
        Item::Sup(_) | Item::Sub(_) | Item::Group(..) => 1,
        Item::Matrix(rows) => rows.iter().map(|r| r.len()).sum(),
        _ => 0,
    }
}

fn item_all_empty(item: &mut Item) -> bool {
    let n = item_slot_count(item);
    (0..n).all(|s| item_slot(item, s).map_or(true, |r| r.is_empty()))
}

pub fn row_at<'a>(row: &'a mut Row, path: &[(usize, usize)]) -> Option<&'a mut Row> {
    let mut cur = row;
    for (i, s) in path {
        let item = cur.get_mut(*i)?;
        cur = item_slot(item, *s)?;
    }
    Some(cur)
}

fn is_operand_item(it: &Item) -> bool {
    match it {
        Item::Char(c) => c.is_alphanumeric() || *c == '.' || *c == '∞' || *c == 'ⅇ' || *c == 'ⅈ',
        Item::Func(_) | Item::Group(..) | Item::Sup(_) | Item::Sub(_) | Item::Frac(..) | Item::Sqrt(..) | Item::Matrix(_) | Item::Text(_) => true,
        _ => false,
    }
}

impl EditorState {
    pub fn from_latex(src: &str) -> Self {
        let row = from_latex(src);
        let mut s = EditorState { row, cur: Cursor::default(), undo: Vec::new() };
        s.cur.pos = s.row.len();
        s
    }

    pub fn latex(&self) -> String {
        row_to_latex(&self.row)
    }

    fn cur_row(&mut self) -> &mut Row {
        let path = self.cur.path.clone();
        if row_at(&mut self.row, &path).is_none() {
            self.cur = Cursor { path: vec![], pos: self.row.len() };
        }
        let path = self.cur.path.clone();
        row_at(&mut self.row, &path).unwrap()
    }

    fn clamp(&mut self) {
        let n = self.cur_row().len();
        if self.cur.pos > n {
            self.cur.pos = n;
        }
    }

    pub fn clear(&mut self) {
        self.row.clear();
        self.cur = Cursor::default();
    }

    pub fn is_empty(&self) -> bool {
        self.row.is_empty()
    }

    fn enter(&mut self, idx: usize, slot: usize, pos: usize) {
        self.cur.path.push((idx, slot));
        self.cur.pos = pos;
    }

    /// Inserts a single item at the cursor and moves past it.
    fn put(&mut self, item: Item) {
        let pos = self.cur.pos;
        let r = self.cur_row();
        r.insert(pos.min(r.len()), item);
        self.cur.pos = pos + 1;
    }

    /// Index range in the current row of the operand just before the cursor.
    fn operand_start(&mut self) -> usize {
        let pos = self.cur.pos;
        let r = self.cur_row();
        let mut j = pos.min(r.len());
        while j > 0 && is_operand_item(&r[j - 1]) {
            j -= 1;
        }
        j
    }

    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.type_char(c);
        }
    }

    pub fn type_char(&mut self, c: char) {
        if c.is_whitespace() || c.is_control() {
            return;
        }
        self.clamp();
        match c {
            '/' => {
                let start = self.operand_start();
                let pos = self.cur.pos;
                let r = self.cur_row();
                let num: Row = r.drain(start..pos).collect();
                r.insert(start, Item::Frac(num, vec![]));
                self.cur.pos = start;
                self.enter(start, 1, 0);
            }
            '^' if self.cur.pos == 0 && self.in_empty_sup() => {
                self.put(Item::Char(maximus_math::NC_MARK));
            }
            '^' => {
                let pos = self.cur.pos;
                self.cur_row().insert(pos, Item::Sup(vec![]));
                self.cur.pos = pos;
                self.enter(pos, 0, 0);
            }
            '_' => {
                let pos = self.cur.pos;
                self.cur_row().insert(pos, Item::Sub(vec![]));
                self.cur.pos = pos;
                self.enter(pos, 0, 0);
            }
            '(' | '[' | '{' => {
                let pos = self.cur.pos;
                let r = self.cur_row();
                // `sqrt(` and `abs(` turn into the textbook forms.
                let tail = |name: &str| -> bool {
                    let n = name.chars().count();
                    pos >= n
                        && name.chars().enumerate().all(|(k, ch)| matches!(r.get(pos - n + k), Some(Item::Char(x)) if *x == ch))
                        && !(pos > n && matches!(r.get(pos - n - 1), Some(Item::Char(x)) if x.is_ascii_alphabetic()))
                };
                if c == '(' && tail("sqrt") {
                    r.drain(pos - 4..pos);
                    r.insert(pos - 4, Item::Sqrt(None, vec![]));
                    self.cur.pos = pos - 4;
                    self.enter(pos - 4, 0, 0);
                } else if c == '(' && tail("abs") {
                    r.drain(pos - 3..pos);
                    r.insert(pos - 3, Item::Group('|', '|', vec![]));
                    self.cur.pos = pos - 3;
                    self.enter(pos - 3, 0, 0);
                } else {
                    r.insert(pos, Item::Group(c, closer_of(c), vec![]));
                    self.cur.pos = pos;
                    self.enter(pos, 0, 0);
                }
            }
            ')' | ']' | '}' => {
                if !self.exit_group(Some(c)) {
                    self.put(Item::Char(c));
                }
            }
            '|' => {
                let in_bar = matches!(self.cur.path.last(), Some(_)) && self.enclosing_group_open() == Some('|');
                if in_bar {
                    self.exit_group(Some('|'));
                } else {
                    let pos = self.cur.pos;
                    self.cur_row().insert(pos, Item::Group('|', '|', vec![]));
                    self.cur.pos = pos;
                    self.enter(pos, 0, 0);
                }
            }
            '=' => {
                let pos = self.cur.pos;
                let r = self.cur_row();
                let merged = match pos.checked_sub(1).and_then(|p| r.get(p)) {
                    Some(Item::Char('<')) => Some('≤'),
                    Some(Item::Char('>')) => Some('≥'),
                    Some(Item::Char('!')) => Some('≠'),
                    _ => None,
                };
                match merged {
                    Some(m) => r[pos - 1] = Item::Char(m),
                    None => self.put(Item::Char('=')),
                }
            }
            '*' => self.put(Item::Char('·')),
            '-' => self.put(Item::Char('−')),
            c => self.put(Item::Char(c)),
        }
    }

    fn in_empty_sup(&mut self) -> bool {
        let Some((i, _)) = self.cur.path.last().copied() else { return false };
        let mut parent = self.cur.path.clone();
        parent.pop();
        let in_sup = row_at(&mut self.row, &parent).map_or(false, |r| matches!(r.get(i), Some(Item::Sup(_))));
        in_sup && self.cur_row().is_empty()
    }

    fn enclosing_group_open(&mut self) -> Option<char> {
        let (i, _) = *self.cur.path.last()?;
        let mut path = self.cur.path.clone();
        path.pop();
        match row_at(&mut self.row, &path)?.get(i) {
            Some(Item::Group(o, ..)) => Some(*o),
            _ => None,
        }
    }

    /// Moves the cursor just after the nearest enclosing bracket matching `closer`.
    /// `)` also closes `sqrt(` and `abs(` forms.
    fn exit_group(&mut self, closer: Option<char>) -> bool {
        let mut path = self.cur.path.clone();
        while let Some((i, slot)) = path.pop() {
            let item = row_at(&mut self.row, &path).and_then(|r| r.get(i)).cloned();
            let ok = match (&item, closer) {
                (Some(Item::Group(o, c, _)), Some('|')) => *o == '|' && *c == '|',
                (Some(Item::Group(o, ..)), Some(')')) => *o == '(' || *o == '|',
                (Some(Item::Sqrt(..)), Some(')')) => slot == 0,
                (Some(Item::Group(_, c, _)), Some(cl)) => *c == cl,
                (Some(Item::Group(..)), None) => true,
                _ => false,
            };
            if ok {
                self.cur = Cursor { path, pos: i + 1 };
                return true;
            }
        }
        false
    }

    /// Replaces the group at `idx` in the row at `path` by its content; returns the content length.
    fn unwrap_group(&mut self, path: &[(usize, usize)], idx: usize) -> usize {
        let Some(row) = row_at(&mut self.row, path) else { return 0 };
        let Item::Group(_, _, body) = row.remove(idx) else { return 0 };
        let n = body.len();
        row.splice(idx..idx, body);
        n
    }

    pub fn backspace(&mut self) {
        self.clamp();
        let pos = self.cur.pos;
        if pos == 0 {
            // At the start of a slot: dissolve a bracket pair, remove an empty structure, or step out.
            if let Some((i, _)) = self.cur.path.last().copied() {
                let mut path = self.cur.path.clone();
                path.pop();
                let is_group = row_at(&mut self.row, &path).map_or(false, |r| matches!(r.get(i), Some(Item::Group(..))));
                if is_group {
                    self.unwrap_group(&path, i);
                    self.cur = Cursor { path, pos: i };
                    return;
                }
                let empty = row_at(&mut self.row, &path).and_then(|r| r.get_mut(i)).map_or(false, item_all_empty);
                if empty {
                    row_at(&mut self.row, &path).unwrap().remove(i);
                }
                self.cur = Cursor { path, pos: i };
            }
            return;
        }
        let path = self.cur.path.clone();
        let r = self.cur_row();
        match &mut r[pos - 1] {
            Item::Char(_) | Item::Func(_) | Item::Text(_) => {
                r.remove(pos - 1);
                self.cur.pos = pos - 1;
            }
            Item::Group(..) => {
                let n = self.unwrap_group(&path, pos - 1);
                self.cur.pos = pos - 1 + n;
            }
            item => {
                if item_all_empty(item) {
                    r.remove(pos - 1);
                    self.cur.pos = pos - 1;
                } else {
                    // Step into the last non-empty slot.
                    let n = item_slot_count(item);
                    let slot = (0..n).rev().find(|s| item_slot(item, *s).map_or(false, |r| !r.is_empty())).unwrap_or(n - 1);
                    let len = item_slot(item, slot).map_or(0, |r| r.len());
                    self.enter(pos - 1, slot, len);
                }
            }
        }
    }

    pub fn delete_forward(&mut self) {
        self.clamp();
        let pos = self.cur.pos;
        let path = self.cur.path.clone();
        let r = self.cur_row();
        if pos >= r.len() {
            // At the end of a bracket pair: dissolve it.
            if let Some((i, _)) = path.last().copied() {
                let mut parent = path.clone();
                parent.pop();
                let is_group = row_at(&mut self.row, &parent).map_or(false, |r| matches!(r.get(i), Some(Item::Group(..))));
                if is_group {
                    let n = self.unwrap_group(&parent, i);
                    self.cur = Cursor { path: parent, pos: i + n };
                }
            }
            return;
        }
        match &mut r[pos] {
            Item::Char(_) | Item::Func(_) | Item::Text(_) => {
                r.remove(pos);
            }
            Item::Group(..) => {
                self.unwrap_group(&path, pos);
            }
            item => {
                if item_all_empty(item) {
                    r.remove(pos);
                } else {
                    self.enter(pos, 0, 0);
                }
            }
        }
    }

    pub fn insert_row(&mut self, items: Row) {
        self.clamp();
        let n = items.len();
        let pos = self.cur.pos;
        let r = self.cur_row();
        for (k, it) in items.into_iter().enumerate() {
            r.insert(pos + k, it);
        }
        self.cur.pos = pos + n;
    }

    /// Replaces the whole content (used when pasting into an empty editor).
    pub fn set_row(&mut self, row: Row) {
        self.row = row;
        self.cur = Cursor { path: vec![], pos: self.row.len() };
    }

    /// Parses pasted/inserted text (LaTeX or Maxima syntax) into items.
    pub fn parse_text(text: &str) -> Row {
        let t = text.trim();
        let looks_latex = t.contains('\\') || t.starts_with('$') || t.contains("^{") || t.contains("_{") || t.contains("\\(");
        if !looks_latex {
            if let Ok(e) = parse_maxima(t) {
                return expr_to_tree(&e);
            }
        }
        from_latex(t)
    }

    pub fn paste(&mut self, text: &str) {
        let row = Self::parse_text(text);
        self.insert_row(row);
    }

    pub fn insert_symbol(&mut self, c: char) {
        self.put(Item::Char(c));
    }

    pub fn insert_template(&mut self, t: Template) {
        self.clamp();
        let pos = self.cur.pos;
        match t {
            Template::Frac => {
                let start = self.operand_start();
                let r = self.cur_row();
                let num: Row = r.drain(start..pos).collect();
                let has_num = !num.is_empty();
                r.insert(start, Item::Frac(num, vec![]));
                self.cur.pos = start;
                self.enter(start, if has_num { 1 } else { 0 }, 0);
            }
            Template::Sup => self.type_char('^'),
            Template::Sub => self.type_char('_'),
            Template::Sqrt => {
                self.cur_row().insert(pos, Item::Sqrt(None, vec![]));
                self.enter(pos, 0, 0);
            }
            Template::NthRoot => {
                self.cur_row().insert(pos, Item::Sqrt(Some(vec![]), vec![]));
                self.enter(pos, 1, 0);
            }
            Template::Paren => self.type_char('('),
            Template::Abs => self.type_char('|'),
            Template::Integral => {
                self.insert_row(vec![Item::Big(BigOp::IntNoLimits, vec![], vec![]), Item::Char('d'), Item::Char('x')]);
                self.cur.pos = pos + 1;
            }
            Template::DefIntegral => {
                self.cur_row().splice(pos..pos, [Item::Big(BigOp::Int, vec![], vec![]), Item::Char('d'), Item::Char('x')]);
                self.enter(pos, 0, 0);
            }
            Template::Sum | Template::Product => {
                let op = if t == Template::Sum { BigOp::Sum } else { BigOp::Prod };
                let lo = vec![Item::Char('n'), Item::Char('=')];
                self.cur_row().insert(pos, Item::Big(op, lo, vec![]));
                self.enter(pos, 0, 2);
            }
            Template::Limit => {
                let lo = vec![Item::Char('x'), Item::Char('→')];
                self.cur_row().insert(pos, Item::Big(BigOp::Lim, lo, vec![]));
                self.enter(pos, 0, 2);
            }
            Template::Derivative => {
                let num = vec![Item::Char('d')];
                let den = vec![Item::Char('d'), Item::Char('x')];
                self.cur_row().splice(pos..pos, [Item::Frac(num, den), Item::Group('(', ')', vec![])]);
                self.enter(pos + 1, 0, 0);
            }
            Template::Matrix(rows, cols) => {
                let m = vec![vec![vec![]; cols.max(1)]; rows.max(1)];
                self.cur_row().insert(pos, Item::Matrix(m));
                self.enter(pos, 0, 0);
            }
        }
    }

    // ------------------------------------------------------------ navigation

    fn mark_index(marks: &[Mark], cur: &Cursor) -> Option<usize> {
        marks.iter().position(|m| m.path == cur.path && m.pos == cur.pos)
    }

    fn set_from_mark(&mut self, m: &Mark) {
        self.cur = Cursor { path: m.path.clone(), pos: m.pos };
    }

    fn dedup(marks: &[Mark]) -> Vec<&Mark> {
        let mut out: Vec<&Mark> = Vec::new();
        for m in marks {
            if out.last().map_or(true, |l| l.path != m.path || l.pos != m.pos) {
                out.push(m);
            }
        }
        out
    }

    pub fn move_horizontal(&mut self, marks: &[Mark], dir: i32) {
        let marks = Self::dedup(marks);
        let Some(i) = marks.iter().position(|m| m.path == self.cur.path && m.pos == self.cur.pos) else {
            return;
        };
        let j = i as i32 + dir;
        if j >= 0 && (j as usize) < marks.len() {
            let m = marks[j as usize].clone();
            self.set_from_mark(&m);
        }
    }

    pub fn move_home_end(&mut self, marks: &[Mark], end: bool) {
        let top: Vec<&Mark> = Self::dedup(marks).into_iter().filter(|m| m.path.is_empty()).collect();
        let m = if end { top.last() } else { top.first() };
        if let Some(m) = m {
            let m = (*m).clone();
            self.set_from_mark(&m);
        }
    }

    pub fn move_vertical(&mut self, marks: &[Mark], dir: i32) {
        let Some(i) = Self::mark_index(marks, &self.cur) else { return };
        let here = &marks[i];
        let cy = (here.y0 + here.y1) / 2.0;
        let mut best: Option<(f32, &Mark)> = None;
        for m in marks {
            let my = (m.y0 + m.y1) / 2.0;
            let dy = (my - cy) * dir as f32;
            if dy < 3.0 {
                continue;
            }
            let score = (m.x - here.x).abs() * 1.0 + dy * 0.6;
            if best.map_or(true, |(s, _)| score < s) {
                best = Some((score, m));
            }
        }
        if let Some((_, m)) = best {
            let m = m.clone();
            self.set_from_mark(&m);
        }
    }

    /// Puts the cursor at the mark closest to a point (layout coordinates relative to the baseline origin).
    pub fn click(&mut self, marks: &[Mark], x: f32, y: f32) {
        let mut best: Option<(f32, &Mark)> = None;
        for m in marks {
            let my = (m.y0 + m.y1) / 2.0;
            let d = (m.x - x).abs() + (my - y).abs() * 1.5;
            if best.map_or(true, |(s, _)| d < s) {
                best = Some((d, m));
            }
        }
        if let Some((_, m)) = best {
            let m = m.clone();
            self.set_from_mark(&m);
        }
    }

    /// The Maxima function the cursor is on or inside the arguments of, for contextual help.
    pub fn help_word(&mut self) -> Option<String> {
        use maximus_math::words::Word;
        fn letters_before(r: &[Item], end: usize) -> Option<String> {
            // The last function word in the run of letters/function items ending at `end`.
            let mut start = end;
            while start > 0 && matches!(&r[start - 1], Item::Char(c) if c.is_ascii_alphabetic() || *c == '_') {
                start -= 1;
            }
            if start < end {
                let run: Vec<char> = r[start..end].iter().filter_map(|i| if let Item::Char(c) = i { Some(*c) } else { None }).collect();
                let all = run.iter().collect::<String>();
                let paren = matches!(r.get(end), Some(Item::Group('(', _, _)));
                let words = maximus_math::words::split_letters_ctx(&run, paren);
                if let Some((_, _, Word::Func(f))) = words.last() {
                    return Some(f.clone());
                }
                // A whole identifier (e.g. `ratsimp` is a function word, `nusum` is not).
                return Some(all);
            }
            if end > 0 {
                if let Item::Func(n) = &r[end - 1] {
                    return Some(n.clone());
                }
            }
            None
        }
        let path = self.cur.path.clone();
        let pos = self.cur.pos;
        let here = row_at(&mut self.row, &path)?.clone();
        // On or right after a word.
        let mut e = pos.min(here.len());
        while e < here.len() && matches!(&here[e], Item::Char(c) if c.is_ascii_alphabetic() || *c == '_') {
            e += 1;
        }
        if let Some(w) = letters_before(&here, e) {
            if maximus_math::words::is_known_func(&w) || w.len() > 2 {
                return Some(w);
            }
        }
        let mut p = path;
        while let Some((i, _)) = p.pop() {
            let parent = row_at(&mut self.row, &p)?.clone();
            match parent.get(i)? {
                Item::Big(op, ..) => {
                    return Some(
                        match op {
                            BigOp::Int | BigOp::IntNoLimits => "integrate",
                            BigOp::Sum => "sum",
                            BigOp::Prod => "product",
                            BigOp::Lim => "limit",
                        }
                        .to_string(),
                    )
                }
                Item::Sqrt(..) => return Some("sqrt".into()),
                Item::Group('|', ..) => return Some("abs".into()),
                Item::Group('(', ..) => {
                    if let Some(w) = letters_before(&parent, i) {
                        return Some(w);
                    }
                }
                Item::Sup(_) | Item::Sub(_) | Item::Group(..) => {
                    if let Some(w) = letters_before(&parent, i) {
                        return Some(w);
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub fn cursor_mark<'a>(&self, b: &'a Boxed) -> Option<&'a Mark> {
        Self::mark_index(&b.marks, &self.cur).map(|i| &b.marks[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> EditorState {
        let mut e = EditorState::default();
        e.type_text(s);
        e
    }

    #[test]
    fn typing_fraction_and_exit() {
        let mut e = typed("1/2");
        assert_eq!(e.row.len(), 1);
        assert!(matches!(&e.row[0], Item::Frac(n, d) if n.len() == 1 && d.len() == 1));
        assert_eq!(e.cur.path, vec![(0, 1)]);
        e.cur = Cursor { path: vec![], pos: 1 };
        e.type_text("+x");
        assert_eq!(e.latex(), "\\frac{1}{2}+x");
    }

    #[test]
    fn parens_and_exit_with_close() {
        let mut e = typed("sin(x)");
        assert_eq!(e.row.len(), 4);
        assert!(e.cur.path.is_empty());
        e.type_text("^2");
        assert!(matches!(e.row.last(), Some(Item::Sup(_))));
    }

    #[test]
    fn sqrt_shortcut() {
        let e = typed("sqrt(x+1)");
        assert!(matches!(&e.row[0], Item::Sqrt(None, b) if b.len() == 3));
    }

    #[test]
    fn spec_example_typed_in_editor() {
        let mut e = EditorState::default();
        e.insert_template(Template::DefIntegral);
        e.type_text("0");
        // move to upper limit then the integrand using marks-free manual moves
        e.cur = Cursor { path: vec![(0, 1)], pos: 0 };
        e.type_text("inf");
        e.cur = Cursor { path: vec![], pos: 1 };
        e.type_text("xsin^3");
        // Leave the superscript (the → key in the UI), then type the argument.
        e.cur = Cursor { path: vec![], pos: e.row.len() - 2 };
        e.type_text("(x)");
        let latex = e.latex();
        let m = maximus_math::latex_to_maxima(&latex, &Default::default()).unwrap_or_else(|e| format!("ERR {e} from {latex}"));
        assert_eq!(m, "integrate(x*sin(x)^3,x,0,inf);", "{latex}");
    }

    #[test]
    fn backspace_removes_empty_structures() {
        let mut e = typed("1/");
        e.backspace();
        // The denominator is empty but the numerator is not: cursor steps out, structure stays.
        assert_eq!(e.row.len(), 1);
        let mut e = typed("^");
        e.backspace();
        assert!(e.row.is_empty());
    }

    #[test]
    fn paste_maxima_and_latex() {
        let mut e = EditorState::default();
        e.paste("integrate(x*sin(x)^3,x,0,inf)");
        assert!(matches!(e.row[0], Item::Big(BigOp::Int, ..)));
        let mut e = EditorState::default();
        e.paste("\\frac{a}{b}");
        assert!(matches!(e.row[0], Item::Frac(..)));
    }

    #[test]
    fn deleting_brackets_with_content() {
        let mut e = typed("(ab)+c");
        // Cursor after `c`; move to just after the group and backspace: the pair dissolves.
        e.cur = Cursor { path: vec![], pos: 1 };
        e.backspace();
        assert_eq!(e.latex(), "ab+c");
        let mut e = typed("(ab");
        e.cur = Cursor { path: vec![(0, 0)], pos: 0 };
        e.backspace();
        assert_eq!(e.latex(), "ab");
        let mut e = typed("(ab");
        e.delete_forward();
        assert_eq!(e.latex(), "(ab)".replace("(", "").replace(")", ""));
    }

    #[test]
    fn double_caret_marks_noncommutative_power() {
        let mut e = typed("A^^");
        e.type_text("-1");
        let m = maximus_math::tree_to_maxima(&e.row, &Default::default()).unwrap();
        assert_eq!(m, "A^^(-1);");
    }

    #[test]
    fn le_ge_merge() {
        let e = typed("a<=b");
        assert_eq!(e.row[1], Item::Char('≤'));
    }
}

#[cfg(test)]
mod help_tests {
    use super::*;

    #[test]
    fn help_word_contextual() {
        let mut e = EditorState::default();
        e.type_text("integrate(x");
        assert_eq!(e.help_word().as_deref(), Some("integrate"));
        let mut e = EditorState::default();
        e.type_text("solve");
        assert_eq!(e.help_word().as_deref(), Some("solve"));
        let mut e = EditorState::default();
        e.insert_template(Template::Sum);
        assert_eq!(e.help_word().as_deref(), Some("sum"));
    }
}
