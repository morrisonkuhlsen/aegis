//! Statistics tab: spatial weights (build, inspect, select neighbours, import and export), the
//! global statistics that take them (Moran's I, Geary's C, General G) and the local ones (LISA,
//! Gi* hot spots), which write their results as fields on the layer.

use std::collections::HashMap;
use std::sync::Arc;

use mkaegis_core::{Error, Result, Value};
use mkaegis_geom::GeometryType;
use mkaegis_model::symbology::{HOT_SPOT_COLORS, LISA_COLORS};
use mkaegis_model::weights::{Bandwidth, Kernel, Method, Standardization, Weights};
use mkaegis_model::{Field, FieldType, Layer, Renderer};
use mkaegis_stats::Spec;
use mkaegis_stats::autocorrelation::{self, Statistic};
use mkaegis_stats::explore::{self, Bins};
use mkaegis_stats::local::{self, Correction};
use serde_json::{Value as Json, json};

use super::{CommandSpec, Kind, has_layers, layer_key, mode, opt_bool, opt_f64, opt_str, path, req_str};
use crate::Session;

/// The largest weights file read.
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
/// Permutations a test runs unless told otherwise, and the seed they start from: a stated seed
/// makes a pseudo p-value reproducible.
const DEFAULT_PERMUTATIONS: u32 = 999;
const DEFAULT_SEED: u64 = 12_345;
const AUTOCORRELATION_PARAMS: &str = "{weights, field, permutations? (default 999, 0 for none, up to 99999), seed? (default 12345)} — \
                                      a numeric field without NULLs, on the weights' layer";

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
            // A row's neighbours mean something only while the weights fit their layer.
            layer_of(s, w)?;
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
        let layer = layer_of(s, w)?;
        let (ids, id_field) = ids(layer, opt_str(p, "idField"))?;
        let file = path(p, "path")?;
        let text = match extension(&file).as_str() {
            "gal" => mkaegis_stats::gal::write_gal(&w.neighbours, &ids, &layer.name, &id_field)?,
            "gwt" => mkaegis_stats::gal::write_gwt(&w.neighbours, &ids, &layer.name, &id_field)?,
            other => return Err(Error::param("path", format!("`.{other}`: weights are written as .gal or .gwt"))),
        };
        let message = format!("weights `{}` written to {}", w.name, file.display());
        s.disk.write(&file, text.as_bytes())?;
        Ok(json!({ "message": message }))
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
        if s.disk.len(&file)? > MAX_FILE_BYTES {
            return Err(Error::param("path", format!("larger than {} MB", MAX_FILE_BYTES / 1024 / 1024)));
        }
        let text = s.disk.read_text(&file)?;
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

pub const GLOBAL_MORAN: CommandSpec = CommandSpec {
    id: "stats.globalMoran",
    label: "Spatial Autocorrelation (Global Moran's I)",
    ribbon: "Statistics ▸ Autocorrelation",
    shortcut: None,
    params: AUTOCORRELATION_PARAMS,
    kind: Kind::Query,
    enabled: has_weights,
    run: |s, p| global(s, p, Statistic::MoranI),
};

pub const GEARY_C: CommandSpec = CommandSpec {
    id: "stats.gearyC",
    label: "Geary's C",
    ribbon: "Statistics ▸ Autocorrelation",
    shortcut: None,
    params: AUTOCORRELATION_PARAMS,
    kind: Kind::Query,
    enabled: has_weights,
    run: |s, p| global(s, p, Statistic::GearyC),
};

pub const GENERAL_G: CommandSpec = CommandSpec {
    id: "stats.generalG",
    label: "High/Low Clustering (Getis-Ord General G)",
    ribbon: "Statistics ▸ Autocorrelation",
    shortcut: None,
    params: AUTOCORRELATION_PARAMS,
    kind: Kind::Query,
    enabled: has_weights,
    run: |s, p| global(s, p, Statistic::GeneralG),
};

