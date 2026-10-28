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
    /// Remembered inner window size for full and minimal mode.
    pub full_size: [f32; 2],
    pub minimal_size: [f32; 2],
    /// Whether the window was maximized when last closed, for full and minimal mode.
    pub full_maximized: bool,
    pub minimal_maximized: bool,
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
            full_size: DEFAULT_FULL_SIZE,
            minimal_size: DEFAULT_MINIMAL_SIZE,
            full_maximized: false,
            minimal_maximized: false,
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

    /// Raises saved window sizes that are below the usable minimum (for example from an earlier bug).
    pub fn sanitize(&mut self) {
        self.ui_scale = self.ui_scale.clamp(0.5, 3.0);
        self.full_size = clamp_size(self.full_size, min_size(false, self.ui_scale));
        self.minimal_size = clamp_size(self.minimal_size, min_size(true, self.ui_scale));
        self.side_fraction = self.side_fraction.clamp(0.15, 0.5);
    }

    pub fn save(&self) {
        if let Some(d) = config_dir() {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("settings.json"), serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }
}

impl Settings {
    pub fn size_for_mode(&self) -> [f32; 2] {
        clamp_size(if self.minimal { self.minimal_size } else { self.full_size }, self.min_size_for_mode())
    }

    pub fn maximized_for_mode(&self) -> bool {
        if self.minimal {
            self.minimal_maximized
        } else {
            self.full_maximized
        }
    }

    pub fn min_size_for_mode(&self) -> [f32; 2] {
        min_size(self.minimal, self.ui_scale)
    }
}

/// Minimum window size in logical pixels; a larger UI scale needs a proportionally larger window.
pub fn min_size(minimal: bool, ui_scale: f32) -> [f32; 2] {
    let base = if minimal { MIN_MINIMAL_SIZE } else { MIN_FULL_SIZE };
    [(base[0] * ui_scale).ceil(), (base[1] * ui_scale).ceil()]
}

fn clamp_size(size: [f32; 2], min: [f32; 2]) -> [f32; 2] {
    [size[0].max(min[0]), size[1].max(min[1])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undersized_windows_are_raised_to_the_minimum_but_sensible_ones_kept() {
        let mut s = Settings { full_size: [410.0, 307.0], minimal_size: [460.0, 740.0], ..Default::default() };
        s.sanitize();
        assert_eq!(s.full_size, MIN_FULL_SIZE);
        assert_eq!(s.minimal_size, [460.0, 740.0]);
        s.minimal = true;
        s.minimal_size = [100.0, 100.0];
        assert_eq!(s.size_for_mode(), MIN_MINIMAL_SIZE);
        s.ui_scale = 2.0;
        assert_eq!(s.size_for_mode(), [800.0, 1000.0]);
        let mut s = Settings { full_size: [900.0, 700.0], ..Default::default() };
        s.sanitize();
        assert_eq!(s.full_size, [900.0, 700.0]);
    }
}
