//! The map document (L1): feature classes (a geometry type, a CRS, fields and features), layers
//! that draw them with a renderer, and the map that orders the layers. `symbology` holds the
//! renderers and classification; `query` the SQL `WHERE` clauses of Select By Attributes;
//! `weights` the spatial weights that the statistics use.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod classify;
pub mod query;
pub mod symbology;
pub mod weights;

use std::collections::BTreeSet;
use std::sync::Arc;

use mkaegis_core::{Color, Error, Result, Value};
use mkaegis_crs::Crs;
use mkaegis_geom::{Envelope, Geometry, GeometryType};
use serde::{Deserialize, Serialize};

pub use symbology::{Labels, Marker, Renderer, Symbol};

/// The longest field name (the dBASE limit is 10; geodatabases allow more).
pub const MAX_FIELD_NAME: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    /// Long integer.
    Integer,
    /// Short integer: 16 bits, -32768 to 32767.
    Short,
    Double,
    Text,
    /// True or false, as GeoJSON booleans and dBASE logical (`L`) columns hold them.
    Boolean,
}

impl FieldType {
    pub fn name(self) -> &'static str {
        match self {
            FieldType::Integer => "Long",
            FieldType::Short => "Short",
            FieldType::Double => "Double",
            FieldType::Text => "Text",
            FieldType::Boolean => "Boolean",
        }
    }

    pub fn is_numeric(self) -> bool {
        matches!(self, FieldType::Integer | FieldType::Short | FieldType::Double)
    }

    /// A value converted to this type, `None` when it doesn't fit: text that isn't a number,
    /// a number out of range, a boolean other than 0 or 1. Fractions round to the nearest integer.
    pub fn coerce(self, v: Value) -> Option<Value> {
        let integer = |v: &Value, min: f64, max: f64| v.as_f64().map(f64::round).filter(|x| (min..=max).contains(x)).map(|x| Value::Int(x as i64));
        match (self, v) {
            (_, Value::Null) => Some(Value::Null),
            (FieldType::Text, v) => Some(Value::Text(v.to_string())),
            (FieldType::Double, v) => v.as_f64().map(Value::Float),
            (FieldType::Integer, v) => integer(&v, -9.0e15, 9.0e15),
            (FieldType::Short, v) => integer(&v, f64::from(i16::MIN), f64::from(i16::MAX)),
            (FieldType::Boolean, Value::Bool(b)) => Some(Value::Bool(b)),
            (FieldType::Boolean, v) => match v.as_f64() {
                Some(0.0) => Some(Value::Bool(false)),
                Some(1.0) => Some(Value::Bool(true)),
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub kind: FieldType,
}

impl Field {
    pub fn new(name: &str, kind: FieldType) -> Self {
        Field { name: name.to_string(), kind }
    }
}

/// One row: a shape (or none) and a value per field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub geometry: Option<Geometry>,
    pub values: Vec<Value>,
}

/// Features of one geometry type, in one CRS, with one schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureClass {
    pub name: String,
    pub geometry_type: GeometryType,
    pub crs: Crs,
    /// The `.prj` text of a coordinate system MK aeGIS doesn't know (#6), kept so the data can
    /// be written back with it. `None` when `crs` is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prj: Option<String>,
    pub fields: Vec<Field>,
    pub features: Vec<Feature>,
}

impl FeatureClass {
    pub fn new(name: &str, geometry_type: GeometryType, crs: Crs, fields: Vec<Field>) -> Self {
        FeatureClass { name: name.to_string(), geometry_type, crs, prj: None, fields, features: Vec::new() }
    }

    /// About how many bytes the features take in memory: coordinates, values and text. An
    /// estimate, for budgets such as how much undo may hold.
    pub fn approx_bytes(&self) -> usize {
        let value = |v: &Value| std::mem::size_of::<Value>() + if let Value::Text(t) = v { t.len() } else { 0 };
        self.features
            .iter()
            .map(|f| {
                let coords = f.geometry.as_ref().map_or(0, |g| g.coords().count());
                std::mem::size_of::<Feature>() + coords * std::mem::size_of::<mkaegis_geom::Coord>() + f.values.iter().map(value).sum::<usize>()
            })
            .sum()
    }