/// A global statistic of a field for a weights matrix, as a report.
fn global(s: &Session, p: &Json, stat: Statistic) -> Result<Json> {
    let w = find(s, req_str(p, "weights")?)?;
    let layer = layer_of(s, w)?;
    let field = req_str(p, "field")?;
    let values = numeric_values(layer, field, stat.name())?;
    let (permutations, seed) = permutations_and_seed(p)?;
    let report = autocorrelation::global(stat, &values, &w.neighbours, permutations, seed)?;
    let inference = match &report.permutation {
        Some(perm) => format!("pseudo p = {} ({} permutations, seed {})", number(perm.p), perm.permutations, perm.seed),
        None => format!("p = {} (randomization)", number(report.randomization.p)),
    };
    let message = format!(
        "{} of {field} = {} (expected {}), z = {}, {inference}",
        stat.name(),
        number(report.value),
        number(report.expected),
        number(report.randomization.z)
    );
    let mut out = serde_json::to_value(&report).map_err(|e| Error::Internal(e.to_string()))?;
    if let Some(o) = out.as_object_mut() {
        o.insert("message".into(), json!(message));
        o.insert("layer".into(), json!(layer.name));
        o.insert("field".into(), json!(field));
        o.insert("weights".into(), json!(w.name));
    }
    Ok(out)
}

pub const LOCAL_MORAN: CommandSpec = CommandSpec {
    id: "stats.localMoran",
    label: "Cluster and Outlier Analysis (Anselin Local Moran's I)",
    ribbon: "Statistics ▸ Local",
    shortcut: None,
    params: "{weights, field, permutations? (default 999, 0 for the z-score), seed? (default 12345), alpha? (default 0.05), \
             correction?: none|bonferroni|fdr} — writes LISA_I, LISA_Z, LISA_P and LISA_CL on the layer and maps the clusters",
    kind: Kind::Edit,
    enabled: has_weights,
    run: |s, p| {
        let w = find(s, req_str(p, "weights")?)?;
        let layer = layer_of(s, w)?;
        let field = req_str(p, "field")?;
        let values = numeric_values(layer, field, "Local Moran's I")?;
        let (permutations, seed) = permutations_and_seed(p)?;
        let (alpha, correction) = (alpha(p)?, correction(p)?);
        let lisa = local::local_moran(&values, &w.neighbours, permutations, seed, alpha, correction)?;
        let (layer_id, weights_name) = (layer.id, w.name.clone());
        let float = |v: &[Option<f64>]| v.iter().map(|x| x.map_or(Value::Null, Value::Float)).collect::<Vec<_>>();
        let columns = vec![
            ("LISA_I", FieldType::Double, float(&lisa.i)),
            ("LISA_Z", FieldType::Double, float(&lisa.z)),
            ("LISA_P", FieldType::Double, float(&lisa.p)),
            ("LISA_CL", FieldType::Text, lisa.cluster.iter().map(|c| Value::Text(c.name().into())).collect()),
        ];
        let layer = s.map.layers.iter_mut().find(|l| l.id == layer_id).ok_or_else(|| not_found(&weights_name))?;
        write_columns(layer, columns)?;
        layer.renderer = Renderer::categories("LISA_CL", &LISA_COLORS, layer.renderer.base());
        let count = |c: local::Cluster| lisa.cluster.iter().filter(|x| **x == c).count();
        let clusters = json!({
            "High-High": count(local::Cluster::HighHigh), "Low-Low": count(local::Cluster::LowLow),
            "High-Low": count(local::Cluster::HighLow), "Low-High": count(local::Cluster::LowHigh),
            "Not significant": count(local::Cluster::NotSignificant), "No neighbours": count(local::Cluster::Island),
        });
        let m = &lisa.summary;
        Ok(json!({
            "message": format!(
                "Local Moran's I of {field}: {} of {} features significant at {} ({}): {} High-High, {} Low-Low, {} High-Low, {} Low-High",
                m.significant, m.tested, m.alpha, correction_text(m.correction),
                count(local::Cluster::HighHigh), count(local::Cluster::LowLow), count(local::Cluster::HighLow), count(local::Cluster::LowHigh)
            ),
            "fields": ["LISA_I", "LISA_Z", "LISA_P", "LISA_CL"],
            "clusters": clusters,
            "summary": m,
        }))
    },
};

