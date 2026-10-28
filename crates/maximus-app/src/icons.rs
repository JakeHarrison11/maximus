//! Icons drawn with shapes instead of font glyphs, so they are centred exactly
//! regardless of which fallback font would have supplied the glyph.

use eframe::egui;
use egui::{Color32, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Play,
    Stop,
    Dots,
    Close,
    Check,
    Cross,
    Backspace,
}

pub fn draw_icon(ui: &Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let p = ui.painter();
    let s = size * 0.5;
    let stroke = Stroke::new((size * 0.14).max(1.3), color);
    match icon {
        Icon::Play => {
            let pts = vec![
                Pos2::new(center.x - 0.55 * s, center.y - 0.8 * s),
                Pos2::new(center.x - 0.55 * s, center.y + 0.8 * s),
                Pos2::new(center.x + 0.85 * s, center.y),
            ];
            p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Stop => {
            let r = Rect::from_center_size(center, Vec2::splat(1.4 * s));
            p.rect_filled(r, 1.5, color);
        }
        Icon::Dots => {
            for dy in [-0.8, 0.0, 0.8] {
                p.circle_filled(Pos2::new(center.x, center.y + dy * s), (size * 0.09).max(1.2), color);
            }
        }
        Icon::Close | Icon::Cross => {
            let d = 0.6 * s;
            p.line_segment([Pos2::new(center.x - d, center.y - d), Pos2::new(center.x + d, center.y + d)], stroke);
            p.line_segment([Pos2::new(center.x - d, center.y + d), Pos2::new(center.x + d, center.y - d)], stroke);
        }
        Icon::Check => {
            let pts = vec![
                Pos2::new(center.x - 0.8 * s, center.y + 0.05 * s),
                Pos2::new(center.x - 0.25 * s, center.y + 0.6 * s),
                Pos2::new(center.x + 0.85 * s, center.y - 0.6 * s),
            ];
            p.add(Shape::line(pts, stroke));
        }
        Icon::Backspace => {
            let (w, h) = (1.9 * s, 1.3 * s);
            let pts = vec![
                Pos2::new(center.x - w / 2.0, center.y),
                Pos2::new(center.x - w / 2.0 + 0.55 * s, center.y - h / 2.0),
                Pos2::new(center.x + w / 2.0, center.y - h / 2.0),
                Pos2::new(center.x + w / 2.0, center.y + h / 2.0),
                Pos2::new(center.x - w / 2.0 + 0.55 * s, center.y + h / 2.0),
                Pos2::new(center.x - w / 2.0, center.y),
            ];
            p.add(Shape::line(pts, Stroke::new(1.2, color)));
            let d = 0.28 * s;
            let c = Pos2::new(center.x + 0.15 * s, center.y);
            p.line_segment([Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)], Stroke::new(1.2, color));
            p.line_segment([Pos2::new(c.x - d, c.y + d), Pos2::new(c.x + d, c.y - d)], Stroke::new(1.2, color));
        }
    }
}

fn paint_button_bg(ui: &Ui, rect: Rect, resp: &Response, framed: bool) -> Color32 {
    let v = ui.style().interact(resp);
    if framed || resp.hovered() || resp.has_focus() {
        ui.painter().rect(rect, v.corner_radius, v.weak_bg_fill, v.bg_stroke, StrokeKind::Inside);
    }
    v.fg_stroke.color
}

/// A square-ish button showing only an icon.
pub fn icon_button(ui: &mut Ui, icon: Icon, size: Vec2, framed: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let color = paint_button_bg(ui, rect, &resp, framed);
        draw_icon(ui, icon, rect.center(), size.min_elem() * 0.5, color);
    }
    resp
}

/// A button with an icon followed by a label.
pub fn icon_label_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let color = ui.visuals().widgets.inactive.fg_stroke.color;
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let icon_size = 10.0;
    let pad = ui.spacing().button_padding;
    let size = Vec2::new(pad.x * 2.0 + icon_size + 6.0 + galley.size().x, (galley.size().y + pad.y * 2.0).max(ui.spacing().interact_size.y));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let mut c = if enabled { paint_button_bg(ui, rect, &resp, true) } else { ui.visuals().widgets.noninteractive.fg_stroke.color.gamma_multiply(0.6) };
        if !enabled {
            ui.painter().rect(rect, 4.0, ui.visuals().widgets.inactive.weak_bg_fill.gamma_multiply(0.6), Stroke::NONE, StrokeKind::Inside);
        }
        c = if enabled { c } else { c.gamma_multiply(0.8) };
        draw_icon(ui, icon, Pos2::new(rect.min.x + pad.x + icon_size / 2.0, rect.center().y), icon_size, c);
        let pos = Pos2::new(rect.min.x + pad.x + icon_size + 6.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, c);
    }
    resp
}

/// A small coloured status dot followed by text.
pub fn status_dot(ui: &mut Ui, color: Color32, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, color);
        ui.colored_label(color, text);
    });
}
