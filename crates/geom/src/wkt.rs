//! OGC Well-Known Text (Simple Features Access, part 1): `POINT`, `MULTIPOINT`, `LINESTRING`,
//! `MULTILINESTRING`, `POLYGON`, `MULTIPOLYGON`, 2D. Z and M values are read and dropped.

use mkaegis_core::{Error, Result};

use crate::{Coord, Geometry};

/// The longest WKT text we parse.
const MAX_LEN: usize = 64 << 20;

/// The geometry as WKT. Multi-part polygons are grouped back into shells with their holes.
pub fn to_wkt(g: &Geometry) -> String {
    let c = |c: &Coord| format!("{} {}", c.x, c.y);
    let list = |cs: &[Coord]| cs.iter().map(c).collect::<Vec<_>>().join(", ");
    let rings = |rs: &[&Vec<Coord>]| rs.iter().map(|r| format!("({})", list(r))).collect::<Vec<_>>().join(", ");
    match g {
        Geometry::Point(p) => format!("POINT ({})", c(p)),
        Geometry::Multipoint(ps) => format!("MULTIPOINT ({})", ps.iter().map(|p| format!("({})", c(p))).collect::<Vec<_>>().join(", ")),
        Geometry::Polyline(parts) if parts.len() == 1 => format!("LINESTRING ({})", parts.first().map(|p| list(p)).unwrap_or_default()),
        Geometry::Polyline(parts) => format!("MULTILINESTRING ({})", rings(&parts.iter().collect::<Vec<_>>())),
        Geometry::Polygon(rs) => {
            let groups = shells_with_holes(rs);
            if groups.len() == 1 {
                format!("POLYGON ({})", groups.first().map(|g| rings(g)).unwrap_or_default())
            } else {
                format!("MULTIPOLYGON ({})", groups.iter().map(|g| format!("({})", rings(g))).collect::<Vec<_>>().join(", "))
            }
        }
    }
}

/// Polygon rings grouped as shell, holes…: a ring inside an odd number of others is a hole of
/// the smallest ring that contains it (containment as `algo::ring_containers` decides it).
pub fn shells_with_holes(rings: &[Vec<Coord>]) -> Vec<Vec<&Vec<Coord>>> {
    let containers = crate::algo::ring_containers(rings);
    let area = |i: usize| crate::algo::area(&Geometry::Polygon(rings.get(i).cloned().into_iter().collect()));
    let mut groups: Vec<(usize, Vec<&Vec<Coord>>)> = Vec::new();
    let mut holes = Vec::new();
    for (i, (ring, inside)) in rings.iter().zip(containers).enumerate() {
        if inside.len() % 2 == 0 {
            groups.push((i, vec![ring]));
        } else {
            holes.push((i, inside));
        }
    }
    for (i, inside) in holes {
        let parent = inside.into_iter().filter(|c| groups.iter().any(|(s, _)| s == c)).min_by(|a, b| area(*a).total_cmp(&area(*b)));
        match (parent.and_then(|p| groups.iter_mut().find(|(s, _)| *s == p)), rings.get(i)) {
            (Some((_, group)), Some(ring)) => group.push(ring),
            (None, Some(ring)) => groups.push((i, vec![ring])),
            _ => {}
        }
    }
    groups.into_iter().map(|(_, g)| g).collect()
}

