//! Notebook view: cells, inputs and outputs.

use crate::app::*;
use crate::settings::PlotBackend;
use eframe::egui;
use egui::{Color32, Key, Modifiers, RichText};
use maximus_core::*;
use maximus_math::{maxima_to_latex, tree_to_plain_maxima};
use maximus_render::editor_ui::{math_editor, EditorStyle};

pub enum CellAction {
    Run(u64),
    RunThrough(u64),
    Delete(u64),
    Up(u64),
    Down(u64),
    Duplicate(u64),
    InsertBelow(u64, CellKind),
    ToggleCollapse(u64),
    ClearOutput(u64),
    SetMode(u64, InputMode),
    ToggleLatex(u64),
}

const RED: Color32 = Color32::from_rgb(0xe5, 0x39, 0x35);

impl MaximusApp {
    pub fn notebook_ui(&mut self, ui: &mut egui::Ui) {
        let mut actions: Vec<CellAction> = Vec::new();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(4.0);
            for idx in 0..self.session.notebook.cells.len() {
                self.cell_ui(ui, idx, &mut actions);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("+ Code cell").clicked() {
                    self.new_code_cell_at_end();
                }
                if ui.button("+ Text cell").clicked() {
                    let id = self.session.notebook.add_cell(CellKind::Text, InputMode::Maxima);
                    self.focus_cell = Some(id);
                }
            });
            ui.add_space(120.0);
        });
        for a in actions {
            self.apply_action(a);
        }
    }

    fn apply_action(&mut self, a: CellAction) {
        let nb = &mut self.session.notebook;
        match a {
            CellAction::Run(id) => self.run_cell(id),
            CellAction::RunThrough(id) => self.session.evaluate_through(id),
            CellAction::Delete(id) => {
                nb.remove(id);
                self.cell_ui.remove(&id);
                if nb.cells.is_empty() {
                    let n = nb.add_cell(CellKind::Code, InputMode::Math);
                    self.focus_cell = Some(n);
                }
            }
            CellAction::Up(id) => nb.move_up(id),
            CellAction::Down(id) => nb.move_down(id),
            CellAction::Duplicate(id) => {
                if let Some(n) = nb.duplicate(id) {
                    self.focus_cell = Some(n);
                }
            }
            CellAction::InsertBelow(id, kind) => {
                let at = nb.index_of(id).map_or(nb.cells.len(), |i| i + 1);
                let mode = if kind == CellKind::Code { InputMode::Math } else { InputMode::Maxima };
                let n = nb.insert_cell(at, kind, mode);
                self.focus_cell = Some(n);
                self.active_cell = Some(n);
            }
            CellAction::ToggleCollapse(id) => {
                if let Some(c) = nb.cell_mut(id) {
                    c.collapsed = !c.collapsed;
                }
            }
            CellAction::ClearOutput(id) => {
                if let Some(c) = nb.cell_mut(id) {
                    c.outputs.clear();
                    c.state = CellState::Idle;
                }
            }
            CellAction::SetMode(id, mode) => self.set_cell_mode(id, mode),
            CellAction::ToggleLatex(id) => {
                let src = self.session.notebook.cell(id).map(|c| c.source.clone()).unwrap_or_default();
                if let Some(cu) = self.cell_ui.get_mut(&id) {
                    if cu.latex_view {
                        cu.editor = maximus_render::editor::EditorState::from_latex(&src);
                    }
                    cu.latex_view = !cu.latex_view;
                }
                self.focus_cell = Some(id);
            }
        }
    }

    /// Switches a cell between textbook and Maxima input, converting the content.
    fn set_cell_mode(&mut self, id: u64, mode: InputMode) {
        let ctx = self.session.parse_ctx();
        let Some(cell) = self.session.notebook.cell(id) else { return };
        if cell.mode == mode {
            return;
        }
        let new_source = match mode {
            InputMode::Maxima => match cell.to_maxima(&ctx) {
                Ok(s) => s,
                Err(e) => {
                    self.messages.push(format!("Cannot convert to Maxima: {}", e));
                    return;
                }
            },
            InputMode::Math => maximus_math::maxima_to_latex(cell.source.trim().trim_end_matches([';', '$'])).unwrap_or_else(|_| cell.source.clone()),
        };
        let c = self.session.notebook.cell_mut(id).unwrap();
        c.source = new_source;
        c.mode = mode;
        self.cell_ui.remove(&id);
        self.ensure_cell_ui(id);
        self.focus_cell = Some(id);
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, idx: usize, actions: &mut Vec<CellAction>) {
        let (id, kind, state, collapsed) = {
            let c = &self.session.notebook.cells[idx];
            (c.id, c.kind, c.state, c.collapsed)
        };
        let active = self.active_cell == Some(id);
        let visuals = ui.visuals().clone();
        let stroke = if active { egui::Stroke::new(1.5, visuals.selection.bg_fill) } else { egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color) };
        let frame = egui::Frame::new().stroke(stroke).corner_radius(6.0).inner_margin(egui::Margin::same(8)).fill(visuals.faint_bg_color);
        let fr = frame.show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(46.0);
                    self.gutter(ui, idx, id, kind, state, actions);
                });
                ui.vertical(|ui| {
                    ui.set_min_width(ui.available_width());
                    match kind {
                        CellKind::Code => self.code_cell(ui, idx, id, collapsed, actions),
                        _ => self.prose_cell(ui, idx, id, kind),
                    }
                });
            });
        });
        if self.scroll_to_cell == Some(id) {
            fr.response.scroll_to_me(Some(egui::Align::Center));
            self.scroll_to_cell = None;
        }
        ui.add_space(4.0);
    }

    fn gutter(&mut self, ui: &mut egui::Ui, idx: usize, id: u64, kind: CellKind, state: CellState, actions: &mut Vec<CellAction>) {
        match kind {
            CellKind::Code => {
                let n = self.session.notebook.cells[..=idx].iter().filter(|c| c.kind == CellKind::Code).count();
                ui.label(RichText::new(format!("In {}", n)).small().weak());
                match state {
                    CellState::Running => {
                        ui.spinner();
                    }
                    CellState::Queued => {
                        ui.label(RichText::new("queued").small().weak());
                    }
                    CellState::Failed => {
                        ui.colored_label(RED, "✗");
                    }
                    CellState::Done => {
                        ui.label(RichText::new("✓").weak());
                    }
                    CellState::Idle => {}
                }
                if ui.small_button("▶").on_hover_text("Evaluate").clicked() {
                    actions.push(CellAction::Run(id));
                }
            }
            CellKind::Text => {
                ui.label(RichText::new("Text").small().weak());
            }
            CellKind::Section => {
                ui.label(RichText::new("Section").small().weak());
            }
            CellKind::Title => {
                ui.label(RichText::new("Title").small().weak());
            }
        }
        ui.menu_button("⋮", |ui| {
            if kind == CellKind::Code {
                let mode = self.session.notebook.cells[idx].mode;
                if ui.selectable_label(mode == InputMode::Math && !self.cell_ui.get(&id).map_or(false, |c| c.latex_view), "Textbook input").clicked() {
                    if self.cell_ui.get(&id).map_or(false, |c| c.latex_view) {
                        actions.push(CellAction::ToggleLatex(id));
                    }
                    actions.push(CellAction::SetMode(id, InputMode::Math));
                    ui.close();
                }
                if ui.selectable_label(mode == InputMode::Maxima, "Maxima input").clicked() {
                    actions.push(CellAction::SetMode(id, InputMode::Maxima));
                    ui.close();
                }
                if mode == InputMode::Math && ui.selectable_label(self.cell_ui.get(&id).map_or(false, |c| c.latex_view), "LaTeX source").clicked() {
                    actions.push(CellAction::ToggleLatex(id));
                    ui.close();
                }
                ui.separator();
                if ui.button("Evaluate to here").clicked() {
                    actions.push(CellAction::RunThrough(id));
                    ui.close();
                }
                if ui.button("Clear output").clicked() {
                    actions.push(CellAction::ClearOutput(id));
                    ui.close();
                }
                if ui.button("Collapse / expand output").clicked() {
                    actions.push(CellAction::ToggleCollapse(id));
                    ui.close();
                }
                ui.separator();
            }
            if ui.button("Move up").clicked() {
                actions.push(CellAction::Up(id));
                ui.close();
            }
            if ui.button("Move down").clicked() {
                actions.push(CellAction::Down(id));
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                actions.push(CellAction::Duplicate(id));
                ui.close();
            }
            ui.menu_button("Insert below", |ui| {
                for (l, k) in [("Code", CellKind::Code), ("Text", CellKind::Text), ("Section", CellKind::Section), ("Title", CellKind::Title)] {
                    if ui.button(l).clicked() {
                        actions.push(CellAction::InsertBelow(id, k));
                        ui.close();
                    }
                }
            });
            ui.separator();
            if ui.button("Delete").clicked() {
                actions.push(CellAction::Delete(id));
                ui.close();
            }
        });
    }

    fn prose_cell(&mut self, ui: &mut egui::Ui, idx: usize, id: u64, kind: CellKind) {
        let font = match kind {
            CellKind::Title => egui::FontId::proportional(28.0),
            CellKind::Section => egui::FontId::proportional(21.0),
            _ => egui::FontId::proportional(15.0),
        };
        let eid = ui.make_persistent_id(("prose", id));
        if self.focus_cell == Some(id) {
            ui.memory_mut(|m| m.request_focus(eid));
            self.focus_cell = None;
        }
        let hint = match kind {
            CellKind::Title => "Title",
            CellKind::Section => "Section heading",
            _ => "Notes…",
        };
        let c = &mut self.session.notebook.cells[idx];
        let te = if kind == CellKind::Text { egui::TextEdit::multiline(&mut c.source).desired_rows(1) } else { egui::TextEdit::singleline(&mut c.source) };
        let r = ui.add(te.id(eid).font(font).hint_text(hint).desired_width(f32::INFINITY).frame(egui::Frame::NONE));
        if r.has_focus() {
            self.active_cell = Some(id);
        }
    }

    fn code_cell(&mut self, ui: &mut egui::Ui, idx: usize, id: u64, collapsed: bool, actions: &mut Vec<CellAction>) {
        self.ensure_cell_ui(id);
        let mode = self.session.notebook.cells[idx].mode;
        let eid = ui.make_persistent_id(("editor", id));
        if self.focus_cell == Some(id) {
            ui.memory_mut(|m| m.request_focus(eid));
            self.focus_cell = None;
        }
        let latex_view = self.cell_ui.get(&id).map_or(false, |c| c.latex_view);
        let text_color = ui.visuals().text_color();
        let mut submit = false;
        let focused;
        let mut word: Option<String> = None;

        if latex_view || mode == InputMode::Maxima {
            let hint = if latex_view { "LaTeX, e.g. \\int_0^{\\infty} x \\sin^3 x \\, dx" } else { "Maxima code, e.g. integrate(x*sin(x), x);  (Shift+Enter evaluates)" };
            if ui.memory(|m| m.has_focus(eid)) {
                submit = ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Enter));
            }
            let src = &mut self.session.notebook.cells[idx].source;
            let out = egui::TextEdit::multiline(src).id(eid).code_editor().desired_rows(1).desired_width(f32::INFINITY).hint_text(hint).show(ui);
            focused = out.response.has_focus();
            if focused {
                if let Some(cr) = out.cursor_range {
                    word = maximus_help::word_at(src, cr.primary.index.0);
                }
            }
            if out.response.changed() && latex_view {
                // keep the textbook editor in sync when switching back
            }
        } else {
            let size = self.settings.math_size;
            let mut style = EditorStyle::new(size, text_color);
            style.hint = "Type math here — e.g. 1/2 for a fraction, x^2, sqrt(x), integrate(x*sin(x), x)".into();
            let cu = self.cell_ui.get_mut(&id).unwrap();
            let out = math_editor(ui, eid, &mut cu.editor, &style);
            if out.changed {
                let latex = cu.editor.latex();
                self.session.notebook.cells[idx].source = latex;
            }
            focused = out.focused;
            submit = out.submit;
            word = out.help_word.clone();
            if out.secondary_clicked {
                if let Some(w) = &word {
                    if self.help.first(w).is_some() {
                        self.help_topic = Some(w.clone());
                    }
                }
            }
            out.response.context_menu(|ui| {
                if let Some(w) = word.clone().filter(|w| self.help.first(w).is_some()) {
                    if ui.button(format!("Help for “{}”", w)).clicked() {
                        self.show_help(&w);
                        ui.close();
                    }
                    ui.separator();
                }
                let (latex, row) = self.cell_ui.get(&id).map(|c| (c.editor.latex(), c.editor.row.clone())).unwrap_or_default();
                if ui.button("Copy as LaTeX").clicked() {
                    ui.ctx().copy_text(latex.clone());
                    ui.close();
                }
                if ui.button("Copy as Maxima").clicked() {
                    match tree_to_plain_maxima(&row, &self.session.parse_ctx()) {
                        Ok(m) => ui.ctx().copy_text(m),
                        Err(e) => self.messages.push(format!("Cannot convert: {}", e)),
                    }
                    ui.close();
                }
                if ui.button("Convert cell to Maxima input").clicked() {
                    actions.push(CellAction::SetMode(id, InputMode::Maxima));
                    ui.close();
                }
                if ui.button("Clear input").clicked() {
                    if let Some(cu) = self.cell_ui.get_mut(&id) {
                        cu.editor.clear();
                    }
                    self.session.notebook.cells[idx].source.clear();
                    ui.close();
                }
                ui.separator();
                if ui.button("Evaluate").clicked() {
                    actions.push(CellAction::Run(id));
                    ui.close();
                }
            });
        }
        if focused {
            self.active_cell = Some(id);
            if self.settings.contextual_help {
                if let Some(w) = word {
                    if self.help.first(&w).is_some() && self.help_topic.as_deref() != Some(&w) {
                        self.help_topic = Some(w);
                    }
                }
            }
        }
        if submit {
            actions.push(CellAction::Run(id));
        }

        if collapsed {
            if !self.session.notebook.cells[idx].outputs.is_empty() {
                ui.label(RichText::new("output collapsed").small().weak());
            }
            return;
        }
        let outputs = std::mem::take(&mut self.session.notebook.cells[idx].outputs);
        for (k, o) in outputs.iter().enumerate() {
            self.show_output(ui, id, k, o);
        }
        if let Some(c) = self.session.notebook.cells.get_mut(idx) {
            if c.id == id && c.outputs.is_empty() {
                c.outputs = outputs;
            }
        }
    }

    pub fn show_output(&mut self, ui: &mut egui::Ui, cell: u64, k: usize, o: &OutputItem) {
        match o {
            OutputItem::Text(t) => {
                ui.label(RichText::new(t).monospace());
            }
            OutputItem::Error(e) => {
                ui.colored_label(RED, RichText::new(e).monospace());
            }
            OutputItem::Plot(p) => self.show_plot_output(ui, cell, k, p),
            OutputItem::Result { label, text, .. } => {
                let color = ui.visuals().text_color();
                let tree = self.tree_for(text);
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new(format!("({})", label.trim_start_matches('%').replace('o', "%o"))).small().weak());
                    let resp = egui::ScrollArea::horizontal()
                        .id_salt(("out", cell, k))
                        .auto_shrink([true, true])
                        .show(ui, |ui| match &tree {
                            Some(t) => maximus_render::show_math(ui, t, self.settings.math_size, color),
                            None => ui.add(egui::Label::new(RichText::new(text).monospace()).sense(egui::Sense::click())),
                        })
                        .inner;
                    resp.context_menu(|ui| self.result_menu(ui, text));
                });
            }
        }
    }

    fn result_menu(&mut self, ui: &mut egui::Ui, text: &str) {
        if ui.button("Copy as LaTeX").clicked() {
            ui.ctx().copy_text(maxima_to_latex(text).unwrap_or_else(|_| text.to_string()));
            ui.close();
        }
        if ui.button("Copy as Maxima").clicked() {
            ui.ctx().copy_text(text.to_string());
            ui.close();
        }
        if ui.button("Use as input").clicked() {
            self.pending.push(Insert::Text(text.to_string()));
            ui.close();
        }
        let mut seen = Vec::new();
        for w in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if w.len() > 1 && !seen.iter().any(|s: &String| s == w) && self.help.first(w).is_some() {
                seen.push(w.to_string());
            }
        }
        if !seen.is_empty() {
            ui.separator();
            for w in seen.into_iter().take(6) {
                if ui.button(format!("Help for “{}”", w)).clicked() {
                    self.show_help(&w);
                    ui.close();
                }
            }
        }
    }

    fn show_plot_output(&mut self, ui: &mut egui::Ui, cell: u64, k: usize, p: &PlotData) {
        if self.settings.plot_backend == PlotBackend::Gnuplot {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let sig = plot_signature(p);
                let key = format!("{}-{}-{}", cell, k, sig);
                let gp = self.settings.gnuplot_path.clone();
                let entry = self.gnuplot_cache.entry(key.clone()).or_insert_with(|| {
                    let out = std::env::temp_dir().join(format!("maximus-plot-{}-{}.png", std::process::id(), sig));
                    maximus_plot::gnuplot::render(p, &gp, &out).map(|_| out)
                });
                match entry {
                    Ok(path) => {
                        ui.add(egui::Image::new(format!("file://{}", path.display())).max_width(ui.available_width().min(900.0)));
                        return;
                    }
                    Err(e) => {
                        ui.colored_label(RED, format!("gnuplot failed ({}); showing built-in plot", e.lines().next().unwrap_or("")));
                    }
                }
            }
        }
        maximus_plot::show_plot(ui, (cell, k), p, if self.settings.minimal { 200.0 } else { 300.0 });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn plot_signature(p: &PlotData) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.kind.hash(&mut h);
    for s in &p.series {
        s.label.hash(&mut h);
        s.pts.len().hash(&mut h);
        for pt in s.pts.iter().flatten().step_by(37) {
            pt.0.to_bits().hash(&mut h);
            pt.1.to_bits().hash(&mut h);
        }
    }
    if let Some(g) = &p.grid {
        g.label.hash(&mut h);
        g.xs.len().hash(&mut h);
        for z in g.zs.iter().flatten().flatten().step_by(11) {
            z.to_bits().hash(&mut h);
        }
    }
    h.finish()
}
