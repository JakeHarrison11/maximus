//! Browser-only platform services.
//!
//! Integration:
//! - Declare `#[cfg(target_arch = "wasm32")] mod web_platform;` in lib.rs.
//! - Add wasm-only `js-sys = "0.3"`; existing wasm-bindgen, wasm-bindgen-futures,
//!   serde, serde_json and eframe suffice. No additional web-sys features are needed:
//!   DOM, localStorage, IndexedDB, File and Blob access are entirely in platform.js.
//! - Construct one Platform with the egui context. Poll `take_events` each frame,
//!   display Error events, and wait for Initialized before requesting restore.
//!   Do not autosave the initial/default notebook before NotebookRestored arrives.
//!   All storage operations additionally await initialization internally.
//!   Alternatively, `initialize().await` and `load_notebook().await` before app
//!   construction to restore startup state without polling.
//! - Apply settings via `load_settings::<Settings>()`; persist via `save_settings`.
//! - File pickers/export must be called directly from a user click handler.
//! - On FilesChanged, call `plugins()` to rebuild menus/palettes and restart/remount
//!   the CAS when appropriate. Stop must NOT remove plugins or workspace files.
//! - The backend bridge should `await window.maximusRuntimeFiles()` before each CAS
//!   initialization/restart; it returns {path, data: number[]} snapshots. Create parent
//!   directories and mount these files. Direct JS imports can also use
//!   `getRuntimeFiles()` (its byte field is named `bytes`, not `data`).
//!   Arrays contain unsigned bytes, not base64. Workspace paths begin /workspace/.
//!   `runtime_files()` gives the main-thread snapshot after Initialized/FilesChanged.
//!
//! Persistence is origin-scoped and subject to browser quota/eviction/private mode.
//! Errors are both queued for the UI and logged to the browser console. Failed
//! initialization requires reload; no volatile fallback hides failed persistence.
//! Concurrent tabs use last-writer-wins snapshots; use one editing tab per origin.
//! Plugin directory selection requires browser support for webkitdirectory.

use eframe::egui;
use maximus_core::{plugin::LoadedPlugin, Notebook};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::PathBuf;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};

#[wasm_bindgen(module = "/web/platform.js")]
extern "C" {
    #[wasm_bindgen(js_name = setRepaintCallback)]
    fn set_repaint_callback(callback: &JsValue);
    #[wasm_bindgen(js_name = initPlatform)]
    fn init_platform() -> js_sys::Promise;
    #[wasm_bindgen(js_name = takeEvents)]
    fn take_js_events() -> String;
    #[wasm_bindgen(js_name = reportError)]
    fn report_error(operation: &str, message: &str);
    #[wasm_bindgen(catch, js_name = loadSettings)]
    fn load_js_settings() -> Result<Option<String>, JsValue>;
    #[wasm_bindgen(catch, js_name = saveSettings)]
    fn save_js_settings(json: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = restoreNotebook)]
    fn restore_js_notebook() -> js_sys::Promise;
    #[wasm_bindgen(js_name = loadNotebook)]
    fn load_js_notebook() -> js_sys::Promise;
    #[wasm_bindgen(js_name = saveNotebook)]
    fn save_js_notebook(contents: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = openNotebook)]
    fn open_js_notebook() -> js_sys::Promise;
    #[wasm_bindgen(catch, js_name = exportNotebook)]
    fn export_js_notebook(name: &str, contents: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = exportText)]
    fn export_js_text(name: &str, contents: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = exportBytes)]
    fn export_js_bytes(name: &str, contents: &js_sys::Uint8Array) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = selectPluginDirectory)]
    fn select_plugin_directory() -> js_sys::Promise;
    #[wasm_bindgen(js_name = commitPluginFiles)]
    fn commit_plugin_files(json: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = uploadWorkspace)]
    fn upload_workspace(directory: bool) -> js_sys::Promise;
    #[wasm_bindgen(js_name = removePlugin)]
    fn remove_js_plugin(folder: &str) -> js_sys::Promise;
    #[wasm_bindgen(catch, js_name = runtimeFilesSnapshot)]
    fn runtime_files_snapshot() -> Result<String, JsValue>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Initialized,
    /// Validate with Notebook::from_json before replacing app state.
    NotebookRestored {
        contents: Option<String>,
    },
    NotebookSaved,
    /// Opening a notebook does not autosave or replace the previous recovery copy.
    NotebookOpened {
        name: String,
        contents: String,
    },
    FilesChanged,
    Error {
        operation: String,
        message: String,
    },
    /// Internal candidate, consumed by Platform::take_events after TOML validation.
    PluginSelected {
        folder: String,
        files: Vec<RuntimeFile>,
    },
}

