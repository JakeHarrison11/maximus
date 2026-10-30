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
    /// Other end of the selection; the selection runs between this and `cur`.
    pub anchor: Option<Cursor>,
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
    /// An editor state holding the given tree (used to make results selectable).
    pub fn from_row(row: Row) -> Self {
        EditorState { row, ..Default::default() }
    }

    pub fn from_latex(src: &str) -> Self {
        let row = from_latex(src);
        let mut s = EditorState { row, cur: Cursor::default(), anchor: None, undo: Vec::new() };
        s.cur.pos = s.row.len();
        s
    }

    /// Like [`Self::from_latex`] for converted input: a lone trailing `;` is dropped.
    pub fn from_latex_converted(src: &str) -> Self {
        let mut s = Self::from_latex(src);
        s.row = maximus_math::drop_single_semicolon(std::mem::take(&mut s.row));
        s.cur = Cursor { path: vec![], pos: s.row.len() };
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
        self.anchor = None;
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
        if self.has_selection() {
            let wrap = match c {
                '/' => Some(Wrap::Frac),
                '(' | '[' | '{' => Some(Wrap::Group(c)),
                '|' => Some(Wrap::Group('|')),
                _ => None,
            };
            match wrap {
                Some(w) => {
                    self.wrap_selection(w);
                    return;
                }
                None => {
                    self.delete_selection();
                }
            }
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

    /// Dissolves the bracket pair at `idx` in the row at `path`.
    /// `keep` says which bracket survives as a plain character: `None` removes both (and is used for `|`),
    /// `Some(true)` keeps the opening one, `Some(false)` keeps the closing one.
    /// Returns the number of items that now stand where the group's content was, and the content offset.
    fn dissolve_group(&mut self, path: &[(usize, usize)], idx: usize, keep: Option<bool>) -> (usize, usize) {
        let Some(row) = row_at(&mut self.row, path) else { return (0, 0) };
        let Item::Group(o, c, body) = row.remove(idx) else { return (0, 0) };
        let n = body.len();
        let keep = if o == '|' { None } else { keep };
        let mut items: Vec<Item> = Vec::new();
        let mut offset = 0;
        if keep == Some(true) {
            items.push(Item::Char(o));
            offset = 1;
        }
        items.extend(body);
        if keep == Some(false) {
            items.push(Item::Char(c));
        }
        row.splice(idx..idx, items);
        (n, offset)
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.clamp();
        let pos = self.cur.pos;
        if pos == 0 {
            let Some((i, slot)) = self.cur.path.last().copied() else { return };
            let mut path = self.cur.path.clone();
            path.pop();
            let kind = row_at(&mut self.row, &path).and_then(|r| r.get(i)).cloned();
            match kind {
                // Deleting the opening bracket leaves the closing one behind.
                Some(Item::Group(..)) => {
                    self.dissolve_group(&path, i, Some(false));
                    self.cur = Cursor { path, pos: i };
                }
                Some(mut item) => {
                    let empty = item_all_empty(&mut item);
                    let multi = matches!(item, Item::Frac(..) | Item::Big(..) | Item::Matrix(_));
                    if empty {
                        row_at(&mut self.row, &path).unwrap().remove(i);
                        self.cur = Cursor { path, pos: i };
                    } else if multi && slot > 0 {
                        // Backspacing out of a denominator (or later cell) lands at the end of the slot before it.
                        let mut p = path;
                        p.push((i, slot - 1));
                        let len = row_at(&mut self.row, &p).map_or(0, |r| r.len());
                        self.cur = Cursor { path: p, pos: len };
                    } else {
                        self.cur = Cursor { path, pos: i };
                    }
                }
                None => {}
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
                // Deleting the closing bracket leaves the opening one behind.
                let (n, off) = self.dissolve_group(&path, pos - 1, Some(true));
                self.cur.pos = pos - 1 + off + n;
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
        if self.delete_selection() {
            return;
        }
        self.clamp();
        let pos = self.cur.pos;
        let path = self.cur.path.clone();
        let r = self.cur_row();
        if pos >= r.len() {
            // At the end of a bracket pair: delete the closing bracket.
            if let Some((i, _)) = path.last().copied() {
                let mut parent = path.clone();
                parent.pop();
                let is_group = row_at(&mut self.row, &parent).map_or(false, |r| matches!(r.get(i), Some(Item::Group(..))));
                if is_group {
                    let (n, off) = self.dissolve_group(&parent, i, Some(true));
                    self.cur = Cursor { path: parent, pos: i + off + n };
                }
            }
            return;
        }
        match &mut r[pos] {
            Item::Char(_) | Item::Func(_) | Item::Text(_) => {
                r.remove(pos);
            }
            Item::Group(..) => {
                // Deleting the opening bracket leaves the closing one behind.
                self.dissolve_group(&path, pos, Some(false));
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
        self.delete_selection();
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
        self.anchor = None;
        self.row = row;
        self.cur = Cursor { path: vec![], pos: self.row.len() };
    }

    /// Parses pasted/inserted text (LaTeX or Maxima syntax) into items.
    pub fn parse_text(text: &str) -> Row {
        let t = text.trim();
        let looks_latex = t.contains('\\') || t.starts_with('$') || t.contains("^{") || t.contains("_{") || t.contains("\\(");
        if !looks_latex {
            if let Ok(e) = parse_maxima(t) {
                return maximus_math::drop_single_semicolon(expr_to_tree(&e));
            }
        }
        maximus_math::drop_single_semicolon(from_latex(t))
    }

    pub fn paste(&mut self, text: &str) {
        let row = Self::parse_text(text);
        self.insert_row(row);
    }

    pub fn insert_symbol(&mut self, c: char) {
        self.delete_selection();
        self.put(Item::Char(c));
    }

    pub fn insert_template(&mut self, t: Template) {
        if self.has_selection() {
            let wrap = match t {
                Template::Frac => Some(Wrap::Frac),
                Template::Sqrt => Some(Wrap::Sqrt),
                Template::NthRoot => Some(Wrap::NthRoot),
                Template::Paren => Some(Wrap::Group('(')),
                Template::Abs => Some(Wrap::Group('|')),
                _ => None,
            };
            match wrap {
                Some(w) => {
                    self.wrap_selection(w);
                    return;
                }
                None => {
                    self.delete_selection();
                }
            }
        }
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

    // ------------------------------------------------------------ matrices

    /// Path of the innermost matrix around the cursor and the (row, col) of the cell holding it.
    fn matrix_context(&mut self) -> Option<(Vec<(usize, usize)>, usize, usize, usize)> {
        for depth in (0..self.cur.path.len()).rev() {
            let (i, slot) = self.cur.path[depth];
            let parent: Vec<(usize, usize)> = self.cur.path[..depth].to_vec();
            if let Some(Item::Matrix(m)) = row_at(&mut self.row, &parent).and_then(|r| r.get(i)) {
                let ncols = m.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                return Some((parent, i, slot / ncols, slot % ncols));
            }
        }
        None
    }

    pub fn in_matrix(&mut self) -> bool {
        self.matrix_context().is_some()
    }

    fn with_matrix<R>(&mut self, f: impl FnOnce(&mut Vec<Vec<Row>>, usize, usize) -> R) -> Option<R> {
        let (parent, i, r, c) = self.matrix_context()?;
        let Item::Matrix(m) = row_at(&mut self.row, &parent)?.get_mut(i)? else { return None };
        Some(f(m, r, c))
    }

    /// Inserts a row after (`after`) or before the current one.
    pub fn matrix_add_row(&mut self, after: bool) {
        self.with_matrix(|m, r, _| {
            let ncols = m.iter().map(|x| x.len()).max().unwrap_or(1).max(1);
            m.insert(if after { r + 1 } else { r }, vec![vec![]; ncols]);
        });
    }

    pub fn matrix_add_col(&mut self, after: bool) {
        self.with_matrix(|m, _, c| {
            for row in m.iter_mut() {
                row.insert(if after { (c + 1).min(row.len()) } else { c.min(row.len()) }, vec![]);
            }
        });
    }

    pub fn matrix_remove_row(&mut self) {
        if let Some((parent, i, r, _)) = self.matrix_context() {
            let removed = self.with_matrix(|m, r, _| if m.len() > 1 { m.remove(r); true } else { false });
            if removed == Some(true) {
                let mut p = parent;
                p.push((i, 0));
                self.cur = Cursor { path: p, pos: 0 };
                let _ = r;
            }
        }
    }

    pub fn matrix_remove_col(&mut self) {
        if let Some((parent, i, _, _)) = self.matrix_context() {
            let removed = self.with_matrix(|m, _, c| {
                if m.iter().map(|x| x.len()).max().unwrap_or(0) > 1 {
                    for row in m.iter_mut() {
                        if c < row.len() {
                            row.remove(c);
                        }
                    }
                    true
                } else {
                    false
                }
            });
            if removed == Some(true) {
                let mut p = parent;
                p.push((i, 0));
                self.cur = Cursor { path: p, pos: 0 };
            }
        }
    }

    /// Tab: next matrix cell (row-major); at the very end a new row is appended. Shift+Tab goes back.
    pub fn matrix_tab(&mut self, back: bool) {
        let Some((parent, i, r, c)) = self.matrix_context() else { return };
        let depth = parent.len();
        let (rows, cols) = match row_at(&mut self.row, &parent).and_then(|x| x.get(i)) {
            Some(Item::Matrix(m)) => (m.len(), m.iter().map(|x| x.len()).max().unwrap_or(1).max(1)),
            _ => return,
        };
        let mut idx = r * cols + c;
        let total = rows * cols;
        if back {
            if idx == 0 {
                return;
            }
            idx -= 1;
        } else {
            idx += 1;
            if idx >= total {
                self.matrix_add_row(true);
            }
        }
        let mut path = parent;
        path.push((i, idx));
        let _ = depth;
        let len = row_at(&mut self.row, &path).map_or(0, |x| x.len());
        self.cur = Cursor { path, pos: if back { len } else { 0 } };
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
        // A fraction's outer baseline lies between its slots; don't land there
        // when moving directly between the numerator and denominator.
        for depth in (0..self.cur.path.len()).rev() {
            let (idx, slot) = self.cur.path[depth];
            let mut target = self.cur.path[..depth].to_vec();
            if matches!(row_at(&mut self.row, &target).and_then(|row| row.get(idx)), Some(Item::Frac(..)))
                && ((slot == 0 && dir > 0) || (slot == 1 && dir < 0))
            {
                target.push((idx, 1 - slot));
                if let Some(mark) = marks.iter().filter(|m| m.path == target).min_by(|a, b| {
                    (a.x - here.x).abs().total_cmp(&(b.x - here.x).abs())
                }) {
                    self.set_from_mark(mark);
                    return;
                }
            }
        }
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


/// A selected run of items `start..end` in the row at `path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub path: Path,
    pub start: usize,
    pub end: usize,
}

/// One piece of a selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Items(Selection),
    /// The opening bracket of the pair at `idx` in the row `parent`.
    Open { parent: Path, idx: usize },
    Close { parent: Path, idx: usize },
}

/// Which structure to wrap a selection in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    Frac,
    Sqrt,
    NthRoot,
    Group(char),
}

fn cmp_cursor(a: &Cursor, b: &Cursor) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let n = a.path.len().min(b.path.len());
    for j in 0..n {
        if a.path[j] != b.path[j] {
            return a.path[j].cmp(&b.path[j]);
        }
    }
    match a.path.len().cmp(&b.path.len()) {
        Equal => a.pos.cmp(&b.pos),
        // `a` sits in the row that holds the item `b` is inside of.
        Less => {
            if a.pos <= b.path[n].0 {
                Less
            } else {
                Greater
            }
        }
        Greater => {
            if b.pos <= a.path[n].0 {
                Greater
            } else {
                Less
            }
        }
    }
}

impl EditorState {
    fn endpoint_valid(&mut self, c: &Cursor) -> bool {
        match row_at(&mut self.row, &c.path) {
            Some(r) => c.pos <= r.len(),
            None => false,
        }
    }

    fn is_bracket_item(&mut self, path: &[(usize, usize)], idx: usize) -> bool {
        matches!(row_at(&mut self.row, path).and_then(|r| r.get(idx)), Some(Item::Group(o, _, _)) if *o != '|')
    }

    /// If the end `e` sits inside a structure other than a bracket pair, the whole outermost such
    /// structure counts as selected: returns the cursor just before it (`after == false`) or after it.
    fn snap_to_structure(&mut self, e: &Cursor, from_depth: usize, after: bool) -> Cursor {
        for t in from_depth..e.path.len() {
            let idx = e.path[t].0;
            if !self.is_bracket_item(&e.path[..t], idx) {
                return Cursor { path: e.path[..t].to_vec(), pos: idx + after as usize };
            }
        }
        e.clone()
    }

    /// The two ends of the selection in document order, with ends inside fractions, roots and the like
    /// moved to the edge of that structure. Brackets are see-through, so selecting over them selects
    /// only the terms passed, not everything inside.
    fn selection_bounds(&mut self) -> Option<(Cursor, Cursor)> {
        let a = self.anchor.clone()?;
        let c = self.cur.clone();
        if a == c || !self.endpoint_valid(&a) || !self.endpoint_valid(&c) {
            return None;
        }
        let (lo, hi) = if cmp_cursor(&a, &c) == std::cmp::Ordering::Less { (a, c) } else { (c, a) };
        let k = lo.path.iter().zip(hi.path.iter()).take_while(|(x, y)| x == y).count();
        let lo2 = self.snap_to_structure(&lo, k, false);
        let hi2 = self.snap_to_structure(&hi, k, true);
        (lo2 != hi2).then_some((lo2, hi2))
    }

    /// The selection in document order as runs of items and selected brackets. It has several parts
    /// when it starts or ends inside a bracket pair: the bracket itself is then selected too.
    pub fn selection_parts(&mut self) -> Vec<Part> {
        let Some((lo, hi)) = self.selection_bounds() else { return vec![] };
        let k = lo.path.iter().zip(hi.path.iter()).take_while(|(x, y)| x == y).count();
        let mut parts: Vec<Part> = Vec::new();
        let len_of = |s: &mut Self, p: &[(usize, usize)]| row_at(&mut s.row, p).map_or(0, |r| r.len());
        // Start side, innermost row first; each row is followed by the closing bracket of its pair.
        for d in (k + 1..=lo.path.len()).rev() {
            let path = lo.path[..d].to_vec();
            let start = if d == lo.path.len() { lo.pos } else { lo.path[d].0 + 1 };
            let end = len_of(self, &path);
            parts.push(Part::Items(Selection { path, start, end }));
            parts.push(Part::Close { parent: lo.path[..d - 1].to_vec(), idx: lo.path[d - 1].0 });
        }
        // The row both ends share.
        let start = if lo.path.len() > k { lo.path[k].0 + 1 } else { lo.pos };
        let end = if hi.path.len() > k { hi.path[k].0 } else { hi.pos };
        parts.push(Part::Items(Selection { path: lo.path[..k].to_vec(), start, end }));
        // End side, outermost first; each row is preceded by the opening bracket of its pair.
        for d in k + 1..=hi.path.len() {
            parts.push(Part::Open { parent: hi.path[..d - 1].to_vec(), idx: hi.path[d - 1].0 });
            let path = hi.path[..d].to_vec();
            let end = if d == hi.path.len() { hi.pos } else { hi.path[d].0 };
            parts.push(Part::Items(Selection { path, start: 0, end }));
        }
        parts.retain(|p| match p {
            Part::Items(s) => s.start < s.end,
            _ => true,
        });
        parts
    }

    /// The selected runs of items (without bracket characters).
    pub fn selection_segments(&mut self) -> Vec<Selection> {
        self.selection_parts()
            .into_iter()
            .filter_map(|p| match p {
                Part::Items(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    /// The selection when it is a single run of items with no bracket selected.
    pub fn selection(&mut self) -> Option<Selection> {
        let mut p = self.selection_parts();
        if p.len() == 1 {
            if let Part::Items(s) = p.remove(0) {
                return Some(s);
            }
        }
        None
    }

    pub fn has_selection(&mut self) -> bool {
        !self.selection_parts().is_empty()
    }

    /// The selected items in document order; selected brackets appear as plain characters.
    pub fn selected_row(&mut self) -> Option<Row> {
        let parts = self.selection_parts();
        if parts.is_empty() {
            return None;
        }
        let mut out: Row = Vec::new();
        for p in parts {
            match p {
                Part::Items(s) => {
                    if let Some(r) = row_at(&mut self.row, &s.path) {
                        out.extend(r[s.start..s.end].iter().cloned());
                    }
                }
                Part::Open { ref parent, idx } | Part::Close { ref parent, idx } => {
                    let is_open = matches!(p, Part::Open { .. });
                    if let Some(Item::Group(o, c, _)) = row_at(&mut self.row, parent).and_then(|r| r.get(idx)) {
                        out.push(Item::Char(if is_open { *o } else { *c }));
                    }
                }
            }
        }
        Some(out)
    }

    /// LaTeX of the selection, if there is one.
    pub fn selection_latex(&mut self) -> Option<String> {
        self.selected_row().map(|r| row_to_latex(&r))
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Removes the selected items. A selected bracket is removed on its own, which leaves the other
    /// bracket of the pair behind as a plain character. Returns whether anything was selected.
    pub fn delete_selection(&mut self) -> bool {
        let Some((lo, hi)) = self.selection_bounds() else {
            self.anchor = None;
            return false;
        };
        let parts = self.selection_parts();
        let k = lo.path.iter().zip(hi.path.iter()).take_while(|(x, y)| x == y).count();
        // Where the shared row's selected run starts; groups the selection ends in sit right there afterwards.
        let common_start = if lo.path.len() > k { lo.path[k].0 + 1 } else { lo.pos };
        // Items first, last run first so earlier positions stay valid.
        for p in parts.iter().rev() {
            if let Part::Items(s) = p {
                if let Some(r) = row_at(&mut self.row, &s.path) {
                    r.drain(s.start..s.end);
                }
            }
        }
        // Then the brackets: the end side (innermost first), then the start side (innermost first).
        let opens: Vec<&Part> = parts.iter().filter(|p| matches!(p, Part::Open { .. })).collect();
        for p in opens.iter().rev() {
            if let Part::Open { parent, .. } = p {
                // Deleting the run before the first end-side group moved it to `common_start`.
                let mut parent = parent.clone();
                if parent.len() > k {
                    parent[k].0 = common_start;
                }
                let idx = if parent.len() == k { common_start } else { 0 };
                self.dissolve_group(&parent, idx, Some(false));
            }
        }
        for p in parts.iter().filter(|p| matches!(p, Part::Close { .. })) {
            if let Part::Close { parent, idx } = p {
                self.dissolve_group(parent, *idx, Some(true));
            }
        }
        self.cur = if lo.path.len() > k {
            // The start side's brackets stay behind as plain `(` characters in front of what is left.
            let pos: usize = lo.path[k..].iter().map(|(i, _)| i + 1).sum::<usize>() + lo.pos;
            Cursor { path: lo.path[..k].to_vec(), pos }
        } else {
            lo
        };
        self.anchor = None;
        true
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Cursor { path: vec![], pos: 0 });
        self.cur = Cursor { path: vec![], pos: self.row.len() };
        if self.row.is_empty() {
            self.anchor = None;
        }
    }

    /// Selects the word (letters and digits) around the cursor, or everything if there is none.
    pub fn select_word(&mut self) {
        self.clamp();
        let path = self.cur.path.clone();
        let pos = self.cur.pos;
        let r = self.cur_row().clone();
        let is_w = |it: &Item| matches!(it, Item::Char(c) if c.is_alphanumeric() || *c == '.') || matches!(it, Item::Func(_));
        let mut s = pos;
        while s > 0 && is_w(&r[s - 1]) {
            s -= 1;
        }
        let mut e = pos;
        while e < r.len() && is_w(&r[e]) {
            e += 1;
        }
        if s == e {
            self.select_all();
        } else {
            self.anchor = Some(Cursor { path: path.clone(), pos: s });
            self.cur = Cursor { path, pos: e };
        }
    }

    /// Replaces the selection by `wrap` around it. Returns false unless one run of items is selected.
    pub fn wrap_selection(&mut self, wrap: Wrap) -> bool {
        let Some(s) = self.selection() else { return false };
        let Some(row) = row_at(&mut self.row, &s.path) else { return false };
        let sel: Row = row.drain(s.start..s.end).collect();
        let (item, inner): (Item, Option<usize>) = match wrap {
            Wrap::Frac => (Item::Frac(sel, vec![]), Some(1)),
            Wrap::Sqrt => (Item::Sqrt(None, sel), None),
            Wrap::NthRoot => (Item::Sqrt(Some(vec![]), sel), Some(1)),
            Wrap::Group(o) => (Item::Group(o, closer_of(o), sel), None),
        };
        row.insert(s.start, item);
        self.anchor = None;
        self.cur = match inner {
            Some(slot) => {
                let mut p = s.path;
                p.push((s.start, slot));
                Cursor { path: p, pos: 0 }
            }
            None => Cursor { path: s.path, pos: s.start + 1 },
        };
        true
    }

    /// Starts (or keeps) a selection so that the next cursor move extends it.
    pub fn begin_extend(&mut self) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cur.clone());
        }
    }

    /// Drops the selection if it has collapsed to nothing.
    pub fn end_extend(&mut self) {
        if self.anchor.as_ref() == Some(&self.cur) {
            self.anchor = None;
        }
    }

    /// Collapses the selection to its left or right edge (what the arrow keys do).
    pub fn collapse_selection(&mut self, to_end: bool) -> bool {
        let Some((lo, hi)) = self.selection_bounds() else {
            self.anchor = None;
            return false;
        };
        self.cur = if to_end { hi } else { lo };
        self.anchor = None;
        true
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
        // Backspace after `)` removes only the closing bracket.
        let mut e = typed("(ab)+c");
        e.cur = Cursor { path: vec![], pos: 1 };
        e.backspace();
        assert_eq!(e.latex(), "(ab+c");
        assert_eq!(e.cur, Cursor { path: vec![], pos: 3 });
        // Backspace right after `(` removes only the opening bracket.
        let mut e = typed("(ab");
        e.cur = Cursor { path: vec![(0, 0)], pos: 0 };
        e.backspace();
        assert_eq!(e.latex(), "ab)");
        // Delete before `(` and at the end of a group do the same from the other side.
        let mut e = typed("(ab)");
        e.cur = Cursor { path: vec![], pos: 0 };
        e.delete_forward();
        assert_eq!(e.latex(), "ab)");
        let mut e = typed("(ab");
        e.delete_forward();
        assert_eq!(e.latex(), "(ab");
        // Even an empty pair leaves its opposite behind; a second backspace removes that one.
        let mut e = typed("()");
        e.cur = Cursor { path: vec![], pos: 1 };
        e.backspace();
        assert_eq!(e.latex(), "(");
        e.backspace();
        assert!(e.row.is_empty());
        let mut e = typed("()");
        e.cur = Cursor { path: vec![(0, 0)], pos: 0 };
        e.backspace();
        assert_eq!(e.latex(), ")");
        let mut e = typed("()");
        e.cur = Cursor { path: vec![], pos: 0 };
        e.delete_forward();
        assert_eq!(e.latex(), ")");
        let mut e = typed("(");
        e.cur = Cursor { path: vec![(0, 0)], pos: 0 };
        e.delete_forward();
        assert_eq!(e.latex(), "(");
        // Bars always go together.
        let mut e = typed("|ab|");
        e.cur = Cursor { path: vec![], pos: 1 };
        e.backspace();
        assert_eq!(e.latex(), "ab");
    }

    #[test]
    fn backspace_from_denominator_goes_to_numerator() {
        let mut e = typed("a+1/b");
        assert_eq!(e.cur.path, vec![(2, 1)]);
        e.backspace(); // deletes b
        e.backspace(); // empty denominator, non-empty numerator: move up
        assert_eq!(e.cur, Cursor { path: vec![(2, 0)], pos: 1 });
        assert_eq!(e.latex(), "a+\\frac{1}{}");
    }

    #[test]
    fn matrices_of_any_size() {
        let mut e = EditorState::default();
        e.insert_template(Template::Matrix(2, 2));
        assert!(e.in_matrix());
        e.matrix_add_col(true);
        e.matrix_add_row(true);
        let Item::Matrix(m) = &e.row[0] else { panic!() };
        assert_eq!((m.len(), m[0].len()), (3, 3));
        e.type_text("1");
        e.matrix_tab(false);
        e.type_text("2");
        assert_eq!(e.cur.path, vec![(0, 1)]);
        e.matrix_remove_col();
        e.matrix_remove_row();
        let Item::Matrix(m) = &e.row[0] else { panic!() };
        assert_eq!((m.len(), m[0].len()), (2, 2));
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

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn typed(s: &str) -> EditorState {
        let mut e = EditorState::default();
        e.type_text(s);
        e
    }

    fn select(e: &mut EditorState, a: (Vec<(usize, usize)>, usize), c: (Vec<(usize, usize)>, usize)) {
        e.anchor = Some(Cursor { path: a.0, pos: a.1 });
        e.cur = Cursor { path: c.0, pos: c.1 };
    }

    #[test]
    fn selection_within_a_row() {
        let mut e = typed("a+bc");
        select(&mut e, (vec![], 1), (vec![], 3));
        assert_eq!(e.selection(), Some(Selection { path: vec![], start: 1, end: 3 }));
        assert_eq!(e.selection_latex().as_deref(), Some("+b"));
        assert!(e.delete_selection());
        assert_eq!(e.latex(), "ac");
        assert_eq!(e.cur, Cursor { path: vec![], pos: 1 });
        assert!(e.anchor.is_none());
    }

    #[test]
    fn selection_spanning_numerator_and_denominator_takes_the_whole_fraction() {
        let mut e = typed("x+1/2");
        select(&mut e, (vec![(2, 0)], 0), (vec![(2, 1)], 1));
        assert_eq!(e.selection(), Some(Selection { path: vec![], start: 2, end: 3 }));
        e.backspace();
        assert_eq!(e.latex(), "x+");
    }

    #[test]
    fn selection_inside_a_slot_stays_in_the_slot() {
        let mut e = typed("1/abc");
        select(&mut e, (vec![(0, 1)], 0), (vec![(0, 1)], 2));
        assert_eq!(e.selection(), Some(Selection { path: vec![(0, 1)], start: 0, end: 2 }));
        e.type_text("z");
        assert_eq!(e.latex(), "\\frac{1}{zc}");
    }

    #[test]
    fn selection_from_inside_a_structure_to_outside() {
        let mut e = typed("1/2");
        e.cur = Cursor { path: vec![], pos: 1 };
        e.type_text("+y");
        // From the numerator to after `y`: everything from the fraction on is selected.
        select(&mut e, (vec![(0, 0)], 0), (vec![], 3));
        assert_eq!(e.selection(), Some(Selection { path: vec![], start: 0, end: 3 }));
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut e = typed("abc");
        select(&mut e, (vec![], 0), (vec![], 2));
        e.type_text("x");
        assert_eq!(e.latex(), "xc");
    }

    #[test]
    fn slash_and_brackets_wrap_the_selection() {
        let mut e = typed("a+b");
        e.select_all();
        e.type_char('/');
        assert_eq!(e.latex(), "\\frac{a+b}{}");
        assert_eq!(e.cur, Cursor { path: vec![(0, 1)], pos: 0 });
        let mut e = typed("a+b");
        select(&mut e, (vec![], 2), (vec![], 3));
        e.type_char('(');
        assert_eq!(e.latex(), "a+\\left( b \\right)");
        assert_eq!(e.cur, Cursor { path: vec![], pos: 3 });
        let mut e = typed("x+1");
        e.select_all();
        e.insert_template(Template::Sqrt);
        assert_eq!(e.latex(), "\\sqrt{x+1}");
        let mut e = typed("x");
        e.select_all();
        e.insert_template(Template::NthRoot);
        assert_eq!(e.latex(), "\\sqrt[]{x}");
        assert_eq!(e.cur.path, vec![(0, 1)]);
    }

    #[test]
    fn paste_replaces_selection() {
        let mut e = typed("abc");
        select(&mut e, (vec![], 1), (vec![], 2));
        e.paste("\\frac{1}{2}");
        assert_eq!(e.latex(), "a\\frac{1}{2}c");
    }

    #[test]
    fn word_and_all_selection_and_collapse() {
        let mut e = typed("1+abc");
        e.cur = Cursor { path: vec![], pos: 3 };
        e.select_word();
        assert_eq!(e.selection(), Some(Selection { path: vec![], start: 2, end: 5 }));
        assert!(e.collapse_selection(false));
        assert_eq!(e.cur.pos, 2);
        e.select_all();
        assert_eq!(e.selection(), Some(Selection { path: vec![], start: 0, end: 5 }));
        assert!(e.collapse_selection(true));
        assert_eq!(e.cur.pos, 5);
        // A stale anchor (for example after the row changed) yields no selection.
        e.anchor = Some(Cursor { path: vec![(9, 0)], pos: 0 });
        assert_eq!(e.selection(), None);
    }
}

#[cfg(test)]
mod bracket_selection_tests {
    use super::*;

    fn typed(s: &str) -> EditorState {
        let mut e = EditorState::default();
        e.type_text(s);
        e
    }

    fn latex_of(rows: Option<Row>) -> Option<String> {
        rows.map(|r| row_to_latex(&r))
    }

    #[test]
    fn selecting_into_brackets_selects_only_the_terms_passed() {
        let mut e = typed("a(b+c)d");
        e.cur = Cursor { path: vec![], pos: 4 };
        // From before `a` into the bracket, after `b+`.
        e.anchor = Some(Cursor { path: vec![], pos: 0 });
        e.cur = Cursor { path: vec![(1, 0)], pos: 2 };
        let segs = e.selection_segments();
        assert_eq!(
            segs,
            vec![Selection { path: vec![], start: 0, end: 1 }, Selection { path: vec![(1, 0)], start: 0, end: 2 }]
        );
        // The opening bracket is selected too, in its place between `a` and `b`.
        assert_eq!(latex_of(e.selected_row()).as_deref(), Some("a(b+"));
        assert!(e.delete_selection());
        // Its closing partner stays behind as a lone bracket.
        assert_eq!(e.latex(), "c)d");
        assert_eq!(e.cur, Cursor { path: vec![], pos: 0 });
    }

    #[test]
    fn selecting_out_of_brackets_and_across_them() {
        let mut e = typed("a(b+c)d");
        // From inside after `b` to after `d`.
        e.anchor = Some(Cursor { path: vec![(1, 0)], pos: 1 });
        e.cur = Cursor { path: vec![], pos: 3 };
        assert_eq!(
            e.selection_segments(),
            vec![Selection { path: vec![(1, 0)], start: 1, end: 3 }, Selection { path: vec![], start: 2, end: 3 }]
        );
        // The same selection built in the other direction is identical.
        let mut f = typed("a(b+c)d");
        f.anchor = Some(Cursor { path: vec![], pos: 3 });
        f.cur = Cursor { path: vec![(1, 0)], pos: 1 };
        assert_eq!(f.selection_segments(), e.selection_segments());
        assert_eq!(latex_of(e.selected_row()).as_deref(), Some("+c)d"));
        assert!(e.delete_selection());
        assert_eq!(e.latex(), "a(b");
        assert_eq!(e.cur, Cursor { path: vec![], pos: 3 });
        // Through the whole bracket from outside to outside selects the bracket pair as a whole item.
        let mut g = typed("a(b+c)d");
        g.anchor = Some(Cursor { path: vec![], pos: 0 });
        g.cur = Cursor { path: vec![], pos: 3 };
        assert_eq!(g.selection_segments(), vec![Selection { path: vec![], start: 0, end: 3 }]);
    }

    #[test]
    fn a_bracket_can_be_selected_on_its_own() {
        let mut e = typed("a(b+c)d");
        // One step right from just before `(`: only the bracket is selected.
        e.anchor = Some(Cursor { path: vec![], pos: 1 });
        e.cur = Cursor { path: vec![(1, 0)], pos: 0 };
        assert_eq!(e.selection_parts(), vec![Part::Open { parent: vec![], idx: 1 }]);
        assert_eq!(latex_of(e.selected_row()).as_deref(), Some("("));
        assert!(e.delete_selection());
        assert_eq!(e.latex(), "ab+c)d");
        // The same from the closing side.
        let mut f = typed("a(b+c)d");
        f.anchor = Some(Cursor { path: vec![(1, 0)], pos: 3 });
        f.cur = Cursor { path: vec![], pos: 2 };
        assert_eq!(f.selection_parts(), vec![Part::Close { parent: vec![], idx: 1 }]);
        assert!(f.delete_selection());
        assert_eq!(f.latex(), "a(b+cd");
    }

    #[test]
    fn nested_brackets_are_selected_and_deleted_term_by_term() {
        // Ending deep inside: both opening brackets on the way are selected.
        let mut e = typed("a((b)c)d");
        e.anchor = Some(Cursor { path: vec![], pos: 0 });
        e.cur = Cursor { path: vec![(1, 0), (0, 0)], pos: 1 };
        assert_eq!(latex_of(e.selected_row()).as_deref(), Some("a((b"));
        assert!(e.delete_selection());
        assert_eq!(e.latex(), ")c)d");
        // Starting deep inside: both closing brackets on the way out are selected.
        let mut f = typed("a((b)c)d");
        f.anchor = Some(Cursor { path: vec![(1, 0), (0, 0)], pos: 0 });
        f.cur = Cursor { path: vec![], pos: 3 };
        assert_eq!(latex_of(f.selected_row()).as_deref(), Some("b)c)d"));
        assert!(f.delete_selection());
        assert_eq!(f.latex(), "a((");
        assert_eq!(f.cur, Cursor { path: vec![], pos: 3 });
    }

    #[test]
    fn selection_between_two_brackets() {
        let mut e = typed("a(b)c(d)e");
        e.anchor = Some(Cursor { path: vec![(1, 0)], pos: 0 });
        e.cur = Cursor { path: vec![(3, 0)], pos: 1 };
        assert_eq!(latex_of(e.selected_row()).as_deref(), Some("b)c(d"));
        assert!(e.delete_selection());
        assert_eq!(e.latex(), "a()e");
        assert_eq!(e.cur, Cursor { path: vec![], pos: 2 });
        // Everything stays editable and consistent afterwards.
        e.type_text("x");
        assert_eq!(e.latex(), "a(x)e");
    }

    #[test]
    fn selecting_inside_a_bracket_only_stays_there() {
        let mut e = typed("(abc)");
        e.anchor = Some(Cursor { path: vec![(0, 0)], pos: 1 });
        e.cur = Cursor { path: vec![(0, 0)], pos: 3 };
        assert_eq!(e.selection_segments(), vec![Selection { path: vec![(0, 0)], start: 1, end: 3 }]);
    }

    #[test]
    fn fractions_inside_brackets_still_select_whole() {
        let mut e = typed("x(1/2)");
        // From before x to inside the denominator, which is inside the bracket.
        e.anchor = Some(Cursor { path: vec![], pos: 0 });
        e.cur = Cursor { path: vec![(1, 0), (0, 1)], pos: 1 };
        let segs = e.selection_segments();
        assert_eq!(segs, vec![Selection { path: vec![], start: 0, end: 1 }, Selection { path: vec![(1, 0)], start: 0, end: 1 }]);
    }

    #[test]
    fn collapse_goes_to_the_real_edges() {
        let mut e = typed("a(b+c)d");
        e.anchor = Some(Cursor { path: vec![], pos: 0 });
        e.cur = Cursor { path: vec![(1, 0)], pos: 2 };
        assert!(e.collapse_selection(true));
        assert_eq!(e.cur, Cursor { path: vec![(1, 0)], pos: 2 });
        e.anchor = Some(Cursor { path: vec![], pos: 0 });
        e.cur = Cursor { path: vec![(1, 0)], pos: 2 };
        assert!(e.collapse_selection(false));
        assert_eq!(e.cur, Cursor { path: vec![], pos: 0 });
    }
}
