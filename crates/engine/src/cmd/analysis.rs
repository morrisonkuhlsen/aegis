//! Analysis tab and the Geoprocessing pane's tools; Share ▸ Export Map.

use mkaegis_core::Error;
use mkaegis_crs::Crs;
use serde_json::{Value as Json, json};

use super::{CommandSpec, Kind, has_layers, layer_key, opt_bool, opt_str, path, req_f64, req_str};
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
        let dim = |k: &str, d: f64| -> Result<u32, Error> {
            let v = super::opt_f64(p, k)?.unwrap_or(d);
            if (1.0..=16_384.0).contains(&v) { Ok(v as u32) } else { Err(Error::param(k, "1 to 16384 pixels")) }
        };
        let png = mkaegis_render::export_png(&s.map, dim("width", 1600.0)?, dim("height", 1000.0)?, s.map.extent)?;
        crate::io::write_atomic(&file, &png)?;
        Ok(json!({ "message": format!("map exported to {}", file.display()) }))
    },
};
