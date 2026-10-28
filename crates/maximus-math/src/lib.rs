//! Math model for Maximus: structural tree, LaTeX bridge, textbook and Maxima parsers.
//! Pure Rust with no platform dependencies (WASM friendly).

pub mod expr;
pub mod maxima_parse;
pub mod semantic;
pub mod tree;
pub mod words;

pub use expr::{expr_to_tree, sym_row, to_maxima, to_maxima_with, Expr, PrintOpts};
pub use maxima_parse::{parse_maxima, split_statements};
pub use semantic::{parse_tree, ParseCtx, Stmt};
pub use tree::{closer_of, drop_single_semicolon, NC_MARK, from_latex, row_from_str, row_to_latex, BigOp, Item, Row};

/// Converts a textbook tree to a Maxima script (statements terminated with `;` or `$`).
pub fn tree_to_maxima(row: &[Item], ctx: &ParseCtx) -> Result<String, String> {
    let stmts = parse_tree(row, ctx)?;
    let opts = PrintOpts { eval_sums: true };
    Ok(stmts
        .iter()
        .map(|s| format!("{}{}", to_maxima_with(&s.expr, opts), if s.display { ';' } else { '$' }))
        .collect::<Vec<_>>()
        .join(" "))
}

/// Converts LaTeX math to a Maxima script.
pub fn latex_to_maxima(latex: &str, ctx: &ParseCtx) -> Result<String, String> {
    tree_to_maxima(&from_latex(latex), ctx)
}

/// Converts a Maxima expression string (as printed by `string()`) to a display tree.
pub fn maxima_to_tree(src: &str) -> Result<Row, String> {
    Ok(expr_to_tree(&parse_maxima(src)?))
}

/// Converts a Maxima script (several statements, `;`/`$` terminators, comments) to a display tree.
pub fn maxima_script_to_tree(src: &str) -> Result<Row, String> {
    let mut out: Row = Vec::new();
    for (stmt, term) in split_statements(src) {
        out.extend(maxima_to_tree(&stmt).map_err(|e| format!("{} (in `{}`)", e, stmt))?);
        out.push(Item::Char(term));
    }
    Ok(drop_single_semicolon(out))
}

/// Converts a Maxima script to LaTeX.
pub fn maxima_script_to_latex(src: &str) -> Result<String, String> {
    Ok(row_to_latex(&maxima_script_to_tree(src)?))
}

/// Converts a Maxima expression string to LaTeX.
pub fn maxima_to_latex(src: &str) -> Result<String, String> {
    Ok(row_to_latex(&maxima_to_tree(src)?))
}

