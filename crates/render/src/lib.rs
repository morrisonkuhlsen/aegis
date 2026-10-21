//! Drawing a map (L2): the [`Viewport`] (what part of the map, at what resolution), the display
//! list — every layer projected on the fly into the map's CRS, culled, symbolised, with the
//! selection and labels on top — and a software rasteriser (`raster`) that turns it into PNG.
//! The UI paints the same display list with its own painter.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod raster;

use aegis_core::{Color, Result, Value};
use aegis_crs::Crs;
use aegis_geom::{Coord, Envelope, Geometry, algo};
use aegis_model::{Layer, Map, Marker, Symbol};

/// Metres per pixel at 96 dpi, for the scale (1 : n).
const METRES_PER_PIXEL: f64 = 0.0254 / 96.0;
/// Resolutions allowed (map units per pixel): keeps zooming finite.
const MIN_RESOLUTION: f64 = 1e-9;
const MAX_RESOLUTION: f64 = 1e9;

/// What part of the map a view shows: its centre, map units per pixel, and its size in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub center: Coord,
    pub resolution: f64,
    pub width: f64,
    pub height: f64,
}

impl Viewport {
    /// A view of `width`×`height` pixels that shows all of `extent`, with a margin.
    pub fn fit(extent: Envelope, width: f64, height: f64) -> Viewport {
        let (w, h) = (width.max(1.0), height.max(1.0));
        let res = (extent.width() / w).max(extent.height() / h) * 1.05;
        let res = if res.is_finite() && res > 0.0 { res } else { 1.0 };
        Viewport { center: extent.center(), resolution: res.clamp(MIN_RESOLUTION, MAX_RESOLUTION), width: w, height: h }
    }

    /// The map extent this view shows.
    pub fn extent(&self) -> Envelope {
        let (hw, hh) = (self.width * self.resolution / 2.0, self.height * self.resolution / 2.0);
        Envelope { min_x: self.center.x - hw, min_y: self.center.y - hh, max_x: self.center.x + hw, max_y: self.center.y + hh }
    }

    /// A map position in pixels (y down).
    pub fn to_screen(&self, c: Coord) -> [f32; 2] {
        [((c.x - self.center.x) / self.resolution + self.width / 2.0) as f32, ((self.center.y - c.y) / self.resolution + self.height / 2.0) as f32]
    }

    /// The map position under pixel `p`.
    pub fn to_map(&self, p: [f32; 2]) -> Coord {
        Coord::new(
            self.center.x + (f64::from(p[0]) - self.width / 2.0) * self.resolution,
            self.center.y - (f64::from(p[1]) - self.height / 2.0) * self.resolution,
        )
    }

    /// Zooms by `factor` (> 1 zooms in) keeping the map position under pixel `anchor` still.
    pub fn zoom(&mut self, factor: f64, anchor: [f32; 2]) {
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        let before = self.to_map(anchor);
        self.resolution = (self.resolution / factor).clamp(MIN_RESOLUTION, MAX_RESOLUTION);
        let after = self.to_map(anchor);
        self.center = Coord::new(self.center.x + before.x - after.x, self.center.y + before.y - after.y);
    }

    /// Moves the view by a drag of `dx`, `dy` pixels.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.center = Coord::new(self.center.x - f64::from(dx) * self.resolution, self.center.y + f64::from(dy) * self.resolution);
    }

    /// The map scale's denominator (1 : n) at the view's centre, at 96 dpi.
    pub fn scale(&self, crs: Crs) -> f64 {
        let lat = crs.to_geographic(self.center).map_or(0.0, |g| g.y);
        self.resolution * crs.metres_per_unit(lat) / METRES_PER_PIXEL
    }

    /// The resolution that gives scale 1 : `n` at the view's centre.
    pub fn set_scale(&mut self, crs: Crs, n: f64) {
        let lat = crs.to_geographic(self.center).map_or(0.0, |g| g.y);
        let res = n * METRES_PER_PIXEL / crs.metres_per_unit(lat).max(1e-12);
        if res.is_finite() && res > 0.0 {
            self.resolution = res.clamp(MIN_RESOLUTION, MAX_RESOLUTION);
        }
    }
}

/// Something to paint, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Polygon { rings: Vec<Vec<[f32; 2]>>, fill: Color, stroke: Color, width: f32 },
    Line { path: Vec<[f32; 2]>, color: Color, width: f32 },
    Marker { at: [f32; 2], marker: Marker, size: f32, fill: Color, stroke: Color, width: f32 },
    Label { at: [f32; 2], text: String, color: Color, size: f32 },
}

