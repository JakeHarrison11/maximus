//! The Maximus application: state, frame loop, menus and panels.

use crate::catalog::{self, CallTemplate, Category, PaletteGroup};
use crate::icons::{self, Icon};
pub use crate::settings::SideTab;
use crate::settings::{self, PlotBackend, Settings};
use eframe::egui;
use egui::{Color32, Key, Modifiers, RichText};
use maximus_core::plugin::{load_plugins, LoadedPlugin};
use maximus_core::*;
use maximus_help::HelpIndex;
use maximus_math::{maxima_to_tree, Row};
use maximus_render::editor::{EditorState, Template};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum Insert {
    Symbol(char),
    Template(Template),
    /// Maxima or LaTeX text, parsed into textbook form.
    Text(String),
    /// A Maxima call; optionally evaluated right away.
    Call { text: String, evaluate: bool, pre: Option<String> },
}


pub struct CellUi {
    pub editor: EditorState,
    pub latex_view: bool,
}

pub struct CallDialog {
    pub template: CallTemplate,
    pub values: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Save,
    Open,
    ExportLatex,
    ExportMaxima,
}

pub struct FileDialog {
    pub kind: FileKind,
    pub path: String,
}

#[derive(Default)]
pub struct Options {
    pub open_file: Option<PathBuf>,
    pub minimal: bool,
    pub maxima_path: Option<String>,
    /// Evaluate every cell once Maxima is ready.
    pub run_all: bool,
}

pub struct MaximusApp {
    pub settings: Settings,
    pub session: Session,
    pub help: HelpIndex,
    pub plugins: Vec<LoadedPlugin>,
    pub messages: Vec<String>,
    pub cell_ui: HashMap<u64, CellUi>,
    pub active_cell: Option<u64>,
    pub focus_cell: Option<u64>,
    pub pending: Vec<Insert>,
    pub tab: SideTab,
    pub help_topic: Option<String>,
    pub help_search: String,
    pub func_search: String,
    pub dialog: Option<CallDialog>,
    pub var_edit: Option<(String, String)>,
    pub new_var: (String, String),
    pub tree_cache: HashMap<String, Option<Arc<Row>>>,
    pub gnuplot_cache: HashMap<String, Result<PathBuf, String>>,
    pub file_dialog: Option<FileDialog>,
    pub current_path: Option<PathBuf>,
    pub settings_open: bool,
    pub about_open: bool,
    pub plugins_open: bool,
    pub min_editor: EditorState,
    pub min_scroll_to_end: bool,
    pub catalog: Vec<Category>,
    pub palettes: Vec<PaletteGroup>,
    pub startup_tokens: Vec<(String, u64)>,
    pub last_autosave: f64,
    pub last_saved_json: String,
    pub scroll_to_cell: Option<u64>,
    pub manual_cats: Vec<(String, Vec<(String, Option<String>)>)>,
    pub run_on_start: bool,
    pub show_keypad: bool,
    pub last_mode: bool,
    pub resize_hold_until: f64,
    pub size_dirty_since: Option<f64>,
    pub min_scroll_frames: u32,
}

