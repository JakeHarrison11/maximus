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
    /// A copy with a different hint text.
    pub fn clone_with_hint(&self, hint: &str) -> EditorStyle {
        EditorStyle { size: self.size, color: self.color, hint: hint.to_string(), min_height: self.min_height }
    }

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

    let shift_held = ui.input(|i| i.modifiers.shift);
    if resp.drag_started() {
        ctx.memory_mut(|m| m.request_focus(rect_id));
        let o = origin(&b);
        if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
            if shift_held {
                state.begin_extend();
            }
            state.click(&b.marks, p.x - o.x, p.y - o.y);
            if !shift_held {
                state.anchor = Some(state.cur.clone());
            }
        }
    } else if resp.clicked() || resp.secondary_clicked() {
        ctx.memory_mut(|m| m.request_focus(rect_id));
        secondary = resp.secondary_clicked();
        // A right click inside an existing selection keeps it (so it can be copied).
        if !(secondary && state.has_selection()) {
            if let Some(p) = resp.interact_pointer_pos() {
                let o = origin(&b);
                if shift_held {
                    state.begin_extend();
                } else {
                    state.clear_selection();
                }
                state.click(&b.marks, p.x - o.x, p.y - o.y);
                state.end_extend();
            }
        }
    }
    if resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            let o = origin(&b);
            state.click(&b.marks, p.x - o.x, p.y - o.y);
        }
    }
    if resp.drag_stopped() {
        state.end_extend();
    }
    if resp.triple_clicked() {
        state.select_all();
    } else if resp.double_clicked() {
        state.select_word();
    }

    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::Text);
    }
    let focused = ctx.memory(|m| m.has_focus(rect_id));
    if focused {
        let in_matrix = state.in_matrix();
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                rect_id,
                EventFilter { tab: in_matrix, horizontal_arrows: true, vertical_arrows: true, escape: false },
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
                    let text = state.selection_latex().unwrap_or_else(|| state.latex());
                    ctx.copy_text(text);
                    mutated = false;
                }
                Event::Cut => {
                    match state.selection_latex() {
                        Some(t) => {
                            ctx.copy_text(t);
                            state.delete_selection();
                        }
                        None => {
                            ctx.copy_text(state.latex());
                            state.clear();
                        }
                    }
                }
                Event::Key { key, pressed: true, modifiers, .. } => match key {
                    Key::Backspace => {
                        if state.row.is_empty() {
                            backspace_on_empty = true;
                        }
                        state.backspace()
                    }
                    Key::Delete => state.delete_forward(),
                    Key::Tab if in_matrix => state.matrix_tab(modifiers.shift),
                    Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End => {
                        mutated = false;
                        let marks = &b.marks;
                        let go = |s: &mut EditorState| match key {
                            Key::ArrowLeft => s.move_horizontal(marks, -1),
                            Key::ArrowRight => s.move_horizontal(marks, 1),
                            Key::ArrowUp => s.move_vertical(marks, -1),
                            Key::ArrowDown => s.move_vertical(marks, 1),
                            Key::Home => s.move_home_end(marks, false),
                            _ => s.move_home_end(marks, true),
                        };
                        if modifiers.shift {
                            state.begin_extend();
                            go(state);
                            state.end_extend();
                        } else if matches!(key, Key::ArrowLeft | Key::ArrowRight) && state.has_selection() {
                            state.collapse_selection(key == Key::ArrowRight);
                        } else {
                            state.clear_selection();
                            go(state);
                        }
                    }
                    Key::A if modifiers.command => {
                        state.select_all();
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
                            state.anchor = None;
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
    {
        let x_at = |path: &[(usize, usize)], pos: usize| b.marks.iter().find(|m| m.path == path && m.pos == pos).map(|m| m.x);
        let y_of = |path: &[(usize, usize)]| {
            let (mut y0, mut y1) = (f32::MAX, f32::MIN);
            for r in b.rows.iter().filter(|r| r.path == path) {
                y0 = y0.min(r.y0);
                y1 = y1.max(r.y1);
            }
            if y0 > y1 {
                (y0, y1) = (-style.size * 0.8, style.size * 0.25);
            }
            (y0, y1)
        };
        let mut rects: Vec<(f32, f32, f32, f32)> = Vec::new();
        for part in state.selection_parts() {
            match part {
                crate::editor::Part::Items(sel) => {
                    if let (Some(x0), Some(x1)) = (x_at(&sel.path, sel.start), x_at(&sel.path, sel.end)) {
                        let (y0, y1) = y_of(&sel.path);
                        rects.push((x0.min(x1), x0.max(x1), y0, y1));
                    }
                }
                crate::editor::Part::Open { parent, idx } => {
                    let mut inner = parent.clone();
                    inner.push((idx, 0));
                    if let (Some(x0), Some(x1)) = (x_at(&parent, idx), x_at(&inner, 0)) {
                        let (y0, y1) = y_of(&inner);
                        rects.push((x0.min(x1), x0.max(x1), y0, y1));
                    }
                }
                crate::editor::Part::Close { parent, idx } => {
                    let mut inner = parent.clone();
                    inner.push((idx, 0));
                    let end = b.marks.iter().filter(|m| m.path == inner).map(|m| m.pos).max().unwrap_or(0);
                    if let (Some(x0), Some(x1)) = (x_at(&inner, end), x_at(&parent, idx + 1)) {
                        let (y0, y1) = y_of(&inner);
                        rects.push((x0.min(x1), x0.max(x1), y0, y1));
                    }
                }
            }
        }
        let painter = ui.painter().with_clip_rect(rect);
        for (x0, x1, y0, y1) in rects {
            let hl = Rect::from_min_max(Pos2::new(o.x + x0, o.y + y0 - 1.0), Pos2::new(o.x + x1, o.y + y1 + 1.0));
            painter.rect_filled(hl, 2.0, visuals.selection.bg_fill);
        }
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

    fn key(k: Key, mods: Modifiers) -> Event {
        Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: mods }
    }

    fn focused_editor(text: &str) -> (Context, EditorState, Id) {
        let ctx = Context::default();
        crate::install_fonts(&ctx);
        let id = Id::unique("sel");
        let mut st = EditorState::default();
        frame(&ctx, &mut st, id, vec![]);
        ctx.memory_mut(|m| m.request_focus(id));
        frame(&ctx, &mut st, id, vec![]);
        frame(&ctx, &mut st, id, vec![Event::Text(text.into())]);
        (ctx, st, id)
    }

    #[test]
    fn shift_arrows_select_and_typing_replaces() {
        let (ctx, mut st, id) = focused_editor("abcd");
        let shift = Modifiers::SHIFT;
        frame(&ctx, &mut st, id, vec![key(Key::ArrowLeft, shift), key(Key::ArrowLeft, shift)]);
        assert_eq!(st.selected_row().map(|r| maximus_math::row_to_latex(&r)).as_deref(), Some("cd"));
        frame(&ctx, &mut st, id, vec![key(Key::ArrowRight, shift)]);
        assert_eq!(st.selected_row().map(|r| maximus_math::row_to_latex(&r)).as_deref(), Some("d"));
        frame(&ctx, &mut st, id, vec![Event::Text("X".into())]);
        assert_eq!(st.latex(), "abcX");
        // Plain arrows collapse without moving past the edge.
        frame(&ctx, &mut st, id, vec![key(Key::ArrowLeft, shift), key(Key::ArrowLeft, Modifiers::NONE)]);
        assert!(!st.has_selection());
        assert_eq!(st.cur.pos, 3);
    }

    #[test]
    fn select_all_cut_copy_and_undo() {
        let (ctx, mut st, id) = focused_editor("1/2");
        frame(&ctx, &mut st, id, vec![key(Key::A, Modifiers::COMMAND)]);
        assert_eq!(st.selection_latex().as_deref(), Some("\\frac{1}{2}"));
        frame(&ctx, &mut st, id, vec![Event::Cut]);
        assert_eq!(st.latex(), "");
        frame(&ctx, &mut st, id, vec![key(Key::Z, Modifiers::COMMAND)]);
        assert_eq!(st.latex(), "\\frac{1}{2}");
        assert!(!st.has_selection());
    }

    #[test]
    fn dragging_with_the_mouse_selects() {
        let (ctx, mut st, id) = focused_editor("abc");
        st.cur = crate::editor::Cursor { path: vec![], pos: 3 };
        let pos = |x: f32| Pos2::new(x, 24.0);
        let mut ptr = |events: Vec<Event>| frame(&ctx, &mut st, id, events);
        ptr(vec![Event::PointerMoved(pos(10.0))]);
        ptr(vec![Event::PointerButton { pos: pos(10.0), button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
        ptr(vec![Event::PointerMoved(pos(60.0))]);
        ptr(vec![Event::PointerMoved(pos(300.0))]);
        ptr(vec![Event::PointerButton { pos: pos(300.0), button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
        ptr(vec![]);
        assert_eq!(st.selection_latex().as_deref(), Some("abc"), "anchor {:?} cur {:?}", st.anchor, st.cur);
    }
}
