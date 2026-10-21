//! Statistics tab: spatial weights (build, inspect, select neighbours, import and export). The
//! spatial statistics that take them come next.

use mkaegis_core::{Error, Result};
use mkaegis_geom::GeometryType;
use mkaegis_model::Layer;
use mkaegis_model::weights::{Bandwidth, Kernel, Method, Standardization, Weights};
use mkaegis_stats::Spec;
use serde_json::{Value as Json, json};

use super::{CommandSpec, Kind, has_layers, layer_key, mode, opt_bool, opt_f64, opt_str, path, req_str};
use crate::Session;

/// The largest weights file read.
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) fn has_weights(s: &Session) -> bool {
    !s.map.weights.is_empty()
}

pub const WEIGHTS: CommandSpec = CommandSpec {
    id: "stats.weights",
    label: "Spatial Weights",
    ribbon: "Statistics ▸ Weights",
    shortcut: None,
    params: "{layer, method?: queen|rook|knn|distance|kernel, name?, standardize?: row|binary|variance|none, \
             order?, cumulative?, tolerance? (contiguity, metres), k? (knn; adaptive kernel), threshold? (distance, metres; \
             default: the smallest that leaves no island), power? (inverse distance), kernel?: uniform|triangular|bisquare|gaussian, \
             bandwidth? (metres; else adaptive with k), diagonal?} — queen for polygons and knn (k 4) for points by default; row-standardised",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let polygons = layer.data.geometry_type == GeometryType::Polygon;
        let method_name = opt_str(p, "method").unwrap_or(if polygons { "queen" } else { "knn" }).to_ascii_lowercase();
        let count = |key: &str, default: u32| -> Result<u32> {
            match opt_f64(p, key)? {
                None => Ok(default),
                Some(x) if x.fract() == 0.0 && (1.0..=f64::from(u32::MAX)).contains(&x) => Ok(x as u32),
                Some(_) => Err(Error::param(key, "a whole number from 1")),
            }
        };
        let method = match method_name.as_str() {
            "queen" | "rook" => Method::Contiguity {
                queen: method_name == "queen",
                order: count("order", 1)?,
                cumulative: opt_bool(p, "cumulative")?.unwrap_or(true),
            },
            "knn" | "nearest" | "k-nearest" => Method::Nearest { k: count("k", 4)? },
            "distance" | "distanceband" | "band" => {
                let threshold = match opt_f64(p, "threshold")? {
                    Some(t) => t,
                    None => mkaegis_stats::min_threshold(&layer.data)?
                        .map(|t| t * (1.0 + 1e-9))
                        .ok_or_else(|| Error::param("threshold", "needs two located features"))?,
                };
                Method::DistanceBand { threshold, power: opt_f64(p, "power")? }
            }
            "kernel" => {
                let kernel = match opt_str(p, "kernel").unwrap_or("bisquare").to_ascii_lowercase().as_str() {
                    "uniform" => Kernel::Uniform,
                    "triangular" => Kernel::Triangular,
                    "bisquare" | "quartic" => Kernel::Bisquare,
                    "gaussian" => Kernel::Gaussian,
                    other => return Err(Error::param("kernel", format!("unknown kernel `{other}`"))),
                };
                let bandwidth = match opt_f64(p, "bandwidth")? {
                    Some(h) => Bandwidth::Fixed(h),
                    None => Bandwidth::Adaptive(count("k", 8)?),
                };
                Method::Kernel { kernel, bandwidth, diagonal: opt_bool(p, "diagonal")?.unwrap_or(false) }
            }
            other => return Err(Error::param("method", format!("unknown method `{other}`: queen, rook, knn, distance or kernel"))),
        };
        let standardization = standardization(p, Standardization::Row)?;
        let tolerance = opt_f64(p, "tolerance")?.unwrap_or(0.0);
        let default = format!("{}_{}", layer.name, method_name);
        let name = unique_weights_name(s, opt_str(p, "name").unwrap_or(&default));
        let weights = mkaegis_stats::build(&layer.data, layer.id, &name, &Spec { method, standardization, tolerance })?;
        Ok(add_weights(s, weights))
    },
};

pub const WEIGHTS_INFO: CommandSpec = CommandSpec {
    id: "stats.weightsInfo",
    label: "Weights Properties",
    ribbon: "Statistics ▸ Weights",
    shortcut: None,
    params: "{weights?, row?} — a weights matrix's summary (every one when omitted); with row, that feature's neighbours and weights",
    kind: Kind::Query,
    enabled: has_weights,
    run: |s, p| {
        let Some(name) = opt_str(p, "weights") else {
            let all: Vec<Json> = s.map.weights.iter().map(|w| describe(s, w)).collect();
            return Ok(json!({ "weights": all }));
        };
        let w = find(s, name)?;
        let mut out = describe(s, w);
        if let Some(row) = opt_row(p, w.n())?
            && let Some(o) = out.as_object_mut()
        {
            let list: Vec<Json> = w.of(row).iter().map(|(j, x)| json!({ "row": j, "weight": x })).collect();
            o.insert("row".into(), json!(row));
            o.insert("neighbours".into(), json!(list));
        }
        Ok(out)
    },
};

