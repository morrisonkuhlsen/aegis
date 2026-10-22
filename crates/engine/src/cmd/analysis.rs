//! Analysis tab and the Geoprocessing pane's tools; Share ▸ Export Map.

use std::collections::HashMap;

use mkaegis_core::{Error, Result, Value};
use mkaegis_crs::Crs;
use mkaegis_geom::{Coord, Geometry, GeometryType};
use mkaegis_model::{FeatureClass, Field, FieldType};
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
    params: "{layer, field or fields [..], caseField?, weightField?, percentiles? [0–100], quantileType? (Hyndman–Fan 1–9, default 7), \
             selectedOnly?, path? (.csv: the table written)} — N, missing, sum, mean, sample variance and SD, SE of the mean, CV, \
             min, quartiles, median, max, IQR, skewness (G1), excess kurtosis (G2), by case if asked",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let rows = rows_of(layer, p)?;
        let fields: Vec<String> = match (p.get("fields"), opt_str(p, "field")) {
            (Some(Json::Array(a)), _) => {
                a.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| Error::param("fields", "field names"))).collect::<Result<_>>()?
            }
            (_, Some(f)) => vec![f.to_string()],
            _ => return Err(Error::param("field", "a numeric field, or fields")),
        };
        let percentiles: Vec<f64> = match p.get("percentiles") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Array(a)) if a.len() <= 99 => {
                a.iter().map(|v| v.as_f64().ok_or_else(|| Error::param("percentiles", "numbers from 0 to 100"))).collect::<Result<_>>()?
            }
            Some(_) => return Err(Error::param("percentiles", "a list of up to 99 numbers from 0 to 100")),
        };
        let kind = match opt_f64(p, "quantileType")? {
            None => 7,
            Some(k) if k.fract() == 0.0 && (1.0..=9.0).contains(&k) => k as u8,
            Some(_) => return Err(Error::param("quantileType", "a Hyndman–Fan type from 1 to 9")),
        };
        let weights = match opt_str(p, "weightField") {
            Some(w) => Some(numeric_column(layer, w, &rows)?),
            None => None,
        };
        // The groups: every row, or the rows of each value of the case field.
        let groups: Vec<(Option<String>, Vec<usize>)> = match opt_str(p, "caseField") {
            None => vec![(None, (0..rows.len()).collect())],
            Some(case) => {
                let values = text_column(layer, case, &rows)?;
                let mut by: std::collections::BTreeMap<String, Vec<usize>> = std::collections::BTreeMap::new();
                for (k, v) in values.into_iter().enumerate() {
                    by.entry(v.unwrap_or_else(|| "NULL".into())).or_default().push(k);
                }
                if by.len() > 10_000 {
                    return Err(Error::param("caseField", "more than 10,000 cases"));
                }
                by.into_iter().map(|(k, v)| (Some(k), v)).collect()
            }
        };
        let mut table = Vec::new();
        for field in &fields {
            let column = numeric_column(layer, field, &rows)?;
            for (case, members) in &groups {
                let values: Vec<Option<f64>> = members.iter().map(|k| column.get(*k).copied().flatten()).collect();
                let w: Option<Vec<Option<f64>>> = weights.as_ref().map(|ws| members.iter().map(|k| ws.get(*k).copied().flatten()).collect());
                let summary = mkaegis_stats::descriptive::summarize(&values, w.as_deref(), &percentiles, kind)?;
                table.push((field.clone(), case.clone(), summary));
            }
        }
        let written = match opt_str(p, "path") {
            Some(_) => {
                let file = path(p, "path")?;
                crate::io::write_atomic(&file, summary_csv(&table, opt_str(p, "caseField")).as_bytes())?;
                Some(file.display().to_string())
            }
            None => None,
        };
        let rows_json: Vec<Json> = table
            .iter()
            .map(|(field, case, summary)| {
                let mut v = serde_json::to_value(summary).unwrap_or(Json::Null);
                if let Some(o) = v.as_object_mut() {
                    o.insert("field".into(), json!(field));
                    o.insert("case".into(), json!(case));
                }
                v
            })
            .collect();
        let mut out = json!({
            "message": format!(
                "{} {}{}{}",
                fields.len(),
                if fields.len() == 1 { "field" } else { "fields" },
                if groups.len() > 1 { format!(" in {} cases", groups.len()) } else { String::new() },
                written.as_ref().map_or(String::new(), |f| format!(", written to {f}"))
            ),
            "statistics": rows_json,
            "definitions": "sample variance and SD (n − 1); skewness G1 and excess kurtosis G2 (unweighted); quantiles by Hyndman–Fan's type",
        });
        // One field without cases: its numbers at the top too, as before.
        if let ([(field, None, summary)], Some(o)) = (table.as_slice(), out.as_object_mut()) {
            o.insert("field".into(), json!(field));
            o.insert("count".into(), json!(summary.n as u64));
            o.insert("nulls".into(), json!(summary.missing));
            for (k, v) in [("sum", Some(summary.sum)), ("mean", summary.mean), ("min", summary.min), ("max", summary.max), ("std", summary.sd)] {
                o.insert(k.into(), json!(v));
            }
        }
        Ok(out)
    },
};

