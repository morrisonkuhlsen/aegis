//! GeoJSON (RFC 7946) in and out (L2). A feature class holds one geometry type, so a file mixing
//! points, lines and polygons is read as its most common family; the rest are counted as skipped.
//! A feature whose geometry can't be read keeps its properties with a null geometry, and a warning
//! counts them: one bad feature never costs the rest of the file (#57). Field types are inferred
//! from the properties. Output is always WGS 84, as the RFC requires.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use mkaegis_core::{Error, Result, Value};
use mkaegis_crs::Crs;
use mkaegis_geom::{Coord, Geometry, GeometryType};
use mkaegis_model::{FeatureClass, Field, FieldType};
use serde_json::{Map as JsonMap, Value as Json, json};

/// The largest GeoJSON text we read.
pub const MAX_BYTES: usize = 1 << 30;
/// How many unreadable geometries a read names one by one; the rest are only counted.
const NAMED_PROBLEMS: usize = 5;

/// A feature class read from GeoJSON, with what the reader had to work around.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub data: FeatureClass,
    pub warnings: Vec<String>,
}

/// One geometry as read: `None` for an empty one (RFC 7946 §3.1 allows empty `coordinates`), and
/// how many polygon rings were left out.
struct Parsed {
    geometry: Option<Geometry>,
    rings_left_out: usize,
}

