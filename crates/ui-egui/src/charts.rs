//! The Charts pane: a histogram, a box plot, a scatter plot and the Moran scatter plot of a
//! layer's fields, linked to the map. Selected features are highlighted in every chart; brushing
//! a chart (dragging across it, or clicking a bar or a point) selects its features through
//! `feature.select`, so a selection made in a chart is a command like any other (Shift adds).
//! The statistics come from the engine (`mkaegis_engine::explore`); this module only draws them.

use std::collections::BTreeSet;

use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use mkaegis_core::Value;
use mkaegis_engine::explore::{self, Bins};
use serde_json::json;

use crate::MapApp;
use crate::theme::Tokens;

/// The most features a chart draws; beyond, it says so instead of slowing every frame.
const MAX_FEATURES: usize = 200_000;
const PLOT_HEIGHT: f32 = 220.0;
/// Room around the plot for the axes' numbers.
const MARGIN: f32 = 34.0;
/// How near a click must be to a point to pick it, in points.
const PICK: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chart {
    #[default]
    Histogram,
    BoxPlot,
    Scatter,
    MoranScatter,
}

impl Chart {
    const ALL: [Chart; 4] = [Chart::Histogram, Chart::BoxPlot, Chart::Scatter, Chart::MoranScatter];

    pub fn name(self) -> &'static str {
        match self {
            Chart::Histogram => "Histogram",
            Chart::BoxPlot => "Box Plot",
            Chart::Scatter => "Scatter Plot",
            Chart::MoranScatter => "Moran Scatter Plot",
        }
    }
}

/// What the Charts pane shows.
#[derive(Debug, Clone)]
pub struct ChartState {
    pub kind: Chart,
    pub layer: Option<u32>,
    pub x: String,
    pub y: String,
    pub weights: String,
    /// Histogram bins: `sturges`, `fd` or a count.
    pub bins: String,
    /// Box plot fences, in IQRs.
    pub hinge: f64,
    /// Where a drag across the plot started.
    brush: Option<Pos2>,
    /// The plot's area in the last frame (tests click into it).
    pub(crate) plot: Option<Rect>,
}

impl Default for ChartState {
    fn default() -> Self {
        ChartState {
            kind: Chart::Histogram,
            layer: None,
            x: String::new(),
            y: String::new(),
            weights: String::new(),
            bins: "sturges".into(),
            hinge: 1.5,
            brush: None,
            plot: None,
        }
    }
}

/// A plot's data-to-screen mapping.
struct Frame {
    rect: Rect,
    x: (f64, f64),
    y: (f64, f64),
}

impl Frame {
    fn new(rect: Rect, x: (f64, f64), y: (f64, f64)) -> Frame {
        let pad = |(a, b): (f64, f64)| if b > a { (a, b) } else { (a - 0.5, b + 0.5) };
        Frame { rect, x: pad(x), y: pad(y) }
    }

    fn at(&self, x: f64, y: f64) -> Pos2 {
        let fx = ((x - self.x.0) / (self.x.1 - self.x.0)) as f32;
        let fy = ((y - self.y.0) / (self.y.1 - self.y.0)) as f32;
        pos2(self.rect.left() + fx * self.rect.width(), self.rect.bottom() - fy * self.rect.height())
    }

    fn x_of(&self, px: f32) -> f64 {
        self.x.0 + f64::from((px - self.rect.left()) / self.rect.width()) * (self.x.1 - self.x.0)
    }
}