/// One main-thread instance; owns the repaint callback for its entire lifetime.
pub struct Platform {
    _repaint: Closure<dyn Fn()>,
}

fn launch(promise: js_sys::Promise) {
    spawn_local(async move {
        // JS logs and queues every rejected operation; do not report twice here.
        let _ = JsFuture::from(promise).await;
    });
}

fn js_error(error: JsValue) -> String {
    error.as_string().unwrap_or_else(|| {
        error
            .dyn_ref::<js_sys::Error>()
            .map(|e| String::from(e.message()))
            .unwrap_or_else(|| format!("{error:?}"))
    })
}

fn checked<T>(operation: &str, result: Result<T, String>) -> Result<T, String> {
    result.map_err(|message| {
        report_error(operation, &message);
        message
    })
}

/// Complete persistent file initialization before constructing the app/backend.
pub async fn initialize() -> Result<(), String> {
    JsFuture::from(init_platform()).await.map_err(js_error)?;
    Ok(())
}

/// Restore and validate the recovery notebook before constructing app state.
/// `None` means no recovery copy exists. Errors must not trigger saving a default
/// notebook over the previous recovery copy without an explicit user decision.
pub async fn load_notebook() -> Result<Option<Notebook>, String> {
    let value = JsFuture::from(load_js_notebook()).await.map_err(js_error)?;
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    checked(
        "decode recovery notebook",
        (|| {
            let contents = value.as_string().ok_or("Stored notebook is not text")?;
            Notebook::from_json(&contents).map(Some)
        })(),
    )
}

impl Platform {
    pub fn new(ctx: egui::Context) -> Self {
        let repaint = Closure::wrap(Box::new(move || ctx.request_repaint()) as Box<dyn Fn()>);
        set_repaint_callback(repaint.as_ref());
        launch(init_platform());
        Self { _repaint: repaint }
    }

    pub fn load_settings<T: DeserializeOwned>(&self) -> Result<Option<T>, String> {
        let json = load_js_settings().map_err(js_error)?;
        checked(
            "decode settings",
            json.map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
                .transpose(),
        )
    }

    pub fn save_settings<T: Serialize>(&self, settings: &T) -> Result<(), String> {
        let json = checked(
            "encode settings",
            serde_json::to_string(settings).map_err(|e| e.to_string()),
        )?;
        save_js_settings(&json).map_err(js_error)
    }

    pub fn restore_notebook(&self) {
        launch(restore_js_notebook());
    }

    /// Call after restore completes, and debounce changes in the app.
    pub fn autosave_notebook(&self, notebook: &Notebook) {
        launch(save_js_notebook(&notebook.to_json()));
    }

    pub fn open_notebook(&self) {
        launch(open_js_notebook());
    }

    pub fn export_notebook(&self, name: &str, notebook: &Notebook) -> Result<(), String> {
        export_js_notebook(name, &notebook.to_json()).map_err(js_error)
    }

    pub fn export_text(&self, name: &str, text: &str) -> Result<(), String> {
        export_js_text(name, text).map_err(js_error)
    }

    pub fn export_bytes(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let contents = js_sys::Uint8Array::from(bytes);
        export_js_bytes(name, &contents).map_err(js_error)
    }

    pub fn read_settings<T: DeserializeOwned>() -> Result<Option<T>, String> {
        let json = load_js_settings().map_err(js_error)?;
        checked("decode settings", json.map(|s| serde_json::from_str(&s).map_err(|e| e.to_string())).transpose())
    }

    pub fn write_settings<T: Serialize>(settings: &T) -> Result<(), String> {
        let json = checked("encode settings", serde_json::to_string(settings).map_err(|e| e.to_string()))?;
        save_js_settings(&json).map_err(js_error)
    }

    pub fn import_plugin(&self) {
        launch(select_plugin_directory());
    }

    /// `directory = false`: multiple CAS files; true: preserve a selected directory.
    pub fn upload_workspace(&self, directory: bool) {
        launch(upload_workspace(directory));
    }

