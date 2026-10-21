//! Symbology: symbols, the renderers that pick a symbol per feature (single symbol, unique
//! values, graduated colours), classification (equal interval, quantile) and labels. Every
//! colour and ramp here is mkaeGIS's own.

use mkaegis_core::{Color, Error, Result, Value};
use mkaegis_geom::GeometryType;
use serde::{Deserialize, Serialize};

use crate::FeatureClass;

/// The most classes a renderer may have.
pub const MAX_CLASSES: usize = 256;

/// mkaeGIS's default fills, cycled by layer id: muted, distinct, readable under black labels.
const DEFAULT_FILLS: [Color; 8] = [
    Color::rgb(0xe8, 0xc5, 0x8f),
    Color::rgb(0x9f, 0xc9, 0xa4),
    Color::rgb(0xa9, 0xc4, 0xe0),
    Color::rgb(0xd9, 0xa7, 0xb5),
    Color::rgb(0xc8, 0xb8, 0xe0),
    Color::rgb(0xe6, 0xd8, 0x8c),
    Color::rgb(0x9c, 0xd3, 0xcf),
    Color::rgb(0xd8, 0xb0, 0x94),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Marker {
    #[default]
    Circle,
    Square,
    Triangle,
    Diamond,
}

impl Marker {
    pub fn parse(text: &str) -> Option<Marker> {
        match text.to_ascii_lowercase().as_str() {
            "circle" => Some(Marker::Circle),
            "square" => Some(Marker::Square),
            "triangle" => Some(Marker::Triangle),
            "diamond" => Some(Marker::Diamond),
            _ => None,
        }
    }
}

/// How one feature is drawn. Points use `marker` and `size` (points, as pixels at 96 dpi);
/// lines use `outline` and `width`; polygons fill with `fill` and stroke with `outline`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Symbol {
    pub fill: Color,
    pub outline: Color,
    pub width: f32,
    pub size: f32,
    pub marker: Marker,
}

impl Symbol {
    /// The symbol a new layer starts with.
    pub fn default_for(kind: GeometryType, id: u32) -> Symbol {
        let fill = DEFAULT_FILLS.get(id as usize % DEFAULT_FILLS.len()).copied().unwrap_or(Color::WHITE);
        match kind {
            GeometryType::Polygon => Symbol { fill, outline: Color::rgb(0x6e, 0x6e, 0x6e), width: 0.7, size: 6.0, marker: Marker::Circle },
            GeometryType::Polyline => Symbol { fill, outline: darken(fill, 0.45), width: 1.5, size: 6.0, marker: Marker::Circle },
            GeometryType::Point | GeometryType::Multipoint => {
                Symbol { fill: darken(fill, 0.25), outline: Color::rgb(0x30, 0x30, 0x30), width: 0.7, size: 7.0, marker: Marker::Circle }
            }
        }
    }

    pub fn with_fill(self, fill: Color) -> Symbol {
        Symbol { fill, ..self }
    }
}

fn darken(c: Color, by: f32) -> Color {
    let f = |v: u8| (f32::from(v) * (1.0 - by)).round().clamp(0.0, 255.0) as u8;
    Color::rgba(f(c.r), f(c.g), f(c.b), c.a)
}

fn lerp(a: Color, b: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color::rgba(f(a.r, b.r), f(a.g, b.g), f(a.b, b.b), f(a.a, b.a))
}

/// A colour ramp: stops evenly spaced from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Ramp {
    /// Pale yellow to deep teal.
    #[default]
    Meadow,
    /// Cream to brick red.
    Ember,
    /// Pale blue to navy.
    Depth,
    /// Teal through cream to brick: diverging.
    Balance,
}

impl Ramp {
    pub fn parse(text: &str) -> Option<Ramp> {
        match text.to_ascii_lowercase().as_str() {
            "meadow" => Some(Ramp::Meadow),
            "ember" => Some(Ramp::Ember),
            "depth" => Some(Ramp::Depth),
            "balance" => Some(Ramp::Balance),
            _ => None,
        }
    }

    fn stops(self) -> &'static [Color] {
        const MEADOW: [Color; 3] = [Color::rgb(0xf7, 0xf4, 0xc8), Color::rgb(0x8f, 0xcf, 0x9a), Color::rgb(0x1f, 0x6f, 0x78)];
        const EMBER: [Color; 3] = [Color::rgb(0xfb, 0xf0, 0xd9), Color::rgb(0xf0, 0xa0, 0x62), Color::rgb(0x9a, 0x2a, 0x1e)];
        const DEPTH: [Color; 3] = [Color::rgb(0xe4, 0xef, 0xf7), Color::rgb(0x6f, 0xa8, 0xd2), Color::rgb(0x1b, 0x2f, 0x6b)];
        const BALANCE: [Color; 3] = [Color::rgb(0x1f, 0x6f, 0x78), Color::rgb(0xf4, 0xef, 0xdc), Color::rgb(0x9a, 0x2a, 0x1e)];
        match self {
            Ramp::Meadow => &MEADOW,
            Ramp::Ember => &EMBER,
            Ramp::Depth => &DEPTH,
            Ramp::Balance => &BALANCE,
        }
    }

    /// The colour at `t` in 0..=1.
    pub fn at(self, t: f64) -> Color {
        let stops = self.stops();
        let segments = stops.len().saturating_sub(1).max(1);
        let pos = t.clamp(0.0, 1.0) * segments as f64;
        let i = (pos.floor() as usize).min(segments - 1);
        match (stops.get(i), stops.get(i + 1)) {
            (Some(a), Some(b)) => lerp(*a, *b, pos - i as f64),
            (Some(a), None) => *a,
            _ => Color::WHITE,
        }
    }

    /// `n` colours from one end to the other.
    pub fn colors(self, n: usize) -> Vec<Color> {
        (0..n).map(|i| self.at(if n <= 1 { 0.5 } else { i as f64 / (n - 1) as f64 })).collect()
    }
}