    /// The index of a field by name, ignoring case.
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Adds a feature, checking its geometry type and padding or trimming its values to the schema.
    pub fn push(&mut self, geometry: Option<Geometry>, mut values: Vec<Value>) -> Result<()> {
        if let Some(g) = &geometry {
            let ok = g.geometry_type() == self.geometry_type
                || (self.geometry_type == GeometryType::Multipoint && g.geometry_type() == GeometryType::Point);
            if !ok {
                return Err(Error::Geometry(format!("a {} in a {} feature class", g.geometry_type().name(), self.geometry_type.name())));
            }
        }
        if self.features.len() >= mkaegis_core::MAX_ITEMS {
            return Err(Error::param("features", "too many features"));
        }
        values.resize(self.fields.len(), Value::Null);
        self.features.push(Feature { geometry, values });
        Ok(())
    }

    /// Adds a field, Null in every row.
    pub fn add_field(&mut self, field: Field) -> Result<usize> {
        let name = field.name.trim();
        if name.is_empty() || name.len() > MAX_FIELD_NAME || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(Error::param("field", format!("`{name}` is not a valid field name (letters, digits and _)")));
        }
        if self.field_index(name).is_some() {
            return Err(Error::param("field", format!("a field named `{name}` already exists")));
        }
        self.fields.push(Field { name: name.to_string(), kind: field.kind });
        for f in &mut self.features {
            f.values.push(Value::Null);
        }
        Ok(self.fields.len() - 1)
    }

    pub fn value(&self, row: usize, field: usize) -> Option<&Value> {
        self.features.get(row)?.values.get(field)
    }

    pub fn extent(&self) -> Option<Envelope> {
        self.features.iter().filter_map(|f| f.geometry.as_ref()?.envelope()).reduce(Envelope::union)
    }
}

/// How a new selection combines with the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionMode {
    #[default]
    New,
    Add,
    Remove,
    Intersect,
}

impl SelectionMode {
    pub fn parse(text: &str) -> Option<SelectionMode> {
        match text.to_ascii_lowercase().as_str() {
            "" | "new" => Some(SelectionMode::New),
            "add" => Some(SelectionMode::Add),
            "remove" => Some(SelectionMode::Remove),
            "intersect" | "subset" => Some(SelectionMode::Intersect),
            _ => None,
        }
    }
}

/// A feature class drawn on the map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    /// The features, shared with undo snapshots until a command changes them: read it as it
    /// is, change it through [`Layer::data_mut`], which copies it first if it is shared.
    pub data: Arc<FeatureClass>,
    pub renderer: Renderer,
    pub labels: Option<Labels>,
    /// Selected rows.
    pub selection: BTreeSet<usize>,
    /// Opacity 0–100 (ArcGIS shows transparency; we keep opacity).
    pub opacity: u8,
}

impl Layer {
    pub fn new(id: u32, data: FeatureClass) -> Self {
        let symbol = Symbol::default_for(data.geometry_type, id);
        Layer {
            id,
            name: data.name.clone(),
            visible: true,
            data: Arc::new(data),
            renderer: Renderer::Single(symbol),
            labels: None,
            selection: BTreeSet::new(),
            opacity: 100,
        }
    }

    /// The features to change: copied first when an undo snapshot still shares them.
    pub fn data_mut(&mut self) -> &mut FeatureClass {
        Arc::make_mut(&mut self.data)
    }

    /// Applies `rows` to the selection the way `mode` says.
    pub fn select(&mut self, rows: impl IntoIterator<Item = usize>, mode: SelectionMode) {
        let n = self.data.features.len();
        let rows: BTreeSet<usize> = rows.into_iter().filter(|r| *r < n).collect();
        self.selection = match mode {
            SelectionMode::New => rows,
            SelectionMode::Add => self.selection.union(&rows).copied().collect(),
            SelectionMode::Remove => self.selection.difference(&rows).copied().collect(),
            SelectionMode::Intersect => self.selection.intersection(&rows).copied().collect(),
        };
    }
}

