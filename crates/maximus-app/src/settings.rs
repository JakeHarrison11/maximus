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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Last selected side panel tab.
    pub side_tab: SideTab,
    /// Remembered inner window size for full and minimal mode.
    pub full_size: [f32; 2],
    pub minimal_size: [f32; 2],
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
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            side_tab: SideTab::Help,
            full_size: DEFAULT_FULL_SIZE,
            minimal_size: DEFAULT_MINIMAL_SIZE,
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

    /// Replaces absurd saved values (for example windows shrunk by an earlier bug) with the defaults.
    pub fn sanitize(&mut self) {
        if self.full_size[0] < 640.0 || self.full_size[1] < 480.0 {
            self.full_size = DEFAULT_FULL_SIZE;
        }
        if self.minimal_size[0] < 360.0 || self.minimal_size[1] < 480.0 {
            self.minimal_size = DEFAULT_MINIMAL_SIZE;
        }
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

impl Settings {
    pub fn size_for_mode(&self) -> [f32; 2] {
        if self.minimal {
            self.minimal_size
        } else {
            self.full_size
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrunken_sizes_are_reset_but_sensible_ones_kept() {
        let mut s = Settings { full_size: [410.0, 307.0], minimal_size: [460.0, 740.0], ..Default::default() };
        s.sanitize();
        assert_eq!(s.full_size, DEFAULT_FULL_SIZE);
        assert_eq!(s.minimal_size, [460.0, 740.0]);
        let mut s = Settings { full_size: [900.0, 700.0], ..Default::default() };
        s.sanitize();
        assert_eq!(s.full_size, [900.0, 700.0]);
    }
}
