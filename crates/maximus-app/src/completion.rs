//! Identifier completion shared by notebook and calculator inputs.

use crate::app::MaximusApp;
use eframe::egui::{self, Id, Key};
use maximus_render::editor::EditorState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    name: String,
    function: bool,
}

#[derive(Clone, Default)]
struct MenuState {
    prefix: String,
    selected: usize,
    dismissed: bool,
}

fn matches_prefix(candidate: &Completion, prefix: &str) -> bool {
    !prefix.is_empty() && (!candidate.function || prefix.chars().count() >= 2)
        && candidate.name.starts_with(prefix) && (candidate.name != prefix || candidate.function)
}

fn consume_key(ui: &mut egui::Ui, key: Key) -> bool {
    ui.input_mut(|input| {
        let mut consumed = false;
        input.events.retain(|event| {
            let matches = matches!(event, egui::Event::Key { key: pressed, pressed: true, modifiers, .. } if *pressed == key && modifiers.is_none());
            consumed |= matches;
            !matches
        });
        consumed
    })
}

impl MaximusApp {
    pub fn completion_candidates(&self) -> Vec<Completion> {
        let mut names = std::collections::BTreeMap::new();
        for name in maximus_math::words::FUNC_WORDS {
            names.insert((*name).to_string(), true);
        }
        for entry in self.help.entries.iter().filter(|e| e.is_function()) {
            names.insert(entry.name.clone(), true);
        }
        for call in self.catalog.iter().flat_map(|category| &category.items) {
            names.insert(call.function_name(), true);
        }
        for variable in &self.session.variables {
            names.insert(variable.name.clone(), variable.is_function);
        }
        names.into_iter()
            .filter(|(name, _)| name.starts_with(|c: char| c.is_alphabetic() || c == '%')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '%'))
            .map(|(name, function)| Completion { name, function })
            .collect()
    }
}

fn choose(ui: &mut egui::Ui, id: Id, prefix: Option<&str>, candidates: &[Completion]) -> Option<Completion> {
    let response = ui.ctx().read_response(id)?;
    let popup_id = id.with("completion");
    let clicking_popup = ui.ctx().read_response(popup_id).is_some_and(|r| {
        ui.input(|i| i.pointer.any_down() || i.pointer.any_released())
            && ui.ctx().pointer_interact_pos().is_some_and(|pos| r.rect.contains(pos))
    });
    if !ui.memory(|m| m.has_focus(id)) && !clicking_popup {
        ui.data_mut(|d| d.remove::<MenuState>(popup_id));
        return None;
    }
    let Some(prefix) = prefix.filter(|prefix| !prefix.is_empty()) else {
        ui.data_mut(|d| d.remove::<MenuState>(popup_id));
        return None;
    };
    let matches: Vec<&Completion> = candidates.iter()
        .filter(|c| matches_prefix(c, prefix))
        .take(8)
        .collect();
    if matches.is_empty() {
        ui.data_mut(|d| d.remove::<MenuState>(popup_id));
        return None;
    }
    let mut state = ui.data_mut(|d| d.get_temp::<MenuState>(popup_id)).unwrap_or_default();
    if state.prefix != prefix {
        state = MenuState { prefix: prefix.to_string(), ..Default::default() };
    }
    state.selected = state.selected.min(matches.len() - 1);
    if consume_key(ui, Key::Escape) {
        state.dismissed = true;
    }
    let mut picked = None;
    if !state.dismissed {
        if consume_key(ui, Key::ArrowDown) {
            state.selected = (state.selected + 1) % matches.len();
        }
        if consume_key(ui, Key::ArrowUp) {
            state.selected = (state.selected + matches.len() - 1) % matches.len();
        }
        if consume_key(ui, Key::Tab) {
            picked = Some(matches[state.selected].clone());
        }
        egui::Popup::from_response(&response).id(popup_id).open(true).show(|ui| {
            ui.set_min_width(180.0);
            for (index, candidate) in matches.iter().enumerate() {
                let label = if candidate.function { format!("{}(...)", candidate.name) } else { candidate.name.clone() };
                if ui.selectable_label(index == state.selected, label).clicked() {
                    picked = Some((*candidate).clone());
                }
            }
            ui.label(egui::RichText::new("Tab to insert; Esc to dismiss").small().weak());
        });
    }
    if picked.is_some() {
        state = MenuState::default();
        ui.memory_mut(|m| m.request_focus(id));
    }
    ui.data_mut(|d| d.insert_temp(popup_id, state));
    picked
}

pub fn math_editor(
    ui: &mut egui::Ui,
    id: Id,
    editor: &mut EditorState,
    style: &maximus_render::editor_ui::EditorStyle,
    candidates: &[Completion],
) -> maximus_render::editor_ui::EditorOutput {
    let prefix = editor.completion_prefix();
    let completed = choose(ui, id, prefix.as_deref(), candidates);
    if let Some(candidate) = &completed {
        editor.complete_identifier(&candidate.name, candidate.function);
    }
    let pointer_on_popup = ui.ctx().read_response(id.with("completion")).is_some_and(|r| {
        ui.ctx().pointer_interact_pos().is_some_and(|pos| r.rect.contains(pos))
    });
    let mut style = style.clone_with_hint(&style.hint);
    style.keep_focus |= pointer_on_popup;
    let mut out = maximus_render::editor_ui::math_editor(ui, id, editor, &style);
    out.changed |= completed.is_some();
    let prefix = editor.completion_prefix();
    let state = ui.data_mut(|d| d.get_temp::<MenuState>(id.with("completion"))).unwrap_or_default();
    let open = prefix.is_some_and(|prefix| {
        !(state.prefix == prefix && state.dismissed)
            && candidates.iter().any(|c| matches_prefix(c, &prefix))
    });
    if out.focused {
        ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter {
            tab: open || editor.in_matrix(), escape: open, horizontal_arrows: true, vertical_arrows: true,
        }));
    }
    out
}