pub const SELECT_NEIGHBORS: CommandSpec = CommandSpec {
    id: "stats.selectNeighbors",
    label: "Select Neighbors",
    ribbon: "Statistics ▸ Weights",
    shortcut: None,
    params: "{weights, row, mode?} — selects a feature's neighbours on the map (mode new|add|remove|intersect)",
    kind: Kind::View,
    enabled: has_weights,
    run: |s, p| {
        let w = find(s, req_str(p, "weights")?)?;
        let row = opt_row(p, w.n())?.ok_or_else(|| Error::param("row", "required"))?;
        let rows: Vec<usize> = w.of(row).iter().map(|(j, _)| *j as usize).collect();
        let selection = mode(p)?;
        let layer = layer_of_mut(s, req_str(p, "weights")?)?;
        layer.select(rows, selection);
        Ok(json!({ "message": format!("{} selected", layer.selection.len()), "selected": layer.selection.len() }))
    },
};

pub const REMOVE_WEIGHTS: CommandSpec = CommandSpec {
    id: "stats.removeWeights",
    label: "Remove Weights",
    ribbon: "Statistics ▸ Weights",
    shortcut: None,
    params: "{weights}",
    kind: Kind::Edit,
    enabled: has_weights,
    run: |s, p| {
        let name = req_str(p, "weights")?;
        let i = s.map.weights.iter().position(|w| w.name.eq_ignore_ascii_case(name)).ok_or_else(|| not_found(name))?;
        s.map.weights.remove(i);
        Ok(json!({ "message": format!("weights `{name}` removed") }))
    },
};

pub const EXPORT_WEIGHTS: CommandSpec = CommandSpec {
    id: "stats.exportWeights",
    label: "Export Weights",
    ribbon: "Statistics ▸ Weights",
    shortcut: None,
    params: "{weights, path, idField?} — .gal (neighbour lists) or .gwt (weights); features by idField's values or 1-based row",
    kind: Kind::View,
    enabled: has_weights,
    run: |s, p| {
        let w = find(s, req_str(p, "weights")?)?;
        let layer = s.map.layer(&w.layer.to_string())?;
        w.check_fits(layer.id, layer.data.features.len())?;
        let (ids, id_field) = ids(layer, opt_str(p, "idField"))?;
        let file = path(p, "path")?;
        let text = match extension(&file).as_str() {
            "gal" => mkaegis_stats::gal::write_gal(&w.neighbours, &ids, &layer.name, &id_field),
            "gwt" => mkaegis_stats::gal::write_gwt(&w.neighbours, &ids, &layer.name, &id_field),
            other => return Err(Error::param("path", format!("`.{other}`: weights are written as .gal or .gwt"))),
        };
        crate::io::write_atomic(&file, text.as_bytes())?;
        Ok(json!({ "message": format!("weights `{}` written to {}", w.name, file.display()) }))
    },
};

pub const IMPORT_WEIGHTS: CommandSpec = CommandSpec {
    id: "stats.importWeights",
    label: "Import Weights",
    ribbon: "Statistics ▸ Weights",
    shortcut: None,
    params: "{layer, path, name?, idField?, standardize?: row|binary|variance|none} — a .gal or .gwt file for the layer's features",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let file = path(p, "path")?;
        if std::fs::metadata(&file)?.len() > MAX_FILE_BYTES {
            return Err(Error::param("path", format!("larger than {} MB", MAX_FILE_BYTES / 1024 / 1024)));
        }
        let text = std::fs::read_to_string(&file)?;
        let (ids, _) = ids(layer, opt_str(p, "idField"))?;
        let ext = extension(&file);
        let raw = match ext.as_str() {
            "gal" => mkaegis_stats::gal::read_gal(&text, &ids)?,
            "gwt" => mkaegis_stats::gal::read_gwt(&text, &ids)?,
            other => return Err(Error::param("path", format!("`.{other}`: weights are read from .gal or .gwt"))),
        };
        // GAL holds no weights: row-standardise by default; GWT's are kept as they are.
        let how = standardization(p, if ext == "gal" { Standardization::Row } else { Standardization::None })?;
        let default = file.file_stem().map_or_else(|| "weights".to_string(), |s| s.to_string_lossy().into_owned());
        let name = unique_weights_name(s, opt_str(p, "name").unwrap_or(&default));
        let weights = Weights {
            name,
            layer: layer.id,
            method: Method::File { path: file.display().to_string() },
            standardization: how,
            neighbours: mkaegis_stats::standardize(raw, how),
        };
        Ok(add_weights(s, weights))
    },
};

