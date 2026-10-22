//! Planar geometry (L0): coordinates, envelopes and the four feature geometries a feature class
//! holds — point, multipoint, polyline and polygon, as in the shapefile and geodatabase model —
//! with the algorithms geoprocessing needs (`algo`) and OGC Well-Known Text (`wkt`).
//!
//! Polygons keep their rings closed (first vertex repeated last). Ring orientation is not
//! trusted: holes are found by containment (even-odd), so rings from GeoJSON (outer
//! counter-clockwise) and from shapefiles (outer clockwise) read the same.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod algo;
pub mod wkt;

use mkaegis_core::{Error, Result};
use serde::{Deserialize, Serialize};

/// Why a ring is refused.
pub const SHORT_RING: &str = "a polygon ring needs at least 3 vertices";

/// A position in the layer's coordinate system (x = easting or longitude, y = northing or latitude).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Coord {
    pub x: f64,
    pub y: f64,
}

impl Coord {
    pub const fn new(x: f64, y: f64) -> Self {
        Coord { x, y }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    pub fn distance(self, other: Coord) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

/// An axis-aligned bounding rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Envelope {
    pub fn new(a: Coord, b: Coord) -> Self {
        Envelope { min_x: a.x.min(b.x), min_y: a.y.min(b.y), max_x: a.x.max(b.x), max_y: a.y.max(b.y) }
    }

    /// The envelope of these coordinates (non-finite ones skipped), or `None` when there are none.
    pub fn of(coords: impl IntoIterator<Item = Coord>) -> Option<Envelope> {
        let mut env: Option<Envelope> = None;
        for c in coords.into_iter().filter(|c| c.is_finite()) {
            env = Some(match env {
                None => Envelope::new(c, c),
                Some(e) => e.expand_to(c),
            });
        }
        env
    }

    pub fn expand_to(self, c: Coord) -> Self {
        Envelope { min_x: self.min_x.min(c.x), min_y: self.min_y.min(c.y), max_x: self.max_x.max(c.x), max_y: self.max_y.max(c.y) }
    }

    pub fn union(self, o: Envelope) -> Self {
        Envelope { min_x: self.min_x.min(o.min_x), min_y: self.min_y.min(o.min_y), max_x: self.max_x.max(o.max_x), max_y: self.max_y.max(o.max_y) }
    }

    /// Grown by `d` on every side.
    pub fn buffer(self, d: f64) -> Self {
        Envelope { min_x: self.min_x - d, min_y: self.min_y - d, max_x: self.max_x + d, max_y: self.max_y + d }
    }

    /// Grown around its centre by `factor` (2.0 doubles width and height).
    pub fn scale(self, factor: f64) -> Self {
        let c = self.center();
        let (hw, hh) = (self.width() * factor / 2.0, self.height() * factor / 2.0);
        Envelope { min_x: c.x - hw, min_y: c.y - hh, max_x: c.x + hw, max_y: c.y + hh }
    }

    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    pub fn center(&self) -> Coord {
        Coord::new((self.min_x + self.max_x) / 2.0, (self.min_y + self.max_y) / 2.0)
    }

    pub fn contains(&self, c: Coord) -> bool {
        c.x >= self.min_x && c.x <= self.max_x && c.y >= self.min_y && c.y <= self.max_y
    }

    pub fn intersects(&self, o: &Envelope) -> bool {
        self.min_x <= o.max_x && o.min_x <= self.max_x && self.min_y <= o.max_y && o.min_y <= self.max_y
    }

    pub fn is_finite(&self) -> bool {
        self.min_x.is_finite() && self.min_y.is_finite() && self.max_x.is_finite() && self.max_y.is_finite()
    }
}

/// The kind of geometry a feature class holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeometryType {
    Point,
    Multipoint,
    Polyline,
    Polygon,
}

impl GeometryType {
    pub fn name(self) -> &'static str {
        match self {
            GeometryType::Point => "Point",
            GeometryType::Multipoint => "Multipoint",
            GeometryType::Polyline => "Polyline",
            GeometryType::Polygon => "Polygon",
        }
    }
}

/// A feature's shape. Polylines are a list of paths, polygons a list of closed rings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Geometry {
    Point(Coord),
    Multipoint(Vec<Coord>),
    Polyline(Vec<Vec<Coord>>),
    Polygon(Vec<Vec<Coord>>),
}

impl Geometry {
    pub fn geometry_type(&self) -> GeometryType {
        match self {
            Geometry::Point(_) => GeometryType::Point,
            Geometry::Multipoint(_) => GeometryType::Multipoint,
            Geometry::Polyline(_) => GeometryType::Polyline,
            Geometry::Polygon(_) => GeometryType::Polygon,
        }
    }

