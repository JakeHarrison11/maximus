//! Model Context Protocol server (JSON-RPC 2.0, newline-delimited over stdio).
//! It drives a headless [`Session`], so agents see the same engine the GUI uses.

use maximus_core::*;
use maximus_help::HelpIndex;
use maximus_math::{latex_to_maxima, maxima_to_latex};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::time::Duration;

const PROTOCOL: &str = "2024-11-05";

pub struct McpServer {
    pub session: Session,
    pub help: HelpIndex,
    pub timeout: Duration,
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn text_result(v: Value, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}], "isError": is_error})
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn output_json(o: &OutputItem) -> Value {
    match o {
        OutputItem::Text(t) => json!({"kind": "text", "text": t}),
        OutputItem::Error(e) => json!({"kind": "error", "message": e}),
        OutputItem::Result { label, text, tex } => {
            let latex = maxima_to_latex(text).unwrap_or_else(|_| tex.trim_matches('$').to_string());
            json!({"kind": "result", "label": label, "text": text, "latex": latex})
        }
        OutputItem::Plot(p) => {
            json!({"kind": "plot", "plot_kind": p.kind, "series": p.series.iter().map(|s| json!({"label": s.label, "points": s.pts.len()})).collect::<Vec<_>>()})
        }
    }
}

impl McpServer {
    pub fn new(session: Session, help: HelpIndex) -> Self {
        McpServer { session, help, timeout: Duration::from_secs(60) }
    }

    pub fn tools() -> Value {
        let s = |desc: &str, props: Value, req: Vec<&str>| json!({"type": "object", "description": desc, "properties": props, "required": req});
        json!([
            {"name": "evaluate", "description": "Evaluate a Maxima expression. `format` is `maxima` (default) or `latex` (textbook math such as \\int_0^\\infty x\\sin^3 x\\,dx). State persists between calls.",
             "inputSchema": s("", json!({"input": {"type": "string"}, "format": {"type": "string", "enum": ["maxima", "latex"]}}), vec!["input"])},
            {"name": "latex_to_maxima", "description": "Translate textbook LaTeX math into Maxima syntax without evaluating it.",
             "inputSchema": s("", json!({"latex": {"type": "string"}}), vec!["latex"])},
            {"name": "maxima_to_latex", "description": "Translate a Maxima expression into LaTeX.",
             "inputSchema": s("", json!({"maxima": {"type": "string"}}), vec!["maxima"])},
            {"name": "list_variables", "description": "List user-defined variables and functions in the current session.",
             "inputSchema": s("", json!({}), vec![])},
            {"name": "reset", "description": "Forget all user-defined variables and functions (kill(all)).",
             "inputSchema": s("", json!({}), vec![])},
            {"name": "help", "description": "Look up Maxima documentation for a function or variable.",
             "inputSchema": s("", json!({"topic": {"type": "string"}}), vec!["topic"])},
            {"name": "search_help", "description": "Search Maxima function names.",
             "inputSchema": s("", json!({"query": {"type": "string"}}), vec!["query"])},
            {"name": "plot", "description": "Plot one or more expressions of x. Returns a PNG image when gnuplot is installed, and sampled data summary otherwise.",
             "inputSchema": s("", json!({"expressions": {"type": "array", "items": {"type": "string"}}, "variable": {"type": "string"}, "min": {"type": "number"}, "max": {"type": "number"}}), vec!["expressions"])},
            {"name": "notebook_add_cell", "description": "Append a code cell to the notebook, optionally evaluating it. Useful for demonstrations.",
             "inputSchema": s("", json!({"source": {"type": "string"}, "format": {"type": "string", "enum": ["maxima", "latex"]}, "evaluate": {"type": "boolean"}}), vec!["source"])},
            {"name": "notebook_get", "description": "Return all notebook cells with their inputs and outputs.",
             "inputSchema": s("", json!({}), vec![])},
            {"name": "notebook_clear", "description": "Remove all cells from the notebook.",
             "inputSchema": s("", json!({}), vec![])}
        ])
    }

    fn translate(&self, input: &str, format: &str) -> Result<String, String> {
        if format == "latex" {
            latex_to_maxima(input, &self.session.parse_ctx())
        } else {
            Ok(maximus_core::ensure_terminated(input))
        }
    }

    fn eval_value(&mut self, input: &str, format: &str) -> Value {
        match self.translate(input, format) {
            Err(e) => text_result(json!({"ok": false, "error": format!("Input error: {}", e)}), true),
            Ok(code) => {
                let r = self.session.run_blocking(&code, self.timeout);
                self.session.refresh_variables();
                text_result(json!({"ok": r.ok, "maxima_input": code, "outputs": r.items.iter().map(output_json).collect::<Vec<_>>()}), !r.ok)
            }
        }
    }

