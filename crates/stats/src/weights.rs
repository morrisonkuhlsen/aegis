//! Building spatial weights from a feature class: contiguity (queen, rook, higher orders),
//! k nearest neighbours, distance bands (optionally inverse distance) and kernels, then the
//! standardisation. Distances are geodesic, in metres, on the CRS's ellipsoid, between
//! features' centroids (points: the point itself).
//!
//! Contiguity matches vertices within a tolerance: two polygons are queen neighbours when they
//! share a vertex, rook neighbours when they share an edge whose two ends match. Edges that
//! touch without matching vertices (a T-junction) are not found: topologically built data
//! (census tracts, municipalities) shares its vertices.

use std::collections::{BTreeSet, HashMap};

use mkaegis_core::{Error, Result};
use mkaegis_crs::Crs;
use mkaegis_geom::{Coord, Geometry, GeometryType, algo};
use mkaegis_model::FeatureClass;
use mkaegis_model::weights::{Bandwidth, Kernel, Method, Standardization, Weights};

use crate::kdtree::KdTree;

/// The most features weights are built for.
pub const MAX_FEATURES: usize = 2_000_000;
/// The most non-zero weights in one matrix.
pub const MAX_LINKS: usize = 50_000_000;
/// The highest contiguity order and the largest k.
pub const MAX_ORDER: u32 = 10;
pub const MAX_K: u32 = 1_000;
/// Nearest-neighbour candidates fetched by chord distance before ranking by geodesic distance.
const CANDIDATE_SLACK: usize = 4;
/// An adaptive bandwidth is the k-th nearest neighbour's distance times this, so that neighbour
/// sits just inside it and every one of the k nearest gets a positive weight, even with kernels
/// that reach 0 at the bandwidth (triangular, bisquare) (#48). Small enough that, even 20,000 km
/// away, it reaches only 20 µm past the k-th: ties aside, exactly k neighbours.
const ADAPTIVE_MARGIN: f64 = 1.0 + 1e-12;

/// What to build.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub method: Method,
    pub standardization: Standardization,
    /// Contiguity: vertices this close (metres) count as the same vertex.
    pub tolerance: f64,
}

/// Builds the weights for `fc` (the layer `layer`).
pub fn build(fc: &FeatureClass, layer: u32, name: &str, spec: &Spec) -> Result<Weights> {
    let n = fc.features.len();
    if n > MAX_FEATURES {
        return Err(Error::param("layer", format!("{n} features; spatial weights take up to {MAX_FEATURES}")));
    }
    let raw = match &spec.method {
        Method::Contiguity { queen, order, cumulative } => {
            if !(1..=MAX_ORDER).contains(order) {
                return Err(Error::param("order", format!("from 1 to {MAX_ORDER}")));
            }
            let first = contiguity(fc, *queen, spec.tolerance)?;
            let lists = if *order == 1 { first } else { higher_order(&first, *order, *cumulative)? };
            lists.into_iter().map(|row| row.into_iter().map(|j| (j, 1.0)).collect()).collect()
        }
        Method::Nearest { k } => {
            if !(1..=MAX_K).contains(k) {
                return Err(Error::param("k", format!("from 1 to {MAX_K}")));
            }
            let pts = Points::new(fc)?;
            pts.nearest_all(*k as usize)?.into_iter().map(|row| row.into_iter().map(|(j, _)| (j, 1.0)).collect()).collect()
        }
        Method::DistanceBand { threshold, power } => {
            let pts = Points::new(fc)?;
            distance_band(&pts, *threshold, *power)?
        }
        Method::Kernel { kernel, bandwidth, diagonal } => {
            let pts = Points::new(fc)?;
            kernel_weights(&pts, *kernel, *bandwidth, *diagonal)?
        }
        Method::File { .. } => return Err(Error::param("method", "file weights are read with `read_gal` or `read_gwt`")),
    };
    Ok(Weights {
        name: name.to_string(),
        layer,
        method: spec.method.clone(),
        standardization: spec.standardization,
        neighbours: standardize(raw, spec.standardization),
    })
}

/// The smallest distance band (metres) that leaves no feature without a neighbour: the largest
/// nearest-neighbour distance. `None` with fewer than two located features.
pub fn min_threshold(fc: &FeatureClass) -> Result<Option<f64>> {
    let pts = Points::new(fc)?;
    let nearest = pts.nearest_all(1)?;
    Ok(nearest.iter().filter_map(|row| row.iter().map(|(_, d)| *d).reduce(f64::min)).reduce(f64::max))
}

