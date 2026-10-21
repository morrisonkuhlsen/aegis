//! Geoprocessing tools (L3), named after the toolboxes': Buffer (Analysis), Project and Calculate
//! Geometry Attributes (Data Management), Select Layer By Location, Summary Statistics.
//!
//! Distances and areas are in metres. Data in a geographic CRS is measured in the UTM zone of
//! each feature (good to a fraction of a percent inside a zone); geodesic measures come later.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use mkaegis_core::{Error, Result, Value};
use mkaegis_crs::Crs;
use mkaegis_geom::{Coord, Geometry, GeometryType, algo};
use mkaegis_model::{FeatureClass, Field, FieldType};
use serde::{Deserialize, Serialize};

/// The largest buffer distance (metres): a hemisphere.
const MAX_DISTANCE: f64 = 2.0e7;

/// The geometry in a metric CRS for measuring: itself if projected, else in its UTM zone.
fn metric(g: &Geometry, crs: Crs) -> Result<(Geometry, Crs)> {
    if !crs.is_geographic() {
        return Ok((g.clone(), crs));
    }
    let c = algo::centroid(g).or_else(|| g.coords().next()).ok_or_else(|| Error::Geometry("empty geometry".into()))?;
    let utm = Crs::utm_for(c.x, c.y.clamp(-80.0, 84.0))?;
    Ok((g.try_map(|p| crs.transform(utm, p))?, utm))
}

/// Buffer: a polygon around each point at `distance` metres (Null shapes stay Null).
pub fn buffer(fc: &FeatureClass, distance: f64, name: &str) -> Result<FeatureClass> {
    if !distance.is_finite() || distance <= 0.0 || distance > MAX_DISTANCE {
        return Err(Error::param("distance", format!("must be a positive number of metres up to {MAX_DISTANCE}")));
    }
    if !matches!(fc.geometry_type, GeometryType::Point | GeometryType::Multipoint) {
        return Err(Error::NotAvailable(format!("Buffer of {} features (points only so far)", fc.geometry_type.name().to_lowercase())));
    }
    let mut out = FeatureClass::new(name, GeometryType::Polygon, fc.crs, fc.fields.clone());
    let distance_field = out.add_field(Field::new("BUFF_DIST", FieldType::Double))?;
    for f in &fc.features {
        let mut values = f.values.clone();
        values.resize(distance_field, Value::Null);
        values.push(Value::Float(distance));
        let Some(g) = &f.geometry else {
            out.push(None, values)?;
            continue;
        };
        let (m, mcrs) = metric(g, fc.crs)?;
        let rings: Vec<Vec<Coord>> = m.coords().map(|c| algo::circle(c, distance, 72)).collect();
        let back = Geometry::polygon(rings)?.try_map(|p| mcrs.transform(fc.crs, p))?;
        out.push(Some(back), values)?;
    }
    Ok(out)
}

/// Project: the feature class in another CRS.
pub fn project(fc: &FeatureClass, to: Crs, name: &str) -> Result<FeatureClass> {
    let mut out = FeatureClass::new(name, fc.geometry_type, to, fc.fields.clone());
    for (i, f) in fc.features.iter().enumerate() {
        let g = match &f.geometry {
            Some(g) => Some(g.try_map(|c| fc.crs.transform(to, c)).map_err(|e| Error::Projection(format!("feature {}: {e}", i + 1)))?),
            None => None,
        };
        out.push(g, f.values.clone())?;
    }
    Ok(out)
}

/// What Calculate Geometry Attributes computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Property {
    /// Square metres.
    Area,
    /// Hectares.
    AreaHectares,
    /// Square kilometres.
    AreaKm2,
    /// Metres (perimeter for polygons).
    Length,
    /// Kilometres.
    LengthKm,
    /// Centroid x / y in the data's CRS.
    CentroidX,
    CentroidY,
}

