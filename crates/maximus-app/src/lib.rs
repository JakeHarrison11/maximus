//! Maximus GUI application.

mod app;
mod catalog;
mod cells;
mod completion;
mod icons;
mod minimal;
mod pdf_export;
mod settings;
mod side;
#[cfg(target_arch = "wasm32")]
mod web_platform;

pub use app::{MaximusApp, Options};

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native(opts: Options) -> eframe::Result {
    let mut saved = settings::Settings::load();
    if opts.minimal {
        saved.minimal = true;
    }
    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size(settings::default_size(saved.minimal, saved.ui_scale)).with_min_inner_size(settings::min_size(saved.minimal, saved.ui_scale)).with_title("Maximus").with_app_id("io.github.jakeharrison11.Maximus").with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/maximus.png")).expect("valid app icon")),
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
    let platform_ready = web_platform::initialize().await;
    let recovery = if platform_ready.is_ok() { web_platform::load_notebook().await } else {
        Err("Browser storage initialization failed; autosave is disabled".into())
    };
    let help = load_web_help().await;
    eframe::WebRunner::new()
        .start(canvas, eframe::WebOptions::default(), Box::new(move |cc| {
            let mut app = MaximusApp::new(cc, Options::default());
            match recovery {
                Ok(Some(notebook)) => {
                    app.session.notebook = notebook;
                    app.session.notebook.clear_outputs();
                    app.last_saved_json = app.session.notebook.to_json();
                }
                Ok(None) => {}
                Err(error) => {
                    app.web_autosave = false;
                    app.messages.push(format!("Could not restore notebook: {error}"));
                }
            }
            if let Err(error) = platform_ready { app.messages.push(error); }
            match help {
                Ok(help) => {
                    maximus_math::words::register_functions(help.entries.iter().filter(|e| e.is_function()).map(|e| e.name.clone()));
                    app.manual_cats = help.categories().into_iter()
                        .map(|(category, entries)| (category, entries.iter().map(|e| (e.name.clone(), e.signatures.first().cloned())).collect()))
                        .collect();
                    app.help = help;
                }
                Err(error) => app.messages.push(error),
            }
            Ok(Box::new(app))
        }))
        .await?;
    if let Some(startup) = document.get_element_by_id("startup") {
        startup.remove();
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn load_web_help() -> Result<maximus_help::HelpIndex, String> {
    use wasm_bindgen::JsCast;
    let window = web_sys::window().ok_or("Browser window unavailable")?;
    let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str("runtime/help.info"))
        .await.map_err(|e| format!("Could not load offline help: {e:?}"))?
        .dyn_into::<web_sys::Response>().map_err(|_| "Invalid help response")?;
    if !response.ok() {
        return Err(format!("Help download failed: HTTP {}", response.status()));
    }
    let text = wasm_bindgen_futures::JsFuture::from(response.text().map_err(|e| format!("Could not read help: {e:?}"))?)
        .await.map_err(|e| format!("Could not decode help: {e:?}"))?
        .as_string().ok_or("Help is not text")?;
    let help = maximus_help::HelpIndex::from_info(&text);
    if help.entries.is_empty() {
        return Err("The bundled Maxima help index is empty".into());
    }
    Ok(help)
}