/// What to draw besides the layers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub selection: bool,
    pub labels: bool,
    pub graticule: bool,
    /// Pixels per point: symbol sizes are in points.
    pub pixels_per_point: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options { selection: true, labels: true, graticule: true, pixels_per_point: 1.0 }
    }
}

/// Points closer than this (pixels) to the previous one are dropped.
const PIXEL_TOLERANCE: f32 = 0.5;

fn screen_path(vp: &Viewport, path: &[Coord]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(path.len());
    for c in path {
        let p = vp.to_screen(*c);
        if !(p[0].is_finite() && p[1].is_finite()) {
            continue;
        }
        if let Some(last) = out.last()
            && (last[0] - p[0]).abs() < PIXEL_TOLERANCE
            && (last[1] - p[1]).abs() < PIXEL_TOLERANCE
        {
            continue;
        }
        out.push(p);
    }
    out
}

fn with_opacity(c: Color, opacity: u8) -> Color {
    c.with_alpha(((u16::from(c.a) * u16::from(opacity.min(100))) / 100) as u8)
}

/// The geometry in the map's CRS (`None` if it cannot be projected).
pub fn project(g: &Geometry, from: Crs, to: Crs) -> Option<Geometry> {
    if from == to { Some(g.clone()) } else { g.try_map(|c| from.transform(to, c)).ok() }
}

fn push_geometry(out: &mut Vec<Shape>, vp: &Viewport, g: &Geometry, s: &Symbol, opacity: u8, ppp: f32) {
    let (fill, stroke) = (with_opacity(s.fill, opacity), with_opacity(s.outline, opacity));
    match g {
        Geometry::Point(c) => {
            out.push(Shape::Marker { at: vp.to_screen(*c), marker: s.marker, size: s.size * ppp, fill, stroke, width: s.width * ppp })
        }
        Geometry::Multipoint(cs) => {
            for c in cs {
                out.push(Shape::Marker { at: vp.to_screen(*c), marker: s.marker, size: s.size * ppp, fill, stroke, width: s.width * ppp });
            }
        }
        Geometry::Polyline(parts) => {
            for p in parts {
                let path = screen_path(vp, p);
                if path.len() >= 2 {
                    out.push(Shape::Line { path, color: stroke, width: s.width * ppp });
                }
            }
        }
        Geometry::Polygon(rings) => {
            let rings: Vec<Vec<[f32; 2]>> = rings.iter().map(|r| screen_path(vp, r)).filter(|r| r.len() >= 3).collect();
            if !rings.is_empty() {
                out.push(Shape::Polygon { rings, fill, stroke, width: s.width * ppp });
            }
        }
    }
}

/// A selection outline over the feature.
fn selection_symbol(s: &Symbol) -> Symbol {
    Symbol { fill: Color::TRANSPARENT, outline: Color::SELECTION, width: (s.width + 1.5).max(2.0), size: s.size + 2.0, marker: s.marker }
}

/// Where a feature's label goes.
fn label_anchor(g: &Geometry) -> Option<Coord> {
    match g {
        Geometry::Point(c) => Some(*c),
        Geometry::Multipoint(cs) => cs.first().copied(),
        Geometry::Polyline(parts) => {
            let longest = parts
                .iter()
                .max_by(|a, b| algo::length(&Geometry::Polyline(vec![a.to_vec()])).total_cmp(&algo::length(&Geometry::Polyline(vec![b.to_vec()]))))?;
            longest.get(longest.len() / 2).copied()
        }
        Geometry::Polygon(_) => algo::centroid(g).filter(|c| algo::contains_point(g, *c)).or_else(|| g.coords().next()),
    }
}

/// One layer's shapes (and, for labels, its candidates).
fn draw_layer(out: &mut Vec<Shape>, labels: &mut Vec<Shape>, map: &Map, layer: &Layer, vp: &Viewport, opts: &Options) {
    let view = vp.extent();
    let mut selected = Vec::new();
    let label_field = layer.labels.as_ref().and_then(|l| layer.data.field_index(&l.field).map(|i| (i, l)));
    for (row, f) in layer.data.features.iter().enumerate() {
        let Some(g) = f.geometry.as_ref() else { continue };
        let Some(g) = project(g, layer.data.crs, map.crs) else { continue };
        if !g.envelope().is_some_and(|e| e.intersects(&view.scale(1.2))) {
            continue;
        }
        let s = layer.renderer.symbol_for(&layer.data, row);
        push_geometry(out, vp, &g, &s, layer.opacity, opts.pixels_per_point);
        if opts.selection && layer.selection.contains(&row) {
            selected.push((g.clone(), selection_symbol(&s)));
        }
        if opts.labels
            && let Some((i, l)) = label_field
            && let Some(v) = f.values.get(i).filter(|v| !v.is_null())
            && let Some(at) = label_anchor(&g)
        {
            let mut p = vp.to_screen(at);
            if matches!(g, Geometry::Point(_) | Geometry::Multipoint(_)) {
                p[0] += s.size * opts.pixels_per_point / 2.0 + 3.0;
            }
            labels.push(Shape::Label { at: p, text: label_text(v), color: l.color, size: l.size * opts.pixels_per_point });
        }
    }
    for (g, s) in selected {
        push_geometry(out, vp, &g, &s, 100, opts.pixels_per_point);
    }
}

