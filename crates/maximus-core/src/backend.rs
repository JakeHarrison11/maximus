//! Backend abstraction. A backend is anything that can evaluate Maxima input:
//! a local subprocess, a remote server over a socket, or (eventually) an in-browser build.
//! The interface is message based and non-blocking so it also fits single-threaded WASM.

pub type RequestId = u64;

#[derive(Clone, Debug, PartialEq)]
pub enum RequestKind {
    /// Evaluate Maxima statements.
    Eval(String),
    /// List user variables and functions.
    Vars,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResultItem {
    /// Output label such as `%o3`.
    pub label: String,
    /// `false` when the statement was terminated with `$`.
    pub shown: bool,
    pub tex: String,
    /// Maxima linear syntax of the result.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VarInfo {
    pub name: String,
    pub value: String,
    pub is_function: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EventKind {
    Ready,
    /// Text printed by the statement (print, disp, ...).
    Out(String),
    Result(ResultItem),
    Error(String),
    Vars(Vec<VarInfo>),
    /// Sampled plot data as JSON (see [`crate::PlotData`]).
    Plot(String),
    Done { ok: bool },
    /// The backend process died or the connection was lost.
    Crashed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub id: RequestId,
    pub kind: EventKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BackendStatus {
    Starting,
    Ready,
    Dead(String),
}

pub trait Backend {
    fn name(&self) -> String;
    fn status(&self) -> BackendStatus;
    fn submit(&mut self, id: RequestId, req: RequestKind);
    /// Returns events produced since the last call. Never blocks.
    fn poll(&mut self) -> Vec<Event>;
    /// Interrupts the running evaluation.
    fn interrupt(&mut self);
    /// Starts a fresh Maxima, discarding all state.
    fn restart(&mut self);
}

/// A backend that always fails; used when no Maxima could be found.
pub struct NullBackend {
    reason: String,
    pending: Vec<Event>,
}

impl NullBackend {
    pub fn new(reason: impl Into<String>) -> Self {
        Self { reason: reason.into(), pending: Vec::new() }
    }
}

impl Backend for NullBackend {
    fn name(&self) -> String {
        "none".into()
    }
    fn status(&self) -> BackendStatus {
        BackendStatus::Dead(self.reason.clone())
    }
    fn submit(&mut self, id: RequestId, _req: RequestKind) {
        self.pending.push(Event { id, kind: EventKind::Error(self.reason.clone()) });
        self.pending.push(Event { id, kind: EventKind::Done { ok: false } });
    }
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }
    fn interrupt(&mut self) {}
    fn restart(&mut self) {}
}