    /// A polygon from rings, closing any ring that is not closed. Rings need 3 distinct vertices.
    pub fn polygon(rings: Vec<Vec<Coord>>) -> Result<Geometry> {
        let closed = rings.into_iter().map(close_ring).collect::<Option<Vec<_>>>().ok_or_else(|| Error::Geometry(SHORT_RING.into()))?;
        if closed.is_empty() {
            return Err(Error::Geometry("a polygon needs at least one ring".into()));
        }
        Ok(Geometry::Polygon(closed))
    }

    /// A polygon as [`Geometry::polygon`] makes it, except that a ring with fewer than 3 vertices
    /// (it has no area) is left out instead of failing the whole shape (#57). Returns the polygon,
    /// `None` when no ring is left, and how many rings were left out.
    pub fn polygon_lenient(rings: Vec<Vec<Coord>>) -> (Option<Geometry>, usize) {
        let total = rings.len();
        let closed: Vec<Vec<Coord>> = rings.into_iter().filter_map(close_ring).collect();
        let left_out = total - closed.len();
        ((!closed.is_empty()).then_some(Geometry::Polygon(closed)), left_out)
    }

    /// Every vertex, in order.
    pub fn coords(&self) -> Box<dyn Iterator<Item = Coord> + '_> {
        match self {
            Geometry::Point(c) => Box::new(std::iter::once(*c)),
            Geometry::Multipoint(cs) => Box::new(cs.iter().copied()),
            Geometry::Polyline(parts) | Geometry::Polygon(parts) => Box::new(parts.iter().flatten().copied()),
        }
    }

    pub fn vertex_count(&self) -> usize {
        match self {
            Geometry::Point(_) => 1,
            Geometry::Multipoint(cs) => cs.len(),
            Geometry::Polyline(parts) | Geometry::Polygon(parts) => parts.iter().map(Vec::len).sum(),
        }
    }

    pub fn envelope(&self) -> Option<Envelope> {
        Envelope::of(self.coords())
    }

    /// The same shape with every vertex passed through `f`; fails if `f` fails for any.
    pub fn try_map(&self, mut f: impl FnMut(Coord) -> Result<Coord>) -> Result<Geometry> {
        let mut parts =
            |parts: &[Vec<Coord>]| -> Result<Vec<Vec<Coord>>> { parts.iter().map(|p| p.iter().map(|c| f(*c)).collect::<Result<Vec<_>>>()).collect() };
        Ok(match self {
            Geometry::Point(c) => Geometry::Point(f(*c)?),
            Geometry::Multipoint(cs) => Geometry::Multipoint(cs.iter().map(|c| f(*c)).collect::<Result<Vec<_>>>()?),
            Geometry::Polyline(p) => Geometry::Polyline(parts(p)?),
            Geometry::Polygon(p) => Geometry::Polygon(parts(p)?),
        })
    }

    /// Whether every vertex is a finite number.
    pub fn is_finite(&self) -> bool {
        self.coords().all(Coord::is_finite)
    }
}

/// The ring closed (its first vertex repeated at the end if it isn't), or `None` with fewer than
/// 3 vertices.
fn close_ring(mut ring: Vec<Coord>) -> Option<Vec<Coord>> {
    if let (Some(first), Some(last)) = (ring.first().copied(), ring.last().copied())
        && first != last
    {
        ring.push(first);
    }
    (ring.len() >= 4).then_some(ring)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_closes_rings_and_rejects_degenerate_ones() {
        let g = Geometry::polygon(vec![vec![Coord::new(0.0, 0.0), Coord::new(1.0, 0.0), Coord::new(1.0, 1.0)]]).unwrap();
        let Geometry::Polygon(rings) = &g else { panic!("not a polygon") };
        assert_eq!(rings[0].len(), 4);
        assert!(Geometry::polygon(vec![vec![Coord::new(0.0, 0.0), Coord::new(1.0, 0.0)]]).is_err());
        assert!(Geometry::polygon(vec![]).is_err());
    }

    #[test]
    fn a_lenient_polygon_leaves_out_only_the_short_rings() {
        let shell = vec![Coord::new(0.0, 0.0), Coord::new(4.0, 0.0), Coord::new(4.0, 4.0), Coord::new(0.0, 0.0)];
        let short = vec![Coord::new(1.0, 1.0), Coord::new(2.0, 1.0)];
        let (g, left_out) = Geometry::polygon_lenient(vec![shell.clone(), short.clone(), vec![]]);
        assert_eq!((g, left_out), (Some(Geometry::Polygon(vec![shell])), 2));
        assert_eq!(Geometry::polygon_lenient(vec![short]), (None, 1));
        assert_eq!(Geometry::polygon_lenient(vec![]), (None, 0));
    }

    #[test]
    fn envelopes_skip_non_finite_coords() {
        let e = Envelope::of([Coord::new(f64::NAN, 1.0), Coord::new(2.0, 3.0), Coord::new(-1.0, 5.0)]).unwrap();
        assert_eq!(e, Envelope { min_x: -1.0, min_y: 3.0, max_x: 2.0, max_y: 5.0 });
        assert!(Envelope::of([]).is_none());
    }
}