pub const FREQUENCIES: CommandSpec = CommandSpec {
    id: "analysis.frequencies",
    label: "Frequencies",
    ribbon: "Analysis ▸ Tools",
    shortcut: None,
    params: "{layer, field, selectedOnly?} — each value's count and percentage, most frequent first",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let rows = rows_of(layer, p)?;
        let values = text_column(layer, req_str(p, "field")?, &rows)?;
        let table = mkaegis_stats::descriptive::frequencies(&values);
        Ok(json!({ "message": format!("{} distinct values in {} rows", table.len(), values.len()), "frequencies": table }))
    },
};

pub const CROSS_TABULATION: CommandSpec = CommandSpec {
    id: "analysis.crossTabulation",
    label: "Crosstabs",
    ribbon: "Analysis ▸ Tools",
    shortcut: None,
    params: "{layer, rows, columns, selectedOnly?} — counts of two categorical fields, Pearson's χ², df, p, Cramér's V, and expected counts below 5",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let rows = rows_of(layer, p)?;
        let (r, c) = (text_column(layer, req_str(p, "rows")?, &rows)?, text_column(layer, req_str(p, "columns")?, &rows)?);
        let t = mkaegis_stats::descriptive::cross_tab(&r, &c)?;
        let warning = (t.low_expected > 0)
            .then(|| format!("{} of {} cells expect fewer than 5: χ² may be unreliable", t.low_expected, t.rows.len() * t.columns.len()));
        let mut out = serde_json::to_value(&t).map_err(|e| Error::Internal(e.to_string()))?;
        if let Some(o) = out.as_object_mut() {
            o.insert("message".into(), json!(format!("χ² = {:.4}, df = {}, p = {:.4}, Cramér's V = {:.4}", t.chi2, t.df, t.p, t.cramers_v)));
            o.insert("warnings".into(), json!(warning.into_iter().collect::<Vec<_>>()));
        }
        Ok(out)
    },
};

pub const CORRELATION: CommandSpec = CommandSpec {
    id: "analysis.correlation",
    label: "Correlations",
    ribbon: "Analysis ▸ Tools",
    shortcut: None,
    params: "{layer, fields [..], method?: pearson|spearman, selectedOnly?} — the correlation matrix with n and two-sided p per pair (pairwise complete)",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let rows = rows_of(layer, p)?;
        let fields: Vec<String> = match p.get("fields") {
            Some(Json::Array(a)) if a.len() <= 64 => {
                a.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| Error::param("fields", "field names"))).collect::<Result<_>>()?
            }
            _ => return Err(Error::param("fields", "a list of 2 to 64 numeric fields")),
        };
        let spearman = match opt_str(p, "method").unwrap_or("pearson").to_ascii_lowercase().as_str() {
            "pearson" => false,
            "spearman" => true,
            other => return Err(Error::param("method", format!("`{other}`: pearson or spearman"))),
        };
        let columns: Vec<Vec<Option<f64>>> = fields.iter().map(|f| numeric_column(layer, f, &rows)).collect::<Result<_>>()?;
        let c = mkaegis_stats::descriptive::correlation(&fields, &columns, spearman)?;
        let mut out = serde_json::to_value(&c).map_err(|e| Error::Internal(e.to_string()))?;
        if let Some(o) = out.as_object_mut() {
            o.insert("message".into(), json!(format!("{} correlations of {} fields", c.method, fields.len())));
        }
        Ok(out)
    },
};

