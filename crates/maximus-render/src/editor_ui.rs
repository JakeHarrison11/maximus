//! egui widget for the structured math editor.

use crate::editor::EditorState;
use crate::{layout_tree, paint};
use egui::{Color32, Event, EventFilter, Id, Key, Pos2, Rect, Sense, Stroke, Vec2};

pub struct EditorOutput {
    pub response: egui::Response,
    pub changed: bool,
    pub submit: bool,
    pub focused: bool,
    /// Function name under/around the cursor (for contextual help).
    pub help_word: Option<String>,
    /// Set when the user right-clicked; `help_word` then refers to the clicked position.
    pub secondary_clicked: bool,
    /// Backspace was pressed while the editor was already empty.
    pub backspace_on_empty: bool,
}

pub struct EditorStyle {
    pub size: f32,
    pub color: Color32,
    pub hint: String,
    pub min_height: f32,
}

impl EditorStyle {
    pub fn new(size: f32, color: Color32) -> Self {
        EditorStyle { size, color, hint: String::new(), min_height: 0.0 }
    }
}

const UNDO_LIMIT: usize = 100;

pub fn math_editor(ui: &mut egui::Ui, id: Id, state: &mut EditorState, style: &EditorStyle) -> EditorOutput {
    let ctx = ui.ctx().clone();
    let pad = 6.0;
    let mut b = layout_tree(&ctx, &state.row, style.size, true);
    let width = ui.available_width().max(b.w + 2.0 * pad + 4.0);
    let height = (b.height() + 2.0 * pad).max(style.size * 1.7 + 2.0 * pad).max(style.min_height);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let rect_id = id;
    let resp = ui.interact(rect, rect_id, Sense::click_and_drag());

    let mut changed = false;
    let mut submit = false;
    let mut secondary = false;
    let mut backspace_on_empty = false;

    let origin = |b: &crate::layout::Boxed| Pos2::new(rect.min.x + pad, rect.center().y + (b.asc - b.desc) / 2.0);

    if resp.clicked() || resp.secondary_clicked() || resp.drag_started() {
        ctx.memory_mut(|m| m.request_focus(rect_id));
        if let Some(p) = resp.interact_pointer_pos() {
            let o = origin(&b);
            state.click(&b.marks, p.x - o.x, p.y - o.y);
        }
        secondary = resp.secondary_clicked();
    }

    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::Text);
    }
    let focused = ctx.memory(|m| m.has_focus(rect_id));
    if focused {
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                rect_id,
                EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: false },
            )
        });
        let events = ui.input(|i| i.events.clone());
        for ev in events {
            let before = (state.row.clone(), state.cur.clone());
            let mut mutated = true;
            match ev {
                Event::Text(t) => state.type_text(&t),
                Event::Paste(t) => state.paste(&t),
                Event::Copy => {
                    ctx.copy_text(state.latex());
                    mutated = false;
                }
                Event::Cut => {
                    ctx.copy_text(state.latex());
                    state.clear();
                }
                Event::Key { key, pressed: true, modifiers, .. } => match key {
                    Key::Backspace => {
                        if state.row.is_empty() {
                            backspace_on_empty = true;
                        }
                        state.backspace()
                    }
                    Key::Delete => state.delete_forward(),
                    Key::ArrowLeft => {
                        state.move_horizontal(&b.marks, -1);
                        mutated = false;
                    }
                    Key::ArrowRight => {
                        state.move_horizontal(&b.marks, 1);
                        mutated = false;
                    }
                    Key::ArrowUp => {
                        state.move_vertical(&b.marks, -1);
                        mutated = false;
                    }
                    Key::ArrowDown => {
                        state.move_vertical(&b.marks, 1);
                        mutated = false;
                    }
                    Key::Home => {
                        state.move_home_end(&b.marks, false);
                        mutated = false;
                    }
                    Key::End => {
                        state.move_home_end(&b.marks, true);
                        mutated = false;
                    }
                    Key::Enter => {
                        submit = true;
                        mutated = false;
                    }
                    Key::Z if modifiers.command => {
                        if let Some((r, c)) = state.undo.pop() {
                            state.row = r;
                            state.cur = c;
                            changed = true;
                        }
                        mutated = false;
                    }
                    _ => mutated = false,
                },
                _ => mutated = false,
            }
            if mutated && state.row != before.0 {
                state.undo.push(before);
                if state.undo.len() > UNDO_LIMIT {
                    state.undo.remove(0);
                }
                changed = true;
            }
            // Later events in the same frame navigate with up-to-date marks.
            b = layout_tree(&ctx, &state.row, style.size, true);
        }
    }
    let visuals = ui.visuals().clone();
    let stroke = if focused { Stroke::new(1.5, visuals.selection.stroke.color) } else { Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color) };
    ui.painter().rect(rect, 4.0, visuals.extreme_bg_color, stroke, egui::StrokeKind::Inside);
    let o = origin(&b);
    if state.row.is_empty() && !focused && !style.hint.is_empty() {
        ui.painter().text(
            Pos2::new(rect.min.x + pad, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &style.hint,
            egui::FontId::proportional(style.size * 0.8),
            style.color.gamma_multiply(0.4),
        );
    }
    paint(&ui.painter().with_clip_rect(rect), o, &b, style.color);

    if focused {
        if let Some(m) = state.cursor_mark(&b) {
            let t = ui.input(|i| i.time);
            if (t * 1.8).fract() < 0.6 {
                let x = o.x + m.x;
                let r = Rect::from_min_max(Pos2::new(x, o.y + m.y0), Pos2::new(x, o.y + m.y1));
                ui.painter().line_segment([r.min, r.max], Stroke::new(1.5, style.color));
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    let help_word = if focused || secondary { state.help_word() } else { None };
    EditorOutput { response: resp, changed, submit, focused, help_word, secondary_clicked: secondary, backspace_on_empty }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Modifiers, RawInput};

    fn frame_out(ctx: &Context, state: &mut EditorState, id: Id, events: Vec<Event>) -> bool {
        let mut flag = false;
        let mut raw = RawInput::default();
        raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));
        raw.events = events;
        let mut out = ctx.run_ui(raw, |ui| {
            flag |= math_editor(ui, id, state, &EditorStyle::new(20.0, Color32::WHITE)).backspace_on_empty;
        });
        out.textures_delta.clear();
        flag
    }

    fn frame(ctx: &Context, state: &mut EditorState, id: Id, events: Vec<Event>) -> bool {
        let mut raw = RawInput::default();
        raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));
        raw.events = events;
        let mut submit = false;
        let mut out = ctx.run_ui(raw, |ui| {
            let o = math_editor(ui, id, state, &EditorStyle::new(20.0, Color32::WHITE));
            submit |= o.submit;
        });
        out.textures_delta.clear();
        submit
    }

    #[test]
    fn typing_and_navigation_through_widget() {
        let ctx = Context::default();
        crate::install_fonts(&ctx);
        let id = Id::unique("ed");
        let mut st = EditorState::default();
        frame(&ctx, &mut st, id, vec![]);
        ctx.memory_mut(|m| m.request_focus(id));
        frame(&ctx, &mut st, id, vec![]);
        frame(&ctx, &mut st, id, vec![Event::Text("1/2".into())]);
        assert_eq!(st.latex(), "\\frac{1}{2}");
        // Move out of the fraction with the right arrow, then continue typing.
        let key = |k| Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        frame(&ctx, &mut st, id, vec![key(Key::ArrowRight)]);
        frame(&ctx, &mut st, id, vec![Event::Text("+x^2".into())]);
        assert_eq!(st.latex(), "\\frac{1}{2}+x^{2}");
        let submitted = frame(&ctx, &mut st, id, vec![key(Key::Enter)]);
        assert!(submitted);
        frame(&ctx, &mut st, id, vec![key(Key::Backspace)]);
        assert_eq!(st.latex(), "\\frac{1}{2}+x^{}");
        frame(&ctx, &mut st, id, vec![key(Key::Backspace)]);
        assert_eq!(st.latex(), "\\frac{1}{2}+x");
    }

    #[test]
    fn backspace_in_empty_editor_is_reported() {
        let ctx = Context::default();
        crate::install_fonts(&ctx);
        let id = Id::unique("ed2");
        let mut st = EditorState::default();
        frame(&ctx, &mut st, id, vec![]);
        ctx.memory_mut(|m| m.request_focus(id));
        frame(&ctx, &mut st, id, vec![]);
        let bs = Event::Key { key: Key::Backspace, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        assert!(frame_out(&ctx, &mut st, id, vec![bs.clone()]));
        frame(&ctx, &mut st, id, vec![Event::Text("x".into())]);
        assert!(!frame_out(&ctx, &mut st, id, vec![bs]), "not empty: plain deletion");
    }
}
