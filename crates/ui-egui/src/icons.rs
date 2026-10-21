//! MK aeGIS's icon set, drawn in code from simple strokes: original, no vendor artwork.

use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    New,
    Open,
    Save,
    AddData,
    Explore,
    Select,
    SelectByAttributes,
    SelectByLocation,
    Clear,
    ZoomToLayer,
    FullExtent,
    Buffer,
    Project,
    Calculate,
    Statistics,
    Export,
    Table,
    Undo,
    Redo,
    Symbology,
    Labels,
    Tools,
    CoordinateSystem,
    Logo,
    Contrast,
    Search,
    Share,
    ChevronUp,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    Layers,
    Plus,
    Minus,
    History,
    /// Spatial weights: features joined to their neighbours.
    Weights,
    /// The File backstage's Home.
    House,
    Info,
    Close,
}

/// Paints `icon` in `rect` with `color` (the accent marks the icon's key part).
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32, accent: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let at = |x: f32, y: f32| pos2(c.x + (x - 0.5) * s, c.y + (y - 0.5) * s);
    let w = (s / 14.0).max(1.2);
    let line = Stroke::new(w, color);
    let hi = Stroke::new(w, accent);
    let poly = |pts: &[(f32, f32)], st: Stroke, closed: bool| {
        let pts: Vec<Pos2> = pts.iter().map(|(x, y)| at(*x, *y)).collect();
        if closed {
            p.add(egui::Shape::closed_line(pts, st));
        } else {
            p.add(egui::Shape::line(pts, st));
        }
    };
    let ring = |x: f32, y: f32, r: f32, st: Stroke| {
        p.circle_stroke(at(x, y), r * s, st);
    };
    let layer_stack = |st: Stroke| {
        poly(&[(0.15, 0.62), (0.5, 0.80), (0.85, 0.62)], st, false);
        poly(&[(0.15, 0.45), (0.5, 0.27), (0.85, 0.45), (0.5, 0.63)], st, true);
    };
    match icon {
        Icon::New => {
            poly(&[(0.25, 0.12), (0.62, 0.12), (0.78, 0.28), (0.78, 0.88), (0.25, 0.88)], line, true);
            poly(&[(0.62, 0.12), (0.62, 0.28), (0.78, 0.28)], line, false);
        }
        Icon::Open => {
            poly(&[(0.12, 0.25), (0.40, 0.25), (0.48, 0.33), (0.85, 0.33), (0.85, 0.80), (0.12, 0.80)], line, true);
            poly(&[(0.12, 0.45), (0.85, 0.45)], hi, false);
        }
        Icon::Save => {
            poly(&[(0.5, 0.12), (0.5, 0.60)], hi, false);
            poly(&[(0.32, 0.43), (0.5, 0.61), (0.68, 0.43)], hi, false);
            poly(&[(0.15, 0.60), (0.15, 0.85), (0.85, 0.85), (0.85, 0.60)], line, false);
        }
        Icon::AddData => {
            layer_stack(line);
            poly(&[(0.78, 0.08), (0.78, 0.34)], hi, false);
            poly(&[(0.65, 0.21), (0.91, 0.21)], hi, false);
        }
        Icon::Explore => {
            poly(&[(0.25, 0.12), (0.25, 0.80), (0.42, 0.64), (0.55, 0.90), (0.65, 0.85), (0.53, 0.60), (0.75, 0.60)], line, true);
        }
        Icon::Select => {
            for (a, b) in [((0.12, 0.15), (0.40, 0.15)), ((0.55, 0.15), (0.80, 0.15)), ((0.12, 0.15), (0.12, 0.45)), ((0.80, 0.15), (0.80, 0.40))] {
                poly(&[a, b], line, false);
            }
            poly(&[(0.45, 0.45), (0.45, 0.92), (0.57, 0.80), (0.66, 0.95), (0.73, 0.91), (0.64, 0.77), (0.80, 0.77)], hi, true);
        }
        Icon::SelectByAttributes => {
            p.rect_stroke(Rect::from_two_pos(at(0.12, 0.18), at(0.88, 0.82)), 1.0, line, egui::StrokeKind::Middle);
            for y in [0.39, 0.60] {
                poly(&[(0.12, y), (0.88, y)], line, false);
            }
            p.rect_filled(Rect::from_two_pos(at(0.14, 0.41), at(0.86, 0.58)), 0.0, accent.gamma_multiply(0.45));
        }
        Icon::SelectByLocation => {
            poly(&[(0.10, 0.70), (0.35, 0.50), (0.62, 0.62), (0.90, 0.45), (0.90, 0.90), (0.10, 0.90)], line, true);
            ring(0.5, 0.28, 0.14, hi);
            poly(&[(0.42, 0.38), (0.5, 0.56), (0.58, 0.38)], hi, false);
        }
        Icon::Clear => {
            p.rect_stroke(Rect::from_two_pos(at(0.12, 0.2), at(0.70, 0.75)), 1.0, line, egui::StrokeKind::Middle);
            poly(&[(0.60, 0.58), (0.90, 0.88)], hi, false);
            poly(&[(0.90, 0.58), (0.60, 0.88)], hi, false);
        }
        Icon::ZoomToLayer => {
            ring(0.42, 0.42, 0.26, line);
            poly(&[(0.61, 0.61), (0.88, 0.88)], Stroke::new(w * 1.6, color), false);
            poly(&[(0.30, 0.48), (0.42, 0.36), (0.54, 0.48)], hi, false);
        }
        Icon::FullExtent | Icon::CoordinateSystem => {
            ring(0.5, 0.5, 0.36, line);
            p.add(egui::Shape::ellipse_stroke(at(0.5, 0.5), vec2(0.15 * s, 0.36 * s), hi));
            poly(&[(0.14, 0.5), (0.86, 0.5)], line, false);
            if icon == Icon::CoordinateSystem {
                poly(&[(0.22, 0.30), (0.78, 0.30)], line, false);
                poly(&[(0.22, 0.70), (0.78, 0.70)], line, false);
            }
        }
        Icon::Buffer => {
            ring(0.5, 0.5, 0.38, hi);
            ring(0.5, 0.5, 0.24, line);
            p.circle_filled(at(0.5, 0.5), 0.07 * s, color);
        }
        Icon::Project => {
            ring(0.38, 0.5, 0.26, line);
            p.add(egui::Shape::ellipse_stroke(at(0.38, 0.5), vec2(0.10 * s, 0.26 * s), line));
            poly(&[(0.62, 0.32), (0.92, 0.32)], hi, false);
            poly(&[(0.82, 0.22), (0.92, 0.32), (0.82, 0.42)], hi, false);
        }
        Icon::Calculate => {
            p.rect_stroke(Rect::from_two_pos(at(0.2, 0.1), at(0.8, 0.9)), 2.0, line, egui::StrokeKind::Middle);
            p.rect_filled(Rect::from_two_pos(at(0.3, 0.2), at(0.7, 0.35)), 1.0, accent.gamma_multiply(0.6));
            for (x, y) in [(0.35, 0.52), (0.5, 0.52), (0.65, 0.52), (0.35, 0.72), (0.5, 0.72), (0.65, 0.72)] {
                p.circle_filled(at(x, y), 0.04 * s, color);
            }
        }
        Icon::House => {
            poly(&[(0.12, 0.48), (0.5, 0.15), (0.88, 0.48)], line, false);
            poly(&[(0.22, 0.40), (0.22, 0.86), (0.78, 0.86), (0.78, 0.40)], line, false);
            poly(&[(0.42, 0.86), (0.42, 0.62), (0.58, 0.62), (0.58, 0.86)], hi, false);
        }
        Icon::Info => {
            ring(0.5, 0.5, 0.38, line);
            poly(&[(0.5, 0.45), (0.5, 0.72)], hi, false);
            p.circle_filled(at(0.5, 0.31), 0.045 * s, accent);
        }
        Icon::Close => {
            poly(&[(0.22, 0.22), (0.78, 0.78)], line, false);
            poly(&[(0.78, 0.22), (0.22, 0.78)], line, false);
        }
        Icon::Weights => {
            let nodes = [(0.22, 0.3), (0.72, 0.22), (0.5, 0.55), (0.2, 0.78), (0.8, 0.75)];
            for (a, b) in [(0, 2), (1, 2), (3, 2), (4, 2), (0, 1), (3, 4)] {
                if let (Some(&(ax, ay)), Some(&(bx, by))) = (nodes.get(a), nodes.get(b)) {
                    poly(&[(ax, ay), (bx, by)], line, false);
                }
            }
            for (i, (x, y)) in nodes.iter().enumerate() {
                p.circle_filled(at(*x, *y), 0.08 * s, if i == 2 { accent } else { color });
            }
        }
        Icon::Statistics => {
            for (x, h) in [(0.22, 0.35), (0.42, 0.6), (0.62, 0.45), (0.82, 0.75)] {
                p.rect_filled(Rect::from_two_pos(at(x - 0.07, 0.88 - h), at(x + 0.07, 0.88)), 1.0, if x > 0.8 { accent } else { color });
            }
        }
        Icon::Export => {
            poly(&[(0.55, 0.15), (0.15, 0.15), (0.15, 0.85), (0.85, 0.85), (0.85, 0.45)], line, false);
            poly(&[(0.45, 0.55), (0.88, 0.12)], hi, false);
            poly(&[(0.62, 0.12), (0.88, 0.12), (0.88, 0.38)], hi, false);
        }
        Icon::Table => {
            p.rect_stroke(Rect::from_two_pos(at(0.1, 0.18), at(0.9, 0.82)), 1.0, line, egui::StrokeKind::Middle);
            p.rect_filled(Rect::from_two_pos(at(0.1, 0.18), at(0.9, 0.36)), 0.0, accent.gamma_multiply(0.5));
            for y in [0.36, 0.59] {
                poly(&[(0.1, y), (0.9, y)], line, false);
            }
            poly(&[(0.4, 0.18), (0.4, 0.82)], line, false);
        }
        Icon::Undo | Icon::Redo => {
            let flip = |x: f32| if icon == Icon::Redo { 1.0 - x } else { x };
            let arc: Vec<(f32, f32)> = (0..=10)
                .map(|i| {
                    let t = std::f32::consts::PI * (1.0 - i as f32 / 10.0);
                    (flip(0.5 + 0.3 * t.cos()), 0.55 - 0.28 * t.sin())
                })
                .collect();
            poly(&arc, line, false);
            poly(&[(flip(0.08), 0.42), (flip(0.2), 0.58), (flip(0.34), 0.44)], hi, false);
        }
        Icon::Symbology => {
            p.circle_filled(at(0.32, 0.35), 0.13 * s, Color32::from_rgb(0xe0, 0x9b, 0x5c));
            p.circle_filled(at(0.66, 0.35), 0.13 * s, Color32::from_rgb(0x5c, 0xa8, 0x8f));
            p.circle_filled(at(0.49, 0.66), 0.13 * s, Color32::from_rgb(0x6f, 0x8f, 0xd2));
        }
        Icon::Labels => {
            poly(&[(0.2, 0.85), (0.5, 0.15), (0.8, 0.85)], line, false);
            poly(&[(0.32, 0.6), (0.68, 0.6)], hi, false);
        }
        Icon::Logo => {
            // A map pin over a folded map: MK aeGIS's own mark.
            poly(&[(0.08, 0.80), (0.36, 0.68), (0.64, 0.80), (0.92, 0.68), (0.92, 0.95), (0.64, 0.95), (0.36, 0.83), (0.08, 0.95)], line, true);
            let pin: Vec<(f32, f32)> = (0..=20)
                .map(|i| {
                    let a = std::f32::consts::PI * (0.8 + 1.4 * i as f32 / 20.0);
                    (0.5 + 0.22 * a.cos(), 0.32 + 0.22 * a.sin())
                })
                .chain(std::iter::once((0.5, 0.78)))
                .collect();
            poly(&pin, hi, true);
            p.circle_filled(at(0.5, 0.32), 0.07 * s, accent);
        }
        Icon::Contrast => {
            ring(0.5, 0.5, 0.36, line);
            let half: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0;
                    at(0.5 + 0.36 * a.cos(), 0.5 + 0.36 * a.sin())
                })
                .collect();
            p.add(egui::Shape::convex_polygon(half, color, Stroke::NONE));
        }
        Icon::Search => {
            ring(0.42, 0.42, 0.26, line);
            poly(&[(0.61, 0.61), (0.88, 0.88)], Stroke::new(w * 1.4, color), false);
        }
        Icon::Share => {
            poly(&[(0.3, 0.42), (0.15, 0.42), (0.15, 0.88), (0.85, 0.88), (0.85, 0.42), (0.7, 0.42)], line, false);
            poly(&[(0.5, 0.62), (0.5, 0.1)], hi, false);
            poly(&[(0.33, 0.27), (0.5, 0.1), (0.67, 0.27)], hi, false);
        }
        Icon::ChevronUp => poly(&[(0.25, 0.62), (0.5, 0.37), (0.75, 0.62)], line, false),
        Icon::ChevronDown => poly(&[(0.25, 0.38), (0.5, 0.63), (0.75, 0.38)], line, false),
        Icon::ChevronLeft => poly(&[(0.62, 0.25), (0.37, 0.5), (0.62, 0.75)], line, false),
        Icon::ChevronRight => poly(&[(0.38, 0.25), (0.63, 0.5), (0.38, 0.75)], line, false),
        Icon::Layers => {
            poly(&[(0.1, 0.62), (0.5, 0.82), (0.9, 0.62)], line, false);
            poly(&[(0.1, 0.45), (0.5, 0.25), (0.9, 0.45), (0.5, 0.65)], hi, true);
        }
        Icon::Plus => {
            poly(&[(0.5, 0.2), (0.5, 0.8)], line, false);
            poly(&[(0.2, 0.5), (0.8, 0.5)], line, false);
        }
        Icon::Minus => poly(&[(0.2, 0.5), (0.8, 0.5)], line, false),
        Icon::History => {
            ring(0.5, 0.5, 0.36, line);
            poly(&[(0.5, 0.26), (0.5, 0.5), (0.68, 0.62)], hi, false);
        }
        Icon::Tools => {
            ring(0.5, 0.5, 0.2, line);
            for i in 0..8 {
                let t = std::f32::consts::TAU * i as f32 / 8.0;
                poly(&[(0.5 + 0.28 * t.cos(), 0.5 + 0.28 * t.sin()), (0.5 + 0.40 * t.cos(), 0.5 + 0.40 * t.sin())], hi, false);
            }
        }
    }
}

/// An icon-sized square for laying out next to text.
pub fn square(ui: &mut egui::Ui, size: f32, icon: Icon) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    let t = crate::theme::Tokens::get(ui.ctx());
    paint(ui.painter(), rect, icon, t.text, t.accent);
}