/// Scales raw weights; rows without neighbours stay empty.
pub fn standardize(mut rows: Vec<Vec<(u32, f64)>>, how: Standardization) -> Vec<Vec<(u32, f64)>> {
    match how {
        Standardization::None => {}
        Standardization::Binary => {
            for row in &mut rows {
                for w in row.iter_mut() {
                    w.1 = 1.0;
                }
            }
        }
        Standardization::Row => {
            for row in &mut rows {
                let sum: f64 = row.iter().map(|w| w.1).sum();
                if sum != 0.0 {
                    for w in row.iter_mut() {
                        w.1 /= sum;
                    }
                }
            }
        }
        Standardization::Variance => {
            let n = rows.len() as f64;
            for row in &mut rows {
                let norm = row.iter().map(|w| w.1 * w.1).sum::<f64>().sqrt();
                if norm != 0.0 {
                    for w in row.iter_mut() {
                        w.1 /= norm;
                    }
                }
            }
            let total: f64 = rows.iter().flatten().map(|w| w.1).sum();
            if total != 0.0 {
                for w in rows.iter_mut().flatten() {
                    w.1 *= n / total;
                }
            }
        }
    }
    rows
}

// Contiguity.

/// First-order neighbour rows of each polygon, sorted.
fn contiguity(fc: &FeatureClass, queen: bool, tolerance: f64) -> Result<Vec<Vec<u32>>> {
    if fc.geometry_type != GeometryType::Polygon {
        return Err(Error::param(
            "method",
            format!("contiguity needs polygons; `{}` has {} features", fc.name, fc.geometry_type.name().to_lowercase()),
        ));
    }
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(Error::param("tolerance", "a distance of zero or more metres"));
    }
    // Tolerance in the layer's units: degrees on geographic data (a metre is ~1/111 320 degree).
    let tol = if fc.crs.is_geographic() { tolerance / 111_320.0 } else { tolerance };
    // Every vertex: (row, ring, index, coordinate).
    let mut vertices: Vec<(u32, u32, u32, Coord)> = Vec::new();
    let mut largest = 0.0f64;
    for (row, f) in fc.features.iter().enumerate() {
        let Some(Geometry::Polygon(rings)) = &f.geometry else { continue };
        for (r, ring) in rings.iter().enumerate() {
            // A closed ring repeats its first vertex last.
            for (v, c) in ring.iter().enumerate().take(ring.len().saturating_sub(1)) {
                if vertices.len() >= crate::MAX_VERTICES {
                    return Err(Error::param("layer", format!("more than {} vertices", crate::MAX_VERTICES)));
                }
                if !c.x.is_finite() || !c.y.is_finite() {
                    return Err(Error::Geometry(format!("row {}: a coordinate is not a finite number", row + 1)));
                }
                largest = largest.max(c.x.abs()).max(c.y.abs());
                vertices.push((index(row)?, index(r)?, index(v)?, *c));
            }
        }
    }
    // Vertices in grid cells. Without a tolerance, a cell is one exact coordinate (its bits, with
    // -0 as 0), so any finite coordinate works (#46). With one, cells are at least the tolerance
    // wide, so a vertex's matches are in its own and the eight cells around; never narrower than
    // 1e-12 of the largest coordinate, so cell numbers stay far inside i64.
    let exact = tol == 0.0;
    let cell = tol.max(largest * 1e-12);
    let key = |c: Coord| -> (i64, i64) {
        if exact {
            let bits = |v: f64| (if v == 0.0 { 0.0f64 } else { v }).to_bits() as i64;
            (bits(c.x), bits(c.y))
        } else {
            ((c.x / cell).floor() as i64, (c.y / cell).floor() as i64)
        }
    };
    let mut grid: HashMap<(i64, i64), Vec<(u32, u32, u32, Coord)>> = HashMap::new();
    for v in vertices {
        grid.entry(key(v.3)).or_default().push(v);
    }
    // Matching vertices between distinct polygons: (row, ring, index) ↔ (row, ring, index). A
    // tolerance wide enough to put most vertices in a few cells would compare every pair, in time
    // and memory: past a budget well above what real coverages need, it is refused.
    let total: usize = grid.values().map(Vec::len).sum();
    let budget = total.saturating_mul(16).saturating_add(1_000_000);
    let mut compared = 0usize;
    let around: &[i64] = if exact { &[0] } else { &[-1, 0, 1] };
    let mut matches: HashMap<(u32, u32), Vec<((u32, u32), (u32, u32))>> = HashMap::new();
    for (k, here) in &grid {
        for dx in around {
            for dy in around {
                let Some(there) = grid.get(&(k.0.saturating_add(*dx), k.1.saturating_add(*dy))) else { continue };
                compared = compared.saturating_add(here.len().saturating_mul(there.len()));
                if compared > budget {
                    return Err(Error::param("tolerance", "too large for this layer: it makes most vertices match each other"));
                }
                for a in here {
                    for b in there.iter().filter(|b| b.0 > a.0) {
                        let same = if exact { true } else { a.3.distance(b.3) <= tol };
                        if same {
                            matches.entry((a.0, b.0)).or_default().push(((a.1, a.2), (b.1, b.2)));
                        }
                    }
                }
            }
        }
    }
    let mut lists = vec![BTreeSet::new(); fc.features.len()];
    for ((i, j), pairs) in matches {
        if queen || shares_edge(fc, i, j, &pairs) {
            if let Some(l) = lists.get_mut(i as usize) {
                l.insert(j);
            }
            if let Some(l) = lists.get_mut(j as usize) {
                l.insert(i);
            }
        }
    }
    Ok(lists.into_iter().map(|s| s.into_iter().collect()).collect())
}

