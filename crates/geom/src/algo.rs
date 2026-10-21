//! Planar measures and relations: area, length, centroid, point-in-polygon, distance,
//! intersection, Douglas–Peucker simplification and point buffers. All iterative (no recursion)
//! and total: degenerate input gives 0, `None` or `false`, never a panic.

use crate::{Coord, Envelope, Geometry};

/// Twice the signed area of a ring (positive when counter-clockwise), by the shoelace formula.
fn ring_area2(ring: &[Coord]) -> f64 {
    ring.windows(2)
        .map(|w| match w {
            [a, b] => a.x * b.y - b.x * a.y,
            _ => 0.0,
        })
        .sum()
}

/// Whether `p` is inside `ring` (even-odd ray casting; a point on the edge may go either way).
pub fn ring_contains(ring: &[Coord], p: Coord) -> bool {
    let mut inside = false;
    for w in ring.windows(2) {
        let [a, b] = w else { continue };
        if (a.y > p.y) != (b.y > p.y) {
            let t = (p.y - a.y) / (b.y - a.y);
            if p.x < a.x + t * (b.x - a.x) {
                inside = !inside;
            }
        }
    }
    inside
}

/// How many of the other rings contain each ring: even depth is a shell, odd a hole.
fn ring_depths(rings: &[Vec<Coord>]) -> Vec<usize> {
    rings
        .iter()
        .enumerate()
        .map(|(i, ring)| {
            let Some(probe) = ring.first().copied() else { return 0 };
            rings.iter().enumerate().filter(|(j, other)| *j != i && ring_contains(other, probe)).count()
        })
        .collect()
}

/// The area of a polygon (holes subtracted); 0 for other geometries.
pub fn area(g: &Geometry) -> f64 {
    let Geometry::Polygon(rings) = g else { return 0.0 };
    let depths = ring_depths(rings);
    let total: f64 = rings.iter().zip(depths).map(|(r, d)| if d % 2 == 0 { ring_area2(r).abs() } else { -ring_area2(r).abs() }).sum();
    (total / 2.0).max(0.0)
}

fn path_length(path: &[Coord]) -> f64 {
    path.windows(2).map(|w| if let [a, b] = w { a.distance(*b) } else { 0.0 }).sum()
}

/// The length of a polyline, the perimeter of a polygon; 0 for points.
pub fn length(g: &Geometry) -> f64 {
    match g {
        Geometry::Polyline(parts) | Geometry::Polygon(parts) => parts.iter().map(|p| path_length(p)).sum(),
        _ => 0.0,
    }
}

/// The centre of mass: of the area for polygons, of the length for polylines, of the vertices
/// for points. `None` for an empty or degenerate shape.
pub fn centroid(g: &Geometry) -> Option<Coord> {
    match g {
        Geometry::Point(c) => Some(*c),
        Geometry::Multipoint(cs) => mean(cs),
        Geometry::Polyline(parts) => {
            let (mut sx, mut sy, mut sl) = (0.0, 0.0, 0.0);
            for w in parts.iter().flat_map(|p| p.windows(2)) {
                let [a, b] = w else { continue };
                let l = a.distance(*b);
                sx += l * (a.x + b.x) / 2.0;
                sy += l * (a.y + b.y) / 2.0;
                sl += l;
            }
            if sl > 0.0 { Some(Coord::new(sx / sl, sy / sl)) } else { mean(&parts.iter().flatten().copied().collect::<Vec<_>>()) }
        }
        Geometry::Polygon(rings) => {
            let depths = ring_depths(rings);
            let (mut sx, mut sy, mut sa) = (0.0, 0.0, 0.0);
            for (ring, depth) in rings.iter().zip(depths) {
                // Shift to the ring's first vertex: the sums stay accurate far from the origin.
                let Some(o) = ring.first().copied() else { continue };
                let (mut rx, mut ry, mut ra) = (0.0, 0.0, 0.0);
                for w in ring.windows(2) {
                    let [a, b] = w else { continue };
                    let (ax, ay, bx, by) = (a.x - o.x, a.y - o.y, b.x - o.x, b.y - o.y);
                    let cross = ax * by - bx * ay;
                    rx += (ax + bx) * cross;
                    ry += (ay + by) * cross;
                    ra += cross;
                }
                if ra == 0.0 {
                    continue;
                }
                // Each ring counts with its own sign: + for shells, − for holes.
                let sign = if depth % 2 == 0 { 1.0 } else { -1.0 };
                let a = ra / 2.0;
                let (cx, cy) = (rx / (6.0 * a) + o.x, ry / (6.0 * a) + o.y);
                sx += sign * a.abs() * cx;
                sy += sign * a.abs() * cy;
                sa += sign * a.abs();
            }
            if sa.abs() > 0.0 { Some(Coord::new(sx / sa, sy / sa)) } else { None }
        }
    }
}

