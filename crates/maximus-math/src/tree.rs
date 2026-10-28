//! Structural math tree: the editor model, the renderer input, and the LaTeX bridge.

use crate::words::*;
use serde::{Deserialize, Serialize};

pub type Row = Vec<Item>;

/// Marks a superscript as Maxima's non-commutative power `^^` (hidden when drawn).
pub const NC_MARK: char = '\u{2063}';

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BigOp {
    /// Definite integral (limit slots).
    Int,
    /// Integral without limits.
    IntNoLimits,
    Sum,
    Prod,
    Lim,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Item {
    Char(char),
    /// Upright name (function or multi-letter identifier).
    Func(String),
    /// Quoted string.
    Text(String),
    Frac(Row, Row),
    Sup(Row),
    Sub(Row),
    Sqrt(Option<Row>, Row),
    /// Big operator with lower and upper limits. The operand follows in the enclosing row.
    Big(BigOp, Row, Row),
    Group(char, char, Row),
    Matrix(Vec<Vec<Row>>),
}

pub fn row_from_str(s: &str) -> Row {
    s.chars().map(Item::Char).collect()
}

pub fn closer_of(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '|' => '|',
        c => c,
    }
}

// ---------------------------------------------------------------- LaTeX output

pub fn row_to_latex(row: &[Item]) -> String {
    let mut out = String::new();
    write_row(row, &mut out);
    out.trim().to_string()
}

fn push_cmd(out: &mut String, cmd: &str) {
    out.push('\\');
    out.push_str(cmd);
    out.push(' ');
}

fn char_to_latex(c: char, out: &mut String) {
    match c {
        NC_MARK => push_cmd(out, "wedge"),
        '·' => push_cmd(out, "cdot"),
        '×' => push_cmd(out, "times"),
        '÷' => push_cmd(out, "div"),
        '≤' => push_cmd(out, "le"),
        '≥' => push_cmd(out, "ge"),
        '≠' => push_cmd(out, "ne"),
        '→' => push_cmd(out, "to"),
        '∞' => push_cmd(out, "infty"),
        '∂' => push_cmd(out, "partial"),
        '∇' => push_cmd(out, "nabla"),
        '±' => push_cmd(out, "pm"),
        '≈' => push_cmd(out, "approx"),
        '∈' => push_cmd(out, "in"),
        '∧' => push_cmd(out, "land"),
        '∨' => push_cmd(out, "lor"),
        '¬' => push_cmd(out, "neg"),
        '°' => out.push_str("^\\circ "),
        'ⅈ' => out.push_str("\\mathrm{i}"),
        'ⅇ' => out.push_str("\\mathrm{e}"),
        '−' => out.push('-'),
        '{' => out.push_str("\\{"),
        '}' => out.push_str("\\}"),
        '%' => out.push_str("\\%"),
        '_' => out.push_str("\\_"),
        '#' => out.push_str("\\#"),
        '$' => out.push_str("\\$"),
        '&' => out.push_str("\\&"),
        c => {
            if let Some(l) = glyph_to_greek_latex(c) {
                push_cmd(out, l);
            } else {
                out.push(c);
            }
        }
    }
}