/// Whether two polygons share an edge: consecutive vertices of one match consecutive vertices
/// of the other.
fn shares_edge(fc: &FeatureClass, i: u32, j: u32, pairs: &[((u32, u32), (u32, u32))]) -> bool {
    let ring_len = |row: u32, ring: u32| -> u32 {
        match fc.features.get(row as usize).and_then(|f| f.geometry.as_ref()) {
            Some(Geometry::Polygon(rings)) => rings.get(ring as usize).map_or(0, |r| r.len().saturating_sub(1) as u32),
            _ => 0,
        }
    };
    let adjacent = |len: u32, a: u32, b: u32| len > 1 && ((a + 1) % len == b || (b + 1) % len == a);
    pairs.iter().enumerate().any(|(x, (pa, qa))| {
        pairs
            .iter()
            .skip(x + 1)
            .any(|(pb, qb)| pa.0 == pb.0 && qa.0 == qb.0 && adjacent(ring_len(i, pa.0), pa.1, pb.1) && adjacent(ring_len(j, qa.0), qa.1, qb.1))
    })
}

/// Neighbours up to `order` steps away along first-order links (breadth first): all of them
/// when `cumulative`, else only those exactly `order` steps away.
fn higher_order(first: &[Vec<u32>], order: u32, cumulative: bool) -> Result<Vec<Vec<u32>>> {
    let mut out = Vec::with_capacity(first.len());
    let mut links = 0usize;
    let mut depth = vec![u32::MAX; first.len()];
    for start in 0..first.len() {
        let mut seen = vec![start];
        if let Some(d) = depth.get_mut(start) {
            *d = 0;
        }
        let mut frontier = vec![start];
        for step in 1..=order {
            let mut next = Vec::new();
            for &u in &frontier {
                for &v in first.get(u).map_or(&[][..], Vec::as_slice) {
                    if let Some(d) = depth.get_mut(v as usize).filter(|d| **d == u32::MAX) {
                        *d = step;
                        seen.push(v as usize);
                        next.push(v as usize);
                    }
                }
            }
            frontier = next;
        }
        let mut row: Vec<u32> = seen
            .iter()
            .filter(|&&v| depth.get(v).is_some_and(|&d| d != 0 && (cumulative || d == order)))
            .filter_map(|&v| u32::try_from(v).ok())
            .collect();
        row.sort_unstable();
        links = links.saturating_add(row.len());
        if links > MAX_LINKS {
            return Err(too_many_links());
        }
        for v in seen {
            if let Some(d) = depth.get_mut(v) {
                *d = u32::MAX;
            }
        }
        out.push(row);
    }
    Ok(out)
}

// Distance-based weights.

/// Features' representative points, in longitude/latitude, with a k-d tree on the unit sphere
/// (chord distance ranks like the great-circle distance).
struct Points {
    crs: Crs,
    lonlat: Vec<Option<Coord>>,
    /// Located rows, in tree order.
    rows: Vec<usize>,
    tree: KdTree,
}

impl Points {
    fn new(fc: &FeatureClass) -> Result<Points> {
        let mut lonlat = Vec::with_capacity(fc.features.len());
        let (mut rows, mut xyz) = (Vec::new(), Vec::new());
        for (row, f) in fc.features.iter().enumerate() {
            let p = match f.geometry.as_ref().and_then(algo::centroid) {
                Some(c) => Some(fc.crs.to_geographic(c).map_err(|e| Error::Projection(format!("row {}: {e}", row + 1)))?),
                None => None,
            };
            if let Some(g) = p {
                let (lon, lat) = (g.x.to_radians(), g.y.to_radians());
                xyz.push([lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]);
                rows.push(row);
            }
            lonlat.push(p);
        }
        Ok(Points { crs: fc.crs, lonlat, rows, tree: KdTree::new(xyz) })
    }

    fn distance(&self, a: usize, b: usize) -> Result<f64> {
        match (self.lonlat.get(a).copied().flatten(), self.lonlat.get(b).copied().flatten()) {
            (Some(p), Some(q)) => self.crs.ellipsoid().distance(p, q),
            _ => Err(Error::Geometry("a feature without a location".into())),
        }
    }

    fn xyz(&self, row: usize) -> Option<[f64; 3]> {
        let g = self.lonlat.get(row).copied().flatten()?;
        let (lon, lat) = (g.x.to_radians(), g.y.to_radians());
        Some([lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()])
    }