fn mean(cs: &[Coord]) -> Option<Coord> {
    if cs.is_empty() {
        return None;
    }
    let n = cs.len() as f64;
    Some(Coord::new(cs.iter().map(|c| c.x).sum::<f64>() / n, cs.iter().map(|c| c.y).sum::<f64>() / n))
}

/// Whether a polygon contains `p` (holes excluded); `false` for other geometries.
pub fn contains_point(g: &Geometry, p: Coord) -> bool {
    let Geometry::Polygon(rings) = g else { return false };
    rings.iter().filter(|r| ring_contains(r, p)).count() % 2 == 1
}

/// The distance from `p` to the segment `a`–`b`.
pub fn segment_distance(p: Coord, a: Coord, b: Coord) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    p.distance(Coord::new(a.x + t * dx, a.y + t * dy))
}

/// The shortest distance from `p` to the geometry (0 inside a polygon); infinite when empty.
pub fn distance_to(g: &Geometry, p: Coord) -> f64 {
    match g {
        Geometry::Point(c) => p.distance(*c),
        Geometry::Multipoint(cs) => cs.iter().map(|c| p.distance(*c)).fold(f64::INFINITY, f64::min),
        Geometry::Polyline(parts) | Geometry::Polygon(parts) => {
            if contains_point(g, p) {
                return 0.0;
            }
            parts
                .iter()
                .flat_map(|path| path.windows(2))
                .map(|w| if let [a, b] = w { segment_distance(p, *a, *b) } else { f64::INFINITY })
                .chain(parts.iter().filter(|path| path.len() == 1).flatten().map(|c| p.distance(*c)))
                .fold(f64::INFINITY, f64::min)
        }
    }
}

