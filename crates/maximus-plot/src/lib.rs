//! Plot rendering: `egui_plot` (default) and an optional gnuplot backend.

use egui::Color32;
use egui_plot::{CoordinatesFormatter, Corner, Legend, Line, Plot, PlotPoint, PlotPoints, Points, Text};
use maximus_core::{Grid3, PlotData};

pub const PALETTE: [Color32; 8] = [
    Color32::from_rgb(0x1f, 0x77, 0xb4),
    Color32::from_rgb(0xd6, 0x27, 0x28),
    Color32::from_rgb(0x2c, 0xa0, 0x2c),
    Color32::from_rgb(0xff, 0x7f, 0x0e),
    Color32::from_rgb(0x94, 0x67, 0xbd),
    Color32::from_rgb(0x8c, 0x56, 0x4b),
    Color32::from_rgb(0xe3, 0x77, 0xc2),
    Color32::from_rgb(0x17, 0xbe, 0xcf),
];

/// Splits a series at undefined points into drawable segments.
pub fn segments(pts: &[Option<(f64, f64)>]) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    for p in pts {
        match p {
            Some((x, y)) => cur.push([*x, *y]),
            None => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A point of interest on a curve that the cursor snaps to.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapPoint {
    pub x: f64,
    pub y: f64,
    pub kind: &'static str,
}

/// Zeros, local extrema and the y-intercept of one series (poles are ignored).
pub fn critical_points(pts: &[Option<(f64, f64)>]) -> Vec<SnapPoint> {
    let ys: Vec<f64> = pts.iter().flatten().map(|p| p.1).collect();
    if ys.len() < 3 {
        return vec![];
    }
    let (lo, hi) = robust_range(&ys);
    let limit = lo.abs().max(hi.abs()) * 2.0;
    let mut out = Vec::new();
    for seg in segments(pts) {
        for w in seg.windows(2) {
            let ([x0, y0], [x1, y1]) = (w[0], w[1]);
            if y0.abs() > limit || y1.abs() > limit {
                continue;
            }
            if y0 == 0.0 || (y0 < 0.0) != (y1 < 0.0) && y1 != 0.0 {
                let t = if y1 == y0 { 0.0 } else { y0 / (y0 - y1) };
                out.push(SnapPoint { x: x0 + t * (x1 - x0), y: 0.0, kind: "zero" });
            }
            if x0 < 0.0 && x1 >= 0.0 && x1 > x0 {
                let t = -x0 / (x1 - x0);
                out.push(SnapPoint { x: 0.0, y: y0 + t * (y1 - y0), kind: "y-intercept" });
            }
        }
        for w in seg.windows(3) {
            let (a, b, c) = (w[0], w[1], w[2]);
            if b[1].abs() > limit {
                continue;
            }
            if b[1] > a[1] && b[1] >= c[1] {
                out.push(SnapPoint { x: b[0], y: b[1], kind: "max" });
            } else if b[1] < a[1] && b[1] <= c[1] {
                out.push(SnapPoint { x: b[0], y: b[1], kind: "min" });
            }
        }
    }
    out
}

/// Chooses what the cursor snaps to: a point of interest within `radius`, else the nearest sample.
/// `to_screen` maps plot coordinates to pixels.
pub fn snap_target(data: &PlotData, pointer: (f32, f32), radius: f32, to_screen: &dyn Fn(f64, f64) -> (f32, f32)) -> Option<SnapPoint> {
    let dist = |x: f64, y: f64| {
        let (sx, sy) = to_screen(x, y);
        ((sx - pointer.0).powi(2) + (sy - pointer.1).powi(2)).sqrt()
    };
    let mut best: Option<(f32, SnapPoint)> = None;
    for s in &data.series {
        if s.label == "points" {
            for p in s.pts.iter().flatten() {
                let d = dist(p.0, p.1);
                if d <= radius && best.as_ref().map_or(true, |(b, _)| d < *b) {
                    best = Some((d, SnapPoint { x: p.0, y: p.1, kind: "point" }));
                }
            }
            continue;
        }
        for c in critical_points(&s.pts) {
            let d = dist(c.x, c.y);
            // Points of interest win over plain curve points.
            if d <= radius && best.as_ref().map_or(true, |(b, _)| d < *b) {
                best = Some((d, c));
            }
        }
    }
    if best.is_some() {
        return best.map(|b| b.1);
    }
    for s in &data.series {
        for p in s.pts.iter().flatten() {
            let d = dist(p.0, p.1);
            if d <= radius * 0.7 && best.as_ref().map_or(true, |(b, _)| d < *b) {
                best = Some((d, SnapPoint { x: p.0, y: p.1, kind: "" }));
            }
        }
    }
    best.map(|b| b.1)
}

fn fmt_num(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    if !(1e-4..1e6).contains(&a) {
        format!("{:.4e}", v)
    } else {
        let s = format!("{:.5}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ViewCmd {
    ZoomIn,
    ZoomOut,
    Reset,
}

/// Draws a 2D or 3D plot of the data using egui_plot.
pub fn show_plot(ui: &mut egui::Ui, id: impl std::hash::Hash, data: &PlotData, height: f32) {
    if data.is_3d() {
        if let Some(g) = &data.grid {
            show_surface(ui, id, g, height);
        }
        return;
    }
    let cmd_key = ui.make_persistent_id(("plot2d-cmd", id_hash(&id)));
    let cmd: Option<ViewCmd> = ui.data_mut(|d| d.remove_temp::<u8>(cmd_key)).map(|c| match c {
        0 => ViewCmd::ZoomIn,
        1 => ViewCmd::ZoomOut,
        _ => ViewCmd::Reset,
    });
    let mut plot = Plot::new(("plot2d", id_hash(&id)))
        .height(height)
        .legend(Legend::default())
        .x_axis_label(data.xlabel.clone())
        .allow_scroll(false)
        .allow_zoom(true)
        .allow_boxed_zoom(true)
        .allow_drag(true)
        .allow_double_click_reset(true)
        .show_axes(true)
        .show_grid(true)
        .coordinates_formatter(Corner::LeftBottom, CoordinatesFormatter::with_decimals(4));
    if let Some((lo, hi)) = data.yrange {
        plot = plot.default_y_bounds(lo, hi);
    }
    // Keep runaway asymptotes from dominating the automatic view.
    let ys: Vec<f64> = data.series.iter().flat_map(|s| s.pts.iter().flatten().map(|p| p.1)).collect();
    if data.yrange.is_none() && !ys.is_empty() {
        let (lo, hi) = robust_range(&ys);
        plot = plot.default_y_bounds(lo, hi);
    }
    if cmd == Some(ViewCmd::Reset) {
        plot = plot.reset();
    }
    plot.show(ui, |pu| {
        match cmd {
            Some(ViewCmd::ZoomIn) => {
                let b = pu.plot_bounds();
                pu.zoom_bounds(egui::vec2(1.3, 1.3), PlotPoint::new((b.min()[0] + b.max()[0]) / 2.0, (b.min()[1] + b.max()[1]) / 2.0));
            }
            Some(ViewCmd::ZoomOut) => {
                let b = pu.plot_bounds();
                pu.zoom_bounds(egui::vec2(1.0 / 1.3, 1.0 / 1.3), PlotPoint::new((b.min()[0] + b.max()[0]) / 2.0, (b.min()[1] + b.max()[1]) / 2.0));
            }
            _ => {}
        }
        for (i, s) in data.series.iter().enumerate() {
            let color = PALETTE[i % PALETTE.len()];
            if s.label == "points" {
                let pts: Vec<[f64; 2]> = s.pts.iter().flatten().map(|(x, y)| [*x, *y]).collect();
                pu.points(Points::new(s.label.clone(), PlotPoints::new(pts)).color(color).radius(3.5).filled(true));
                continue;
            }
            for seg in segments(&s.pts) {
                pu.line(Line::new(s.label.clone(), PlotPoints::new(seg)).color(color).width(2.0));
            }
        }
        if let Some(hover) = pu.response().hover_pos() {
            let tf = pu.transform().clone();
            let to_screen = |x: f64, y: f64| {
                let p = tf.position_from_point(&PlotPoint::new(x, y));
                (p.x, p.y)
            };
            if let Some(sp) = snap_target(data, (hover.x, hover.y), 16.0, &to_screen) {
                pu.points(Points::new("", PlotPoints::new(vec![[sp.x, sp.y]])).color(Color32::from_rgb(0xff, 0xc1, 0x07)).radius(5.0).filled(true));
                let label = if sp.kind.is_empty() { format!("({}, {})", fmt_num(sp.x), fmt_num(sp.y)) } else { format!("{}  ({}, {})", sp.kind, fmt_num(sp.x), fmt_num(sp.y)) };
                pu.text(Text::new("", PlotPoint::new(sp.x, sp.y), egui::RichText::new(label).color(Color32::WHITE).background_color(Color32::from_black_alpha(170)).size(12.0)).anchor(egui::Align2::LEFT_BOTTOM));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (label, c) in [("Zoom in", 0u8), ("Zoom out", 1), ("Reset view", 2)] {
            if ui.small_button(label).clicked() {
                ui.data_mut(|d| d.insert_temp(cmd_key, c));
            }
        }
        ui.label(egui::RichText::new("drag: pan · Ctrl+scroll: zoom · right-drag: zoom box · double-click: reset").small().weak());
    });
}

fn id_hash(id: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut h);
    h.finish()
}

/// Range covering the bulk of the values (5th..95th percentile padded), for functions with poles.
pub fn robust_range(ys: &[f64]) -> (f64, f64) {
    let mut v: Vec<f64> = ys.iter().copied().filter(|y| y.is_finite()).collect();
    if v.is_empty() {
        return (-1.0, 1.0);
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let (min, max) = (v[0], v[v.len() - 1]);
    let q = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
    let (lo, hi) = (q(0.02), q(0.98));
    let span = (hi - lo).max(1e-9);
    let full = max - min;
    if full <= span * 3.0 {
        let pad = full.max(1e-9) * 0.08;
        (min - pad, max + pad)
    } else {
        (lo - span * 0.15, hi + span * 0.15)
    }
}

#[derive(Clone, Copy)]
struct View3 {
    az: f32,
    el: f32,
}

fn show_surface(ui: &mut egui::Ui, id: impl std::hash::Hash, g: &Grid3, height: f32) {
    let key = ui.make_persistent_id(("plot3d-view", id_hash(&id)));
    let mut view = ui.data_mut(|d| d.get_temp::<(f32, f32)>(key)).map(|(az, el)| View3 { az, el }).unwrap_or(View3 { az: 0.6, el: 0.5 });
    let finite: Vec<f64> = g.zs.iter().flatten().flatten().copied().filter(|z| z.is_finite()).collect();
    let (zmin, zmax) = finite.iter().fold((f64::MAX, f64::MIN), |(a, b), z| (a.min(*z), b.max(*z)));
    let norm = |v: f64, lo: f64, hi: f64| if hi > lo { 2.0 * (v - lo) / (hi - lo) - 1.0 } else { 0.0 };
    let (x0, x1) = (g.xs.first().copied().unwrap_or(0.0), g.xs.last().copied().unwrap_or(1.0));
    let (y0, y1) = (g.ys.first().copied().unwrap_or(0.0), g.ys.last().copied().unwrap_or(1.0));
    let (sa, ca) = (view.az.sin() as f64, view.az.cos() as f64);
    let (se, ce) = (view.el.sin() as f64, view.el.cos() as f64);
    let project = |i: usize, j: usize| -> Option<([f64; 2], f64)> {
        let z = g.zs.get(i)?.get(j).copied().flatten()?;
        let (x, y, z) = (norm(g.xs[i], x0, x1), norm(g.ys[j], y0, y1), norm(z, zmin, zmax) * 0.8);
        let xr = x * ca - y * sa;
        let yr = x * sa + y * ca;
        Some(([xr, z * ce + yr * se], z))
    };
    let resp = Plot::new(("surface", id_hash(&id)))
        .height(height)
        .data_aspect(1.0)
        .show_axes(false)
        .show_grid(false)
        .allow_drag(false)
        .allow_zoom(true)
        .allow_scroll(false)
        .include_x(-1.6)
        .include_x(1.6)
        .include_y(-1.3)
        .include_y(1.3)
        .show(ui, |pu| {
            let color_at = |z: f64| {
                let t = ((z / 0.8 + 1.0) / 2.0).clamp(0.0, 1.0) as f32;
                Color32::from_rgb((40.0 + 200.0 * t) as u8, (90.0 + 80.0 * (1.0 - (2.0 * t - 1.0).abs())) as u8, (220.0 - 190.0 * t) as u8)
            };
            let (nx, ny) = (g.xs.len(), g.ys.len());
            for i in 0..nx {
                let mut seg: Vec<[f64; 2]> = Vec::new();
                let mut zc = 0.0;
                for j in 0..ny {
                    match project(i, j) {
                        Some((p, z)) => {
                            seg.push(p);
                            zc += z;
                        }
                        None => {
                            if seg.len() > 1 {
                                pu.line(Line::new("", PlotPoints::new(std::mem::take(&mut seg))).color(color_at(zc / 8.0)).width(1.0));
                            }
                            seg.clear();
                        }
                    }
                }
                if seg.len() > 1 {
                    let c = seg.len() as f64;
                    pu.line(Line::new("", PlotPoints::new(seg)).color(color_at(zc / c)).width(1.0));
                }
            }
            for j in 0..ny {
                let seg: Vec<[f64; 2]> = (0..nx).filter_map(|i| project(i, j).map(|(p, _)| p)).collect();
                if seg.len() > 1 {
                    pu.line(Line::new("", PlotPoints::new(seg)).color(Color32::from_gray(150)).width(0.8));
                }
            }
        });
    if resp.response.dragged() {
        let d = resp.response.drag_delta();
        view.az -= d.x * 0.01;
        view.el = (view.el + d.y * 0.01).clamp(-1.4, 1.4);
        ui.data_mut(|dm| dm.insert_temp(key, (view.az, view.el)));
    }
    ui.label(egui::RichText::new(format!("{}  (drag to rotate)", g.label)).small().weak());
}

// ---------------------------------------------------------------- gnuplot

#[cfg(not(target_arch = "wasm32"))]
pub mod gnuplot {
    use maximus_core::PlotData;
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};

    /// Builds a gnuplot script (with inline data) that renders `data` to a PNG at `out`.
    pub fn script(data: &PlotData, out: &Path) -> String {
        let mut s = String::new();
        s.push_str("set terminal pngcairo size 900,560 enhanced font 'sans,11'\n");
        s.push_str(&format!("set output '{}'\nset grid\n", out.display()));
        if data.is_3d() {
            if let Some(g) = &data.grid {
                s.push_str("set hidden3d\nset pm3d\nset view 60,30\n");
                s.push_str(&format!("set xlabel '{}'\nset ylabel '{}'\n", data.xlabel, data.ylabel));
                s.push_str(&format!("splot '-' with lines title '{}'\n", g.label.replace('\'', "")));
                for (i, x) in g.xs.iter().enumerate() {
                    for (j, y) in g.ys.iter().enumerate() {
                        match g.zs[i][j] {
                            Some(z) => s.push_str(&format!("{} {} {}\n", x, y, z)),
                            None => s.push_str(&format!("{} {} NaN\n", x, y)),
                        }
                    }
                    s.push('\n');
                }
                s.push_str("e\n");
            }
            return s;
        }
        s.push_str(&format!("set xlabel '{}'\n", data.xlabel));
        if let Some((lo, hi)) = data.yrange {
            s.push_str(&format!("set yrange [{}:{}]\n", lo, hi));
        } else {
            let ys: Vec<f64> = data.series.iter().flat_map(|s| s.pts.iter().flatten().map(|p| p.1)).collect();
            if !ys.is_empty() {
                let (lo, hi) = crate::robust_range(&ys);
                s.push_str(&format!("set yrange [{}:{}]\n", lo, hi));
            }
        }
        let plots: Vec<String> = data
            .series
            .iter()
            .map(|se| {
                let style = if se.label == "points" { "points pt 7" } else { "lines lw 2" };
                format!("'-' with {} title '{}'", style, se.label.replace('\'', ""))
            })
            .collect();
        s.push_str(&format!("plot {}\n", plots.join(", ")));
        for se in &data.series {
            for p in &se.pts {
                match p {
                    Some((x, y)) => s.push_str(&format!("{} {}\n", x, y)),
                    None => s.push('\n'),
                }
            }
            s.push_str("e\n");
        }
        s
    }

    pub fn render(data: &PlotData, gnuplot: &str, out: &Path) -> Result<(), String> {
        let script = script(data, out);
        let mut child = Command::new(gnuplot)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run '{}': {}", gnuplot, e))?;
        child.stdin.take().unwrap().write_all(script.as_bytes()).map_err(|e| e.to_string())?;
        let o = child.wait_with_output().map_err(|e| e.to_string())?;
        if o.status.success() && out.exists() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maximus_core::Series;

    #[test]
    fn segments_split_on_gaps() {
        let s = segments(&[Some((0.0, 0.0)), Some((1.0, 1.0)), None, Some((3.0, 3.0))]);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].len(), 2);
    }

    #[test]
    fn finds_zeros_extrema_and_intercept() {
        let pts: Vec<Option<(f64, f64)>> = (0..=240).map(|i| {
            let x = -6.0 + 12.0 * i as f64 / 240.0;
            Some((x, x.sin()))
        }).collect();
        let c = critical_points(&pts);
        let zeros: Vec<f64> = c.iter().filter(|p| p.kind == "zero").map(|p| p.x).collect();
        assert!(zeros.iter().any(|z| (z - std::f64::consts::PI).abs() < 0.01), "{zeros:?}");
        assert!(zeros.iter().any(|z| z.abs() < 0.01));
        assert!(c.iter().any(|p| p.kind == "max" && (p.x - std::f64::consts::FRAC_PI_2).abs() < 0.1));
        assert!(c.iter().any(|p| p.kind == "min" && (p.x + std::f64::consts::FRAC_PI_2).abs() < 0.1));
        assert!(c.iter().any(|p| p.kind == "y-intercept" && p.y.abs() < 0.01));
    }

    #[test]
    fn poles_do_not_create_snap_points() {
        let pts: Vec<Option<(f64, f64)>> = (-100..=100).map(|i| {
            let x = i as f64 / 10.0 + 0.001;
            Some((x, 1.0 / x))
        }).collect();
        assert!(critical_points(&pts).iter().all(|p| p.kind != "zero" || p.x.abs() < 0.2), "pole must not count as a zero");
    }

    #[test]
    fn snapping_prefers_points_of_interest() {
        let d = PlotData {
            kind: "plot2d".into(),
            series: vec![Series { label: "sin(x)".into(), pts: (0..=240).map(|i| { let x = -6.0 + 12.0 * i as f64 / 240.0; Some((x, x.sin())) }).collect() }],
            ..Default::default()
        };
        let to_screen = |x: f64, y: f64| ((x * 50.0) as f32, (-y * 50.0) as f32);
        // Pointer a few pixels from the maximum at pi/2.
        let (px, py) = to_screen(std::f64::consts::FRAC_PI_2, 1.0);
        let s = snap_target(&d, (px + 6.0, py + 5.0), 16.0, &to_screen).unwrap();
        assert_eq!(s.kind, "max");
        assert!(snap_target(&d, (px + 6.0, py + 200.0), 16.0, &to_screen).is_none());
    }

    #[test]
    fn robust_range_ignores_poles() {
        let mut ys: Vec<f64> = (0..200).map(|i| (i as f64 / 20.0).sin()).collect();
        ys.push(1e9);
        let (lo, hi) = robust_range(&ys);
        assert!(hi < 100.0 && lo > -100.0, "{lo} {hi}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gnuplot_renders_png_if_installed() {
        if std::process::Command::new("gnuplot").arg("--version").output().is_err() {
            return;
        }
        let d = PlotData {
            kind: "plot2d".into(),
            series: vec![Series { label: "sin(x)".into(), pts: (0..50).map(|i| Some((i as f64 / 5.0, (i as f64 / 5.0).sin()))).collect() }],
            ..Default::default()
        };
        let out = std::env::temp_dir().join("maximus-test-plot.png");
        let _ = std::fs::remove_file(&out);
        gnuplot::render(&d, "gnuplot", &out).unwrap();
        assert!(std::fs::metadata(&out).unwrap().len() > 1000);
        let _ = std::fs::remove_file(&out);
    }
}
