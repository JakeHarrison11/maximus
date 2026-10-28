//! Textbook (2D tree) -> semantic expression parser.

use crate::expr::*;
use crate::tree::*;
use crate::words::*;
use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct ParseCtx {
    /// Single-letter (or other) names known to be functions, so `f(x)` is a call, not `f*(x)`.
    pub functions: HashSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub expr: Expr,
    /// `false` when the statement was terminated with `$` (result hidden).
    pub display: bool,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
enum S {
    Num(String),
    Sym(String),
    Func(String),
    Str(String),
    Op(&'static str),
    Comma,
    Semi(char),
    Deg,
    Group(char, char, Row),
    Frac(Row, Row),
    Sup(Row),
    Sub(Row),
    Sqrt(Option<Row>, Row),
    Big(BigOp, Row, Row),
    Matrix(Vec<Vec<Row>>),
}

fn tokenize(row: &[Item]) -> Result<Vec<S>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < row.len() {
        match &row[i] {
            Item::Char(c) if c.is_ascii_digit() || (*c == '.' && matches!(row.get(i + 1), Some(Item::Char(d)) if d.is_ascii_digit())) => {
                let mut s = String::new();
                let mut dot = false;
                while let Some(Item::Char(d)) = row.get(i) {
                    if d.is_ascii_digit() {
                        s.push(*d);
                    } else if *d == '.' && !dot && matches!(row.get(i + 1), Some(Item::Char(n)) if n.is_ascii_digit()) {
                        dot = true;
                        s.push('.');
                    } else {
                        break;
                    }
                    i += 1;
                }
                out.push(S::Num(s));
                continue;
            }
            Item::Char(c) if c.is_ascii_alphabetic() => {
                let (run, j) = collect_run(row, i);
                i = j;
                for (_, _, w) in split_letters(&run) {
                    out.push(match w {
                        Word::Func(f) => S::Func(f),
                        Word::Greek(g) => S::Sym(greek_name_of_glyph(g).unwrap().to_string()),
                        Word::Inf => S::Sym("inf".into()),
                        Word::Letter(c) => S::Sym(c.to_string()),
                        Word::Num(n) => S::Num(n),
                    });
                }
                continue;
            }
            Item::Char(c) => {
                let c = *c;
                let next = row.get(i + 1);
                let next_is = |x: char| matches!(next, Some(Item::Char(n)) if *n == x);
                match c {
                    '+' => out.push(S::Op("+")),
                    '-' | '−' => out.push(S::Op("-")),
                    '*' | '·' | '×' => out.push(S::Op("*")),
                    '/' | '÷' => out.push(S::Op("/")),
                    '=' => {
                        if next_is('=') {
                            i += 1;
                        }
                        out.push(S::Op("="))
                    }
                    '<' if next_is('=') => {
                        i += 1;
                        out.push(S::Op("<="))
                    }
                    '>' if next_is('=') => {
                        i += 1;
                        out.push(S::Op(">="))
                    }
                    '<' => out.push(S::Op("<")),
                    '>' => out.push(S::Op(">")),
                    '≤' => out.push(S::Op("<=")),
                    '≥' => out.push(S::Op(">=")),
                    '≠' => out.push(S::Op("#")),
                    '!' if next_is('=') => {
                        i += 1;
                        out.push(S::Op("#"))
                    }
                    '!' => out.push(S::Op("!")),
                    ':' if next_is('=') => {
                        i += 1;
                        out.push(S::Op(":="))
                    }
                    ':' => out.push(S::Op(":")),
                    '∧' => out.push(S::Op("and")),
                    '∨' => out.push(S::Op("or")),
                    '¬' => out.push(S::Op("not")),
                    '→' => out.push(S::Op("->")),
                    '.' => out.push(S::Op(".")),
                    ',' => out.push(S::Comma),
                    ';' => out.push(S::Semi(';')),
                    '$' => out.push(S::Semi('$')),
                    '°' => out.push(S::Deg),
                    '%' => out.push(S::Sym("%".into())),
                    '∞' => out.push(S::Sym("inf".into())),
                    'ⅇ' => out.push(S::Sym("%e".into())),
                    'ⅈ' => out.push(S::Sym("%i".into())),
                    '\'' => {}
                    c if greek_name_of_glyph(c).is_some() => out.push(S::Sym(greek_name_of_glyph(c).unwrap().to_string())),
                    c if c.is_alphabetic() => out.push(S::Sym(c.to_string())),
                    c => return Err(format!("unsupported symbol '{}'", c)),
                }
            }
            Item::Func(n) => out.push(S::Func(n.clone())),
            Item::Text(t) => out.push(S::Str(t.clone())),
            Item::Frac(a, b) => out.push(S::Frac(a.clone(), b.clone())),
            Item::Sup(a) => out.push(S::Sup(a.clone())),
            Item::Sub(a) => out.push(S::Sub(a.clone())),
            Item::Sqrt(a, b) => out.push(S::Sqrt(a.clone(), b.clone())),
            Item::Big(o, a, b) => out.push(S::Big(*o, a.clone(), b.clone())),
            Item::Group(o, c, b) => out.push(S::Group(*o, *c, b.clone())),
            Item::Matrix(m) => out.push(S::Matrix(m.clone())),
        }
        i += 1;
    }
    Ok(out)
}

struct Sp<'a> {
    t: Vec<S>,
    i: usize,
    ctx: &'a ParseCtx,
}

