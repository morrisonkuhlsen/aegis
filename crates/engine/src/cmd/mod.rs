//! The command registry. Ids follow ArcGIS Pro's ribbon tabs and geoprocessing toolboxes
//! (`map.selectByAttributes`, `analysis.buffer`, `management.project`) plus primitives
//! (`feature.select`, `layer.setVisible`). Each command runs from JSON parameters; with empty
//! parameters it runs with defaults or returns an error — it never opens a dialog.

mod analysis;
mod feature;
mod layer;
mod map;
mod project;
mod stats;

use mkaegis_core::{Error, Result};
use serde_json::Value as Json;

use crate::Session;

/// What a command does to the session, which decides undo and history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Changes the map: undoable, marks the project modified.
    Edit,
    /// Moves the view, changes the selection or the file: in the history, not undoable.
    View,
    /// Reads only, or keeps the map's extent in step with the view: not in the history.
    Query,
}

/// A command: what the ribbon, the shortcuts, the control channel, the CLI and MCP all run.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Where it lives in the ribbon: `Tab ▸ Group`.
    pub ribbon: &'static str,
    pub shortcut: Option<&'static str>,
    /// The parameters, for people and agents.
    pub params: &'static str,
    pub kind: Kind,
    pub enabled: fn(&Session) -> bool,
    pub run: fn(&mut Session, &Json) -> Result<Json>,
}

pub(crate) fn always(_: &Session) -> bool {
    true
}

pub(crate) fn has_layers(s: &Session) -> bool {
    !s.map.layers.is_empty()
}

pub(crate) fn has_selection(s: &Session) -> bool {
    s.map.layers.iter().any(|l| !l.selection.is_empty())
}

/// Every command, in ribbon order.
pub static COMMANDS: &[CommandSpec] = &[
    project::NEW,
    project::OPEN,
    project::SAVE,
    project::SAVE_AS,
    map::ADD_DATA,
    map::ADD_SAMPLE,
    map::SELECT_BY_ATTRIBUTES,
    map::SELECT_BY_LOCATION,
    map::SELECT_BY_RECTANGLE,
    map::CLEAR_SELECTION,
    map::ZOOM_TO_LAYER,
    map::ZOOM_TO_SELECTION,
    map::FULL_EXTENT,
    map::SET_EXTENT,
    map::SET_CRS,
    map::DESCRIBE,
    feature::SELECT,
    feature::IDENTIFY,
    feature::SET_VALUE,
    feature::DELETE,
    layer::REMOVE,
    layer::RENAME,
    layer::SET_VISIBLE,
    layer::MOVE,
    layer::SET_OPACITY,
    layer::SYMBOLOGY,
    layer::LABELS,
    layer::ADD_FIELD,
    layer::CALCULATE_FIELD,
    layer::EXPORT,
    layer::INFO,
    analysis::BUFFER,
    analysis::PROJECT,
    analysis::DEFINE_PROJECTION,
    analysis::CALCULATE_GEOMETRY,
    analysis::SUMMARY_STATISTICS,
    analysis::EXPORT_MAP,
    stats::WEIGHTS,
    stats::WEIGHTS_INFO,
    stats::SELECT_NEIGHBORS,
    stats::REMOVE_WEIGHTS,
    stats::EXPORT_WEIGHTS,
    stats::IMPORT_WEIGHTS,
    stats::GLOBAL_MORAN,
    stats::GEARY_C,
    stats::GENERAL_G,
    stats::LOCAL_MORAN,
    stats::HOT_SPOTS,
    stats::HISTOGRAM,
    stats::BOX_PLOT,
    stats::SCATTER_PLOT,
    stats::MORAN_SCATTER,
    project::UNDO,
    project::REDO,
];

// Parameter helpers: every one tolerates missing, mistyped and hostile values.

pub(crate) fn opt_str<'a>(p: &'a Json, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Json::as_str).map(str::trim).filter(|s| !s.is_empty())
}

pub(crate) fn req_str<'a>(p: &'a Json, key: &str) -> Result<&'a str> {
    opt_str(p, key).ok_or_else(|| Error::param(key, "required"))
}

pub(crate) fn opt_f64(p: &Json, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Number(n)) => n.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| Error::param(key, "not a finite number")),
        Some(Json::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Json::String(s)) => s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).map(Some).ok_or_else(|| Error::param(key, "not a number")),
        Some(_) => Err(Error::param(key, "not a number")),
    }
}

pub(crate) fn req_f64(p: &Json, key: &str) -> Result<f64> {
    opt_f64(p, key)?.ok_or_else(|| Error::param(key, "required"))
}

pub(crate) fn opt_bool(p: &Json, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Bool(b)) => Ok(Some(*b)),
        Some(Json::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(Some(true)),
            "false" | "no" | "0" => Ok(Some(false)),
            _ => Err(Error::param(key, "not true or false")),
        },
        Some(Json::Number(n)) => Ok(Some(n.as_f64().is_some_and(|x| x != 0.0))),
        Some(_) => Err(Error::param(key, "not true or false")),
    }
}

/// A layer reference: its id (number or string) or name.
pub(crate) fn layer_key(p: &Json, key: &str) -> Result<String> {
    match p.get(key) {
        Some(Json::String(s)) if !s.trim().is_empty() => Ok(s.trim().to_string()),
        Some(Json::Number(n)) => Ok(n.to_string()),
        _ => Err(Error::param(key, "required: a layer id or name")),
    }
}

/// A list of row numbers (0-based), capped.
pub(crate) fn rows(p: &Json, key: &str, len: usize) -> Result<Vec<usize>> {
    let list = p.get(key).and_then(Json::as_array).ok_or_else(|| Error::param(key, "required: a list of row numbers"))?;
    if list.len() > mkaegis_core::MAX_ITEMS {
        return Err(Error::param(key, "too many rows"));
    }
    list.iter()
        .map(|v| {
            v.as_u64()
                .and_then(|r| usize::try_from(r).ok())
                .filter(|r| *r < len)
                .ok_or_else(|| Error::param(key, format!("{v} is not a row number below {len}")))
        })
        .collect()
}

pub(crate) fn mode(p: &Json) -> Result<mkaegis_model::SelectionMode> {
    let text = opt_str(p, "mode").unwrap_or("new");
    mkaegis_model::SelectionMode::parse(text).ok_or_else(|| Error::param("mode", "new, add, remove or intersect"))
}

/// A path parameter.
pub(crate) fn path(p: &Json, key: &str) -> Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::from(req_str(p, key)?))
}
