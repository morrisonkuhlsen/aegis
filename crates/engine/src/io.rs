//! Files in and out: data by extension (GeoJSON, shapefile) and the project file (`.aegis`,
//! the map as JSON).

use std::path::Path;

use mkaegis_core::{Error, Result};
use mkaegis_model::{FeatureClass, Map};

/// The extension of the project file.
pub const PROJECT_EXTENSION: &str = "aegis";

/// The data file extensions Add Data opens.
pub const DATA_EXTENSIONS: [&str; 3] = ["geojson", "json", "shp"];

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "layer".into())
}

/// A feature class from a data file, and any warnings.
pub fn read_data(path: &Path) -> Result<(FeatureClass, Vec<String>)> {
    match extension(path).as_str() {
        "geojson" | "json" => {
            let text = std::fs::read_to_string(path)?;
            let r = mkaegis_geojson::read(&text, &stem(path))?;
            let warnings = if r.skipped > 0 { vec![format!("{} features of another geometry type skipped", r.skipped)] } else { Vec::new() };
            Ok((r.data, warnings))
        }
        "shp" => {
            let r = mkaegis_shp::read_path(path)?;
            Ok((r.data, r.warnings))
        }
        other => Err(Error::NotAvailable(format!("reading `.{other}` files"))),
    }
}

/// Writes a feature class to a data file chosen by extension.
pub fn write_data(fc: &FeatureClass, path: &Path) -> Result<()> {
    match extension(path).as_str() {
        "geojson" | "json" => Ok(std::fs::write(path, mkaegis_geojson::write(fc)?)?),
        "shp" => mkaegis_shp::write_path(fc, path),
        other => Err(Error::NotAvailable(format!("writing `.{other}` files"))),
    }
}

pub fn save_project(map: &Map, path: &Path) -> Result<()> {
    let text = serde_json::to_string_pretty(&serde_json::json!({ "format": "aegis", "version": 1, "map": map }))
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(std::fs::write(path, text)?)
}

pub fn open_project(path: &Path) -> Result<Map> {
    let text = std::fs::read_to_string(path)?;
    let mut root: serde_json::Value = serde_json::from_str(&text).map_err(|e| Error::format("project", e.to_string()))?;
    if root.get("format").and_then(serde_json::Value::as_str) != Some("aegis") {
        return Err(Error::format("project", "not a MK aeGIS project"));
    }
    let map = root.get_mut("map").map(serde_json::Value::take).ok_or_else(|| Error::format("project", "no map"))?;
    serde_json::from_value(map).map_err(|e| Error::format("project", e.to_string()))
}
