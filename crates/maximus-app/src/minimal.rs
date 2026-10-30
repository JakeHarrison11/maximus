//! Minimal (calculator) mode: one input line, a history of results and a keypad.
//! The full CAS stays available; it uses the same notebook and session.

use crate::app::*;
use crate::icons::{self, Icon};
use eframe::egui;
use egui::{Align, Color32, Layout, RichText, Vec2};
use maximus_core::*;
use maximus_math::{from_latex, Item, ParseCtx};
use maximus_render::editor::{EditorState, Template};
use maximus_render::editor_ui::{math_editor, EditorStyle};
use std::sync::Arc;

const INPUT_ID: &str = "minimal-input";
const KEY_HEIGHT: f32 = 48.0;

fn keypad_menu(response: &egui::Response, content: impl FnOnce(&mut egui::Ui)) {
    if let Some(menu) = egui::Popup::menu(response).show(content) {
        let ctx = &response.ctx;
        let rect = ctx.content_rect();
        egui::Area::new(egui::Id::unique("calculator-menu-backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .constrain(false)
            .show(ctx, |ui| {
                ui.allocate_rect(rect, egui::Sense::click_and_drag());
            });
        ctx.move_to_top(menu.response.layer_id);
    }
}

fn key_width(ui: &egui::Ui, columns: usize) -> f32 {
    ((ui.available_width() - (columns - 1) as f32 * 6.0) / columns as f32).floor()
}

fn calculator_button(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    let (symbol, description) = label.split_once("  ").unwrap_or((label, ""));
    let latex = match symbol {
        "x" | "y" | "z" | "π" => Some(symbol),
        "ⅇ" => Some("\\mathrm{e}"),
        "ⅈ" => Some("\\mathrm{i}"),
        "xⁿ" => Some("x^{n}"),
        "x²" => Some("x^{2}"),
        "xₙ" => Some("x_{n}"),
        "|x|" => Some("|x|"),
        "ⁿ√" => Some("\\sqrt[n]{x}"),
        "d/dx" => Some("\\frac{d}{dx}"),
        "∫ₐᵇ" => Some("\\int_a^b"),
        "×10ˣ" => Some("\\times 10^{x}"),
        _ => None,
    };
    if let Some(latex) = latex {
        let response = ui.add_sized(Vec2::new(width, KEY_HEIGHT), egui::Button::new(""));
        if ui.is_rect_visible(response.rect) {
            let math = maximus_render::layout_tree(ui.ctx(), &from_latex(latex), 20.0, false);
            let color = ui.style().interact(&response).fg_stroke.color;
            let text = ui.painter().layout_no_wrap(description.into(), egui::FontId::proportional(16.0), color);
            let gap = if description.is_empty() { 0.0 } else { 8.0 };
            let x = response.rect.center().x - (math.w + gap + text.size().x) / 2.0;
            maximus_render::paint(ui.painter(), egui::pos2(x, response.rect.center().y + (math.asc - math.desc) / 2.0), &math, color);
            if !description.is_empty() {
                ui.painter().galley(egui::pos2(x + math.w + gap, response.rect.center().y - text.size().y / 2.0), text, color);
            }
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
        return response;
    }
    ui.add_sized(Vec2::new(width, KEY_HEIGHT), egui::Button::new(RichText::new(label).size(18.0)))
}

fn insert_scientific_notation(editor: &mut EditorState) {
    editor.insert_symbol('×');
    editor.insert_symbol('1');
    editor.insert_symbol('0');
    editor.insert_template(Template::Sup);
}

fn insert_square(editor: &mut EditorState, ctx: &egui::Context, size: f32) {
    if editor.has_selection() {
        editor.insert_template(Template::Paren);
    }
    editor.insert_template(Template::Sup);
    editor.insert_symbol('2');
    navigate(editor, ctx, size, egui::Key::ArrowRight);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Key};
    use maximus_math::tree_to_maxima;

    fn editor_context() -> Context {
        let ctx = Context::default();
        maximus_render::install_fonts(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        ctx
    }

    #[test]
    fn function_keys_place_cursor_inside_argument_and_wrap_selection() {
        for name in ["sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "log", "exp"] {
            let mut editor = EditorState::default();
            insert_function(&mut editor, name);
            editor.insert_symbol('2');
            assert_eq!(tree_to_maxima(&editor.row, &ParseCtx::default()).unwrap(), format!("{name}(2);"));
            assert_eq!(editor.cur.path, vec![(1, 0)]);
        }
        let mut editor = EditorState::from_latex("x+1");
        editor.select_all();
        insert_function(&mut editor, "sin");
        assert_eq!(tree_to_maxima(&editor.row, &ParseCtx::default()).unwrap(), "sin(x+1);");
    }

    #[test]
    fn navigation_enters_and_leaves_templates_and_collapses_selection() {
        let ctx = editor_context();
        let mut editor = EditorState::default();
        editor.insert_template(Template::Frac);
        editor.insert_symbol('1');
        navigate(&mut editor, &ctx, 22.0, Key::ArrowDown);
        editor.insert_symbol('2');
        assert_eq!(editor.latex(), "\\frac{1}{2}");
        navigate(&mut editor, &ctx, 22.0, Key::ArrowUp);
        assert_eq!(editor.cur.path, vec![(0, 0)]);
        navigate(&mut editor, &ctx, 22.0, Key::End);
        editor.insert_symbol('+');
        editor.insert_symbol('3');
        editor.select_all();
        navigate(&mut editor, &ctx, 22.0, Key::ArrowLeft);
        assert!(!editor.has_selection());
        assert!(editor.cur.path.is_empty());
        assert_eq!(editor.cur.pos, 0);
        navigate(&mut editor, &ctx, 22.0, Key::End);
        editor.insert_template(Template::Sup);
        editor.insert_symbol('2');
        navigate(&mut editor, &ctx, 22.0, Key::ArrowRight);
        editor.insert_symbol('+');
        editor.insert_symbol('4');
        assert_eq!(tree_to_maxima(&editor.row, &ParseCtx::default()).unwrap(), "1/2+3^2+4;");
        navigate(&mut editor, &ctx, 22.0, Key::Home);
        assert_eq!(editor.cur.pos, 0);
    }

    #[test]
    fn decimal_conversion_uses_input_before_last_result_and_reports_invalid_input() {
        let ctx = ParseCtx::default();
        assert_eq!(decimal_source(&from_latex("\\frac{1}{3}"), Some("%o7"), &ctx).unwrap(), "float(1/3);");
        assert_eq!(decimal_source(&[], Some("%o7"), &ctx).unwrap(), "float(%o7);");
        assert!(decimal_source(&[], None, &ctx).is_err());
        assert!(decimal_source(&from_latex("1+"), Some("%o7"), &ctx).is_err());
        assert!(decimal_source(&from_latex("1;2"), None, &ctx).is_err());
    }

    #[test]
    fn division_template_uses_previous_operand_as_numerator() {
        let mut editor = EditorState::from_latex("1+6");
        editor.insert_template(Template::Frac);
        assert_eq!(editor.latex(), "1+\\frac{6}{}");
        editor.insert_symbol('3');
        assert_eq!(tree_to_maxima(&editor.row, &ParseCtx::default()).unwrap(), "1+6/3;");
        assert_eq!(decimal_source(&editor.row, None, &ParseCtx::default()).unwrap(), "float(1+6/3);");
    }

    #[test]
    fn scientific_notation_places_cursor_in_exponent() {
        let ctx = editor_context();
        for exponent in ["6", "-3"] {
            let mut editor = EditorState::from_latex("2.5");
            insert_scientific_notation(&mut editor);
            assert!(!editor.cur.path.is_empty());
            editor.type_text(exponent);
            navigate(&mut editor, &ctx, 22.0, Key::ArrowRight);
            editor.type_text("+1");
            let source = tree_to_maxima(&editor.row, &ParseCtx::default()).unwrap();
            let expected = if exponent == "6" { "2.5*10^6+1;" } else { "2.5*10^(-3)+1;" };
            assert_eq!(source, expected);
        }
    }

    #[test]
    fn square_key_squares_operand_or_selection_and_leaves_exponent() {
        let ctx = editor_context();
        let mut editor = EditorState::from_latex("3");
        insert_square(&mut editor, &ctx, 22.0);
        editor.type_text("+1");
        assert_eq!(tree_to_maxima(&editor.row, &ParseCtx::default()).unwrap(), "3^2+1;");
        let mut editor = EditorState::from_latex("x+1");
        editor.select_all();
        insert_square(&mut editor, &ctx, 22.0);
        editor.type_text("+2");
        assert_eq!(tree_to_maxima(&editor.row, &ParseCtx::default()).unwrap(), "(x+1)^2+2;");
    }

    #[test]
    fn last_result_ignores_pending_cells_and_non_result_outputs() {
        let mut notebook = Notebook::empty();
        assert_eq!(latest_result(&notebook), None);
        let id = notebook.add_cell(CellKind::Code, InputMode::Math);
        notebook.cell_mut(id).unwrap().outputs = vec![
            OutputItem::Result { label: "%o1".into(), text: "1/2".into(), tex: String::new() },
            OutputItem::Result { label: "%o2".into(), text: "1/3".into(), tex: String::new() },
            OutputItem::Text("printed".into()),
        ];
        let id = notebook.add_cell(CellKind::Code, InputMode::Math);
        notebook.cell_mut(id).unwrap().outputs.push(OutputItem::Error("invalid input".into()));
        assert_eq!(latest_result(&notebook), Some("%o2"));
    }

    #[test]
    fn keypad_menus_consume_outside_clicks_without_blocking_menu_items() {
        for label in ["Trig", "Math", "Calc", "Vars"] {
            let ctx = editor_context();
            let mut trigger = egui::Rect::NOTHING;
            let mut outside = egui::Rect::NOTHING;
            let mut item = egui::Rect::NOTHING;
            let mut outside_clicks = 0;
            let mut selections = 0;
            let mut popup_id = egui::Id::NULL;
            let mut frame = |events| {
                let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                    let response = ui.button(label);
                    trigger = response.rect;
                    popup_id = egui::Popup::default_response_id(&response);
                    keypad_menu(&response, |ui| {
                        let response = ui.button("Insert");
                        item = response.rect;
                        if response.clicked() {
                            selections += 1;
                            ui.close();
                        }
                    });
                    let response = ui.put(
                        egui::Rect::from_min_size(egui::pos2(350.0, 350.0), Vec2::splat(48.0)),
                        egui::Button::new("7"),
                    );
                    outside = response.rect;
                    if response.clicked() {
                        outside_clicks += 1;
                    }
                });
                output.textures_delta.clear();
                (trigger.center(), outside.center(), item.center(), popup_id, outside_clicks, selections)
            };
            let pointer = |pos, pressed| vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
            ];
            let (trigger, outside, ..) = frame(vec![]);
            frame(pointer(trigger, true));
            frame(pointer(trigger, false));
            frame(vec![]);
            frame(pointer(outside, true));
            let (_, _, _, id, clicks, selections) = frame(pointer(outside, false));
            assert!(!egui::Popup::is_id_open(&ctx, id), "{label} did not dismiss");
            assert_eq!((clicks, selections), (0, 0), "{label} passed the dismissal click through");
            frame(vec![]);
            frame(pointer(outside, true));
            let (_, _, _, _, clicks, _) = frame(pointer(outside, false));
            assert_eq!(clicks, 1, "key stays blocked after {label} closes");
            frame(pointer(trigger, true));
            frame(pointer(trigger, false));
            let (_, _, item, ..) = frame(vec![]);
            frame(pointer(item, true));
            let (_, _, _, id, _, selections) = frame(pointer(item, false));
            assert_eq!(selections, 1, "{label} item was blocked");
            assert!(!egui::Popup::is_id_open(&ctx, id));
        }
    }

    #[test]
    fn keypad_targets_fit_the_minimum_window_and_are_touch_sized() {
        for zoom in [1.0, 2.0] {
            let ctx = editor_context();
            ctx.set_zoom_factor(zoom);
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(400.0, 500.0) * zoom));
            let mut output = ctx.run_ui(input, |ui| {
                ui.set_max_width(crate::settings::MIN_MINIMAL_SIZE[0] - 32.0);
                ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                for labels in [
                    &["Trig ▾", "Math ▾", "Calc ▾", "Vars ▾"][..],
                    &["C", "Ans", "( )", "≈", "="][..],
                    &["←", "→", "×10ˣ", "π", "x²", "!"][..],
                ] {
                    let width = key_width(ui, labels.len());
                    let right = ui.max_rect().right();
                    ui.horizontal(|ui| {
                        for label in labels {
                            let rect = calculator_button(ui, label, width).rect;
                            assert!(rect.width() >= 44.0 && rect.height() >= 44.0, "{label}: {rect:?}");
                            assert!(rect.right() <= right + 0.01, "{label} overflows the keypad at {zoom}x: {rect:?}, right edge {right}");
                        }
                    });
                }
            });
            output.textures_delta.clear();
        }
    }
}

