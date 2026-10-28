//! Parser for Maxima's linear (1D) syntax, as produced by `string()` and typed by users.

use crate::expr::Expr;

#[derive(Clone, Debug, PartialEq)]
enum T {
    Num(String),
    Id(String),
    Str(String),
    Op(String),
    Eof,
}

fn lex(src: &str) -> Result<Vec<T>, String> {
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).map_or(false, |d| d.is_ascii_digit())) {
            let s = i;
            while i < c.len() && c[i].is_ascii_digit() {
                i += 1;
            }
            if i < c.len() && c[i] == '.' && c.get(i + 1).map_or(true, |d| d.is_ascii_digit()) {
                i += 1;
                while i < c.len() && c[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < c.len() && matches!(c[i], 'e' | 'E' | 'd' | 'D' | 'b' | 'B') {
                let mut j = i + 1;
                if j < c.len() && matches!(c[j], '+' | '-') {
                    j += 1;
                }
                if j < c.len() && c[j].is_ascii_digit() {
                    while j < c.len() && c[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                }
            }
            out.push(T::Num(c[s..i].iter().collect()));
        } else if ch.is_alphabetic() || ch == '_' || ch == '%' || ch == '?' || ch == '\\' {
            let s = i;
            i += 1;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '%' || c[i] == '\\') {
                if c[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            let id: String = c[s..i.min(c.len())].iter().filter(|x| **x != '\\').collect();
            out.push(T::Id(id));
        } else if ch == '"' {
            i += 1;
            let mut s = String::new();
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' && i + 1 < c.len() {
                    i += 1;
                }
                s.push(c[i]);
                i += 1;
            }
            if i >= c.len() {
                return Err("unterminated string".into());
            }
            i += 1;
            out.push(T::Str(s));
        } else {
            let three: String = c[i..(i + 3).min(c.len())].iter().collect();
            let two: String = c[i..(i + 2).min(c.len())].iter().collect();
            if three == "::=" {
                out.push(T::Op(three));
                i += 3;
            } else if ["<=", ">=", ":=", "::", "**", "!!", "^^", "-\u{3e}", "#="].contains(&two.as_str()) && two != "#=" {
                out.push(T::Op(if two == "**" { "^".into() } else { two }));
                i += 2;
            } else if "+-*/^=<>:()[]{},;$#!.'|".contains(ch) {
                out.push(T::Op(ch.to_string()));
                i += 1;
            } else {
                return Err(format!("unexpected character '{}'", ch));
            }
        }
    }
    out.push(T::Eof);
    Ok(out)
}

struct P {
    t: Vec<T>,
    i: usize,
}

