use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlotBackend {
    Egui,
    Gnuplot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SideTab {
    Help,
    Variables,
    Symbols,
    Functions,
}

pub const DEFAULT_FULL_SIZE: [f32; 2] = [1180.0, 800.0];
pub const DEFAULT_MINIMAL_SIZE: [f32; 2] = [460.0, 740.0];
/// Smallest window in which each mode stays usable, in points (logical pixels at 100% UI scale).
pub const MIN_FULL_SIZE: [f32; 2] = [640.0, 440.0];
pub const MIN_MINIMAL_SIZE: [f32; 2] = [400.0, 500.0];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Last selected side panel tab.
    pub side_tab: SideTab,
    pub dark: bool,
    /// Overall interface scale (egui zoom factor).
    pub ui_scale: f32,
    /// Font size of textbook math in points.
    pub math_size: f32,
    /// Empty means search `PATH`.
    pub maxima_path: String,
    pub plot_backend: PlotBackend,
    pub gnuplot_path: String,
    pub minimal: bool,
    pub side_panel: bool,
    /// Share of the window width taken by the sidebar.
    pub side_fraction: f32,
    pub contextual_help: bool,
    pub disabled_plugins: Vec<String>,
    /// Folder of the last notebook that was opened, saved or exported; the file picker starts there.
    pub last_dir: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            side_tab: SideTab::Help,
            dark: true,
            ui_scale: 1.0,
            math_size: 20.0,
            maxima_path: String::new(),
            plot_backend: PlotBackend::Egui,
            gnuplot_path: "gnuplot".into(),
            minimal: false,
            side_panel: true,
            side_fraction: 0.3,
            contextual_help: true,
            disabled_plugins: Vec::new(),
            last_dir: None,
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
        let mut s: Settings = config_dir()
            .and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.sanitize();
        s
    }

    /// Replaces out-of-range saved values with usable ones.
    pub fn sanitize(&mut self) {
        self.ui_scale = self.ui_scale.clamp(0.5, 3.0);
        self.side_fraction = self.side_fraction.clamp(0.15, 0.5);
    }

    pub fn save(&self) {
        if let Some(d) = config_dir() {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("settings.json"), serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }
}

/// Minimum window size in logical pixels; a larger UI scale needs a proportionally larger window.
pub fn min_size(minimal: bool, ui_scale: f32) -> [f32; 2] {
    let base = if minimal { MIN_MINIMAL_SIZE } else { MIN_FULL_SIZE };
    [(base[0] * ui_scale).ceil(), (base[1] * ui_scale).ceil()]
}

/// Raises a window size to the minimum for the mode and UI scale.
pub fn clamp_size(size: [f32; 2], minimal: bool, ui_scale: f32) -> [f32; 2] {
    let min = min_size(minimal, ui_scale);
    [size[0].max(min[0]), size[1].max(min[1])]
}

/// Window size the app starts with in each mode (logical pixels).
pub fn default_size(minimal: bool, ui_scale: f32) -> [f32; 2] {
    clamp_size(if minimal { DEFAULT_MINIMAL_SIZE } else { DEFAULT_FULL_SIZE }, minimal, ui_scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_raised_to_the_minimum_which_grows_with_ui_scale() {
        assert_eq!(clamp_size([100.0, 100.0], true, 1.0), MIN_MINIMAL_SIZE);
        assert_eq!(clamp_size([100.0, 100.0], true, 2.0), [800.0, 1000.0]);
        assert_eq!(clamp_size([900.0, 700.0], false, 1.0), [900.0, 700.0]);
        assert_eq!(default_size(false, 1.0), DEFAULT_FULL_SIZE);
        assert_eq!(default_size(false, 3.0), [1920.0, 1320.0]);
    }
}