/// Adds a weights matrix to the map; its summary is the command's result.
fn add_weights(s: &mut Session, weights: Weights) -> Json {
    let mut out = describe(s, &weights);
    let summary = weights.summary();
    let islands = if summary.islands.is_empty() { String::new() } else { format!(", {} without neighbours", summary.islands.len()) };
    let message = format!("`{}`: {} features, {:.2} neighbours on average{islands}", weights.name, summary.n, summary.mean_neighbours);
    if let Some(o) = out.as_object_mut() {
        o.insert("message".into(), json!(message));
    }
    s.map.weights.push(weights);
    out
}

fn describe(s: &Session, w: &Weights) -> Json {
    let layer = s.map.layer(&w.layer.to_string()).map(|l| l.name.clone()).unwrap_or_default();
    json!({
        "name": w.name,
        "layer": layer,
        "method": method_json(&w.method),
        "standardize": w.standardization.name(),
        "summary": w.summary(),
    })
}

fn method_json(m: &Method) -> Json {
    match m {
        Method::Contiguity { queen, order, cumulative } => {
            json!({ "type": if *queen { "queen" } else { "rook" }, "order": order, "cumulative": cumulative })
        }
        Method::Nearest { k } => json!({ "type": "knn", "k": k }),
        Method::DistanceBand { threshold, power } => json!({ "type": "distance", "threshold": threshold, "power": power }),
        Method::Kernel { kernel, bandwidth, diagonal } => {
            let (fixed, adaptive) = match bandwidth {
                Bandwidth::Fixed(h) => (Some(*h), None),
                Bandwidth::Adaptive(k) => (None, Some(*k)),
            };
            json!({ "type": "kernel", "kernel": format!("{kernel:?}").to_lowercase(), "bandwidth": fixed, "k": adaptive, "diagonal": diagonal })
        }
        Method::File { path } => json!({ "type": "file", "path": path }),
    }
}

fn standardization(p: &Json, default: Standardization) -> Result<Standardization> {
    match opt_str(p, "standardize") {
        None => Ok(default),
        Some(t) => Standardization::parse(t).ok_or_else(|| Error::param("standardize", "row, binary, variance or none")),
    }
}

fn find<'a>(s: &'a Session, name: &str) -> Result<&'a Weights> {
    s.map.weights.iter().find(|w| w.name.eq_ignore_ascii_case(name)).ok_or_else(|| not_found(name))
}

fn layer_of_mut<'a>(s: &'a mut Session, name: &str) -> Result<&'a mut Layer> {
    let w = find(s, name)?;
    let (id, n) = (w.layer, w.n());
    let layer = s.map.layer_mut(&id.to_string())?;
    if layer.data.features.len() != n {
        return Err(Error::param(
            "weights",
            format!("`{name}` was built for {n} features and the layer has {}: build it again", layer.data.features.len()),
        ));
    }
    Ok(layer)
}

fn not_found(name: &str) -> Error {
    Error::NotFound(format!("weights `{name}`"))
}

/// A row parameter below `n`.
fn opt_row(p: &Json, n: usize) -> Result<Option<usize>> {
    match p.get("row") {
        None | Some(Json::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|r| usize::try_from(r).ok())
            .filter(|r| *r < n)
            .map(Some)
            .ok_or_else(|| Error::param("row", format!("a row number below {n}"))),
    }
}

/// Features' ids for weights files: an id field's values, or 1-based row numbers.
fn ids(layer: &Layer, field: Option<&str>) -> Result<(Vec<String>, String)> {
    match field {
        None => Ok(((1..=layer.data.features.len()).map(|r| r.to_string()).collect(), "ROW".into())),
        Some(f) => {
            let i = layer.data.field_index(f).ok_or_else(|| Error::NotFound(format!("field `{f}`")))?;
            let ids = layer
                .data
                .features
                .iter()
                .enumerate()
                .map(|(r, feature)| match feature.values.get(i) {
                    Some(v) if !v.is_null() => Ok(v.to_string().split_whitespace().collect::<Vec<_>>().join("_")),
                    _ => Err(Error::param("idField", format!("row {} has no `{f}`", r + 1))),
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((ids, f.to_string()))
        }
    }
}

fn unique_weights_name(s: &Session, base: &str) -> String {
    let base: String = base.trim().chars().take(64).collect();
    let base = if base.is_empty() { "weights".to_string() } else { base };
    let taken = |n: &str| s.map.weights.iter().any(|w| w.name.eq_ignore_ascii_case(n));
    if !taken(&base) {
        return base;
    }
    (1..).map(|i| format!("{base}_{i}")).find(|n| !taken(n)).unwrap_or(base)
}

fn extension(path: &std::path::Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}
