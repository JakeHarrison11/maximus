//! Notebook view: cells, inputs and outputs.

use crate::app::*;
use crate::icons::{self, Icon};
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
    /// Backspace in an empty cell: delete it and move to the previous cell.
    DeleteBack(u64),
    Up(u64),
    Down(u64),
    Duplicate(u64),
    InsertBelow(u64, CellKind),
    ToggleCollapse(u64),
    ClearOutput(u64),
    SetInput(u64, InputKind),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Textbook,
    Maxima,
    Latex,
}

const RED: Color32 = Color32::from_rgb(0xe5, 0x39, 0x35);
const GREEN: Color32 = Color32::from_rgb(0x4c, 0xaf, 0x50);

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
                if icons::icon_label_button(ui, Icon::Plus, "Code cell", true).clicked() {
                    self.new_code_cell_at_end();
                }
                if icons::icon_label_button(ui, Icon::Plus, "Text cell", true).clicked() {
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
        match a {
            CellAction::Run(id) => self.run_cell(id),
            CellAction::RunThrough(id) => self.session.evaluate_through(id),
            CellAction::Delete(id) => {
                let (mode, latex) = self.input_style_like(Some(id));
                let nb = &mut self.session.notebook;
                nb.remove(id);
                self.cell_ui.remove(&id);
                if nb.cells.is_empty() {
                    let n = nb.add_cell(CellKind::Code, mode);
                    self.ensure_cell_ui(n);
                    if let Some(cu) = self.cell_ui.get_mut(&n) {
                        cu.latex_view = latex;
                    }
                    self.focus_cell = Some(n);
                }
            }
            CellAction::DeleteBack(id) => {
                let Some(i) = self.session.notebook.index_of(id) else { return };
                if self.session.notebook.cells.len() == 1 {
                    // The only cell is replaced by a fresh empty one.
                    let (mode, latex) = self.input_style_like(Some(id));
                    let kind = self.session.notebook.cells[0].kind;
                    self.session.notebook.remove(id);
                    self.cell_ui.remove(&id);
                    let n = if kind == CellKind::Code { self.add_code_cell(None, None) } else { self.session.notebook.add_cell(kind, InputMode::Maxima) };
                    if let Some(cu) = self.cell_ui.get_mut(&n) {
                        cu.latex_view = latex;
                    }
                    let _ = mode;
                    self.focus_cell = Some(n);
                    self.active_cell = Some(n);
                    return;
                }
                let nb = &mut self.session.notebook;
                nb.remove(id);
                self.cell_ui.remove(&id);
                let target = nb.cells[i.saturating_sub(1).min(nb.cells.len() - 1)].id;
                self.focus_cell = Some(target);
                self.active_cell = Some(target);
                if let Some(cu) = self.cell_ui.get_mut(&target) {
                    cu.editor.cur = maximus_render::editor::Cursor { path: vec![], pos: cu.editor.row.len() };
                }
            }
            CellAction::Up(id) => self.session.notebook.move_up(id),
            CellAction::Down(id) => self.session.notebook.move_down(id),
            CellAction::Duplicate(id) => {
                if let Some(n) = self.session.notebook.duplicate(id) {
                    self.focus_cell = Some(n);
                }
            }
            CellAction::InsertBelow(id, kind) => {
                let at = self.session.notebook.index_of(id).map_or(self.session.notebook.cells.len(), |i| i + 1);
                let n = if kind == CellKind::Code { self.add_code_cell(Some(at), Some(id)) } else { self.session.notebook.insert_cell(at, kind, InputMode::Maxima) };
                self.focus_cell = Some(n);
                self.active_cell = Some(n);
            }
            CellAction::ToggleCollapse(id) => {
                if let Some(c) = self.session.notebook.cell_mut(id) {
                    c.collapsed = !c.collapsed;
                }
            }
            CellAction::ClearOutput(id) => {
                if let Some(c) = self.session.notebook.cell_mut(id) {
                    c.outputs.clear();
                    c.state = CellState::Idle;
                }
            }
            CellAction::SetInput(id, kind) => self.set_cell_input(id, kind),
        }
    }

    /// Switches a cell between textbook, Maxima and LaTeX-source input, converting the content.
    fn set_cell_input(&mut self, id: u64, kind: InputKind) {
        self.ensure_cell_ui(id);
        let Some(cell) = self.session.notebook.cell(id) else { return };
        let (cur_mode, cur_latex) = (cell.mode, self.cell_ui.get(&id).map_or(false, |c| c.latex_view));
        let (want_mode, want_latex) = match kind {
            InputKind::Textbook => (InputMode::Math, false),
            InputKind::Latex => (InputMode::Math, true),
            InputKind::Maxima => (InputMode::Maxima, false),
        };
        if cur_mode == want_mode && cur_latex == want_latex {
            return;
        }
        let ctx = self.session.parse_ctx();
        if cur_mode != want_mode {
            let cell = self.session.notebook.cell(id).unwrap();
            let new_source = if cell.source.trim().is_empty() {
                String::new()
            } else if want_mode == InputMode::Maxima {
                match cell.to_maxima(&ctx) {
                    Ok(s) => s,
                    Err(e) => {
                        self.messages.push(format!("Cannot convert to Maxima: {}", e));
                        return;
                    }
                }
            } else {
                match maximus_math::maxima_script_to_latex(&cell.source) {
                    Ok(l) => l,
                    Err(e) => {
                        self.messages.push(format!("Cannot convert to textbook input: {}", e));
                        return;
                    }
                }
            };
            let c = self.session.notebook.cell_mut(id).unwrap();
            c.source = new_source;
            c.mode = want_mode;
        }
        // Rebuild the textbook editor from the (possibly converted) source, dropping a lone trailing `;`.
        let src = self.session.notebook.cell(id).unwrap().source.clone();
        if let Some(cu) = self.cell_ui.get_mut(&id) {
            if want_mode == InputMode::Math && !want_latex {
                cu.editor = maximus_render::editor::EditorState::from_latex_converted(&src);
                let converted = cu.editor.latex();
                self.session.notebook.cell_mut(id).unwrap().source = converted;
            }
            cu.latex_view = want_latex;
        }
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
                        _ => self.prose_cell(ui, idx, id, kind, actions),
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
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(RichText::new(format!("In {}", n)).small().weak());
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
                    match state {
                        CellState::Running => {
                            ui.put(rect, egui::Spinner::new().size(11.0));
                        }
                        CellState::Queued => {
                            for dx in [-3.5, 0.0, 3.5] {
                                ui.painter().circle_filled(egui::pos2(rect.center().x + dx, rect.center().y), 1.2, ui.visuals().weak_text_color());
                            }
                        }
                        CellState::Failed => icons::draw_icon(ui, Icon::Cross, rect.center(), 11.0, RED),
                        CellState::Done => icons::draw_icon(ui, Icon::Check, rect.center(), 11.0, GREEN),
                        CellState::Idle => {}
                    }
                });
                if icons::icon_button(ui, Icon::Play, egui::vec2(24.0, 22.0), true).on_hover_text("Evaluate").clicked() {
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
        let menu_btn = icons::icon_button(ui, Icon::Dots, egui::vec2(24.0, 22.0), true);
        egui::Popup::menu(&menu_btn).show(|ui| {
            if kind == CellKind::Code {
                let mode = self.session.notebook.cells[idx].mode;
                let latex = self.cell_ui.get(&id).map_or(false, |c| c.latex_view);
                for (label, k, on) in [
                    ("Textbook input", InputKind::Textbook, mode == InputMode::Math && !latex),
                    ("Maxima input", InputKind::Maxima, mode == InputMode::Maxima),
                    ("LaTeX source", InputKind::Latex, mode == InputMode::Math && latex),
                ] {
                    if ui.selectable_label(on, label).clicked() {
                        actions.push(CellAction::SetInput(id, k));
                        ui.close();
                    }
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

    fn prose_cell(&mut self, ui: &mut egui::Ui, idx: usize, id: u64, kind: CellKind, actions: &mut Vec<CellAction>) {
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
        let empty_back = self.session.notebook.cells.len() > 1
            && self.session.notebook.cells[idx].source.is_empty()
            && ui.memory(|m| m.has_focus(eid))
            && ui.input(|i| i.key_pressed(Key::Backspace));
        if empty_back {
            actions.push(CellAction::DeleteBack(id));
        }
        let c = &mut self.session.notebook.cells[idx];
        let te = if kind == CellKind::Text { egui::TextEdit::multiline(&mut c.source).desired_rows(1) } else { egui::TextEdit::singleline(&mut c.source) };
        let r = ui.add(te.id(eid).font(font).hint_text(hint).desired_width(ui.available_width()).frame(egui::Frame::NONE));
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
            let hint = if latex_view { "Type LaTeX here" } else { "Type Maxima code here" };
            if ui.memory(|m| m.has_focus(eid)) {
                submit = ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Enter));
            }
            let empty_back = self.session.notebook.cells.len() > 1
                && self.session.notebook.cells[idx].source.is_empty()
                && ui.memory(|m| m.has_focus(eid))
                && ui.input(|i| i.key_pressed(Key::Backspace));
            if empty_back {
                actions.push(CellAction::DeleteBack(id));
            }
            let src = &mut self.session.notebook.cells[idx].source;
            let out = egui::TextEdit::multiline(src).id(eid).code_editor().desired_rows(1).desired_width(ui.available_width()).hint_text(hint).show(ui);
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
            style.hint = "Type math here".into();
            let cu = self.cell_ui.get_mut(&id).unwrap();
            let out = math_editor(ui, eid, &mut cu.editor, &style);
            if out.changed {
                let latex = cu.editor.latex();
                self.session.notebook.cells[idx].source = latex;
            }
            focused = out.focused;
            submit = out.submit;
            if out.backspace_on_empty && self.session.notebook.cells.len() > 1 {
                actions.push(CellAction::DeleteBack(id));
            }
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
                // With a selection the copy actions apply to the selection only.
                let (latex, row, has_sel) = self
                    .cell_ui
                    .get_mut(&id)
                    .map(|c| match c.editor.selected_row() {
                        Some(r) => (maximus_math::row_to_latex(&r), r, true),
                        None => (c.editor.latex(), c.editor.row.clone(), false),
                    })
                    .unwrap_or_default();
                let suffix = if has_sel { " (selection)" } else { "" };
                if ui.button(format!("Copy as LaTeX{}", suffix)).clicked() {
                    ui.ctx().copy_text(latex.clone());
                    ui.close();
                }
                if ui.button(format!("Copy as Maxima{}", suffix)).clicked() {
                    match tree_to_plain_maxima(&row, &self.session.parse_ctx()) {
                        Ok(m) => ui.ctx().copy_text(m),
                        Err(e) => self.messages.push(format!("Cannot convert: {}", e)),
                    }
                    ui.close();
                }
                let in_matrix = self.cell_ui.get_mut(&id).map_or(false, |c| c.editor.in_matrix());
                if in_matrix {
                    ui.separator();
                    type MatrixOp = fn(&mut maximus_render::editor::EditorState);
                    let ops: [(&str, MatrixOp); 6] = [
                        ("Add row below", |e| e.matrix_add_row(true)),
                        ("Add row above", |e| e.matrix_add_row(false)),
                        ("Add column right", |e| e.matrix_add_col(true)),
                        ("Add column left", |e| e.matrix_add_col(false)),
                        ("Remove row", |e| e.matrix_remove_row()),
                        ("Remove column", |e| e.matrix_remove_col()),
                    ];
                    for (label, op) in ops {
                        if ui.button(label).clicked() {
                            if let Some(cu) = self.cell_ui.get_mut(&id) {
                                op(&mut cu.editor);
                                let latex = cu.editor.latex();
                                self.session.notebook.cells[idx].source = latex;
                            }
                            ui.close();
                        }
                    }
                    ui.separator();
                }
                if ui.button("Convert cell to Maxima input").clicked() {
                    actions.push(CellAction::SetInput(id, InputKind::Maxima));
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