impl MaximusApp {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        maximus_render::install_fonts(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let mut settings = Settings::load();
        if opts.minimal {
            settings.minimal = true;
        }
        if let Some(p) = &opts.maxima_path {
            settings.maxima_path = p.clone();
        }
        cc.egui_ctx.set_visuals(if settings.dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut style = (*cc.egui_ctx.global_style()).clone();
        style.spacing.item_spacing = egui::vec2(6.0, 6.0);
        cc.egui_ctx.set_global_style(style);

        let help = HelpIndex::load_system();
        maximus_math::words::register_functions(help.entries.iter().filter(|e| e.is_function()).map(|e| e.name.clone()));
        let init_tab = settings.side_tab;
        let last_mode = settings.minimal;
        let session = Session::new(make_backend(&settings));
        let mut app = MaximusApp {
            settings,
            session,
            help,
            plugins: Vec::new(),
            messages: Vec::new(),
            cell_ui: HashMap::new(),
            active_cell: None,
            focus_cell: None,
            pending: Vec::new(),
            tab: init_tab,
            help_topic: Some("integrate".into()),
            help_search: String::new(),
            func_search: String::new(),
            dialog: None,
            var_edit: None,
            new_var: (String::new(), String::new()),
            tree_cache: HashMap::new(),
            gnuplot_cache: HashMap::new(),
            file_dialog: None,
            current_path: None,
            settings_open: false,
            about_open: false,
            plugins_open: false,
            min_editor: EditorState::default(),
            min_scroll_to_end: false,
            catalog: catalog::curated(),
            palettes: catalog::palettes(),
            startup_tokens: Vec::new(),
            last_autosave: 0.0,
            last_saved_json: String::new(),
            scroll_to_cell: None,
            manual_cats: Vec::new(),
            run_on_start: opts.run_all,
            show_keypad: true,
            last_mode,
            resize_hold_until: 0.0,
            size_dirty_since: None,
            min_scroll_frames: 0,
        };
        app.manual_cats = app
            .help
            .categories()
            .into_iter()
            .map(|(c, v)| (c, v.iter().map(|e| (e.name.clone(), e.signatures.first().cloned())).collect()))
            .collect();
        app.load_plugins();
        if let Some(p) = opts.open_file {
            app.open_path(&p);
        } else if let Some(json) = settings::data_dir().and_then(|d| std::fs::read_to_string(d.join("autosave.maximus")).ok()) {
            if let Ok(nb) = Notebook::from_json(&json) {
                if !nb.cells.is_empty() {
                    app.session.notebook = nb;
                }
            }
        }
        app.last_saved_json = app.session.notebook.to_json();
        app
    }

    pub fn load_plugins(&mut self) {
        self.plugins.clear();
        self.startup_tokens.clear();
        let dir = std::env::var("MAXIMUS_PLUGIN_DIR").ok().map(PathBuf::from).or_else(settings::plugin_dir);
        if let Some(dir) = dir {
            let (mut loaded, errs) = load_plugins(&dir);
            self.messages.extend(errs);
            for p in &mut loaded {
                p.enabled = !self.settings.disabled_plugins.contains(&p.manifest.name);
            }
            self.plugins = loaded;
        }
        for p in self.plugins.iter().filter(|p| p.enabled) {
            for code in p.startup_code() {
                let t = self.session.submit_raw(&code);
                self.startup_tokens.push((p.manifest.name.clone(), t));
            }
        }
    }

    pub fn restart_backend(&mut self) {
        let backend = make_backend(&self.settings);
        let nb = std::mem::replace(&mut self.session.notebook, Notebook::empty());
        self.session = Session::new(backend);
        self.session.notebook = nb;
        self.session.notebook.clear_outputs();
        self.load_plugins();
    }

    // ------------------------------------------------------------ files

    pub fn open_path(&mut self, p: &std::path::Path) {
        match std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| Notebook::from_json(&t)) {
            Ok(nb) => {
                self.session.notebook = nb;
                self.cell_ui.clear();
                self.active_cell = None;
                self.current_path = Some(p.to_path_buf());
                self.messages.push(format!("Opened {}", p.display()));
            }
            Err(e) => self.messages.push(format!("Could not open {}: {}", p.display(), e)),
        }
    }

    pub fn save_path(&mut self, p: &std::path::Path) {
        match std::fs::write(p, self.session.notebook.to_json()) {
            Ok(()) => {
                self.current_path = Some(p.to_path_buf());
                self.messages.push(format!("Saved {}", p.display()));
            }
            Err(e) => self.messages.push(format!("Could not save {}: {}", p.display(), e)),
        }
    }

    pub fn export(&mut self, kind: FileKind, p: &std::path::Path) {
        let text = match kind {
            FileKind::ExportLatex => self.session.notebook.to_latex_document(),
            _ => self.session.notebook.to_maxima_script(&self.session.parse_ctx()),
        };
        match std::fs::write(p, text) {
            Ok(()) => self.messages.push(format!("Exported {}", p.display())),
            Err(e) => self.messages.push(format!("Could not export {}: {}", p.display(), e)),
        }
    }

    fn autosave(&mut self, now: f64) {
        if now - self.last_autosave < 5.0 {
            return;
        }
        self.last_autosave = now;
        let json = self.session.notebook.to_json();
        if json != self.last_saved_json {
            if let Some(d) = settings::data_dir() {
                let _ = std::fs::create_dir_all(&d);
                if std::fs::write(d.join("autosave.maximus"), &json).is_ok() {
                    self.last_saved_json = json;
                }
            }
        }
    }

    // ------------------------------------------------------------ helpers

    pub fn ensure_cell_ui(&mut self, id: u64) {
        if !self.cell_ui.contains_key(&id) {
            let src = self.session.notebook.cell(id).map(|c| c.source.clone()).unwrap_or_default();
            self.cell_ui.insert(id, CellUi { editor: EditorState::from_latex(&src), latex_view: false });
        }
    }

