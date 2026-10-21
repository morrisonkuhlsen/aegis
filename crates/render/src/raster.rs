//! A small software rasteriser: anti-aliased polygon fill (even-odd for polygons with holes,
//! non-zero for strokes) by sub-scanline coverage, strokes as quads with round joins, and
//! markers. Straight-alpha sRGB, blended "over". Labels are not drawn yet (no font here).

use aegis_core::{Color, Error, Result};
use aegis_model::Marker;

use crate::Shape;

/// Sub-scanlines per pixel row.
const SUB: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    EvenOdd,
    NonZero,
}

/// An RGBA image being drawn on.
pub struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, background: Color) -> Result<Canvas> {
        let (w, h) = (width as usize, height as usize);
        let n = w.checked_mul(h).filter(|n| *n <= aegis_core::MAX_ITEMS).ok_or_else(|| Error::param("size", "image too large"))?;
        let mut pixels = Vec::with_capacity(n * 4);
        for _ in 0..n {
            pixels.extend_from_slice(&[background.r, background.g, background.b, background.a]);
        }
        Ok(Canvas { width: w, height: h, pixels })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// The RGBA bytes, row by row.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixel(&self, x: usize, y: usize) -> Option<[u8; 4]> {
        let i = (y.checked_mul(self.width)?.checked_add(x)?).checked_mul(4)?;
        let p = self.pixels.get(i..i + 4)?;
        Some([*p.first()?, *p.get(1)?, *p.get(2)?, *p.get(3)?])
    }

    fn blend(&mut self, x: usize, y: usize, c: Color, coverage: f32) {
        let a = f32::from(c.a) / 255.0 * coverage.clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let i = (y * self.width + x) * 4;
        let Some(px) = self.pixels.get_mut(i..i + 4) else { return };
        let da = f32::from(px[3]) / 255.0;
        let out_a = a + da * (1.0 - a);
        for (k, s) in [c.r, c.g, c.b].into_iter().enumerate() {
            let d = f32::from(px[k]);
            let v = if out_a > 0.0 { (f32::from(s) * a + d * da * (1.0 - a)) / out_a } else { 0.0 };
            px[k] = v.round().clamp(0.0, 255.0) as u8;
        }
        px[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    }

    /// Fills the area enclosed by `rings` (closed implicitly) with `c`.
    fn fill(&mut self, rings: &[Vec<[f32; 2]>], c: Color, rule: Rule) {
        if c.a == 0 {
            return;
        }
        let edges: Vec<([f32; 2], [f32; 2])> = rings
            .iter()
            .flat_map(|r| r.iter().zip(r.iter().cycle().skip(1)).take(r.len()).map(|(a, b)| (*a, *b)))
            .filter(|(a, b)| a[1] != b[1] && a.iter().chain(b).all(|v| v.is_finite()))
            .collect();
        if edges.is_empty() {
            return;
        }
        let min_y = edges.iter().map(|(a, b)| a[1].min(b[1])).fold(f32::INFINITY, f32::min).max(0.0);
        let max_y = edges.iter().map(|(a, b)| a[1].max(b[1])).fold(f32::NEG_INFINITY, f32::max).min(self.height as f32);
        if min_y >= max_y {
            return;
        }
        let w = self.width;
        let mut coverage = vec![0f32; w + 1];
        let mut xs: Vec<(f32, i32)> = Vec::new();
        for y in (min_y.floor() as usize)..(max_y.ceil() as usize).min(self.height) {
            coverage.iter_mut().for_each(|v| *v = 0.0);
            let (mut lo, mut hi) = (w, 0usize);
            for s in 0..SUB {
                let sy = y as f32 + (s as f32 + 0.5) / SUB as f32;
                xs.clear();
                for (a, b) in &edges {
                    if (a[1] <= sy) != (b[1] <= sy) {
                        let t = (sy - a[1]) / (b[1] - a[1]);
                        xs.push((a[0] + t * (b[0] - a[0]), if b[1] > a[1] { 1 } else { -1 }));
                    }
                }
                xs.sort_by(|p, q| p.0.total_cmp(&q.0));
                let mut winding = 0;
                for pair in xs.windows(2) {
                    let [(x0, d0), (x1, _)] = pair else { continue };
                    winding += d0;
                    let inside = match rule {
                        Rule::NonZero => winding != 0,
                        Rule::EvenOdd => winding % 2 != 0,
                    };
                    if !inside {
                        continue;
                    }
                    let (x0, x1) = (x0.clamp(0.0, w as f32), x1.clamp(0.0, w as f32));
                    if x1 <= x0 {
                        continue;
                    }
                    // Exact horizontal coverage of [x0, x1) per pixel.
                    let (i0, i1) = (x0.floor() as usize, (x1.ceil() as usize).min(w));
                    lo = lo.min(i0);
                    hi = hi.max(i1);
                    for (i, cov) in coverage.iter_mut().enumerate().take(i1).skip(i0) {
                        let (p0, p1) = (i as f32, i as f32 + 1.0);
                        *cov += (x1.min(p1) - x0.max(p0)).max(0.0) / SUB as f32;
                    }
                }
            }
            for x in lo..hi.min(w) {
                let cov = coverage.get(x).copied().unwrap_or(0.0);
                if cov > 0.0 {
                    self.blend(x, y, c, cov);
                }
            }
        }
    }

    /// Strokes a path `width` pixels wide (at least a hairline), round joins.
    fn stroke(&mut self, path: &[[f32; 2]], closed: bool, c: Color, width: f32) {
        if c.a == 0 || path.len() < 2 {
            return;
        }
        let hw = width.max(0.6) / 2.0;
        let mut quads: Vec<Vec<[f32; 2]>> = Vec::new();
        let segs = path.windows(2).map(|w| (w[0], w[1])).chain(if closed { path.last().zip(path.first()).map(|(a, b)| (*a, *b)) } else { None });
        for (a, b) in segs {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = dx.hypot(dy);
            if len == 0.0 {
                continue;
            }
            let (nx, ny) = (-dy / len * hw, dx / len * hw);
            quads.push(vec![[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]]);
        }
        if hw > 0.75 {
            for p in path {
                quads.push(circle(*p, hw, 12));
            }
        }
        // Every quad and disc wound the same way: non-zero fills their union once.
        for q in &mut quads {
            if signed_area(q) < 0.0 {
                q.reverse();
            }
        }
        self.fill(&quads, c, Rule::NonZero);
    }

    pub fn draw(&mut self, s: &Shape) {
        match s {
            Shape::Polygon { rings, fill, stroke, width } => {
                self.fill(rings, *fill, Rule::EvenOdd);
                for r in rings {
                    self.stroke(r, true, *stroke, *width);
                }
            }
            Shape::Line { path, color, width } => self.stroke(path, false, *color, *width),
            Shape::Marker { at, marker, size, fill, stroke, width } => {
                let outline = marker_outline(*marker, *at, *size);
                self.fill(std::slice::from_ref(&outline), *fill, Rule::NonZero);
                self.stroke(&outline, true, *stroke, *width);
            }
            Shape::Label { .. } => {}
        }
    }

    pub fn png(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        {
            let (w, h) = (u32::try_from(self.width).unwrap_or(0), u32::try_from(self.height).unwrap_or(0));
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().map_err(|e| Error::Io(e.to_string()))?;
            writer.write_image_data(&self.pixels).map_err(|e| Error::Io(e.to_string()))?;
        }
        Ok(out)
    }
}

