//! Analysis tab and the Geoprocessing pane's tools; Share ▸ Export Map.

use std::collections::HashMap;

use mkaegis_core::{Error, Result, Value};
use mkaegis_crs::Crs;
use mkaegis_geom::{Coord, Geometry, GeometryType};
use mkaegis_model::{FeatureClass, FieldType};
use serde_json::{Value as Json, json};

use super::{CommandSpec, Kind, has_layers, layer_key, opt_bool, opt_f64, opt_str, path, req_f64, req_str};
use crate::Session;

/// Adds a tool's output as a new layer named `output` (or a default), returning its id.
fn add_output(s: &mut Session, mut data: mkaegis_model::FeatureClass, p: &Json, default: &str) -> u32 {
    data.name = s.map.unique_name(opt_str(p, "output").unwrap_or(default));
    s.map.add_layer(data)
}

pub const BUFFER: CommandSpec = CommandSpec {
    id: "analysis.buffer",
    label: "Buffer",
    ribbon: "Analysis ▸ Tools",
    shortcut: None,
    params: "{layer, distance, unit?: meters|kilometers, output?, selectedOnly?}",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let mut distance = req_f64(p, "distance")?;
        match opt_str(p, "unit").unwrap_or("meters").to_ascii_lowercase().as_str() {
            "m" | "meter" | "meters" | "metre" | "metres" => {}
            "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => distance *= 1000.0,
            other => return Err(Error::param("unit", format!("unknown unit `{other}`"))),
        }
        let mut data = (*layer.data).clone();
        if opt_bool(p, "selectedOnly")?.unwrap_or(false) || (!layer.selection.is_empty() && p.get("selectedOnly").is_none()) {
            // Like ArcGIS, a tool works on the selection when there is one.
            data.features = layer.selection.iter().filter_map(|r| layer.data.features.get(*r).cloned()).collect();
        }
        let default = format!("{}_Buffer", layer.name);
        let out = mkaegis_analysis::buffer(&data, distance, &default)?;
        let n = out.features.len();
        let id = add_output(s, out, p, &default);
        Ok(json!({ "message": format!("Buffer: {n} features"), "layer": id }))
    },
};

pub const PROJECT: CommandSpec = CommandSpec {
    id: "management.project",
    label: "Project",
    ribbon: "Analysis ▸ Tools",
    shortcut: None,
    params: "{layer, epsg, output?}",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let code = req_f64(p, "epsg")?;
        let crs = Crs::from_epsg(u32::try_from(code as i64).map_err(|_| Error::param("epsg", "not an EPSG code"))?)?;
        let default = format!("{}_Project", layer.name);
        let out = mkaegis_analysis::project(&layer.data, crs, &default)?;
        let id = add_output(s, out, p, &default);
        Ok(json!({ "message": format!("Project: in {}", crs.name()), "layer": id }))
    },
};

pub const DEFINE_PROJECTION: CommandSpec = CommandSpec {
    id: "management.defineProjection",
    label: "Define Projection",
    ribbon: "Analysis ▸ Tools",
    shortcut: None,
    params: "{layer, epsg} — says which coordinate system the layer's coordinates are in; they don't move",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let key = layer_key(p, "layer")?;
        let code = req_f64(p, "epsg")?;
        let crs = Crs::from_epsg(u32::try_from(code as i64).map_err(|_| Error::param("epsg", "not an EPSG code"))?)?;
        let layer = s.map.layer_mut(&key)?;
        let data = layer.data_mut();
        let was = data.crs;
        data.crs = crs;
        data.prj = None;
        s.view_serial += 1;
        Ok(json!({ "message": format!("{} defined as {} (was {})", layer.name, crs.name(), was.name()) }))
    },
};