    pub fn tree_for(&mut self, text: &str) -> Option<Arc<Row>> {
        if let Some(t) = self.tree_cache.get(text) {
            return t.clone();
        }
        if self.tree_cache.len() > 800 {
            self.tree_cache.clear();
        }
        let t = maxima_to_tree(text).ok().map(Arc::new);
        self.tree_cache.insert(text.to_string(), t.clone());
        t
    }

    pub fn new_code_cell_at_end(&mut self) -> u64 {
        let id = self.session.notebook.add_cell(CellKind::Code, InputMode::Math);
        self.active_cell = Some(id);
        self.focus_cell = Some(id);
        self.scroll_to_cell = Some(id);
        id
    }

    /// Applies an insertion to the active cell (creating one if needed).
    pub fn apply_insert(&mut self, ins: Insert) {
        let id = match self.active_cell.filter(|id| self.session.notebook.cell(*id).map_or(false, |c| c.kind == CellKind::Code)) {
            Some(id) => id,
            None => {
                let last_empty = self.session.notebook.cells.last().filter(|c| c.kind == CellKind::Code && c.is_empty()).map(|c| c.id);
                match last_empty {
                    Some(id) => id,
                    None => self.new_code_cell_at_end(),
                }
            }
        };
        self.active_cell = Some(id);
        self.focus_cell = Some(id);
        self.ensure_cell_ui(id);
        if let Insert::Call { pre: Some(pre), .. } = &ins {
            self.session.submit_raw(pre);
        }
        let mode = self.session.notebook.cell(id).unwrap().mode;
        let latex_view = self.cell_ui.get(&id).map_or(false, |c| c.latex_view);
        let evaluate = matches!(&ins, Insert::Call { evaluate: true, .. });
        if mode == InputMode::Maxima || latex_view {
            let text = match &ins {
                Insert::Symbol(c) => {
                    if latex_view {
                        maximus_math::row_to_latex(&[maximus_math::Item::Char(*c)])
                    } else {
                        maxima_symbol(*c)
                    }
                }
                Insert::Template(t) => template_text(*t, latex_view),
                Insert::Text(s) => s.clone(),
                Insert::Call { text, .. } => text.clone(),
            };
            if let Some(c) = self.session.notebook.cell_mut(id) {
                c.source.push_str(&text);
            }
            if let Some(cu) = self.cell_ui.get_mut(&id) {
                if !latex_view {
                    cu.editor = EditorState::from_latex(&self.session.notebook.cell(id).unwrap().source);
                }
            }
        } else {
            let cu = self.cell_ui.get_mut(&id).unwrap();
            match &ins {
                Insert::Symbol(c) => cu.editor.insert_symbol(*c),
                Insert::Template(t) => cu.editor.insert_template(*t),
                Insert::Text(s) => cu.editor.paste(s),
                Insert::Call { text, .. } => cu.editor.paste(text),
            }
            let latex = cu.editor.latex();
            self.session.notebook.cell_mut(id).unwrap().source = latex;
        }
        if evaluate {
            self.run_cell(id);
        }
    }

    /// Evaluates a cell and makes sure there is an empty cell after it to continue typing.
    pub fn run_cell(&mut self, id: u64) {
        self.session.evaluate_cell(id);
        if self.session.notebook.cells.last().map(|c| c.id) == Some(id) {
            let new = self.session.notebook.add_cell(CellKind::Code, InputMode::Math);
            self.focus_cell = Some(new);
            self.active_cell = Some(new);
            self.scroll_to_cell = Some(new);
        }
    }

    pub fn open_call_dialog(&mut self, template: CallTemplate) {
        let values = template.placeholders().iter().map(|p| p.default.clone()).collect();
        self.dialog = Some(CallDialog { template, values });
    }

    pub fn show_help(&mut self, topic: &str) {
        self.help_topic = Some(topic.to_string());
        self.tab = SideTab::Help;
        self.settings.side_panel = true;
    }

    pub fn plugin_templates(&self) -> Vec<(String, Vec<CallTemplate>)> {
        let mut by_cat: Vec<(String, Vec<CallTemplate>)> = Vec::new();
        for p in self.plugins.iter().filter(|p| p.enabled) {
            for m in &p.manifest.menu {
                let t = CallTemplate::new(&m.label, &m.template, &m.description);
                match by_cat.iter_mut().find(|(c, _)| *c == m.category) {
                    Some((_, v)) => v.push(t),
                    None => by_cat.push((m.category.clone(), vec![t])),
                }
            }
        }
        by_cat
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        ctx.set_visuals(if self.settings.dark { egui::Visuals::dark() } else { egui::Visuals::light() });
    }