fn orient(a: Coord, b: Coord, c: Coord) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Whether segments `a`–`b` and `c`–`d` touch or cross.
pub fn segments_intersect(a: Coord, b: Coord, c: Coord, d: Coord) -> bool {
    let (d1, d2, d3, d4) = (orient(c, d, a), orient(c, d, b), orient(a, b, c), orient(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    let on = |p: Coord, q: Coord, r: Coord, o: f64| o == 0.0 && Envelope::new(p, q).contains(r);
    on(c, d, a, d1) || on(c, d, b, d2) || on(a, b, c, d3) || on(a, b, d, d4)
}

fn segments(g: &Geometry) -> Vec<(Coord, Coord)> {
    match g {
        Geometry::Polyline(parts) | Geometry::Polygon(parts) => {
            parts.iter().flat_map(|p| p.windows(2)).filter_map(|w| if let [a, b] = w { Some((*a, *b)) } else { None }).collect()
        }
        _ => Vec::new(),
    }
}

fn points(g: &Geometry) -> Vec<Coord> {
    match g {
        Geometry::Point(c) => vec![*c],
        Geometry::Multipoint(cs) => cs.clone(),
        _ => Vec::new(),
    }
}

/// Whether two geometries share any point (OGC *intersects*), with a tolerance for points
/// meeting points and lines.
pub fn intersects(a: &Geometry, b: &Geometry, tolerance: f64) -> bool {
    let (Some(ea), Some(eb)) = (a.envelope(), b.envelope()) else { return false };
    if !ea.buffer(tolerance).intersects(&eb) {
        return false;
    }
    // A point of one in, on or near the other.
    let near = |g: &Geometry, p: Coord| distance_to(g, p) <= tolerance;
    if points(a).into_iter().any(|p| near(b, p)) || points(b).into_iter().any(|p| near(a, p)) {
        return true;
    }
    let (sa, sb) = (segments(a), segments(b));
    if sa.is_empty() || sb.is_empty() {
        return false;
    }
    for (p, q) in &sa {
        if !Envelope::new(*p, *q).intersects(&eb) {
            continue;
        }
        if sb.iter().any(|(r, s)| segments_intersect(*p, *q, *r, *s)) {
            return true;
        }
    }
    // No edges cross: one may lie wholly inside the other.
    let first = |g: &Geometry| g.coords().next();
    first(a).is_some_and(|p| contains_point(b, p)) || first(b).is_some_and(|p| contains_point(a, p))
}

/// Whether `inner` lies within polygon `outer`, its boundary included (OGC *within* as the
/// closure: a municipality on its state's border is within the state). Every vertex and every
/// edge's midpoint of `inner` is in or on `outer`, no edge crosses `outer`'s boundary, a polygon
/// `inner` has an interior point in `outer`, and no hole of `outer` lies inside it (#51).
pub fn within(inner: &Geometry, outer: &Geometry) -> bool {
    let Geometry::Polygon(outer_rings) = outer else { return false };
    let covered = |p: Coord| covers_point(outer, p);
    if !inner.coords().all(covered) {
        return false;
    }
    let inner_edges = segments(inner);
    if !inner_edges.iter().all(|(p, q)| covered(Coord::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0))) {
        return false;
    }
    let edges = segments(outer);
    let crosses = inner_edges.iter().any(|(p, q)| {
        edges.iter().any(|(r, s)| {
            // A proper crossing (not merely touching at a shared vertex) leaves the polygon.
            let (d1, d2, d3, d4) = (orient(*r, *s, *p), orient(*r, *s, *q), orient(*p, *q, *r), orient(*p, *q, *s));
            d1 * d2 < 0.0 && d3 * d4 < 0.0
        })
    });
    if crosses {
        return false;
    }
    if let Geometry::Polygon(_) = inner {
        // Its inside must be the outer polygon's too: not one of its holes, and no hole within it.
        if !interior_point(inner).is_some_and(covered) {
            return false;
        }
        let depths = ring_depths(outer_rings);
        let holes = outer_rings.iter().zip(depths).filter(|(_, d)| d % 2 == 1).map(|(r, _)| r);
        for hole in holes {
            if hole.iter().any(|v| contains_point(inner, *v) && !on_boundary(inner, *v)) {
                return false;
            }
        }
    }
    true
}

/// Whether `p` is in a polygon or on its boundary.
pub fn covers_point(g: &Geometry, p: Coord) -> bool {
    contains_point(g, p) || on_boundary(g, p)
}

/// Whether `p` lies on one of the geometry's edges, to within 1e-12 of its coordinates' size.
pub fn on_boundary(g: &Geometry, p: Coord) -> bool {
    let scale = g.coords().map(|c| c.x.abs().max(c.y.abs())).fold(1.0, f64::max);
    segments(g).iter().any(|(a, b)| segment_distance(p, *a, *b) <= scale * 1e-12)
}

/// A point strictly inside a polygon (on a horizontal line through its envelope's middle, midway
/// across the first stretch inside); the centroid may lie outside a concave or holed polygon.
/// `None` for other geometries or a degenerate polygon.
pub fn interior_point(g: &Geometry) -> Option<Coord> {
    let Geometry::Polygon(rings) = g else { return None };
    let env = g.envelope()?;
    // A line through vertices would make crossings ambiguous: nudge it between vertex heights.
    let mut ys: Vec<f64> = rings.iter().flatten().map(|c| c.y).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let middle = env.center().y;
    let y = ys
        .windows(2)
        .filter_map(|w| if let [a, b] = w { Some((a + b) / 2.0) } else { None })
        .min_by(|a, b| (a - middle).abs().total_cmp(&(b - middle).abs()))?;
    let mut xs: Vec<f64> = rings
        .iter()
        .flat_map(|r| r.windows(2))
        .filter_map(|w| match w {
            [a, b] if (a.y > y) != (b.y > y) => Some(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x)),
            _ => None,
        })
        .collect();
    xs.sort_by(f64::total_cmp);
    // Even-odd: between the first and second crossing is inside.
    match xs.as_slice() {
        [x0, x1, ..] => Some(Coord::new((x0 + x1) / 2.0, y)),
        _ => None,
    }
}

/// The shortest distance between two geometries (0 when they intersect). Two shapes that don't
/// meet are closest at a vertex of one, so vertex-to-shape distances both ways give it.
pub fn distance(a: &Geometry, b: &Geometry) -> f64 {
    if intersects(a, b, 0.0) {
        return 0.0;
    }
    let one_way = |from: &Geometry, to: &Geometry| from.coords().map(|p| distance_to(to, p)).fold(f64::INFINITY, f64::min);
    one_way(a, b).min(one_way(b, a))
}