pub const HOT_SPOTS: CommandSpec = CommandSpec {
    id: "stats.hotSpots",
    label: "Hot Spot Analysis (Getis-Ord Gi*)",
    ribbon: "Statistics ▸ Local",
    shortcut: None,
    params: "{weights, field, correction?: none|bonferroni|fdr} — writes GI_Z, GI_P, GI_BIN (−3 to 3) and GI_CL on the layer and maps \
             the hot and cold spots at 90/95/99 % confidence",
    kind: Kind::Edit,
    enabled: has_weights,
    run: |s, p| {
        let w = find(s, req_str(p, "weights")?)?;
        let layer = layer_of(s, w)?;
        let field = req_str(p, "field")?;
        let values = numeric_values(layer, field, "Gi*")?;
        let hs = local::hot_spots(&values, &w.neighbours, correction(p)?)?;
        let (layer_id, weights_name) = (layer.id, w.name.clone());
        let label = |b: i8| HOT_SPOT_COLORS.get(usize::try_from(3 - i32::from(b)).unwrap_or(3)).map_or("Not significant", |c| c.0);
        let columns = vec![
            ("GI_Z", FieldType::Double, hs.z.iter().map(|z| Value::Float(*z)).collect()),
            ("GI_P", FieldType::Double, hs.p.iter().map(|p| Value::Float(*p)).collect()),
            ("GI_BIN", FieldType::Short, hs.bin.iter().map(|b| Value::Int(i64::from(*b))).collect()),
            ("GI_CL", FieldType::Text, hs.bin.iter().map(|b| Value::Text(label(*b).into())).collect()),
        ];
        let layer = s.map.layers.iter_mut().find(|l| l.id == layer_id).ok_or_else(|| not_found(&weights_name))?;
        write_columns(layer, columns)?;
        layer.renderer = Renderer::categories("GI_CL", &HOT_SPOT_COLORS, layer.renderer.base());
        let count = |f: fn(i8) -> bool| hs.bin.iter().filter(|b| f(**b)).count();
        let (hot, cold) = (count(|b| b >= 2), count(|b| b <= -2));
        let m = &hs.summary;
        Ok(json!({
            "message": format!("Gi* of {field}: {hot} hot and {cold} cold spots at 95 % confidence ({}), of {} features", correction_text(m.correction), m.tested),
            "fields": ["GI_Z", "GI_P", "GI_BIN", "GI_CL"],
            "hot": hot,
            "cold": cold,
            "summary": m,
        }))
    },
};

pub const HISTOGRAM: CommandSpec = CommandSpec {
    id: "stats.histogram",
    label: "Histogram",
    ribbon: "Statistics ▸ Charts",
    shortcut: None,
    params: "{layer, field, bins?: sturges|fd|<count up to 200>} — bin edges and counts of a numeric field (NULLs left out)",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let values = column(layer, req_str(p, "field")?)?;
        let rule = opt_str(p, "bins").map_or(Some(Bins::Sturges), Bins::parse).ok_or_else(|| Error::param("bins", "sturges, fd, or 1 to 200"))?;
        let h = explore::histogram(&values, rule)?;
        let missing = values.iter().filter(|x| !x.is_finite()).count();
        Ok(json!({ "message": format!("{} bins", h.counts.len()), "edges": h.edges, "counts": h.counts, "missing": missing }))
    },
};

pub const BOX_PLOT: CommandSpec = CommandSpec {
    id: "stats.boxPlot",
    label: "Box Plot",
    ribbon: "Statistics ▸ Charts",
    shortcut: None,
    params: "{layer, field, hinge? (default 1.5)} — quartiles, fences, whiskers and the rows beyond the fences",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let values = column(layer, req_str(p, "field")?)?;
        let b = explore::box_plot(&values, opt_f64(p, "hinge")?.unwrap_or(1.5))?;
        let mut out = serde_json::to_value(&b).map_err(|e| Error::Internal(e.to_string()))?;
        if let Some(o) = out.as_object_mut() {
            o.insert("message".into(), json!(format!("median {}, {} outliers", number(b.median), b.outliers.len())));
        }
        Ok(out)
    },
};

pub const SCATTER_PLOT: CommandSpec = CommandSpec {
    id: "stats.scatterPlot",
    label: "Scatter Plot",
    ribbon: "Statistics ▸ Charts",
    shortcut: None,
    params: "{layer, x, y} — the least-squares line of y on x, Pearson's r and R² (rows with both values)",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let (x, y) = (column(layer, req_str(p, "x")?)?, column(layer, req_str(p, "y")?)?);
        let f = explore::ols(&x, &y)?;
        let mut out = serde_json::to_value(&f).map_err(|e| Error::Internal(e.to_string()))?;
        if let Some(o) = out.as_object_mut() {
            o.insert(
                "message".into(),
                json!(format!("y = {} + {}·x, r = {}, R² = {}", number(f.intercept), number(f.slope), number(f.r), number(f.r2))),
            );
        }
        Ok(out)
    },
};