    fn poll_session(&mut self, ctx: &egui::Context) {
        self.session.tick();
        let mut remaining = Vec::new();
        for (name, token) in std::mem::take(&mut self.startup_tokens) {
            match self.session.take_raw(token) {
                Some(r) => {
                    if let Some(e) = r.error() {
                        self.messages.push(format!("Plugin '{}': {}", name, e));
                    }
                }
                None => remaining.push((name, token)),
            }
        }
        self.startup_tokens = remaining;
        let busy = self.session.is_busy() || !self.session.ready && self.session.last_error.is_none();
        if busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
    }

    /// Switches the window to the remembered size of the active mode and remembers manual resizes.
    fn manage_window(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if self.tab != self.settings.side_tab {
            self.settings.side_tab = self.tab;
            self.settings.save();
        }
        if self.settings.minimal != self.last_mode {
            self.last_mode = self.settings.minimal;
            let [w, h] = self.settings.size_for_mode();
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
            self.resize_hold_until = now + 0.8;
            self.settings.save();
            return;
        }
        if now < self.resize_hold_until {
            return;
        }
        if let Some(r) = ctx.input(|i| i.viewport().inner_rect) {
            let size = [r.width().round(), r.height().round()];
            let slot = if self.settings.minimal { &mut self.settings.minimal_size } else { &mut self.settings.full_size };
            if (slot[0] - size[0]).abs() >= 1.0 || (slot[1] - size[1]).abs() >= 1.0 {
                *slot = size;
                self.size_dirty_since = Some(now);
            }
        }
        if let Some(t) = self.size_dirty_since {
            if now - t > 1.0 {
                self.settings.save();
                self.size_dirty_since = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (save, open, minimal, run_all, stop) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::S),
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::M),
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter),
                i.consume_key(Modifiers::COMMAND, Key::Period),
            )
        });
        if save {
            self.save_or_dialog();
        }
        if open {
            self.file_dialog = Some(FileDialog { kind: FileKind::Open, path: self.default_path() });
        }
        if minimal {
            self.settings.minimal = !self.settings.minimal;
            self.settings.save();
        }
        if run_all {
            self.session.evaluate_all();
        }
        if stop {
            self.session.interrupt();
        }
    }

    pub fn default_path(&self) -> String {
        self.current_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| {
            let base = dirs_home().unwrap_or_else(|| PathBuf::from("."));
            base.join("notebook.maximus").display().to_string()
        })
    }

    pub fn save_or_dialog(&mut self) {
        match self.current_path.clone() {
            Some(p) => self.save_path(&p),
            None => self.file_dialog = Some(FileDialog { kind: FileKind::Save, path: self.default_path() }),
        }
    }

    pub fn new_notebook(&mut self) {
        self.session.notebook = Notebook::default();
        self.cell_ui.clear();
        self.active_cell = None;
        self.current_path = None;
        self.session.restart();
        self.load_plugins();
    }

    // ------------------------------------------------------------ UI

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New notebook").clicked() {
                    self.new_notebook();
                    ui.close();
                }
                if ui.add(egui::Button::new("Open…").shortcut_text("Ctrl+O")).clicked() {
                    self.file_dialog = Some(FileDialog { kind: FileKind::Open, path: self.default_path() });
                    ui.close();
                }
                if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                    self.save_or_dialog();
                    ui.close();
                }
                if ui.button("Save as…").clicked() {
                    self.file_dialog = Some(FileDialog { kind: FileKind::Save, path: self.default_path() });
                    ui.close();
                }
                ui.separator();
                if ui.button("Export as LaTeX…").clicked() {
                    let p = self.default_path().replace(".maximus", ".tex");
                    self.file_dialog = Some(FileDialog { kind: FileKind::ExportLatex, path: p });
                    ui.close();
                }
                if ui.button("Export as Maxima script…").clicked() {
                    let p = self.default_path().replace(".maximus", ".mac");
                    self.file_dialog = Some(FileDialog { kind: FileKind::ExportMaxima, path: p });
                    ui.close();
                }
                ui.separator();
                if ui.button("Settings…").clicked() {
                    self.settings_open = true;
                    ui.close();
                }
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                ui.label(RichText::new("In an input cell: Ctrl+C copies as LaTeX, Ctrl+V pastes LaTeX or Maxima").weak().small());
                if ui.button("Clear all outputs").clicked() {
                    self.session.notebook.clear_outputs();
                    ui.close();
                }
            });
            ui.menu_button("Cell", |ui| {
                for (label, kind) in [("Code cell", CellKind::Code), ("Text cell", CellKind::Text), ("Section", CellKind::Section), ("Title", CellKind::Title)] {
                    if ui.button(format!("Add {}", label.to_lowercase())).clicked() {
                        let mode = if kind == CellKind::Code { InputMode::Math } else { InputMode::Maxima };
                        let id = self.session.notebook.add_cell(kind, mode);
                        self.active_cell = Some(id);
                        self.focus_cell = Some(id);
                        self.scroll_to_cell = Some(id);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.add(egui::Button::new("Evaluate all").shortcut_text("Ctrl+Shift+Enter")).clicked() {
                    self.session.evaluate_all();
                    ui.close();
                }
                if ui.add(egui::Button::new("Interrupt").shortcut_text("Ctrl+.")).clicked() {
                    self.session.interrupt();
                    ui.close();
                }
            });
            self.maxima_menu(ui);
            ui.menu_button("Symbols", |ui| {
                self.symbols_menu(ui);
            });
            ui.menu_button("View", |ui| {
                if ui.checkbox(&mut self.settings.minimal, "Minimal (calculator) mode").clicked() {
                    self.settings.save();
                }
                ui.checkbox(&mut self.settings.side_panel, "Side panel");
                if ui.checkbox(&mut self.settings.contextual_help, "Contextual help follows cursor").clicked() {
                    self.settings.save();
                }
                ui.separator();
                if ui.button("Dark theme").clicked() {
                    self.settings.dark = true;
                    self.apply_theme(ui.ctx());
                    self.settings.save();
                }
                if ui.button("Light theme").clicked() {
                    self.settings.dark = false;
                    self.apply_theme(ui.ctx());
                    self.settings.save();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Help for function under cursor").clicked() {
                    if let Some(t) = self.help_topic.clone() {
                        self.show_help(&t);
                    }
                    ui.close();
                }
                if ui.button("Plugins…").clicked() {
                    self.plugins_open = true;
                    ui.close();
                }
                if ui.button("About Maximus").clicked() {
                    self.about_open = true;
                    ui.close();
                }
            });
        });
    }

    fn maxima_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Maxima", |ui| {
            let cats = std::mem::take(&mut self.catalog);
            for cat in &cats {
                ui.menu_button(cat.name, |ui| {
                    for it in &cat.items {
                        let r = ui.button(&it.label);
                        if !it.description.is_empty() {
                            r.clone().on_hover_text(&it.description);
                        }
                        if r.clicked() {
                            self.open_call_dialog(it.clone());
                            ui.close();
                        }
                    }
                });
            }
            self.catalog = cats;
            let plug = self.plugin_templates();
            for (cat, items) in plug {
                ui.menu_button(format!("{} (plugin)", cat), |ui| {
                    for it in items {
                        if ui.button(&it.label).clicked() {
                            self.open_call_dialog(it);
                            ui.close();
                        }
                    }
                });
            }
            ui.separator();
            if ui.button("All functions…").clicked() {
                self.tab = SideTab::Functions;
                self.settings.side_panel = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Restart Maxima").clicked() {
                self.restart_backend();
                ui.close();
            }
            if ui.button("Clear all variables").clicked() {
                self.session.reset_variables();
                ui.close();
            }
        });
    }

    pub fn symbols_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Structures", |ui| {
            for (label, t) in structure_buttons() {
                if ui.button(label).clicked() {
                    self.pending.push(Insert::Template(t));
                    ui.close();
                }
            }
        });
        let groups = self.all_palettes();
        for g in groups {
            ui.menu_button(g.name.clone(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(260.0);
                    for s in &g.symbols {
                        if ui.button(RichText::new(s).size(18.0)).clicked() {
                            if let Some(c) = s.chars().next() {
                                self.pending.push(Insert::Symbol(c));
                            }
                        }
                    }
                });
            });
        }
    }

    pub fn all_palettes(&self) -> Vec<PaletteGroup> {
        let mut g = catalog::palettes();
        for p in self.plugins.iter().filter(|p| p.enabled) {
            for pal in &p.manifest.palette {
                g.push(PaletteGroup { name: format!("{} ({})", pal.name, p.manifest.name), symbols: pal.symbols.clone() });
            }
        }
        g
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if icons::icon_label_button(ui, Icon::Play, "Evaluate all", true).clicked() {
                self.session.evaluate_all();
            }
            if icons::icon_label_button(ui, Icon::Stop, "Stop", self.session.is_busy()).clicked() {
                self.session.interrupt();
            }
            ui.separator();
            for (label, kind) in [("+ Code", CellKind::Code), ("+ Text", CellKind::Text), ("+ Section", CellKind::Section)] {
                if ui.button(label).clicked() {
                    let mode = if kind == CellKind::Code { InputMode::Math } else { InputMode::Maxima };
                    let id = self.session.notebook.add_cell(kind, mode);
                    self.active_cell = Some(id);
                    self.focus_cell = Some(id);
                    self.scroll_to_cell = Some(id);
                }
            }
            ui.separator();
            if ui.button("Minimal mode").on_hover_text("Ctrl+M").clicked() {
                self.settings.minimal = true;
                self.settings.save();
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match self.session.backend.status() {
                BackendStatus::Ready => {
                    icons::status_dot(ui, Color32::from_rgb(0x4c, 0xaf, 0x50), "Maxima ready");
                }
                BackendStatus::Starting => {
                    ui.spinner();
                    ui.label("Starting Maxima…");
                }
                BackendStatus::Dead(m) => {
                    icons::status_dot(ui, Color32::from_rgb(0xe5, 0x39, 0x35), "Maxima not running");
                    ui.label(RichText::new(m).small());
                    if ui.button("Restart").clicked() {
                        self.restart_backend();
                    }
                }
            }
            if self.session.is_busy() {
                ui.separator();
                ui.spinner();
                ui.label("Computing…");
                if ui.small_button("Stop").clicked() {
                    self.session.interrupt();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(m) = self.messages.last() {
                    ui.label(RichText::new(m).small().weak());
                }
            });
        });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        self.call_dialog_window(ctx);
        self.file_dialog_window(ctx);
        self.settings_window(ctx);
        self.plugins_window(ctx);
        let mut about = self.about_open;
        egui::Window::new("About Maximus").open(&mut about).collapsible(false).show(ctx, |ui| {
            ui.heading("Maximus");
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.label("A textbook-style notebook frontend for the Maxima computer algebra system.");
            ui.separator();
            ui.label("Shortcuts");
            ui.label("Enter – evaluate cell (textbook input)   Shift+Enter – evaluate (Maxima input)");
            ui.label("Ctrl+Shift+Enter – evaluate all   Ctrl+. – interrupt   Ctrl+M – minimal mode");
            ui.label("Type / for a fraction, ^ for a power, _ for a subscript, sqrt( for a root.");
            ui.label("Right-click a function for help, copy and conversion options.");
        });
        self.about_open = about;
    }

    fn call_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialog.take() else { return };
        let mut keep = true;
        let mut insert: Option<bool> = None;
        let phs = d.template.placeholders();
        egui::Window::new(d.template.label.clone()).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            if !d.template.description.is_empty() {
                ui.label(&d.template.description);
            }
            egui::Grid::new("call-dialog").num_columns(2).show(ui, |ui| {
                for (i, ph) in phs.iter().enumerate() {
                    ui.label(&ph.name);
                    let r = ui.add(egui::TextEdit::singleline(&mut d.values[i]).desired_width(260.0).font(egui::TextStyle::Monospace));
                    if i == 0 && !r.has_focus() && r.gained_focus() {
                        // keep default focus behaviour
                    }
                    ui.end_row();
                }
            });
            let call = d.template.fill(&d.values);
            ui.label(RichText::new(&call).monospace().weak());
            ui.horizontal(|ui| {
                if ui.button("Insert").clicked() {
                    insert = Some(false);
                }
                if ui.button("Insert && evaluate").clicked() {
                    insert = Some(true);
                }
                let name = d.template.function_name();
                if self.help.first(&name).is_some() && ui.button("Help").clicked() {
                    self.show_help(&name);
                }
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
        });
        if let Some(evaluate) = insert {
            let text = d.template.fill(&d.values);
            self.pending.push(Insert::Call { text, evaluate, pre: d.template.pre.clone() });
            keep = false;
        }
        if keep {
            self.dialog = Some(d);
        }
    }

    fn file_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut fd) = self.file_dialog.take() else { return };
        let mut keep = true;
        let mut go = false;
        let title = match fd.kind {
            FileKind::Save => "Save notebook",
            FileKind::Open => "Open notebook",
            FileKind::ExportLatex => "Export LaTeX",
            FileKind::ExportMaxima => "Export Maxima script",
        };
        egui::Window::new(title).collapsible(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("Path");
            let r = ui.add(egui::TextEdit::singleline(&mut fd.path).desired_width(440.0));
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                go = true;
            }
            if fd.kind == FileKind::Open {
                if let Some(dir) = std::path::Path::new(&fd.path).parent() {
                    let mut files: Vec<_> = std::fs::read_dir(dir)
                        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().map_or(false, |e| e == "maximus")).collect())
                        .unwrap_or_default();
                    files.sort();
                    egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                        for f in files {
                            if ui.selectable_label(false, f.file_name().and_then(|n| n.to_str()).unwrap_or("")).clicked() {
                                fd.path = f.display().to_string();
                            }
                        }
                    });
                }
            }
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    go = true;
                }
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
        });
        if go {
            let p = PathBuf::from(&fd.path);
            match fd.kind {
                FileKind::Save => self.save_path(&p),
                FileKind::Open => self.open_path(&p),
                k => self.export(k, &p),
            }
            keep = false;
        }
        if keep {
            self.file_dialog = Some(fd);
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.settings_open;
        let mut restart = false;
        let mut changed = false;
        egui::Window::new("Settings").open(&mut open).collapsible(false).show(ctx, |ui| {
            if ui.checkbox(&mut self.settings.dark, "Dark theme").changed() {
                changed = true;
                self.apply_theme(ctx);
            }
            changed |= ui.add(egui::Slider::new(&mut self.settings.math_size, 12.0..=40.0).text("Math size")).changed();
            changed |= ui.checkbox(&mut self.settings.contextual_help, "Contextual help follows the cursor").changed();
            ui.separator();
            ui.label("Maxima executable (empty = search PATH)");
            changed |= ui.text_edit_singleline(&mut self.settings.maxima_path).changed();
            if ui.button("Apply and restart Maxima").clicked() {
                restart = true;
            }
            ui.separator();
            ui.label("Plots");
            changed |= ui.radio_value(&mut self.settings.plot_backend, PlotBackend::Egui, "Built-in (egui_plot)").changed();
            changed |= ui.radio_value(&mut self.settings.plot_backend, PlotBackend::Gnuplot, "gnuplot").changed();
            ui.add_enabled_ui(self.settings.plot_backend == PlotBackend::Gnuplot, |ui| {
                ui.label("gnuplot executable");
                changed |= ui.text_edit_singleline(&mut self.settings.gnuplot_path).changed();
            });
            if let Some(d) = settings::plugin_dir() {
                ui.separator();
                ui.label(format!("Plugin folder: {}", d.display()));
            }
        });
        self.settings_open = open;
        if changed {
            self.settings.save();
            self.gnuplot_cache.clear();
        }
        if restart {
            self.restart_backend();
        }
    }

    fn plugins_window(&mut self, ctx: &egui::Context) {
        let mut open = self.plugins_open;
        let mut reload = false;
        egui::Window::new("Plugins").open(&mut open).default_width(420.0).show(ctx, |ui| {
            if self.plugins.is_empty() {
                ui.label("No plugins found.");
                if let Some(d) = settings::plugin_dir() {
                    ui.label(format!("Put a folder with a plugin.toml in {}", d.display()));
                }
            }
            for p in &mut self.plugins {
                let name = p.manifest.name.clone();
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut p.enabled, &name).changed() {
                        if p.enabled {
                            self.settings.disabled_plugins.retain(|n| *n != name);
                        } else {
                            self.settings.disabled_plugins.push(name.clone());
                        }
                        self.settings.save();
                        reload = true;
                    }
                    ui.label(RichText::new(&p.manifest.version).weak());
                });
                if !p.manifest.description.is_empty() {
                    ui.label(RichText::new(&p.manifest.description).small());
                }
                ui.label(RichText::new(format!("{} menu items, {} palettes, {} snippets", p.manifest.menu.len(), p.manifest.palette.len(), p.manifest.snippet.len())).small().weak());
                ui.separator();
            }
            if ui.button("Reload plugins").clicked() {
                reload = true;
            }
        });
        self.plugins_open = open;
        if reload {
            self.load_plugins();
        }
    }
}

