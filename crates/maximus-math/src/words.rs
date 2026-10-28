//! Word tables shared by the parser, the editor and the renderer.

/// (maxima/ascii name, glyph, latex command)
pub const GREEK: &[(&str, char, &str)] = &[
    ("alpha", 'α', "alpha"),
    ("beta", 'β', "beta"),
    ("gamma", 'γ', "gamma"),
    ("delta", 'δ', "delta"),
    ("epsilon", 'ε', "epsilon"),
    ("zeta", 'ζ', "zeta"),
    ("eta", 'η', "eta"),
    ("theta", 'θ', "theta"),
    ("iota", 'ι', "iota"),
    ("kappa", 'κ', "kappa"),
    ("lambda", 'λ', "lambda"),
    ("mu", 'μ', "mu"),
    ("nu", 'ν', "nu"),
    ("xi", 'ξ', "xi"),
    ("pi", 'π', "pi"),
    ("rho", 'ρ', "rho"),
    ("sigma", 'σ', "sigma"),
    ("tau", 'τ', "tau"),
    ("upsilon", 'υ', "upsilon"),
    ("phi", 'φ', "phi"),
    ("chi", 'χ', "chi"),
    ("psi", 'ψ', "psi"),
    ("omega", 'ω', "omega"),
    ("Gamma", 'Γ', "Gamma"),
    ("Delta", 'Δ', "Delta"),
    ("Theta", 'Θ', "Theta"),
    ("Lambda", 'Λ', "Lambda"),
    ("Xi", 'Ξ', "Xi"),
    ("Pi", 'Π', "Pi"),
    ("Sigma", 'Σ', "Sigma"),
    ("Phi", 'Φ', "Phi"),
    ("Psi", 'Ψ', "Psi"),
    ("Omega", 'Ω', "Omega"),
];

/// Function words that are recognised inside a run of typed letters.
pub const FUNC_WORDS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "asin", "acos", "atan", "acot", "asec", "acsc", "sinh",
    "cosh", "tanh", "coth", "sech", "csch", "asinh", "acosh", "atanh", "arcsin", "arccos", "arctan",
    "ln", "log", "exp", "sqrt", "abs", "min", "max", "gcd", "lcm", "mod", "floor", "ceiling", "round",
    "signum", "det", "rank", "diff", "integrate", "limit", "solve", "expand", "factor", "simplify",
    "ratsimp", "taylor", "trace", "transpose", "invert", "binomial", "gamma_incomplete", "erf",
    "sum", "product", "float", "realpart", "imagpart", "conjugate", "cabs", "carg",
];

/// Functions that take an argument without needing parentheses (`\sin x`).
pub const TRIG_LIKE: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "asin", "acos", "atan", "acot", "asec", "acsc", "sinh",
    "cosh", "tanh", "coth", "sech", "csch", "asinh", "acosh", "atanh", "arcsin", "arccos", "arctan",
    "ln", "log", "exp", "erf", "det", "abs", "sqrt",
];

pub fn is_known_func(name: &str) -> bool {
    FUNC_WORDS.contains(&name)
}

/// Maps LaTeX function command names to the maxima name.
pub fn latex_func_to_maxima(name: &str) -> &str {
    match name {
        "arcsin" => "asin",
        "arccos" => "acos",
        "arctan" => "atan",
        "arccot" => "acot",
        "arcsec" => "asec",
        "arccsc" => "acsc",
        "arcsinh" => "asinh",
        "arccosh" => "acosh",
        "arctanh" => "atanh",
        "max" | "min" | "gcd" => name,
        n => n,
    }
}

pub fn maxima_func_to_latex(name: &str) -> Option<&'static str> {
    Some(match name {
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "cot" => "cot",
        "sec" => "sec",
        "csc" => "csc",
        "asin" => "arcsin",
        "acos" => "arccos",
        "atan" => "arctan",
        "sinh" => "sinh",
        "cosh" => "cosh",
        "tanh" => "tanh",
        "coth" => "coth",
        "ln" => "ln",
        "log" => "ln",
        "exp" => "exp",
        "min" => "min",
        "max" => "max",
        "gcd" => "gcd",
        "det" => "det",
        _ => return None,
    })
}

pub fn greek_glyph(name: &str) -> Option<char> {
    GREEK.iter().find(|(n, _, _)| *n == name).map(|(_, g, _)| *g)
}

pub fn greek_name_of_glyph(c: char) -> Option<&'static str> {
    GREEK.iter().find(|(_, g, _)| *g == c).map(|(n, _, _)| *n)
}