pub fn maxima_editor(ui: &mut egui::Ui, id: Id, source: &mut String, hint: &str, candidates: &[Completion]) -> egui::text_edit::TextEditOutput {
    let mut output = egui::CompletionPopup::new(id)
        .word_boundary(|c| !c.is_alphanumeric() && c != '_' && c != '%')
        .accept_keys(vec![Key::Tab])
        .show(ui, egui::TextEdit::multiline(source).code_editor().desired_rows(1).desired_width(ui.available_width()).hint_text(hint), |query| {
            if query.word.is_empty() || !query.word.starts_with(|c: char| c.is_alphabetic() || c == '%') {
                return vec![];
            }
            let existing_paren = query.text.chars().nth(query.word_range.end.0) == Some('(');
            candidates.iter().filter(|c| matches_prefix(c, query.word)).take(8)
                .map(|c| {
                    let insert = if c.function && !existing_paren { format!("{}()", c.name) } else { c.name.clone() };
                    egui::Suggestion::new(insert).content(if c.function { format!("{}(...)", c.name) } else { c.name.clone() })
                }).collect()
        });
    if let Some(accepted) = &output.accepted {
        let function = candidates.iter().any(|c| c.function && accepted.insert.trim_end_matches("()") == c.name);
        if function {
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap();
            if let Some(cursor) = state.cursor.char_range().and_then(|r| r.single()) {
                let offset = if accepted.insert.ends_with("()") { cursor.index.0 - 1 } else { cursor.index.0 + 1 };
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(offset))));
                state.store(ui.ctx(), id);
            }
        }
        output.text_edit.response.mark_changed();
    }
    if output.text_edit.response.has_focus() {
        ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter {
            tab: true, escape: output.is_open, horizontal_arrows: true, vertical_arrows: true,
        }));
    }
    output.text_edit
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Modifiers, RawInput};

    fn key(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    fn math_frame(ctx: &Context, id: Id, editor: &mut EditorState, events: Vec<Event>) -> bool {
        let candidates = [
            Completion { name: "integrate".into(), function: true },
            Completion { name: "invert".into(), function: true },
            Completion { name: "mass".into(), function: false },
        ];
        let mut submitted = false;
        let mut output = ctx.run_ui(RawInput { events, ..Default::default() }, |ui| {
            submitted |= math_editor(
                ui, id, editor, &maximus_render::editor_ui::EditorStyle::new(20.0, egui::Color32::WHITE), &candidates,
            ).submit;
        });
        output.textures_delta.clear();
        submitted
    }

    #[test]
    fn textbook_completion_handles_keyboard_dismissal_and_undo() {
        let ctx = Context::default();
        maximus_render::install_fonts(&ctx);
        let id = Id::unique("completion-test");
        let mut editor = EditorState::default();
        math_frame(&ctx, id, &mut editor, vec![]);
        ctx.memory_mut(|m| m.request_focus(id));
        math_frame(&ctx, id, &mut editor, vec![Event::Text("in".into())]);
        math_frame(&ctx, id, &mut editor, vec![key(Key::ArrowDown)]);
        math_frame(&ctx, id, &mut editor, vec![key(Key::Tab)]);
        assert_eq!(editor.latex(), "\\mathrm{invert}\\left(  \\right)");
        assert!(ctx.memory(|m| m.has_focus(id)));
        let undo = Event::Key { key: Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND };
        math_frame(&ctx, id, &mut editor, vec![undo]);
        assert_eq!(editor.latex(), "in");
        math_frame(&ctx, id, &mut editor, vec![key(Key::Escape)]);
        assert!(math_frame(&ctx, id, &mut editor, vec![key(Key::Enter)]));
        assert_eq!(editor.latex(), "in");
    }

    #[test]
    fn raw_completion_preserves_unicode_and_places_cursor_inside_call() {
        let ctx = Context::default();
        let id = Id::unique("raw-completion-test");
        let mut source = String::new();
        let candidates = [Completion { name: "integrate".into(), function: true }];
        let frame = |source: &mut String, events| {
            let mut output = ctx.run_ui(RawInput { events, ..Default::default() }, |ui| {
                maxima_editor(ui, id, source, "", &candidates);
            });
            output.textures_delta.clear();
        };
        frame(&mut source, vec![]);
        ctx.memory_mut(|m| m.request_focus(id));
        frame(&mut source, vec![Event::Text("α + inte".into())]);
        frame(&mut source, vec![key(Key::Tab)]);
        assert_eq!(source, "α + integrate()");
        frame(&mut source, vec![Event::Text("x".into())]);
        assert_eq!(source, "α + integrate(x)");
    }
}