impl eframe::App for MaximusApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_session(&ctx);
        if self.run_on_start && self.session.ready {
            self.run_on_start = false;
            self.session.evaluate_all();
        }
        self.shortcuts(&ctx);
        self.manage_window(&ctx);
        let now = ctx.input(|i| i.time);
        self.autosave(now);
        for ins in std::mem::take(&mut self.pending) {
            if self.settings.minimal {
                self.apply_insert_minimal(ins);
            } else {
                self.apply_insert(ins);
            }
        }

        egui::Panel::top("menu").show(ui, |ui| {
            self.menu_bar(ui);
            if !self.settings.minimal {
                self.toolbar(ui);
            }
        });
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        if self.settings.minimal {
            egui::CentralPanel::default().show(ui, |ui| self.minimal_ui(ui));
        } else {
            if self.settings.side_panel {
                let max_w = (ctx.content_rect().width() * 0.42).max(240.0);
                egui::Panel::right("side")
                    .resizable(true)
                    .default_size(340.0_f32.min(max_w))
                    .size_range(240.0_f32.min(max_w)..=max_w)
                    .show(ui, |ui| self.side_panel(ui));
            }
            egui::CentralPanel::default().show(ui, |ui| self.notebook_ui(ui));
        }
        self.windows(&ctx);
    }
}

