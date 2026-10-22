//! KML 2.2 (OGC 07-147r2) in and out (L2). A document's Placemarks, in any Document or Folder,
//! become feature classes in WGS 84 (KML's only coordinate system); styles, overlays, network
//! links and time are not read. A feature class holds one geometry type and KML documents mix
//! them freely (a parcel's polygon with its vertices as points, as SIGEF exports it), so each
//! family present becomes its own layer: `<name>_Polygons`, `<name>_Lines`, `<name>_Points`, or
//! just `<name>` when there is one. A Placemark whose geometry can't be read keeps its attributes
//! with a null geometry, in the layer of the most common family.
//!
//! Attributes are `name`, `description` and the `ExtendedData` (`Data` and `SchemaData`), their
//! types inferred from the values. The reader streams: only the Placemark being read is held as a
//! tree, so elements around them cost nothing.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fmt::Write as _;

use mkaegis_core::{Error, Result, Value};
use mkaegis_crs::Crs;
use mkaegis_geom::{Coord, Geometry, GeometryType};
use mkaegis_model::{FeatureClass, Field, FieldType};
use quick_xml::escape::{escape, resolve_predefined_entity};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

/// The largest KML text read.
pub const MAX_BYTES: usize = 1 << 30;
/// The deepest element nesting read (Folders in Folders, MultiGeometries in MultiGeometries).
const MAX_DEPTH: usize = 256;
/// The most elements one Placemark may hold.
const MAX_PLACEMARK_ELEMENTS: usize = 10_000_000;
/// How many unreadable geometries a read names one by one; the rest are only counted.
const NAMED_PROBLEMS: usize = 5;
/// The elements a Placemark is read from; anything else in it (styles, time, gx: tracks) is
/// skipped with its content.
const READ: [&str; 15] = [
    "name",
    "description",
    "ExtendedData",
    "Data",
    "value",
    "SchemaData",
    "SimpleData",
    "Point",
    "LineString",
    "LinearRing",
    "Polygon",
    "outerBoundaryIs",
    "innerBoundaryIs",
    "coordinates",
    "MultiGeometry",
];

/// The feature classes read from KML, polygons first, with what the reader had to work around.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub layers: Vec<FeatureClass>,
    pub warnings: Vec<String>,
}

/// One element inside a Placemark: its local name (no namespace prefix), its `name` attribute
/// (`Data` and `SimpleData` have one), its text and its children.
#[derive(Debug, Default)]
struct Node {
    name: String,
    name_attr: Option<String>,
    text: String,
    children: Vec<Node>,
}

/// A Placemark as read: its attributes, and its geometry or why it couldn't be read.
struct Row {
    name: Option<String>,
    description: Option<String>,
    data: Vec<(String, String)>,
    geometry: Result<Parsed>,
}

/// A geometry: `None` for an empty one, and how many polygon rings were left out.
struct Parsed {
    geometry: Option<Geometry>,
    rings_left_out: usize,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// A child's text, trimmed, if it has any.
    fn child_text(&self, name: &str) -> Option<String> {
        self.child(name).map(|c| c.text.trim().to_string()).filter(|t| !t.is_empty())
    }
}

