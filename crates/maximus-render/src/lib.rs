//! Textbook math rendering for egui.

pub mod editor;
pub mod editor_ui;
pub mod layout;

use egui::{Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use layout::{Boxed, FontKind, Layout, Measure, Op};
use maximus_math::Item;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

pub use layout::{Mark, Path};

const ROMAN: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
const ITALIC: &[u8] = include_bytes!("../assets/DejaVuSans-Oblique.ttf");

pub fn roman_family() -> FontFamily {
    FontFamily::Name("math".into())
}

pub fn italic_family() -> FontFamily {
    FontFamily::Name("math-italic".into())
}

/// Registers the math fonts. Call once at startup.
pub fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("dejavu".into(), Arc::new(FontData::from_static(ROMAN)));
    fonts.font_data.insert("dejavu-italic".into(), Arc::new(FontData::from_static(ITALIC)));
    fonts.families.insert(roman_family(), vec!["dejavu".into()]);
    fonts.families.insert(italic_family(), vec!["dejavu-italic".into(), "dejavu".into()]);
    // DejaVu also serves as a fallback for symbols missing in the default fonts.
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.get_mut(&fam).unwrap().push("dejavu".into());
    }
    ctx.set_fonts(fonts);
}

fn font_id(size: f32, kind: FontKind) -> FontId {
    FontId::new(
        size,
        match kind {
            FontKind::Roman => roman_family(),
            FontKind::Italic => italic_family(),
        },
    )
}

/// Measures text with egui's font system.
pub struct EguiMeasure<'a> {
    ctx: &'a Context,
    cache: RefCell<HashMap<(String, u32, bool), f32>>,
}

impl<'a> EguiMeasure<'a> {
    pub fn new(ctx: &'a Context) -> Self {
        EguiMeasure { ctx, cache: RefCell::new(HashMap::new()) }
    }
}

impl Measure for EguiMeasure<'_> {
    fn width(&self, text: &str, size: f32, font: FontKind) -> f32 {
        let key = (text.to_string(), size.to_bits(), font == FontKind::Italic);
        if let Some(w) = self.cache.borrow().get(&key) {
            return *w;
        }
        let fid = font_id(size, font);
        let w = self.ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), fid, Color32::WHITE).size().x);
        self.cache.borrow_mut().insert(key, w);
        w
    }
}

/// Paints a laid-out box with its baseline origin at `origin`.
pub fn paint(painter: &Painter, origin: Pos2, b: &Boxed, color: Color32) {
    for op in &b.ops {
        match op {
            Op::Text { x, y, text, size, font } => {
                let galley = painter.layout_no_wrap(text.clone(), font_id(*size, *font), color);
                let base = galley
                    .rows
                    .first()
                    .map(|r| r.pos.y + r.row.glyphs.first().map_or(0.8 * size, |g| g.pos.y))
                    .unwrap_or(0.8 * size);
                painter.galley(Pos2::new(origin.x + x, origin.y + y - base), galley, color);
            }
            Op::Line { x0, y0, x1, y1, width } => {
                painter.line_segment(
                    [Pos2::new(origin.x + x0, origin.y + y0), Pos2::new(origin.x + x1, origin.y + y1)],
                    Stroke::new(*width, color),
                );
            }
            Op::Poly { pts, width } => {
                let pts: Vec<Pos2> = pts.iter().map(|(x, y)| Pos2::new(origin.x + x, origin.y + y)).collect();
                painter.add(Shape::line(pts, Stroke::new(*width, color)));
            }
            Op::Placeholder { x, y, w, h } => {
                let r = Rect::from_min_size(Pos2::new(origin.x + x, origin.y + y), Vec2::new(*w, *h)).shrink(1.0);
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, color.gamma_multiply(0.45)), egui::StrokeKind::Inside);
            }
        }
    }
}

/// Lays out a tree at the given font size.
pub fn layout_tree(ctx: &Context, row: &[Item], size: f32, editing: bool) -> Boxed {
    let m = EguiMeasure::new(ctx);
    Layout::new(&m, size, editing).layout(row)
}

/// Draws a read-only math expression.
pub fn show_math(ui: &mut egui::Ui, row: &[Item], size: f32, color: Color32) -> egui::Response {
    let b = layout_tree(ui.ctx(), row, size, false);
    let pad = 2.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(b.w + 2.0 * pad, b.height() + 2.0 * pad), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), Pos2::new(rect.min.x + pad, rect.min.y + pad + b.asc), &b, color);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use maximus_math::maxima_to_tree;

    fn ctx() -> Context {
        let ctx = Context::default();
        install_fonts(&ctx);
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        ctx
    }

    #[test]
    fn layout_has_sane_metrics() {
        let ctx = ctx();
        let tree = maxima_to_tree("integrate(x*sin(x),x,0,%pi)").unwrap();
        let b = layout_tree(&ctx, &tree, 20.0, false);
        assert!(b.w > 40.0 && b.asc > 10.0 && b.desc > 5.0, "{} {} {}", b.w, b.asc, b.desc);
        let f = layout_tree(&ctx, &maxima_to_tree("1/2").unwrap(), 20.0, false);
        assert!(f.height() > 25.0, "fraction height {}", f.height());
        let sb = layout_tree(&ctx, &maxima_to_tree("x^2").unwrap(), 20.0, false);
        let xb = layout_tree(&ctx, &maxima_to_tree("x").unwrap(), 20.0, false);
        assert!(sb.asc > xb.asc * 1.05 && sb.w > xb.w);
    }

    #[test]
    fn marks_cover_every_position() {
        let ctx = ctx();
        let row = maximus_math::from_latex("\\frac{a}{b}+2x^2");
        let b = layout_tree(&ctx, &row, 20.0, true);
        let top: Vec<_> = b.marks.iter().filter(|m| m.path.is_empty()).map(|m| m.pos).collect();
        assert!(top.contains(&0) && top.contains(&row.len()), "{:?}", top);
        assert!(b.marks.iter().any(|m| m.path == vec![(0, 0)]), "numerator marks");
        assert!(b.marks.iter().any(|m| m.path == vec![(0, 1)]), "denominator marks");
    }
}
