//! A session ties a notebook to a backend and runs the evaluation queue.

use crate::backend::*;
use crate::notebook::*;
use maximus_math::{parse_maxima, split_statements, Expr, ParseCtx};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Debug, PartialEq)]
enum JobKind {
    Cell(u64),
    Vars,
    Raw(u64),
}

#[derive(Clone, Debug)]
struct Job {
    kind: JobKind,
    req: RequestKind,
    numerical: bool,
}

/// Result of an evaluation that is not attached to a cell (plots, MCP, variable edits).
#[derive(Clone, Debug, Default)]
pub struct RawResult {
    pub ok: bool,
    pub items: Vec<OutputItem>,
}

/// The unevaluated source of a successful, directly entered definition.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalDefinition {
    pub source: String,
    pub rhs: String,
}

impl RawResult {
    /// The last displayed result expression in Maxima syntax, if any.
    pub fn last_text(&self) -> Option<&str> {
        self.items.iter().rev().find_map(|i| match i {
            OutputItem::Result { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }

    pub fn error(&self) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            OutputItem::Error(e) => Some(e.as_str()),
            _ => None,
        })
    }
}

pub struct Session {
    pub notebook: Notebook,
    pub variables: Vec<VarInfo>,
    pub backend: Box<dyn Backend>,
    pub ready: bool,
    pub last_error: Option<String>,
    queue: VecDeque<Job>,
    inflight: Option<(RequestId, Job)>,
    next_req: RequestId,
    next_raw: u64,
    numerical: bool,
    applied_numerical: bool,
    raw_collect: RawResult,
    /// Outputs of the cell being evaluated; they replace the old ones only once evaluation ends.
    cell_collect: Vec<OutputItem>,
    /// Names the user has assigned or defined; packages' own variables and functions are not listed.
    user_names: HashSet<String>,
    original_definitions: HashMap<String, OriginalDefinition>,
    raw_done: HashMap<u64, RawResult>,
}