/// Reads KML text (UTF-8, as KML 2.2 requires).
pub fn read(text: &str, name: &str) -> Result<Read> {
    if text.len() > MAX_BYTES {
        return Err(Error::format("KML", "file too large"));
    }
    let rows = placemarks(text)?;
    let mut warnings = Vec::new();

    let (mut unreadable, mut empty, mut rings_left_out) = (0usize, 0usize, 0usize);
    let mut shapes: Vec<Option<Geometry>> = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        shapes.push(match &row.geometry {
            Ok(p) => {
                rings_left_out += p.rings_left_out;
                empty += usize::from(p.geometry.is_none() && p.rings_left_out == 0);
                p.geometry.clone()
            }
            Err(e) => {
                unreadable += 1;
                if unreadable <= NAMED_PROBLEMS {
                    warnings.push(format!("placemark {}: {e}", i + 1));
                }
                None
            }
        });
    }

    // The families present; placemarks without a geometry go with the most common one.
    let mut counts = [0usize; 3];
    for g in shapes.iter().flatten() {
        if let Some(c) = counts.get_mut(family(g.geometry_type())) {
            *c += 1;
        }
    }
    let best = counts.iter().enumerate().max_by_key(|(i, c)| (**c, usize::MAX - i)).map_or(0, |(i, _)| i);
    let families: Vec<usize> = [2, 1, 0].into_iter().filter(|f| *f == best || counts.get(*f).is_some_and(|c| *c > 0)).collect();
    let of = |g: &Option<Geometry>| g.as_ref().map_or(best, |g| family(g.geometry_type()));
    let mut layers = Vec::with_capacity(families.len());
    for f in families.iter().copied() {
        let members: Vec<(&Row, &Option<Geometry>)> = rows.iter().zip(&shapes).filter(|(_, g)| of(g) == f).collect();
        let has_multi = members.iter().any(|(_, g)| g.as_ref().is_some_and(|g| g.geometry_type() == GeometryType::Multipoint));
        let (geometry_type, label) = match f {
            0 if has_multi => (GeometryType::Multipoint, "Points"),
            0 => (GeometryType::Point, "Points"),
            1 => (GeometryType::Polyline, "Lines"),
            _ => (GeometryType::Polygon, "Polygons"),
        };
        let columns = columns(members.iter().map(|(r, _)| *r));
        let fields: Vec<Field> = columns.iter().map(|c| Field::new(&c.name, c.kind)).collect();
        let layer_name = if families.len() == 1 { name.to_string() } else { format!("{name}_{label}") };
        let mut data = FeatureClass::new(&layer_name, geometry_type, Crs::WGS84, fields);
        for (row, g) in members {
            let g = match (g.clone(), geometry_type) {
                (Some(Geometry::Point(c)), GeometryType::Multipoint) => Some(Geometry::Multipoint(vec![c])),
                (g, _) => g,
            };
            let values = columns.iter().map(|c| value((c.get)(row).as_deref(), c.kind)).collect();
            data.push(g, values)?;
        }
        layers.push(data);
    }

    if unreadable > NAMED_PROBLEMS {
        warnings.push(format!("{unreadable} placemarks in all have a geometry that can't be read"));
    }
    if unreadable > 0 {
        warnings.push(format!("{unreadable} {} with a null geometry, their attributes kept", plural(unreadable, "placemark", "placemarks")));
    }
    if empty > 0 {
        warnings.push(format!("{empty} {} without a geometry read as null", plural(empty, "placemark", "placemarks")));
    }
    if rings_left_out > 0 {
        warnings.push(format!("{rings_left_out} polygon {} with fewer than 3 vertices left out", plural(rings_left_out, "ring", "rings")));
    }
    if rows.is_empty() {
        warnings.push("no Placemark in the document".into());
    }
    Ok(Read { layers, warnings })
}

/// The feature class as a KML 2.2 document in WGS 84: a Placemark per feature, its `name` and
/// `description` from fields so named (any case), every other field in `ExtendedData`.
pub fn write(fc: &FeatureClass) -> Result<String> {
    let find = |n: &str| fc.fields.iter().position(|f| f.name.eq_ignore_ascii_case(n));
    let (name_at, description_at) = (find("name"), find("description"));
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n<Document>\n");
    let _ = writeln!(out, "<name>{}</name>", escape(fc.name.as_str()));
    for f in &fc.features {
        out.push_str("<Placemark>");
        for (at, tag) in [(name_at, "name"), (description_at, "description")] {
            if let Some(text) = at.and_then(|i| f.values.get(i)).and_then(text_of) {
                let _ = write!(out, "<{tag}>{}</{tag}>", escape(text.as_str()));
            }
        }
        let data: Vec<(&Field, String)> = fc
            .fields
            .iter()
            .zip(&f.values)
            .enumerate()
            .filter(|(i, _)| Some(*i) != name_at && Some(*i) != description_at)
            .filter_map(|(_, (field, v))| text_of(v).map(|t| (field, t)))
            .collect();
        if !data.is_empty() {
            out.push_str("<ExtendedData>");
            for (field, t) in data {
                let _ = write!(out, "<Data name=\"{}\"><value>{}</value></Data>", escape(field.name.as_str()), escape(t.as_str()));
            }
            out.push_str("</ExtendedData>");
        }
        if let Some(g) = &f.geometry {
            write_geometry(&mut out, g, fc.crs)?;
        }
        out.push_str("</Placemark>\n");
    }
    out.push_str("</Document>\n</kml>\n");
    Ok(out)
}

