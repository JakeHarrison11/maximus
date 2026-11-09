use maximus_core::{CellKind, Notebook, OutputItem, PlotData};
use maximus_math::{from_latex, maxima_to_tree, Item};
use maximus_render::layout::{FontKind, Layout, Measure, Op};
use std::collections::BTreeMap;

const PAGE_WIDTH: f32 = 612.0;
const PAGE_HEIGHT: f32 = 792.0;
const MARGIN: f32 = 48.0;
const FONT: &[u8] = include_bytes!("../../maximus-render/assets/DejaVuSans.ttf");

struct PdfMeasure<'a>(&'a ttf_parser::Face<'static>);

impl Measure for PdfMeasure<'_> {
    fn width(&self, text: &str, size: f32, _font: FontKind) -> f32 {
        text.chars()
            .filter_map(|ch| {
                self.0
                    .glyph_index(ch)
                    .and_then(|id| self.0.glyph_hor_advance(id))
            })
            .map(|advance| advance as f32 * size / self.0.units_per_em() as f32)
            .sum()
    }
}

struct PdfDocument {
    face: ttf_parser::Face<'static>,
    content: String,
    pages: Vec<String>,
    y: f32,
    unicode: BTreeMap<u16, char>,
}

impl PdfDocument {
    fn new() -> Result<Self, String> {
        let face = ttf_parser::Face::parse(FONT, 0)
            .map_err(|e| format!("Could not load the bundled PDF font: {e:?}"))?;
        Ok(Self {
            face,
            content: String::new(),
            pages: Vec::new(),
            y: MARGIN,
            unicode: BTreeMap::new(),
        })
    }

    fn finish_page(&mut self) {
        self.pages.push(std::mem::take(&mut self.content));
        self.y = MARGIN;
    }

    fn ensure_space(&mut self, height: f32) {
        if self.y + height > PAGE_HEIGHT - MARGIN {
            self.finish_page();
        }
    }

    fn glyphs(&mut self, text: &str) -> Result<(String, f32), String> {
        let mut encoded = String::new();
        let mut width = 0.0;
        for ch in text.chars() {
            let glyph = self.face.glyph_index(ch).ok_or_else(|| {
                format!("PDF font does not support character U+{:04X}", ch as u32)
            })?;
            encoded.push_str(&format!("{:04X}", glyph.0));
            self.unicode.entry(glyph.0).or_insert(ch);
            width += self.face.glyph_hor_advance(glyph).unwrap_or(0) as f32
                / self.face.units_per_em() as f32;
        }
        Ok((encoded, width))
    }

    fn text_line(&mut self, text: &str, size: f32, indent: f32) -> Result<(), String> {
        let (glyphs, _) = self.glyphs(text)?;
        self.ensure_space(size * 1.5);
        let x = MARGIN + indent;
        let baseline = PAGE_HEIGHT - self.y - size;
        self.content.push_str(&format!(
            "BT /F1 {size:.3} Tf 1 0 0 1 {x:.3} {baseline:.3} Tm <{glyphs}> Tj ET\n"
        ));
        self.y += size * 1.5;
        Ok(())
    }

    fn text(&mut self, text: &str, size: f32, indent: f32) -> Result<(), String> {
        let max_width = PAGE_WIDTH - MARGIN * 2.0 - indent;
        for paragraph in text.split('\n') {
            let mut line = String::new();
            for word in paragraph.split_whitespace() {
                let candidate = if line.is_empty() {
                    word.to_owned()
                } else {
                    format!("{line} {word}")
                };
                if PdfMeasure(&self.face).width(&candidate, size, FontKind::Roman) <= max_width {
                    line = candidate;
                } else {
                    if !line.is_empty() {
                        self.text_line(&line, size, indent)?;
                        line.clear();
                    }
                    let mut part = String::new();
                    for ch in word.chars() {
                        let candidate = format!("{part}{ch}");
                        if !part.is_empty()
                            && PdfMeasure(&self.face).width(&candidate, size, FontKind::Roman)
                                > max_width
                        {
                            self.text_line(&part, size, indent)?;
                            part.clear();
                        }
                        part.push(ch);
                    }
                    line = part;
                }
            }
            if !line.is_empty() {
                self.text_line(&line, size, indent)?;
            } else if paragraph.is_empty() {
                self.y += size * 0.7;
            }
        }
        Ok(())
    }