fn row_text(r: &Row) -> Option<String> {
    let mut s = String::new();
    for it in r {
        match it {
            Item::Char(c) if c.is_alphanumeric() => s.push(*c),
            _ => return None,
        }
    }
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn is_minus_one(r: &Row) -> bool {
    matches!(r.as_slice(), [Item::Char('−' | '-'), Item::Char('1')])
}

impl<'a> Sp<'a> {
    fn sub(&self, toks: Vec<S>) -> Sp<'a> {
        Sp { t: toks, i: 0, ctx: self.ctx }
    }

    fn peek(&self) -> Option<&S> {
        self.t.get(self.i)
    }

    fn row_expr(&self, r: &[Item]) -> Result<Expr, String> {
        let mut p = self.sub(tokenize(r)?);
        let e = p.expr(0)?;
        if p.i < p.t.len() {
            return Err(format!("unexpected {}", describe(&p.t[p.i])));
        }
        Ok(e)
    }

    fn row_args(&self, r: &[Item]) -> Result<Vec<Expr>, String> {
        let toks = tokenize(r)?;
        if toks.is_empty() {
            return Ok(vec![]);
        }
        let mut args = Vec::new();
        let mut cur: Vec<S> = Vec::new();
        for t in toks {
            if matches!(t, S::Comma) {
                args.push(std::mem::take(&mut cur));
            } else {
                cur.push(t);
            }
        }
        args.push(cur);
        args.into_iter()
            .map(|a| {
                let mut p = self.sub(a);
                if p.t.is_empty() {
                    return Err("missing argument".to_string());
                }
                let e = p.expr(0)?;
                if p.i < p.t.len() {
                    return Err(format!("unexpected {}", describe(&p.t[p.i])));
                }
                Ok(e)
            })
            .collect()
    }

    fn infix(&self) -> Option<(u8, u8, &'static str)> {
        match self.peek()? {
            S::Op(o) => Some(match *o {
                ":" | ":=" => (180, 20, *o),
                "." => (130, 130, *o),
                "*" | "/" => (120, 120, *o),
                "+" | "-" => (100, 100, *o),
                "=" | "#" | "<" | ">" | "<=" | ">=" => (80, 80, *o),
                "and" => (65, 65, *o),
                "or" => (60, 60, *o),
                _ => return None,
            }),
            _ => None,
        }
    }

    fn starts_atom(&self) -> bool {
        matches!(
            self.peek(),
            Some(S::Num(_) | S::Sym(_) | S::Func(_) | S::Str(_) | S::Group(..) | S::Frac(..) | S::Sqrt(..) | S::Big(..) | S::Matrix(_))
        )
    }

    fn expr(&mut self, min: u8) -> Result<Expr, String> {
        let mut lhs = self.prefix()?;
        loop {
            if let Some((l, r, op)) = self.infix() {
                if l <= min {
                    break;
                }
                self.i += 1;
                let rhs = self.expr(r)?;
                lhs = match op {
                    "+" => match lhs {
                        Expr::Add(mut v) => {
                            v.push(rhs);
                            Expr::Add(v)
                        }
                        l => Expr::Add(vec![l, rhs]),
                    },
                    "-" => match lhs {
                        Expr::Add(mut v) => {
                            v.push(Expr::Neg(Box::new(rhs)));
                            Expr::Add(v)
                        }
                        l => Expr::Add(vec![l, Expr::Neg(Box::new(rhs))]),
                    },
                    "*" => match lhs {
                        Expr::Mul(mut v) => {
                            v.push(rhs);
                            Expr::Mul(v)
                        }
                        l => Expr::Mul(vec![l, rhs]),
                    },
                    "/" => Expr::Div(Box::new(lhs), Box::new(rhs)),
                    o => Expr::Bin(o.to_string(), Box::new(lhs), Box::new(rhs)),
                };
            } else if self.starts_atom() && 120 > min {
                let rhs = self.postfix_atom()?;
                lhs = match lhs {
                    Expr::Mul(mut v) => {
                        v.push(rhs);
                        Expr::Mul(v)
                    }
                    l => Expr::Mul(vec![l, rhs]),
                };
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn prefix(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(S::Op("-")) => {
                self.i += 1;
                Ok(Expr::Neg(Box::new(self.expr(100)?)))
            }
            Some(S::Op("+")) => {
                self.i += 1;
                self.expr(100)
            }
            Some(S::Op("not")) => {
                self.i += 1;
                Ok(Expr::Not(Box::new(self.expr(70)?)))
            }
            Some(_) => self.postfix_atom(),
            None => Err("expression is incomplete".into()),
        }
    }

    fn postfix_atom(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Some(S::Sup(r)) => {
                    let r = r.clone();
                    self.i += 1;
                    e = Expr::Pow(Box::new(e), Box::new(self.row_expr(&r)?));
                }
                Some(S::Op("!")) => {
                    self.i += 1;
                    e = Expr::Fact(Box::new(e));
                }
                Some(S::Deg) => {
                    self.i += 1;
                    e = Expr::Mul(vec![e, Expr::Div(Box::new(Expr::sym("%pi")), Box::new(Expr::num(180)))]);
                }
                _ => break,
            }
        }
        Ok(e)
    }

    /// Operand of a function applied without parentheses: a run of tightly bound atoms.
    fn tight_operand(&mut self) -> Result<Expr, String> {
        if matches!(self.peek(), Some(S::Op("-"))) {
            self.i += 1;
            return Ok(Expr::Neg(Box::new(self.tight_operand()?)));
        }
        if !matches!(self.peek(), Some(S::Num(_) | S::Sym(_) | S::Group(..) | S::Frac(..) | S::Sqrt(..) | S::Str(_) | S::Matrix(_) | S::Func(_))) {
            return Err("function is missing its argument".into());
        }
        let mut f = vec![self.postfix_atom()?];
        while matches!(self.peek(), Some(S::Num(_) | S::Sym(_) | S::Group(..) | S::Frac(..) | S::Sqrt(..) | S::Str(_) | S::Matrix(_))) {
            // A following `d` + variable would be a differential; stop before it.
            if matches!(self.peek(), Some(S::Sym(d)) if d == "d") && matches!(self.t.get(self.i + 1), Some(S::Sym(_))) {
                break;
            }
            f.push(self.postfix_atom()?);
        }
        Ok(if f.len() == 1 { f.pop().unwrap() } else { Expr::Mul(f) })
    }

    fn name_with_sub(&mut self, name: String) -> Result<Expr, String> {
        if let Some(S::Sub(r)) = self.peek() {
            let r = r.clone();
            self.i += 1;
            if let Some(txt) = row_text(&r) {
                return Ok(Expr::Sym(format!("{}_{}", name, txt)));
            }
            let idx = self.row_args(&r)?;
            return Ok(Expr::Index(Box::new(Expr::Sym(map_symbol(&name, true))), idx));
        }
        Ok(Expr::Sym(map_symbol(&name, false)))
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let Some(t) = self.peek().cloned() else {
            return Err("expression is incomplete".into());
        };
        self.i += 1;
        match t {
            S::Num(n) => Ok(Expr::Num(n)),
            S::Str(s) => Ok(Expr::Str(s)),
            S::Sym(name) => {
                if self.ctx.functions.contains(&name) {
                    if let Some(S::Group('(', _, body)) = self.peek().cloned() {
                        self.i += 1;
                        return Ok(Expr::Call(name, self.row_args(&body)?));
                    }
                }
                self.name_with_sub(name)
            }
            S::Func(f) => self.func(f),
            S::Group(o, _, body) => match o {
                '(' => {
                    let mut v = self.row_args(&body)?;
                    if v.len() == 1 {
                        Ok(v.pop().unwrap())
                    } else {
                        Err("parentheses need exactly one expression (use a function name for argument lists)".into())
                    }
                }
                '[' => Ok(Expr::List(self.row_args(&body)?)),
                '{' => Ok(Expr::Set(self.row_args(&body)?)),
                '|' => Ok(Expr::call("abs", vec![self.row_expr(&body)?])),
                _ => Err("unsupported bracket".into()),
            },
            S::Frac(n, d) => self.frac(n, d),
            S::Sqrt(idx, body) => {
                let b = self.row_expr(&body)?;
                match idx {
                    None => Ok(Expr::call("sqrt", vec![b])),
                    Some(i) => Ok(Expr::Pow(Box::new(b), Box::new(Expr::Div(Box::new(Expr::num(1)), Box::new(self.row_expr(&i)?))))),
                }
            }
            S::Big(op, lo, hi) => self.big(op, lo, hi),
            S::Matrix(rows) => {
                let mut out = Vec::new();
                for r in rows {
                    out.push(r.iter().map(|c| self.row_expr(c)).collect::<Result<Vec<_>, _>>()?);
                }
                Ok(Expr::Matrix(out))
            }
            other => Err(format!("unexpected {}", describe(&other))),
        }
    }

    fn func(&mut self, raw: String) -> Result<Expr, String> {
        let mapped = match latex_func_to_maxima(&raw) {
            "ln" => "log".to_string(),
            n => n.to_string(),
        };
        let mut base = None;
        if let Some(S::Sub(r)) = self.peek().cloned() {
            self.i += 1;
            base = Some(self.row_expr(&r)?);
        }
        let mut power = None;
        if let Some(S::Sup(r)) = self.peek().cloned() {
            self.i += 1;
            power = Some(r);
        }
        let mut name = mapped.clone();
        if let Some(p) = &power {
            if is_minus_one(p)
                && matches!(mapped.as_str(), "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "sinh" | "cosh" | "tanh" | "coth")
            {
                name = format!("a{}", mapped);
                power = None;
            }
        }
        let args = if let Some(S::Group('(', _, body)) = self.peek().cloned() {
            self.i += 1;
            self.row_args(&body)?
        } else if TRIG_LIKE.contains(&raw.as_str()) || TRIG_LIKE.contains(&mapped.as_str()) {
            vec![self.tight_operand()?]
        } else if is_known_func(&mapped) {
            return Err(format!("{} needs its arguments in parentheses", raw));
        } else {
            let sym = if raw.chars().all(|c| c.is_ascii_alphabetic()) && !is_known_func(&mapped) && raw.chars().count() > 1 {
                Expr::Mul(raw.chars().map(|c| Expr::Sym(map_symbol(&c.to_string(), false))).collect())
            } else {
                Expr::Sym(raw)
            };
            return match power {
                Some(p) => Ok(Expr::Pow(Box::new(sym), Box::new(self.row_expr(&p)?))),
                None => Ok(sym),
            };
        };
        let mut call = if let (Some(b), true) = (&base, name == "log") {
            Expr::Div(Box::new(Expr::Call("log".into(), args)), Box::new(Expr::call("log", vec![b.clone()])))
        } else {
            Expr::Call(name, args)
        };
        if let Some(p) = power {
            call = Expr::Pow(Box::new(call), Box::new(self.row_expr(&p)?));
        }
        Ok(call)
    }

    fn frac(&mut self, n: Row, d: Row) -> Result<Expr, String> {
        if let Some((var, order, rest)) = deriv_pattern(&n, &d) {
            let var = self.row_expr(&var)?;
            let body = if rest.is_empty() {
                if self.peek().is_none() {
                    return Err("derivative operator is missing its operand".into());
                }
                self.expr(100)?
            } else {
                self.row_expr(&rest)?
            };
            let mut args = vec![body, var];
            if let Some(o) = order {
                args.push(self.row_expr(&o)?);
            }
            return Ok(Expr::Call("diff".into(), args));
        }
        Ok(Expr::Div(Box::new(self.row_expr(&n)?), Box::new(self.row_expr(&d)?)))
    }

    fn big(&mut self, op: BigOp, lo: Row, hi: Row) -> Result<Expr, String> {
        match op {
            BigOp::Int | BigOp::IntNoLimits => {
                let start = self.i;
                let mut body_toks: Vec<S> = Vec::new();
                // \int \frac{dx}{f}
                if let Some(S::Frac(n, d)) = self.t.get(start).cloned() {
                    if let [Item::Char('d'), Item::Char(v)] = n.as_slice() {
                        let v = *v;
                        let var = self.row_expr(&vec![Item::Char(v)])?;
                        let den = self.row_expr(&d)?;
                        self.i = start + 1;
                        let body = Expr::Div(Box::new(Expr::num(1)), Box::new(den));
                        return self.finish_int(body, var, lo, hi);
                    }
                }
                let mut k = None;
                for j in start..self.t.len().saturating_sub(1) {
                    if matches!(&self.t[j], S::Sym(d) if d == "d") && matches!(self.t[j + 1], S::Sym(_)) {
                        k = Some(j);
                        break;
                    }
                }
                let Some(k) = k else {
                    return Err("integral is missing its differential (for example dx)".into());
                };
                body_toks.extend(self.t[start..k].iter().cloned());
                let mut vi = k + 1;
                let S::Sym(vname) = self.t[vi].clone() else { unreachable!() };
                vi += 1;
                let mut sub = self.sub(vec![S::Sym(vname)]);
                if let Some(S::Sub(r)) = self.t.get(vi).cloned() {
                    sub.t.push(S::Sub(r));
                    vi += 1;
                }
                let v = sub.expr(0)?;
                self.i = vi;
                let body = if body_toks.is_empty() {
                    Expr::num(1)
                } else {
                    let mut p = self.sub(body_toks);
                    let e = p.expr(0)?;
                    if p.i < p.t.len() {
                        return Err(format!("unexpected {} in integrand", describe(&p.t[p.i])));
                    }
                    e
                };
                self.finish_int(body, v, lo, hi)
            }
            BigOp::Sum | BigOp::Prod => {
                let name = if op == BigOp::Sum { "sum" } else { "product" };
                let l = self.row_expr(&lo)?;
                let Expr::Bin(eq, v, a) = l else {
                    return Err(format!("the lower limit of a {} must look like n=1", name));
                };
                if eq != "=" {
                    return Err(format!("the lower limit of a {} must look like n=1", name));
                }
                if hi.is_empty() {
                    return Err(format!("{} needs an upper limit", name));
                }
                let b = self.row_expr(&hi)?;
                let body = self.expr(100)?;
                Ok(Expr::Call(name.into(), vec![body, *v, *a, b]))
            }
            BigOp::Lim => {
                let pos = lo.iter().position(|i| matches!(i, Item::Char('→')));
                let Some(pos) = pos else {
                    return Err("limit needs the form x→a below lim".into());
                };
                let var = self.row_expr(&lo[..pos])?;
                let mut target = lo[pos + 1..].to_vec();
                let mut dir = None;
                if let Some(Item::Sup(s)) = target.last() {
                    match s.as_slice() {
                        [Item::Char('+')] => dir = Some("plus"),
                        [Item::Char('−' | '-')] => dir = Some("minus"),
                        _ => {}
                    }
                    if dir.is_some() {
                        target.pop();
                    }
                }
                let tgt = self.row_expr(&target)?;
                let body = self.expr(100)?;
                let mut args = vec![body, var, tgt];
                if let Some(d) = dir {
                    args.push(Expr::sym(d));
                }
                Ok(Expr::Call("limit".into(), args))
            }
        }
    }

    fn finish_int(&mut self, body: Expr, v: Expr, lo: Row, hi: Row) -> Result<Expr, String> {
        let mut args = vec![body, v];
        match (lo.is_empty(), hi.is_empty()) {
            (true, true) => {}
            (false, false) => {
                args.push(self.row_expr(&lo)?);
                args.push(self.row_expr(&hi)?);
            }
            _ => return Err("an integral needs both limits or none".into()),
        }
        Ok(Expr::Call("integrate".into(), args))
    }
}

fn map_symbol(name: &str, _indexed: bool) -> String {
    match name {
        "e" => "%e".into(),
        "pi" => "%pi".into(),
        n => n.to_string(),
    }
}

fn describe(t: &S) -> String {
    match t {
        S::Num(n) => format!("number {}", n),
        S::Sym(s) | S::Func(s) => format!("'{}'", s),
        S::Op(o) => format!("operator '{}'", o),
        S::Comma => "','".into(),
        S::Semi(c) => format!("'{}'", c),
        S::Str(_) => "text".into(),
        S::Group(..) => "bracket group".into(),
        S::Sup(_) => "superscript".into(),
        S::Sub(_) => "subscript".into(),
        _ => "expression".into(),
    }
}

/// Recognises d/dx, d^n/dx^n, ∂/∂x and the `d f / d x` forms. Returns (variable row, order, body row).
fn deriv_pattern(n: &Row, d: &Row) -> Option<(Row, Option<Row>, Row)> {
    let dch = match n.first() {
        Some(Item::Char(c @ ('d' | '∂'))) => *c,
        _ => return None,
    };
    if !matches!(d.first(), Some(Item::Char(c)) if *c == dch) {
        return None;
    }
    let mut k = 1;
    let mut order = None;
    if let Some(Item::Sup(o)) = n.get(k) {
        order = Some(o.clone());
        k += 1;
    }
    let rest: Row = n[k..].to_vec();
    // variable: Char (+ optional Sub), then optional Sup
    let var_item = d.get(1)?;
    let Item::Char(vc) = var_item else { return None };
    if !vc.is_alphabetic() {
        return None;
    }
    let mut var = vec![var_item.clone()];
    let mut j = 2;
    if let Some(Item::Sub(s)) = d.get(j) {
        var.push(Item::Sub(s.clone()));
        j += 1;
    }
    if let Some(Item::Sup(o)) = d.get(j) {
        if order.is_none() {
            order = Some(o.clone());
        }
        j += 1;
    }
    if j != d.len() {
        return None;
    }
    Some((var, order, rest))
}

fn scan_function_defs(toks: &[S], ctx: &mut ParseCtx) {
    if let (Some(S::Sym(n)), Some(S::Group('(', ..)), Some(S::Op(":="))) = (toks.first(), toks.get(1), toks.get(2)) {
        ctx.functions.insert(n.clone());
    }
}

/// Parses a textbook tree into one or more statements.
pub fn parse_tree(row: &[Item], ctx: &ParseCtx) -> Result<Vec<Stmt>, String> {
    let toks = tokenize(row)?;
    let mut segs: Vec<(Vec<S>, char)> = Vec::new();
    let mut cur = Vec::new();
    for t in toks {
        if let S::Semi(c) = t {
            segs.push((std::mem::take(&mut cur), c));
        } else {
            cur.push(t);
        }
    }
    if !cur.is_empty() {
        segs.push((cur, ';'));
    }
    let mut local = ctx.clone();
    for (s, _) in &segs {
        scan_function_defs(s, &mut local);
    }
    let mut out = Vec::new();
    for (s, term) in segs {
        if s.is_empty() {
            continue;
        }
        let mut p = Sp { t: s, i: 0, ctx: &local };
        let e = p.expr(0)?;
        if p.i < p.t.len() {
            return Err(format!("unexpected {}", describe(&p.t[p.i])));
        }
        out.push(Stmt { expr: e, display: term == ';' });
    }
    if out.is_empty() {
        return Err("nothing to evaluate".into());
    }
    Ok(out)
}