impl Session {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        let mut s = Session {
            notebook: Notebook::default(),
            variables: Vec::new(),
            backend,
            ready: false,
            last_error: None,
            queue: VecDeque::new(),
            inflight: None,
            next_req: 1,
            next_raw: 1,
            numerical: false,
            applied_numerical: false,
            raw_collect: RawResult::default(),
            cell_collect: Vec::new(),
            user_names: HashSet::new(),
            original_definitions: HashMap::new(),
            raw_done: HashMap::new(),
        };
        s.sync_status();
        s
    }

    fn sync_status(&mut self) {
        match self.backend.status() {
            BackendStatus::Dead(m) => {
                self.ready = false;
                self.last_error = Some(m);
            }
            BackendStatus::Ready => self.ready = true,
            BackendStatus::Starting => self.ready = false,
        }
    }

    pub fn parse_ctx(&self) -> ParseCtx {
        let mut ctx = ParseCtx::default();
        for v in &self.variables {
            if v.is_function {
                ctx.functions.insert(v.name.clone());
            } else {
                ctx.variables.insert(v.name.clone());
            }
        }
        ctx
    }

    pub fn original_definition(&self, name: &str) -> Option<&OriginalDefinition> {
        self.original_definitions.get(name)
    }

    pub fn is_busy(&self) -> bool {
        self.inflight.is_some() || !self.queue.is_empty()
    }

    pub fn running_cell(&self) -> Option<u64> {
        match &self.inflight {
            Some((_, Job { kind: JobKind::Cell(id), .. })) => Some(*id),
            _ => None,
        }
    }

    // ------------------------------------------------------------ queueing

    pub fn set_numerical(&mut self, numerical: bool) {
        self.numerical = numerical;
    }

    pub fn evaluate_cell(&mut self, id: u64) {
        let numerical = self.numerical;
        let Some(cell) = self.notebook.cell_mut(id) else { return };
        if cell.kind != CellKind::Code || cell.state == CellState::Queued || cell.state == CellState::Running {
            return;
        }
        cell.state = CellState::Queued;
        self.queue.push_back(Job { kind: JobKind::Cell(id), req: RequestKind::Eval(String::new()), numerical });
    }

    pub fn evaluate_all(&mut self) {
        let ids: Vec<u64> = self.notebook.cells.iter().filter(|c| c.kind == CellKind::Code).map(|c| c.id).collect();
        for id in ids {
            self.evaluate_cell(id);
        }
    }

    /// Evaluates every code cell up to and including `id`.
    pub fn evaluate_through(&mut self, id: u64) {
        let Some(end) = self.notebook.index_of(id) else { return };
        let ids: Vec<u64> = self.notebook.cells[..=end].iter().filter(|c| c.kind == CellKind::Code).map(|c| c.id).collect();
        for id in ids {
            self.evaluate_cell(id);
        }
    }

    /// Queues an evaluation that is not attached to a cell. Poll the result with `take_raw`.
    pub fn submit_raw(&mut self, input: &str) -> u64 {
        let token = self.next_raw;
        self.next_raw += 1;
        self.queue.push_back(Job { kind: JobKind::Raw(token), req: RequestKind::Eval(input.to_string()), numerical: self.numerical });
        token
    }

    pub fn take_raw(&mut self, token: u64) -> Option<RawResult> {
        self.raw_done.remove(&token)
    }

    pub fn refresh_variables(&mut self) {
        if !self.queue.iter().any(|j| j.kind == JobKind::Vars) {
            self.queue.push_back(Job { kind: JobKind::Vars, req: RequestKind::Vars, numerical: self.numerical });
        }
    }

    pub fn reset_variables(&mut self) {
        self.submit_raw("kill(all)$");
        self.refresh_variables();
    }

    pub fn set_variable(&mut self, name: &str, value: &str) {
        self.submit_raw(&format!("{}: {}$", name, value));
        self.refresh_variables();
    }

    pub fn remove_variable(&mut self, name: &str) {
        self.queue.retain(|job| !matches!(job.kind, JobKind::Vars));
        self.submit_raw(&format!("kill({})$", name));
        self.refresh_variables();
    }

    pub fn interrupt(&mut self) {
        self.backend.interrupt();
        self.cancel_queued_cells();
    }

    pub fn restart(&mut self) {
        self.backend.restart();
        self.ready = false;
        if let Some((_, job)) = self.inflight.take() {
            self.fail_job(job, "Maxima was restarted");
        }
        self.cancel_queued_cells();
        while let Some(job) = self.queue.pop_front() {
            self.fail_job(job, "Maxima was restarted");
        }
        self.variables.clear();
        self.user_names.clear();
        self.original_definitions.clear();
        self.applied_numerical = false;
        self.last_error = None;
        self.sync_status();
    }

    fn cancel_queued_cells(&mut self) {
        let mut keep = VecDeque::new();
        for j in self.queue.drain(..) {
            match j.kind {
                JobKind::Cell(id) => {
                    if let Some(c) = self.notebook.cell_mut(id) {
                        c.state = CellState::Idle;
                    }
                }
                _ => keep.push_back(j),
            }
        }
        self.queue = keep;
    }

    fn fail_job(&mut self, job: Job, msg: &str) {
        match job.kind {
            JobKind::Cell(id) => {
                let mut outs = std::mem::take(&mut self.cell_collect);
                outs.push(OutputItem::Error(msg.to_string()));
                if let Some(c) = self.notebook.cell_mut(id) {
                    c.outputs = outs;
                    c.state = CellState::Failed;
                }
            }
            JobKind::Raw(t) => {
                self.raw_done.insert(t, RawResult { ok: false, items: vec![OutputItem::Error(msg.to_string())] });
            }
            JobKind::Vars => {}
        }
    }

    // ------------------------------------------------------------ pump

    /// Processes backend events and dispatches queued work. Returns true if anything changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        for ev in self.backend.poll() {
            changed = true;
            self.handle_event(ev);
        }
        if !self.ready {
            if let BackendStatus::Dead(message) = self.backend.status() {
                while let Some(job) = self.queue.pop_front() {
                    self.fail_job(job, &message);
                    changed = true;
                }
            }
        }
        while self.inflight.is_none() && self.ready {
            let Some(job) = self.queue.pop_front() else { break };
            changed = true;
            self.dispatch(job);
        }
        changed
    }

    fn dispatch(&mut self, mut job: Job) {
        let req = match &job.kind {
            JobKind::Cell(id) => {
                let ctx = self.parse_ctx();
                let Some(cell) = self.notebook.cell_mut(*id) else { return };
                self.cell_collect.clear();
                match cell.to_maxima(&ctx) {
                    Ok(src) if src.trim().is_empty() => {
                        cell.outputs.clear();
                        cell.state = CellState::Done;
                        return;
                    }
                    Ok(src) => {
                        cell.state = CellState::Running;
                        RequestKind::Eval(src)
                    }
                    Err(e) => {
                        cell.outputs = vec![OutputItem::Error(format!("Input error: {}", e))];
                        cell.state = CellState::Failed;
                        self.cancel_queued_cells();
                        return;
                    }
                }
            }
            _ => job.req.clone(),
        };
        // Keep the exact dispatched source, before numerical-mode prefixes or later cell edits.
        job.req = req.clone();
        let req = match req {
            RequestKind::Eval(src) if job.numerical => {
                self.applied_numerical = true;
                RequestKind::Eval(format!("numer:true$ float:true$ {src}"))
            }
            RequestKind::Eval(src) if self.applied_numerical => {
                self.applied_numerical = false;
                RequestKind::Eval(format!("numer:false$ float:false$ {src}"))
            }
            req => req,
        };
        let id = self.next_req;
        self.next_req += 1;
        if matches!(job.kind, JobKind::Raw(_)) {
            self.raw_collect = RawResult::default();
        }
        self.backend.submit(id, req);
        self.inflight = Some((id, job));
    }

    fn handle_event(&mut self, ev: Event) {
        match ev.kind {
            EventKind::Ready => {
                self.ready = true;
                self.last_error = None;
            }
            EventKind::Crashed(msg) => {
                self.ready = false;
                self.last_error = Some(msg.clone());
                if let Some((_, job)) = self.inflight.take() {
                    self.fail_job(job, &msg);
                }
                self.cancel_queued_cells();
                while let Some(job) = self.queue.pop_front() {
                    self.fail_job(job, &msg);
                }
            }
            kind => {
                let Some((rid, job)) = self.inflight.clone() else { return };
                if rid != ev.id {
                    return;
                }
                self.handle_job_event(job, kind);
            }
        }
    }

    fn handle_job_event(&mut self, job: Job, kind: EventKind) {
        let item = match kind {
            EventKind::Out(t) => Some(OutputItem::Text(t.trim_end_matches('\n').to_string())),
            EventKind::Result(r) if r.shown => Some(OutputItem::Result { label: r.label, text: r.text, tex: r.tex }),
            EventKind::Result(_) => None,
            EventKind::Error(e) => Some(OutputItem::Error(clean_error(&e))),
            EventKind::Plot(j) => Some(match crate::PlotData::from_json(&j) {
                Ok(p) => OutputItem::Plot(p),
                Err(e) => OutputItem::Error(format!("Bad plot data: {}", e)),
            }),
            EventKind::Vars(v) => {
                self.variables = v.into_iter().filter(|x| self.user_names.contains(&x.name)).collect();
                self.original_definitions.retain(|name, _| self.variables.iter().any(|v| &v.name == name));
                None
            }
            EventKind::Done { ok } => {
                self.finish(job, ok);
                return;
            }
            _ => None,
        };
        if let Some(item) = item {
            match &job.kind {
                JobKind::Cell(_) => self.cell_collect.push(item),
                JobKind::Raw(_) => self.raw_collect.items.push(item),
                JobKind::Vars => {}
            }
        }
    }

    fn finish(&mut self, job: Job, ok: bool) {
        self.inflight = None;
        if ok {
            if let RequestKind::Eval(src) = &job.req {
                self.record_definitions(src);
            }
        }
        match job.kind {
            JobKind::Cell(id) => {
                let outs = std::mem::take(&mut self.cell_collect);
                if let Some(c) = self.notebook.cell_mut(id) {
                    c.outputs = outs;
                    c.state = if ok { CellState::Done } else { CellState::Failed };
                }
                if !ok {
                    self.cancel_queued_cells();
                }
                self.refresh_variables();
            }
            JobKind::Raw(t) => {
                let mut r = std::mem::take(&mut self.raw_collect);
                r.ok = ok;
                self.raw_done.insert(t, r);
            }
            JobKind::Vars => {}
        }
    }

    fn record_definitions(&mut self, src: &str) {
        for (statement, _) in split_statements(src) {
            // Nested/dynamic definitions may be real, but must not inherit an older formula.
            for name in defined_names(&statement) {
                self.original_definitions.remove(&name);
                self.user_names.insert(name);
            }
            let Ok(expr) = parse_maxima(&statement) else { continue };
            match expr {
                Expr::Bin(ref op, ref lhs, ref value) if matches!(op.as_str(), ":" | ":=" | "::=") => {
                    let name = match lhs.as_ref() {
                        Expr::Sym(name) if op == ":" => name,
                        Expr::Call(name, args) if op != ":" && args.iter().all(|arg| matches!(arg, Expr::Sym(_))) => name,
                        _ => continue,
                    };
                    let Some(at) = top_level_separator(&statement, ':') else { continue };
                    let rhs = statement[at + op.len()..].trim().to_string();
                    if matches!(value.as_ref(), Expr::Bin(inner, ..) if matches!(inner.as_str(), ":" | ":=" | "::=")) {
                        self.record_definitions(&rhs);
                    }
                    self.original_definitions.insert(name.clone(), OriginalDefinition { source: statement, rhs });
                    self.user_names.insert(name.clone());
                }
                Expr::Call(ref call, ref args) if call == "define" || call == "define_variable" => {
                    let name = match args.first() {
                        Some(Expr::Call(name, params)) if call == "define" && params.iter().all(|arg| matches!(arg, Expr::Sym(_))) && args.len() == 2 => name,
                        Some(Expr::Sym(name)) if call == "define_variable" && args.len() >= 2 => name,
                        _ => continue,
                    };
                    let Some(open) = statement.find('(') else { continue };
                    if statement[..open].trim() != call {
                        continue;
                    }
                    let inner = &statement[open + 1..statement.len() - 1];
                    let Some(comma) = top_level_separator(inner, ',') else { continue };
                    let rhs = &inner[comma + 1..];
                    let end = top_level_separator(rhs, ',').unwrap_or(rhs.len());
                    let rhs = rhs[..end].trim().to_string();
                    self.original_definitions.insert(name.clone(), OriginalDefinition { source: statement, rhs });
                    self.user_names.insert(name.clone());
                }
                Expr::Call(call, args) if call == "kill" => {
                    for arg in args {
                        if let Expr::Sym(name) = arg {
                            if name == "all" {
                                self.original_definitions.clear();
                                self.user_names.clear();
                            } else {
                                self.original_definitions.remove(&name);
                                self.user_names.remove(&name);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Blocking evaluation helper for headless use (MCP, tests).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_blocking(&mut self, input: &str, timeout: std::time::Duration) -> RawResult {
        let token = self.submit_raw(input);
        let start = std::time::Instant::now();
        loop {
            self.tick();
            if let Some(r) = self.take_raw(token) {
                return r;
            }
            if start.elapsed() > timeout {
                self.interrupt();
                // Give Maxima a moment to unwind so the next request starts clean.
                let grace = std::time::Instant::now();
                while grace.elapsed() < std::time::Duration::from_secs(2) {
                    self.tick();
                    if let Some(mut r) = self.take_raw(token) {
                        r.ok = false;
                        r.items.push(OutputItem::Error("Timed out".into()));
                        return r;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                return RawResult { ok: false, items: vec![OutputItem::Error("Timed out".into())] };
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}

/// Find a delimiter outside strings and brackets in an already split statement.
fn top_level_separator(src: &str, separator: char) -> Option<usize> {
    let (mut depth, mut quoted, mut escaped) = (0, false, false);
    for (at, ch) in src.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
        } else if !quoted {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ if ch == separator && depth == 0 => return Some(at),
                _ => {}
            }
        }
    }
    None
}

/// Names that a piece of Maxima source assigns (`a: 1`, `a[i]: 1`, `a :: 1`) or defines
/// (`f(x) := ...`, `f[n] := ...`, `define(f(x), ...)`). Strings and comments are skipped.
pub fn defined_names(src: &str) -> Vec<String> {
    let c: Vec<char> = src.chars().collect();
    let is_start = |ch: char| ch.is_alphabetic() || ch == '_' || ch == '%';
    let is_part = |ch: char| ch.is_alphanumeric() || ch == '_' || ch == '%';
    let skip_ws = |mut j: usize| {
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        j
    };
    let skip_balanced = |j: usize, open: char, close: char| -> usize {
        let mut depth = 0;
        let mut k = j;
        while k < c.len() {
            if c[k] == open {
                depth += 1;
            } else if c[k] == close {
                depth -= 1;
                if depth == 0 {
                    return k + 1;
                }
            }
            k += 1;
        }
        c.len()
    };
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else if c[i] == '"' {
            i += 1;
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
        } else if is_start(c[i]) {
            let s = i;
            while i < c.len() && is_part(c[i]) {
                i += 1;
            }
            let name: String = c[s..i].iter().collect();
            let mut j = skip_ws(i);
            if name == "define" || name == "define_variable" {
                if c.get(j) == Some(&'(') {
                    let k = skip_ws(j + 1);
                    let e = {
                        let mut e = k;
                        while e < c.len() && is_part(c[e]) {
                            e += 1;
                        }
                        e
                    };
                    if e > k && is_start(c[k]) {
                        out.push(c[k..e].iter().collect());
                    }
                }
                continue;
            }
            if c.get(j) == Some(&'[') {
                j = skip_ws(skip_balanced(j, '[', ']'));
            }
            let mut k = j;
            if c.get(k) == Some(&'(') {
                k = skip_ws(skip_balanced(k, '(', ')'));
            }
            let at = |p: usize, s: &str| s.chars().enumerate().all(|(n, ch)| c.get(p + n) == Some(&ch));
            if at(k, ":=") || at(k, "::=") {
                out.push(name);
            } else if k == j && c.get(j) == Some(&':') {
                out.push(name);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Strips Maxima's debugger hint from error messages.
pub fn clean_error(e: &str) -> String {
    let e = e.trim();
    let e = e.split("-- an error.").next().unwrap_or(e).trim();
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_assigned_and_defined_names() {
        let n = |s: &str| defined_names(s);
        assert_eq!(n("a: 5$ b:[1,2]; c : a+1"), vec!["a", "b", "c"]);
        assert_eq!(n("f(x) := x^2 + a$ g[n] := n!; h(x,y):=x"), vec!["f", "g", "h"]);
        assert_eq!(n("define(k(x), x+1)$ x_1: 3"), vec!["k", "x_1"]);
        assert_eq!(n("a[1]: 4$ s :: \"t\""), vec!["a", "s"]);
        // Not assignments: keyword arguments, strings, comments, calls.
        assert!(n("plot2d(sin(x),[x,0,1])$ ev(e, x=1)$ \"a: b\"; /* z: 1 */ solve(x^2=1,x)").is_empty());
        assert_eq!(n("block([i:0], total: i)"), vec!["i", "total"]);
    }

    #[test]
    fn original_formulas_preserve_rhs_and_ignore_nested_or_dynamic_assignments() {
        let mut s = Session::new(Box::new(Echo { pending: vec![] }));
        s.record_definitions(r#"a: 2 * %pi / 3$ /* b:wrong */ b: [a, "x:$,y", f(1,2)]$ f(x) := x + a$ a: b + 1;"#);
        assert_eq!(s.original_definition("a").unwrap().rhs, "b + 1");
        assert_eq!(s.original_definition("b").unwrap().rhs, r#"[a, "x:$,y", f(1,2)]"#);
        assert_eq!(s.original_definition("f").unwrap().source, "f(x) := x + a");
        s.record_definitions(r#"define(g(x), x + f(a))$ define_variable(c, [a,"a,b"], any)$"#);
        assert_eq!(s.original_definition("g").unwrap().rhs, "x + f(a)");
        assert_eq!(s.original_definition("c").unwrap().rhs, r#"[a,"a,b"]"#);
        s.record_definitions(r#"block([i:0], a: i)$ a[1]: 7$ b :: 4$ "fake: 1";"#);
        assert!(s.original_definition("a").is_none());
        assert!(s.original_definition("b").is_none());
        assert!(s.original_definition("fake").is_none());
        assert!(s.original_definition("i").is_none());
        s.record_definitions("a: b: 3 + 4$");
        assert_eq!(s.original_definition("a").unwrap().rhs, "b: 3 + 4");
        assert_eq!(s.original_definition("b").unwrap().rhs, "3 + 4");
        s.record_definitions("a: [1,2]$ a[1]: 4$");
        assert!(s.original_definition("a").is_none());
    }

    #[test]
    fn successful_raw_definitions_replace_formulas_but_failed_edits_do_not() {
        let mut s = Session::new(Box::new(Echo { pending: vec![] }));
        s.submit_raw("a: 2 * %pi$ f(x):=x+a$");
        s.tick();
        assert!(s.original_definition("a").is_none());
        s.tick();
        assert_eq!(s.original_definition("a").unwrap().rhs, "2 * %pi");
        s.submit_raw("a: boom$ b: 3$");
        s.tick();
        s.tick();
        assert_eq!(s.original_definition("a").unwrap().rhs, "2 * %pi");
        assert!(!s.user_names.contains("b"));
        s.set_variable("a", "f(2) + 1");
        s.tick();
        s.tick();
        assert_eq!(s.original_definition("a").unwrap().rhs, "f(2) + 1");
    }

    #[test]
    fn cell_formulas_use_the_dispatched_source_and_survive_failed_reevaluation() {
        let mut s = Session::new(Box::new(Echo { pending: vec![] }));
        let id = s.notebook.cells[0].id;
        let cell = s.notebook.cell_mut(id).unwrap();
        cell.mode = InputMode::Maxima;
        cell.source = "a: 1 + 2$".into();
        s.evaluate_cell(id);
        s.tick();
        s.notebook.cell_mut(id).unwrap().source = "a: boom$".into();
        s.tick();
        assert_eq!(s.original_definition("a").unwrap().rhs, "1 + 2");
        // Supply the variable snapshot for the successful evaluation.
        let vars_id = s.inflight.as_ref().unwrap().0;
        s.handle_event(Event { id: vars_id, kind: EventKind::Vars(vec![VarInfo { name: "a".into(), value: "3".into(), is_function: false }]) });
        s.handle_event(Event { id: vars_id, kind: EventKind::Done { ok: true } });
        s.evaluate_cell(id);
        s.tick();
        s.tick();
        assert_eq!(s.original_definition("a").unwrap().rhs, "1 + 2");
    }

    #[test]
    fn removing_resetting_and_restarting_discard_original_formulas() {
        let mut s = Session::new(Box::new(Echo { pending: vec![] }));
        s.record_definitions("a: 1+2$ b: a+1$");
        s.remove_variable("a");
        s.tick();
        s.tick();
        assert!(s.original_definition("a").is_none());
        assert!(s.original_definition("b").is_some());
        s.tick();
        assert!(s.original_definition("b").is_none()); // Absent from the backend snapshot.
        s.record_definitions("a: 1+2$");
        s.reset_variables();
        s.tick();
        s.tick();
        assert!(s.original_definitions.is_empty());
        assert!(s.user_names.is_empty());
        s.record_definitions("f(x):=x+1$");
        s.restart();
        assert!(s.original_definitions.is_empty());
    }

    #[test]
    fn deleting_variable_refreshes_after_any_pending_snapshot() {
        let mut s = Session::new(Box::new(Echo { pending: vec![] }));
        s.record_definitions("a: 1$ b: a + 1$");
        s.variables = vec![
            VarInfo { name: "a".into(), value: "1".into(), is_function: false },
            VarInfo { name: "b".into(), value: "2".into(), is_function: false },
        ];
        s.refresh_variables();
        s.remove_variable("a");
        assert!(matches!(s.queue.front().map(|j| &j.req), Some(RequestKind::Eval(src)) if src == "kill(a)$"));
        assert!(matches!(s.queue.get(1).map(|j| &j.req), Some(RequestKind::Vars)));

        s.tick();
        s.tick();
        let vars_id = s.inflight.as_ref().unwrap().0;
        s.handle_event(Event {
            id: vars_id,
            kind: EventKind::Vars(vec![VarInfo { name: "b".into(), value: "4".into(), is_function: false }]),
        });
        s.handle_event(Event { id: vars_id, kind: EventKind::Done { ok: true } });
        assert_eq!(s.variables, vec![VarInfo { name: "b".into(), value: "4".into(), is_function: false }]);
    }

    #[test]
    fn parse_context_includes_multi_letter_variable_names() {
        let mut s = Session::new(Box::new(Echo { pending: vec![] }));
        s.variables.push(VarInfo { name: "mass".into(), value: "2".into(), is_function: false });
        s.variables.push(VarInfo { name: "force".into(), value: "force(x):=mass*x".into(), is_function: true });
        let ctx = s.parse_ctx();
        let row = maximus_math::from_latex("mass+1");
        assert_eq!(maximus_math::tree_to_plain_maxima(&row, &ctx).unwrap(), "mass+1");
    }

    /// Echo backend: replies to every Eval with the input as a result.
    struct Echo {
        pending: Vec<Event>,
    }
    impl Backend for Echo {
        fn name(&self) -> String {
            "echo".into()
        }
        fn status(&self) -> BackendStatus {
            BackendStatus::Ready
        }
        fn submit(&mut self, id: RequestId, req: RequestKind) {
            match req {
                RequestKind::Eval(s) => {
                    if s.contains("boom") {
                        self.pending.push(Event { id, kind: EventKind::Error("bad".into()) });
                        self.pending.push(Event { id, kind: EventKind::Done { ok: false } });
                    } else {
                        self.pending.push(Event {
                            id,
                            kind: EventKind::Result(ResultItem { label: "%o1".into(), shown: true, tex: s.clone(), text: s }),
                        });
                        self.pending.push(Event { id, kind: EventKind::Done { ok: true } });
                    }
                }
                RequestKind::Vars => {
                    self.pending.push(Event { id, kind: EventKind::Vars(vec![]) });
                    self.pending.push(Event { id, kind: EventKind::Done { ok: true } });
                }
            }
        }
        fn poll(&mut self) -> Vec<Event> {
            std::mem::take(&mut self.pending)
        }
        fn interrupt(&mut self) {}
        fn restart(&mut self) {}
    }

    #[test]
    fn waits_for_ready_before_dispatching() {
        let mut session = Session::new(Box::new(Echo { pending: vec![] }));
        session.ready = false;
        let token = session.submit_raw("a:1$");
        session.tick();
        assert!(session.inflight.is_none());
        session.handle_event(Event { id: 0, kind: EventKind::Ready });
        session.tick();
        session.tick();
        assert!(session.take_raw(token).unwrap().ok);
    }

    #[test]
    fn restart_finishes_raw_requests_and_discards_names() {
        let mut session = Session::new(Box::new(Echo { pending: vec![] }));
        let first = session.submit_raw("f(x):=x$");
        session.tick();
        let queued = session.submit_raw("b:2$");
        assert!(!session.user_names.contains("f"));
        session.restart();
        assert!(session.take_raw(first).unwrap().error().is_some());
        assert!(session.take_raw(queued).unwrap().error().is_some());
        assert!(session.user_names.is_empty());
        assert!(!session.is_busy());
    }

    #[test]
    fn unavailable_backend_fails_jobs_without_hanging() {
        let mut session = Session::new(Box::new(NullBackend::new("unavailable")));
        let token = session.submit_raw("1+1;");
        session.tick();
        assert_eq!(session.take_raw(token).unwrap().error(), Some("unavailable"));
        assert!(!session.is_busy());
    }

    #[test]
    fn old_output_stays_until_reevaluation_finishes() {
        let mut s = Session::new(Box::new(Echo { pending: vec![] }));
        let a = s.notebook.cells[0].id;
        s.notebook.cell_mut(a).unwrap().mode = InputMode::Maxima;
        s.notebook.cell_mut(a).unwrap().source = "1+1".into();
        s.evaluate_cell(a);
        for _ in 0..10 {
            s.tick();
        }
        assert_eq!(s.notebook.cell(a).unwrap().outputs.len(), 1);
        s.notebook.cell_mut(a).unwrap().source = "2+2".into();
        s.evaluate_cell(a);
        // Dispatched but the Echo backend has not been polled yet: the old output must still be there.
        s.tick();
        assert_eq!(s.notebook.cell(a).unwrap().state, CellState::Running);
        match &s.notebook.cell(a).unwrap().outputs[0] {
            OutputItem::Result { text, .. } => assert_eq!(text, "1+1;"),
            o => panic!("{:?}", o),
        }
        for _ in 0..10 {
            s.tick();
        }
        match &s.notebook.cell(a).unwrap().outputs[0] {
            OutputItem::Result { text, .. } => assert_eq!(text, "2+2;"),
            o => panic!("{:?}", o),
        }
    }

    #[test]
    fn numerical_mode_is_applied_to_new_evaluations_and_reset_when_disabled() {
        let mut s = Session::new(Box::new(Echo { pending: vec![] }));
        let id = s.notebook.cells[0].id;
        s.notebook.cell_mut(id).unwrap().mode = InputMode::Maxima;
        s.notebook.cell_mut(id).unwrap().source = "1/3".into();

        s.set_numerical(true);
        s.evaluate_cell(id);
        for _ in 0..10 {
            s.tick();
        }
        match &s.notebook.cell(id).unwrap().outputs[0] {
            OutputItem::Result { text, .. } => assert_eq!(text, "numer:true$ float:true$ 1/3;"),
            output => panic!("{output:?}"),
        }

        s.set_numerical(false);
        s.evaluate_cell(id);
        for _ in 0..10 {
            s.tick();
        }
        match &s.notebook.cell(id).unwrap().outputs[0] {
            OutputItem::Result { text, .. } => assert_eq!(text, "numer:false$ float:false$ 1/3;"),
            output => panic!("{output:?}"),
        }
    }

    #[test]
    fn evaluates_cells_in_order_and_stops_on_error() {
        let mut s = Session::new(Box::new(Echo { pending: vec![] }));
        let a = s.notebook.cells[0].id;
        s.notebook.cell_mut(a).unwrap().mode = InputMode::Maxima;
        s.notebook.cell_mut(a).unwrap().source = "1+1".into();
        let b = s.notebook.add_cell(CellKind::Code, InputMode::Maxima);
        s.notebook.cell_mut(b).unwrap().source = "boom".into();
        let c = s.notebook.add_cell(CellKind::Code, InputMode::Maxima);
        s.notebook.cell_mut(c).unwrap().source = "3".into();
        s.evaluate_all();
        for _ in 0..20 {
            s.tick();
        }
        assert_eq!(s.notebook.cell(a).unwrap().state, CellState::Done);
        assert_eq!(s.notebook.cell(b).unwrap().state, CellState::Failed);
        assert_eq!(s.notebook.cell(c).unwrap().state, CellState::Idle);
    }
}