fn make_backend(settings: &Settings) -> Box<dyn Backend> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let cfg = maximus_backend_native::NativeConfig {
            maxima_path: if settings.maxima_path.trim().is_empty() { None } else { Some(settings.maxima_path.clone()) },
            extra_args: vec![],
        };
        Box::new(maximus_backend_native::NativeBackend::new(cfg))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = settings;
        Box::new(NullBackend::new("Maxima cannot run inside the browser yet. Connect a remote Maxima backend to use the web version."))
    }
}

fn dirs_home() -> Option<PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        dirs::document_dir().or_else(dirs::home_dir)
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

pub fn structure_buttons() -> Vec<(&'static str, Template)> {
    vec![
        ("a/b  Fraction", Template::Frac),
        ("xⁿ  Power", Template::Sup),
        ("xₙ  Subscript", Template::Sub),
        ("√  Square root", Template::Sqrt),
        ("ⁿ√  Nth root", Template::NthRoot),
        ("( )  Parentheses", Template::Paren),
        ("|x|  Absolute value", Template::Abs),
        ("∫  Integral", Template::Integral),
        ("∫ₐᵇ  Definite integral", Template::DefIntegral),
        ("Σ  Sum", Template::Sum),
        ("Π  Product", Template::Product),
        ("lim  Limit", Template::Limit),
        ("d/dx  Derivative", Template::Derivative),
        ("2×2 Matrix", Template::Matrix(2, 2)),
        ("3×3 Matrix", Template::Matrix(3, 3)),
    ]
}