    pub fn remove_plugin(&self, folder: &str) {
        launch(remove_js_plugin(folder));
    }

    pub fn runtime_files(&self) -> Result<Vec<RuntimeFile>, String> {
        let json = runtime_files_snapshot().map_err(js_error)?;
        checked(
            "decode runtime files",
            serde_json::from_str(&json).map_err(|e| e.to_string()),
        )
    }

    /// Includes persisted TOML menus, palettes, snippets, init and Maxima file paths.
    /// A malformed persisted manifest is returned as an error, never silently skipped.
    pub fn plugins(&self) -> Result<Vec<LoadedPlugin>, String> {
        let files = self.runtime_files()?;
        checked(
            "list plugins",
            (|| {
                let mut plugins = Vec::new();
                for file in &files {
                    if let Some(folder) = file
                        .path
                        .strip_prefix("/plugins/")
                        .and_then(|p| p.strip_suffix("/plugin.toml"))
                    {
                        if !folder.contains('/') {
                            plugins.push(validate_plugin(folder, &files)?);
                        }
                    }
                }
                Ok(plugins)
            })(),
        )
    }

    pub fn take_events(&self) -> Vec<Event> {
        let events: Vec<Event> = match checked(
            "decode platform events",
            serde_json::from_str(&take_js_events()).map_err(|e| e.to_string()),
        ) {
            Ok(events) => events,
            Err(_) => return Vec::new(),
        };
        events
            .into_iter()
            .filter_map(|event| {
                if let Event::PluginSelected { folder, files } = event {
                    if checked("validate plugin", validate_plugin(&folder, &files)).is_ok() {
                        if let Ok(json) = checked(
                            "encode plugin",
                            serde_json::to_string(&files).map_err(|e| e.to_string()),
                        ) {
                            launch(commit_plugin_files(&json));
                        }
                    }
                    None
                } else {
                    Some(event)
                }
            })
            .collect()
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        set_repaint_callback(&JsValue::NULL);
    }
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':' | '"'))
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("Unsafe relative path: {path}"));
    }
    Ok(())
}

fn validate_plugin(folder: &str, files: &[RuntimeFile]) -> Result<LoadedPlugin, String> {
    validate_relative_path(folder)?;
    if folder.contains('/') {
        return Err("Expected one plugin folder".into());
    }
    let prefix = format!("/plugins/{folder}/");
    let manifest_path = format!("{prefix}plugin.toml");
    let manifest = files
        .iter()
        .find(|f| f.path == manifest_path)
        .ok_or("Missing plugin.toml")?;
    let text = std::str::from_utf8(&manifest.bytes).map_err(|e| e.to_string())?;
    let manifest = maximus_core::plugin::parse_manifest(text)?;
    for path in &manifest.maxima_files {
        validate_relative_path(path)?;
        if !files.iter().any(|f| f.path == format!("{prefix}{path}")) {
            return Err(format!("Plugin references missing file: {path}"));
        }
    }
    Ok(LoadedPlugin {
        manifest,
        dir: PathBuf::from(format!("/plugins/{folder}")),
        enabled: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_paths() {
        for path in [
            "", "/a", "../a", "a/../b", "a//b", "a\\b", "a/", "a:\"b", "a\nb",
        ] {
            assert!(validate_relative_path(path).is_err(), "{path:?}");
        }
        assert!(validate_relative_path("Physics/sub dir/α.mac").is_ok());
    }

    #[test]
    fn validates_existing_manifest_and_referenced_files() {
        let mut files = vec![RuntimeFile {
            path: "/plugins/physics/plugin.toml".into(),
            bytes: b"name = 'Physics'\nmaxima_files = ['sub/constants.mac']".to_vec(),
        }];
        assert!(validate_plugin("physics", &files).is_err());
        files.push(RuntimeFile {
            path: "/plugins/physics/sub/constants.mac".into(),
            bytes: vec![],
        });
        let plugin = validate_plugin("physics", &files).unwrap();
        assert_eq!(plugin.manifest.name, "Physics");
        assert_eq!(
            plugin.startup_code(),
            vec!["load(\"/plugins/physics/sub/constants.mac\")$"]
        );
        files[0].bytes = b"name = 'Unsafe'\nmaxima_files = ['../escape.mac']".to_vec();
        assert!(validate_plugin("physics", &files).is_err());
    }
}