fn label_text(v: &Value) -> String {
    let s = v.to_string();
    s.chars().take(80).collect()
}

/// Labels that would overlap one already placed are left out (first come, first placed).
fn place_labels(labels: Vec<Shape>) -> Vec<Shape> {
    let mut boxes: Vec<[f32; 4]> = Vec::new();
    let mut out = Vec::new();
    for l in labels {
        let Shape::Label { at, text, size, .. } = &l else { continue };
        let w = text.chars().count() as f32 * size * 0.56;
        let b = [at[0], at[1] - size * 0.6, at[0] + w, at[1] + size * 0.6];
        if boxes.iter().any(|o| b[0] < o[2] && o[0] < b[2] && b[1] < o[3] && o[1] < b[3]) {
            continue;
        }
        boxes.push(b);
        out.push(l);
    }
    out
}

/// A graticule step (degrees) that gives a handful of lines across `span` degrees.
fn graticule_step(span: f64) -> f64 {
    [0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0].into_iter().find(|s| span / s <= 12.0).unwrap_or(30.0)
}

/// Meridians and parallels across the view, projected into the map's CRS.
fn graticule(out: &mut Vec<Shape>, map: &Map, vp: &Viewport) {
    let Some(geo) = aegis_model::project_envelope(vp.extent(), map.crs, Crs::WGS84) else { return };
    let geo = Envelope { min_x: geo.min_x.max(-180.0), min_y: geo.min_y.max(-85.0), max_x: geo.max_x.min(180.0), max_y: geo.max_y.min(85.0) };
    if geo.width() <= 0.0 || geo.height() <= 0.0 {
        return;
    }
    let step = graticule_step(geo.width().max(geo.height()));
    let color = Color::rgba(0x80, 0x90, 0xa0, 70);
    let line = |out: &mut Vec<Shape>, pts: Vec<Coord>| {
        let projected: Vec<Coord> = pts.into_iter().filter_map(|p| Crs::WGS84.transform(map.crs, p).ok()).collect();
        let path = screen_path(vp, &projected);
        if path.len() >= 2 {
            out.push(Shape::Line { path, color, width: 0.8 });
        }
    };
    let n = 64;
    let mut x = (geo.min_x / step).ceil() * step;
    let mut guard = 0;
    while x <= geo.max_x && guard < 100 {
        line(out, (0..=n).map(|i| Coord::new(x, geo.min_y + geo.height() * f64::from(i) / f64::from(n))).collect());
        x += step;
        guard += 1;
    }
    let mut y = (geo.min_y / step).ceil() * step;
    guard = 0;
    while y <= geo.max_y && guard < 100 {
        line(out, (0..=n).map(|i| Coord::new(geo.min_x + geo.width() * f64::from(i) / f64::from(n), y)).collect());
        y += step;
        guard += 1;
    }
}

/// Everything to paint for `map` in `vp`, bottom first.
pub fn display_list(map: &Map, vp: &Viewport, opts: &Options) -> Vec<Shape> {
    let mut out = Vec::new();
    let mut labels = Vec::new();
    if opts.graticule {
        graticule(&mut out, map, vp);
    }
    for layer in map.layers.iter().rev().filter(|l| l.visible) {
        draw_layer(&mut out, &mut labels, map, layer, vp, opts);
    }
    // Top layers' labels win: they were pushed last.
    labels.reverse();
    out.extend(place_labels(labels));
    out
}

