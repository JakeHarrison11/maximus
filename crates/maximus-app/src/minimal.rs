//! Minimal (calculator) mode: one input line, a history of results and a keypad.
//! The full CAS stays available; it uses the same notebook and session.

use crate::app::*;
use crate::icons::{self, Icon};
use eframe::egui;
use egui::{Align, Color32, Layout, RichText, Vec2};
use maximus_core::*;
use maximus_math::from_latex;
use maximus_render::editor::{EditorState, Template};
use maximus_render::editor_ui::{math_editor, EditorStyle};
use std::sync::Arc;

/// Shows math that never widens its container: right-aligned when it fits, otherwise in a horizontal scroll area.
fn math_fit(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, tree: &[maximus_math::Item], size: f32, color: Color32, right: bool) -> egui::Response {
    let w = maximus_render::layout_tree(ui.ctx(), tree, size, false).w + 8.0;
    let avail = ui.available_width();
    if w <= avail {
        if right {
            ui.add_space(0.0);
            return ui.with_layout(Layout::top_down(Align::Max), |ui| maximus_render::show_math(ui, tree, size, color)).inner;
        }
        return maximus_render::show_math(ui, tree, size, color);
    }
    egui::ScrollArea::horizontal().id_salt(("fit", id)).auto_shrink([false, true]).show(ui, |ui| maximus_render::show_math(ui, tree, size, color)).inner
}

impl MaximusApp {
    pub fn apply_insert_minimal(&mut self, ins: Insert) {
        match ins {
            Insert::Symbol(c) => self.min_editor.insert_symbol(c),
            Insert::Template(t) => self.min_editor.insert_template(t),
            Insert::Text(s) => self.min_editor.paste(&s),
            Insert::Call { text, evaluate, pre } => {
                if let Some(p) = pre {
                    self.session.submit_raw(&p);
                }
                self.min_editor.paste(&text);
                if evaluate {
                    self.submit_minimal();
                }
            }
        }
    }

    fn submit_minimal(&mut self) {
        if self.min_editor.is_empty() {
            return;
        }
        let latex = self.min_editor.latex();
        let id = self.session.notebook.add_cell(CellKind::Code, InputMode::Math);
        self.session.notebook.cell_mut(id).unwrap().source = latex;
        self.session.evaluate_cell(id);
        self.min_editor.clear();
        self.min_scroll_to_end = true;
        self.min_scroll_frames = 6;
    }

    fn key(&mut self, ui: &mut egui::Ui, label: &str, ins: Insert, w: f32) {
        if ui.add_sized(Vec2::new(w, 34.0), egui::Button::new(RichText::new(label).size(17.0))).clicked() {
            self.pending.push(ins);
        }
    }