/// Every Placemark in the document, read as it ends.
fn placemarks(text: &str) -> Result<Vec<Row>> {
    let xml = |e: quick_xml::Error| Error::format("KML", e.to_string());
    let mut reader = Reader::from_str(text);
    let mut rows = Vec::new();
    let mut depth = 0usize;
    let mut root_seen = false;
    // Inside a Placemark: the elements open, the Placemark first.
    let mut open: Vec<Node> = Vec::new();
    let mut elements = 0usize;
    // Inside an element a Placemark doesn't read: how deep, to skip to its end.
    let mut skipping = 0usize;
    loop {
        match reader.read_event().map_err(xml)? {
            Event::Start(e) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(Error::format("KML", format!("elements nested more than {MAX_DEPTH} deep")));
                }
                let name = local_name(&e)?;
                if !root_seen {
                    root_seen = true;
                    if name != "kml" {
                        return Err(Error::format("KML", format!("not a KML document (it starts with <{name}>)")));
                    }
                }
                if skipping > 0 {
                    skipping += 1;
                } else if open.is_empty() {
                    if name == "Placemark" {
                        open.push(Node { name, ..Node::default() });
                        elements = 1;
                    }
                } else if READ.contains(&name.as_str()) {
                    elements += 1;
                    if elements > MAX_PLACEMARK_ELEMENTS {
                        return Err(Error::format("KML", format!("a Placemark with more than {MAX_PLACEMARK_ELEMENTS} elements")));
                    }
                    open.push(Node { name, name_attr: name_attribute(&e)?, ..Node::default() });
                } else {
                    skipping = 1;
                }
            }
            Event::Empty(e) => {
                let name = local_name(&e)?;
                if !root_seen {
                    root_seen = true;
                    if name != "kml" {
                        return Err(Error::format("KML", format!("not a KML document (it starts with <{name}/>)")));
                    }
                }
                if skipping == 0
                    && let Some(parent) = open.last_mut()
                    && READ.contains(&name.as_str())
                {
                    parent.children.push(Node { name, name_attr: name_attribute(&e)?, ..Node::default() });
                }
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if skipping > 0 {
                    skipping -= 1;
                } else if let Some(done) = open.pop() {
                    match open.last_mut() {
                        Some(parent) => parent.children.push(done),
                        None => rows.push(row(&done)),
                    }
                }
            }
            Event::Text(t) => {
                if skipping == 0
                    && let Some(top) = open.last_mut()
                {
                    top.text.push_str(&t.xml10_content().map_err(|e| Error::format("KML", e.to_string()))?);
                }
            }
            Event::CData(c) => {
                if skipping == 0
                    && let Some(top) = open.last_mut()
                {
                    top.text.push_str(&c.decode().map_err(|e| Error::format("KML", e.to_string()))?);
                }
            }
            Event::GeneralRef(r) => {
                if skipping == 0
                    && let Some(top) = open.last_mut()
                {
                    match r.resolve_char_ref().map_err(xml)? {
                        Some(c) => top.text.push(c),
                        None => {
                            let entity = r.decode().map_err(|e| Error::format("KML", e.to_string()))?;
                            // Entities a DTD declares are never expanded: kept as written.
                            match resolve_predefined_entity(&entity) {
                                Some(s) => top.text.push_str(s),
                                None => {
                                    let _ = write!(top.text, "&{entity};");
                                }
                            }
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen {
        return Err(Error::format("KML", "no <kml> element"));
    }
    if depth != 0 || !open.is_empty() {
        return Err(Error::format("KML", "the document ends inside an element (cut short?)"));
    }
    Ok(rows)
}

fn local_name(e: &BytesStart) -> Result<String> {
    std::str::from_utf8(e.local_name().as_ref()).map(str::to_string).map_err(|_| Error::format("KML", "an element name is not UTF-8"))
}

/// The `name` attribute, which `Data` and `SimpleData` carry.
fn name_attribute(e: &BytesStart) -> Result<Option<String>> {
    for a in e.attributes() {
        let a = a.map_err(|e| Error::format("KML", e.to_string()))?;
        if a.key.local_name().as_ref() == b"name" {
            return Ok(Some(a.normalized_value(XmlVersion::Implicit1_0).map_err(|e| Error::format("KML", e.to_string()))?.into_owned()));
        }
    }
    Ok(None)
}

fn row(placemark: &Node) -> Row {
    let mut data: Vec<(String, String)> = Vec::new();
    let mut set = |key: &str, value: String| match data.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => *v = value,
        None => data.push((key.to_string(), value)),
    };
    if let Some(extended) = placemark.child("ExtendedData") {
        for d in extended.children_named("Data") {
            if let Some(key) = d.name_attr.as_deref() {
                set(key, d.child("value").map(|v| v.text.trim().to_string()).unwrap_or_default());
            }
        }
        for schema in extended.children_named("SchemaData") {
            for d in schema.children_named("SimpleData") {
                if let Some(key) = d.name_attr.as_deref() {
                    set(key, d.text.trim().to_string());
                }
            }
        }
    }
    let shape = placemark.children.iter().find(|c| is_geometry(&c.name));
    Row {
        name: placemark.child_text("name"),
        description: placemark.child_text("description"),
        data,
        geometry: shape.map_or(Ok(Parsed { geometry: None, rings_left_out: 0 }), geometry),
    }
}

fn is_geometry(name: &str) -> bool {
    matches!(name, "Point" | "LineString" | "LinearRing" | "Polygon" | "MultiGeometry")
}

/// A geometry element. Empty `coordinates` read as an empty geometry; a polygon's hole with
/// fewer than 3 vertices is left out, but a polygon whose outer ring is too short can't be read.
fn geometry(n: &Node) -> Result<Parsed> {
    let whole = |g: Option<Geometry>| Parsed { geometry: g, rings_left_out: 0 };
    match n.name.as_str() {
        "Point" => match coordinates(n)?.as_slice() {
            [] => Ok(whole(None)),
            [c] => Ok(whole(Some(Geometry::Point(*c)))),
            _ => Err(Error::Geometry("a Point with more than one position".into())),
        },
        "LineString" => match coordinates(n)? {
            cs if cs.is_empty() => Ok(whole(None)),
            cs if cs.len() < 2 => Err(Error::Geometry("a LineString needs at least 2 positions".into())),
            cs => Ok(whole(Some(Geometry::Polyline(vec![cs])))),
        },
        "LinearRing" => match coordinates(n)? {
            cs if cs.is_empty() => Ok(whole(None)),
            cs => Ok(whole(Some(Geometry::polygon(vec![cs])?))),
        },
        "Polygon" => polygon(n),
        "MultiGeometry" => {
            let (mut parts, mut left_out) = (Vec::new(), 0);
            for c in n.children.iter().filter(|c| is_geometry(&c.name)) {
                let p = geometry(c)?;
                left_out += p.rings_left_out;
                parts.extend(p.geometry);
            }
            Ok(Parsed { geometry: merge(parts)?, rings_left_out: left_out })
        }
        other => Err(Error::Geometry(format!("unknown geometry <{other}>"))),
    }
}

/// A Polygon: its outer ring must be one, holes too short to be one are left out (and counted).
/// Holes are found by containment, so a hole kept without its outer ring would turn into land.
fn polygon(n: &Node) -> Result<Parsed> {
    let ring = |boundary: &Node| -> Result<Vec<Vec<Coord>>> { boundary.children_named("LinearRing").map(coordinates).collect() };
    let outer = n.child("outerBoundaryIs").ok_or_else(|| Error::Geometry("a Polygon without outerBoundaryIs".into()))?;
    let exterior = ring(outer)?.into_iter().next().unwrap_or_default();
    if exterior.is_empty() {
        return Ok(Parsed { geometry: None, rings_left_out: 0 });
    }
    let Ok(Geometry::Polygon(mut rings)) = Geometry::polygon(vec![exterior]) else {
        return Err(Error::Geometry(format!("the outer ring: {}", mkaegis_geom::SHORT_RING)));
    };
    let holes: Vec<Vec<Coord>> = n.children_named("innerBoundaryIs").map(ring).collect::<Result<Vec<_>>>()?.into_iter().flatten().collect();
    let total = holes.len();
    if let (Some(Geometry::Polygon(kept)), _) = Geometry::polygon_lenient(holes) {
        rings.extend(kept);
    }
    let left_out = total - (rings.len() - 1);
    Ok(Parsed { geometry: Some(Geometry::Polygon(rings)), rings_left_out: left_out })
}

/// A MultiGeometry's parts as one geometry: they must be of one family.
fn merge(parts: Vec<Geometry>) -> Result<Option<Geometry>> {
    let Some(first) = parts.first().map(|g| family(g.geometry_type())) else { return Ok(None) };
    if parts.iter().any(|g| family(g.geometry_type()) != first) {
        return Err(Error::Geometry("a MultiGeometry mixing points, lines and polygons".into()));
    }
    let mut points = Vec::new();
    let mut paths = Vec::new();
    for g in parts {
        match g {
            Geometry::Point(c) => points.push(c),
            Geometry::Multipoint(cs) => points.extend(cs),
            Geometry::Polyline(p) | Geometry::Polygon(p) => paths.extend(p),
        }
    }
    Ok(Some(match first {
        0 if points.len() == 1 => Geometry::Point(points.first().copied().unwrap_or(Coord::new(0.0, 0.0))),
        0 => Geometry::Multipoint(points),
        1 => Geometry::Polyline(paths),
        _ => Geometry::Polygon(paths),
    }))
}

/// A `coordinates` child's positions: `lon,lat[,alt]` tuples separated by white space (a space
/// after a comma, which some writers put, is tolerated). Altitude is not kept.
fn coordinates(n: &Node) -> Result<Vec<Coord>> {
    let text = n.child("coordinates").map_or("", |c| c.text.as_str());
    let mut tuples: Vec<String> = Vec::new();
    for token in text.split_whitespace() {
        match tuples.last_mut() {
            Some(t) if t.ends_with(',') || token.starts_with(',') => t.push_str(token),
            _ => tuples.push(token.to_string()),
        }
    }
    tuples
        .iter()
        .map(|t| {
            let mut it = t.split(',').map(|v| v.parse::<f64>());
            match (it.next(), it.next()) {
                (Some(Ok(x)), Some(Ok(y))) if x.is_finite() && y.is_finite() && y.abs() <= 90.0 => Ok(Coord::new(x, y)),
                _ => Err(Error::format("KML", format!("`{}` is not a longitude,latitude position", t.chars().take(40).collect::<String>()))),
            }
        })
        .collect()
}

/// A column: its name and type, and how a Placemark's value is found.
struct Column {
    name: String,
    kind: FieldType,
    get: Box<dyn Fn(&Row) -> Option<String>>,
}

/// `name` and `description` when any Placemark has them, then the ExtendedData names in order of
/// first appearance, each typed by its values.
fn columns<'a>(rows: impl Iterator<Item = &'a Row> + Clone) -> Vec<Column> {
    let mut out = Vec::new();
    if rows.clone().any(|r| r.name.is_some()) {
        out.push(Column { name: "name".into(), kind: FieldType::Text, get: Box::new(|r: &Row| r.name.clone()) });
    }
    if rows.clone().any(|r| r.description.is_some()) {
        out.push(Column { name: "description".into(), kind: FieldType::Text, get: Box::new(|r: &Row| r.description.clone()) });
    }
    let mut keys: Vec<String> = Vec::new();
    for (k, _) in rows.clone().flat_map(|r| &r.data) {
        if !keys.contains(k) {
            keys.push(k.clone());
        }
    }
    for key in keys {
        let values = rows.clone().filter_map(|r| r.data.iter().find(|(k, _)| *k == key)).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
        let kind = infer(values);
        // ExtendedData named like the Placemark's own name or description keeps its own column.
        let name = if out.iter().any(|c| c.name.eq_ignore_ascii_case(&key)) { format!("data_{key}") } else { key.clone() };
        out.push(Column { name, kind, get: Box::new(move |r: &Row| r.data.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) });
    }
    out
}

/// Integer when every value is one, Double when every value is a number, else Text.
fn infer<'a>(values: impl Iterator<Item = &'a str>) -> FieldType {
    let mut any = false;
    let (mut ints, mut numbers) = (true, true);
    for v in values {
        any = true;
        ints &= v.parse::<i64>().is_ok();
        numbers &= v.parse::<f64>().is_ok_and(f64::is_finite);
    }
    match (any, ints, numbers) {
        (true, true, _) => FieldType::Integer,
        (true, _, true) => FieldType::Double,
        _ => FieldType::Text,
    }
}

fn value(v: Option<&str>, kind: FieldType) -> Value {
    match v.filter(|v| !v.is_empty()) {
        None => Value::Null,
        Some(v) => match kind {
            FieldType::Integer => v.parse::<i64>().map_or(Value::Null, Value::Int),
            FieldType::Double => v.parse::<f64>().map_or(Value::Null, Value::Float),
            _ => Value::Text(v.to_string()),
        },
    }
}

fn write_geometry(out: &mut String, g: &Geometry, crs: Crs) -> Result<()> {
    let positions = |cs: &[Coord]| -> Result<String> {
        let mut s = String::new();
        for c in cs {
            let p = crs.to_geographic(*c)?;
            if !s.is_empty() {
                s.push(' ');
            }
            let _ = write!(s, "{},{}", p.x, p.y);
        }
        Ok(s)
    };
    match g {
        Geometry::Point(c) => {
            let _ = write!(out, "<Point><coordinates>{}</coordinates></Point>", positions(&[*c])?);
        }
        Geometry::Multipoint(cs) => {
            out.push_str("<MultiGeometry>");
            for c in cs {
                let _ = write!(out, "<Point><coordinates>{}</coordinates></Point>", positions(&[*c])?);
            }
            out.push_str("</MultiGeometry>");
        }
        Geometry::Polyline(parts) => {
            let many = parts.len() > 1;
            if many {
                out.push_str("<MultiGeometry>");
            }
            for p in parts {
                let _ = write!(out, "<LineString><coordinates>{}</coordinates></LineString>", positions(p)?);
            }
            if many {
                out.push_str("</MultiGeometry>");
            }
        }
        Geometry::Polygon(rings) => {
            let groups = mkaegis_geom::wkt::shells_with_holes(rings);
            let many = groups.len() > 1;
            if many {
                out.push_str("<MultiGeometry>");
            }
            for group in groups {
                out.push_str("<Polygon>");
                for (i, ring) in group.into_iter().enumerate() {
                    // KML 2.2: outer rings counter-clockwise; holes the other way round.
                    let tag = if i == 0 { "outerBoundaryIs" } else { "innerBoundaryIs" };
                    let _ = write!(out, "<{tag}><LinearRing><coordinates>{}</coordinates></LinearRing></{tag}>", positions(&oriented(ring, i == 0))?);
                }
                out.push_str("</Polygon>");
            }
            if many {
                out.push_str("</MultiGeometry>");
            }
        }
    }
    Ok(())
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

/// A value as KML text; `None` for null.
fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::Bool(b) => Some(b.to_string()),
        Value::Int(i) => Some(i.to_string()),
        Value::Float(x) => Some(x.to_string()),
        Value::Text(t) => Some(t.clone()),
    }
}