fn write_row(row: &[Item], out: &mut String) {
    let mut i = 0;
    while i < row.len() {
        match &row[i] {
            Item::Char(c) if c.is_ascii_alphabetic() => {
                let (run, j, paren) = collect_run(row, i);
                for (_, _, w) in split_letters_ctx(&run, paren) {
                    match w {
                        Word::Func(f) => match maxima_func_to_latex(&f) {
                            Some(l) => push_cmd(out, l),
                            None => out.push_str(&format!("\\mathrm{{{}}}", f)),
                        },
                        Word::Greek(g) => char_to_latex(g, out),
                        Word::Inf => push_cmd(out, "infty"),
                        Word::Letter(c) => out.push(c),
                        Word::Num(n) => out.push_str(&n),
                    }
                }
                i = j;
                continue;
            }
            Item::Char(c) => char_to_latex(*c, out),
            Item::Func(n) => match maxima_func_to_latex(n) {
                Some(l) => push_cmd(out, l),
                None => out.push_str(&format!("\\mathrm{{{}}}", n)),
            },
            Item::Text(t) => out.push_str(&format!("\\text{{{}}}", t)),
            Item::Frac(a, b) => {
                out.push_str("\\frac{");
                write_row(a, out);
                out.push_str("}{");
                write_row(b, out);
                out.push('}');
            }
            Item::Sup(a) => {
                out.push_str("^{");
                write_row(a, out);
                out.push('}');
            }
            Item::Sub(a) => {
                out.push_str("_{");
                write_row(a, out);
                out.push('}');
            }
            Item::Sqrt(idx, b) => {
                out.push_str("\\sqrt");
                if let Some(idx) = idx {
                    out.push('[');
                    write_row(idx, out);
                    out.push(']');
                }
                out.push('{');
                write_row(b, out);
                out.push('}');
            }
            Item::Big(op, lo, hi) => {
                let cmd = match op {
                    BigOp::Int | BigOp::IntNoLimits => "int",
                    BigOp::Sum => "sum",
                    BigOp::Prod => "prod",
                    BigOp::Lim => "lim",
                };
                push_cmd(out, cmd);
                if !lo.is_empty() {
                    out.push_str("_{");
                    write_row(lo, out);
                    out.push('}');
                }
                if !hi.is_empty() {
                    out.push_str("^{");
                    write_row(hi, out);
                    out.push('}');
                }
                out.push(' ');
            }
            Item::Group(o, c, b) => {
                let esc = |c: char| if matches!(c, '{' | '}') { format!("\\{}", c) } else { c.to_string() };
                out.push_str(&format!("\\left{} ", esc(*o)));
                write_row(b, out);
                out.push_str(&format!(" \\right{}", esc(*c)));
            }
            Item::Matrix(rows) => {
                out.push_str("\\begin{pmatrix}");
                for (ri, r) in rows.iter().enumerate() {
                    if ri > 0 {
                        out.push_str("\\\\ ");
                    }
                    for (ci, cell) in r.iter().enumerate() {
                        if ci > 0 {
                            out.push_str(" & ");
                        }
                        write_row(cell, out);
                    }
                }
                out.push_str("\\end{pmatrix}");
            }
        }
        i += 1;
    }
}

// ---------------------------------------------------------------- LaTeX input

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Cmd(String),
    Ch(char),
    LBrace,
    RBrace,
}

fn lex(src: &str) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                i += 1;
                if i >= chars.len() {
                    break;
                }
                if chars[i].is_ascii_alphabetic() {
                    let s = i;
                    while i < chars.len() && chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    out.push(Tok::Cmd(chars[s..i].iter().collect()));
                } else {
                    out.push(Tok::Cmd(chars[i].to_string()));
                    i += 1;
                }
                continue;
            }
            '{' => out.push(Tok::LBrace),
            '}' => out.push(Tok::RBrace),
            c if c.is_whitespace() => {}
            '$' => {}
            c => out.push(Tok::Ch(c)),
        }
        i += 1;
    }
    out
}

struct LParser {
    toks: Vec<Tok>,
    pos: usize,
}

#[derive(PartialEq, Clone, Copy)]
enum Stop {
    Eof,
    Brace,
    Bracket,
    Amp,
}