fn navigate(editor: &mut EditorState, ctx: &egui::Context, size: f32, key: egui::Key) {
    let b = maximus_render::layout_tree(ctx, &editor.row, size, true);
    match key {
        egui::Key::ArrowLeft | egui::Key::ArrowRight => {
            let right = key == egui::Key::ArrowRight;
            if !editor.collapse_selection(right) {
                editor.move_horizontal(&b.marks, if right { 1 } else { -1 });
            }
        }
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            editor.clear_selection();
            editor.move_vertical(&b.marks, if key == egui::Key::ArrowDown { 1 } else { -1 });
        }
        egui::Key::Home | egui::Key::End => {
            editor.clear_selection();
            editor.move_home_end(&b.marks, key == egui::Key::End);
        }
        _ => unreachable!("not a calculator navigation key"),
    }
}

fn insert_function(editor: &mut EditorState, name: &str) {
    let argument = editor.selected_row().unwrap_or_default();
    editor.insert_row(vec![Item::Func(name.into())]);
    editor.insert_template(Template::Paren);
    editor.insert_row(argument);
}

fn latest_result(notebook: &Notebook) -> Option<&str> {
    notebook.cells.iter().rev().flat_map(|cell| cell.outputs.iter().rev()).find_map(|out| match out {
        OutputItem::Result { label, .. } => Some(label.as_str()),
        _ => None,
    })
}