impl Property {
    pub fn parse(text: &str) -> Option<Property> {
        match text.to_ascii_lowercase().replace(['_', ' '], "").as_str() {
            "area" | "areasqm" | "aream2" => Some(Property::Area),
            "areahectares" | "areaha" | "hectares" => Some(Property::AreaHectares),
            "areakm2" | "areasqkm" => Some(Property::AreaKm2),
            "length" | "perimeter" | "lengthm" => Some(Property::Length),
            "lengthkm" => Some(Property::LengthKm),
            "centroidx" | "x" => Some(Property::CentroidX),
            "centroidy" | "y" => Some(Property::CentroidY),
            _ => None,
        }
    }
}

/// One feature's property.
pub fn measure(g: &Geometry, crs: Crs, p: Property) -> Result<f64> {
    match p {
        Property::CentroidX | Property::CentroidY => {
            let c = algo::centroid(g).ok_or_else(|| Error::Geometry("no centroid".into()))?;
            Ok(if p == Property::CentroidX { c.x } else { c.y })
        }
        _ => {
            let (m, _) = metric(g, crs)?;
            Ok(match p {
                Property::Area => algo::area(&m),
                Property::AreaHectares => algo::area(&m) / 1e4,
                Property::AreaKm2 => algo::area(&m) / 1e6,
                Property::Length => algo::length(&m),
                _ => algo::length(&m) / 1e3,
            })
        }
    }
}

/// Calculate Geometry Attributes: writes `property` into `field` (added as Double if missing).
/// Returns how many rows were computed.
pub fn calculate_geometry(fc: &mut FeatureClass, field: &str, property: Property) -> Result<usize> {
    let i = match fc.field_index(field) {
        Some(i) => {
            if !matches!(fc.fields.get(i).map(|f| f.kind), Some(FieldType::Double | FieldType::Integer)) {
                return Err(Error::param("field", format!("`{field}` is not numeric")));
            }
            i
        }
        None => fc.add_field(Field::new(field, FieldType::Double))?,
    };
    let integer = fc.fields.get(i).is_some_and(|f| f.kind == FieldType::Integer);
    let crs = fc.crs;
    let mut done = 0;
    for f in &mut fc.features {
        let v = match f.geometry.as_ref().map(|g| measure(g, crs, property)) {
            Some(Ok(x)) if integer => Value::Int(x.round() as i64),
            Some(Ok(x)) => Value::Float(x),
            _ => Value::Null,
        };
        if !v.is_null() {
            done += 1;
        }
        if let Some(slot) = f.values.get_mut(i) {
            *slot = v;
        }
    }
    Ok(done)
}

/// How Select Layer By Location relates features.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Relation {
    Intersect,
    WithinDistance(f64),
    Within,
    Contains,
}

impl Relation {
    pub fn parse(text: &str, distance: Option<f64>) -> Result<Relation> {
        match text.to_ascii_lowercase().replace(['_', ' '], "").as_str() {
            "" | "intersect" | "intersects" => Ok(Relation::Intersect),
            "within" | "completelywithin" | "havetheircenterin" => Ok(Relation::Within),
            "contains" | "completelycontains" => Ok(Relation::Contains),
            "withinadistance" | "withindistance" => {
                let d = distance.filter(|d| d.is_finite() && *d >= 0.0 && *d <= MAX_DISTANCE);
                Ok(Relation::WithinDistance(d.ok_or_else(|| Error::param("distance", "a distance in metres is needed"))?))
            }
            other => Err(Error::param("relation", format!("unknown relation `{other}`"))),
        }
    }
}