/// The rows a statistic runs on: the selection, or every feature.
fn rows_of(layer: &mkaegis_model::Layer, p: &Json) -> Result<Vec<usize>> {
    Ok(if opt_bool(p, "selectedOnly")?.unwrap_or(false) {
        layer.selection.iter().copied().collect()
    } else {
        (0..layer.data.features.len()).collect()
    })
}

/// A numeric field's values at `rows`; a Text field is refused (#4).
fn numeric_column(layer: &mkaegis_model::Layer, field: &str, rows: &[usize]) -> Result<Vec<Option<f64>>> {
    let i = layer.data.field_index(field).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
    if let Some(f) = layer.data.fields.get(i).filter(|f| !f.kind.is_numeric()) {
        return Err(Error::param("field", format!("`{}` is a {} field; a numeric one is needed (Short, Long or Double)", f.name, f.kind.name())));
    }
    Ok(rows.iter().map(|r| layer.data.value(*r, i).and_then(Value::as_f64)).collect())
}

/// Any field's values at `rows` as text (categories), `None` for NULL.
fn text_column(layer: &mkaegis_model::Layer, field: &str, rows: &[usize]) -> Result<Vec<Option<String>>> {
    let i = layer.data.field_index(field).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
    Ok(rows.iter().map(|r| layer.data.value(*r, i).filter(|v| !v.is_null()).map(Value::to_string)).collect())
}

/// The summary table as CSV, one row per field and case.
fn summary_csv(table: &[(String, Option<String>, mkaegis_stats::descriptive::Summary)], case: Option<&str>) -> String {
    let percentiles: Vec<f64> = table.first().map(|t| t.2.percentiles.iter().map(|p| p.0).collect()).unwrap_or_default();
    let mut fields = vec![Field::new("FIELD", FieldType::Text)];
    if let Some(c) = case {
        fields.push(Field::new(c, FieldType::Text));
    }
    for name in ["N", "MISSING", "SUM", "MEAN", "VARIANCE", "SD", "SE_MEAN", "CV", "MIN", "Q1", "MEDIAN", "Q3", "MAX", "IQR", "SKEWNESS", "KURTOSIS"]
    {
        fields.push(Field::new(name, FieldType::Double));
    }
    for p in &percentiles {
        fields.push(Field::new(&format!("P{}", format!("{p}").replace('.', "_")), FieldType::Double));
    }
    let rows: Vec<Vec<Value>> = table
        .iter()
        .map(|(field, c, s)| {
            let num = |x: Option<f64>| x.map_or(Value::Null, Value::Float);
            let mut row = vec![Value::Text(field.clone())];
            if case.is_some() {
                row.push(c.clone().map_or(Value::Null, Value::Text));
            }
            row.extend([
                Value::Float(s.n),
                Value::Int(s.missing as i64),
                Value::Float(s.sum),
                num(s.mean),
                num(s.variance),
                num(s.sd),
                num(s.se_mean),
                num(s.cv),
                num(s.min),
                num(s.q1),
                num(s.median),
                num(s.q3),
                num(s.max),
                num(s.iqr),
                num(s.skewness),
                num(s.kurtosis),
            ]);
            row.extend(s.percentiles.iter().map(|p| Value::Float(p.1)));
            row
        })
        .collect();
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
        let png = mkaegis_render::export_png(&s.map, dim("width", 1600.0)?, dim("height", 1000.0)?, s.map.extent)?;
        crate::io::write_atomic(&file, &png)?;
        Ok(json!({ "message": format!("map exported to {}", file.display()) }))
    },
};