impl P {
    fn peek(&self) -> &T {
        &self.t[self.i.min(self.t.len() - 1)]
    }
    fn bump(&mut self) -> T {
        let t = self.peek().clone();
        self.i += 1;
        t
    }
    fn is_op(&self, s: &str) -> bool {
        matches!(self.peek(), T::Op(o) if o == s)
    }
    fn expect(&mut self, s: &str) -> Result<(), String> {
        if self.is_op(s) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}'", s))
        }
    }

    fn infix_bp(&self) -> Option<(u8, u8, String)> {
        let name = match self.peek() {
            T::Op(o) => o.clone(),
            T::Id(i) if i == "and" || i == "or" => i.clone(),
            _ => return None,
        };
        let (l, r) = match name.as_str() {
            ":" | ":=" | "::" | "::=" => (180, 20),
            "^" | "^^" => (140, 139),
            "." => (130, 130),
            "*" | "/" => (120, 120),
            "+" | "-" => (100, 100),
            "=" | "#" | "<" | ">" | "<=" | ">=" => (80, 80),
            "and" => (65, 65),
            "or" => (60, 60),
            _ => return None,
        };
        Some((l, r, name))
    }

    fn expr(&mut self, min: u8) -> Result<Expr, String> {
        let mut lhs = self.prefix()?;
        loop {
            if self.is_op("!") && 160 > min {
                self.i += 1;
                lhs = Expr::Fact(Box::new(lhs));
                continue;
            }
            let Some((l, r, op)) = self.infix_bp() else { break };
            if l <= min {
                break;
            }
            self.i += 1;
            let rhs = self.expr(r)?;
            lhs = match op.as_str() {
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
                "^" => Expr::Pow(Box::new(lhs), Box::new(rhs)),
                o => Expr::Bin(o.to_string(), Box::new(lhs), Box::new(rhs)),
            };
        }
        Ok(lhs)
    }

    fn prefix(&mut self) -> Result<Expr, String> {
        match self.bump() {
            T::Num(n) => Ok(Expr::Num(n)),
            T::Str(s) => Ok(Expr::Str(s)),
            T::Id(id) => {
                if id == "not" {
                    return Ok(Expr::Not(Box::new(self.expr(70)?)));
                }
                if self.is_op("(") {
                    self.i += 1;
                    let args = self.args(")")?;
                    return Ok(match (id.as_str(), &args[..]) {
                        ("matrix", rows) if !rows.is_empty() && rows.iter().all(|r| matches!(r, Expr::List(_))) => Expr::Matrix(
                            rows.iter()
                                .map(|r| match r {
                                    Expr::List(v) => v.clone(),
                                    _ => unreachable!(),
                                })
                                .collect(),
                        ),
                        _ => Expr::Call(id, args),
                    });
                }
                let mut e = Expr::Sym(id);
                while self.is_op("[") {
                    self.i += 1;
                    let idx = self.args("]")?;
                    e = Expr::Index(Box::new(e), idx);
                }
                Ok(e)
            }
            T::Op(o) => match o.as_str() {
                "-" => Ok(Expr::Neg(Box::new(self.expr(100)?))),
                "+" => self.expr(100),
                "'" => self.prefix(),
                "(" => {
                    let mut v = self.args(")")?;
                    if v.len() == 1 {
                        Ok(v.pop().unwrap())
                    } else {
                        Ok(Expr::Call("block".into(), v))
                    }
                }
                "[" => Ok(Expr::List(self.args("]")?)),
                "{" => Ok(Expr::Set(self.args("}")?)),
                o => Err(format!("unexpected '{}'", o)),
            },
            T::Eof => Err("unexpected end of input".into()),
        }
    }

    fn args(&mut self, close: &str) -> Result<Vec<Expr>, String> {
        let mut v = Vec::new();
        if self.is_op(close) {
            self.i += 1;
            return Ok(v);
        }
        loop {
            v.push(self.expr(0)?);
            if self.is_op(",") {
                self.i += 1;
            } else {
                self.expect(close)?;
                return Ok(v);
            }
        }
    }
}

/// Splits a Maxima script into `(statement, terminator)` pairs at top-level `;` and `$`.
/// Comments are dropped; a missing final terminator yields `;`.
pub fn split_statements(src: &str) -> Vec<(String, char)> {
    let c: Vec<char> = src.chars().collect();
    let (mut out, mut cur, mut depth, mut i) = (Vec::new(), String::new(), 0i32, 0);
    while i < c.len() {
        match c[i] {
            '/' if c.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                cur.push('"');
                i += 1;
                while i < c.len() && c[i] != '"' {
                    if c[i] == '\\' && i + 1 < c.len() {
                        cur.push(c[i]);
                        i += 1;
                    }
                    cur.push(c[i]);
                    i += 1;
                }
                cur.push('"');
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c[i]);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                cur.push(c[i]);
            }
            t @ (';' | '$') if depth <= 0 => {
                if !cur.trim().is_empty() {
                    out.push((std::mem::take(&mut cur).trim().to_string(), t));
                }
                cur.clear();
            }
            ch => cur.push(ch),
        }
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push((cur.trim().to_string(), ';'));
    }
    out
}

/// Parses a single Maxima expression (no trailing terminator required).
pub fn parse_maxima(src: &str) -> Result<Expr, String> {
    let mut p = P { t: lex(src)?, i: 0 };
    let e = p.expr(0)?;
    while p.is_op(";") || p.is_op("$") {
        p.i += 1;
    }
    if *p.peek() != T::Eof {
        return Err(format!("unexpected trailing input near {:?}", p.peek()));
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::to_maxima;

    fn rt(s: &str) -> String {
        to_maxima(&parse_maxima(s).unwrap())
    }

    #[test]
    fn roundtrips() {
        assert_eq!(rt("a - (b+c)"), "a-(b+c)");
        assert_eq!(rt("2*%pi/3"), "2*%pi/3");
        assert_eq!(rt("x^2^3"), "x^2^3");
        assert_eq!(rt("(x^2)^3"), "(x^2)^3");
        assert_eq!(rt("[x = -1, x = 1]"), "[x=-1,x=1]");
        assert_eq!(rt("'integrate(x*sin(x),x,0,%pi)"), "integrate(x*sin(x),x,0,%pi)");
        assert_eq!(rt("matrix([1,2],[3,4])"), "matrix([1,2],[3,4])");
        assert_eq!(rt("1.0E-5*x"), "1.0E-5*x");
        assert_eq!(rt("-a^2"), "-a^2");
    }
}