/// A map: its CRS, its layers (index 0 is the top of the Contents pane, drawn last) and the
/// extent last shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Map {
    pub name: String,
    pub crs: Crs,
    pub layers: Vec<Layer>,
    pub extent: Option<Envelope>,
    pub background: Color,
    next_id: u32,
    /// Spatial weights, by name (projects saved before weights existed have none).
    #[serde(default)]
    pub weights: Vec<Arc<weights::Weights>>,
}

impl Default for Map {
    fn default() -> Self {
        Map::new("Map", Crs::WEB_MERCATOR)
    }
}

impl Map {
    pub fn new(name: &str, crs: Crs) -> Self {
        Map { name: name.to_string(), crs, layers: Vec::new(), extent: None, background: Color::WHITE, next_id: 1, weights: Vec::new() }
    }

    /// Checks a map read from a project file, which is hostile input (#58), and repairs what is
    /// safe to repair. Refused: two layers with one id, two fields with one name, a row with more
    /// or fewer values than fields or a value its field can't hold, a shape of another geometry
    /// type or with a non-finite coordinate, more than `MAX_ITEMS` features or links. Repaired,
    /// each with a warning: the next layer id behind the ids in use, selected rows past the
    /// end, spatial weights for a missing layer or naming rows that don't exist (dropped).
    pub fn validate(&mut self) -> Result<Vec<String>> {
        let bad = |message: String| Err(Error::format("project", message));
        let mut warnings = Vec::new();
        let mut ids = BTreeSet::new();
        let mut total = 0usize;
        for layer in &mut self.layers {
            if !ids.insert(layer.id) {
                return bad(format!("two layers have the id {}", layer.id));
            }
            let layer_name = layer.name.clone();
            let fc = layer.data_mut();
            let mut names = BTreeSet::new();
            for f in &fc.fields {
                if !names.insert(f.name.to_ascii_lowercase()) {
                    return bad(format!("layer `{}` has two fields named `{}`", fc.name, f.name));
                }
            }
            total = total.saturating_add(fc.features.len());
            if total > mkaegis_core::MAX_ITEMS {
                return bad(format!("more than {} features", mkaegis_core::MAX_ITEMS));
            }
            let (kind, fields) = (fc.geometry_type, fc.fields.clone());
            for (row, feature) in fc.features.iter_mut().enumerate() {
                let at = || format!("layer `{layer_name}`, row {}", row + 1);
                if let Some(g) = &feature.geometry {
                    let fits = g.geometry_type() == kind || (kind == GeometryType::Multipoint && g.geometry_type() == GeometryType::Point);
                    if !fits {
                        return bad(format!("{}: a {} in a {} layer", at(), g.geometry_type().name(), kind.name()));
                    }
                    if !g.coords().all(|c| c.is_finite()) {
                        return bad(format!("{}: a coordinate is not a finite number", at()));
                    }
                }
                if feature.values.len() != fields.len() {
                    return bad(format!("{}: {} values for {} fields", at(), feature.values.len(), fields.len()));
                }
                for (value, field) in feature.values.iter_mut().zip(&fields) {
                    match field.kind.coerce(value.clone()) {
                        Some(v) => *value = v,
                        None => return bad(format!("{}: `{value}` doesn't fit the {} field `{}`", at(), field.kind.name(), field.name)),
                    }
                }
            }
            let n = layer.data.features.len();
            let before = layer.selection.len();
            layer.selection.retain(|r| *r < n);
            if layer.selection.len() < before {
                warnings.push(format!("layer `{}`: {} selected rows past its end were cleared", layer.name, before - layer.selection.len()));
            }
        }
        // The next id comes after every id in use, or Add Data would give two layers one id.
        if let Some(&last) = ids.last()
            && self.next_id <= last
        {
            self.next_id = last.checked_add(1).ok_or_else(|| Error::format("project", "a layer id is too large"))?;
        }
        let mut links = 0usize;
        let layers = &self.layers;
        self.weights.retain(|w| {
            let rows = w.neighbours.len();
            links = links.saturating_add(w.neighbours.iter().map(Vec::len).sum::<usize>());
            let problem = if !layers.iter().any(|l| l.id == w.layer) {
                Some("its layer is not in the project")
            } else if w.neighbours.iter().flatten().any(|(j, x)| *j as usize >= rows || !x.is_finite()) {
                Some("it names rows that don't exist")
            } else {
                None
            };
            if let Some(p) = problem {
                warnings.push(format!("spatial weights `{}` dropped: {p}", w.name));
            }
            problem.is_none()
        });
        if links > mkaegis_core::MAX_ITEMS {
            return bad(format!("more than {} spatial weights links", mkaegis_core::MAX_ITEMS));
        }
        Ok(warnings)
    }