/// Reads GeoJSON text: a FeatureCollection, a Feature, or a bare geometry.
pub fn read(text: &str, name: &str) -> Result<Read> {
    if text.len() > MAX_BYTES {
        return Err(Error::format("GeoJSON", "file too large"));
    }
    let root: Json = serde_json::from_str(text).map_err(|e| Error::format("GeoJSON", e.to_string()))?;
    let crs = legacy_crs(&root)?;
    let mut warnings = Vec::new();
    let (mut unreadable, mut empty, mut rings_left_out) = (0usize, 0usize, 0usize);
    // A feature's geometry, or null when it can't be read: the feature keeps its properties.
    let mut lenient = |number: usize, g: Option<&Json>| -> Option<Geometry> {
        match g.filter(|g| !g.is_null()).map(geometry) {
            None => None,
            Some(Ok(p)) => {
                rings_left_out += p.rings_left_out;
                empty += usize::from(p.geometry.is_none());
                p.geometry
            }
            Some(Err(e)) => {
                unreadable += 1;
                if unreadable <= NAMED_PROBLEMS {
                    warnings.push(format!("feature {number}: {e}"));
                }
                None
            }
        }
    };
    let features: Vec<(Option<Geometry>, JsonMap<String, Json>)> = match root.get("type").and_then(Json::as_str) {
        Some("FeatureCollection") => {
            let list = root.get("features").and_then(Json::as_array).ok_or_else(|| Error::format("GeoJSON", "FeatureCollection without features"))?;
            let mut out = Vec::with_capacity(list.len());
            for (i, f) in list.iter().enumerate() {
                let props = properties(f)?;
                out.push((lenient(i + 1, f.get("geometry")), props));
            }
            out
        }
        Some("Feature") => {
            let props = properties(&root)?;
            vec![(lenient(1, root.get("geometry")), props)]
        }
        // A bare geometry has nothing else to keep: it reads or it doesn't.
        Some(_) => {
            let p = geometry(&root)?;
            rings_left_out += p.rings_left_out;
            vec![(p.geometry, JsonMap::new())]
        }
        None => return Err(Error::format("GeoJSON", "no `type` member")),
    };
    if unreadable > NAMED_PROBLEMS {
        warnings.push(format!("{unreadable} features in all have a geometry that can't be read"));
    }
    if unreadable > 0 {
        warnings.push(format!("{unreadable} {} with a null geometry, their attributes kept", plural(unreadable, "feature", "features")));
    }
    if empty > 0 {
        warnings.push(format!("{empty} empty {} read as null", plural(empty, "geometry", "geometries")));
    }
    if rings_left_out > 0 {
        warnings.push(format!("{rings_left_out} polygon {} with fewer than 3 vertices left out", plural(rings_left_out, "ring", "rings")));
    }

    // The geometry family most features have.
    let mut counts = [0usize; 3];
    for g in features.iter().filter_map(|(g, _)| g.as_ref()) {
        if let Some(c) = counts.get_mut(family(g.geometry_type())) {
            *c += 1;
        }
    }
    let best = counts.iter().enumerate().max_by_key(|(i, c)| (**c, usize::MAX - i)).map_or(0, |(i, _)| i);
    let has_multi = features.iter().filter_map(|(g, _)| g.as_ref()).any(|g| g.geometry_type() == GeometryType::Multipoint);
    let geometry_type = match best {
        0 if has_multi => GeometryType::Multipoint,
        0 => GeometryType::Point,
        1 => GeometryType::Polyline,
        _ => GeometryType::Polygon,
    };

    let fields = infer_fields(features.iter().map(|(_, p)| p));
    let mut data = FeatureClass::new(name, geometry_type, crs, fields);
    let mut skipped = 0;
    for (g, props) in features {
        if g.as_ref().is_some_and(|g| family(g.geometry_type()) != best) {
            skipped += 1;
            continue;
        }
        let g = match (g, geometry_type) {
            (Some(Geometry::Point(c)), GeometryType::Multipoint) => Some(Geometry::Multipoint(vec![c])),
            (g, _) => g,
        };
        let values = data.fields.iter().map(|f| value(props.get(&f.name), f.kind)).collect();
        data.push(g, values)?;
    }
    if skipped > 0 {
        warnings.insert(0, format!("{skipped} {} of another geometry type skipped", plural(skipped, "feature", "features")));
    }
    Ok(Read { data, warnings })
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

fn family(t: GeometryType) -> usize {
    match t {
        GeometryType::Point | GeometryType::Multipoint => 0,
        GeometryType::Polyline => 1,
        GeometryType::Polygon => 2,
    }
}

/// The pre-RFC `crs` member (`urn:ogc:def:crs:EPSG::3857`, `EPSG:4674`), else WGS 84.
fn legacy_crs(root: &Json) -> Result<Crs> {
    let Some(name) = root.pointer("/crs/properties/name").and_then(Json::as_str) else { return Ok(Crs::WGS84) };
    if name.contains("CRS84") {
        return Ok(Crs::WGS84);
    }
    let code: String = name.rsplit(':').next().unwrap_or_default().chars().filter(char::is_ascii_digit).collect();
    let code = code.parse::<u32>().map_err(|_| Error::Projection(format!("unrecognised crs `{name}`")))?;
    Crs::from_epsg(code)
}

/// A Feature's properties; anything else in `features` makes the file unreadable.
fn properties(f: &Json) -> Result<JsonMap<String, Json>> {
    if f.get("type").and_then(Json::as_str) != Some("Feature") {
        return Err(Error::format("GeoJSON", "a member of `features` is not a Feature"));
    }
    Ok(f.get("properties").and_then(Json::as_object).cloned().unwrap_or_default())
}

fn coord(v: &Json) -> Result<Coord> {
    let a = v.as_array().ok_or_else(|| Error::format("GeoJSON", "a position is not an array"))?;
    match (a.first().and_then(Json::as_f64), a.get(1).and_then(Json::as_f64)) {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok(Coord::new(x, y)),
        _ => Err(Error::format("GeoJSON", "a position needs two numbers")),
    }
}

fn coords(v: &Json) -> Result<Vec<Coord>> {
    v.as_array().ok_or_else(|| Error::format("GeoJSON", "expected an array of positions"))?.iter().map(coord).collect()
}

fn coords2(v: &Json) -> Result<Vec<Vec<Coord>>> {
    v.as_array().ok_or_else(|| Error::format("GeoJSON", "expected an array of arrays"))?.iter().map(coords).collect()
}

/// A geometry. Empty `coordinates` read as an empty geometry; a polygon's hole with fewer than 3
/// vertices is left out, but a polygon whose exterior ring is too short can't be read.
fn geometry(g: &Json) -> Result<Parsed> {
    let c = g.get("coordinates");
    let need = || c.ok_or_else(|| Error::format("GeoJSON", "geometry without coordinates"));
    let kind = g.get("type").and_then(Json::as_str);
    if matches!(kind, Some("Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon"))
        && need()?.as_array().is_some_and(Vec::is_empty)
    {
        return Ok(Parsed { geometry: None, rings_left_out: 0 });
    }
    let whole = |g: Geometry| Parsed { geometry: Some(g), rings_left_out: 0 };
    match kind {
        Some("Point") => Ok(whole(Geometry::Point(coord(need()?)?))),
        Some("MultiPoint") => Ok(whole(Geometry::Multipoint(coords(need()?)?))),
        Some("LineString") => Ok(whole(Geometry::Polyline(vec![coords(need()?)?]))),
        Some("MultiLineString") => Ok(whole(Geometry::Polyline(coords2(need()?)?))),
        Some("Polygon") => {
            let (rings, left_out) = polygon_rings(coords2(need()?)?)?;
            Ok(Parsed { geometry: Some(Geometry::Polygon(rings)), rings_left_out: left_out })
        }
        Some("MultiPolygon") => {
            let polys = need()?.as_array().ok_or_else(|| Error::format("GeoJSON", "MultiPolygon coordinates"))?;
            let (mut rings, mut left_out) = (Vec::new(), 0);
            for p in polys {
                let (r, n) = polygon_rings(coords2(p)?)?;
                rings.extend(r);
                left_out += n;
            }
            Ok(Parsed { geometry: Some(Geometry::Polygon(rings)), rings_left_out: left_out })
        }
        Some("GeometryCollection") => Err(Error::NotAvailable("GeoJSON GeometryCollection".into())),
        Some(other) => Err(Error::format("GeoJSON", format!("unknown geometry type `{other}`"))),
        None => Err(Error::format("GeoJSON", "geometry without `type`")),
    }
}

/// One polygon's rings, closed: the exterior must be a ring, holes too short to be one are left
/// out (and counted). Holes are found by containment, so a hole kept without its exterior would
/// turn into land: a short exterior fails the polygon instead.
fn polygon_rings(rings: Vec<Vec<Coord>>) -> Result<(Vec<Vec<Coord>>, usize)> {
    let mut rings = rings.into_iter();
    let exterior = rings.next().ok_or_else(|| Error::Geometry("a polygon needs at least one ring".into()))?;
    let Ok(Geometry::Polygon(mut kept)) = Geometry::polygon(vec![exterior]) else {
        return Err(Error::Geometry(format!("the exterior ring: {}", mkaegis_geom::SHORT_RING)));
    };
    let holes: Vec<Vec<Coord>> = rings.collect();
    let total = holes.len();
    if let (Some(Geometry::Polygon(h)), _) = Geometry::polygon_lenient(holes) {
        kept.extend(h);
    }
    let left_out = total - (kept.len() - 1);
    Ok((kept, left_out))
}

fn infer_fields<'a>(props: impl Iterator<Item = &'a JsonMap<String, Json>>) -> Vec<Field> {
    let mut fields: Vec<(String, Option<FieldType>)> = Vec::new();
    for p in props {
        for (k, v) in p {
            let kind = match v {
                Json::Null => None,
                Json::Bool(_) => Some(FieldType::Boolean),
                Json::Number(n) if n.is_i64() => Some(FieldType::Integer),
                Json::Number(_) => Some(FieldType::Double),
                _ => Some(FieldType::Text),
            };
            match fields.iter_mut().find(|(n, _)| n == k) {
                None => fields.push((k.clone(), kind)),
                Some((_, have)) => {
                    *have = match (*have, kind) {
                        (h, None) => h,
                        (None, k) => k,
                        (Some(a), Some(b)) if a == b => Some(a),
                        (Some(FieldType::Integer), Some(FieldType::Double)) | (Some(FieldType::Double), Some(FieldType::Integer)) => {
                            Some(FieldType::Double)
                        }
                        _ => Some(FieldType::Text),
                    }
                }
            }
        }
    }
    fields.into_iter().map(|(name, kind)| Field { name, kind: kind.unwrap_or(FieldType::Text) }).collect()
}