pub const MORAN_SCATTER: CommandSpec = CommandSpec {
    id: "stats.moranScatter",
    label: "Moran Scatter Plot",
    ribbon: "Statistics ▸ Charts",
    shortcut: None,
    params: "{weights, field} — standardised values z and their spatial lag Wz by row, and the slope (Moran's I with row-standardised weights)",
    kind: Kind::Query,
    enabled: has_weights,
    run: |s, p| {
        let w = find(s, req_str(p, "weights")?)?;
        let layer = layer_of(s, w)?;
        let field = req_str(p, "field")?;
        let values = numeric_values(layer, field, "the Moran scatter plot")?;
        let m = explore::moran_scatter(&values, &w.neighbours)?;
        Ok(
            json!({ "message": format!("slope {}", number(m.slope)), "slope": m.slope, "z": m.z, "lag": m.lag, "standardize": w.standardization.name() }),
        )
    },
};

/// A numeric field's values by row, NaN where a feature has none.
fn column(layer: &Layer, field: &str) -> Result<Vec<f64>> {
    let i = layer.data.field_index(field).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
    if layer.data.fields.get(i).is_some_and(|f| !f.kind.is_numeric()) {
        return Err(Error::param("field", format!("`{field}` is not numeric")));
    }
    Ok(layer.data.features.iter().map(|f| f.values.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN)).collect())
}

/// A numeric field's values on every feature of `layer`, for `what`: a statistic needs them all.
fn numeric_values(layer: &Layer, field: &str, what: &str) -> Result<Vec<f64>> {
    let i = layer.data.field_index(field).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
    if layer.data.fields.get(i).is_some_and(|f| !f.kind.is_numeric()) {
        return Err(Error::param("field", format!("`{field}` is not numeric")));
    }
    let values: Vec<Option<f64>> = layer.data.features.iter().map(|f| f.values.get(i).and_then(Value::as_f64)).collect();
    let missing = values.iter().filter(|v| v.is_none()).count();
    if missing > 0 {
        let first = values.iter().position(Option::is_none).map_or(0, |r| r + 1);
        return Err(Error::param(
            "field",
            format!("{missing} features have no `{field}` (the first is row {first}): {what} needs a value at every feature"),
        ));
    }
    Ok(values.into_iter().flatten().collect())
}

fn permutations_and_seed(p: &Json) -> Result<(u32, u64)> {
    let permutations = match opt_f64(p, "permutations")? {
        None => DEFAULT_PERMUTATIONS,
        Some(x) if x.fract() == 0.0 && (0.0..=f64::from(autocorrelation::MAX_PERMUTATIONS)).contains(&x) => x as u32,
        Some(_) => return Err(Error::param("permutations", format!("a whole number from 0 to {}", autocorrelation::MAX_PERMUTATIONS))),
    };
    let seed = match p.get("seed") {
        None | Some(Json::Null) => DEFAULT_SEED,
        Some(v) => v.as_u64().ok_or_else(|| Error::param("seed", "a whole number from 0"))?,
    };
    Ok((permutations, seed))
}

fn correction_text(correction: &str) -> &'static str {
    match correction {
        "fdr" => "FDR correction",
        "bonferroni" => "Bonferroni correction",
        _ => "no correction",
    }
}

fn alpha(p: &Json) -> Result<f64> {
    match opt_f64(p, "alpha")? {
        None => Ok(0.05),
        Some(a) if a > 0.0 && a < 1.0 => Ok(a),
        Some(_) => Err(Error::param("alpha", "a significance level between 0 and 1")),
    }
}

fn correction(p: &Json) -> Result<Correction> {
    match opt_str(p, "correction") {
        None => Ok(Correction::None),
        Some(c) => Correction::parse(c).ok_or_else(|| Error::param("correction", "none, bonferroni or fdr")),
    }
}