    /// For every row, its `k` nearest rows by geodesic distance (ties by row): (row, metres).
    fn nearest_all(&self, k: usize) -> Result<Vec<Vec<(u32, f64)>>> {
        if self.lonlat.len().saturating_mul(k) > MAX_LINKS {
            return Err(too_many_links());
        }
        let mut out = Vec::with_capacity(self.lonlat.len());
        for row in 0..self.lonlat.len() {
            let Some(q) = self.xyz(row) else {
                out.push(Vec::new());
                continue;
            };
            let mut found: Vec<(f64, usize)> = Vec::new();
            for (_, at) in self.tree.nearest(q, k + 1 + CANDIDATE_SLACK) {
                let Some(&other) = self.rows.get(at) else { continue };
                if other != row {
                    found.push((self.distance(row, other)?, other));
                }
            }
            found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            found.truncate(k);
            // The k nearest, listed in row order like every weights row.
            found.sort_by_key(|w| w.1);
            out.push(found.into_iter().filter_map(|(d, j)| u32::try_from(j).ok().map(|j| (j, d))).collect());
        }
        Ok(out)
    }

    /// For every row, the rows within `metres` (geodesic), excluding itself: (row, metres).
    fn within_all(&self, metres: f64) -> Result<Vec<Vec<(u32, f64)>>> {
        let mut out = Vec::with_capacity(self.lonlat.len());
        let mut links = 0usize;
        for row in 0..self.lonlat.len() {
            let found = self.within(row, metres)?;
            links = links.saturating_add(found.len());
            if links > MAX_LINKS {
                return Err(too_many_links());
            }
            out.push(found);
        }
        Ok(out)
    }

    /// The rows within `metres` (geodesic) of `row`, excluding itself, in row order: (row, metres).
    fn within(&self, row: usize, metres: f64) -> Result<Vec<(u32, f64)>> {
        let Some(q) = self.xyz(row) else { return Ok(Vec::new()) };
        // A geodesic of length d spans at most d / b radians (b: the semi-minor axis, the
        // smallest radius of curvature bound used here); the chord of that angle bounds it.
        let e = self.crs.ellipsoid();
        let b = e.a * (1.0 - 1.0 / e.inv_f);
        let angle = (metres / b * 1.01).min(std::f64::consts::PI);
        let chord = 2.0 * (angle / 2.0).sin();
        let mut found = Vec::new();
        for (_, at) in self.tree.within(q, chord) {
            let Some(&other) = self.rows.get(at) else { continue };
            if other == row {
                continue;
            }
            let d = self.distance(row, other)?;
            if d <= metres {
                found.push((u32::try_from(other).map_err(|_| too_many_links())?, d));
            }
        }
        found.sort_by_key(|w| w.0);
        Ok(found)
    }
}

fn distance_band(pts: &Points, threshold: f64, power: Option<f64>) -> Result<Vec<Vec<(u32, f64)>>> {
    if !threshold.is_finite() || threshold <= 0.0 || threshold > 2.1e7 {
        return Err(Error::param("threshold", "a distance in metres, above 0 and up to half the Earth's circumference"));
    }
    if let Some(p) = power
        && (!p.is_finite() || p <= 0.0 || p > 10.0)
    {
        return Err(Error::param("power", "above 0 and up to 10"));
    }
    let mut rows = pts.within_all(threshold)?;
    if let Some(p) = power {
        for (i, row) in rows.iter_mut().enumerate() {
            for w in row.iter_mut() {
                if w.1 == 0.0 {
                    return Err(Error::param(
                        "power",
                        format!("rows {} and {} are at the same place: inverse distance is undefined", i + 1, w.0 + 1),
                    ));
                }
                w.1 = w.1.powf(-p);
            }
        }
    } else {
        for w in rows.iter_mut().flatten() {
            w.1 = 1.0;
        }
    }
    Ok(rows)
}

fn kernel_weights(pts: &Points, kernel: Kernel, bandwidth: Bandwidth, diagonal: bool) -> Result<Vec<Vec<(u32, f64)>>> {
    let n = pts.lonlat.len();
    // Each row's bandwidth, in metres; `None` for a row without a location.
    let widths: Vec<Option<f64>> = match bandwidth {
        Bandwidth::Fixed(h) => {
            if !h.is_finite() || h <= 0.0 || h > 2.1e7 {
                return Err(Error::param("bandwidth", "a distance in metres, above 0"));
            }
            vec![Some(h); n]
        }
        Bandwidth::Adaptive(k) => {
            if !(1..=MAX_K).contains(&k) {
                return Err(Error::param("k", format!("from 1 to {MAX_K}")));
            }
            let mut widths = Vec::with_capacity(n);
            for (row, near) in pts.nearest_all(k as usize)?.iter().enumerate() {
                let kth = near.iter().map(|w| w.1).reduce(f64::max);
                if kth == Some(0.0) {
                    // Its k nearest are all at its own location: a bandwidth of 0 weighs nothing.
                    return Err(Error::param(
                        "k",
                        format!(
                            "row {}: its {k} nearest neighbours are at the same place, so its bandwidth would be 0; use a larger k or a fixed bandwidth",
                            row + 1
                        ),
                    ));
                }
                widths.push(kth.map(|d| d * ADAPTIVE_MARGIN));
            }
            widths
        }
    };
    // Each row searches its own bandwidth: one far-off point's wide bandwidth doesn't make every
    // row search that far.
    let mut out = Vec::with_capacity(n);
    let mut links = 0usize;
    for (row, h) in widths.into_iter().enumerate() {
        let Some(h) = h else {
            out.push(Vec::new());
            continue;
        };
        let mut list: Vec<(u32, f64)> =
            pts.within(row, h)?.into_iter().map(|(j, d)| (j, kernel_value(kernel, d / h))).filter(|w| w.1 > 0.0).collect();
        if diagonal && let Ok(me) = u32::try_from(row) {
            list.push((me, kernel_value(kernel, 0.0)));
            list.sort_by_key(|w| w.0);
        }
        links = links.saturating_add(list.len());
        if links > MAX_LINKS {
            return Err(too_many_links());
        }
        out.push(list);
    }
    Ok(out)
}

