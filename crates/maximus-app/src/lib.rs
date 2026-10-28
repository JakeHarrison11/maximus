//! Maximus GUI application.

mod app;
mod catalog;
mod cells;
mod minimal;
mod settings;
mod side;

pub use app::{MaximusApp, Options};

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native(opts: Options) -> eframe::Result {
    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1180.0, 800.0]).with_min_inner_size([420.0, 480.0]).with_title("Maximus"),
        ..Default::default()
    };
    eframe::run_native("Maximus", native, Box::new(move |cc| Ok(Box::new(MaximusApp::new(cc, opts)))))
}

/// Headless MCP server on stdin/stdout.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_mcp(opts: Options) -> std::io::Result<()> {
    use maximus_core::plugin::load_plugins;
    let settings = settings::Settings::load();
    let cfg = maximus_backend_native::NativeConfig {
        maxima_path: opts.maxima_path.or(if settings.maxima_path.trim().is_empty() { None } else { Some(settings.maxima_path.clone()) }),
        extra_args: vec![],
    };
    let help = maximus_help::HelpIndex::load_system();
    maximus_math::words::register_functions(help.entries.iter().filter(|e| e.is_function()).map(|e| e.name.clone()));
    let mut session = maximus_core::Session::new(Box::new(maximus_backend_native::NativeBackend::new(cfg)));
    let dir = std::env::var("MAXIMUS_PLUGIN_DIR").ok().map(std::path::PathBuf::from).or_else(settings::plugin_dir);
    if let Some(dir) = dir {
        let (plugins, _) = load_plugins(&dir);
        for p in plugins.iter().filter(|p| !settings.disabled_plugins.contains(&p.manifest.name)) {
            for code in p.startup_code() {
                session.submit_raw(&code);
            }
        }
    }
    let mut server = maximus_mcp::McpServer::new(session, help);
    let stdin = std::io::stdin();
    server.serve(stdin.lock(), std::io::stdout())
}

/// Browser entry point: `maximus_app.start("canvas-id")` (see `web/index.html`).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    let canvas = document
        .get_element_by_id(&canvas_id)
        .ok_or("canvas not found")?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| "element is not a canvas")?;
    eframe::WebRunner::new()
        .start(canvas, eframe::WebOptions::default(), Box::new(|cc| Ok(Box::new(MaximusApp::new(cc, Options::default())))))
        .await
}