fn decimal_source(row: &[Item], last_result: Option<&str>, ctx: &ParseCtx) -> Result<String, String> {
    let expression = if row.is_empty() {
        last_result.ok_or("Enter an expression or evaluate a result first")?.to_owned()
    } else {
        let statements = maximus_math::parse_tree(row, ctx)?;
        if statements.len() != 1 {
            return Err("Decimal conversion needs a single expression".into());
        }
        maximus_math::to_maxima_with(&statements[0].expr, maximus_math::PrintOpts { eval_sums: true })
    };
    Ok(format!("float({expression});"))
}

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

    fn key(&mut self, ui: &mut egui::Ui, label: &str, ins: Insert, w: f32) -> bool {
        let clicked = calculator_button(ui, label, w).clicked();
        if clicked {
            self.apply_insert_minimal(ins);
            ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
        }
        clicked
    }

    fn key_menu(&mut self, ui: &mut egui::Ui, label: &str, keys: &[(&str, Insert)], w: f32) {
        let response = calculator_button(ui, label, w);
        keypad_menu(&response, |ui| {
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for (label, insert) in keys {
                    if self.key(ui, label, insert.clone(), 196.0) {
                        ui.close();
                    }
                }
            });
        });
    }

    fn decimal_minimal(&mut self) {
        match decimal_source(&self.min_editor.row, latest_result(&self.session.notebook), &self.session.parse_ctx()) {
            Ok(source) => {
                let id = self.session.notebook.add_cell(CellKind::Code, InputMode::Maxima);
                self.session.notebook.cell_mut(id).unwrap().source = source;
                self.session.evaluate_cell(id);
                self.min_editor.clear();
                self.min_scroll_to_end = true;
                self.min_scroll_frames = 6;
            }
            Err(error) => self.messages.push(format!("Cannot convert to decimal: {error}")),
        }
    }

    fn keypad(&mut self, ui: &mut egui::Ui, size: f32) {
        ui.spacing_mut().item_spacing = Vec2::splat(6.0);
        let menu_w = key_width(ui, 4);
        ui.horizontal(|ui| {
            let trig = calculator_button(ui, "Trig ▾", menu_w).on_hover_text("Trigonometry (radians)");
            keypad_menu(&trig, |ui| {
                ui.weak("Angles in radians");
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for name in ["sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh"] {
                        if calculator_button(ui, name, 196.0).clicked() {
                            insert_function(&mut self.min_editor, name);
                            ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
                            ui.close();
                        }
                    }
                });
            });
            let powers = calculator_button(ui, "Math ▾", menu_w);
            keypad_menu(&powers, |ui| {
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    if calculator_button(ui, "x²  Square", 196.0).clicked() {
                        insert_square(&mut self.min_editor, ui.ctx(), size);
                        ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
                        ui.close();
                    }
                    for (label, name) in [("ln  Natural logarithm", "log"), ("exp  Exponential", "exp")] {
                        if calculator_button(ui, label, 196.0).clicked() {
                            insert_function(&mut self.min_editor, name);
                            ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
                            ui.close();
                        }
                    }
                    for (label, insert) in [
                        ("ⁿ√  Nth root", Insert::Template(Template::NthRoot)),
                        ("|x|  Absolute value", Insert::Template(Template::Abs)),
                        ("xₙ  Subscript", Insert::Template(Template::Sub)),
                        ("!  Factorial", Insert::Symbol('!')),
                    ] {
                        if self.key(ui, label, insert, 196.0) {
                            ui.close();
                        }
                    }
                });
            });
            self.key_menu(ui, "Calc ▾", &[
                ("∫  Integral", Insert::Template(Template::Integral)),
                ("∫ₐᵇ  Definite integral", Insert::Template(Template::DefIntegral)),
                ("d/dx  Derivative", Insert::Template(Template::Derivative)),
                ("Σ  Sum", Insert::Template(Template::Sum)),
                ("Π  Product", Insert::Template(Template::Product)),
                ("lim  Limit", Insert::Template(Template::Limit)),
            ], menu_w);
            self.key_menu(ui, "Vars ▾", &[
                ("x", Insert::Symbol('x')), ("y", Insert::Symbol('y')), ("z", Insert::Symbol('z')),
                ("π  Pi", Insert::Symbol('π')), ("ⅇ  Euler's number", Insert::Symbol('ⅇ')),
                ("ⅈ  Imaginary unit", Insert::Symbol('ⅈ')), ("∞  Infinity", Insert::Symbol('∞')),
                ("=  Equation", Insert::Symbol('=')), (":  Assignment", Insert::Symbol(':')),
            ], menu_w);
        });
        let nav_w = key_width(ui, 6);
        ui.horizontal(|ui| {
            for (label, key, tip) in [
                ("←", egui::Key::ArrowLeft, "Move left"),
                ("→", egui::Key::ArrowRight, "Move right / leave a template"),
            ] {
                if calculator_button(ui, label, nav_w).on_hover_text(tip).clicked() {
                    navigate(&mut self.min_editor, ui.ctx(), size, key);
                    ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
                }
            }
            if calculator_button(ui, "×10ˣ", nav_w).on_hover_text("Multiply by a power of ten; enter the exponent next").clicked() {
                insert_scientific_notation(&mut self.min_editor);
                ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
            }
            self.key(ui, "π", Insert::Symbol('π'), nav_w);
            if calculator_button(ui, "x²", nav_w).on_hover_text("Square the preceding operand or selection").clicked() {
                insert_square(&mut self.min_editor, ui.ctx(), size);
                ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
            }
            self.key(ui, "!", Insert::Symbol('!'), nav_w);
        });
        let w = key_width(ui, 5);
        egui::Grid::new("calculator-keys").spacing([6.0, 6.0]).show(ui, |ui| {
            if calculator_button(ui, "C", w).on_hover_text("Clear input (keep history)").clicked() {
                self.min_editor.clear();
                ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
            }
            if icons::icon_button(ui, Icon::Backspace, Vec2::new(w, KEY_HEIGHT), true).on_hover_text("Backspace").clicked() {
                self.min_editor.backspace();
                ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
            }
            self.key(ui, "( )", Insert::Template(Template::Paren), w);
            ui.add_enabled_ui(latest_result(&self.session.notebook).is_some(), |ui| {
                self.key(ui, "Ans", Insert::Symbol('%'), w);
            });
            self.key(ui, "√", Insert::Template(Template::Sqrt), w);
            ui.end_row();
            for (digits, operator, extra) in [
                (["7", "8", "9"], '÷', Some(("xⁿ", Insert::Template(Template::Sup)))),
                (["4", "5", "6"], '×', Some(("x", Insert::Symbol('x')))),
                (["1", "2", "3"], '−', None),
            ] {
                for digit in digits {
                    self.key(ui, digit, Insert::Symbol(digit.chars().next().unwrap()), w);
                }
                let insert = if operator == '÷' { Insert::Template(Template::Frac) } else { Insert::Symbol(operator) };
                self.key(ui, &operator.to_string(), insert, w);
                if let Some((label, insert)) = extra {
                    self.key(ui, label, insert, w);
                } else {
                    let can_decimal = self.session.ready && (!self.min_editor.is_empty() || latest_result(&self.session.notebook).is_some());
                    ui.add_enabled_ui(can_decimal, |ui| {
                        if calculator_button(ui, "≈", w).on_hover_text("Decimal approximation of the input, or of the latest result when empty").clicked() {
                            self.decimal_minimal();
                            ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
                        }
                    });
                }
                ui.end_row();
            }
            for symbol in ['0', '.', ',', '+'] {
                self.key(ui, &symbol.to_string(), Insert::Symbol(symbol), w);
            }
            ui.add_enabled_ui(self.session.ready && !self.min_editor.is_empty(), |ui| {
                let enter = egui::Button::new(RichText::new("=").size(23.0).color(Color32::WHITE)).fill(Color32::from_rgb(0x2e, 0x7d, 0x32));
                if ui.add_sized(Vec2::new(w, KEY_HEIGHT), enter).on_hover_text("Evaluate (Enter)").clicked() {
                    self.submit_minimal();
                    ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
                }
            });
            ui.end_row();
        });
    }

    pub fn minimal_ui(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(ui.available_width());
        let size = self.settings.math_size + 2.0;
        let color = ui.visuals().text_color();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Maximus").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::icon_button(ui, Icon::Notebook, Vec2::new(28.0, 24.0), true).on_hover_text("Notebook mode (Ctrl+M)").clicked() {
                    self.settings.minimal = false;
                    self.settings.save();
                }
                let more = icons::icon_button(ui, Icon::Dots, Vec2::new(28.0, 24.0), true).on_hover_text("Calculator options");
                egui::Popup::menu(&more).show(|ui| {
                    ui.checkbox(&mut self.show_keypad, "Show keypad");
                    if ui.add_enabled(!self.session.is_busy(), egui::Button::new("Clear history")).on_hover_text("Remove notebook cells; Maxima variables are kept").clicked() {
                        self.session.notebook = Notebook::empty();
                        self.cell_ui.clear();
                        self.out_views.clear();
                        ui.close();
                    }
                    ui.separator();
                    self.status_bar(ui);
                });
            });
        });
        ui.separator();

        let input_height = (maximus_render::layout_tree(ui.ctx(), &self.min_editor.row, size, true).height() + 12.0).clamp(56.0, 120.0);
        let keypad_height = (ui.available_height() - input_height - 120.0).clamp(144.0, 380.0);
        let panel_height = input_height + 16.0 + if self.show_keypad { keypad_height + 10.0 } else { 0.0 };
        egui::Panel::bottom("min-input").resizable(false).show_separator_line(false).exact_size(panel_height).show(ui, |ui| {
            ui.style_mut().spacing.scroll.floating = false;
            let eid = egui::Id::unique(INPUT_ID);
            if !ui.memory(|m| m.focused().is_some()) {
                ui.memory_mut(|m| m.request_focus(eid));
            }
            let mut style = EditorStyle::new(size, color);
            style.min_height = 56.0;
            style.keep_focus = true;
            let out = egui::ScrollArea::both().id_salt("calculator-input-scroll").max_height(input_height).auto_shrink([false, true]).show(ui, |ui| {
                math_editor(ui, eid, &mut self.min_editor, &style)
            }).inner;
            if out.submit {
                self.submit_minimal();
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
            egui::ScrollArea::vertical().id_salt("calculator-keypad-scroll").max_height(keypad_height).auto_shrink([false, true]).show(ui, |ui| {
                self.keypad(ui, size);
            });
        });

        let mut recall: Option<String> = None;
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
                        OutputItem::Result { text, .. } => {
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
            ui.memory_mut(|m| m.request_focus(egui::Id::unique(INPUT_ID)));
        }
    }
}
