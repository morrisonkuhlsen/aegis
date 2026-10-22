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
                let t = crate::io::read_table(&s.disk, &file, &types(p)?)?;
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
        let table = crate::io::read_table(&s.disk, &file, &types(p)?)?;
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
    params: "{layer, fields: [numeric fields] (or field), caseField?, weightField? (frequency weights), percentiles? [0–100], quantileType? (Hyndman–Fan 1–9, default 6 as MK Sigma and SPSS), selectedOnly?, path? (.csv)} — N, missing, sum, mean, SD, variance, SE of the mean, min, quartiles, max, range, IQR, skewness and kurtosis with their SEs, CV and percentiles of each field, for each value of the case field",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let fc = &layer.data;
        let numeric = |name: &str, param: &str| -> Result<usize> {
            let i = fc.field_index(name).ok_or_else(|| Error::NotFound(format!("field `{name}`")))?;
            match fc.fields.get(i) {
                Some(f) if f.kind.is_numeric() => Ok(i),
                Some(f) => Err(Error::param(
                    param,
                    format!("`{}` is a {} field; Summary Statistics needs a numeric one (Short, Long or Double)", f.name, f.kind.name()),
                )),
                None => Err(Error::NotFound(format!("field `{name}`"))),
            }
        };
        let names: Vec<String> = match (p.get("fields"), opt_str(p, "field")) {
            (Some(Json::Array(a)), _) => {
                a.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| Error::param("fields", "field names"))).collect::<Result<_>>()?
            }
            (_, Some(f)) => vec![f.to_string()],
            _ => return Err(Error::param("fields", "one or more numeric fields")),
        };
        if names.is_empty() || names.len() > MAX_STATISTICS_FIELDS {
            return Err(Error::param("fields", format!("1 to {MAX_STATISTICS_FIELDS} numeric fields")));
        }
        let columns: Vec<(String, usize)> = names.iter().map(|n| numeric(n, "fields").map(|i| (n.clone(), i))).collect::<Result<_>>()?;
        let case = opt_str(p, "caseField").map(|n| fc.field_index(n).ok_or_else(|| Error::NotFound(format!("field `{n}`")))).transpose()?;
        let weight = opt_str(p, "weightField").map(|n| numeric(n, "weightField")).transpose()?;
        let percentiles: Vec<f64> = match p.get("percentiles") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Array(a)) if a.len() <= 99 => {
                a.iter().map(|v| v.as_f64().ok_or_else(|| Error::param("percentiles", "numbers from 0 to 100"))).collect::<Result<_>>()?
            }
            Some(_) => return Err(Error::param("percentiles", "a list of at most 99 numbers from 0 to 100")),
        };
        let kind = match opt_f64(p, "quantileType")? {
            None => mkaegis_stats::descriptive::DEFAULT_QUANTILE_TYPE,
            Some(t) if t.fract() == 0.0 && (1.0..=9.0).contains(&t) => t as u8,
            Some(_) => return Err(Error::param("quantileType", "Hyndman–Fan type 1 to 9")),
        };
        let rows: Vec<usize> =
            if opt_bool(p, "selectedOnly")?.unwrap_or(false) { layer.selection.iter().copied().collect() } else { (0..fc.features.len()).collect() };
        let groups = group_rows(fc, &rows, case)?;
        let weight_of = |r: usize| weight.map_or(1.0, |w| fc.value(r, w).and_then(Value::as_f64).unwrap_or(f64::NAN));
        let mut table = Vec::with_capacity(groups.len());
        for (key, members) in &groups {
            let mut stats = Vec::with_capacity(columns.len());
            for (name, i) in &columns {
                let d = mkaegis_stats::descriptive::describe(
                    members.iter().map(|r| (fc.value(*r, *i).and_then(Value::as_f64), weight_of(*r))),
                    &percentiles,
                    kind,
                )?;
                stats.push((name.clone(), d));
            }
            table.push((key.clone(), stats));
        }
        let case_name = case.and_then(|i| fc.fields.get(i)).map(|f| f.name.clone());
        let weight_name = weight.and_then(|i| fc.fields.get(i)).map(|f| f.name.clone());
        let csv = opt_str(p, "path").map(|_| statistics_csv(case_name.as_deref(), &table, &percentiles));
        let out = json!({
            "message": format!("{} {} in {} {}", columns.len(), if columns.len() == 1 { "field" } else { "fields" }, table.len(), if table.len() == 1 { "group" } else { "groups" }),
            "fields": names,
            "caseField": case_name,
            "weightField": weight_name,
            "quantileType": kind,
            "percentiles": percentiles,
            "groups": table.iter().map(|(key, stats)| json!({
                "case": key,
                "statistics": stats.iter().map(|(name, d)| {
                    let mut v = serde_json::to_value(d).unwrap_or(Json::Null);
                    if let Some(o) = v.as_object_mut() {
                        o.insert("field".into(), json!(name));
                    }
                    v
                }).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "definitions": {
                "sd": "sample: divided by N − 1",
                "weights": if weight_name.is_some() { "frequency: a case of weight w counts as w cases; a missing, zero or negative weight leaves it out" } else { "none" },
                "quantiles": format!("Hyndman–Fan type {kind}"),
                "skewness": "adjusted Fisher–Pearson G1",
                "kurtosis": "excess, G2",
                "cv": "SD / |mean|",
            },
        });
        if let (Some(csv), Some(file)) = (csv, opt_str(p, "path")) {
            s.disk.write(std::path::Path::new(file), csv.as_bytes())?;
        }
        Ok(out)
    },
};

/// The most fields one Summary Statistics run describes.
const MAX_STATISTICS_FIELDS: usize = 200;
/// The most groups a case field may make.
const MAX_GROUPS: usize = 10_000;

/// `rows` by the value of the `case` field (one group of all of them without one), the groups
/// in the value's order with Null first.
fn group_rows(fc: &FeatureClass, rows: &[usize], case: Option<usize>) -> Result<Vec<(Value, Vec<usize>)>> {
    let Some(c) = case else { return Ok(vec![(Value::Null, rows.to_vec())]) };
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut groups: Vec<(Value, Vec<usize>)> = Vec::new();
    for &r in rows {
        let v = fc.value(r, c).cloned().unwrap_or(Value::Null);
        let key = format!("{v:?}");
        let at = match index.get(&key) {
            Some(at) => *at,
            None => {
                if groups.len() >= MAX_GROUPS {
                    return Err(Error::param("caseField", format!("more than {MAX_GROUPS} distinct values")));
                }
                index.insert(key, groups.len());
                groups.push((v, Vec::new()));
                groups.len() - 1
            }
        };
        if let Some(g) = groups.get_mut(at) {
            g.1.push(r);
        }
    }
    let rank = |v: &Value| match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Int(_) | Value::Float(_) => 2,
        Value::Text(_) => 3,
    };
    groups.sort_by(|(a, _), (b, _)| {
        rank(a).cmp(&rank(b)).then_with(|| match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            _ => a.to_string().cmp(&b.to_string()),
        })
    });
    Ok(groups)
}