    fn math(&mut self, row: &[Item], size: f32) -> Result<(), String> {
        let boxed = {
            let measure = PdfMeasure(&self.face);
            let mut boxed = Layout::new(&measure, size, false).layout(row);
            let fit = ((PAGE_WIDTH - MARGIN * 2.0) / boxed.w.max(1.0))
                .min((PAGE_HEIGHT - MARGIN * 2.0 - 8.0) / boxed.height().max(1.0))
                .min(1.0);
            if fit < 1.0 {
                boxed = Layout::new(&measure, size * fit, false).layout(row);
            }
            boxed
        };
        let height = boxed.height() + 8.0;
        self.ensure_space(height);
        let origin_x = MARGIN;
        let origin_y = self.y + boxed.asc;
        for op in &boxed.ops {
            match op {
                Op::Text {
                    x, y, text, size, ..
                } => {
                    let (glyphs, _) = self.glyphs(text)?;
                    let baseline = PAGE_HEIGHT - origin_y - y;
                    self.content.push_str(&format!(
                        "BT /F1 {:.3} Tf 1 0 0 1 {:.3} {:.3} Tm <{}> Tj ET\n",
                        size,
                        origin_x + x,
                        baseline,
                        glyphs
                    ));
                }
                Op::Line {
                    x0,
                    y0,
                    x1,
                    y1,
                    width,
                } => {
                    self.content.push_str(&format!(
                        "{:.3} w {:.3} {:.3} m {:.3} {:.3} l S\n",
                        width,
                        origin_x + x0,
                        PAGE_HEIGHT - origin_y - y0,
                        origin_x + x1,
                        PAGE_HEIGHT - origin_y - y1
                    ));
                }
                Op::Poly { pts, width } => {
                    if let Some((x, y)) = pts.first() {
                        self.content.push_str(&format!(
                            "{:.3} w {:.3} {:.3} m ",
                            width,
                            origin_x + x,
                            PAGE_HEIGHT - origin_y - y
                        ));
                        for (x, y) in &pts[1..] {
                            self.content.push_str(&format!(
                                "{:.3} {:.3} l ",
                                origin_x + x,
                                PAGE_HEIGHT - origin_y - y
                            ));
                        }
                        self.content.push_str("S\n");
                    }
                }
                Op::Placeholder { .. } => {}
            }
        }
        self.y += height;
        Ok(())
    }