    pub fn minimal_ui(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(ui.available_width());
        let size = self.settings.math_size + 2.0;
        let color = ui.visuals().text_color();
        ui.horizontal(|ui| {
            ui.heading("Maximus");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Full mode").on_hover_text("Ctrl+M").clicked() {
                    self.settings.minimal = false;
                    self.settings.save();
                }
                ui.checkbox(&mut self.show_keypad, "Keypad");
                if ui.button("Clear history").clicked() {
                    self.session.notebook = Notebook::empty();
                    self.cell_ui.clear();
                }
            });
        });
        ui.separator();

        egui::Panel::bottom("min-input").resizable(false).show(ui, |ui| {
            let eid = ui.make_persistent_id("minimal-input");
            let mut style = EditorStyle::new(size, color);
            style.hint = "Type an expression and press Enter".into();
            let out = math_editor(ui, eid, &mut self.min_editor, &style);
            if out.submit {
                self.submit_minimal();
            }
            if !self.min_editor.is_empty() || out.focused {
                // keep typing without clicking after the keypad is used
            } else if !ui.memory(|m| m.focused().is_some()) {
                ui.memory_mut(|m| m.request_focus(eid));
            }
            if self.settings.contextual_help {
                if let Some(w) = out.help_word {
                    if self.help.first(&w).is_some() {
                        self.help_topic = Some(w);
                    }
                }
            }

            if !self.show_keypad {
                return;
            }
            ui.add_space(4.0);
            // Eight columns shrink to fit narrow windows instead of being clipped.
            let key_w = ((ui.available_width() - 7.0 * 4.0) / 8.0).clamp(30.0, 46.0).floor();
            egui::Grid::new("keypad").spacing([4.0, 4.0]).show(ui, |ui| {
                let w = key_w;
                // Row 1
                self.key(ui, "x²", Insert::Template(Template::Sup), w);
                self.key(ui, "√", Insert::Template(Template::Sqrt), w);
                self.key(ui, "a/b", Insert::Template(Template::Frac), w);
                self.key(ui, "( )", Insert::Template(Template::Paren), w);
                self.key(ui, "π", Insert::Symbol('π'), w);
                self.key(ui, "ⅇ", Insert::Symbol('ⅇ'), w);
                self.key(ui, "ⅈ", Insert::Symbol('ⅈ'), w);
                self.key(ui, "∞", Insert::Symbol('∞'), w);
                ui.end_row();
                // Row 2
                self.key(ui, "sin", Insert::Text("sin()".into()), w);
                self.key(ui, "cos", Insert::Text("cos()".into()), w);
                self.key(ui, "tan", Insert::Text("tan()".into()), w);
                self.key(ui, "ln", Insert::Text("log()".into()), w);
                self.key(ui, "x", Insert::Symbol('x'), w);
                self.key(ui, "y", Insert::Symbol('y'), w);
                self.key(ui, "Ans", Insert::Symbol('%'), w);
                self.key(ui, "=", Insert::Symbol('='), w);
                ui.end_row();
                // Row 3
                self.key(ui, "∫", Insert::Template(Template::Integral), w);
                self.key(ui, "∫ₐᵇ", Insert::Template(Template::DefIntegral), w);
                self.key(ui, "d/dx", Insert::Template(Template::Derivative), w);
                self.key(ui, "Σ", Insert::Template(Template::Sum), w);
                self.key(ui, "lim", Insert::Template(Template::Limit), w);
                self.key(ui, "|x|", Insert::Template(Template::Abs), w);
                self.key(ui, "xⁿ", Insert::Template(Template::Sup), w);
                self.key(ui, "xₙ", Insert::Template(Template::Sub), w);
                ui.end_row();
            });
            ui.add_space(4.0);
            egui::Grid::new("digits").spacing([4.0, 4.0]).show(ui, |ui| {
                let w = 54.0;
                for row in [["7", "8", "9", "÷"], ["4", "5", "6", "×"], ["1", "2", "3", "−"], ["0", ".", ",", "+"]] {
                    for k in row {
                        let c = k.chars().next().unwrap();
                        self.key(ui, k, Insert::Symbol(c), w);
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                if icons::icon_button(ui, Icon::Backspace, Vec2::new(118.0, 34.0), true).clicked() {
                    self.min_editor.backspace();
                }
                if ui.add_sized(Vec2::new(118.0, 34.0), egui::Button::new(RichText::new("C").size(17.0))).clicked() {
                    self.min_editor.clear();
                }
                let enter = egui::Button::new(RichText::new("=  Enter").size(17.0).color(Color32::WHITE)).fill(Color32::from_rgb(0x2e, 0x7d, 0x32));
                if ui.add_sized(Vec2::new(118.0, 34.0), enter).clicked() {
                    self.submit_minimal();
                }
            });
        });

        let mut recall: Option<String> = None;
        let mut approx: Option<String> = None;
        egui::ScrollArea::vertical().max_height(ui.available_height()).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            let cells: Vec<(u64, String, InputMode, Vec<OutputItem>, CellState)> = self
                .session
                .notebook
                .cells
                .iter()
                .filter(|c| c.kind == CellKind::Code && !c.is_empty())
                .map(|c| (c.id, c.source.clone(), c.mode, c.outputs.clone(), c.state))
                .collect();
            for (id, src, mode, outs, state) in cells {
                ui.add_space(4.0);
                let row = Arc::new(match mode {
                    InputMode::Math => from_latex(&src),
                    InputMode::Maxima => maximus_math::maxima_script_to_tree(&src).unwrap_or_else(|_| maximus_math::row_from_str(&src)),
                });
                let r = math_fit(ui, ("in", id), &row, size * 0.8, color.gamma_multiply(0.7), false);
                if r.on_hover_text("Click to edit this input").clicked() {
                    recall = Some(match mode {
                        InputMode::Math => src.clone(),
                        InputMode::Maxima => maximus_math::maxima_script_to_latex(&src).unwrap_or_else(|_| src.clone()),
                    });
                }
                if matches!(state, CellState::Running | CellState::Queued) {
                    ui.spinner();
                }
                for (k, o) in outs.iter().enumerate() {
                    match o {
                        OutputItem::Result { text, label, .. } => {
                            ui.with_layout(Layout::top_down(Align::Max), |ui| {
                                // Results can be selected; right-click offers copy and "Use as input".
                                match self.selectable_math(ui, (id, k), text, size * 1.15, color) {
                                    Some((resp, sel)) => {
                                        if sel.is_some() || !self.help_words_in(text).is_empty() {
                                            resp.context_menu(|ui| self.result_menu(ui, text, sel.clone()));
                                        }
                                    }
                                    None => {
                                        ui.label(RichText::new(text).monospace().size(size * 0.8));
                                    }
                                }
                                if ui.small_button("≈ decimal").on_hover_text("Decimal approximation").clicked() {
                                    approx = Some(label.clone());
                                }
                            });
                        }
                        other => self.show_output(ui, id, k, other),
                    }
                }
                ui.separator();
            }
            // Keep following the bottom until results have arrived and been laid out.
            let end = ui.allocate_response(Vec2::new(1.0, 1.0), egui::Sense::hover());
            if self.min_scroll_to_end {
                end.scroll_to_me(Some(Align::BOTTOM));
                if self.session.is_busy() {
                    self.min_scroll_frames = 6;
                } else if self.min_scroll_frames > 0 {
                    self.min_scroll_frames -= 1;
                } else {
                    self.min_scroll_to_end = false;
                }
                ui.ctx().request_repaint();
            }
        });
        if let Some(src) = recall {
            self.min_editor = EditorState::from_latex(&src);
        }
        if let Some(label) = approx {
            let id = self.session.notebook.add_cell(CellKind::Code, InputMode::Maxima);
            self.session.notebook.cell_mut(id).unwrap().source = format!("float({});", label);
            self.session.evaluate_cell(id);
            self.min_scroll_to_end = true;
        }
    }
}