fn maxima_symbol(c: char) -> String {
    use maximus_math::words::greek_name_of_glyph;
    match c {
        'π' => "%pi".into(),
        'ⅇ' => "%e".into(),
        'ⅈ' => "%i".into(),
        '∞' => "inf".into(),
        '·' | '×' => "*".into(),
        '÷' => "/".into(),
        '≤' => "<=".into(),
        '≥' => ">=".into(),
        '≠' => "#".into(),
        c => greek_name_of_glyph(c).map(String::from).unwrap_or_else(|| c.to_string()),
    }
}

fn template_text(t: Template, latex: bool) -> String {
    match (t, latex) {
        (Template::Frac, true) => "\\frac{}{}".into(),
        (Template::Frac, false) => "()/()".into(),
        (Template::Sup, true) => "^{}".into(),
        (Template::Sup, false) => "^".into(),
        (Template::Sub, true) => "_{}".into(),
        (Template::Sub, false) => "[]".into(),
        (Template::Sqrt, true) => "\\sqrt{}".into(),
        (Template::Sqrt, false) => "sqrt()".into(),
        (Template::NthRoot, true) => "\\sqrt[n]{}".into(),
        (Template::NthRoot, false) => "()^(1/n)".into(),
        (Template::Paren, _) => "()".into(),
        (Template::Abs, true) => "\\left| \\right|".into(),
        (Template::Abs, false) => "abs()".into(),
        (Template::Integral, true) => "\\int  \\,dx".into(),
        (Template::Integral, false) => "integrate(f, x)".into(),
        (Template::DefIntegral, true) => "\\int_{a}^{b}  \\,dx".into(),
        (Template::DefIntegral, false) => "integrate(f, x, a, b)".into(),
        (Template::Sum, true) => "\\sum_{n=1}^{N} ".into(),
        (Template::Sum, false) => "sum(f, n, 1, N)".into(),
        (Template::Product, true) => "\\prod_{n=1}^{N} ".into(),
        (Template::Product, false) => "product(f, n, 1, N)".into(),
        (Template::Limit, true) => "\\lim_{x\\to a} ".into(),
        (Template::Limit, false) => "limit(f, x, a)".into(),
        (Template::Derivative, true) => "\\frac{d}{dx}".into(),
        (Template::Derivative, false) => "diff(f, x)".into(),
        (Template::Matrix(r, c), true) => {
            let row = vec![""; c].join(" & ");
            format!("\\begin{{pmatrix}}{}\\end{{pmatrix}}", vec![row; r].join("\\\\ "))
        }
        (Template::Matrix(r, c), false) => format!("matrix({})", vec![format!("[{}]", vec!["0"; c].join(",")); r].join(",")),
    }
}
