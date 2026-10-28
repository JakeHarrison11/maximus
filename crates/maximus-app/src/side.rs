//! Side panel: contextual help, variables, symbols and the function browser.

use crate::app::*;
use crate::catalog::CallTemplate;
use crate::icons::{self, Icon};
use eframe::egui;
use egui::{Key, RichText};
use maximus_help::DocEntry;
use maximus_math::Item;
use maximus_render::editor::EditorState;
use maximus_render::editor_ui::{math_editor, EditorStyle};

impl MaximusApp {
    pub fn side_panel(&mut self, ui: &mut egui::Ui) {
        // Never let content demand more width than the panel has.
        ui.set_max_width(ui.available_width());
        ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (tab, label) in [(SideTab::Help, "Help"), (SideTab::Variables, "Variables"), (SideTab::Symbols, "Symbols"), (SideTab::Functions, "Functions")] {
                ui.selectable_value(&mut self.tab, tab, label);
            }
        });
        ui.separator();
        match self.tab {
            SideTab::Help => self.help_tab(ui),
            SideTab::Variables => self.variables_tab(ui),
            SideTab::Symbols => self.symbols_tab(ui),
            SideTab::Functions => self.functions_tab(ui),
        }
    }

    fn help_tab(&mut self, ui: &mut egui::Ui) {
        if self.help.is_empty() {
            ui.label("The Maxima manual was not found on this system.");
            ui.label(RichText::new("Install the Maxima documentation (Info files) to enable integrated help.").weak());
            return;
        }
        ui.add(egui::TextEdit::singleline(&mut self.help_search).hint_text("Search functions…").desired_width(ui.available_width()));
        if !self.help_search.trim().is_empty() {
            let q = self.help_search.trim().to_string();
            let results: Vec<(String, String)> = self.help.search(&q, 30).iter().map(|e| (e.name.clone(), e.kind.clone())).collect();
            egui::ScrollArea::vertical().id_salt("help-results").show(ui, |ui| {
                for (name, kind) in results {
                    ui.horizontal(|ui| {
                        if ui.link(&name).clicked() {
                            self.help_topic = Some(name.clone());
                            self.help_search.clear();
                        }
                        ui.label(RichText::new(kind).small().weak());
                    });
                }
            });
            return;
        }
        let Some(topic) = self.help_topic.clone() else {
            ui.label("Place the cursor on a function name, or search above.");
            return;
        };
        let entries: Vec<DocEntry> = self.help.lookup(&topic).into_iter().cloned().collect();
        if entries.is_empty() {
            ui.label(format!("No documentation found for “{}”.", topic));
            return;
        }
        egui::ScrollArea::vertical().id_salt("help-doc").show(ui, |ui| {
            for e in entries {
                ui.heading(&e.name);
                ui.label(RichText::new(format!("{} — {}", e.kind, e.category)).small().weak());
                for sig in &e.signatures {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(sig).monospace().strong());
                        if e.is_function() && ui.small_button("Use…").on_hover_text("Insert a call using this signature").clicked() {
                            self.open_call_dialog(CallTemplate::from_signature(sig));
                        }
                    });
                }
                ui.separator();
                for (code, text) in e.reflowed() {
                    if code {
                        ui.add(egui::Label::new(RichText::new(text).monospace().size(12.0)));
                    } else {
                        ui.add(egui::Label::new(RichText::new(text).size(13.5)).wrap());
                    }
                    ui.add_space(4.0);
                }
                ui.add_space(10.0);
            }
        });
    }

    fn variables_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.session.refresh_variables();
            }
            if ui.button("Reset all").on_hover_text("kill(all): forget every variable and function").clicked() {
                self.session.reset_variables();
            }
        });
        ui.separator();
        let vars = self.session.variables.clone();
        if vars.is_empty() {
            ui.label(RichText::new("No user-defined variables yet.\nTry  a: 5  or  f(x) := x^2").weak());
        }
        let color = ui.visuals().text_color();
        // A little larger than the original 16, a little smaller than the math size.
        let size = self.settings.math_size * 0.9;
        egui::ScrollArea::vertical().id_salt("vars").max_height((ui.available_height() - 150.0).max(80.0)).show(ui, |ui| {
            for v in &vars {
                ui.horizontal_top(|ui| {
                    if icons::icon_button(ui, Icon::Close, egui::vec2(20.0, 20.0), false).on_hover_text("Delete").clicked() {
                        let name = v.name.clone();
                        self.session.remove_variable(&name);
                    }
                    let editing = self.var_edit.as_ref().map_or(false, |e| e.name == v.name);
                    if editing {
                        let mut edit = self.var_edit.take().unwrap();
                        let id = ui.make_persistent_id(("var-edit", &v.name));
                        if edit.fresh {
                            ui.memory_mut(|m| m.request_focus(id));
                            edit.fresh = false;
                        }
                        let style = EditorStyle::new(size, color);
                        let out = math_editor(ui, id, &mut edit.editor, &style);
                        if out.submit {
                            self.commit_var_edit(edit);
                        } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                            // cancelled
                        } else {
                            self.var_edit = Some(edit);
                        }
                    } else {
                        egui::ScrollArea::horizontal().id_salt(("var-scroll", &v.name)).auto_shrink([false, true]).show(ui, |ui| {
                            if v.is_function {
                                match self.tree_for(&v.value) {
                                    Some(t) => {
                                        let r = maximus_render::show_math(ui, &t, size, color);
                                        if r.double_clicked() {
                                            self.start_var_edit(v);
                                        }
                                        r.on_hover_text("Double-click to edit");
                                    }
                                    None => {
                                        ui.label(RichText::new(&v.value).monospace());
                                    }
                                }
                            } else {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&v.name).italics().strong());
                                    ui.label("=");
                                    match self.tree_for(&v.value) {
                                        Some(t) => {
                                            let r = maximus_render::show_math(ui, &t, size, color);
                                            if r.double_clicked() {
                                                self.start_var_edit(v);
                                            }
                                            r.on_hover_text("Double-click to edit");
                                        }
                                        None => {
                                            ui.label(RichText::new(&v.value).monospace());
                                        }
                                    }
                                });
                            }
                        });
                    }
                });
                ui.separator();
            }
        });
        ui.separator();
        ui.label("Define variable");
        let mut define = false;
        ui.horizontal_top(|ui| {
            let total = ui.available_width();
            let style = EditorStyle::new(size * 0.9, color);
            let name_id = ui.make_persistent_id("new-var-name");
            let value_id = ui.make_persistent_id("new-var-value");
            let mut s1 = style.clone_with_hint("name");
            s1.min_height = 0.0;
            ui.allocate_ui(egui::vec2(total * 0.3, 0.0), |ui| {
                let out = math_editor(ui, name_id, &mut self.new_var_name, &s1);
                if out.submit {
                    ui.memory_mut(|m| m.request_focus(value_id));
                }
            });
            ui.label("=");
            let s2 = style.clone_with_hint("value");
            let w = (ui.available_width() - 56.0).max(60.0);
            ui.allocate_ui(egui::vec2(w, 0.0), |ui| {
                let out = math_editor(ui, value_id, &mut self.new_var_value, &s2);
                if out.submit {
                    define = true;
                }
            });
            if ui.button("Set").clicked() {
                define = true;
            }
        });
        if define {
            self.define_variable();
        }
    }

    /// Starts editing a variable or function definition as textbook input.
    fn start_var_edit(&mut self, v: &maximus_core::VarInfo) {
        let latex = if v.is_function {
            maximus_math::maxima_script_to_latex(&v.value)
        } else {
            maximus_math::maxima_to_latex(&v.value)
        }
        .unwrap_or_else(|_| v.value.clone());
        self.var_edit = Some(VarEdit { name: v.name.clone(), is_function: v.is_function, editor: EditorState::from_latex(&latex), fresh: true });
    }

    fn commit_var_edit(&mut self, e: VarEdit) {
        let ctx = self.session.parse_ctx();
        if e.is_function {
            match maximus_math::tree_to_maxima(&e.editor.row, &ctx) {
                Ok(code) => {
                    self.session.submit_raw(&format!("{}$", code.trim_end_matches(';')));
                    self.session.refresh_variables();
                }
                Err(err) => self.messages.push(format!("Cannot define {}: {}", e.name, err)),
            }
        } else {
            match maximus_math::tree_to_plain_maxima(&e.editor.row, &ctx) {
                Ok(value) => self.session.set_variable(&e.name, &value),
                Err(err) => self.messages.push(format!("Cannot set {}: {}", e.name, err)),
            }
        }
    }

    fn define_variable(&mut self) {
        let name = match variable_name_from_row(&self.new_var_name.row) {
            Ok(n) => n,
            Err(e) => {
                self.messages.push(e);
                return;
            }
        };
        if self.new_var_value.row.is_empty() {
            self.messages.push("Enter a value for the variable".into());
            return;
        }
        match maximus_math::tree_to_plain_maxima(&self.new_var_value.row, &self.session.parse_ctx()) {
            Ok(value) => {
                self.session.set_variable(&name, &value);
                self.new_var_name.clear();
                self.new_var_value.clear();
            }
            Err(e) => self.messages.push(format!("Cannot set {}: {}", name, e)),
        }
    }

    fn symbols_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("symbols").show(ui, |ui| {
            ui.label(RichText::new("Click to insert at the cursor.").weak().small());
            egui::CollapsingHeader::new("Structures").default_open(true).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (label, t) in structure_buttons() {
                        let short = label.split("  ").next().unwrap_or(label);
                        if ui.button(RichText::new(short).size(18.0)).on_hover_text(label.split("  ").nth(1).unwrap_or(label)).clicked() {
                            self.pending.push(Insert::Template(t));
                        }
                    }
                });
            });
            for g in self.all_palettes() {
                egui::CollapsingHeader::new(&g.name).default_open(true).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
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
            let snippets: Vec<(String, String)> = self.plugins.iter().filter(|p| p.enabled).flat_map(|p| p.manifest.snippet.iter().map(|s| (s.label.clone(), s.code.clone()))).collect();
            if !snippets.is_empty() {
                egui::CollapsingHeader::new("Plugin snippets").show(ui, |ui| {
                    for (l, c) in snippets {
                        if ui.button(l).clicked() {
                            self.pending.push(Insert::Call { text: c, evaluate: false, pre: None });
                        }
                    }
                });
            }
        });
    }

    fn functions_tab(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::TextEdit::singleline(&mut self.func_search).hint_text("Find a function…").desired_width(ui.available_width()));
        ui.separator();
        let q = self.func_search.trim().to_lowercase();
        egui::ScrollArea::vertical().id_salt("functions").show(ui, |ui| {
            if !q.is_empty() {
                let mut hits: Vec<CallTemplate> = Vec::new();
                for c in &self.catalog {
                    hits.extend(c.items.iter().filter(|i| i.label.to_lowercase().contains(&q) || i.template.to_lowercase().contains(&q)).cloned());
                }
                for (_, items) in self.plugin_templates() {
                    hits.extend(items.into_iter().filter(|i| i.label.to_lowercase().contains(&q)));
                }
                for t in hits {
                    if ui.button(&t.label).on_hover_text(&t.template).clicked() {
                        self.open_call_dialog(t);
                    }
                }
                ui.separator();
                let docs: Vec<(String, Option<String>)> = self.help.search(&q, 40).iter().filter(|e| e.is_function()).map(|e| (e.name.clone(), e.signatures.first().cloned())).collect();
                for (name, sig) in docs {
                    ui.horizontal(|ui| {
                        if ui.link(&name).clicked() {
                            if let Some(s) = &sig {
                                self.open_call_dialog(CallTemplate::from_signature(s));
                            }
                        }
                        if ui.small_button("?").clicked() {
                            self.show_help(&name);
                        }
                    });
                }
                return;
            }
            let cats = std::mem::take(&mut self.catalog);
            for cat in &cats {
                egui::CollapsingHeader::new(cat.name).show(ui, |ui| {
                    for it in &cat.items {
                        let r = ui.button(&it.label);
                        let r = if it.description.is_empty() { r } else { r.on_hover_text(&it.description) };
                        if r.clicked() {
                            self.open_call_dialog(it.clone());
                        }
                    }
                });
            }
            self.catalog = cats;
            for (cat, items) in self.plugin_templates() {
                egui::CollapsingHeader::new(format!("{} (plugin)", cat)).show(ui, |ui| {
                    for it in items {
                        if ui.button(&it.label).clicked() {
                            self.open_call_dialog(it);
                        }
                    }
                });
            }
            ui.separator();
            ui.label(RichText::new("Maxima manual").strong());
            let cats = std::mem::take(&mut self.manual_cats);
            for (name, entries) in &cats {
                let title = name.trim_start_matches("Functions and Variables for ").to_string();
                egui::CollapsingHeader::new(title).id_salt(("manual", name)).show(ui, |ui| {
                    for (fname, sig) in entries {
                        ui.horizontal(|ui| {
                            if ui.link(fname).on_hover_text(sig.clone().unwrap_or_default()).clicked() {
                                if let Some(s) = sig {
                                    self.open_call_dialog(CallTemplate::from_signature(s));
                                }
                            }
                            if ui.small_button("?").clicked() {
                                self.show_help(fname);
                            }
                        });
                    }
                });
            }
            self.manual_cats = cats;
        });
    }
}