    fn plot(&mut self, plot: &PlotData, cell: usize) -> Result<(), String> {
        if plot.is_3d() {
            return Err(format!(
                "PDF export does not support 3D plots (cell {cell})"
            ));
        }
        let all: Vec<(f64, f64)> = plot
            .series
            .iter()
            .flat_map(|s| s.pts.iter().flatten().copied())
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .collect();
        self.ensure_space(230.0);
        let left = MARGIN + 38.0;
        let bottom = PAGE_HEIGHT - self.y - 185.0;
        let width = PAGE_WIDTH - MARGIN * 2.0 - 44.0;
        let height = 150.0;
        if all.is_empty() {
            self.text("[Plot contains no finite sampled points]", 10.0, 0.0)?;
            if !plot.xlabel.is_empty() {
                self.text(&plot.xlabel, 9.0, 38.0)?;
            }
            if !plot.ylabel.is_empty() {
                self.text(&plot.ylabel, 9.0, 0.0)?;
            }
            for series in &plot.series {
                if !series.label.is_empty() {
                    self.text(&series.label, 9.0, 12.0)?;
                }
            }
            self.y += 8.0;
            return Ok(());
        }
        let (mut xmin, mut xmax) = all
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.0), hi.max(p.0))
            });
        let (mut ymin, mut ymax) = all
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.1), hi.max(p.1))
            });
        if let Some((lo, hi)) = plot
            .yrange
            .filter(|(lo, hi)| lo.is_finite() && hi.is_finite() && lo < hi)
        {
            ymin = lo;
            ymax = hi;
        }
        if xmin == xmax {
            xmin -= 1.0;
            xmax += 1.0;
        }
        if ymin == ymax {
            ymin -= 1.0;
            ymax += 1.0;
        }
        self.content.push_str(&format!(
            "{:.3} w {:.3} {:.3} m {:.3} {:.3} l {:.3} {:.3} l {:.3} {:.3} l h S\n",
            0.6,
            left,
            bottom,
            left + width,
            bottom,
            left + width,
            bottom + height,
            left,
            bottom + height
        ));
        for series in &plot.series {
            let mut segment = String::new();
            for point in &series.pts {
                let Some((x, y)) = point.filter(|(x, y)| x.is_finite() && y.is_finite()) else {
                    if !segment.is_empty() {
                        self.content.push_str(&segment);
                        self.content.push_str("S\n");
                        segment.clear();
                    }
                    continue;
                };
                let px = left + ((x - xmin) / (xmax - xmin)) as f32 * width;
                let py = bottom + ((y - ymin) / (ymax - ymin)) as f32 * height;
                segment.push_str(&format!(
                    "{:.3} {:.3} {} ",
                    px,
                    py,
                    if segment.is_empty() { "m" } else { "l" }
                ));
            }
            if !segment.is_empty() {
                self.content.push_str(&segment);
                self.content.push_str("S\n");
            }
        }
        self.y += 190.0;
        if !plot.xlabel.is_empty() {
            self.text(&plot.xlabel, 9.0, 38.0)?;
        }
        if !plot.ylabel.is_empty() {
            self.text(&plot.ylabel, 9.0, 0.0)?;
        }
        for series in &plot.series {
            if !series.label.is_empty() {
                self.text(&series.label, 9.0, 12.0)?;
            }
        }
        self.y += 5.0;
        Ok(())
    }

    fn build(mut self) -> Result<Vec<u8>, String> {
        if !self.content.is_empty() || self.pages.is_empty() {
            self.finish_page();
        }
        let units = self.face.units_per_em() as f32;
        let widths = (0..self.face.number_of_glyphs())
            .map(|index| {
                let advance = self
                    .face
                    .glyph_hor_advance(ttf_parser::GlyphId(index))
                    .unwrap_or(0) as f32;
                format!("{:.0}", advance * 1000.0 / units)
            })
            .collect::<Vec<_>>()
            .join(" ");
        let mut objects: Vec<Vec<u8>> = vec![Vec::new(); 8];
        objects[1] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
        objects[2] = Vec::new();
        objects[3] = b"<< /Type /Font /Subtype /Type0 /BaseFont /DejaVuSans /Encoding /Identity-H /DescendantFonts [4 0 R] /ToUnicode 7 0 R >>".to_vec();
        objects[4] = format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /DejaVuSans /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 5 0 R /DW 1000 /W [0 [{}]] /CIDToGIDMap /Identity >>", widths).into_bytes();
        let scale = 1000.0 / units;
        let bbox = self.face.global_bounding_box();
        objects[5] = format!("<< /Type /FontDescriptor /FontName /DejaVuSans /Flags 32 /FontBBox [{:.0} {:.0} {:.0} {:.0}] /ItalicAngle 0 /Ascent {:.0} /Descent {:.0} /CapHeight {:.0} /StemV 80 /FontFile2 6 0 R >>",
            bbox.x_min as f32 * scale, bbox.y_min as f32 * scale, bbox.x_max as f32 * scale, bbox.y_max as f32 * scale,
            self.face.ascender() as f32 * scale, self.face.descender() as f32 * scale, self.face.ascender() as f32 * scale).into_bytes();
        objects[6] = stream_object(FONT, &format!("/Length1 {}", FONT.len()));
        objects[7] = stream_object(&to_unicode_cmap(&self.unicode), "");
        let mut kids = Vec::new();
        for (page, content) in self.pages.iter().enumerate() {
            let page_id = 8 + page * 2;
            let stream_id = page_id + 1;
            kids.push(format!("{page_id} 0 R"));
            while objects.len() <= stream_id {
                objects.push(Vec::new());
            }
            objects[page_id] = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH:.0} {PAGE_HEIGHT:.0}] /Resources << /Font << /F1 3 0 R >> >> /Contents {stream_id} 0 R >>").into_bytes();
            objects[stream_id] = stream_object(content.as_bytes(), "");
        }
        objects[2] = format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            self.pages.len()
        )
        .into_bytes();
        Ok(write_pdf(objects))
    }
}

fn stream_object(data: &[u8], extra: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {} {} >>\nstream\n", data.len(), extra).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

fn to_unicode_cmap(map: &BTreeMap<u16, char>) -> Vec<u8> {
    let mut cmap = String::from("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    for entries in map.iter().collect::<Vec<_>>().chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", entries.len()));
        for (glyph, ch) in entries {
            let mut chars = [0u16; 2];
            let utf16 = ch.encode_utf16(&mut chars);
            let dest = utf16.iter().map(|u| format!("{u:04X}")).collect::<String>();
            cmap.push_str(&format!("<{glyph:04X}> <{dest}>\n"));
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend");
    cmap.into_bytes()
}

fn write_pdf(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0usize; objects.len()];
    for (id, object) in objects.iter().enumerate().skip(1) {
        offsets[id] = pdf.len();
        pdf.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len()
        )
        .as_bytes(),
    );
    pdf
}