pub const JOIN_FIELD: CommandSpec = CommandSpec {
    id: "management.joinField",
    label: "Join Field",
    ribbon: "Analysis ▸ Tools",
    shortcut: None,
    params: "{layer, field, path? (.csv/.tsv/.txt) or joinLayer?, joinField, fields? [names; all by default], types? {column: text|integer|double|boolean}} \
             — copies a table's fields onto the layer by key and reports matched, unmatched and duplicate keys",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let key = req_str(p, "field")?;
        let join_key = req_str(p, "joinField")?;
        let copy: Vec<String> = match p.get("fields") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Array(a)) => {
                a.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| Error::param("fields", "field names"))).collect::<Result<_>>()?
            }
            Some(_) => return Err(Error::param("fields", "a list of field names")),
        };
        let (fields, rows, source, mut warnings) = match (opt_str(p, "path"), opt_str(p, "joinLayer")) {
            (Some(_), None) => {
                let file = path(p, "path")?;
                let t = crate::io::read_table(&file, &types(p)?)?;
                let source = file.file_name().map_or_else(|| "the table".to_string(), |n| n.to_string_lossy().into_owned());
                (t.fields, t.rows, source, t.warnings)
            }
            (None, Some(other)) => {
                let l = s.map.layer(other)?;
                (l.data.fields.clone(), l.data.features.iter().map(|f| f.values.clone()).collect(), l.name.clone(), Vec::new())
            }
            _ => return Err(Error::param("path", "either a table's path or a joinLayer")),
        };
        let layer = s.map.layer_mut(&layer_key(p, "layer")?)?;
        let report = mkaegis_analysis::join::join_field(layer.data_mut(), key, &fields, &rows, join_key, &copy)?;
        if report.duplicate_keys > 0 {
            warnings.push(format!("{} keys appear more than once in {source}: their first row was used", report.duplicate_keys));
        }
        let message = format!(
            "{} of {} features matched in {source}; {} unmatched; {} rows of the table unused",
            report.matched,
            report.matched + report.unmatched,
            report.unmatched,
            report.unused
        );
        let mut out = serde_json::to_value(&report).map_err(|e| Error::Internal(e.to_string()))?;
        if let Some(o) = out.as_object_mut() {
            o.insert("message".into(), json!(message));
            o.insert("warnings".into(), json!(warnings));
        }
        Ok(out)
    },
};

pub const XY_TABLE_TO_POINT: CommandSpec = CommandSpec {
    id: "management.xyTableToPoint",
    label: "XY Table To Point",
    ribbon: "Analysis ▸ Tools",
    shortcut: None,
    params: "{path (.csv/.tsv/.txt), x? and y? (columns), or wkt? (a column of WKT shapes), epsg? (default 4326), output?, \
             types? {column: text|integer|double|boolean}} — a layer from a table's coordinates; rows without them are skipped",
    kind: Kind::Edit,
    enabled: |_| true,
    run: |s, p| {
        let file = path(p, "path")?;
        let table = crate::io::read_table(&file, &types(p)?)?;
        let crs = match opt_f64(p, "epsg")? {
            None => Crs::WGS84,
            Some(code) => Crs::from_epsg(u32::try_from(code as i64).map_err(|_| Error::param("epsg", "not an EPSG code"))?)?,
        };
        let column = |key: &str| -> Result<Option<usize>> {
            match opt_str(p, key) {
                None => Ok(None),
                Some(name) => table
                    .fields
                    .iter()
                    .position(|f| f.name.eq_ignore_ascii_case(name))
                    .map(Some)
                    .ok_or_else(|| Error::NotFound(format!("column `{name}` in the table"))),
            }
        };
        let (x, y, wkt) = (column("x")?, column("y")?, column("wkt")?);
        let shapes: Vec<Option<Geometry>> = match (x, y, wkt) {
            (Some(x), Some(y), None) => table
                .rows
                .iter()
                .map(|r| {
                    let c = Coord::new(r.get(x).and_then(Value::as_f64)?, r.get(y).and_then(Value::as_f64)?);
                    // Degrees out of range are a column swap or a projected table, not a place.
                    (c.is_finite() && (!crs.is_geographic() || (c.x.abs() <= 360.0 && c.y.abs() <= 90.0))).then_some(Geometry::Point(c))
                })
                .collect(),
            (None, None, Some(w)) => table
                .rows
                .iter()
                .map(|r| match r.get(w) {
                    Some(Value::Text(t)) => mkaegis_geom::wkt::from_wkt(t).ok().filter(Geometry::is_finite),
                    _ => None,
                })
                .collect(),
            _ => return Err(Error::param("x", "give x and y columns, or a wkt column")),
        };
        let kind =
            shapes.iter().flatten().next().map(Geometry::geometry_type).ok_or_else(|| Error::param("path", "no row has usable coordinates"))?;
        let kind = if kind == GeometryType::Point && shapes.iter().flatten().any(|g| g.geometry_type() == GeometryType::Multipoint) {
            GeometryType::Multipoint
        } else {
            kind
        };
        let default = file.file_stem().map_or_else(|| "table".to_string(), |n| n.to_string_lossy().into_owned());
        let mut data = FeatureClass::new(&default, kind, crs, table.fields.clone());
        let mut skipped = 0usize;
        for (shape, values) in shapes.into_iter().zip(table.rows) {
            match shape {
                Some(g) if data.push(Some(g.clone()), values.clone()).is_ok() => {}
                _ => skipped += 1,
            }
        }
        let mut warnings = table.warnings;
        if skipped > 0 {
            warnings.push(format!("{skipped} rows without usable coordinates (or of another shape type) were skipped"));
        }
        let n = data.features.len();
        let id = add_output(s, data, p, &default);
        Ok(json!({ "message": format!("XY Table To Point: {n} features, {skipped} rows skipped"), "layer": id, "warnings": warnings }))
    },
};