/// A variable name typed as textbook input: letters, digits and Greek letters, with an optional subscript.
pub fn variable_name_from_row(row: &[Item]) -> Result<String, String> {
    use maximus_math::words::greek_name_of_glyph;
    let bad = || "A variable name is letters and digits, like x, mass, \u{3b1} or x_1".to_string();
    if row.is_empty() {
        return Err("Enter a name for the variable".into());
    }
    let mut name = String::new();
    for it in row {
        match it {
            Item::Char(c) if c.is_ascii_alphanumeric() || *c == '_' => name.push(*c),
            Item::Char(c) => name.push_str(greek_name_of_glyph(*c).ok_or_else(bad)?),
            Item::Sub(s) => {
                name.push('_');
                for i in s {
                    match i {
                        Item::Char(c) if c.is_ascii_alphanumeric() => name.push(*c),
                        _ => return Err(bad()),
                    }
                }
            }
            _ => return Err(bad()),
        }
    }
    if name.chars().next().map_or(true, |c| c.is_ascii_digit() || c == '_') {
        return Err(bad());
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maximus_math::row_from_str;

    #[test]
    fn variable_names_from_textbook_input() {
        assert_eq!(variable_name_from_row(&row_from_str("mass")).unwrap(), "mass");
        assert_eq!(variable_name_from_row(&[Item::Char('α')]).unwrap(), "alpha");
        assert_eq!(variable_name_from_row(&[Item::Char('x'), Item::Sub(row_from_str("12"))]).unwrap(), "x_12");
        assert!(variable_name_from_row(&row_from_str("2x")).is_err());
        assert!(variable_name_from_row(&row_from_str("a+b")).is_err());
        assert!(variable_name_from_row(&[]).is_err());
    }
}