pub fn export(notebook: &Notebook) -> Result<Vec<u8>, String> {
    let mut doc = PdfDocument::new()?;
    for (index, cell) in notebook.cells.iter().enumerate() {
        match cell.kind {
            CellKind::Title => {
                doc.text(&cell.source, 22.0, 0.0)?;
                doc.y += 5.0;
            }
            CellKind::Section => {
                doc.text(&cell.source, 16.0, 0.0)?;
                doc.y += 3.0;
            }
            CellKind::Text => {
                doc.text(&cell.source, 11.0, 0.0)?;
                doc.y += 5.0;
            }
            CellKind::Code => {
                if !cell.source.trim().is_empty() {
                    if cell.mode == maximus_core::InputMode::Math {
                        doc.math(&from_latex(&cell.source), 15.0)?;
                    } else {
                        doc.text(&cell.source, 10.0, 12.0)?;
                    }
                }
                for output in &cell.outputs {
                    match output {
                        OutputItem::Result { label, text, .. } => {
                            if !label.is_empty() {
                                doc.text(label, 9.0, 12.0)?;
                            }
                            let row = maxima_to_tree(text).map_err(|e| {
                                format!("Could not typeset result in cell {}: {e}", index + 1)
                            })?;
                            doc.math(&row, 14.0)?;
                        }
                        OutputItem::Text(text) => doc.text(text, 10.0, 12.0)?,
                        OutputItem::Error(text) => {
                            doc.text(&format!("Error: {text}"), 10.0, 12.0)?
                        }
                        OutputItem::Plot(plot) => doc.plot(plot, index + 1)?,
                    }
                }
                doc.y += 12.0;
            }
        }
    }
    doc.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use maximus_core::{InputMode, PlotData};

    #[test]
    fn exports_typeset_notebook_with_embedded_font() {
        let mut notebook = Notebook::empty();
        let title = notebook.add_cell(CellKind::Title, InputMode::Math);
        notebook.cell_mut(title).unwrap().source = "A Notebook".into();
        let text = notebook.add_cell(CellKind::Text, InputMode::Math);
        notebook.cell_mut(text).unwrap().source = "A short introduction.".into();
        let code = notebook.add_cell(CellKind::Code, InputMode::Math);
        let cell = notebook.cell_mut(code).unwrap();
        cell.source = "\\frac{1}{2}+x".into();
        cell.outputs.push(OutputItem::Result {
            label: "%o1".into(),
            text: "x+1/2".into(),
            tex: String::new(),
        });
        cell.outputs.push(OutputItem::Plot(PlotData {
            kind: "plot2d".into(),
            series: vec![maximus_core::Series {
                label: "line".into(),
                pts: vec![Some((0.0, 0.0)), Some((1.0, 1.0))],
            }],
            xlabel: "x".into(),
            ylabel: "y".into(),
            yrange: None,
            grid: None,
        }));
        let pdf = export(&notebook).unwrap();
        let bytes = String::from_utf8_lossy(&pdf);
        assert!(bytes.starts_with("%PDF-1.7"));
        assert!(bytes.contains("/FontFile2"));
        assert!(pdf.windows(FONT.len()).any(|window| window == FONT));
        assert!(bytes.contains("/Type /Page"));
        assert!(bytes.contains("startxref"));
    }

    #[test]
    fn reports_content_that_cannot_be_exported() {
        let mut notebook = Notebook::empty();
        let id = notebook.add_cell(CellKind::Code, InputMode::Math);
        notebook
            .cell_mut(id)
            .unwrap()
            .outputs
            .push(OutputItem::Result {
                label: "%o1".into(),
                text: "not a maxima expression?".into(),
                tex: String::new(),
            });
        assert!(export(&notebook)
            .unwrap_err()
            .contains("Could not typeset result"));
        notebook.cell_mut(id).unwrap().outputs.clear();
        notebook
            .cell_mut(id)
            .unwrap()
            .outputs
            .push(OutputItem::Plot(PlotData {
                kind: "plot3d".into(),
                ..PlotData::default()
            }));
        assert!(export(&notebook)
            .unwrap_err()
            .contains("does not support 3D plots"));
    }
}