/// Parses WKT text into a geometry.
pub fn from_wkt(text: &str) -> Result<Geometry> {
    if text.len() > MAX_LEN {
        return Err(Error::format("WKT", "text too long"));
    }
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    let tag = p.word().to_ascii_uppercase();
    // Optional dimension marker: Z, M or ZM.
    let mark = p.peek_word().to_ascii_uppercase();
    if matches!(mark.as_str(), "Z" | "M" | "ZM") {
        p.word();
    }
    if p.peek_word().eq_ignore_ascii_case("EMPTY") {
        return Err(Error::Geometry("empty geometry".into()));
    }
    let g = match tag.as_str() {
        "POINT" => {
            let cs = p.coords()?;
            Geometry::Point(cs.first().copied().ok_or_else(|| Error::format("WKT", "POINT without coordinates"))?)
        }
        "MULTIPOINT" => {
            p.expect(b'(')?;
            let mut out = Vec::new();
            loop {
                p.ws();
                // Both `MULTIPOINT ((1 2), (3 4))` and `MULTIPOINT (1 2, 3 4)`.
                if p.peek() == Some(b'(') {
                    out.extend(p.coords()?);
                } else {
                    out.push(p.coord()?);
                }
                if !p.comma() {
                    break;
                }
            }
            p.expect(b')')?;
            Geometry::Multipoint(out)
        }
        "LINESTRING" => Geometry::Polyline(vec![p.coords()?]),
        "MULTILINESTRING" => Geometry::Polyline(p.lists()?),
        "POLYGON" => Geometry::polygon(p.lists()?)?,
        "MULTIPOLYGON" => {
            p.expect(b'(')?;
            let mut rings = Vec::new();
            loop {
                rings.extend(p.lists()?);
                if !p.comma() {
                    break;
                }
            }
            p.expect(b')')?;
            Geometry::polygon(rings)?
        }
        other => return Err(Error::format("WKT", format!("unsupported geometry `{other}`"))),
    };
    if !g.is_finite() {
        return Err(Error::Geometry("non-finite coordinate".into()));
    }
    Ok(g)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn word(&mut self) -> String {
        self.ws();
        let start = self.i;
        while self.s.get(self.i).is_some_and(u8::is_ascii_alphabetic) {
            self.i += 1;
        }
        String::from_utf8_lossy(self.s.get(start..self.i).unwrap_or_default()).into_owned()
    }

    fn peek_word(&mut self) -> String {
        let at = self.i;
        let w = self.word();
        self.i = at;
        w
    }

    fn expect(&mut self, b: u8) -> Result<()> {
        if self.peek() == Some(b) {
            self.i += 1;
            Ok(())
        } else {
            Err(Error::format("WKT", format!("expected `{}` at byte {}", b as char, self.i)))
        }
    }

    fn comma(&mut self) -> bool {
        if self.peek() == Some(b',') {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn number(&mut self) -> Option<f64> {
        self.ws();
        let start = self.i;
        while self.s.get(self.i).is_some_and(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E')) {
            self.i += 1;
        }
        std::str::from_utf8(self.s.get(start..self.i)?).ok()?.parse().ok()
    }

    /// `x y [z [m]]`.
    fn coord(&mut self) -> Result<Coord> {
        let x = self.number().ok_or_else(|| Error::format("WKT", format!("expected a number at byte {}", self.i)))?;
        let y = self.number().ok_or_else(|| Error::format("WKT", format!("expected a number at byte {}", self.i)))?;
        while self.number().is_some() {}
        Ok(Coord::new(x, y))
    }

    /// `(x y, x y, …)`.
    fn coords(&mut self) -> Result<Vec<Coord>> {
        self.expect(b'(')?;
        let mut out = Vec::new();
        loop {
            out.push(self.coord()?);
            if !self.comma() {
                break;
            }
        }
        self.expect(b')')?;
        Ok(out)
    }

    /// `((…), (…))`.
    fn lists(&mut self) -> Result<Vec<Vec<Coord>>> {
        self.expect(b'(')?;
        let mut out = Vec::new();
        loop {
            out.push(self.coords()?);
            if !self.comma() {
                break;
            }
        }
        self.expect(b')')?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for text in [
            "POINT (1 2)",
            "MULTIPOINT ((1 2), (3 4))",
            "LINESTRING (0 0, 1 1, 2 0)",
            "MULTILINESTRING ((0 0, 1 1), (2 2, 3 3))",
            "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 3 2, 3 3, 2 3, 2 2))",
            "MULTIPOLYGON (((0 0, 1 0, 1 1, 0 0)), ((5 5, 6 5, 6 6, 5 5)))",
        ] {
            assert_eq!(to_wkt(&from_wkt(text).unwrap()), text);
        }
    }

    #[test]
    fn reads_variants_and_rejects_junk() {
        assert_eq!(from_wkt("point z (1 2 3)").unwrap(), Geometry::Point(Coord::new(1.0, 2.0)));
        assert_eq!(from_wkt("MULTIPOINT (1 2, 3 4)").unwrap(), Geometry::Multipoint(vec![Coord::new(1.0, 2.0), Coord::new(3.0, 4.0)]));
        for junk in ["", "POINT", "POINT (1)", "POINT EMPTY", "POLYGON ((0 0, 1 1))", "CIRCLE (1 2)", "POINT (1e999 2)", "LINESTRING ((("] {
            assert!(from_wkt(junk).is_err(), "{junk}");
        }
    }
}
