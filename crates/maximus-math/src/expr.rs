//! Semantic expression tree shared by the Maxima parser/printer and the textbook parser.

use crate::tree::*;
use crate::words::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(String),
    Sym(String),
    Str(String),
    Neg(Box<Expr>),
    Add(Vec<Expr>),
    Mul(Vec<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Index(Box<Expr>, Vec<Expr>),
    /// Binary operators that are not arithmetic: relations, assignment, logic, `.`.
    Bin(String, Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Fact(Box<Expr>),
    List(Vec<Expr>),
    Set(Vec<Expr>),
    Matrix(Vec<Vec<Expr>>),
}

impl Expr {
    pub fn num(n: i64) -> Expr {
        Expr::Num(n.to_string())
    }
    pub fn sym(s: &str) -> Expr {
        Expr::Sym(s.to_string())
    }
    pub fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call(name.to_string(), args)
    }
}

// ---------------------------------------------------------------- Maxima printing

const P_ASSIGN: u8 = 10;
const P_OR: u8 = 20;
const P_AND: u8 = 30;
const P_NOT: u8 = 35;
const P_REL: u8 = 40;
const P_ADD: u8 = 50;
const P_NEG: u8 = 55;
const P_MUL: u8 = 60;
const P_DOT: u8 = 65;
const P_POW: u8 = 70;
const P_FACT: u8 = 80;
const P_ATOM: u8 = 100;

fn prec(e: &Expr) -> u8 {
    match e {
        Expr::Num(_) | Expr::Sym(_) | Expr::Str(_) | Expr::Call(..) | Expr::Index(..) | Expr::List(_) | Expr::Set(_) | Expr::Matrix(_) => P_ATOM,
        Expr::Neg(_) => P_NEG,
        Expr::Add(_) => P_ADD,
        Expr::Mul(_) | Expr::Div(..) => P_MUL,
        Expr::Pow(..) => P_POW,
        Expr::Fact(_) => P_FACT,
        Expr::Not(_) => P_NOT,
        Expr::Bin(op, ..) => match op.as_str() {
            ":" | ":=" | "::" | "::=" => P_ASSIGN,
            "or" => P_OR,
            "and" => P_AND,
            "." => P_DOT,
            "^^" => P_POW,
            _ => P_REL,
        },
    }
}

#[derive(Clone, Copy, Default)]
pub struct PrintOpts {
    /// Wrap `sum` in `ev(..., simpsum)` so symbolic sums evaluate.
    pub eval_sums: bool,
}

pub fn to_maxima(e: &Expr) -> String {
    to_maxima_with(e, PrintOpts::default())
}

pub fn to_maxima_with(e: &Expr, o: PrintOpts) -> String {
    pm(e, 0, o)
}

fn wrap(s: String, need: bool) -> String {
    if need {
        format!("({})", s)
    } else {
        s
    }
}

