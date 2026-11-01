//! Browser worker transport for the local ECL/Maxima runtime.

#[cfg(target_arch = "wasm32")]
mod browser {
    use maximus_core::*;
    use serde::Deserialize;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(module = "/web/bridge.js")]
    extern "C" {
        type MaximaEngine;
        #[wasm_bindgen(constructor, catch)]
        fn new() -> Result<MaximaEngine, JsValue>;
        #[wasm_bindgen(method)]
        fn submit(this: &MaximaEngine, id: &str, kind: &str, code: &str);
        #[wasm_bindgen(method, js_name = takeEvents)]
        fn take_events(this: &MaximaEngine) -> String;
        #[wasm_bindgen(method)]
        fn dispose(this: &MaximaEngine);
    }

    #[derive(Deserialize)]
    #[serde(tag = "type", rename_all = "lowercase")]
    enum Message {
        Ready,
        Frame { line: String },
        Error { message: String },
    }

    pub struct WebBackend {
        engine: Option<MaximaEngine>,
        status: BackendStatus,
        pending: Vec<Event>,
    }

    impl Default for WebBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    impl WebBackend {
        pub fn new() -> Self {
            match MaximaEngine::new() {
                Ok(engine) => Self { engine: Some(engine), status: BackendStatus::Starting, pending: Vec::new() },
                Err(error) => Self {
                    engine: None,
                    status: BackendStatus::Dead(format!("Could not create Maxima worker: {error:?}")),
                    pending: Vec::new(),
                },
            }
        }

        fn fail(&mut self, message: String) {
            if let Some(engine) = self.engine.take() {
                engine.dispose();
            }
            self.status = BackendStatus::Dead(message.clone());
            self.pending.push(Event { id: 0, kind: EventKind::Crashed(message) });
        }
    }

    impl Backend for WebBackend {
        fn name(&self) -> String { "Maxima (local WebAssembly)".into() }
        fn status(&self) -> BackendStatus { self.status.clone() }
        fn submit(&mut self, id: RequestId, request: RequestKind) {
            if let Some(engine) = &self.engine {
                match request {
                    RequestKind::Eval(code) => engine.submit(&id.to_string(), "eval", &code),
                    RequestKind::Vars => engine.submit(&id.to_string(), "vars", ""),
                }
            } else {
                let message = match &self.status {
                    BackendStatus::Dead(message) => message.clone(),
                    _ => "Maxima worker is unavailable".into(),
                };
                self.pending.push(Event { id, kind: EventKind::Error(message) });
                self.pending.push(Event { id, kind: EventKind::Done { ok: false } });
            }
        }

        fn poll(&mut self) -> Vec<Event> {
            if let Some(engine) = &self.engine {
                let messages = serde_json::from_str::<Vec<Message>>(&engine.take_events());
                match messages {
                    Err(error) => self.fail(format!("Invalid Maxima worker message: {error}")),
                    Ok(messages) => for message in messages {
                        match message {
                            Message::Ready => {
                                self.status = BackendStatus::Ready;
                                self.pending.push(Event { id: 0, kind: EventKind::Ready });
                            }
                            Message::Frame { line } => match maximus_core::protocol::parse_frame(&line) {
                                Ok(event) => self.pending.push(event),
                                Err(error) => self.fail(format!("Invalid Maxima output: {error}")),
                            },
                            Message::Error { message } => self.fail(message),
                        }
                    },
                }
            }
            std::mem::take(&mut self.pending)
        }

        fn interrupt(&mut self) {
            self.restart();
        }

        fn restart(&mut self) {
            *self = Self::new();
        }
    }

    impl Drop for WebBackend {
        fn drop(&mut self) {
            if let Some(engine) = &self.engine {
                engine.dispose();
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use browser::WebBackend;