fn value(v: Option<&Json>, kind: FieldType) -> Value {
    match (v, kind) {
        (None | Some(Json::Null), _) => Value::Null,
        (Some(Json::Bool(b)), FieldType::Boolean) => Value::Bool(*b),
        (Some(Json::Number(n)), FieldType::Integer) => n.as_i64().map_or(Value::Null, Value::Int),
        (Some(Json::Number(n)), FieldType::Double) => n.as_f64().map_or(Value::Null, Value::Float),
        (Some(Json::String(s)), _) => Value::Text(s.clone()),
        (Some(other), _) => Value::Text(other.to_string()),
    }
}

/// The feature class as a GeoJSON FeatureCollection in WGS 84.
pub fn write(fc: &FeatureClass) -> Result<String> {
    let to84 = |c: Coord| -> Result<Coord> { fc.crs.to_geographic(c) };
    let pos = |c: Coord| -> Result<Json> {
        let g = to84(c)?;
        Ok(json!([g.x, g.y]))
    };
    let list = |cs: &[Coord]| -> Result<Json> { Ok(Json::Array(cs.iter().map(|c| pos(*c)).collect::<Result<_>>()?)) };
    let mut features = Vec::with_capacity(fc.features.len());
    for f in &fc.features {
        let geometry = match &f.geometry {
            None => Json::Null,
            Some(Geometry::Point(c)) => json!({"type": "Point", "coordinates": pos(*c)?}),
            Some(Geometry::Multipoint(cs)) => json!({"type": "MultiPoint", "coordinates": list(cs)?}),
            Some(Geometry::Polyline(parts)) if parts.len() == 1 => {
                json!({"type": "LineString", "coordinates": list(parts.first().map(Vec::as_slice).unwrap_or_default())?})
            }
            Some(Geometry::Polyline(parts)) => {
                json!({"type": "MultiLineString", "coordinates": parts.iter().map(|p| list(p)).collect::<Result<Vec<_>>>()?})
            }
            Some(Geometry::Polygon(rings)) => {
                let polys = mkaegis_geom::wkt::shells_with_holes(rings)
                    .into_iter()
                    .map(|group| {
                        // RFC 7946: shells counter-clockwise, holes clockwise.
                        group.into_iter().enumerate().map(|(i, r)| list(&oriented(r, i == 0))).collect::<Result<Vec<_>>>().map(Json::Array)
                    })
                    .collect::<Result<Vec<_>>>()?;
                if polys.len() == 1 {
                    json!({"type": "Polygon", "coordinates": polys.into_iter().next().unwrap_or(Json::Null)})
                } else {
                    json!({"type": "MultiPolygon", "coordinates": polys})
                }
            }
        };
        let mut props = JsonMap::new();
        for (field, v) in fc.fields.iter().zip(&f.values) {
            let j = match v {
                Value::Null => Json::Null,
                Value::Bool(b) => Json::Bool(*b),
                Value::Int(i) => json!(i),
                Value::Float(x) => serde_json::Number::from_f64(*x).map_or(Json::Null, Json::Number),
                Value::Text(t) => Json::String(t.clone()),
            };
            props.insert(field.name.clone(), j);
        }
        features.push(json!({"type": "Feature", "geometry": geometry, "properties": props}));
    }
    serde_json::to_string(&json!({"type": "FeatureCollection", "name": fc.name, "features": features})).map_err(|e| Error::Internal(e.to_string()))
}

