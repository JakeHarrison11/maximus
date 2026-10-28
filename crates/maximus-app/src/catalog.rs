//! Discoverable Maxima functions: curated categories and symbol palettes.

/// A call template such as `integrate(<expr=x^2>, <var=x>)`.
#[derive(Clone, Debug, PartialEq)]
pub struct CallTemplate {
    pub label: String,
    pub template: String,
    pub description: String,
    /// Statement(s) to run before using the function (e.g. `load(descriptive)$`).
    pub pre: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placeholder {
    pub name: String,
    pub default: String,
}

impl CallTemplate {
    pub fn new(label: &str, template: &str, description: &str) -> Self {
        CallTemplate { label: label.into(), template: template.into(), description: description.into(), pre: None }
    }

    pub fn with_pre(mut self, pre: &str) -> Self {
        self.pre = Some(pre.into());
        self
    }

    /// Function name (text before the first parenthesis).
    pub fn function_name(&self) -> String {
        self.template.split('(').next().unwrap_or("").trim().to_string()
    }

    pub fn placeholders(&self) -> Vec<Placeholder> {
        let mut out = Vec::new();
        let t: Vec<char> = self.template.chars().collect();
        let mut i = 0;
        while i < t.len() {
            if t[i] == '<' {
                if let Some(end) = t[i + 1..].iter().position(|c| *c == '>') {
                    let inner: String = t[i + 1..i + 1 + end].iter().collect();
                    let (name, default) = inner.split_once('=').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((inner.clone(), String::new()));
                    out.push(Placeholder { name, default });
                    i += end + 2;
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    /// Fills the placeholders with `values` (one per placeholder, in order).
    pub fn fill(&self, values: &[String]) -> String {
        let t: Vec<char> = self.template.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        let mut k = 0;
        while i < t.len() {
            if t[i] == '<' {
                if let Some(end) = t[i + 1..].iter().position(|c| *c == '>') {
                    let v = values.get(k).map(|s| s.trim()).unwrap_or("");
                    out.push_str(v);
                    k += 1;
                    i += end + 2;
                    continue;
                }
            }
            out.push(t[i]);
            i += 1;
        }
        out
    }

    /// Builds a template from a manual signature such as `integrate (<expr>, <x>)`.
    pub fn from_signature(sig: &str) -> CallTemplate {
        let name = sig.split(|c: char| c.is_whitespace() || c == '(').next().unwrap_or("").to_string();
        let rest = sig.trim_start_matches(&name).trim_start();
        let cleaned = rest.replace("[", "<").replace("]", ">").replace("...", "");
        let body = if cleaned.contains('(') { cleaned } else { "()".to_string() };
        CallTemplate::new(&name, &format!("{}{}", name, body), "")
    }
}

pub struct Category {
    pub name: &'static str,
    pub items: Vec<CallTemplate>,
}

pub fn curated() -> Vec<Category> {
    let t = CallTemplate::new;
    vec![
        Category {
            name: "Algebra",
            items: vec![
                t("Expand", "expand(<expr=(x+1)^3>)", "Multiply out products and powers."),
                t("Factor", "factor(<expr=x^2-1>)", "Factor into irreducible factors."),
                t("Simplify (rational)", "ratsimp(<expr=(x^2-1)/(x-1)>)", "Simplify a rational expression."),
                t("Simplify (full)", "fullratsimp(<expr>)", "Repeated rational simplification."),
                t("Simplify radicals", "radcan(<expr=sqrt(x^2)>)", "Canonical form for radicals, logs and exponentials."),
                t("Partial fractions", "partfrac(<expr=1/(x^2-1)>, <var=x>)", "Partial fraction decomposition."),
                t("Collect terms", "collect(<expr>, <var=x>)", "Collect coefficients of powers of a variable."),
                t("Substitute", "subst(<new=2>, <old=x>, <expr=x^2+x>)", "Replace a symbol by a value or expression."),
                t("Coefficient", "coeff(<expr>, <var=x>, <n=1>)", "Coefficient of var^n."),
                t("Real part", "realpart(<expr>)", ""),
                t("Imaginary part", "imagpart(<expr>)", ""),
                t("Rectangular form", "rectform(<expr>)", "Write a complex number as a+b%i."),
                t("Polar form", "polarform(<expr>)", ""),
                t("Absolute value", "cabs(<expr>)", "Complex absolute value."),
            ],
        },
        Category {
            name: "Equations",
            items: vec![
                t("Solve equation", "solve(<equation=x^2-4=0>, <var=x>)", "Exact solutions of an equation."),
                t("Solve system", "solve([<eq1=x+y=3>, <eq2=x-y=1>], [<vars=x,y>])", "Solve a system of equations."),
                t("Numeric roots of polynomial", "allroots(<poly=x^3-x-1>)", "Numerical roots of a polynomial."),
                t("Real roots (numeric)", "realroots(<poly=x^3-x-1>)", ""),
                t("Find root in interval", "find_root(<f=sin(x)-x/2>, <var=x>, <a=1>, <b=3>)", "Bisection/Brent root finding."),
                t("Solve ODE", "ode2(<ode='diff(y,x)=y>, <y=y>, <x=x>)", "First and second order ordinary differential equations."),
                t("Left side", "lhs(<eq>)", ""),
                t("Right side", "rhs(<eq>)", ""),
            ],
        },
        Category {
            name: "Calculus",
            items: vec![
                t("Integrate (indefinite)", "integrate(<expr=x*sin(x)>, <var=x>)", "Symbolic antiderivative."),
                t("Integrate (definite)", "integrate(<expr=x*sin(x)>, <var=x>, <a=0>, <b=%pi>)", "Definite integral between limits."),
                t("Numeric integral", "quad_qags(<expr=exp(-x^2)>, <var=x>, <a=0>, <b=1>)", "Adaptive numeric integration."),
                t("Differentiate", "diff(<expr=sin(x)^2>, <var=x>)", "Derivative."),
                t("Higher derivative", "diff(<expr=x^5>, <var=x>, <n=2>)", "n-th derivative."),
                t("Limit", "limit(<expr=sin(x)/x>, <var=x>, <point=0>)", "Limit of an expression."),
                t("Limit from the right", "limit(<expr=1/x>, <var=x>, <point=0>, plus)", ""),
                t("Taylor series", "taylor(<expr=exp(x)>, <var=x>, <point=0>, <order=5>)", "Truncated Taylor series."),
                t("Sum", "sum(<expr=1/k^2>, <var=k>, <from=1>, <to=inf>)", "Summation (use simpsum to evaluate)."),
                t("Product", "product(<expr=k>, <var=k>, <from=1>, <to=5>)", ""),
                t("Laplace transform", "laplace(<expr=exp(-a*t)>, <t=t>, <s=s>)", ""),
                t("Inverse Laplace", "ilt(<expr=1/(s+a)>, <s=s>, <t=t>)", ""),
                t("Gradient / Jacobian", "jacobian([<f1=x^2*y>, <f2=x+y>], [<vars=x,y>])", ""),
            ],
        },
        Category {
            name: "Linear algebra",
            items: vec![
                t("Matrix", "matrix([<row1=1,2>], [<row2=3,4>])", "Create a matrix from rows."),
                t("Identity matrix", "ident(<n=3>)", ""),
                t("Zero matrix", "zeromatrix(<m=2>, <n=2>)", ""),
                t("Determinant", "determinant(<matrix>)", ""),
                t("Inverse", "invert(<matrix>)", ""),
                t("Transpose", "transpose(<matrix>)", ""),
                t("Rank", "rank(<matrix>)", ""),
                t("Eigenvalues", "eigenvalues(<matrix>)", ""),
                t("Eigenvectors", "eigenvectors(<matrix>)", ""),
                t("Characteristic polynomial", "charpoly(<matrix>, <var=x>)", ""),
                t("Row echelon form", "echelon(<matrix>)", ""),
                t("Solve linear system", "linsolve([<eq1=x+y=3>, <eq2=x-y=1>], [<vars=x,y>])", ""),
            ],
        },
        Category {
            name: "Simplification & trig",
            items: vec![
                t("Trig simplify", "trigsimp(<expr=sin(x)^2+cos(x)^2>)", ""),
                t("Trig expand", "trigexpand(<expr=sin(2*x)>)", ""),
                t("Trig reduce", "trigreduce(<expr=sin(x)^2>)", ""),
                t("Exponentials to trig", "demoivre(<expr=exp(%i*x)>)", ""),
                t("Trig to exponentials", "exponentialize(<expr=sin(x)>)", ""),
                t("Expand logarithms", "ev(log(<expr=a*b>), logexpand=super)", ""),
                t("Rationalize denominator", "rationalize(<expr=1/(1+sqrt(2))>)", ""),
                t("Evaluate with options", "ev(<expr>, <option=numer>)", "Evaluate with flags or substitutions."),
            ],
        },
        Category {
            name: "Numeric",
            items: vec![
                t("Decimal value", "float(<expr=%pi>)", "Floating point approximation."),
                t("Big float", "bfloat(<expr=%pi>)", "Arbitrary precision (see fpprec)."),
                t("Set precision", "fpprec: <digits=30>", "Digits used by bfloat."),
                t("Round", "round(<x=2.5>)", ""),
                t("Floor", "floor(<x=2.5>)", ""),
                t("Ceiling", "ceiling(<x=2.5>)", ""),
                t("Convert to rational", "rat(<x=0.75>)", ""),
            ],
        },
        Category {
            name: "Number theory",
            items: vec![
                t("Prime factorization", "ifactors(<n=360>)", ""),
                t("Is prime?", "primep(<n=97>)", ""),
                t("Next prime", "next_prime(<n=100>)", ""),
                t("Divisors", "divisors(<n=36>)", ""),
                t("GCD", "gcd(<a=12>, <b=18>)", ""),
                t("LCM", "lcm(<a=4>, <b=6>)", ""),
                t("Remainder", "mod(<a=17>, <b=5>)", ""),
                t("Binomial coefficient", "binomial(<n=10>, <k=3>)", ""),
                t("Factorial", "factorial(<n=10>)", ""),
                t("Euler totient", "totient(<n=36>)", ""),
            ],
        },
        Category {
            name: "Lists & sets",
            items: vec![
                t("Make list", "makelist(<expr=k^2>, <var=k>, <from=1>, <to=10>)", ""),
                t("Sort", "sort(<list=[3,1,2]>)", ""),
                t("Length", "length(<list>)", ""),
                t("Map function", "map(<f=sin>, <list=[1,2,3]>)", ""),
                t("Sum of list", "apply(\"+\", <list=[1,2,3]>)", ""),
                t("Set union", "union(<a={1,2}>, <b={2,3}>)", ""),
                t("Set intersection", "intersection(<a={1,2}>, <b={2,3}>)", ""),
            ],
        },
        Category {
            name: "Statistics",
            items: vec![
                t("Mean", "mean(<list=[1,2,3,4]>)", "").with_pre("load(descriptive)$"),
                t("Median", "median(<list=[1,2,3,4]>)", "").with_pre("load(descriptive)$"),
                t("Standard deviation", "std(<list=[1,2,3,4]>)", "").with_pre("load(descriptive)$"),
                t("Variance", "var(<list=[1,2,3,4]>)", "").with_pre("load(descriptive)$"),
            ],
        },
        Category {
            name: "Plotting",
            items: vec![
                t("Plot function", "plot2d(<expr=sin(x)>, [<var=x>, <min=-10>, <max=10>])", "Graph of one expression."),
                t("Plot several functions", "plot2d([<f=sin(x)>, <g=cos(x)>], [<var=x>, <min=-10>, <max=10>])", ""),
                t("Parametric plot", "plot2d([parametric, <x=cos(t)>, <y=sin(t)>, [<var=t>, <min=0>, <max=2*%pi>]])", ""),
                t("Plot points", "plot2d([discrete, [<xs=1,2,3>], [<ys=2,4,3>]])", ""),
                t("3D surface", "plot3d(<expr=sin(x)*cos(y)>, [<x=x>, <xmin=-3>, <xmax=3>], [<y=y>, <ymin=-3>, <ymax=3>])", "Rotate with the mouse."),
            ],
        },
    ]
}

pub struct PaletteGroup {
    pub name: String,
    pub symbols: Vec<String>,
}

pub fn palettes() -> Vec<PaletteGroup> {
    let g = |name: &str, s: &str| PaletteGroup { name: name.into(), symbols: s.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_string()).collect() };
    vec![
        g("Greek (lower case)", "α β γ δ ε ζ η θ ι κ λ μ ν ξ π ρ σ τ υ φ χ ψ ω"),
        g("Greek (upper case)", "Γ Δ Θ Λ Ξ Π Σ Φ Ψ Ω"),
        g("Operators", "± × ÷ · ∞ ≤ ≥ ≠ ≈ → ∂ ∇ ° ∈ ∧ ∨ ¬ ! : ,"),
        g("Constants", "π ⅇ ⅈ ∞"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_templates() {
        let t = CallTemplate::new("Integrate", "integrate(<expr=x^2>, <var=x>)", "");
        let ph = t.placeholders();
        assert_eq!(ph.len(), 2);
        assert_eq!(ph[0].default, "x^2");
        assert_eq!(t.fill(&["x^3".into(), "x".into()]), "integrate(x^3, x)");
        assert_eq!(t.function_name(), "integrate");
    }

    #[test]
    fn signature_to_template() {
        let t = CallTemplate::from_signature("integrate (<expr>, <x>, <a>, <b>)");
        assert_eq!(t.template, "integrate(<expr>, <x>, <a>, <b>)");
        assert_eq!(t.placeholders().len(), 4);
        let t = CallTemplate::from_signature("makelist (<expr>, <i>, <i_0>, <i_1>)");
        assert_eq!(t.function_name(), "makelist");
    }

    #[test]
    fn curated_templates_parse_as_maxima_after_filling() {
        for c in curated() {
            for it in c.items {
                let vals: Vec<String> = it.placeholders().iter().map(|p| if p.default.is_empty() { "x".to_string() } else { p.default.clone() }).collect();
                let s = it.fill(&vals);
                assert!(!s.contains('<') || s.contains("<="), "{}: {}", it.label, s);
            }
        }
    }
}