pub fn show(app: &mut MapApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let mut c = std::mem::take(&mut app.charts);
    egui::ComboBox::from_id_salt("chart-kind").width(ui.available_width()).selected_text(c.kind.name()).show_ui(ui, |ui| {
        for k in Chart::ALL {
            ui.selectable_value(&mut c.kind, k, k.name());
        }
    });
    ui.add_space(4.0);
    if c.kind == Chart::MoranScatter {
        let names: Vec<(String, u32)> = app.session.map.weights.iter().map(|w| (w.name.clone(), w.layer)).collect();
        if !names.iter().any(|(n, _)| *n == c.weights) {
            c.weights = names.last().map(|(n, _)| n.clone()).unwrap_or_default();
        }
        ui.label("Spatial Weights");
        egui::ComboBox::from_id_salt("chart-weights").width(ui.available_width()).selected_text(c.weights.as_str()).show_ui(ui, |ui| {
            for (n, _) in &names {
                ui.selectable_value(&mut c.weights, n.clone(), n);
            }
        });
        c.layer = names.iter().find(|(n, _)| *n == c.weights).map(|(_, l)| *l);
        if names.is_empty() {
            ui.label(egui::RichText::new("Build spatial weights first (Statistics › Spatial Weights).").size(11.0).color(t.muted));
        }
    } else {
        layer_combo(app, ui, &mut c.layer);
    }
    let numeric: Vec<String> = c
        .layer
        .and_then(|id| app.session.map.layers.iter().find(|l| l.id == id))
        .map(|l| l.data.fields.iter().filter(|f| f.kind.is_numeric()).map(|f| f.name.clone()).collect())
        .unwrap_or_default();
    // The field the layer is mapped by, when it is numeric: what the person is looking at.
    if !numeric.contains(&c.x)
        && let Some(mapped) = c.layer.and_then(|id| app.session.map.layers.iter().find(|l| l.id == id)).and_then(|l| l.renderer.field())
        && let Some(f) = numeric.iter().find(|f| f.eq_ignore_ascii_case(mapped))
    {
        c.x = f.clone();
    }
    field_combo(ui, if c.kind == Chart::Scatter { "X" } else { "Field" }, "chart-x", &numeric, &mut c.x);
    if c.kind == Chart::Scatter {
        field_combo(ui, "Y", "chart-y", &numeric, &mut c.y);
    }
    match c.kind {
        Chart::Histogram => {
            ui.horizontal(|ui| {
                ui.label("Bins");
                for (label, rule) in [("Sturges", "sturges"), ("Freedman–Diaconis", "fd"), ("10", "10"), ("20", "20")] {
                    if ui.selectable_label(c.bins == rule, label).clicked() {
                        c.bins = rule.into();
                    }
                }
            });
        }
        Chart::BoxPlot => {
            ui.horizontal(|ui| {
                ui.label("Fences");
                for h in [1.5, 3.0] {
                    if ui.selectable_label(c.hinge == h, format!("{h} × IQR")).clicked() {
                        c.hinge = h;
                    }
                }
            });
        }
        _ => {}
    }
    ui.separator();
    if let Some(layer) = c.layer.and_then(|id| app.session.map.layers.iter().find(|l| l.id == id)) {
        if layer.data.features.len() > MAX_FEATURES {
            ui.label(egui::RichText::new(format!("Charts draw up to {MAX_FEATURES} features.")).color(t.muted));
        } else {
            let selected = layer.selection.clone();
            let id = layer.id;
            let column = |name: &str| -> Vec<f64> {
                let i = layer.data.field_index(name);
                layer.data.features.iter().map(|f| i.and_then(|i| f.values.get(i)).and_then(Value::as_f64).unwrap_or(f64::NAN)).collect()
            };
            let (x, y) = (column(&c.x), column(&c.y));
            let weights = app.session.map.weights.iter().find(|w| w.name == c.weights).map(|w| w.neighbours.clone());
            let picked = match c.kind {
                Chart::Histogram => histogram(ui, &t, &mut c, &x, &selected),
                Chart::BoxPlot => box_plot(ui, &t, &mut c, &x, &selected),
                Chart::Scatter => scatter(ui, &t, &mut c, &x, &y, &selected, None),
                Chart::MoranScatter => moran(ui, &t, &mut c, &x, weights.as_deref(), &selected),
            };
            if let Some(rows) = picked {
                let mode = if ui.input(|i| i.modifiers.shift) { "add" } else { "new" };
                app.run("feature.select", json!({ "layer": id, "rows": rows, "mode": mode }));
            }
            ui.label(
                egui::RichText::new("Drag across the chart, or click a bar or a point, to select on the map. Shift adds.").size(11.0).color(t.muted),
            );
        }
    }
    app.charts = c;
}