/// The ring turned counter-clockwise (`ccw`) or clockwise.
fn oriented(ring: &[Coord], ccw: bool) -> Vec<Coord> {
    let area2: f64 = ring.windows(2).map(|w| if let [a, b] = w { a.x * b.y - b.x * a.y } else { 0.0 }).sum();
    let mut out = ring.to_vec();
    if (area2 > 0.0) != ccw {
        out.reverse();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[10,0],[10,10],[0,10],[0,0]],[[2,2],[2,3],[3,3],[3,2],[2,2]]]},"properties":{"name":"a","n":1,"x":1.5,"ok":true}},
        {"type":"Feature","geometry":{"type":"MultiPolygon","coordinates":[[[[20,20],[21,20],[21,21],[20,20]]],[[[30,30],[31,30],[31,31],[30,30]]]]},"properties":{"name":"b","n":2.5,"x":null,"tags":[1,2]}},
        {"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{}},
        {"type":"Feature","geometry":null,"properties":{"name":"c"}}
    ]}"#;

    #[test]
    fn reads_the_dominant_family_and_infers_fields() {
        let r = read(SAMPLE, "areas").unwrap();
        assert_eq!(r.data.geometry_type, GeometryType::Polygon);
        assert_eq!(r.warnings, ["1 feature of another geometry type skipped"]);
        assert_eq!(r.data.features.len(), 3);
        let kinds: Vec<_> = r.data.fields.iter().map(|f| (f.name.as_str(), f.kind)).collect();
        assert_eq!(
            kinds,
            [("name", FieldType::Text), ("n", FieldType::Double), ("x", FieldType::Double), ("ok", FieldType::Boolean), ("tags", FieldType::Text)]
        );
        assert_eq!(r.data.features[1].values[4], Value::Text("[1,2]".into()));
        assert!((mkaegis_geom::algo::area(r.data.features[0].geometry.as_ref().unwrap()) - 99.0).abs() < 1e-12);
    }

    #[test]
    fn round_trips_through_write() {
        let r = read(SAMPLE, "areas").unwrap();
        let again = read(&write(&r.data).unwrap(), "areas").unwrap();
        assert_eq!(again.data.features.len(), 3);
        assert_eq!(
            again.data.features[0].geometry,
            r.data.features[0].geometry.as_ref().map(|g| {
                // Written shells counter-clockwise, holes clockwise.
                let Geometry::Polygon(rings) = g else { panic!("not a polygon") };
                Geometry::Polygon(vec![oriented(&rings[0], true), oriented(&rings[1], false)])
            })
        );
    }

    #[test]
    fn legacy_crs_and_reprojection() {
        let text = r#"{"type":"Feature","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::3857"}},"geometry":{"type":"Point","coordinates":[20037508.342789244,0]},"properties":null}"#;
        let r = read(text, "p").unwrap();
        assert_eq!(r.data.crs, Crs::WEB_MERCATOR);
        let out: Json = serde_json::from_str(&write(&r.data).unwrap()).unwrap();
        let x = out.pointer("/features/0/geometry/coordinates/0").and_then(Json::as_f64).unwrap();
        assert!((x - 180.0).abs() < 1e-9);
    }

    #[test]
    fn a_bad_geometry_costs_only_its_own_shape() {
        let text = r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[4,0],[4,4],[0,0]]]},"properties":{"id":1}},
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[1,1]]]},"properties":{"id":2}},
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[]},"properties":{"id":3}},
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[4,0],[4,4],[0,0]],[[1,1],[2,1]]]},"properties":{"id":4}},
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],["x",0],[4,4],[0,0]]]},"properties":{"id":5}}
        ]}"#;
        let r = read(text, "lots").unwrap();
        let ids: Vec<&Value> = r.data.features.iter().map(|f| &f.values[0]).collect();
        assert_eq!(ids, [&Value::Int(1), &Value::Int(2), &Value::Int(3), &Value::Int(4), &Value::Int(5)], "every feature kept");
        let has_shape: Vec<bool> = r.data.features.iter().map(|f| f.geometry.is_some()).collect();
        assert_eq!(has_shape, [true, false, false, true, false]);
        assert_eq!(
            r.warnings,
            [
                "feature 2: geometry: the exterior ring: a polygon ring needs at least 3 vertices",
                "feature 5: cannot read GeoJSON: a position needs two numbers",
                "2 features with a null geometry, their attributes kept",
                "1 empty geometry read as null",
                "1 polygon ring with fewer than 3 vertices left out",
            ]
        );
        // The hole that was too short is gone; the polygon is whole.
        assert_eq!(r.data.features[3].geometry, r.data.features[0].geometry);
        // A Feature on its own is just as lenient; a bare geometry has nothing else to keep.
        let one = read(r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[1]},"properties":{"a":1}}"#, "p").unwrap();
        assert_eq!((one.data.features.len(), one.data.features[0].geometry.is_none()), (1, true));
        let empty = read(r#"{"type":"Point","coordinates":[]}"#, "p").unwrap();
        assert!(empty.data.features[0].geometry.is_none());
    }

    #[test]
    fn hostile_input_is_an_error() {
        for bad in [
            "",
            "{}",
            "[]",
            r#"{"type":"FeatureCollection"}"#,
            r#"{"type":"Point","coordinates":[1]}"#,
            r#"{"type":"Point","coordinates":"x"}"#,
            r#"{"type":"Polygon","coordinates":[[[0,0],[1,1]]]}"#,
            r#"{"type":"Feature","crs":{"properties":{"name":"EPSG:9999"}},"geometry":null}"#,
        ] {
            assert!(read(bad, "x").is_err(), "{bad}");
        }
        let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        assert!(read(&deep, "x").is_err());
    }
}