    fn sync_variables(&mut self) {
        self.session.refresh_variables();
        for _ in 0..2000 {
            self.session.tick();
            if !self.session.is_busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn call_tool(&mut self, name: &str, args: &Value) -> Value {
        let missing = |k: &str| text_result(json!({"ok": false, "error": format!("missing argument `{}`", k)}), true);
        match name {
            "evaluate" => {
                let Some(input) = str_arg(args, "input") else { return missing("input") };
                self.eval_value(input, str_arg(args, "format").unwrap_or("maxima"))
            }
            "latex_to_maxima" => {
                let Some(l) = str_arg(args, "latex") else { return missing("latex") };
                match latex_to_maxima(l, &self.session.parse_ctx()) {
                    Ok(m) => text_result(json!({"maxima": m}), false),
                    Err(e) => text_result(json!({"error": e}), true),
                }
            }
            "maxima_to_latex" => {
                let Some(m) = str_arg(args, "maxima") else { return missing("maxima") };
                match maxima_to_latex(m) {
                    Ok(l) => text_result(json!({"latex": l}), false),
                    Err(e) => text_result(json!({"error": e}), true),
                }
            }
            "list_variables" => {
                self.sync_variables();
                let v: Vec<Value> = self.session.variables.iter().map(|v| json!({"name": v.name, "value": v.value, "is_function": v.is_function})).collect();
                text_result(json!({"variables": v}), false)
            }
            "reset" => {
                self.session.reset_variables();
                self.sync_variables();
                text_result(json!({"ok": true}), false)
            }
            "help" => {
                let Some(t) = str_arg(args, "topic") else { return missing("topic") };
                let entries = self.help.lookup(t);
                if entries.is_empty() {
                    let sugg: Vec<&str> = self.help.search(t, 8).iter().map(|e| e.name.as_str()).collect();
                    return text_result(json!({"found": false, "suggestions": sugg}), true);
                }
                let v: Vec<Value> = entries.iter().map(|e| json!({"name": e.name, "kind": e.kind, "signatures": e.signatures, "category": e.category, "documentation": e.body})).collect();
                text_result(json!({"found": true, "entries": v}), false)
            }
            "search_help" => {
                let Some(q) = str_arg(args, "query") else { return missing("query") };
                let v: Vec<Value> = self.help.search(q, 25).iter().map(|e| json!({"name": e.name, "kind": e.kind, "signature": e.signatures.first()})).collect();
                text_result(json!({"results": v}), false)
            }
            "plot" => self.plot(args),
            "notebook_add_cell" => {
                let Some(src) = str_arg(args, "source") else { return missing("source") };
                let mode = if str_arg(args, "format") == Some("latex") { InputMode::Math } else { InputMode::Maxima };
                let id = self.session.notebook.add_cell(CellKind::Code, mode);
                self.session.notebook.cell_mut(id).unwrap().source = src.to_string();
                if args.get("evaluate").and_then(Value::as_bool).unwrap_or(true) {
                    self.session.evaluate_cell(id);
                    let start = std::time::Instant::now();
                    while self.session.notebook.cell(id).map_or(false, |c| matches!(c.state, CellState::Queued | CellState::Running)) {
                        self.session.tick();
                        if start.elapsed() > self.timeout {
                            self.session.interrupt();
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                let c = self.session.notebook.cell(id).unwrap();
                text_result(json!({"cell_id": id, "outputs": c.outputs.iter().map(output_json).collect::<Vec<_>>()}), c.state == CellState::Failed)
            }
            "notebook_get" => {
                let cells: Vec<Value> = self
                    .session
                    .notebook
                    .cells
                    .iter()
                    .map(|c| json!({"id": c.id, "kind": format!("{:?}", c.kind), "format": if c.mode == InputMode::Math { "latex" } else { "maxima" }, "source": c.source, "outputs": c.outputs.iter().map(output_json).collect::<Vec<_>>()}))
                    .collect();
                text_result(json!({"cells": cells}), false)
            }
            "notebook_clear" => {
                self.session.notebook = Notebook::empty();
                text_result(json!({"ok": true}), false)
            }
            other => text_result(json!({"error": format!("unknown tool `{}`", other)}), true),
        }
    }

    fn plot(&mut self, args: &Value) -> Value {
        let exprs: Vec<String> = args.get("expressions").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        if exprs.is_empty() {
            return text_result(json!({"error": "missing `expressions`"}), true);
        }
        let var = str_arg(args, "variable").unwrap_or("x");
        let lo = args.get("min").and_then(Value::as_f64).unwrap_or(-10.0);
        let hi = args.get("max").and_then(Value::as_f64).unwrap_or(10.0);
        let code = format!("plot2d([{}],[{},{},{}])$", exprs.join(","), var, lo, hi);
        let r = self.session.run_blocking(&code, self.timeout);
        let Some(OutputItem::Plot(p)) = r.items.iter().find(|i| matches!(i, OutputItem::Plot(_))).cloned() else {
            return text_result(json!({"ok": false, "outputs": r.items.iter().map(output_json).collect::<Vec<_>>()}), true);
        };
        let png = std::env::temp_dir().join(format!("maximus-mcp-{}.png", std::process::id()));
        if maximus_plot::gnuplot::render(&p, "gnuplot", &png).is_ok() {
            if let Ok(bytes) = std::fs::read(&png) {
                let _ = std::fs::remove_file(&png);
                return json!({"content": [
                    {"type": "image", "data": base64(&bytes), "mimeType": "image/png"},
                    {"type": "text", "text": format!("Plot of {} over {} in [{}, {}]", exprs.join(", "), var, lo, hi)}
                ], "isError": false});
            }
        }
        let summary: Vec<Value> = p.series.iter().map(|s| {
            let ys: Vec<f64> = s.pts.iter().flatten().map(|p| p.1).collect();
            json!({"label": s.label, "points": s.pts.len(), "min": ys.iter().cloned().fold(f64::INFINITY, f64::min), "max": ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)})
        }).collect();
        text_result(json!({"ok": true, "note": "gnuplot is not available, returning data summary", "series": summary}), false)
    }

    /// Handles one JSON-RPC message; returns the response, if any.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let method = msg.get("method").and_then(Value::as_str)?;
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result: Result<Value, (i64, String)> = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "maximus", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Maximus exposes the Maxima computer algebra system. Use `evaluate` with format `latex` for textbook-style input."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": Self::tools()})),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                Ok(self.call_tool(name, &args))
            }
            m if m.starts_with("notifications/") => return None,
            m => Err((-32601, format!("method not found: {}", m))),
        };
        let id = id?;
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        })
    }

    pub fn serve<R: BufRead, W: Write>(&mut self, input: R, mut out: W) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let resp = match serde_json::from_str::<Value>(&line) {
                Ok(v) => self.handle(&v),
                Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}})),
            };
            if let Some(r) = resp {
                writeln!(out, "{}", r)?;
                out.flush()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maximus_backend_native::{NativeBackend, NativeConfig};

    fn have_maxima() -> bool {
        std::process::Command::new("maxima").arg("--version").output().is_ok()
    }

    fn server() -> McpServer {
        let session = Session::new(Box::new(NativeBackend::new(NativeConfig::default())));
        McpServer::new(session, HelpIndex::load_system())
    }

    fn call(s: &mut McpServer, name: &str, args: Value) -> Value {
        let resp = s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}})).unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap().to_string();
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    }

    #[test]
    fn protocol_basics() {
        let mut s = McpServer::new(Session::new(Box::new(NullBackend::new("none"))), HelpIndex::default());
        let r = s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})).unwrap();
        assert_eq!(r["result"]["serverInfo"]["name"], "maximus");
        assert!(s.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
        let r = s.handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
        assert!(r["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "evaluate"));
        let r = call(&mut s, "latex_to_maxima", json!({"latex": "\\int_0^{\\infty}x\\sin^3\\left(x\\right)\\,\\mathrm{d}x"}));
        assert_eq!(r["maxima"], "integrate(x*sin(x)^3,x,0,inf);");
    }

    #[test]
    fn evaluates_both_spec_examples_identically() {
        if !have_maxima() {
            return;
        }
        let mut s = server();
        let a = call(&mut s, "evaluate", json!({"input": "\\int_0^{1}\\!x\\sin\\left(x\\right)\\,\\mathrm{d}x", "format": "latex"}));
        let b = call(&mut s, "evaluate", json!({"input": "integrate(x*sin(x),x,0,1)", "format": "maxima"}));
        assert_eq!(a["ok"], true, "{a}");
        assert_eq!(a["outputs"][0]["text"], b["outputs"][0]["text"]);
        assert!(a["outputs"][0]["latex"].as_str().unwrap().contains("sin"));
        let _ = call(&mut s, "evaluate", json!({"input": "k:42$"}));
        let v = call(&mut s, "list_variables", json!({}));
        assert!(v["variables"].as_array().unwrap().iter().any(|x| x["name"] == "k"), "{v}");
        call(&mut s, "reset", json!({}));
        let v = call(&mut s, "list_variables", json!({}));
        assert!(v["variables"].as_array().unwrap().is_empty(), "{v}");
        let h = call(&mut s, "help", json!({"topic": "integrate"}));
        if h["found"] == true {
            assert!(h["entries"][0]["signatures"][0].as_str().unwrap().contains("integrate"));
        }
    }

    #[test]
    fn plot_tool_returns_image_or_summary() {
        if !have_maxima() {
            return;
        }
        let mut s = server();
        let r = s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "plot", "arguments": {"expressions": ["sin(x)"], "min": 0, "max": 6}}})).unwrap();
        assert_eq!(r["result"]["isError"], false, "{r}");
    }
}