fn signed_area(r: &[[f32; 2]]) -> f32 {
    r.iter().zip(r.iter().cycle().skip(1)).take(r.len()).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum()
}

fn circle(c: [f32; 2], r: f32, n: usize) -> Vec<[f32; 2]> {
    (0..n)
        .map(|i| {
            let t = std::f32::consts::TAU * i as f32 / n as f32;
            [c[0] + r * t.cos(), c[1] + r * t.sin()]
        })
        .collect()
}

/// A marker's outline, `size` pixels across, centred on `at`.
pub fn marker_outline(marker: Marker, at: [f32; 2], size: f32) -> Vec<[f32; 2]> {
    let r = size.max(1.0) / 2.0;
    let [x, y] = at;
    match marker {
        Marker::Circle => circle(at, r, 20),
        Marker::Square => vec![[x - r, y - r], [x + r, y - r], [x + r, y + r], [x - r, y + r]],
        Marker::Triangle => vec![[x, y - r * 1.15], [x + r * 1.1, y + r * 0.75], [x - r * 1.1, y + r * 0.75]],
        Marker::Diamond => vec![[x, y - r * 1.2], [x + r, y], [x, y + r * 1.2], [x - r, y]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_with_holes_and_antialiases_edges() {
        let mut c = Canvas::new(20, 20, Color::WHITE).unwrap();
        let outer = vec![[2.0, 2.0], [18.0, 2.0], [18.0, 18.0], [2.0, 18.0]];
        let hole = vec![[8.0, 8.0], [12.0, 8.0], [12.0, 12.0], [8.0, 12.0]];
        c.fill(&[outer, hole], Color::BLACK, Rule::EvenOdd);
        assert_eq!(c.pixel(4, 4), Some([0, 0, 0, 255]));
        assert_eq!(c.pixel(10, 10), Some([255, 255, 255, 255]));
        assert_eq!(c.pixel(0, 0), Some([255, 255, 255, 255]));
        // A half-covered pixel is grey.
        let mut d = Canvas::new(4, 4, Color::WHITE).unwrap();
        d.fill(&[vec![[0.0, 0.0], [1.5, 0.0], [1.5, 4.0], [0.0, 4.0]]], Color::BLACK, Rule::EvenOdd);
        let [g, ..] = d.pixel(1, 1).unwrap();
        assert!((120..=135).contains(&g), "{g}");
    }

    #[test]
    fn strokes_and_hostile_coordinates_never_panic() {
        let mut c = Canvas::new(10, 10, Color::WHITE).unwrap();
        c.stroke(&[[0.0, 5.0], [10.0, 5.0]], false, Color::BLACK, 2.0);
        assert_eq!(c.pixel(5, 5), Some([0, 0, 0, 255]));
        c.stroke(&[[f32::NAN, 1.0], [f32::INFINITY, -1e30], [1e30, 1e30]], true, Color::BLACK, 1e9);
        c.fill(&[vec![[-1e30, -1e30], [1e30, 0.0], [0.0, 1e30]]], Color::BLACK, Rule::NonZero);
        assert!(Canvas::new(100_000, 100_000, Color::WHITE).is_err());
    }
}