/// Layers with a numeric field to chart; the active one first when it has one.
fn layer_combo(app: &MapApp, ui: &mut Ui, layer: &mut Option<u32>) {
    let layers: Vec<(u32, String)> =
        app.session.map.layers.iter().filter(|l| l.data.fields.iter().any(|f| f.kind.is_numeric())).map(|l| (l.id, l.name.clone())).collect();
    if layer.is_none_or(|v| !layers.iter().any(|(i, _)| *i == v)) {
        *layer = app.active.filter(|a| layers.iter().any(|(i, _)| i == a)).or_else(|| layers.first().map(|(i, _)| *i));
    }
    if layers.is_empty() {
        ui.label(egui::RichText::new("No layer has a numeric field to chart.").size(11.0).color(Tokens::get(ui.ctx()).muted));
        return;
    }
    ui.label("Layer");
    let shown = layers.iter().find(|(i, _)| Some(*i) == *layer).map(|(_, n)| n.clone()).unwrap_or_else(|| "—".into());
    egui::ComboBox::from_id_salt("chart-layer").width(ui.available_width()).selected_text(shown).show_ui(ui, |ui| {
        for (i, n) in &layers {
            ui.selectable_value(layer, Some(*i), n);
        }
    });
}

fn field_combo(ui: &mut Ui, label: &str, id: &str, fields: &[String], value: &mut String) {
    if !fields.contains(value) {
        *value = fields.first().cloned().unwrap_or_default();
    }
    ui.label(label);
    egui::ComboBox::from_id_salt(id).width(ui.available_width()).selected_text(value.as_str()).show_ui(ui, |ui| {
        for f in fields {
            ui.selectable_value(value, f.clone(), f);
        }
    });
}

/// The plot's area and its response, with room for the axes.
fn plot_area(ui: &mut Ui, c: &mut ChartState) -> (Rect, egui::Response) {
    let (outer, response) = ui.allocate_exact_size(vec2(ui.available_width(), PLOT_HEIGHT), Sense::click_and_drag());
    let rect = Rect::from_min_max(outer.min + vec2(MARGIN + 8.0, 8.0), outer.max - vec2(8.0, MARGIN));
    c.plot = Some(rect);
    (rect, response)
}

/// A finished drag across the plot, as the rectangle it covered.
fn brushed(c: &mut ChartState, response: &egui::Response, ui: &Ui, t: &Tokens) -> Option<Rect> {
    if response.drag_started() {
        c.brush = response.interact_pointer_pos();
    }
    let (start, now) = (c.brush?, response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.latest_pos()))?);
    let rect = Rect::from_two_pos(start, now);
    ui.painter().rect(rect, 0.0, t.accent.gamma_multiply(0.12), Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    if response.drag_stopped() {
        c.brush = None;
        return Some(rect);
    }
    None
}

/// The axes, with the x range and label, and the y range unless the chart has no y scale.
fn axes(ui: &Ui, t: &Tokens, f: &Frame, x_label: &str, y_scale: bool) {
    let p = ui.painter();
    let stroke = Stroke::new(1.0, t.border);
    p.line_segment([f.rect.left_bottom(), f.rect.right_bottom()], stroke);
    p.line_segment([f.rect.left_bottom(), f.rect.left_top()], stroke);
    let font = egui::FontId::proportional(10.0);
    let num = short;
    p.text(f.rect.left_bottom() + vec2(0.0, 3.0), egui::Align2::LEFT_TOP, num(f.x.0), font.clone(), t.muted);
    p.text(f.rect.right_bottom() + vec2(0.0, 3.0), egui::Align2::RIGHT_TOP, num(f.x.1), font.clone(), t.muted);
    p.text(f.rect.center_bottom() + vec2(0.0, 3.0), egui::Align2::CENTER_TOP, x_label, font.clone(), t.text);
    if y_scale {
        p.text(f.rect.left_bottom() - vec2(3.0, 0.0), egui::Align2::RIGHT_BOTTOM, num(f.y.0), font.clone(), t.muted);
        p.text(f.rect.left_top() - vec2(3.0, 0.0), egui::Align2::RIGHT_TOP, num(f.y.1), font, t.muted);
    }
}