fn pm(e: &Expr, min: u8, o: PrintOpts) -> String {
    let p = prec(e);
    let s = match e {
        Expr::Num(n) => n.clone(),
        Expr::Sym(s) => s.clone(),
        Expr::Str(s) => format!("\"{}\"", s.replace('"', "\\\"")),
        Expr::Neg(x) => format!("-{}", pm(x, P_MUL, o)),
        Expr::Add(ts) => {
            let mut out = String::new();
            for (i, t) in ts.iter().enumerate() {
                match t {
                    Expr::Neg(x) => {
                        out.push('-');
                        out.push_str(&pm(x, P_MUL, o));
                    }
                    t => {
                        if i > 0 {
                            out.push('+');
                        }
                        out.push_str(&pm(t, P_ADD + 1, o));
                    }
                }
            }
            out
        }
        Expr::Mul(fs) => fs
            .iter()
            .enumerate()
            .map(|(i, f)| pm(f, if i == 0 { P_MUL } else { P_MUL + 1 }, o))
            .collect::<Vec<_>>()
            .join("*"),
        Expr::Div(a, b) => format!("{}/{}", pm(a, P_MUL, o), pm(b, P_MUL + 1, o)),
        Expr::Pow(a, b) => format!("{}^{}", pm(a, P_POW + 1, o), pm(b, P_POW, o)),
        Expr::Call(n, args) => {
            let inner = format!("{}({})", n, args.iter().map(|a| pm(a, 0, o)).collect::<Vec<_>>().join(","));
            if n == "sum" && o.eval_sums {
                format!("ev({},simpsum)", inner)
            } else {
                inner
            }
        }
        Expr::Index(a, idx) => format!("{}[{}]", pm(a, P_ATOM, o), idx.iter().map(|a| pm(a, 0, o)).collect::<Vec<_>>().join(",")),
        Expr::Bin(op, a, b) => {
            let sp = if op == "and" || op == "or" { " " } else { "" };
            let right_assoc = matches!(op.as_str(), ":" | ":=" | "::" | "::=");
            let (lp, rp) = if right_assoc { (P_ASSIGN + 1, P_ASSIGN) } else { (p + 1, p + 1) };
            format!("{}{sp}{}{sp}{}", pm(a, lp, o), op, pm(b, rp, o))
        }
        Expr::Not(x) => format!("not {}", pm(x, P_NOT, o)),
        Expr::Fact(x) => format!("{}!", pm(x, P_FACT + 1, o)),
        Expr::List(xs) => format!("[{}]", xs.iter().map(|a| pm(a, 0, o)).collect::<Vec<_>>().join(",")),
        Expr::Set(xs) => format!("{{{}}}", xs.iter().map(|a| pm(a, 0, o)).collect::<Vec<_>>().join(",")),
        Expr::Matrix(rows) => format!(
            "matrix({})",
            rows.iter()
                .map(|r| format!("[{}]", r.iter().map(|a| pm(a, 0, o)).collect::<Vec<_>>().join(",")))
                .collect::<Vec<_>>()
                .join(",")
        ),
    };
    wrap(s, p < min)
}

// ---------------------------------------------------------------- Display tree

fn chars(s: &str) -> Row {
    s.chars().map(Item::Char).collect()
}

fn paren(body: Row) -> Item {
    Item::Group('(', ')', body)
}

fn num_row(n: &str) -> Row {
    // 1.5E-5 / 1.5e-5 / 1.5b0 forms.
    if let Some(pos) = n.find(|c| matches!(c, 'E' | 'e' | 'd' | 'D' | 'b' | 'B')) {
        let (m, ex) = n.split_at(pos);
        let ex = &ex[1..];
        let ex = ex.trim_start_matches('+');
        let exv: i32 = ex.parse().unwrap_or(0);
        let mut r = chars(m);
        if exv != 0 {
            r.push(Item::Char('×'));
            r.extend(chars("10"));
            r.push(Item::Sup(chars(&exv.to_string().replace('-', "−"))));
        }
        return r;
    }
    chars(n)
}

pub fn sym_row(s: &str) -> Row {
    let s = s.trim_start_matches('?');
    let bare = s.strip_prefix('%').unwrap_or(s);
    match s {
        "%pi" => return vec![Item::Char('π')],
        "%e" => return vec![Item::Char('ⅇ')],
        "%i" => return vec![Item::Char('ⅈ')],
        "inf" => return vec![Item::Char('∞')],
        "minf" => return vec![Item::Char('−'), Item::Char('∞')],
        _ => {}
    }
    if let Some(g) = greek_glyph(bare) {
        return vec![Item::Char(g)];
    }
    if let Some((base, sub)) = bare.split_once('_') {
        if !base.is_empty() && !sub.is_empty() {
            let mut r = sym_row(base);
            r.push(Item::Sub(chars(sub)));
            return r;
        }
    }
    if bare.chars().count() == 1 {
        return chars(bare);
    }
    vec![Item::Func(bare.to_string())]
}