    /// Adds a feature class as the top layer and returns the layer's id.
    pub fn add_layer(&mut self, data: FeatureClass) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        // Points over lines over polygons, as ArcGIS orders new layers.
        let rank = |t: GeometryType| match t {
            GeometryType::Point | GeometryType::Multipoint => 0,
            GeometryType::Polyline => 1,
            GeometryType::Polygon => 2,
        };
        let own = rank(data.geometry_type);
        let at = self.layers.iter().position(|l| rank(l.data.geometry_type) >= own).unwrap_or(self.layers.len());
        self.layers.insert(at, Layer::new(id, data));
        id
    }

    /// A layer by id (`"3"`) or by name, ignoring case.
    pub fn find(&self, key: &str) -> Result<usize> {
        let key = key.trim();
        if let Ok(id) = key.parse::<u32>()
            && let Some(i) = self.layers.iter().position(|l| l.id == id)
        {
            return Ok(i);
        }
        self.layers.iter().position(|l| l.name.eq_ignore_ascii_case(key)).ok_or_else(|| Error::NotFound(format!("layer `{key}`")))
    }

    pub fn layer(&self, key: &str) -> Result<&Layer> {
        let i = self.find(key)?;
        self.layers.get(i).ok_or_else(|| Error::NotFound(format!("layer `{key}`")))
    }

    pub fn layer_mut(&mut self, key: &str) -> Result<&mut Layer> {
        let i = self.find(key)?;
        self.layers.get_mut(i).ok_or_else(|| Error::NotFound(format!("layer `{key}`")))
    }

    /// A layer name not yet used: `name`, `name_1`, `name_2`…
    pub fn unique_name(&self, name: &str) -> String {
        let taken = |n: &str| self.layers.iter().any(|l| l.name.eq_ignore_ascii_case(n));
        if !taken(name) {
            return name.to_string();
        }
        (1..).map(|i| format!("{name}_{i}")).find(|n| !taken(n)).unwrap_or_else(|| name.to_string())
    }

    /// The extent of every layer's data, in the map's CRS.
    pub fn full_extent(&self) -> Option<Envelope> {
        self.layers.iter().filter_map(|l| self.layer_extent(l)).reduce(Envelope::union)
    }

    /// A layer's extent in the map's CRS (projected corner by corner, densified).
    pub fn layer_extent(&self, layer: &Layer) -> Option<Envelope> {
        let e = layer.data.extent()?;
        project_envelope(e, layer.data.crs, self.crs)
    }
}