impl LParser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    /// Parses a braced group or a single token as a row.
    fn arg(&mut self) -> Row {
        match self.peek() {
            Some(Tok::LBrace) => {
                self.pos += 1;
                self.row(Stop::Brace)
            }
            Some(_) => {
                let mut r = Vec::new();
                let t = self.next().unwrap();
                self.push_tok(t, &mut r);
                r
            }
            None => Vec::new(),
        }
    }

    fn row(&mut self, stop: Stop) -> Row {
        let mut out: Row = Vec::new();
        // Stack of open delimiters (index into `out` of a placeholder is awkward), so we recurse.
        loop {
            let Some(t) = self.peek().cloned() else { break };
            match (&t, stop) {
                (Tok::RBrace, Stop::Brace) => {
                    self.pos += 1;
                    break;
                }
                (Tok::RBrace, _) => {
                    self.pos += 1;
                    continue;
                }
                (Tok::Ch(']'), Stop::Bracket) => {
                    self.pos += 1;
                    break;
                }
                (Tok::Ch('&'), Stop::Amp) => break,
                (Tok::Cmd(c), Stop::Amp) if c == "\\" || c == "end" => break,
                _ => {}
            }
            self.pos += 1;
            self.push_tok(t, &mut out);
        }
        normalize_delims(out)
    }

    fn push_tok(&mut self, t: Tok, out: &mut Row) {
        match t {
            Tok::LBrace => {
                let r = self.row(Stop::Brace);
                // A bare `{...}` group: splice its contents.
                out.extend(r);
            }
            Tok::RBrace => {}
            Tok::Ch(c) => match c {
                '^' => {
                    let r = self.arg();
                    attach_script(out, true, r);
                }
                '_' => {
                    let r = self.arg();
                    attach_script(out, false, r);
                }
                '*' => out.push(Item::Char('·')),
                '\'' => out.push(Item::Char('\'')),
                '-' => out.push(Item::Char('−')),
                c => out.push(Item::Char(c)),
            },
            Tok::Cmd(cmd) => self.command(&cmd, out),
        }
    }

    fn command(&mut self, cmd: &str, out: &mut Row) {
        match cmd {
            "frac" | "dfrac" | "tfrac" => {
                let a = self.arg();
                let b = self.arg();
                out.push(Item::Frac(a, b));
            }
            "sqrt" => {
                let mut idx = None;
                if matches!(self.peek(), Some(Tok::Ch('['))) {
                    self.pos += 1;
                    idx = Some(self.row(Stop::Bracket));
                }
                let b = self.arg();
                out.push(Item::Sqrt(idx, b));
            }
            "int" | "sum" | "prod" | "lim" | "iint" | "oint" => {
                let (mut lo, mut hi) = (Vec::new(), Vec::new());
                let mut has_limits = false;
                loop {
                    match self.peek() {
                        Some(Tok::Ch('_')) => {
                            self.pos += 1;
                            has_limits = true;
                            lo = self.arg();
                        }
                        Some(Tok::Ch('^')) => {
                            self.pos += 1;
                            has_limits = true;
                            hi = self.arg();
                        }
                        Some(Tok::Cmd(c)) if c == "limits" || c == "nolimits" => {
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                let op = match cmd {
                    "sum" => BigOp::Sum,
                    "prod" => BigOp::Prod,
                    "lim" => BigOp::Lim,
                    _ if has_limits => BigOp::Int,
                    _ => BigOp::IntNoLimits,
                };
                out.push(Item::Big(op, lo, hi));
            }
            "left" | "right" => {
                let left = cmd == "left";
                let marker = |l: bool| if l { '\u{1}' } else { '\u{2}' };
                match self.next() {
                    Some(Tok::Ch(c @ ('(' | ')' | '[' | ']'))) => out.push(Item::Char(c)),
                    Some(Tok::Ch('|')) => out.push(Item::Char(marker(left))),
                    Some(Tok::Cmd(c)) => match c.as_str() {
                        "{" | "lbrace" => out.push(Item::Char('{')),
                        "}" | "rbrace" => out.push(Item::Char('}')),
                        "|" | "vert" | "Vert" => out.push(Item::Char(marker(left))),
                        "langle" => out.push(Item::Char('(')),
                        "rangle" => out.push(Item::Char(')')),
                        _ => {}
                    },
                    _ => {}
                }
            }
            "begin" => {
                let name = self.braced_name();
                if name.ends_with("matrix") || name == "array" || name == "cases" {
                    if name == "array" {
                        let _ = self.braced_name();
                    }
                    let mut rows: Vec<Vec<Row>> = Vec::new();
                    let mut cur: Vec<Row> = Vec::new();
                    loop {
                        let cell = self.row(Stop::Amp);
                        cur.push(cell);
                        match self.peek().cloned() {
                            Some(Tok::Ch('&')) => {
                                self.pos += 1;
                            }
                            Some(Tok::Cmd(c)) if c == "\\" => {
                                self.pos += 1;
                                rows.push(std::mem::take(&mut cur));
                            }
                            Some(Tok::Cmd(c)) if c == "end" => {
                                self.pos += 1;
                                let _ = self.braced_name();
                                if !cur.is_empty() && !(cur.len() == 1 && cur[0].is_empty()) {
                                    rows.push(cur);
                                }
                                break;
                            }
                            _ => {
                                if !cur.is_empty() {
                                    rows.push(cur);
                                }
                                break;
                            }
                        }
                    }
                    out.push(Item::Matrix(rows));
                }
            }
            "end" => {
                let _ = self.braced_name();
            }
            "text" | "mathrm" | "operatorname" | "mathit" | "mathbf" | "textrm" | "mbox" | "mathsf" => {
                let content = self.raw_group();
                if cmd == "text" || cmd == "mbox" || cmd == "textrm" {
                    push_text_content(&content, out);
                } else if content.chars().count() == 1 {
                    // \mathrm{d} stays an ordinary letter.
                    out.push(Item::Char(content.chars().next().unwrap()));
                } else if content.chars().all(|c| c.is_alphanumeric()) {
                    out.push(Item::Func(content));
                } else {
                    push_text_content(&content, out);
                }
            }
            "cdot" | "ast" | "bullet" => out.push(Item::Char('·')),
            "times" => out.push(Item::Char('×')),
            "div" => out.push(Item::Char('÷')),
            "le" | "leq" | "leqslant" => out.push(Item::Char('≤')),
            "ge" | "geq" | "geqslant" => out.push(Item::Char('≥')),
            "ne" | "neq" => out.push(Item::Char('≠')),
            "to" | "rightarrow" | "longrightarrow" => out.push(Item::Char('→')),
            "infty" => out.push(Item::Char('∞')),
            "partial" => out.push(Item::Char('∂')),
            "nabla" => out.push(Item::Char('∇')),
            "pm" => out.push(Item::Char('±')),
            "approx" => out.push(Item::Char('≈')),
            "in" => out.push(Item::Char('∈')),
            "land" | "wedge" => out.push(Item::Char('∧')),
            "lor" | "vee" => out.push(Item::Char('∨')),
            "neg" | "lnot" => out.push(Item::Char('¬')),
            "circ" => out.push(Item::Char('°')),
            "imath" | "jmath" => out.push(Item::Char('ⅈ')),
            "{" | "lbrace" => out.push(Item::Char('{')),
            "}" | "rbrace" => out.push(Item::Char('}')),
            "%" => out.push(Item::Char('%')),
            "_" => out.push(Item::Char('_')),
            "#" => out.push(Item::Char('#')),
            "$" => out.push(Item::Char('$')),
            "&" => out.push(Item::Char('&')),
            "|" | "Vert" | "vert" => out.push(Item::Char('|')),
            "langle" => out.push(Item::Char('(')),
            "rangle" => out.push(Item::Char(')')),
            "," | "!" | ";" | ":" | " " | ">" | "quad" | "qquad" | "displaystyle" | "textstyle" | "scriptstyle"
            | "limits" | "nolimits" | "big" | "Big" | "bigg" | "Bigg" | "cdots" | "ldots" | "dots" | "bigl"
            | "bigr" => {}
            c => {
                if let Some(g) = greek_latex_to_glyph(c) {
                    out.push(Item::Char(g));
                } else if is_known_func(latex_func_to_maxima(c)) || matches!(c, "arccot" | "arcsec" | "arccsc") {
                    out.push(Item::Func(latex_func_to_maxima(c).to_string()));
                } else if c.chars().all(|ch| ch.is_ascii_alphabetic()) {
                    out.push(Item::Func(c.to_string()));
                }
            }
        }
    }

    fn braced_name(&mut self) -> String {
        self.raw_group()
    }

    /// Reads a `{...}` group as raw text (letters only, other tokens flattened).
    fn raw_group(&mut self) -> String {
        let mut s = String::new();
        if matches!(self.peek(), Some(Tok::LBrace)) {
            self.pos += 1;
            let mut depth = 1;
            while let Some(t) = self.next() {
                match t {
                    Tok::LBrace => depth += 1,
                    Tok::RBrace => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Tok::Ch(c) => s.push(c),
                    Tok::Cmd(c) => {
                        if let Some(g) = greek_latex_to_glyph(&c) {
                            s.push(g)
                        } else if c == "_" {
                            s.push('_')
                        }
                    }
                }
            }
        }
        s
    }
}

fn push_text_content(content: &str, out: &mut Row) {
    // Letters become ordinary typed letters (so `\text{integrate(}` acts like typing `integrate(`).
    for c in content.chars() {
        if !c.is_whitespace() {
            out.push(Item::Char(c));
        }
    }
}

fn attach_script(out: &mut Row, sup: bool, mut r: Row) {
    if sup && matches!(r.first(), Some(Item::Char('∧'))) {
        r[0] = Item::Char(NC_MARK);
    }
    // A script on `\prime`-like empties is dropped; otherwise it is its own item.
    if sup {
        out.push(Item::Sup(r));
    } else {
        out.push(Item::Sub(r));
    }
}

/// Pairs delimiter characters into groups. The markers 1/2 stand for `\left|` and `\right|`.
fn normalize_delims(row: Row) -> Row {
    fn is_open(c: char) -> bool {
        matches!(c, '(' | '[' | '{' | '\u{1}')
    }
    fn is_close(c: char) -> bool {
        matches!(c, ')' | ']' | '}' | '\u{2}')
    }
    fn matches_pair(o: char, c: char) -> bool {
        match o {
            '(' | '[' | '{' => matches!(c, ')' | ']' | '}'),
            '\u{1}' => c == '\u{2}',
            _ => false,
        }
    }
    // stack of (open char, items)
    let mut stack: Vec<(char, Row)> = vec![(' ', Vec::new())];
    for item in row {
        match &item {
            Item::Char(c) if is_open(*c) => stack.push((*c, Vec::new())),
            Item::Char('|') => {
                if stack.last().map_or(false, |(o, _)| *o == '|') {
                    let (_, body) = stack.pop().unwrap();
                    stack.last_mut().unwrap().1.push(Item::Group('|', '|', body));
                } else {
                    stack.push(('|', Vec::new()));
                }
            }
            Item::Char(c) if is_close(*c) => {
                // Find a matching opener in the stack.
                let pos = stack.iter().rposition(|(o, _)| matches_pair(*o, *c));
                match pos {
                    Some(p) => {
                        while stack.len() - 1 > p {
                            // Unclosed inner openers stay as plain characters.
                            let (o, body) = stack.pop().unwrap();
                            flatten_open(&mut stack, o, body);
                        }
                        let (o, body) = stack.pop().unwrap();
                        let cc = if *c == '\u{2}' { '|' } else { *c };
                        let g = make_group(o, cc, body);
                        stack.last_mut().unwrap().1.push(g);
                    }
                    None => stack.last_mut().unwrap().1.push(item),
                }
            }
            _ => stack.last_mut().unwrap().1.push(item),
        }
    }
    while stack.len() > 1 {
        let (o, body) = stack.pop().unwrap();
        flatten_open(&mut stack, o, body);
    }
    stack.pop().unwrap().1
}

/// Puts an opener that never got closed back into its parent as a plain character followed by its content.
fn flatten_open(stack: &mut Vec<(char, Row)>, o: char, body: Row) {
    let visible = if o == '\u{1}' { '|' } else { o };
    let parent = &mut stack.last_mut().unwrap().1;
    parent.push(Item::Char(visible));
    parent.extend(body);
}

fn make_group(o: char, c: char, body: Row) -> Item {
    let o = if o == '\u{1}' { '|' } else { o };
    Item::Group(o, c, body)
}

fn is_atom_item(it: &Item) -> bool {
    matches!(it, Item::Char(c) if c.is_alphanumeric() || *c == '.' || *c == '∞' || *c == 'ⅇ' || *c == 'ⅈ') || matches!(it, Item::Func(_))
}

/// Removes brackets that change nothing: around a whole statement, around a single atom that is
/// not a function argument, and doubled brackets. Function arguments are always kept.
pub fn tidy_parens(row: Row) -> Row {
    // Whole-statement brackets: split at terminators, unwrap a statement that is one `( ... )` group.
    let mut out: Row = Vec::new();
    let mut stmt: Row = Vec::new();
    let flush = |stmt: &mut Row, out: &mut Row, term: Option<Item>| {
        let mut s = std::mem::take(stmt);
        while let [Item::Group('(', _, _)] = s.as_slice() {
            let Item::Group(_, _, body) = s.remove(0) else { unreachable!() };
            s = body;
        }
        out.extend(s);
        if let Some(t) = term {
            out.push(t);
        }
    };
    for it in row {
        match it {
            Item::Char(c @ (';' | '$')) => flush(&mut stmt, &mut out, Some(Item::Char(c))),
            it => stmt.push(it),
        }
    }
    flush(&mut stmt, &mut out, None);
    tidy_inner(out)
}

fn tidy_inner(row: Row) -> Row {
    let mut out: Row = Vec::with_capacity(row.len());
    for it in row {
        let it = match it {
            Item::Frac(a, b) => Item::Frac(tidy_slot(a), tidy_slot(b)),
            Item::Sup(a) => Item::Sup(tidy_slot(a)),
            Item::Sub(a) => Item::Sub(tidy_slot(a)),
            Item::Sqrt(i, b) => Item::Sqrt(i.map(tidy_slot), tidy_slot(b)),
            Item::Big(o, a, b) => Item::Big(o, tidy_slot(a), tidy_slot(b)),
            Item::Matrix(m) => Item::Matrix(m.into_iter().map(|r| r.into_iter().map(tidy_slot).collect()).collect()),
            Item::Group(o, c, b) => Item::Group(o, c, tidy_inner(b)),
            it => it,
        };
        // A group directly after an operand is a call (`f(x)`), so it stays.
        let after_operand = out.last().map_or(false, |p| is_atom_item(p) || matches!(p, Item::Group(..) | Item::Sup(_) | Item::Sub(_) | Item::Frac(..) | Item::Sqrt(..)));
        match it {
            Item::Group('(', _, mut body) if !after_operand => {
                // Doubled brackets collapse; a bracket around one atom disappears.
                while let [Item::Group('(', _, _)] = body.as_slice() {
                    let Item::Group(_, _, inner) = body.remove(0) else { unreachable!() };
                    body = inner;
                }
                let single = body.len() == 1 && is_atom_item(&body[0]) || (!body.is_empty() && body.iter().all(|i| matches!(i, Item::Char(c) if c.is_ascii_digit() || *c == '.')));
                if single {
                    out.extend(body);
                } else {
                    out.push(Item::Group('(', ')', body));
                }
            }
            it => out.push(it),
        }
    }
    out
}

fn tidy_slot(r: Row) -> Row {
    // Inside a slot the whole content is one expression, so outer brackets are redundant.
    let mut r = tidy_inner(r);
    while let [Item::Group('(', _, _)] = r.as_slice() {
        let Item::Group(_, _, body) = r.remove(0) else { unreachable!() };
        r = tidy_inner(body);
    }
    r
}

/// Parses LaTeX (math mode content) into a tree.
pub fn from_latex(src: &str) -> Row {
    let src = src.trim();
    let src = src
        .strip_prefix("$$")
        .and_then(|s| s.strip_suffix("$$"))
        .or_else(|| src.strip_prefix("\\[").and_then(|s| s.strip_suffix("\\]")))
        .or_else(|| src.strip_prefix("\\(").and_then(|s| s.strip_suffix("\\)")))
        .or_else(|| src.strip_prefix('$').and_then(|s| s.strip_suffix('$')))
        .unwrap_or(src);
    let mut p = LParser { toks: lex(src), pos: 0 };
    let mut out = Vec::new();
    while p.pos < p.toks.len() {
        let r = p.row(Stop::Eof);
        out.extend(r);
    }
    out
}

/// Parses arithmetic characters typed in the editor: converts `*` etc. for display.
pub fn display_char(c: char) -> char {
    match c {
        '*' => '·',
        '-' => '−',
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latex_roundtrip_frac() {
        let r = from_latex("\\frac{1}{2}x^2");
        assert_eq!(r.len(), 3);
        let l = row_to_latex(&r);
        assert_eq!(from_latex(&l), r);
    }

    #[test]
    fn groups_pair() {
        let r = from_latex("\\left(x\\right)+(y)");
        assert!(matches!(r[0], Item::Group('(', ')', _)));
        assert!(matches!(r[2], Item::Group('(', ')', _)));
    }

    #[test]
    fn weird_example_balances() {
        let r = from_latex("\\text{integrate(}x\\sin^3\\left(x),x,0,\\infty\\right)");
        let last = r.last().unwrap();
        assert!(matches!(last, Item::Group('(', ')', _)), "{:?}", r);
    }
}
