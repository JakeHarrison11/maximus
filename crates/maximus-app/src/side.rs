//! Side panel: contextual help, variables, symbols and the function browser.

use crate::app::*;
use crate::catalog::CallTemplate;
use eframe::egui;
use egui::{Key, RichText};
use maximus_help::DocEntry;

impl MaximusApp {
    pub fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
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
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(&mut self.help_search).hint_text("Search functions…").desired_width(f32::INFINITY));
        });
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
                    ui.horizontal(|ui| {
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
            if ui.button("⟳ Refresh").clicked() {
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
        egui::ScrollArea::vertical().id_salt("vars").max_height(ui.available_height() - 70.0).show(ui, |ui| {
            for v in &vars {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new(if v.is_function { "ƒ" } else { "=" }).weak());
                    if ui.small_button("✕").on_hover_text("Delete").clicked() {
                        let name = v.name.clone();
                        self.session.remove_variable(&name);
                    }
                    let editing = self.var_edit.as_ref().map_or(false, |(n, _)| *n == v.name);
                    if editing {
                        let (name, text) = self.var_edit.as_mut().unwrap();
                        let r = ui.add(egui::TextEdit::singleline(text).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                        r.request_focus();
                        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            let (n, t) = (name.clone(), text.clone());
                            if v.is_function {
                                self.session.submit_raw(&format!("{}$", t.trim_end_matches([';', '$'])));
                                self.session.refresh_variables();
                            } else {
                                self.session.set_variable(&n, &t);
                            }
                            self.var_edit = None;
                        } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                            self.var_edit = None;
                        }
                    } else {
                        ui.vertical(|ui| {
                            if v.is_function {
                                match self.tree_for(&v.value) {
                                    Some(t) => {
                                        let r = maximus_render::show_math(ui, &t, 16.0, color);
                                        if r.double_clicked() {
                                            self.var_edit = Some((v.name.clone(), v.value.clone()));
                                        }
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
                                            let r = maximus_render::show_math(ui, &t, 16.0, color);
                                            if r.double_clicked() {
                                                self.var_edit = Some((v.name.clone(), v.value.clone()));
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
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_var.0).hint_text("name").desired_width(70.0));
            ui.label(":");
            let r = ui.add(egui::TextEdit::singleline(&mut self.new_var.1).hint_text("value").font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
            let go = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (go || ui.button("Set").clicked()) && !self.new_var.0.trim().is_empty() && !self.new_var.1.trim().is_empty() {
                let (n, v) = (self.new_var.0.trim().to_string(), self.new_var.1.trim().to_string());
                self.session.set_variable(&n, &v);
                self.new_var = (String::new(), String::new());
            }
        });
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
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(&mut self.func_search).hint_text("Find a function…").desired_width(f32::INFINITY));
        });
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