/// An envelope from one CRS into another, by projecting points along its edges, for views and
/// selection rectangles: Web Mercator takes the poles at its edge (`Crs::transform_for_display`).
/// Data in an unknown coordinate system, or a map in one, is placed as it is.
pub fn project_envelope(e: Envelope, from: Crs, to: Crs) -> Option<Envelope> {
    if from == to || !from.is_known() || !to.is_known() {
        return Some(e);
    }
    let steps = 16;
    let mut pts = Vec::with_capacity(steps * 4);
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = e.min_x + t * e.width();
        let y = e.min_y + t * e.height();
        pts.extend([
            mkaegis_geom::Coord::new(x, e.min_y),
            mkaegis_geom::Coord::new(x, e.max_y),
            mkaegis_geom::Coord::new(e.min_x, y),
            mkaegis_geom::Coord::new(e.max_x, y),
        ]);
    }
    Envelope::of(pts.into_iter().filter_map(|p| from.transform_for_display(to, p).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkaegis_geom::Coord;

    fn points() -> FeatureClass {
        let mut fc = FeatureClass::new("cities", GeometryType::Point, Crs::WGS84, vec![Field::new("NAME", FieldType::Text)]);
        fc.push(Some(Geometry::Point(Coord::new(1.0, 2.0))), vec![Value::Text("A".into())]).unwrap();
        fc.push(None, vec![]).unwrap();
        fc
    }

    #[test]
    fn values_are_coerced_to_the_field_type() {
        assert_eq!(FieldType::Short.coerce(Value::Float(7.4)), Some(Value::Int(7)));
        assert_eq!(FieldType::Short.coerce(Value::Int(32_767)), Some(Value::Int(32_767)));
        assert_eq!(FieldType::Short.coerce(Value::Int(32_768)), None);
        assert_eq!(FieldType::Short.coerce(Value::Text("x".into())), None);
        assert_eq!(FieldType::Integer.coerce(Value::Float(f64::INFINITY)), None);
        assert_eq!(FieldType::Integer.coerce(Value::Bool(true)), Some(Value::Int(1)));
        assert_eq!(FieldType::Boolean.coerce(Value::Int(1)), Some(Value::Bool(true)));
        assert_eq!(FieldType::Boolean.coerce(Value::Int(5)), None);
        assert_eq!(FieldType::Text.coerce(Value::Int(5)), Some(Value::Text("5".into())));
        assert_eq!(FieldType::Double.coerce(Value::Null), Some(Value::Null));
    }

    #[test]
    fn feature_classes_check_types_and_schema() {
        let mut fc = points();
        assert!(fc.push(Some(Geometry::Polyline(vec![])), vec![]).is_err());
        assert_eq!(fc.features[1].values, vec![Value::Null]);
        assert_eq!(fc.add_field(Field::new("POP", FieldType::Integer)).unwrap(), 1);
        assert!(fc.add_field(Field::new("pop", FieldType::Integer)).is_err());
        assert!(fc.add_field(Field::new("bad name", FieldType::Integer)).is_err());
        assert_eq!(fc.field_index("name"), Some(0));
    }

    #[test]
    fn layers_are_found_and_ordered() {
        let mut map = Map::new("m", Crs::WGS84);
        let poly = FeatureClass::new("areas", GeometryType::Polygon, Crs::WGS84, vec![]);
        map.add_layer(poly);
        let id = map.add_layer(points());
        assert_eq!(map.layers[0].name, "cities");
        assert_eq!(map.find(&id.to_string()).unwrap(), 0);
        assert_eq!(map.find("AREAS").unwrap(), 1);
        assert!(map.find("nope").is_err());
        assert_eq!(map.unique_name("cities"), "cities_1");
    }

    #[test]
    fn selection_modes() {
        let mut layer = Layer::new(1, points());
        layer.select([0, 1, 99], SelectionMode::New);
        assert_eq!(layer.selection.len(), 2);
        layer.select([1], SelectionMode::Remove);
        assert_eq!(layer.selection.iter().copied().collect::<Vec<_>>(), vec![0]);
        layer.select([1], SelectionMode::Intersect);
        assert!(layer.selection.is_empty());
    }

    #[test]
    fn envelopes_project() {
        let e = Envelope::new(Coord::new(-10.0, -10.0), Coord::new(10.0, 10.0));
        let p = project_envelope(e, Crs::WGS84, Crs::WEB_MERCATOR).unwrap();
        assert!((p.max_x - 1_113_194.907_932_736).abs() < 1e-3);
    }
}