/// Douglas–Peucker: the vertices of `path` that keep it within `tolerance`. Iterative.
pub fn simplify_path(path: &[Coord], tolerance: f64) -> Vec<Coord> {
    if path.len() < 3 || tolerance.is_nan() || tolerance <= 0.0 {
        return path.to_vec();
    }
    let mut keep = vec![false; path.len()];
    let last = path.len() - 1;
    keep[0] = true;
    keep[last] = true;
    let mut stack = vec![(0usize, last)];
    while let Some((s, e)) = stack.pop() {
        let (Some(a), Some(b)) = (path.get(s), path.get(e)) else { continue };
        let mut worst = (0.0, 0usize);
        for (i, p) in path.iter().enumerate().take(e).skip(s + 1) {
            let d = segment_distance(*p, *a, *b);
            if d > worst.0 {
                worst = (d, i);
            }
        }
        if worst.0 > tolerance {
            if let Some(k) = keep.get_mut(worst.1) {
                *k = true;
            }
            stack.push((s, worst.1));
            stack.push((worst.1, e));
        }
    }
    path.iter().zip(keep).filter_map(|(p, k)| k.then_some(*p)).collect()
}

/// A geometry simplified with Douglas–Peucker; rings keep at least a triangle.
pub fn simplify(g: &Geometry, tolerance: f64) -> Geometry {
    match g {
        Geometry::Polyline(parts) => Geometry::Polyline(parts.iter().map(|p| simplify_path(p, tolerance)).collect()),
        Geometry::Polygon(rings) => Geometry::Polygon(
            rings.iter().map(|r| simplify_path(r, tolerance)).zip(rings).map(|(s, r)| if s.len() >= 4 { s } else { r.clone() }).collect(),
        ),
        other => other.clone(),
    }
}