/// Writes result columns on a layer: an existing field of the same name and type is
/// overwritten (a re-run), a field of another type is an error, a missing one is added.
fn write_columns(layer: &mut Layer, columns: Vec<(&str, FieldType, Vec<Value>)>) -> Result<()> {
    for (name, kind, _) in &columns {
        if let Some(f) = layer.data.fields.iter().find(|f| f.name.eq_ignore_ascii_case(name))
            && f.kind != *kind
        {
            return Err(Error::param("field", format!("the layer has a `{name}` field of another type: rename or delete it first")));
        }
    }
    let data = layer.data_mut();
    for (name, kind, values) in columns {
        let i = match data.field_index(name) {
            Some(i) => i,
            None => data.add_field(Field::new(name, kind))?,
        };
        for (f, v) in data.features.iter_mut().zip(values) {
            if let Some(slot) = f.values.get_mut(i) {
                *slot = v;
            }
        }
    }
    Ok(())
}

/// A statistic for a message: four decimals, or scientific notation when very small or large.
fn number(x: f64) -> String {
    if x != 0.0 && (x.abs() < 1e-3 || x.abs() >= 1e6) { format!("{x:.3e}") } else { format!("{:.4}", x) }
}

/// Adds a weights matrix to the map; its summary is the command's result.
fn add_weights(s: &mut Session, weights: Weights) -> Json {
    let mut out = describe(s, &weights);
    let summary = weights.summary();
    let islands = if summary.islands.is_empty() { String::new() } else { format!(", {} without neighbours", summary.islands.len()) };
    let message = format!("`{}`: {} features, {:.2} neighbours on average{islands}", weights.name, summary.n, summary.mean_neighbours);
    if let Some(o) = out.as_object_mut() {
        o.insert("message".into(), json!(message));
    }
    s.map.weights.push(Arc::new(weights));
    out
}

/// A weights matrix's summary; `problem` says why it no longer fits its layer, if it doesn't.
fn describe(s: &Session, w: &Weights) -> Json {
    let layer = s.map.layers.iter().find(|l| l.id == w.layer).map(|l| l.name.clone());
    json!({
        "name": w.name,
        "layer": layer,
        "problem": layer_of(s, w).err().map(|e| e.to_string()),
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
    s.map.weights.iter().map(|w| &**w).find(|w| w.name.eq_ignore_ascii_case(name)).ok_or_else(|| not_found(name))
}

/// The layer weights were built for, found by id, never by name (a later layer named "3" is not
/// layer 3), and checked to still have the features the weights name (#49).
fn layer_of<'a>(s: &'a Session, w: &Weights) -> Result<&'a Layer> {
    let layer =
        s.map.layers.iter().find(|l| l.id == w.layer).ok_or_else(|| Error::param("weights", format!("`{}`: its layer was removed", w.name)))?;
    w.check_fits(layer.id, layer.data.features.len())?;
    Ok(layer)
}

fn layer_of_mut<'a>(s: &'a mut Session, name: &str) -> Result<&'a mut Layer> {
    let id = layer_of(s, find(s, name)?)?.id;
    s.map.layers.iter_mut().find(|l| l.id == id).ok_or_else(|| not_found(name))
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

/// Features' ids for weights files: an id field's values, or 1-based row numbers. GAL and GWT
/// separate values by spaces, so spaces in an id become `_` ("Porto Celeste" → `Porto_Celeste`);
/// numbers are written in full, not rounded. Ids must stay unique after that, in export and in
/// import alike, or a file would name two features the same (#49).
fn ids(layer: &Layer, field: Option<&str>) -> Result<(Vec<String>, String)> {
    let Some(f) = field else {
        return Ok(((1..=layer.data.features.len()).map(|r| r.to_string()).collect(), "ROW".into()));
    };
    let i = layer.data.field_index(f).ok_or_else(|| Error::NotFound(format!("field `{f}`")))?;
    let mut ids = Vec::with_capacity(layer.data.features.len());
    let mut first: HashMap<String, usize> = HashMap::new();
    for (r, feature) in layer.data.features.iter().enumerate() {
        let id = match feature.values.get(i) {
            Some(Value::Float(x)) => format!("{x}"),
            Some(v) if !v.is_null() => v.to_string().split_whitespace().collect::<Vec<_>>().join("_"),
            _ => return Err(Error::param("idField", format!("row {} has no `{f}`", r + 1))),
        };
        if let Some(earlier) = first.insert(id.clone(), r) {
            return Err(Error::param(
                "idField",
                format!("rows {} and {} have the same id `{id}`: use a field with unique values", earlier + 1, r + 1),
            ));
        }
        ids.push(id);
    }
    Ok((ids, f.to_string()))
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