/// The feature under pixel `p`, topmost visible layer first: (layer index, row).
pub fn hit_test(map: &Map, vp: &Viewport, p: [f32; 2], tolerance_px: f64) -> Option<(usize, usize)> {
    let at = vp.to_map(p);
    let tol = tolerance_px * vp.resolution;
    for (li, layer) in map.layers.iter().enumerate().filter(|(_, l)| l.visible) {
        let mut best: Option<(f64, usize)> = None;
        for (row, f) in layer.data.features.iter().enumerate() {
            let Some(g) = f.geometry.as_ref().and_then(|g| project(g, layer.data.crs, map.crs)) else { continue };
            if !g.envelope().is_some_and(|e| e.buffer(tol).contains(at)) {
                continue;
            }
            let d = algo::distance_to(&g, at);
            if d <= tol && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, row));
            }
        }
        if let Some((_, row)) = best {
            return Some((li, row));
        }
    }
    None
}

/// The map rendered to a PNG of `width`×`height` pixels showing `extent` (or the full extent).
pub fn export_png(map: &Map, width: u32, height: u32, extent: Option<Envelope>) -> Result<Vec<u8>> {
    let (w, h) = (width.clamp(1, 16_384), height.clamp(1, 16_384));
    let extent = extent.or(map.extent).or_else(|| map.full_extent()).unwrap_or(Envelope::new(Coord::new(-180.0, -90.0), Coord::new(180.0, 90.0)));
    let vp = Viewport::fit(extent, f64::from(w), f64::from(h));
    let shapes = display_list(map, &vp, &Options { labels: false, ..Options::default() });
    let mut canvas = raster::Canvas::new(w, h, map.background)?;
    for s in &shapes {
        canvas.draw(s);
    }
    canvas.png()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aegis_geom::GeometryType;
    use aegis_model::{FeatureClass, Field, FieldType, Labels};

    fn map() -> Map {
        let mut m = Map::new("m", Crs::WEB_MERCATOR);
        let mut fc = FeatureClass::new("pts", GeometryType::Point, Crs::WGS84, vec![Field::new("N", FieldType::Text)]);
        fc.push(Some(Geometry::Point(Coord::new(-46.0, -23.0))), vec![Value::Text("a".into())]).unwrap();
        fc.push(Some(Geometry::Point(Coord::new(-45.0, -22.0))), vec![Value::Text("b".into())]).unwrap();
        m.add_layer(fc);
        m
    }

    #[test]
    fn viewport_maps_both_ways_and_zooms_about_the_anchor() {
        let mut vp = Viewport::fit(Envelope::new(Coord::new(0.0, 0.0), Coord::new(100.0, 50.0)), 200.0, 100.0);
        let p = vp.to_screen(Coord::new(10.0, 10.0));
        let back = vp.to_map(p);
        assert!((back.x - 10.0).abs() < 1e-4 && (back.y - 10.0).abs() < 1e-4);
        let before = vp.to_map([30.0, 40.0]);
        vp.zoom(2.0, [30.0, 40.0]);
        let after = vp.to_map([30.0, 40.0]);
        assert!((before.x - after.x).abs() < 1e-9 && (before.y - after.y).abs() < 1e-9);
        vp.zoom(f64::NAN, [0.0, 0.0]);
        assert!(vp.resolution.is_finite());
        let mut s = vp;
        s.set_scale(Crs::from_epsg(32723).unwrap(), 10_000.0);
        assert!((s.scale(Crs::from_epsg(32723).unwrap()) - 10_000.0).abs() < 1e-6);
    }

    #[test]
    fn display_list_projects_selects_and_labels() {
        let mut m = map();
        m.layers[0].selection.insert(1);
        m.layers[0].labels = Some(Labels { field: "N".into(), color: Color::BLACK, size: 9.0 });
        let extent = m.full_extent().unwrap().buffer(100_000.0);
        let vp = Viewport::fit(extent, 400.0, 300.0);
        let shapes = display_list(&m, &vp, &Options { graticule: false, ..Options::default() });
        let markers = shapes.iter().filter(|s| matches!(s, Shape::Marker { .. })).count();
        let labels = shapes.iter().filter(|s| matches!(s, Shape::Label { .. })).count();
        assert_eq!((markers, labels), (3, 2));
        assert_eq!(hit_test(&m, &vp, vp.to_screen(m.full_extent().unwrap().center()), 3.0), None);
        let first = Crs::WGS84.transform(Crs::WEB_MERCATOR, Coord::new(-46.0, -23.0)).unwrap();
        assert_eq!(hit_test(&m, &vp, vp.to_screen(first), 3.0), Some((0, 0)));
    }

    #[test]
    fn exports_a_png() {
        let png = export_png(&map(), 64, 48, None).unwrap();
        assert_eq!(png.get(1..4), Some(&b"PNG"[..]));
    }
}