fn starts_with_digit(e: &Expr) -> bool {
    match e {
        Expr::Num(_) => true,
        Expr::Div(a, _) => starts_with_digit(a),
        Expr::Pow(a, _) => starts_with_digit(a),
        Expr::Mul(f) => f.first().map_or(false, starts_with_digit),
        _ => false,
    }
}

fn disp_name(n: &str) -> String {
    match n {
        "asin" => "arcsin",
        "acos" => "arccos",
        "atan" => "arctan",
        "log" => "ln",
        n => n,
    }
    .to_string()
}

fn join_commas(items: &[Expr]) -> Row {
    let mut r = Vec::new();
    for (i, a) in items.iter().enumerate() {
        if i > 0 {
            r.push(Item::Char(','));
        }
        r.extend(disp(a, 0));
    }
    r
}

fn is_trigish(n: &str) -> bool {
    matches!(n, "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "sinh" | "cosh" | "tanh" | "coth" | "log" | "asin" | "acos" | "atan")
}

/// Builds a display tree for an expression.
pub fn expr_to_tree(e: &Expr) -> Row {
    disp(e, 0)
}

fn disp(e: &Expr, min: u8) -> Row {
    let p = prec(e);
    let row = disp_inner(e);
    if p < min {
        vec![paren(row)]
    } else {
        row
    }
}

fn operand_for_big(e: &Expr) -> Row {
    match e {
        Expr::Add(_) | Expr::Neg(_) => vec![paren(disp(e, 0))],
        _ => disp(e, 0),
    }
}

fn disp_inner(e: &Expr) -> Row {
    match e {
        Expr::Num(n) => num_row(n),
        Expr::Sym(s) => sym_row(s),
        Expr::Str(s) => vec![Item::Text(s.clone())],
        Expr::Neg(x) => {
            let mut r = vec![Item::Char('−')];
            r.extend(disp(x, P_MUL));
            r
        }
        Expr::Add(ts) => {
            let mut r = Vec::new();
            for (i, t) in ts.iter().enumerate() {
                match t {
                    Expr::Neg(x) => {
                        r.push(Item::Char('−'));
                        r.extend(disp(x, P_MUL));
                    }
                    t => {
                        if i > 0 {
                            r.push(Item::Char('+'));
                        }
                        r.extend(disp(t, P_ADD + 1));
                    }
                }
            }
            r
        }
        Expr::Mul(fs) => {
            let mut r = Vec::new();
            for (i, f) in fs.iter().enumerate() {
                if i > 0 {
                    let left = &fs[i - 1];
                    if starts_with_digit(f) || (matches!(left, Expr::Num(_)) && matches!(f, Expr::Div(..))) {
                        r.push(Item::Char('·'));
                    }
                }
                r.extend(disp(f, if i == 0 { P_MUL } else { P_MUL + 1 }));
            }
            r
        }
        Expr::Div(a, b) => vec![Item::Frac(disp(a, 0), disp(b, 0))],
        Expr::Pow(a, b) => {
            if let Expr::Div(n, d) = &**b {
                if **n == Expr::Num("1".into()) {
                    let idx = if **d == Expr::Num("2".into()) { None } else { Some(disp(d, 0)) };
                    return vec![Item::Sqrt(idx, disp(a, 0))];
                }
            }
            if let Expr::Call(n, args) = &**a {
                if is_trigish(n) && args.len() == 1 {
                    let mut r = vec![Item::Func(disp_name(n)), Item::Sup(disp(b, 0))];
                    r.push(paren(disp(&args[0], 0)));
                    return r;
                }
            }
            let mut r = disp(a, P_POW + 1);
            r.push(Item::Sup(disp(b, 0)));
            r
        }
        Expr::Call(n, args) => disp_call(n, args),
        Expr::Index(a, idx) => {
            let mut r = disp(a, P_ATOM);
            r.push(Item::Group('[', ']', join_commas(idx)));
            r
        }
        Expr::Bin(op, a, b) if op == "^^" => {
            let mut r = disp(a, P_POW + 1);
            let mut sup = vec![Item::Char(NC_MARK)];
            sup.extend(disp(b, 0));
            r.push(Item::Sup(sup));
            r
        }
        Expr::Bin(op, a, b) => {
            let pp = prec(e);
            let mut r = disp(a, pp + 1);
            match op.as_str() {
                "#" | "!=" => r.push(Item::Char('≠')),
                "<=" => r.push(Item::Char('≤')),
                ">=" => r.push(Item::Char('≥')),
                ":=" => {
                    r.push(Item::Char(':'));
                    r.push(Item::Char('='));
                }
                "and" | "or" => r.push(Item::Func(op.clone())),
                "." => r.push(Item::Char('·')),
                o => r.extend(chars(o)),
            }
            r.extend(disp(b, pp + 1));
            r
        }
        Expr::Not(x) => {
            let mut r = vec![Item::Func("not".into())];
            r.extend(disp(x, P_NOT));
            r
        }
        Expr::Fact(x) => {
            let mut r = disp(x, P_FACT + 1);
            r.push(Item::Char('!'));
            r
        }
        Expr::List(xs) => vec![Item::Group('[', ']', join_commas(xs))],
        Expr::Set(xs) => vec![Item::Group('{', '}', join_commas(xs))],
        Expr::Matrix(rows) => vec![Item::Matrix(rows.iter().map(|r| r.iter().map(|c| disp(c, 0)).collect()).collect())],
    }
}

fn disp_call(n: &str, args: &[Expr]) -> Row {
    match (n, args.len()) {
        ("sqrt", 1) => return vec![Item::Sqrt(None, disp(&args[0], 0))],
        ("abs", 1) => return vec![Item::Group('|', '|', disp(&args[0], 0))],
        ("integrate", 2) | ("integrate", 4) => {
            let (lo, hi) = if args.len() == 4 { (disp(&args[2], 0), disp(&args[3], 0)) } else { (vec![], vec![]) };
            let op = if args.len() == 4 { BigOp::Int } else { BigOp::IntNoLimits };
            let mut r = vec![Item::Big(op, lo, hi)];
            r.extend(disp(&args[0], 0));
            r.push(Item::Char('d'));
            r.extend(disp(&args[1], P_ATOM));
            return r;
        }
        ("sum", 4) | ("product", 4) => {
            let op = if n == "sum" { BigOp::Sum } else { BigOp::Prod };
            let mut lo = disp(&args[1], 0);
            lo.push(Item::Char('='));
            lo.extend(disp(&args[2], 0));
            let mut r = vec![Item::Big(op, lo, disp(&args[3], 0))];
            r.extend(operand_for_big(&args[0]));
            return r;
        }
        ("limit", 3) | ("limit", 4) => {
            let mut lo = disp(&args[1], 0);
            lo.push(Item::Char('→'));
            lo.extend(disp(&args[2], P_ATOM));
            if let Some(Expr::Sym(d)) = args.get(3) {
                match d.as_str() {
                    "plus" => lo.push(Item::Sup(vec![Item::Char('+')])),
                    "minus" => lo.push(Item::Sup(vec![Item::Char('−')])),
                    _ => {}
                }
            }
            let mut r = vec![Item::Big(BigOp::Lim, lo, vec![])];
            r.extend(operand_for_big(&args[0]));
            return r;
        }
        ("diff", 2) | ("diff", 3) => {
            let order = args.get(2).map(|o| disp(o, 0));
            let mut num = vec![Item::Char('d')];
            let mut den = vec![Item::Char('d')];
            den.extend(disp(&args[1], P_ATOM));
            if let Some(o) = &order {
                num.push(Item::Sup(o.clone()));
                den.push(Item::Sup(o.clone()));
            }
            let mut r = vec![Item::Frac(num, den)];
            r.extend(operand_for_big(&args[0]));
            return r;
        }
        _ => {}
    }
    let mut r = vec![Item::Func(disp_name(n))];
    r.push(paren(join_commas(args)));
    r
}
