//! Backend that drives a local Maxima process through a small Lisp driver
//! (see `driver.lisp`) using a framed line protocol on stdin/stdout.

use maximus_core::*;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

const DRIVER: &str = include_str!("driver.lisp");

#[derive(Clone, Debug, Default)]
pub struct NativeConfig {
    /// Path to the `maxima` executable; `None` searches `PATH`.
    pub maxima_path: Option<String>,
    /// Extra command line arguments, e.g. `--userdir=...`.
    pub extra_args: Vec<String>,
}

pub struct NativeBackend {
    cfg: NativeConfig,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    rx: Option<Receiver<Event>>,
    ready: bool,
    dead: Option<String>,
    driver_path: PathBuf,
    stderr_tail: Arc<Mutex<Vec<String>>>,
    failed: Vec<Event>,
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\\' => o.push_str("\\\\"),
            c => o.push(c),
        }
    }
    o
}

fn unescape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => o.push('\n'),
                Some('r') => o.push('\r'),
                Some('t') => o.push('\t'),
                Some(c) => o.push(c),
                None => o.push('\\'),
            }
        } else {
            o.push(c);
        }
    }
    o
}

/// Parses one driver frame; returns `None` for lines that are not frames.
pub fn parse_frame(line: &str) -> Option<Event> {
    let rest = line.strip_prefix("@@MX\t")?;
    let mut parts = rest.split('\t');
    let id: RequestId = parts.next()?.parse().ok()?;
    let kind = parts.next()?;
    let args: Vec<String> = parts.map(unescape).collect();
    let kind = match kind {
        "ready" => EventKind::Ready,
        "out" => EventKind::Out(args.first().cloned().unwrap_or_default()),
        "plot" => EventKind::Plot(args.first().cloned().unwrap_or_default()),
        "error" => EventKind::Error(args.first().cloned().unwrap_or_default()),
        "done" => EventKind::Done { ok: args.first().map_or(true, |a| a == "ok") },
        "result" => {
            let g = |i: usize| args.get(i).cloned().unwrap_or_default();
            EventKind::Result(ResultItem { label: g(0), shown: g(1) == "1", tex: g(2), text: g(3) })
        }
        "vars" => {
            let mut v = Vec::new();
            for c in args.chunks(3) {
                if c.len() == 3 {
                    v.push(VarInfo { name: c[1].clone(), value: c[2].clone(), is_function: c[0] == "f" });
                }
            }
            EventKind::Vars(v)
        }
        _ => return None,
    };
    Some(Event { id, kind })
}

