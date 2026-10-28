use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlotBackend {
    Egui,
    Gnuplot,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub dark: bool,
    /// Font size of textbook math in points.
    pub math_size: f32,
    /// Empty means search `PATH`.
    pub maxima_path: String,
    pub plot_backend: PlotBackend,
    pub gnuplot_path: String,
    pub minimal: bool,
    pub side_panel: bool,
    pub contextual_help: bool,
    pub disabled_plugins: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dark: true,
            math_size: 20.0,
            maxima_path: String::new(),
            plot_backend: PlotBackend::Egui,
            gnuplot_path: "gnuplot".into(),
            minimal: false,
            side_panel: true,
            contextual_help: true,
            disabled_plugins: Vec::new(),
        }
    }
}

pub fn config_dir() -> Option<PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        dirs::config_dir().map(|d| d.join("maximus"))
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

pub fn data_dir() -> Option<PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        dirs::data_dir().map(|d| d.join("maximus"))
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

pub fn plugin_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("plugins"))
}

impl Settings {
    pub fn load() -> Settings {
        config_dir()
            .and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(d) = config_dir() {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("settings.json"), serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }
}