/// An axis number in a few characters: four significant digits, or exponent notation.
fn short(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return if x == 0.0 { "0".into() } else { x.to_string() };
    }
    if x.abs() >= 1e5 || x.abs() < 1e-3 {
        return format!("{x:.2e}");
    }
    let decimals = (3 - x.abs().log10().floor() as i32).clamp(0, 6) as usize;
    let s = format!("{x:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

fn histogram(ui: &mut Ui, t: &Tokens, c: &mut ChartState, x: &[f64], selected: &BTreeSet<usize>) -> Option<Vec<usize>> {
    let rule = Bins::parse(&c.bins).unwrap_or(Bins::Sturges);
    let h = match explore::histogram(x, rule) {
        Ok(h) => h,
        Err(e) => {
            ui.label(egui::RichText::new(e.to_string()).color(t.muted));
            return None;
        }
    };
    let mut chosen = vec![0usize; h.counts.len()];
    for (row, v) in x.iter().enumerate() {
        if selected.contains(&row)
            && let Some(n) = h.bin_of(*v).and_then(|b| chosen.get_mut(b))
        {
            *n += 1;
        }
    }
    let (rect, response) = plot_area(ui, c);
    let top = h.counts.iter().copied().max().unwrap_or(1) as f64;
    let (lo, hi) = (h.edges.first().copied().unwrap_or(0.0), h.edges.last().copied().unwrap_or(1.0));
    let f = Frame::new(rect, (lo, hi), (0.0, top));
    let p = ui.painter();
    for (b, (count, sel)) in h.counts.iter().zip(&chosen).enumerate() {
        let (a, z) = (h.edges.get(b).copied().unwrap_or(lo), h.edges.get(b + 1).copied().unwrap_or(hi));
        let bar = Rect::from_min_max(f.at(a, *count as f64), f.at(z, 0.0)).shrink2(vec2(0.5, 0.0));
        p.rect_filled(bar, 0.0, t.accent_soft);
        p.rect_stroke(bar, 0.0, Stroke::new(0.5, t.accent.gamma_multiply(0.5)), egui::StrokeKind::Inside);
        if *sel > 0 {
            p.rect_filled(Rect::from_min_max(f.at(a, *sel as f64), f.at(z, 0.0)).shrink2(vec2(0.5, 0.0)), 0.0, t.accent);
        }
    }
    axes(ui, t, &f, &c.x, true);
    ui.label(egui::RichText::new(format!("{} bins, {} features", h.counts.len(), h.counts.iter().sum::<usize>())).size(11.0).color(t.muted));
    let rows_in = |a: f64, z: f64| -> Vec<usize> {
        let bins: Vec<usize> =
            (0..h.counts.len()).filter(|b| h.edges.get(b + 1).is_some_and(|e| *e >= a) && h.edges.get(*b).is_some_and(|e| *e <= z)).collect();
        x.iter().enumerate().filter(|(_, v)| h.bin_of(**v).is_some_and(|b| bins.contains(&b))).map(|(r, _)| r).collect()
    };
    if let Some(r) = brushed(c, &response, ui, t) {
        return Some(rows_in(f.x_of(r.left()), f.x_of(r.right())));
    }
    if response.clicked() {
        let at = response.interact_pointer_pos()?;
        let v = f.x_of(at.x);
        let b = h.bin_of(v)?;
        let (a, z) = (h.edges.get(b).copied()?, h.edges.get(b + 1).copied()?);
        let mid = (a + z) / 2.0;
        return Some(rows_in(mid, mid));
    }
    None
}

fn box_plot(ui: &mut Ui, t: &Tokens, c: &mut ChartState, x: &[f64], selected: &BTreeSet<usize>) -> Option<Vec<usize>> {
    let b = match explore::box_plot(x, c.hinge) {
        Ok(b) => b,
        Err(e) => {
            ui.label(egui::RichText::new(e.to_string()).color(t.muted));
            return None;
        }
    };
    let finite = x.iter().copied().filter(|v| v.is_finite());
    let (lo, hi) = (finite.clone().fold(f64::INFINITY, f64::min), finite.fold(f64::NEG_INFINITY, f64::max));
    let (rect, response) = plot_area(ui, c);
    let f = Frame::new(rect, (lo, hi), (0.0, 1.0));
    let p = ui.painter();
    let stroke = Stroke::new(1.2, t.text);
    let (y0, y1, ym) = (0.3, 0.7, 0.5);
    p.rect(Rect::from_two_pos(f.at(b.q1, y0), f.at(b.q3, y1)), 0.0, t.accent_soft, stroke, egui::StrokeKind::Inside);
    p.line_segment([f.at(b.median, y0), f.at(b.median, y1)], Stroke::new(2.0, t.text));
    p.line_segment([f.at(b.lower_whisker, ym), f.at(b.q1, ym)], stroke);
    p.line_segment([f.at(b.q3, ym), f.at(b.upper_whisker, ym)], stroke);
    for w in [b.lower_whisker, b.upper_whisker] {
        p.line_segment([f.at(w, 0.4), f.at(w, 0.6)], stroke);
    }
    for &row in &b.outliers {
        if let Some(v) = x.get(row) {
            p.circle_stroke(f.at(*v, ym), 3.0, Stroke::new(1.2, t.text));
        }
    }
    // Selected features as ticks below the box.
    for row in selected {
        if let Some(v) = x.get(*row).filter(|v| v.is_finite()) {
            p.line_segment([f.at(*v, 0.12), f.at(*v, 0.24)], Stroke::new(1.5, t.accent));
        }
    }
    p.circle_filled(f.at(b.mean, ym), 2.5, t.accent);
    axes(ui, t, &f, &c.x, false);
    ui.label(
        egui::RichText::new(format!(
            "Q1 {} · median {} · Q3 {} · mean (dot) {} · {} outliers beyond {} × IQR",
            short(b.q1),
            short(b.median),
            short(b.q3),
            short(b.mean),
            b.outliers.len(),
            b.hinge
        ))
        .size(11.0)
        .color(t.muted),
    );
    if let Some(r) = brushed(c, &response, ui, t) {
        let (a, z) = (f.x_of(r.left()), f.x_of(r.right()));
        return Some(x.iter().enumerate().filter(|(_, v)| **v >= a && **v <= z).map(|(i, _)| i).collect());
    }
    if response.clicked() {
        let at = response.interact_pointer_pos()?;
        let near = b.outliers.iter().copied().find(|row| x.get(*row).is_some_and(|v| f.at(*v, ym).distance(at) <= PICK));
        return Some(near.into_iter().collect());
    }
    None
}

/// A scatter plot of y on x with its least-squares line; with `moran_slope`, the Moran scatter
/// plot's axes and its slope through the origin instead.
fn scatter(
    ui: &mut Ui,
    t: &Tokens,
    c: &mut ChartState,
    x: &[f64],
    y: &[f64],
    selected: &BTreeSet<usize>,
    moran_slope: Option<f64>,
) -> Option<Vec<usize>> {
    let points: Vec<(usize, f64, f64)> =
        x.iter().zip(y).enumerate().filter(|(_, (a, b))| a.is_finite() && b.is_finite()).map(|(i, (a, b))| (i, *a, *b)).collect();
    if points.len() < 2 {
        ui.label(egui::RichText::new("Needs two features with both values.").color(t.muted));
        return None;
    }
    let range = |v: &mut dyn Iterator<Item = f64>| v.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
    let (mut xr, mut yr) = (range(&mut points.iter().map(|p| p.1)), range(&mut points.iter().map(|p| p.2)));
    if moran_slope.is_some() {
        // Symmetric around 0, so the quadrants are the same size.
        let m = xr.0.abs().max(xr.1.abs()).max(yr.0.abs()).max(yr.1.abs());
        (xr, yr) = ((-m, m), (-m, m));
    }
    let (rect, response) = plot_area(ui, c);
    let f = Frame::new(rect, xr, yr);
    let p = ui.painter();
    if moran_slope.is_some() {
        let grid = Stroke::new(1.0, t.border);
        p.line_segment([f.at(0.0, f.y.0), f.at(0.0, f.y.1)], grid);
        p.line_segment([f.at(f.x.0, 0.0), f.at(f.x.1, 0.0)], grid);
        let font = egui::FontId::proportional(10.0);
        for (label, qx, qy, align) in [
            ("High-High", f.x.1, f.y.1, egui::Align2::RIGHT_TOP),
            ("Low-High", f.x.0, f.y.1, egui::Align2::LEFT_TOP),
            ("Low-Low", f.x.0, f.y.0, egui::Align2::LEFT_BOTTOM),
            ("High-Low", f.x.1, f.y.0, egui::Align2::RIGHT_BOTTOM),
        ] {
            p.text(f.at(qx, qy), align, label, font.clone(), t.muted);
        }
    }
    for (row, a, b) in &points {
        let (color, r) = if selected.contains(row) { (t.accent, 3.2) } else { (t.text.gamma_multiply(0.55), 2.4) };
        p.circle_filled(f.at(*a, *b), r, color);
    }
    let line = |slope: f64, intercept: f64| {
        let (a, z) = f.x;
        [f.at(a, intercept + slope * a), f.at(z, intercept + slope * z)]
    };
    let caption = match moran_slope {
        Some(slope) => {
            p.with_clip_rect(f.rect).line_segment(line(slope, 0.0), Stroke::new(1.5, Color32::from_rgb(0xc8, 0x33, 0x3a)));
            format!("slope {} (Moran's I with row-standardised weights) · y: spatial lag Wz", short(slope))
        }
        None => match explore::ols(x, y) {
            Ok(fit) => {
                p.with_clip_rect(f.rect).line_segment(line(fit.slope, fit.intercept), Stroke::new(1.5, t.accent));
                let r4 = short;
                format!("y = {} + {}·x · r = {} · R² = {} · n = {}", r4(fit.intercept), r4(fit.slope), r4(fit.r), r4(fit.r2), fit.n)
            }
            Err(e) => e.to_string(),
        },
    };
    axes(ui, t, &f, if moran_slope.is_some() { "z (standardised)" } else { &c.x }, true);
    ui.label(egui::RichText::new(caption).size(11.0).color(t.muted));
    if let Some(r) = brushed(c, &response, ui, t) {
        return Some(points.iter().filter(|(_, a, b)| r.contains(f.at(*a, *b))).map(|(i, _, _)| *i).collect());
    }
    if response.clicked() {
        let at = response.interact_pointer_pos()?;
        let near = points.iter().map(|(i, a, b)| (*i, f.at(*a, *b).distance(at))).filter(|(_, d)| *d <= PICK).min_by(|a, b| a.1.total_cmp(&b.1));
        return Some(near.map(|(i, _)| i).into_iter().collect());
    }
    None
}

fn moran(
    ui: &mut Ui,
    t: &Tokens,
    c: &mut ChartState,
    x: &[f64],
    weights: Option<&[Vec<(u32, f64)>]>,
    selected: &BTreeSet<usize>,
) -> Option<Vec<usize>> {
    let w = weights?;
    match explore::moran_scatter(x, w) {
        Ok(m) => scatter(ui, t, c, &m.z, &m.lag, selected, Some(m.slope)),
        Err(e) => {
            ui.label(egui::RichText::new(e.to_string()).color(t.muted));
            None
        }
    }
}