impl NativeBackend {
    pub fn new(cfg: NativeConfig) -> Self {
        let dir = std::env::temp_dir().join(format!("maximus-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let driver_path = dir.join("driver.lisp");
        let _ = std::fs::write(&driver_path, DRIVER);
        let mut b = NativeBackend {
            cfg,
            child: None,
            stdin: None,
            rx: None,
            ready: false,
            dead: None,
            driver_path,
            stderr_tail: Arc::new(Mutex::new(Vec::new())),
            failed: Vec::new(),
        };
        b.spawn();
        b
    }

    fn spawn(&mut self) {
        self.ready = false;
        self.dead = None;
        self.stderr_tail.lock().unwrap().clear();
        let exe = self.cfg.maxima_path.clone().unwrap_or_else(|| "maxima".to_string());
        let mut cmd = Command::new(&exe);
        cmd.arg("--very-quiet")
            .arg("--disable-readline")
            .arg(format!("--preload-lisp={}", self.driver_path.display()))
            .arg("-r")
            .arg(":lisp (maxima::mx-loop)")
            .args(&self.cfg.extra_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match cmd.spawn() {
            Ok(mut child) => {
                let stdout = child.stdout.take().unwrap();
                let stderr = child.stderr.take().unwrap();
                self.stdin = child.stdin.take();
                let (tx, rx): (Sender<Event>, Receiver<Event>) = channel();
                let tail = self.stderr_tail.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        let mut t = tail.lock().unwrap();
                        t.push(line);
                        if t.len() > 20 {
                            t.remove(0);
                        }
                    }
                });
                let tail = self.stderr_tail.clone();
                std::thread::spawn(move || {
                    let mut r = BufReader::new(stdout);
                    let mut buf = Vec::new();
                    loop {
                        buf.clear();
                        match r.read_until(b'\n', &mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {
                                let line = String::from_utf8_lossy(&buf);
                                let line = line.trim_end_matches(['\n', '\r']);
                                if let Some(ev) = parse_frame(line) {
                                    if tx.send(ev).is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let extra = tail.lock().unwrap().join("\n");
                    let _ = tx.send(Event {
                        id: 0,
                        kind: EventKind::Crashed(format!("Maxima exited unexpectedly. {}", extra).trim().to_string()),
                    });
                });
                self.rx = Some(rx);
                self.child = Some(child);
            }
            Err(e) => {
                self.dead = Some(format!("Could not start '{}': {}. Install Maxima or set its path in Settings.", exe, e));
                self.child = None;
                self.stdin = None;
                self.rx = None;
            }
        }
    }

    fn kill(&mut self) {
        self.stdin = None;
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.rx = None;
    }
}

impl Drop for NativeBackend {
    fn drop(&mut self) {
        self.kill();
        if let Some(d) = self.driver_path.parent() {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

impl Backend for NativeBackend {
    fn name(&self) -> String {
        format!("maxima ({})", self.cfg.maxima_path.clone().unwrap_or_else(|| "PATH".into()))
    }

    fn status(&self) -> BackendStatus {
        if let Some(d) = &self.dead {
            BackendStatus::Dead(d.clone())
        } else if self.ready {
            BackendStatus::Ready
        } else {
            BackendStatus::Starting
        }
    }

    fn submit(&mut self, id: RequestId, req: RequestKind) {
        let line = match &req {
            RequestKind::Eval(s) => format!("{}\teval\t{}\n", id, escape(s)),
            RequestKind::Vars => format!("{}\tvars\t\n", id),
        };
        let ok = self.stdin.as_mut().map_or(false, |w| w.write_all(line.as_bytes()).and_then(|_| w.flush()).is_ok());
        if !ok {
            let msg = self.dead.clone().unwrap_or_else(|| "Maxima is not running".into());
            self.failed.push(Event { id, kind: EventKind::Error(msg) });
            self.failed.push(Event { id, kind: EventKind::Done { ok: false } });
        }
    }

    fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(ev) = rx.try_recv() {
                match &ev.kind {
                    EventKind::Ready => self.ready = true,
                    EventKind::Crashed(m) => {
                        self.dead = Some(m.clone());
                        self.ready = false;
                    }
                    _ => {}
                }
                out.push(ev);
            }
        }
        out.extend(std::mem::take(&mut self.failed));
        out
    }

    fn interrupt(&mut self) {
        if let Some(c) = &self.child {
            unsafe {
                libc::kill(c.id() as i32, libc::SIGINT);
            }
        }
    }

    fn restart(&mut self) {
        self.kill();
        self.failed.clear();
        self.spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn frame_parsing() {
        let e = parse_frame("@@MX\t3\tresult\t%o1\t1\t\\\\frac{1}{2}\t1/2").unwrap();
        assert_eq!(e.id, 3);
        match e.kind {
            EventKind::Result(r) => {
                assert_eq!(r.label, "%o1");
                assert!(r.shown);
                assert_eq!(r.tex, "\\frac{1}{2}");
                assert_eq!(r.text, "1/2");
            }
            k => panic!("{:?}", k),
        }
    }

    fn maxima_available() -> bool {
        Command::new("maxima").arg("--version").output().is_ok()
    }

    #[test]
    fn evaluates_with_real_maxima() {
        if !maxima_available() {
            return;
        }
        let mut s = Session::new(Box::new(NativeBackend::new(NativeConfig::default())));
        let r = s.run_blocking("integrate(x*sin(x)^3,x,0,%pi);", Duration::from_secs(30));
        assert!(r.ok, "{:?}", r);
        assert_eq!(r.last_text(), Some("(2*%pi)/3"));
        let r = s.run_blocking("1/0;", Duration::from_secs(30));
        assert!(!r.ok);
        assert!(r.error().unwrap().contains("undefined"), "{:?}", r);
        // State persists and variables are reported.
        s.run_blocking("a:5$ f(x):=x^2$", Duration::from_secs(30));
        s.refresh_variables();
        for _ in 0..200 {
            s.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(s.variables.iter().any(|v| v.name == "a" && v.value == "5" && !v.is_function), "{:?}", s.variables);
        assert!(s.variables.iter().any(|v| v.name == "f" && v.is_function));
        s.reset_variables();
        for _ in 0..200 {
            s.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(s.variables.is_empty(), "{:?}", s.variables);
    }

    #[test]
    fn interrupt_stops_long_computation() {
        if !maxima_available() {
            return;
        }
        let mut s = Session::new(Box::new(NativeBackend::new(NativeConfig::default())));
        let r = s.run_blocking("block([i:0], while true do i:i+1);", Duration::from_secs(2));
        assert!(!r.ok);
        let r = s.run_blocking("2+2;", Duration::from_secs(30));
        assert_eq!(r.last_text(), Some("4"), "{:?}", r);
    }
}

#[cfg(test)]
mod plot_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn plot_calls_become_plot_output() {
        if Command::new("maxima").arg("--version").output().is_err() {
            return;
        }
        let mut s = Session::new(Box::new(NativeBackend::new(NativeConfig::default())));
        let r = s.run_blocking("plot2d(sin(x),[x,0,3])$", Duration::from_secs(30));
        assert!(r.ok, "{:?}", r);
        match &r.items[0] {
            OutputItem::Plot(p) => {
                assert_eq!(p.series.len(), 1);
                assert!(p.series[0].pts.len() > 100);
                assert!(p.series[0].pts.iter().flatten().all(|(_, y)| y.abs() <= 1.0));
            }
            o => panic!("{:?}", o),
        }
    }
}