/// A circle of `radius` around `center` as a closed ring with `segments` sides (clamped 8..=720).
pub fn circle(center: Coord, radius: f64, segments: usize) -> Vec<Coord> {
    let n = segments.clamp(8, 720);
    let mut ring: Vec<Coord> = (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            Coord::new(center.x + radius * t.cos(), center.y + radius * t.sin())
        })
        .collect();
    if let Some(first) = ring.first().copied() {
        ring.push(first);
    }
    ring
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, s: f64) -> Vec<Coord> {
        vec![Coord::new(x0, y0), Coord::new(x0 + s, y0), Coord::new(x0 + s, y0 + s), Coord::new(x0, y0 + s), Coord::new(x0, y0)]
    }

    #[test]
    fn area_subtracts_holes_whatever_the_orientation() {
        let mut hole = square(2.0, 2.0, 2.0);
        // Same orientation as the shell: still a hole, found by containment.
        let g = Geometry::Polygon(vec![square(0.0, 0.0, 10.0), hole.clone()]);
        assert_eq!(area(&g), 96.0);
        hole.reverse();
        assert_eq!(area(&Geometry::Polygon(vec![square(0.0, 0.0, 10.0), hole])), 96.0);
        assert_eq!(length(&Geometry::Polygon(vec![square(0.0, 0.0, 10.0)])), 40.0);
    }

    #[test]
    fn centroid_of_a_square_with_an_off_centre_hole() {
        let g = Geometry::Polygon(vec![square(0.0, 0.0, 10.0), square(1.0, 1.0, 5.0)]);
        // 100 at (5,5) minus 25 at (3.5,3.5): (500 − 87.5) / 75.
        let c = centroid(&g).unwrap();
        assert!((c.x - 412.5 / 75.0).abs() < 1e-12 && (c.y - 412.5 / 75.0).abs() < 1e-12);
        let far = Geometry::Polygon(vec![square(1e7, 1e7, 2.0)]);
        let c = centroid(&far).unwrap();
        assert!((c.x - (1e7 + 1.0)).abs() < 1e-6);
    }

    #[test]
    fn point_in_polygon_and_distance() {
        let g = Geometry::Polygon(vec![square(0.0, 0.0, 10.0), square(4.0, 4.0, 2.0)]);
        assert!(contains_point(&g, Coord::new(1.0, 1.0)));
        assert!(!contains_point(&g, Coord::new(5.0, 5.0)));
        assert_eq!(distance_to(&g, Coord::new(13.0, 5.0)), 3.0);
        assert_eq!(distance_to(&g, Coord::new(1.0, 1.0)), 0.0);
    }

    #[test]
    fn intersects_and_within() {
        let big = Geometry::Polygon(vec![square(0.0, 0.0, 10.0)]);
        let small = Geometry::Polygon(vec![square(2.0, 2.0, 1.0)]);
        let apart = Geometry::Polygon(vec![square(20.0, 20.0, 1.0)]);
        let crossing = Geometry::Polyline(vec![vec![Coord::new(-5.0, 5.0), Coord::new(5.0, 5.0)]]);
        assert!(intersects(&big, &small, 0.0) && intersects(&small, &big, 0.0));
        assert!(!intersects(&big, &apart, 0.0));
        assert!(intersects(&big, &crossing, 0.0));
        assert!(within(&small, &big) && !within(&crossing, &big) && !within(&big, &small));
        assert!(intersects(&Geometry::Point(Coord::new(10.5, 5.0)), &big, 0.5));
    }

    /// #51: within() ignored holes and shared borders.
    #[test]
    fn within_respects_holes_and_shared_borders() {
        let holed = Geometry::Polygon(vec![square(0.0, 0.0, 10.0), square(4.0, 4.0, 2.0)]);
        let over_the_hole = Geometry::Polygon(vec![square(2.0, 2.0, 6.0)]);
        assert!(!within(&over_the_hole, &holed), "covers the hole");
        let beside_the_hole = Geometry::Polygon(vec![square(1.0, 1.0, 2.0)]);
        assert!(within(&beside_the_hole, &holed));
        let the_hole = Geometry::Polygon(vec![square(4.0, 4.0, 2.0)]);
        assert!(!within(&the_hole, &holed), "the hole itself is outside");
        // On the right and top edges: a corner municipality of its state.
        let state = Geometry::Polygon(vec![square(0.0, 0.0, 10.0)]);
        let corner = Geometry::Polygon(vec![square(5.0, 5.0, 5.0)]);
        assert!(within(&corner, &state) && within(&Geometry::Polygon(vec![square(0.0, 0.0, 10.0)]), &state));
        assert!(within(&Geometry::Point(Coord::new(10.0, 3.0)), &state), "a point on the border");
        // A chord across a notch goes outside, though both ends are on the boundary.
        let notched = Geometry::Polygon(vec![vec![
            Coord::new(0.0, 0.0),
            Coord::new(10.0, 0.0),
            Coord::new(10.0, 10.0),
            Coord::new(5.0, 5.0),
            Coord::new(0.0, 10.0),
            Coord::new(0.0, 0.0),
        ]]);
        let chord = Geometry::Polyline(vec![vec![Coord::new(0.0, 10.0), Coord::new(10.0, 10.0)]]);
        assert!(!within(&chord, &notched));
    }

    #[test]
    fn distances_between_shapes() {
        let a = Geometry::Polyline(vec![vec![Coord::new(0.0, 0.0), Coord::new(10.0, 0.0)]]);
        let b = Geometry::Polygon(vec![square(3.0, 2.0, 4.0)]);
        assert_eq!(distance(&a, &b), 2.0);
        let crossing = Geometry::Polyline(vec![vec![Coord::new(5.0, -1.0), Coord::new(5.0, 1.0)]]);
        assert_eq!(distance(&a, &crossing), 0.0);
        let c = interior_point(&Geometry::Polygon(vec![square(0.0, 0.0, 10.0), square(2.0, 2.0, 6.0)])).unwrap();
        assert!(c.x < 2.0 && c.y > 0.0 && c.y < 10.0, "{c:?} is in the frame, not the hole");
    }

    #[test]
    fn simplify_keeps_the_ends_and_the_corners() {
        let path = vec![Coord::new(0.0, 0.0), Coord::new(1.0, 0.01), Coord::new(2.0, 0.0), Coord::new(2.0, 5.0)];
        assert_eq!(simplify_path(&path, 0.1), vec![Coord::new(0.0, 0.0), Coord::new(2.0, 0.0), Coord::new(2.0, 5.0)]);
        assert_eq!(simplify_path(&path, f64::NAN), path);
        // A long path: no recursion, no stack overflow.
        let long: Vec<Coord> = (0..200_000).map(|i| Coord::new(i as f64, (i % 2) as f64)).collect();
        assert_eq!(simplify_path(&long, 5.0).len(), 2);
    }

    #[test]
    fn circles_are_closed() {
        let ring = circle(Coord::new(1.0, 1.0), 2.0, 0);
        assert_eq!(ring.len(), 9);
        assert_eq!(ring.first(), ring.last());
        let a = area(&Geometry::Polygon(vec![circle(Coord::default(), 1.0, 720)]));
        assert!((a - std::f64::consts::PI).abs() < 1e-4);
    }
}