/// The statistics as a CSV table: a row per group and field.
fn statistics_csv(case: Option<&str>, table: &[(Value, Vec<(String, mkaegis_stats::descriptive::Descriptives)>)], percentiles: &[f64]) -> String {
    use mkaegis_model::Field;
    let mut fields: Vec<Field> = Vec::new();
    if let Some(c) = case {
        fields.push(Field::new(c, FieldType::Text));
    }
    fields.push(Field::new("FIELD", FieldType::Text));
    let names = [
        "N", "CASES", "MISSING", "SUM", "MEAN", "SD", "VARIANCE", "SE_MEAN", "MIN", "Q1", "MEDIAN", "Q3", "MAX", "RANGE", "IQR", "SKEWNESS",
        "SE_SKEW", "KURTOSIS", "SE_KURT", "CV",
    ];
    fields.extend(names.iter().map(|n| Field::new(n, FieldType::Double)));
    fields.extend(percentiles.iter().map(|p| Field::new(&format!("P{}", p.to_string().replace('.', "_")), FieldType::Double)));
    let num = |x: Option<f64>| x.map_or(Value::Null, Value::Float);
    let mut rows: Vec<Vec<Value>> = Vec::new();
    for (key, stats) in table {
        for (name, d) in stats {
            let mut row = Vec::with_capacity(fields.len());
            if case.is_some() {
                row.push(if key.is_null() { Value::Null } else { Value::Text(key.to_string()) });
            }
            row.push(Value::Text(name.clone()));
            row.extend([
                Value::Float(d.n),
                Value::Float(d.cases as f64),
                Value::Float(d.missing as f64),
                Value::Float(d.sum),
                num(d.mean),
                num(d.sd),
                num(d.variance),
                num(d.se_mean),
                num(d.min),
                num(d.q1),
                num(d.median),
                num(d.q3),
                num(d.max),
                num(d.range),
                num(d.iqr),
                num(d.skewness),
                num(d.se_skewness),
                num(d.kurtosis),
                num(d.se_kurtosis),
                num(d.cv),
            ]);
            row.extend(d.percentiles.iter().map(|q| num(q.value)));
            rows.push(row);
        }
    }
    let refs: Vec<&[Value]> = rows.iter().map(Vec::as_slice).collect();
    mkaegis_csv::write(&fields, &refs)
}

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
        let png = mkaegis_render::export_png(&s.map, dim("width", 1600.0)?, dim("height", 1000.0)?, s.map.extent, Some(&s.tiles))?;
        // Commands never fetch: the basemap is what the map's views have fetched.
        let mut warnings = Vec::new();
        if s.map.basemap.is_some() && s.map.crs != mkaegis_crs::Crs::WEB_MERCATOR {
            warnings.push("the basemap is drawn in Web Mercator maps only".to_string());
        }
        s.disk.write(&file, &png)?;
        Ok(json!({ "message": format!("map exported to {}", file.display()), "warnings": warnings }))
    },
};