/// Converts a tree to a single Maxima expression (the first statement), without wrappers.
pub fn tree_to_plain_maxima(row: &[Item], ctx: &ParseCtx) -> Result<String, String> {
    let stmts = parse_tree(row, ctx)?;
    Ok(to_maxima(&stmts[0].expr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tm(latex: &str) -> String {
        latex_to_maxima(latex, &ParseCtx::default()).unwrap_or_else(|e| format!("ERR {e}"))
    }

    #[test]
    fn integral_examples_match() {
        let a = tm("\\int_0^{\\infty}\\!x\\sin^3\\left(x\\right)\\,\\mathrm{d}x");
        let b = tm("\\text{integrate(}x\\sin^3\\left(x),x,0,\\infty\\right)");
        assert_eq!(a, "integrate(x*sin(x)^3,x,0,inf);", "{a}");
        assert_eq!(a, b);
    }

    #[test]
    fn typed_letters_match() {
        // Typing `integrate(x sin(x)^3, x, 0, inf)` letter by letter.
        let row = vec![
            Item::Func("integrate".into()),
            Item::Group(
                '(',
                ')',
                vec![
                    Item::Char('x'),
                    Item::Char('s'),
                    Item::Char('i'),
                    Item::Char('n'),
                    Item::Sup(vec![Item::Char('3')]),
                    Item::Group('(', ')', vec![Item::Char('x')]),
                    Item::Char(','),
                    Item::Char('x'),
                    Item::Char(','),
                    Item::Char('0'),
                    Item::Char(','),
                    Item::Char('i'),
                    Item::Char('n'),
                    Item::Char('f'),
                ],
            ),
        ];
        assert_eq!(tree_to_maxima(&row, &ParseCtx::default()).unwrap(), "integrate(x*sin(x)^3,x,0,inf);");
    }

    #[test]
    fn basics() {
        assert_eq!(tm("\\frac{1}{2}x^2+3"), "1/2*x^2+3;");
        assert_eq!(tm("\\frac{d}{dx}\\left(x^2\\sin x\\right)"), "diff(x^2*sin(x),x);");
        assert_eq!(tm("\\frac{d^2}{dx^2}x^3"), "diff(x^3,x,2);");
        assert_eq!(tm("\\sum_{n=1}^{\\infty}\\frac{1}{n^2}"), "ev(sum(1/n^2,n,1,inf),simpsum);");
        assert_eq!(tm("\\lim_{x\\to 0}\\frac{\\sin x}{x}"), "limit(sin(x)/x,x,0);");
        assert_eq!(tm("\\lim_{x\\to 0^+}\\frac{1}{x}"), "limit(1/x,x,0,plus);");
        assert_eq!(tm("\\sqrt{x+1}"), "sqrt(x+1);");
        assert_eq!(tm("\\sqrt[3]{x}"), "x^(1/3);");
        assert_eq!(tm("2\\pi r"), "2*%pi*r;");
        assert_eq!(tm("\\sin^{-1}x"), "asin(x);");
        assert_eq!(tm("e^{i\\pi}+1"), "%e^(i*%pi)+1;");
        assert_eq!(tm("\\int \\frac{dx}{x}"), "integrate(1/x,x);");
        assert_eq!(tm("\\int x^2 dx"), "integrate(x^2,x);");
        assert_eq!(tm("\\begin{pmatrix}1&2\\\\3&4\\end{pmatrix}"), "matrix([1,2],[3,4]);");
        assert_eq!(tm("|x-1|"), "abs(x-1);");
        assert_eq!(tm("x^2=4"), "x^2=4;");
        assert_eq!(tm("a:5; a^2"), "a:5; a^2;");
        assert_eq!(tm("f(x):=x^2; f(3)"), "f(x):=x^2; f(3);");
    }

    #[test]
    fn maxima_script_roundtrips_through_textbook() {
        words::register_functions(["plot3d".to_string(), "ilt".to_string()]);
        let src = "matrix([1,2],[3,4])^^-1; a:5$ f(x):=x^2+a$ plot3d(sin(x)*cos(y),[x,-3,3],[y,-3,3])$ ilt(1/(s+a),s,t);";
        let latex = maxima_script_to_latex(src).unwrap();
        let back = latex_to_maxima(&latex, &ParseCtx::default()).unwrap();
        assert_eq!(
            back,
            "matrix([1,2],[3,4])^^(-1); a:5$ f(x):=x^2+a$ plot3d(sin(x)*cos(y),[x,-3,3],[y,-3,3])$ ilt(1/(s+a),s,t);",
            "{latex}"
        );
        // Through the tree directly as well (no LaTeX hop).
        let t = maxima_script_to_tree(src).unwrap();
        assert_eq!(tree_to_maxima(&t, &ParseCtx::default()).unwrap(), back);
    }

    #[test]
    fn output_display_roundtrip() {
        let r = maxima_to_tree("integrate(x*sin(x),x,0,%pi)").unwrap();
        assert!(matches!(r[0], Item::Big(BigOp::Int, ..)));
        assert!(matches!(maxima_to_tree("integrate(x,x)").unwrap()[0], Item::Big(BigOp::IntNoLimits, ..)));
        let l = maxima_to_latex("(2*%pi)/3").unwrap();
        assert!(l.contains("\\frac"), "{l}");
        assert_eq!(maxima_to_latex("x^2+1/2").unwrap(), "x^{2}+\\frac{1}{2}");
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn registered_names_with_digits_are_functions() {
        words::register_functions(["plot2d".to_string(), "ratsimp".to_string()]);
        let r = from_latex("plot2d(sin(x),[x,-5,5])");
        let m = tree_to_maxima(&r, &ParseCtx::default()).unwrap();
        assert_eq!(m, "plot2d(sin(x),[x,−5,5]);".replace('−', "-"), "{m}");
        assert_eq!(latex_to_maxima("ratsimp(x^2/x)", &ParseCtx::default()).unwrap(), "ratsimp(x^2/x);");
        // Unregistered letter runs stay products of single letters.
        assert_eq!(latex_to_maxima("abx", &ParseCtx::default()).unwrap(), "a*b*x;");
    }
}

#[cfg(test)]
mod demo_tests {
    use super::*;

    #[test]
    fn demo_cell_converts() {
        let src = "matrix([1,2],[3,4])^^-1; a:5$ f(x):=x^2+a$ b:[1,2,3]$ g:sqrt(2)/3$";
        let t = maxima_script_to_tree(src);
        assert!(t.is_ok(), "{:?}", t);
    }
}

#[cfg(test)]
mod conversion_tests {
    use super::*;

    #[test]
    fn single_statement_loses_its_semicolon() {
        let l = maxima_script_to_latex("integrate(x^2,x);").unwrap();
        assert!(!l.contains(';') && l.ends_with("dx"), "{l}");
        // `$` hides the output, so it stays; several statements keep their separators.
        assert!(maxima_script_to_latex("a:5$").unwrap().ends_with("$"));
        assert!(maxima_script_to_latex("a:5; b:6;").unwrap().contains(';'));
        // The round trip still evaluates the same.
        let l = maxima_script_to_latex("diff(sin(x),x);").unwrap();
        assert_eq!(latex_to_maxima(&l, &ParseCtx::default()).unwrap(), "diff(sin(x),x);");
    }

    #[test]
    fn unmatched_brackets_survive_a_latex_round_trip() {
        let r = from_latex("x+1)");
        assert_eq!(row_to_latex(&r), "x+1)");
        let r = from_latex("(x+1");
        assert_eq!(r.first(), Some(&Item::Char('(')));
        assert!(latex_to_maxima("(x+1", &ParseCtx::default()).unwrap_err().contains("unbalanced"));
    }
}