/// Distinct colours for categories: hues a golden angle apart, soft saturation.
pub fn categorical(i: usize) -> Color {
    let h = (i as f64 * 137.507_764).rem_euclid(360.0);
    let (s, l) = (0.48, 0.70);
    let c = (1.0 - (2.0 * l - 1.0f64).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let f = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color::rgb(f(r), f(g), f(b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Classification {
    #[default]
    EqualInterval,
    Quantile,
}

impl Classification {
    pub fn parse(text: &str) -> Option<Classification> {
        match text.to_ascii_lowercase().replace([' ', '_'], "").as_str() {
            "" | "equalinterval" => Some(Classification::EqualInterval),
            "quantile" => Some(Classification::Quantile),
            _ => None,
        }
    }
}

/// The upper bound of each of `k` classes of `values` (finite ones only); the last is the maximum.
pub fn class_breaks(values: &[f64], k: usize, method: Classification) -> Vec<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() || k == 0 {
        return Vec::new();
    }
    v.sort_by(f64::total_cmp);
    let k = k.min(MAX_CLASSES);
    let (min, max) = (v.first().copied().unwrap_or(0.0), v.last().copied().unwrap_or(0.0));
    let mut breaks: Vec<f64> = match method {
        Classification::EqualInterval => (1..=k).map(|i| min + (max - min) * i as f64 / k as f64).collect(),
        Classification::Quantile => (1..=k).map(|i| v.get((i * v.len()).div_ceil(k).saturating_sub(1)).copied().unwrap_or(max)).collect(),
    };
    breaks.dedup();
    if let Some(last) = breaks.last_mut() {
        *last = max;
    }
    breaks
}

/// One class of a unique-values renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UniqueClass {
    pub value: String,
    pub symbol: Symbol,
}

/// Picks each feature's symbol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Renderer {
    Single(Symbol),
    UniqueValues { field: String, classes: Vec<UniqueClass>, other: Symbol },
    GraduatedColors { field: String, breaks: Vec<f64>, symbols: Vec<Symbol>, method: Classification },
}

impl Renderer {
    /// The symbol a renderer starts from: a single renderer's, or the one for other values.
    pub fn base(&self) -> Symbol {
        match self {
            Renderer::Single(s) => *s,
            Renderer::UniqueValues { other, .. } => *other,
            Renderer::GraduatedColors { symbols, .. } => symbols.first().copied().unwrap_or(Symbol::default_for(GeometryType::Polygon, 0)),
        }
    }

    /// Unique values: a class per distinct value of `field` (sorted), up to [`MAX_CLASSES`].
    pub fn unique_values(fc: &FeatureClass, field: &str, base: Symbol) -> Result<Renderer> {
        let i = fc.field_index(field).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
        let mut values: Vec<String> = fc.features.iter().filter_map(|f| f.values.get(i)).filter(|v| !v.is_null()).map(Value::to_string).collect();
        values.sort();
        values.dedup();
        if values.len() > MAX_CLASSES {
            return Err(Error::param("field", format!("`{field}` has {} distinct values; at most {MAX_CLASSES}", values.len())));
        }
        let classes = values.into_iter().enumerate().map(|(n, value)| UniqueClass { value, symbol: base.with_fill(categorical(n)) }).collect();
        let field = fc.fields.get(i).map(|f| f.name.clone()).unwrap_or_default();
        Ok(Renderer::UniqueValues { field, classes, other: base.with_fill(Color::rgb(0xc8, 0xc8, 0xc8)) })
    }

    /// Graduated colours: `k` classes of a numeric field, coloured along `ramp`.
    pub fn graduated(fc: &FeatureClass, field: &str, k: usize, method: Classification, ramp: Ramp, base: Symbol) -> Result<Renderer> {
        let i = fc.field_index(field).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
        let values: Vec<f64> = fc.features.iter().filter_map(|f| f.values.get(i)?.as_f64()).collect();
        if values.is_empty() {
            return Err(Error::param("field", format!("`{field}` has no numeric values")));
        }
        let breaks = class_breaks(&values, k.clamp(1, MAX_CLASSES), method);
        let symbols = ramp.colors(breaks.len()).into_iter().map(|c| base.with_fill(c)).collect();
        let field = fc.fields.get(i).map(|f| f.name.clone()).unwrap_or_default();
        Ok(Renderer::GraduatedColors { field, breaks, symbols, method })
    }

    /// The symbol for row `row` of `fc`.
    pub fn symbol_for(&self, fc: &FeatureClass, row: usize) -> Symbol {
        match self {
            Renderer::Single(s) => *s,
            Renderer::UniqueValues { field, classes, other } => {
                let value = fc.field_index(field).and_then(|i| fc.value(row, i)).filter(|v| !v.is_null()).map(Value::to_string);
                value.and_then(|v| classes.iter().find(|c| c.value == v)).map_or(*other, |c| c.symbol)
            }
            Renderer::GraduatedColors { field, breaks, symbols, .. } => {
                let fallback = symbols.first().copied().unwrap_or(Symbol::default_for(fc.geometry_type, 0));
                let Some(x) = fc.field_index(field).and_then(|i| fc.value(row, i)).and_then(Value::as_f64) else {
                    return fallback.with_fill(Color::rgb(0xc8, 0xc8, 0xc8));
                };
                let class = breaks.iter().position(|b| x <= *b).unwrap_or(breaks.len().saturating_sub(1));
                symbols.get(class).copied().unwrap_or(fallback)
            }
        }
    }

    /// Legend rows: a label and its symbol.
    pub fn legend(&self) -> Vec<(String, Symbol)> {
        match self {
            Renderer::Single(s) => vec![(String::new(), *s)],
            Renderer::UniqueValues { classes, other, .. } => {
                let mut rows: Vec<(String, Symbol)> = classes.iter().map(|c| (c.value.clone(), c.symbol)).collect();
                rows.push(("<all other values>".into(), *other));
                rows
            }
            Renderer::GraduatedColors { breaks, symbols, .. } => {
                let mut low: Option<f64> = None;
                breaks
                    .iter()
                    .zip(symbols)
                    .map(|(b, s)| {
                        let label = match low {
                            None => format!("≤ {}", Value::Float(*b)),
                            Some(l) => format!("{} – {}", Value::Float(l), Value::Float(*b)),
                        };
                        low = Some(*b);
                        (label, *s)
                    })
                    .collect()
            }
        }
    }

    /// The field this renderer reads, if any.
    pub fn field(&self) -> Option<&str> {
        match self {
            Renderer::Single(_) => None,
            Renderer::UniqueValues { field, .. } | Renderer::GraduatedColors { field, .. } => Some(field),
        }
    }
}

/// A layer's labels: a field's value at each feature, in a colour and size (points).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Labels {
    pub field: String,
    pub color: Color,
    pub size: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Field, FieldType};
    use mkaegis_crs::Crs;

    fn table() -> FeatureClass {
        let mut fc =
            FeatureClass::new("t", GeometryType::Point, Crs::WGS84, vec![Field::new("K", FieldType::Text), Field::new("V", FieldType::Double)]);
        for (k, v) in [("b", 1.0), ("a", 2.0), ("b", 3.0), ("c", 10.0)] {
            fc.push(None, vec![Value::Text(k.into()), Value::Float(v)]).unwrap();
        }
        fc.push(None, vec![]).unwrap();
        fc
    }

    #[test]
    fn breaks() {
        let v = [1.0, 2.0, 3.0, 10.0, f64::NAN];
        assert_eq!(class_breaks(&v, 3, Classification::EqualInterval), vec![4.0, 7.0, 10.0]);
        assert_eq!(class_breaks(&v, 2, Classification::Quantile), vec![2.0, 10.0]);
        assert!(class_breaks(&[], 3, Classification::Quantile).is_empty());
        assert_eq!(class_breaks(&[5.0, 5.0], 4, Classification::EqualInterval), vec![5.0]);
    }

    #[test]
    fn unique_values_and_graduated_pick_symbols() {
        let fc = table();
        let base = Symbol::default_for(GeometryType::Point, 1);
        let r = Renderer::unique_values(&fc, "k", base).unwrap();
        let Renderer::UniqueValues { classes, other, .. } = &r else { panic!("not unique values") };
        assert_eq!(classes.iter().map(|c| c.value.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(r.symbol_for(&fc, 0), r.symbol_for(&fc, 2));
        assert_eq!(r.symbol_for(&fc, 4), *other);
        let g = Renderer::graduated(&fc, "V", 3, Classification::EqualInterval, Ramp::Ember, base).unwrap();
        assert_eq!(g.symbol_for(&fc, 0).fill, Ramp::Ember.at(0.0));
        assert_eq!(g.symbol_for(&fc, 3).fill, Ramp::Ember.at(1.0));
        assert_eq!(g.legend().len(), 3);
        assert!(Renderer::graduated(&fc, "K", 3, Classification::Quantile, Ramp::Meadow, base).is_err());
        assert!(Renderer::unique_values(&fc, "nope", base).is_err());
    }

    #[test]
    fn ramps_and_categories() {
        assert_eq!(Ramp::Depth.colors(1).len(), 1);
        assert_eq!(Ramp::Meadow.at(-3.0), Ramp::Meadow.at(0.0));
        assert_ne!(categorical(0), categorical(1));
    }
}