/// Column type overrides: `{"COD": "text", …}`.
fn types(p: &Json) -> Result<HashMap<String, FieldType>> {
    match p.get("types") {
        None | Some(Json::Null) => Ok(HashMap::new()),
        Some(Json::Object(o)) => o
            .iter()
            .map(|(k, v)| {
                let kind = v.as_str().and_then(mkaegis_csv::parse_type).ok_or_else(|| Error::param("types", "text, integer, double or boolean"))?;
                Ok((k.clone(), kind))
            })
            .collect(),
        Some(_) => Err(Error::param("types", "an object of column: type")),
    }
}

pub const CALCULATE_GEOMETRY: CommandSpec = CommandSpec {
    id: "management.calculateGeometry",
    label: "Calculate Geometry Attributes",
    ribbon: "Analysis ▸ Tools",
    shortcut: None,
    params: "{layer, field, property: area|areaHectares|areaKm2|length|lengthKm|centroidX|centroidY}",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let property = mkaegis_analysis::Property::parse(req_str(p, "property")?)
            .ok_or_else(|| Error::param("property", "area, areaHectares, areaKm2, length, lengthKm, centroidX or centroidY"))?;
        let field = req_str(p, "field")?;
        let layer = s.map.layer_mut(&layer_key(p, "layer")?)?;
        let n = mkaegis_analysis::calculate_geometry(layer.data_mut(), field, property)?;
        Ok(json!({ "message": format!("{n} rows calculated") }))
    },
};

pub const SUMMARY_STATISTICS: CommandSpec = CommandSpec {
    id: "analysis.summaryStatistics",
    label: "Summary Statistics",
    ribbon: "Analysis ▸ Tools",
    shortcut: None,
    params: "{layer, field, selectedOnly?}",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let rows: Option<Vec<usize>> = opt_bool(p, "selectedOnly")?.unwrap_or(false).then(|| layer.selection.iter().copied().collect());
        let stats = mkaegis_analysis::summary_statistics(&layer.data, req_str(p, "field")?, rows.as_deref())?;
        serde_json::to_value(stats).map_err(|e| Error::Internal(e.to_string()))
    },
};

pub const EXPORT_MAP: CommandSpec = CommandSpec {
    id: "share.exportMap",
    label: "Export Map",
    ribbon: "Share ▸ Output",
    shortcut: None,
    params: "{path, width?, height?} — PNG of the current extent (labels not drawn yet)",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let file = path(p, "path")?;
        let dim = |k: &str, d: f64| -> Result<u32> {
            let v = super::opt_f64(p, k)?.unwrap_or(d);
            if (1.0..=16_384.0).contains(&v) { Ok(v as u32) } else { Err(Error::param(k, "1 to 16384 pixels")) }
        };
        let png = mkaegis_render::export_png(&s.map, dim("width", 1600.0)?, dim("height", 1000.0)?, s.map.extent)?;
        crate::io::write_atomic(&file, &png)?;
        Ok(json!({ "message": format!("map exported to {}", file.display()) }))
    },
};