/// K(z) for z = d/h in [0, 1]: uniform ½, triangular 1 − z, bisquare (quartic) 15/16 (1 − z²)²,
/// Gaussian (2π)^−½ e^(−z²/2), all truncated at the bandwidth.
pub fn kernel_value(kernel: Kernel, z: f64) -> f64 {
    if !(0.0..=1.0).contains(&z) {
        return 0.0;
    }
    match kernel {
        Kernel::Uniform => 0.5,
        Kernel::Triangular => 1.0 - z,
        Kernel::Bisquare => 15.0 / 16.0 * (1.0 - z * z).powi(2),
        Kernel::Gaussian => (-z * z / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt(),
    }
}

fn index(i: usize) -> Result<u32> {
    u32::try_from(i).map_err(|_| Error::param("layer", "too many features or vertices"))
}

fn too_many_links() -> Error {
    Error::param("weights", format!("more than {MAX_LINKS} neighbour links: use fewer neighbours or a smaller distance"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkaegis_core::Value;
    use mkaegis_model::{Field, FieldType};

    /// A UTM zone, so the lattice is in metres.
    const UTM: u32 = 31983;
    const SIDE: f64 = 1000.0;
    const ORIGIN: (f64, f64) = (300_000.0, 7_400_000.0);

    mod contiguity {
        use super::*;

        #[test]
        fn rook_and_queen_on_a_lattice_match_counting_by_hand() {
            let fc = lattice(5);
            let rook = build(&fc, 1, "r", &spec(Method::Contiguity { queen: false, order: 1, cumulative: false })).unwrap();
            let queen = build(&fc, 1, "q", &spec(Method::Contiguity { queen: true, order: 1, cumulative: false })).unwrap();
            // Corner, edge and interior cells.
            for (cell, r, q) in [(0, 2, 3), (2, 3, 5), (12, 4, 8)] {
                assert_eq!(rook.of(cell).len(), r, "rook {cell}");
                assert_eq!(queen.of(cell).len(), q, "queen {cell}");
            }
            assert_eq!(rows(&rook, 12), vec![7, 11, 13, 17]);
            assert_eq!(rows(&queen, 0), vec![1, 5, 6]);
            // 4·2 + 12·3 + 9·4 rook links, all symmetric.
            let s = rook.summary();
            assert_eq!(s.links, 80);
            assert!(s.symmetric && s.islands.is_empty());
        }

        #[test]
        fn higher_orders_count_steps_along_the_links() {
            let fc = lattice(5);
            let exact = build(&fc, 1, "r2", &spec(Method::Contiguity { queen: false, order: 2, cumulative: false })).unwrap();
            // Two rook steps from the centre: the diamond's rim.
            assert_eq!(rows(&exact, 12), vec![2, 6, 8, 10, 14, 16, 18, 22]);
            let cumulative = build(&fc, 1, "r2c", &spec(Method::Contiguity { queen: false, order: 2, cumulative: true })).unwrap();
            assert_eq!(cumulative.of(12).len(), 12);
        }

        #[test]
        fn a_tolerance_joins_vertices_that_nearly_meet() {
            let mut fc = lattice(2);
            // Nudge one polygon's shared vertices by 1 cm.
            if let Some(Some(Geometry::Polygon(rings))) = fc.features.get_mut(1).map(|f| f.geometry.as_mut()) {
                for c in rings.iter_mut().flatten() {
                    c.x += 0.01;
                }
            }
            let exact = build(&fc, 1, "e", &spec(Method::Contiguity { queen: false, order: 1, cumulative: false })).unwrap();
            assert!(exact.of(0).iter().all(|w| w.0 != 1));
            let mut tolerant = spec(Method::Contiguity { queen: false, order: 1, cumulative: false });
            tolerant.tolerance = 0.05;
            assert_eq!(rows(&build(&fc, 1, "t", &tolerant).unwrap(), 0), vec![1, 2]);
        }

        /// The default tolerance (0) on coordinates past 9,000,000 used to fail with "coordinate
        /// out of range" (#46): northern Brazil's UTM northings, Web Mercator far from the equator.
        #[test]
        fn any_finite_coordinates_work() {
            let reference = build(&lattice(3), 1, "r", &spec(Method::Contiguity { queen: false, order: 1, cumulative: false })).unwrap();
            for (dx, dy) in [(0.0, 9_800_000.0 - ORIGIN.1), (-ORIGIN.0 - 20_000_000.0, 0.0), (1e12, 1e12)] {
                let fc = shifted(lattice(3), dx, dy);
                for tolerance in [0.0, 0.05] {
                    let mut rook = spec(Method::Contiguity { queen: false, order: 1, cumulative: false });
                    rook.tolerance = tolerance;
                    let w = build(&fc, 1, "r", &rook).unwrap();
                    for cell in 0..9 {
                        assert_eq!(rows(&w, cell), rows(&reference, cell), "cell {cell} shifted by ({dx}, {dy}), tolerance {tolerance}");
                    }
                }
            }
        }

        #[test]
        fn a_tolerance_that_matches_everything_is_refused() {
            let mut huge = spec(Method::Contiguity { queen: true, order: 1, cumulative: false });
            huge.tolerance = 1e9;
            let err = build(&lattice(40), 1, "h", &huge).unwrap_err();
            assert!(err.to_string().contains("too large"), "{err}");
            // A wide but reasonable one still works.
            huge.tolerance = 10.0;
            assert_eq!(build(&lattice(40), 1, "w", &huge).unwrap().of(0).len(), 3);
        }

        #[test]
        fn contiguity_needs_polygons() {
            let pts = points(&[(0.0, 0.0), (1.0, 0.0)]);
            assert!(build(&pts, 1, "x", &spec(Method::Contiguity { queen: true, order: 1, cumulative: false })).is_err());
            assert!(build(&lattice(2), 1, "x", &spec(Method::Contiguity { queen: true, order: 0, cumulative: false })).is_err());
        }
    }

    mod distance {
        use super::*;

        #[test]
        fn nearest_neighbours_break_ties_by_row() {
            // A 3×3 grid of points 1 km apart: the centre has four at 1 km.
            let pts = grid_points(3);
            let w = build(&pts, 1, "k", &spec(Method::Nearest { k: 4 })).unwrap();
            assert_eq!(rows(&w, 4), vec![1, 3, 5, 7]);
            let k2 = build(&pts, 1, "k2", &spec(Method::Nearest { k: 2 })).unwrap();
            assert_eq!(rows(&k2, 0), vec![1, 3]);
            assert!(!build(&pts, 1, "k1", &spec(Method::Nearest { k: 1 })).unwrap().summary().symmetric);
        }

        #[test]
        fn distance_bands_are_geodesic_metres() {
            let pts = grid_points(3);
            // 1 km apart in UTM is ~1000.1 m on the ellipsoid here (scale factor < 1 off the
            // central meridian): a 1001 m band finds the rook neighbours, not the diagonals.
            let w = build(&pts, 1, "d", &spec(Method::DistanceBand { threshold: 1001.0, power: None })).unwrap();
            assert_eq!(rows(&w, 4), vec![1, 3, 5, 7]);
            let wide = build(&pts, 1, "d2", &spec(Method::DistanceBand { threshold: 1500.0, power: None })).unwrap();
            assert_eq!(wide.of(4).len(), 8);
            let min = min_threshold(&pts).unwrap().unwrap();
            assert!((min - 1000.0).abs() < 1.0, "{min}");
        }

        #[test]
        fn inverse_distance_weights_and_coincident_points() {
            let pts = points(&[(0.0, 0.0), (1000.0, 0.0), (3000.0, 0.0)]);
            let w = build(&pts, 1, "i", &spec(Method::DistanceBand { threshold: 5000.0, power: Some(1.0) })).unwrap();
            let to = |i: usize, j: u32| w.of(i).iter().find(|x| x.0 == j).map_or(0.0, |x| x.1);
            // 1/d, the distances geodesic: within 0.1 % of the grid's.
            assert!((to(0, 1) * 1000.0 - 1.0).abs() < 1e-3 && (to(0, 2) * 3000.0 - 1.0).abs() < 1e-3);
            let twin = points(&[(0.0, 0.0), (0.0, 0.0)]);
            assert!(build(&twin, 1, "t", &spec(Method::DistanceBand { threshold: 10.0, power: Some(2.0) })).is_err());
        }

        #[test]
        fn kernels_weigh_by_distance_over_the_bandwidth() {
            let pts = points(&[(0.0, 0.0), (500.0, 0.0), (2000.0, 0.0)]);
            let method = Method::Kernel { kernel: Kernel::Triangular, bandwidth: Bandwidth::Fixed(1000.0), diagonal: true };
            let w = build(&pts, 1, "k", &spec(method)).unwrap();
            // Itself at K(0) = 1; the neighbour at 500 m: 1 − ½; the far point: outside.
            let row: Vec<(u32, f64)> = w.of(0).to_vec();
            assert_eq!(row.len(), 2);
            assert!((row[0].1 - 1.0).abs() < 1e-12 && (row[1].1 - 0.5).abs() < 1e-3, "{row:?}");
            let adaptive = Method::Kernel { kernel: Kernel::Gaussian, bandwidth: Bandwidth::Adaptive(1), diagonal: false };
            let a = build(&pts, 1, "a", &spec(adaptive)).unwrap();
            // Each point's bandwidth is just past its nearest neighbour's distance: that neighbour
            // sits at z = 1 − 1e-12.
            assert!((a.of(2)[0].1 - (-0.5f64).exp() / (2.0 * std::f64::consts::PI).sqrt()).abs() < 1e-9);
            assert_eq!(kernel_value(Kernel::Bisquare, 0.0), 15.0 / 16.0);
            assert_eq!(kernel_value(Kernel::Bisquare, 1.5), 0.0);
        }

        /// #48: with the bandwidth at the k-th neighbour's distance, that neighbour got K(1) = 0
        /// from triangular and bisquare kernels and was dropped: k = 1 made every row an island.
        #[test]
        fn adaptive_kernels_weigh_all_k_nearest() {
            // On a line at 0, 1000, 3000 and 3500 m.
            let pts = points(&[(0.0, 0.0), (1000.0, 0.0), (3000.0, 0.0), (3500.0, 0.0)]);
            for kernel in [Kernel::Uniform, Kernel::Triangular, Kernel::Bisquare, Kernel::Gaussian] {
                for k in [1u32, 2, 3] {
                    let method = Method::Kernel { kernel, bandwidth: Bandwidth::Adaptive(k), diagonal: false };
                    let w = build(&pts, 1, "a", &spec(method)).unwrap();
                    for row in 0..4 {
                        assert_eq!(w.of(row).len(), k as usize, "{kernel:?}, k = {k}, row {row}");
                    }
                    assert!(w.summary().islands.is_empty());
                }
            }
            // By hand, k = 2, bisquare, raw weights: row 0's bandwidth is 3000 m (its 2nd nearest),
            // so row 1 at 1000 m weighs 15/16 (1 − 1/9)² = 0.7407…, row 2 just inside: ~0.
            let mut raw = spec(Method::Kernel { kernel: Kernel::Bisquare, bandwidth: Bandwidth::Adaptive(2), diagonal: false });
            raw.standardization = Standardization::None;
            let w = build(&pts, 1, "b", &raw).unwrap();
            assert_eq!(rows(&w, 0), vec![1, 2]);
            assert!((w.of(0)[0].1 - 15.0 / 16.0 * (8.0f64 / 9.0).powi(2)).abs() < 1e-3, "{:?}", w.of(0));
            assert!(w.of(0)[1].1 > 0.0 && w.of(0)[1].1 < 1e-9);
            // Row 3's bandwidth is 2500 m (to row 1): row 2 at 500 m weighs 15/16 (1 − 0.04)².
            assert!((w.of(3)[1].1 - 15.0 / 16.0 * 0.96f64.powi(2)).abs() < 1e-3, "{:?}", w.of(3));
        }

        #[test]
        fn a_zero_adaptive_bandwidth_is_an_error_not_an_island() {
            let pts = points(&[(0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (900.0, 0.0)]);
            let method = |k| Method::Kernel { kernel: Kernel::Bisquare, bandwidth: Bandwidth::Adaptive(k), diagonal: true };
            let err = build(&pts, 1, "z", &spec(method(2))).unwrap_err().to_string();
            assert!(err.contains("row 1") && err.contains("same place"), "{err}");
            // With k past the duplicates every row has a bandwidth.
            assert!(build(&pts, 1, "z", &spec(method(3))).unwrap().summary().islands.is_empty());
        }

        /// #48: every row used to search the widest bandwidth of any row.
        #[test]
        fn an_outlier_does_not_widen_every_row_search() {
            // A 100 × 100 cluster 10 m apart and one point 1000 km away; k = 8. Searching every
            // row 1000 km wide gathered ~1e8 links and failed with "use fewer neighbours".
            let mut offsets: Vec<(f64, f64)> = (0..10_000).map(|i| ((i % 100) as f64 * 10.0, (i / 100) as f64 * 10.0)).collect();
            offsets.push((0.0, 1_000_000.0));
            let pts = points(&offsets);
            let method = Method::Kernel { kernel: Kernel::Bisquare, bandwidth: Bandwidth::Adaptive(8), diagonal: false };
            let w = build(&pts, 1, "o", &spec(method)).unwrap();
            assert!(w.summary().links <= 10_001 * 12, "{}", w.summary().links);
            assert_eq!(w.of(10_000).len(), 8);
        }

        #[test]
        fn null_shapes_are_islands() {
            let mut pts = points(&[(0.0, 0.0), (100.0, 0.0)]);
            pts.push(None, vec![Value::Int(9)]).unwrap();
            let w = build(&pts, 1, "n", &spec(Method::Nearest { k: 1 })).unwrap();
            assert_eq!(w.summary().islands, vec![2]);
        }
    }

    mod standardization {
        use super::*;

        #[test]
        fn row_binary_and_variance_scalings() {
            let raw = vec![vec![(1, 2.0), (2, 6.0)], vec![(0, 3.0)], vec![]];
            let row = standardize(raw.clone(), Standardization::Row);
            assert_eq!(row[0], vec![(1, 0.25), (2, 0.75)]);
            assert_eq!(row[2], vec![]);
            assert!(standardize(raw.clone(), Standardization::Binary).iter().flatten().all(|w| w.1 == 1.0));
            // S: rows over their norms, then all scaled to sum to n.
            let s = standardize(raw.clone(), Standardization::Variance);
            let total: f64 = s.iter().flatten().map(|w| w.1).sum();
            assert!((total - 3.0).abs() < 1e-12);
            let ratio = s[0][1].1 / s[0][0].1;
            assert!((ratio - 3.0).abs() < 1e-12, "proportions within a row are kept");
            assert_eq!(standardize(raw.clone(), Standardization::None), raw);
        }
    }

    #[test]
    fn hostile_parameters_never_panic() {
        let pts = grid_points(3);
        let poly = lattice(2);
        for method in [
            Method::Nearest { k: 0 },
            Method::Nearest { k: u32::MAX },
            Method::DistanceBand { threshold: f64::NAN, power: None },
            Method::DistanceBand { threshold: -1.0, power: None },
            Method::DistanceBand { threshold: 1e300, power: None },
            Method::DistanceBand { threshold: 10.0, power: Some(f64::INFINITY) },
            Method::Kernel { kernel: Kernel::Uniform, bandwidth: Bandwidth::Fixed(0.0), diagonal: false },
            Method::Kernel { kernel: Kernel::Uniform, bandwidth: Bandwidth::Adaptive(0), diagonal: false },
            Method::Contiguity { queen: true, order: 99, cumulative: true },
            Method::File { path: "x".into() },
        ] {
            assert!(build(&pts, 1, "x", &spec(method.clone())).is_err(), "{method:?}");
            let _ = build(&poly, 1, "x", &spec(method));
        }
        let mut bad_tol = spec(Method::Contiguity { queen: true, order: 1, cumulative: false });
        bad_tol.tolerance = f64::NAN;
        assert!(build(&poly, 1, "x", &bad_tol).is_err());
        let empty = FeatureClass::new("e", GeometryType::Point, Crs::WGS84, vec![]);
        assert_eq!(build(&empty, 1, "e", &spec(Method::Nearest { k: 3 })).unwrap().n(), 0);
        assert_eq!(min_threshold(&empty).unwrap(), None);
    }

    fn spec(method: Method) -> Spec {
        Spec { method, standardization: Standardization::None, tolerance: 0.0 }
    }

    fn rows(w: &Weights, row: usize) -> Vec<u32> {
        w.of(row).iter().map(|x| x.0).collect()
    }

    /// An n×n lattice of SIDE-metre squares, row by row from the south-west.
    fn lattice(n: usize) -> FeatureClass {
        let crs = Crs::from_epsg(UTM).unwrap();
        let mut fc = FeatureClass::new("cells", GeometryType::Polygon, crs, vec![Field::new("ID", FieldType::Integer)]);
        for r in 0..n {
            for c in 0..n {
                let (x, y) = (ORIGIN.0 + c as f64 * SIDE, ORIGIN.1 + r as f64 * SIDE);
                let ring = vec![Coord::new(x, y), Coord::new(x + SIDE, y), Coord::new(x + SIDE, y + SIDE), Coord::new(x, y + SIDE)];
                fc.push(Some(Geometry::polygon(vec![ring]).unwrap()), vec![Value::Int((r * n + c) as i64)]).unwrap();
            }
        }
        fc
    }

    /// The feature class moved by (dx, dy).
    fn shifted(mut fc: FeatureClass, dx: f64, dy: f64) -> FeatureClass {
        for f in &mut fc.features {
            if let Some(Geometry::Polygon(rings)) = f.geometry.as_mut() {
                for c in rings.iter_mut().flatten() {
                    c.x += dx;
                    c.y += dy;
                }
            }
        }
        fc
    }

    fn grid_points(n: usize) -> FeatureClass {
        let offsets: Vec<(f64, f64)> = (0..n * n).map(|i| ((i % n) as f64 * SIDE, (i / n) as f64 * SIDE)).collect();
        points(&offsets)
    }

    /// Points at metre offsets from ORIGIN, in the UTM zone.
    fn points(offsets: &[(f64, f64)]) -> FeatureClass {
        let crs = Crs::from_epsg(UTM).unwrap();
        let mut fc = FeatureClass::new("pts", GeometryType::Point, crs, vec![Field::new("ID", FieldType::Integer)]);
        for (i, (dx, dy)) in offsets.iter().enumerate() {
            fc.push(Some(Geometry::Point(Coord::new(ORIGIN.0 + dx, ORIGIN.1 + dy))), vec![Value::Int(i as i64)]).unwrap();
        }
        fc
    }
}