/// Select Layer By Location: the rows of `target` related to any of `selecting`'s `rows`
/// (all rows when `None`).
pub fn select_by_location(target: &FeatureClass, selecting: &FeatureClass, rows: Option<&[usize]>, relation: Relation) -> Result<Vec<usize>> {
    // The selecting shapes, in the target's CRS.
    let mut shapes = Vec::new();
    let all: Vec<usize>;
    let rows = match rows {
        Some(r) => r,
        None => {
            all = (0..selecting.features.len()).collect();
            &all
        }
    };
    for &r in rows {
        if let Some(g) = selecting.features.get(r).and_then(|f| f.geometry.as_ref()) {
            shapes.push(g.try_map(|c| selecting.crs.transform(target.crs, c))?);
        }
    }
    let envelopes: Vec<_> = shapes.iter().map(Geometry::envelope).collect();
    // Within a distance: in metres, converted to the target's units near its middle.
    let tolerance = match relation {
        Relation::WithinDistance(d) => {
            let lat = target.extent().and_then(|e| target.crs.to_geographic(e.center()).ok()).map_or(0.0, |g| g.y);
            d / target.crs.metres_per_unit(lat)
        }
        _ => 0.0,
    };
    let mut out = Vec::new();
    for (row, f) in target.features.iter().enumerate() {
        let Some(g) = &f.geometry else { continue };
        let Some(env) = g.envelope() else { continue };
        let hit = shapes.iter().zip(&envelopes).any(|(s, se)| {
            if !se.is_some_and(|se| se.buffer(tolerance).intersects(&env)) {
                return false;
            }
            match relation {
                Relation::Intersect => algo::intersects(g, s, 0.0),
                Relation::WithinDistance(_) => algo::intersects(g, s, tolerance),
                Relation::Within => algo::within(g, s),
                Relation::Contains => algo::within(s, g),
            }
        });
        if hit {
            out.push(row);
        }
    }
    Ok(out)
}

/// Summary Statistics of a numeric field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Statistics {
    pub field: String,
    pub count: usize,
    pub nulls: usize,
    pub sum: f64,
    pub mean: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Sample standard deviation (n − 1).
    pub std: Option<f64>,
}