pub fn greek_latex_to_glyph(cmd: &str) -> Option<char> {
    GREEK.iter().find(|(_, _, l)| *l == cmd).map(|(_, g, _)| *g)
}

pub fn glyph_to_greek_latex(c: char) -> Option<&'static str> {
    GREEK.iter().find(|(_, g, _)| *g == c).map(|(_, _, l)| *l)
}

use std::collections::HashSet;
use std::sync::{OnceLock, RwLock};

fn registry() -> &'static RwLock<HashSet<String>> {
    static R: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();
    R.get_or_init(|| RwLock::new(HashSet::new()))
}

/// Registers additional function names (e.g. every function in the Maxima manual) so that
/// typed runs of letters such as `plot2d` or `ratsimp` are recognised as one word.
pub fn register_functions<I: IntoIterator<Item = String>>(names: I) {
    let mut r = registry().write().unwrap();
    for n in names {
        if n.chars().count() >= 2 && n.chars().all(|c| c.is_ascii_alphanumeric()) && n.starts_with(|c: char| c.is_ascii_alphabetic()) {
            r.insert(n);
        }
    }
}

pub fn is_registered(name: &str) -> bool {
    registry().read().unwrap().contains(name)
}

/// A token produced by splitting a run of typed letters and digits.
#[derive(Clone, Debug, PartialEq)]
pub enum Word {
    Func(String),
    Greek(char),
    Inf,
    Letter(char),
    Num(String),
}

/// Splits a run of ascii letters/digits into words, preferring the longest match at each position.
pub fn split_letters(run: &[char]) -> Vec<(usize, usize, Word)> {
    split_letters_ctx(run, false)
}

/// Like [`split_letters`]; `paren_after` says the run is directly followed by `(`.
/// Registered names shorter than four letters (`ilt`, `ev`) only count as functions when
/// they end the run and a parenthesis follows, so ordinary products like `at` stay products.
pub fn split_letters_ctx(run: &[char], paren_after: bool) -> Vec<(usize, usize, Word)> {
    let reg = registry().read().unwrap();
    let mut out = Vec::new();
    let mut i = 0;
    while i < run.len() {
        if run[i].is_ascii_digit() {
            let mut j = i;
            while j < run.len() && run[j].is_ascii_digit() {
                j += 1;
            }
            out.push((i, j, Word::Num(run[i..j].iter().collect())));
            i = j;
            continue;
        }
        let mut best: Option<(usize, Word)> = None;
        let rest: String = run[i..].iter().collect();
        for w in FUNC_WORDS {
            if rest.starts_with(w) && best.as_ref().map_or(true, |(l, _)| w.len() > *l) {
                best = Some((w.len(), Word::Func((*w).to_string())));
            }
        }
        let max = rest.len().min(40);
        for len in (2..=max).rev() {
            if !rest.is_char_boundary(len) || !reg.contains(&rest[..len]) {
                continue;
            }
            if len < 4 && !(paren_after && len == rest.len()) {
                continue;
            }
            if best.as_ref().map_or(true, |(l, _)| len > *l) {
                best = Some((len, Word::Func(rest[..len].to_string())));
            }
            break;
        }
        for (n, g, _) in GREEK {
            if rest.starts_with(n) && best.as_ref().map_or(true, |(l, _)| n.len() > *l) {
                best = Some((n.len(), Word::Greek(*g)));
            }
        }
        if rest.starts_with("inf") && best.as_ref().map_or(true, |(l, _)| 3 > *l) {
            best = Some((3, Word::Inf));
        }
        match best {
            Some((len, w)) => {
                out.push((i, i + len, w));
                i += len;
            }
            None => {
                out.push((i, i + 1, Word::Letter(run[i])));
                i += 1;
            }
        }
    }
    out
}

/// Collects the maximal run of ascii letters/digits starting at `start` in `row` (a letter must start it).
/// A trailing digit group that is the integer part of a decimal number is left out.
pub fn collect_run(row: &[crate::tree::Item], start: usize) -> (Vec<char>, usize, bool) {
    use crate::tree::Item;
    let mut run = Vec::new();
    let mut j = start;
    while let Some(Item::Char(d)) = row.get(j) {
        if d.is_ascii_alphanumeric() {
            run.push(*d);
            j += 1;
        } else {
            break;
        }
    }
    if matches!(row.get(j), Some(Item::Char('.'))) && matches!(row.get(j + 1), Some(Item::Char(d)) if d.is_ascii_digit()) {
        while matches!(run.last(), Some(c) if c.is_ascii_digit()) {
            run.pop();
            j -= 1;
        }
    }
    let paren = matches!(row.get(j), Some(crate::tree::Item::Group('(', _, _)));
    (run, j, paren)
}