fn family(t: GeometryType) -> usize {
    match t {
        GeometryType::Point | GeometryType::Multipoint => 0,
        GeometryType::Polyline => 1,
        GeometryType::Polygon => 2,
    }
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>Lotes</name>
  <Style id="s"><LineStyle><color>ff0000ff</color></LineStyle></Style>
  <Folder>
    <name>Norte</name>
    <Placemark>
      <name>Lote &amp; casa</name>
      <description><![CDATA[<b>com</b> quintal]]></description>
      <styleUrl>#s</styleUrl>
      <ExtendedData>
        <Data name="area_ha"><value>1.5</value></Data>
        <Data name="codigo"><value>12</value></Data>
      </ExtendedData>
      <Polygon>
        <outerBoundaryIs><LinearRing><coordinates>
          -46.0,-23.0,0 -45.0,-23.0,0 -45.0,-22.0,0 -46.0,-22.0,0 -46.0,-23.0,0
        </coordinates></LinearRing></outerBoundaryIs>
        <innerBoundaryIs><LinearRing><coordinates>-45.8,-22.8 -45.2,-22.8 -45.2,-22.2 -45.8,-22.8</coordinates></LinearRing></innerBoundaryIs>
      </Polygon>
    </Placemark>
    <Folder>
      <Placemark>
        <name>Lote &#233;</name>
        <ExtendedData><SchemaData schemaUrl="#x"><SimpleData name="codigo">7</SimpleData><SimpleData name="dono">Ana</SimpleData></SchemaData></ExtendedData>
        <MultiGeometry>
          <Polygon><outerBoundaryIs><LinearRing><coordinates>0,0 1,0 1,1 0,0</coordinates></LinearRing></outerBoundaryIs></Polygon>
          <Polygon><outerBoundaryIs><LinearRing><coordinates>5,5 6,5 6,6 5,5</coordinates></LinearRing></outerBoundaryIs></Polygon>
        </MultiGeometry>
      </Placemark>
    </Folder>
    <Placemark><name>Poço</name><Point><coordinates>-45.5,-22.5,10</coordinates></Point></Placemark>
  </Folder>
</Document>
</kml>"##;

    #[test]
    fn reads_placemarks_in_folders_with_their_attributes() {
        let r = read(SAMPLE, "lotes").unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        // A layer per family, polygons first, each with the columns its placemarks have.
        let names: Vec<(&str, GeometryType, usize)> = r.layers.iter().map(|l| (l.name.as_str(), l.geometry_type, l.features.len())).collect();
        assert_eq!(names, [("lotes_Polygons", GeometryType::Polygon, 2), ("lotes_Points", GeometryType::Point, 1)]);
        assert_eq!(r.layers[1].fields, [Field::new("name", FieldType::Text)]);
        let data = &r.layers[0];
        assert_eq!(data.crs, Crs::WGS84);
        let kinds: Vec<_> = data.fields.iter().map(|f| (f.name.as_str(), f.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("name", FieldType::Text),
                ("description", FieldType::Text),
                ("area_ha", FieldType::Double),
                ("codigo", FieldType::Integer),
                ("dono", FieldType::Text)
            ]
        );
        let values: Vec<&Vec<Value>> = data.features.iter().map(|f| &f.values).collect();
        assert_eq!(
            values[0],
            &vec![Value::Text("Lote & casa".into()), Value::Text("<b>com</b> quintal".into()), Value::Float(1.5), Value::Int(12), Value::Null]
        );
        assert_eq!(values[1], &vec![Value::Text("Lote é".into()), Value::Null, Value::Null, Value::Int(7), Value::Text("Ana".into())]);
        // The hole is a hole: 1° × 1° less 0.6° × 0.6° / 2.
        let a = mkaegis_geom::algo::area(data.features[0].geometry.as_ref().unwrap());
        assert!((a - (1.0 - 0.18)).abs() < 1e-9, "{a}");
        let Some(Geometry::Polygon(rings)) = &data.features[1].geometry else { panic!("not a polygon") };
        assert_eq!(rings.len(), 2, "both polygons of the MultiGeometry");
    }

    #[test]
    fn a_bad_geometry_costs_only_its_own_shape() {
        let kml = r#"<kml xmlns="http://www.opengis.net/kml/2.2"><Document>
            <Placemark><name>a</name><Point><coordinates>1,2</coordinates></Point></Placemark>
            <Placemark><name>b</name><Point><coordinates>x,2</coordinates></Point></Placemark>
            <Placemark><name>c</name><Point><coordinates></coordinates></Point></Placemark>
            <Placemark><name>d</name></Placemark>
            <Placemark><name>e</name><MultiGeometry><Point><coordinates>1,1</coordinates></Point><LineString><coordinates>0,0 1,1</coordinates></LineString></MultiGeometry></Placemark>
            <kml:Placemark xmlns:kml="http://www.opengis.net/kml/2.2"><kml:name>f</kml:name><kml:Point><kml:coordinates>3, 4</kml:coordinates></kml:Point></kml:Placemark>
        </Document></kml>"#;
        let r = read(kml, "p").unwrap();
        assert_eq!(r.layers.len(), 1, "placemarks without a shape go with the points");
        let data = &r.layers[0];
        assert_eq!(data.name, "p");
        let names: Vec<&Value> = data.features.iter().map(|f| &f.values[0]).collect();
        assert_eq!(names.len(), 6, "every placemark kept");
        assert_eq!(names[5], &Value::Text("f".into()));
        let shapes: Vec<bool> = data.features.iter().map(|f| f.geometry.is_some()).collect();
        assert_eq!(shapes, [true, false, false, false, false, true]);
        assert_eq!(data.features[5].geometry, Some(Geometry::Point(Coord::new(3.0, 4.0))));
        assert_eq!(
            r.warnings,
            [
                "placemark 2: cannot read KML: `x,2` is not a longitude,latitude position",
                "placemark 5: geometry: a MultiGeometry mixing points, lines and polygons",
                "2 placemarks with a null geometry, their attributes kept",
                "2 placemarks without a geometry read as null",
            ]
        );
    }

    #[test]
    fn round_trips_through_write() {
        let r = read(SAMPLE, "lotes").unwrap();
        let polygons = &r.layers[0];
        let again = read(&write(polygons).unwrap(), "lotes").unwrap();
        let again = &again.layers[0];
        assert_eq!(again.fields, polygons.fields);
        assert_eq!(again.features.len(), 2);
        for (a, b) in again.features.iter().zip(&polygons.features) {
            assert_eq!(a.values, b.values);
            let area = |f: &mkaegis_model::Feature| mkaegis_geom::algo::area(f.geometry.as_ref().unwrap());
            assert!((area(a) - area(b)).abs() < 1e-12);
        }
        // Projected data is written in degrees.
        let mut utm = FeatureClass::new("p", GeometryType::Point, Crs::from_epsg(31983).unwrap(), vec![]);
        utm.push(Some(Geometry::Point(Coord::new(500_000.0, 7_400_000.0))), vec![]).unwrap();
        let back = read(&write(&utm).unwrap(), "p").unwrap();
        let Some(Geometry::Point(c)) = back.layers[0].features[0].geometry else { panic!("no point") };
        assert!((c.x + 45.0).abs() < 1e-9 && (c.y + 23.51).abs() < 0.01, "{c:?}");
        // Text is escaped: markup in a value stays text.
        let mut t = FeatureClass::new("t", GeometryType::Point, Crs::WGS84, vec![Field::new("NOTE", FieldType::Text)]);
        t.push(None, vec![Value::Text("a < b & \"c\"".into())]).unwrap();
        let back = read(&write(&t).unwrap(), "t").unwrap();
        assert_eq!(back.layers[0].features[0].values, vec![Value::Text("a < b & \"c\"".into())]);
    }

    #[test]
    fn hostile_input_is_an_error_never_a_panic() {
        for bad in [
            "",
            "not xml",
            "<html><body/></html>",
            "<kml><Document><Placemark></Document></kml>",
            "<kml><Document><Placemark><name>x</name>",
            r#"<kml><Placemark><Point><coordinates>1,2</coordinates></Point></Placemark></kml><kml>"#,
        ] {
            assert!(read(bad, "x").is_err(), "{bad}");
        }
        assert!(read("<kml/>", "x").unwrap().warnings.contains(&"no Placemark in the document".to_string()));
        let deep = format!("<kml>{}{}</kml>", "<Folder>".repeat(MAX_DEPTH + 1), "</Folder>".repeat(MAX_DEPTH + 1));
        assert!(read(&deep, "x").is_err());
        // Every prefix of a real document: an error or a layer, never a panic.
        for cut in (0..SAMPLE.len()).filter(|i| SAMPLE.is_char_boundary(*i)) {
            let _ = read(&SAMPLE[..cut], "x");
        }
        // A declared entity is never expanded (no "billion laughs").
        let laughs = r#"<?xml version="1.0"?><!DOCTYPE kml [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">]>
            <kml><Placemark><name>&b;</name></Placemark></kml>"#;
        let r = read(laughs, "x").unwrap();
        assert_eq!(r.layers[0].features[0].values[0], Value::Text("&b;".into()));
    }
}