/// Summary Statistics over `rows` (all when `None`), of a numeric field: on a Text field every
/// value would count as missing (#4), so it is refused.
pub fn summary_statistics(fc: &FeatureClass, field: &str, rows: Option<&[usize]>) -> Result<Statistics> {
    let i = fc.field_index(field).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
    if let Some(f) = fc.fields.get(i).filter(|f| !f.kind.is_numeric()) {
        return Err(Error::param(
            "field",
            format!("`{}` is a {} field; Summary Statistics needs a numeric one (Short, Long or Double)", f.name, f.kind.name()),
        ));
    }
    let pick: Box<dyn Iterator<Item = usize>> = match rows {
        Some(r) => Box::new(r.iter().copied()),
        None => Box::new(0..fc.features.len()),
    };
    let (mut values, mut nulls) = (Vec::new(), 0);
    for r in pick {
        match fc.value(r, i).and_then(Value::as_f64) {
            Some(x) => values.push(x),
            None => nulls += 1,
        }
    }
    let n = values.len();
    // From +0: an empty f64 sum is -0, which would print as `-0.0`.
    let sum = values.iter().fold(0.0, |a, x| a + x);
    let mean = (n > 0).then(|| sum / n as f64);
    // Two-pass variance: stable for large offsets.
    let std = mean.filter(|_| n > 1).map(|m| (values.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt());
    Ok(Statistics {
        field: fc.fields.get(i).map(|f| f.name.clone()).unwrap_or_default(),
        count: n,
        nulls,
        sum,
        mean,
        min: values.iter().copied().reduce(f64::min),
        max: values.iter().copied().reduce(f64::max),
        std,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cities() -> FeatureClass {
        let mut fc = FeatureClass::new("c", GeometryType::Point, Crs::WGS84, vec![Field::new("POP", FieldType::Integer)]);
        for (x, y, p) in [(-46.63, -23.55, 12), (-46.33, -23.96, 4), (-43.2, -22.9, 6)] {
            fc.push(Some(Geometry::Point(Coord::new(x, y))), vec![Value::Int(p)]).unwrap();
        }
        fc.push(None, vec![]).unwrap();
        fc
    }

    #[test]
    fn buffer_is_a_circle_of_the_right_area() {
        let b = buffer(&cities(), 1000.0, "b").unwrap();
        assert_eq!(b.geometry_type, GeometryType::Polygon);
        assert_eq!(b.features.len(), 4);
        assert_eq!(b.features[3].geometry, None);
        let area = measure(b.features[0].geometry.as_ref().unwrap(), b.crs, Property::Area).unwrap();
        // A 72-gon inscribed in a 1 km circle: π·r² × sin(2π/72)·72/(2π).
        let expected = 0.5 * 72.0 * 1000.0f64.powi(2) * (std::f64::consts::TAU / 72.0).sin();
        assert!((area - expected).abs() / expected < 1e-3, "{area} vs {expected}");
        assert!(buffer(&cities(), -1.0, "b").is_err());
        assert!(buffer(&cities(), f64::NAN, "b").is_err());
    }

    #[test]
    fn project_and_back() {
        let utm = project(&cities(), Crs::from_epsg(31983).unwrap(), "p").unwrap();
        let back = project(&utm, Crs::WGS84, "b").unwrap();
        let (Some(Geometry::Point(a)), Some(Geometry::Point(b))) = (&cities().features[0].geometry, &back.features[0].geometry) else { panic!() };
        assert!((a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9);
    }

    #[test]
    fn calculate_geometry_in_metres_from_degrees() {
        // A 0.01° × 0.01° cell at the equator: about 1113 m × 1106 m.
        let mut fc = FeatureClass::new("cell", GeometryType::Polygon, Crs::WGS84, vec![]);
        let ring = vec![Coord::new(0.0, 0.0), Coord::new(0.01, 0.0), Coord::new(0.01, 0.01), Coord::new(0.0, 0.01), Coord::new(0.0, 0.0)];
        fc.push(Some(Geometry::Polygon(vec![ring])), vec![]).unwrap();
        assert_eq!(calculate_geometry(&mut fc, "HA", Property::AreaHectares).unwrap(), 1);
        let ha = fc.features[0].values[0].as_f64().unwrap();
        assert!((ha - 123.1).abs() < 0.5, "{ha}");
        assert!(calculate_geometry(&mut fc, "bad name", Property::Area).is_err());
    }

    #[test]
    fn select_by_location_and_statistics() {
        let c = cities();
        let zone = buffer(&FeatureClass { features: vec![c.features[0].clone()], ..c.clone() }, 60_000.0, "z").unwrap();
        assert_eq!(select_by_location(&c, &zone, None, Relation::Intersect).unwrap(), vec![0, 1]);
        assert_eq!(select_by_location(&c, &zone, Some(&[]), Relation::Intersect).unwrap(), Vec::<usize>::new());
        let near = select_by_location(&c, &c, Some(&[0]), Relation::WithinDistance(400_000.0)).unwrap();
        assert_eq!(near, vec![0, 1, 2]);
        let s = summary_statistics(&c, "pop", None).unwrap();
        assert_eq!((s.count, s.nulls, s.sum, s.min, s.max), (3, 1, 22.0, Some(4.0), Some(12.0)));
        // Sample variance of 12, 4, 6: (14²/9 + 10²/9 + 4²/9) / 2 = 52/3.
        assert!((s.std.unwrap() - (52.0f64 / 3.0).sqrt()).abs() < 1e-12);
        assert!(summary_statistics(&c, "nope", None).is_err());
    }

    /// #4: a Text field reported every value as null and a sum of -0.0.
    #[test]
    fn statistics_need_a_numeric_field_and_never_report_negative_zero() {
        let mut fc =
            FeatureClass::new("t", GeometryType::Point, Crs::WGS84, vec![Field::new("NAME", FieldType::Text), Field::new("X", FieldType::Double)]);
        for name in ["a", "b"] {
            fc.push(None, vec![Value::Text(name.into()), Value::Null]).unwrap();
        }
        let err = summary_statistics(&fc, "NAME", None).unwrap_err().to_string();
        assert!(err.contains("Text") && err.contains("numeric"), "{err}");
        // A numeric field with only nulls: nothing counted, a sum of +0.
        let s = summary_statistics(&fc, "X", None).unwrap();
        assert_eq!((s.count, s.nulls, s.mean, s.min, s.std), (0, 2, None, None, None));
        assert!(s.sum == 0.0 && s.sum.is_sign_positive(), "{}", s.sum);
        assert!(summary_statistics(&fc, "X", Some(&[])).unwrap().sum.is_sign_positive());
    }
}
