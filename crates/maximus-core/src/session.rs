//! A session ties a notebook to a backend and runs the evaluation queue.

use crate::backend::*;
use crate::notebook::*;
use maximus_math::ParseCtx;
use std::collections::{HashMap, VecDeque};

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
}

/// Result of an evaluation that is not attached to a cell (plots, MCP, variable edits).
#[derive(Clone, Debug, Default)]
pub struct RawResult {
    pub ok: bool,
    pub items: Vec<OutputItem>,
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
    raw_collect: RawResult,
    /// Outputs of the cell being evaluated; they replace the old ones only once evaluation ends.
    cell_collect: Vec<OutputItem>,
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
            raw_collect: RawResult::default(),
            cell_collect: Vec::new(),
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
            BackendStatus::Starting => {}
        }
    }

    pub fn parse_ctx(&self) -> ParseCtx {
        let mut ctx = ParseCtx::default();
        for v in &self.variables {
            if v.is_function {
                ctx.functions.insert(v.name.clone());
            }
        }
        ctx
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

    pub fn evaluate_cell(&mut self, id: u64) {
        let Some(cell) = self.notebook.cell_mut(id) else { return };
        if cell.kind != CellKind::Code || cell.state == CellState::Queued || cell.state == CellState::Running {
            return;
        }
        cell.state = CellState::Queued;
        self.queue.push_back(Job { kind: JobKind::Cell(id), req: RequestKind::Eval(String::new()) });
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
        self.queue.push_back(Job { kind: JobKind::Raw(token), req: RequestKind::Eval(input.to_string()) });
        token
    }

    pub fn take_raw(&mut self, token: u64) -> Option<RawResult> {
        self.raw_done.remove(&token)
    }

    pub fn refresh_variables(&mut self) {
        if !self.queue.iter().any(|j| j.kind == JobKind::Vars) {
            self.queue.push_back(Job { kind: JobKind::Vars, req: RequestKind::Vars });
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
        self.queue.clear();
        self.variables.clear();
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
        while self.inflight.is_none() {
            let Some(job) = self.queue.pop_front() else { break };
            changed = true;
            self.dispatch(job);
        }
        changed
    }

    fn dispatch(&mut self, job: Job) {
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
                self.variables = v;
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

/// Strips Maxima's debugger hint from error messages.
pub fn clean_error(e: &str) -> String {
    let e = e.trim();
    let e = e.split("-- an error.").next().unwrap_or(e).trim();
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
