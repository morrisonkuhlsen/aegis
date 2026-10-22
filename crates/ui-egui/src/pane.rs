//! The right-hand pane: Geoprocessing (find a tool, fill its parameters, run it), Symbology of
//! the active layer, the geoprocessing History, and the linked Charts.

use egui::Ui;
use mkaegis_geom::GeometryType;
use mkaegis_model::Renderer;
use serde_json::{Value as Json, json};

use crate::MapApp;
use crate::theme::Tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Geoprocessing,
    Symbology,
    History,
    Charts,
}

/// The tools the Geoprocessing pane offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Buffer,
    Project,
    CalculateGeometry,
    SelectByAttributes,
    SelectByLocation,
    SummaryStatistics,
    SpatialWeights,
    SpatialAutocorrelation,
}

impl Tool {
    const ALL: [Tool; 8] = [
        Tool::Buffer,
        Tool::Project,
        Tool::CalculateGeometry,
        Tool::SelectByAttributes,
        Tool::SelectByLocation,
        Tool::SummaryStatistics,
        Tool::SpatialWeights,
        Tool::SpatialAutocorrelation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Buffer => "Buffer (Analysis Tools)",
            Tool::Project => "Project (Data Management Tools)",
            Tool::CalculateGeometry => "Calculate Geometry Attributes (Data Management Tools)",
            Tool::SelectByAttributes => "Select Layer By Attribute (Data Management Tools)",
            Tool::SelectByLocation => "Select Layer By Location (Data Management Tools)",
            Tool::SummaryStatistics => "Summary Statistics (Analysis Tools)",
            Tool::SpatialWeights => "Spatial Weights (Spatial Statistics Tools)",
            Tool::SpatialAutocorrelation => "Spatial Autocorrelation (Spatial Statistics Tools)",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Tool::Buffer => "Creates buffer polygons around input features at a distance.",
            Tool::Project => "Projects features into a new coordinate system.",
            Tool::CalculateGeometry => "Writes area, length or centroid coordinates into a field.",
            Tool::SelectByAttributes => "Selects features with a SQL clause.",
            Tool::SelectByLocation => "Selects features by their spatial relation to another layer's.",
            Tool::SummaryStatistics => "Count, sum, mean, minimum, maximum and standard deviation of a field.",
            Tool::SpatialWeights => "Who neighbours whom: contiguity, nearest neighbours, distance bands or kernels, for the spatial statistics.",
            Tool::SpatialAutocorrelation => {
                "Is a field clustered, dispersed or random in space, and where? Global Moran's I, Geary's C, General G; local Moran's I (LISA) and Gi* hot spots, mapped."
            }
        }
    }
}

/// The Geoprocessing pane's form.
#[derive(Debug, Clone, Default)]
pub struct GpState {
    pub tool: Option<Tool>,
    pub search: String,
    layer: Option<u32>,
    other: Option<u32>,
    distance: String,
    unit: &'static str,
    epsg: String,
    field: String,
    property: &'static str,
    clause: String,
    relation: &'static str,
    output: String,
    /// Spatial Weights: method, k, contiguity order, distance threshold (metres), standardisation.
    method: &'static str,
    k: String,
    order: String,
    threshold: String,
    standardize: &'static str,
    /// Spatial Autocorrelation: the weights, the statistic's command, permutations, seed, and
    /// for the local statistics the significance level and the multiple-testing correction.
    weights: String,
    pub statistic: &'static str,
    permutations: String,
    seed: String,
    alpha: String,
    correction: &'static str,
    selected_only: bool,
    result: Option<Json>,
}

impl GpState {
    /// Opens `tool` with `layer` as its input.
    pub fn open(&mut self, tool: Tool, layer: Option<u32>) {
        *self = GpState {
            tool: Some(tool),
            search: std::mem::take(&mut self.search),
            layer,
            distance: "1".into(),
            unit: "kilometers",
            epsg: "31983".into(),
            property: "areaHectares",
            relation: "intersect",
            method: "queen",
            k: "4".into(),
            order: "1".into(),
            standardize: "row",
            statistic: "stats.globalMoran",
            permutations: "999".into(),
            seed: "12345".into(),
            alpha: "0.05".into(),
            correction: "none",
            ..GpState::default()
        };
    }
}

fn layer_combo(ui: &mut Ui, app: &MapApp, label: &str, id: &str, value: &mut Option<u32>, filter: impl Fn(GeometryType) -> bool) {
    let layers: Vec<(u32, String)> = app.session.map.layers.iter().filter(|l| filter(l.data.geometry_type)).map(|l| (l.id, l.name.clone())).collect();
    if value.is_none_or(|v| !layers.iter().any(|(i, _)| *i == v)) {
        *value = layers.first().map(|(i, _)| *i);
    }
    ui.label(label);
    let shown = layers.iter().find(|(i, _)| Some(*i) == *value).map(|(_, n)| n.clone()).unwrap_or_else(|| "—".into());
    egui::ComboBox::from_id_salt(id).width(ui.available_width()).selected_text(shown).show_ui(ui, |ui| {
        for (i, n) in &layers {
            ui.selectable_value(value, Some(*i), n);
        }
    });
}

fn field_combo(ui: &mut Ui, app: &MapApp, layer: Option<u32>, label: &str, id: &str, value: &mut String, numeric_only: bool) {
    let fields: Vec<String> = layer
        .and_then(|l| app.session.map.layers.iter().find(|x| x.id == l))
        .map(|l| l.data.fields.iter().filter(|f| !numeric_only || f.kind.is_numeric()).map(|f| f.name.clone()).collect())
        .unwrap_or_default();
    if !fields.contains(value) {
        *value = fields.first().cloned().unwrap_or_default();
    }
    ui.label(label);
    egui::ComboBox::from_id_salt(id).width(ui.available_width()).selected_text(value.as_str()).show_ui(ui, |ui| {
        for f in &fields {
            ui.selectable_value(value, f.clone(), f);
        }
    });
}

fn gp(app: &mut MapApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(tool) = app.gp.tool else {
        ui.add(egui::TextEdit::singleline(&mut app.gp.search).hint_text("Find Tools").desired_width(f32::INFINITY));
        ui.add_space(6.0);
        let q = app.gp.search.to_lowercase();
        for tool in
            Tool::ALL.into_iter().filter(|t| q.is_empty() || t.name().to_lowercase().contains(&q) || t.description().to_lowercase().contains(&q))
        {
            let r = ui.add(egui::Button::new(egui::RichText::new(tool.name()).color(t.accent)).frame(false));
            ui.label(egui::RichText::new(tool.description()).size(11.0).color(t.muted));
            ui.add_space(4.0);
            if r.clicked() {
                let active = app.active;
                app.gp.open(tool, active);
            }
        }
        return;
    };
    ui.horizontal(|ui| {
        if ui.button("‹ Tools").on_hover_text("Back to the tools").clicked() {
            app.gp.tool = None;
        }
        ui.label(egui::RichText::new(tool.name()).strong().color(t.text));
    });
    ui.label(egui::RichText::new(tool.description()).size(11.0).color(t.muted));
    ui.separator();
    let mut g = std::mem::take(&mut app.gp);
    let points = |k: GeometryType| matches!(k, GeometryType::Point | GeometryType::Multipoint);
    let (command, params): (&str, Json) = match tool {
        Tool::Buffer => {
            layer_combo(ui, app, "Input Features (points)", "gp-in", &mut g.layer, points);
            ui.label("Distance");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut g.distance).desired_width(90.0));
                egui::ComboBox::from_id_salt("gp-unit").selected_text(g.unit).show_ui(ui, |ui| {
                    ui.selectable_value(&mut g.unit, "meters", "meters");
                    ui.selectable_value(&mut g.unit, "kilometers", "kilometers");
                });
            });
            ui.label("Output Feature Class");
            ui.add(egui::TextEdit::singleline(&mut g.output).hint_text("<input>_Buffer").desired_width(f32::INFINITY));
            (
                "analysis.buffer",
                json!({ "layer": g.layer, "distance": g.distance.replace(',', "."), "unit": g.unit, "output": (!g.output.is_empty()).then_some(&g.output) }),
            )
        }
        Tool::Project => {
            layer_combo(ui, app, "Input Features", "gp-in", &mut g.layer, |_| true);
            ui.label("Output Coordinate System (EPSG)");
            ui.add(egui::TextEdit::singleline(&mut g.epsg).desired_width(f32::INFINITY));
            let name =
                g.epsg.trim().parse::<u32>().ok().and_then(|c| mkaegis_crs::Crs::from_epsg(c).ok()).map_or("unknown code".to_string(), |c| c.name());
            ui.label(egui::RichText::new(name).size(11.0).color(t.muted));
            ui.label("Output Feature Class");
            ui.add(egui::TextEdit::singleline(&mut g.output).hint_text("<input>_Project").desired_width(f32::INFINITY));
            ("management.project", json!({ "layer": g.layer, "epsg": g.epsg.trim(), "output": (!g.output.is_empty()).then_some(&g.output) }))
        }
        Tool::CalculateGeometry => {
            layer_combo(ui, app, "Input Features", "gp-in", &mut g.layer, |k| !matches!(k, GeometryType::Point));
            ui.label("Target Field (new or numeric)");
            ui.add(egui::TextEdit::singleline(&mut g.field).hint_text("AREA_HA").desired_width(f32::INFINITY));
            ui.label("Property");
            egui::ComboBox::from_id_salt("gp-prop").width(ui.available_width()).selected_text(g.property).show_ui(ui, |ui| {
                for p in ["area", "areaHectares", "areaKm2", "length", "lengthKm", "centroidX", "centroidY"] {
                    ui.selectable_value(&mut g.property, p, p);
                }
            });
            ("management.calculateGeometry", json!({ "layer": g.layer, "field": g.field, "property": g.property }))
        }
        Tool::SelectByAttributes => {
            layer_combo(ui, app, "Input Rows", "gp-in", &mut g.layer, |_| true);
            ui.label("Where clause");
            ui.add(egui::TextEdit::multiline(&mut g.clause).hint_text("POP > 10000").desired_rows(3).desired_width(f32::INFINITY));
            ("map.selectByAttributes", json!({ "layer": g.layer, "where": g.clause }))
        }
        Tool::SelectByLocation => {
            layer_combo(ui, app, "Input Features", "gp-in", &mut g.layer, |_| true);
            ui.label("Relationship");
            egui::ComboBox::from_id_salt("gp-rel").width(ui.available_width()).selected_text(g.relation).show_ui(ui, |ui| {
                for r in ["intersect", "withinDistance", "within", "contains", "haveTheirCenterIn"] {
                    ui.selectable_value(&mut g.relation, r, r);
                }
            });
            layer_combo(ui, app, "Selecting Features", "gp-sel", &mut g.other, |_| true);
            if g.relation == "withinDistance" {
                ui.label("Search Distance (meters)");
                ui.add(egui::TextEdit::singleline(&mut g.distance).desired_width(f32::INFINITY));
            }
            (
                "map.selectByLocation",
                json!({ "layer": g.layer, "selecting": g.other, "relation": g.relation, "distance": g.distance.replace(',', ".") }),
            )
        }
        Tool::SummaryStatistics => {
            layer_combo(ui, app, "Input Table", "gp-in", &mut g.layer, |_| true);
            field_combo(ui, app, g.layer, "Field (numeric)", "gp-field", &mut g.field, true);
            ui.checkbox(&mut g.selected_only, "Selected records only");
            ("analysis.summaryStatistics", json!({ "layer": g.layer, "field": g.field, "selectedOnly": g.selected_only }))
        }
        Tool::SpatialWeights => {
            layer_combo(ui, app, "Input Features", "gp-in", &mut g.layer, |_| true);
            let polygons = g
                .layer
                .and_then(|id| app.session.map.layers.iter().find(|l| l.id == id))
                .is_some_and(|l| l.data.geometry_type == GeometryType::Polygon);
            if !polygons && matches!(g.method, "queen" | "rook") {
                g.method = "knn";
            }
            ui.label("Conceptualization");
            egui::ComboBox::from_id_salt("gp-method").width(ui.available_width()).selected_text(method_label(g.method)).show_ui(ui, |ui| {
                for m in ["queen", "rook", "knn", "distance", "kernel"] {
                    if polygons || !matches!(m, "queen" | "rook") {
                        ui.selectable_value(&mut g.method, m, method_label(m));
                    }
                }
            });
            match g.method {
                "queen" | "rook" => {
                    ui.label("Order of contiguity");
                    ui.add(egui::TextEdit::singleline(&mut g.order).desired_width(f32::INFINITY));
                }
                "knn" | "kernel" => {
                    ui.label(if g.method == "knn" { "Number of neighbors (k)" } else { "Neighbors for the adaptive bandwidth (k)" });
                    ui.add(egui::TextEdit::singleline(&mut g.k).desired_width(f32::INFINITY));
                }
                _ => {
                    ui.label("Distance band (meters)");
                    ui.add(egui::TextEdit::singleline(&mut g.threshold).hint_text("the smallest with no feature alone").desired_width(f32::INFINITY));
                }
            }
            ui.label("Standardization");
            egui::ComboBox::from_id_salt("gp-std").width(ui.available_width()).selected_text(g.standardize).show_ui(ui, |ui| {
                for s in ["row", "binary", "variance", "none"] {
                    ui.selectable_value(&mut g.standardize, s, s);
                }
            });
            ui.label("Weights name");
            ui.add(egui::TextEdit::singleline(&mut g.output).hint_text("<input>_<method>").desired_width(f32::INFINITY));
            let number = |t: &str| t.trim().replace(',', ".");
            (
                "stats.weights",
                json!({
                    "layer": g.layer,
                    "method": g.method,
                    "k": (matches!(g.method, "knn" | "kernel")).then(|| number(&g.k)),
                    "order": (matches!(g.method, "queen" | "rook")).then(|| number(&g.order)),
                    "threshold": (g.method == "distance" && !g.threshold.trim().is_empty()).then(|| number(&g.threshold)),
                    "standardize": g.standardize,
                    "name": (!g.output.is_empty()).then_some(&g.output),
                }),
            )
        }
        Tool::SpatialAutocorrelation => {
            let names: Vec<(String, u32)> = app.session.map.weights.iter().map(|w| (w.name.clone(), w.layer)).collect();
            if !names.iter().any(|(n, _)| *n == g.weights) {
                g.weights = names.last().map(|(n, _)| n.clone()).unwrap_or_default();
            }
            ui.label("Spatial Weights");
            egui::ComboBox::from_id_salt("gp-weights").width(ui.available_width()).selected_text(g.weights.as_str()).show_ui(ui, |ui| {
                for (n, _) in &names {
                    ui.selectable_value(&mut g.weights, n.clone(), n);
                }
            });
            if names.is_empty() {
                ui.label(egui::RichText::new("Build spatial weights first (Statistics › Spatial Weights).").size(11.0).color(t.muted));
            }
            let layer = names.iter().find(|(n, _)| *n == g.weights).map(|(_, l)| *l);
            field_combo(ui, app, layer, "Input Field (numeric)", "gp-field", &mut g.field, true);
            ui.label("Statistic");
            egui::ComboBox::from_id_salt("gp-stat").width(ui.available_width()).selected_text(statistic_label(g.statistic)).show_ui(ui, |ui| {
                for c in ["stats.globalMoran", "stats.gearyC", "stats.generalG", "stats.localMoran", "stats.hotSpots"] {
                    ui.selectable_value(&mut g.statistic, c, statistic_label(c));
                }
            });
            let local = matches!(g.statistic, "stats.localMoran" | "stats.hotSpots");
            if g.statistic != "stats.hotSpots" {
                ui.label("Permutations (0 for none)");
                ui.add(egui::TextEdit::singleline(&mut g.permutations).desired_width(f32::INFINITY));
                ui.label("Seed");
                ui.add(egui::TextEdit::singleline(&mut g.seed).desired_width(f32::INFINITY));
            }
            if g.statistic == "stats.localMoran" {
                ui.label("Significance level (α)");
                ui.add(egui::TextEdit::singleline(&mut g.alpha).desired_width(f32::INFINITY));
            }
            if local {
                ui.label("Multiple-testing correction");
                egui::ComboBox::from_id_salt("gp-correction").width(ui.available_width()).selected_text(correction_label(g.correction)).show_ui(
                    ui,
                    |ui| {
                        for c in ["none", "fdr", "bonferroni"] {
                            ui.selectable_value(&mut g.correction, c, correction_label(c));
                        }
                    },
                );
                ui.label(egui::RichText::new("The results are written as fields on the layer and mapped.").size(11.0).color(t.muted));
            }
            let number = |t: &str| t.trim().parse::<u64>().ok();
            let params = json!({
                "weights": g.weights,
                "field": g.field,
                "permutations": (g.statistic != "stats.hotSpots").then(|| number(&g.permutations)).flatten(),
                "seed": (g.statistic != "stats.hotSpots").then(|| number(&g.seed)).flatten(),
                "alpha": (g.statistic == "stats.localMoran").then(|| g.alpha.trim().replace(',', ".").parse::<f64>().ok()).flatten(),
                "correction": local.then_some(g.correction),
            });
            (g.statistic, params)
        }
    };
    ui.add_space(8.0);
    if ui.add(egui::Button::new(egui::RichText::new("Run").strong()).min_size(egui::vec2(80.0, 26.0))).clicked() {
        g.result = app.run(command, params);
    }
    if let Some(r) = &g.result
        && tool == Tool::SpatialWeights
    {
        ui.separator();
        ui.label(egui::RichText::new(r.get("name").and_then(Json::as_str).unwrap_or("")).strong());
        egui::Grid::new("weights").striped(true).num_columns(2).show(ui, |ui| {
            let summary = |k: &str| r.pointer(&format!("/summary/{k}")).cloned().unwrap_or(Json::Null);
            let number = |v: Json| v.as_f64().map_or(v.to_string(), |x| mkaegis_core::Value::Float(x).to_string());
            for (label, key) in [
                ("Features", "n"),
                ("Links", "links"),
                ("% non-zero", "percent_nonzero"),
                ("Mean neighbors", "mean_neighbours"),
                ("Min neighbors", "min_neighbours"),
                ("Max neighbors", "max_neighbours"),
            ] {
                ui.label(egui::RichText::new(label).strong());
                ui.label(number(summary(key)));
                ui.end_row();
            }
            ui.label(egui::RichText::new("Without neighbors").strong());
            ui.label(summary("islands").as_array().map_or(0, Vec::len).to_string());
            ui.end_row();
            ui.label(egui::RichText::new("Symmetric").strong());
            ui.label(if summary("symmetric").as_bool().unwrap_or(false) { "yes" } else { "no" });
            ui.end_row();
        });
        ui.label(egui::RichText::new("Select a feature, then Statistics › Neighbors, to see its neighbours on the map.").size(11.0).color(t.muted));
    }
    if let Some(r) = &g.result
        && tool == Tool::SummaryStatistics
    {
        ui.separator();
        egui::Grid::new("stats").striped(true).num_columns(2).show(ui, |ui| {
            for k in ["count", "nulls", "sum", "mean", "min", "max", "std"] {
                ui.label(egui::RichText::new(k).strong());
                ui.label(r.get(k).map(|v| v.as_f64().map_or(v.to_string(), |x| mkaegis_core::Value::Float(x).to_string())).unwrap_or_default());
                ui.end_row();
            }
        });
    }
    if let Some(r) = &g.result
        && tool == Tool::SpatialAutocorrelation
    {
        if r.get("statistic").is_some() {
            autocorrelation_report(ui, r);
        } else if r.get("summary").is_some() {
            local_report(ui, r);
        }
    }
    app.gp = g;
}

fn statistic_label(command: &str) -> &'static str {
    match command {
        "stats.gearyC" => "Geary's C",
        "stats.generalG" => "Getis-Ord General G",
        "stats.localMoran" => "Local Moran's I (clusters and outliers)",
        "stats.hotSpots" => "Getis-Ord Gi* (hot spots)",
        _ => "Global Moran's I",
    }
}

fn correction_label(correction: &str) -> &'static str {
    match correction {
        "fdr" => "False discovery rate (Benjamini–Hochberg)",
        "bonferroni" => "Bonferroni",
        _ => "None",
    }
}

/// A local statistic's summary: what was tested, how, and how many features came out where.
fn local_report(ui: &mut Ui, r: &Json) {
    let t = Tokens::get(ui.ctx());
    ui.separator();
    ui.label(egui::RichText::new(r.get("message").and_then(Json::as_str).unwrap_or("")).strong());
    egui::Grid::new("local").striped(true).num_columns(2).show(ui, |ui| {
        let mut row = |label: &str, value: String| {
            ui.label(egui::RichText::new(label).strong());
            ui.label(value);
            ui.end_row();
        };
        let summary = |k: &str| r.pointer(&format!("/summary/{k}")).cloned().unwrap_or(Json::Null);
        row("Features tested", summary("tested").to_string());
        row("Correction", correction_label(summary("correction").as_str().unwrap_or("none")).to_string());
        if let Some(t) = summary("threshold").as_f64() {
            row("p threshold", if t != 0.0 && t < 1e-4 { format!("{t:.3e}") } else { format!("{t:.4}") });
        }
        if let Some(clusters) = r.get("clusters").and_then(Json::as_object) {
            for (name, count) in clusters {
                row(name, count.to_string());
            }
        }
        if let (Some(hot), Some(cold)) = (r.get("hot"), r.get("cold")) {
            row("Hot spots (95 %)", hot.to_string());
            row("Cold spots (95 %)", cold.to_string());
        }
    });
    let fields: Vec<&str> = r.get("fields").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_str).collect();
    ui.label(egui::RichText::new(format!("Fields written: {}.", fields.join(", "))).size(11.0).color(t.muted));
}

/// A global statistic's report: the value against its expectation, then each inference.
fn autocorrelation_report(ui: &mut Ui, r: &Json) {
    let t = Tokens::get(ui.ctx());
    let num = |path: &str| r.pointer(path).and_then(Json::as_f64);
    let show =
        |x: Option<f64>| x.map_or_else(|| "—".to_string(), |x| if x != 0.0 && x.abs() < 1e-4 { format!("{x:.3e}") } else { format!("{x:.4}") });
    ui.separator();
    ui.label(
        egui::RichText::new(format!(
            "{} — {} by {}",
            r.get("statistic").and_then(Json::as_str).unwrap_or(""),
            r.get("field").and_then(Json::as_str).unwrap_or(""),
            r.get("weights").and_then(Json::as_str).unwrap_or("")
        ))
        .strong(),
    );
    egui::Grid::new("autocorrelation").striped(true).num_columns(2).show(ui, |ui| {
        let mut row = |label: &str, value: String| {
            ui.label(egui::RichText::new(label).strong());
            ui.label(value);
            ui.end_row();
        };
        row("Value", show(num("/value")));
        row("Expected", show(num("/expected")));
        row("Features", r.get("n").map(Json::to_string).unwrap_or_default());
        for (label, key) in [("normality", "/normality"), ("randomization", "/randomization")] {
            if r.pointer(key).is_some_and(|v| !v.is_null()) {
                row(&format!("z ({label})"), show(num(&format!("{key}/z"))));
                row(&format!("p ({label}, two-sided)"), show(num(&format!("{key}/p"))));
            }
        }
        if r.get("permutation").is_some_and(|v| !v.is_null()) {
            row("Pseudo p (one-sided)", show(num("/permutation/p")));
            row("z (permutation)", show(num("/permutation/z")));
            row(
                "Permutations",
                format!(
                    "{} (seed {})",
                    r.pointer("/permutation/permutations").unwrap_or(&Json::Null),
                    r.pointer("/permutation/seed").unwrap_or(&Json::Null)
                ),
            );
        }
    });
    if let Some(n) = r.get("islands").and_then(Json::as_u64).filter(|n| *n > 0) {
        ui.label(egui::RichText::new(format!("{n} features have no neighbours.")).size(11.0).color(t.muted));
    }
}

fn symbology(app: &mut MapApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(layer) = app.active_layer() else {
        ui.label(egui::RichText::new("Select a layer in Contents.").color(t.muted));
        return;
    };
    let id = layer.id;
    let kind = layer.data.geometry_type;
    let base = layer.renderer.base();
    let renderer = match &layer.renderer {
        Renderer::Single(_) => "single",
        Renderer::UniqueValues { .. } => "uniqueValues",
        Renderer::GraduatedColors { .. } => "graduatedColors",
    };
    let field = layer.renderer.field().map(str::to_string);
    let numeric: Vec<String> = layer.data.fields.iter().filter(|f| f.kind.is_numeric()).map(|f| f.name.clone()).collect();
    let all: Vec<String> = layer.data.fields.iter().map(|f| f.name.clone()).collect();
    let labels = layer.labels.clone();
    let opacity = layer.opacity;
    ui.label(egui::RichText::new(format!("Symbology — {}", layer.name)).strong().color(t.text));
    ui.separator();
    let mut next: Option<Json> = None;
    ui.label("Primary symbology");
    let mut r = renderer;
    egui::ComboBox::from_id_salt("sym-renderer")
        .width(ui.available_width())
        .selected_text(match r {
            "uniqueValues" => "Unique Values",
            "graduatedColors" => "Graduated Colors",
            _ => "Single Symbol",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut r, "single", "Single Symbol");
            ui.selectable_value(&mut r, "uniqueValues", "Unique Values");
            ui.add_enabled_ui(!numeric.is_empty(), |ui| ui.selectable_value(&mut r, "graduatedColors", "Graduated Colors"));
        });
    if r != renderer {
        let f = match r {
            "graduatedColors" => numeric.first().cloned(),
            "uniqueValues" => all.first().cloned(),
            _ => None,
        };
        next = Some(json!({ "renderer": r, "field": f }));
    }
    if renderer != "single" {
        let mut f = field.clone().unwrap_or_default();
        ui.label("Field");
        egui::ComboBox::from_id_salt("sym-field").width(ui.available_width()).selected_text(f.as_str()).show_ui(ui, |ui| {
            for name in if renderer == "graduatedColors" { &numeric } else { &all } {
                ui.selectable_value(&mut f, name.clone(), name);
            }
        });
        if Some(&f) != field.as_ref() {
            next = Some(json!({ "field": f }));
        }
    }
    if renderer == "graduatedColors" {
        ui.horizontal(|ui| {
            for (label, method) in [("Equal Interval", "equalInterval"), ("Quantile", "quantile"), ("Natural Breaks", "naturalBreaks")] {
                if ui.button(label).clicked() {
                    next = Some(json!({ "method": method }));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            // Six named classes each, on a diverging ramp centred on the mean or the median.
            for (label, method, tip) in [
                ("Std. Deviation", "standardDeviation", "Six classes at the mean ± 1 and 2 standard deviations"),
                ("Box Map", "boxMap", "Quartiles, with outliers beyond 1.5 × the interquartile range"),
                ("Percentile", "percentile", "< 1 %, 1–10 %, 10–50 %, 50–90 %, 90–99 %, > 99 %"),
                ("Geometric", "geometricInterval", "Bounds in geometric progression, for positive skewed values"),
            ] {
                if ui.button(label).on_hover_text(tip).clicked() {
                    let ramp = if matches!(method, "geometricInterval") { "meadow" } else { "balance" };
                    next = Some(json!({ "method": method, "ramp": ramp }));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Classes");
            for k in [3, 4, 5, 7] {
                if ui.button(k.to_string()).clicked() {
                    next = Some(json!({ "classes": k }));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Ramp");
            for ramp in ["meadow", "ember", "depth", "balance"] {
                if ui.button(ramp).clicked() {
                    next = Some(json!({ "ramp": ramp }));
                }
            }
        });
    }
    ui.separator();
    ui.label(egui::RichText::new("Symbol").strong());
    let to32 = |c: mkaegis_core::Color| egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
    let hex = |c: egui::Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        mkaegis_core::Color::rgba(r, g, b, a).to_hex()
    };
    egui::Grid::new("sym-grid").num_columns(2).show(ui, |ui| {
        if kind != GeometryType::Polyline {
            ui.label("Fill");
            let mut c = to32(base.fill);
            if ui.color_edit_button_srgba(&mut c).changed() && renderer == "single" {
                next = Some(json!({ "fill": hex(c) }));
            }
            ui.end_row();
        }
        ui.label(if kind == GeometryType::Polyline { "Color" } else { "Outline" });
        let mut c = to32(base.outline);
        if ui.color_edit_button_srgba(&mut c).changed() {
            next = Some(json!({ "outline": hex(c) }));
        }
        ui.end_row();
        ui.label("Width");
        let mut w = base.width;
        if ui.add(egui::Slider::new(&mut w, 0.0..=8.0).step_by(0.1)).drag_stopped() {
            next = Some(json!({ "width": w }));
        }
        ui.end_row();
        if matches!(kind, GeometryType::Point | GeometryType::Multipoint) {
            ui.label("Size");
            let mut s = base.size;
            if ui.add(egui::Slider::new(&mut s, 1.0..=30.0).step_by(0.5)).drag_stopped() {
                next = Some(json!({ "size": s }));
            }
            ui.end_row();
            ui.label("Shape");
            ui.horizontal(|ui| {
                for m in ["circle", "square", "triangle", "diamond"] {
                    if ui.small_button(m).clicked() {
                        next = Some(json!({ "marker": m }));
                    }
                }
            });
            ui.end_row();
        }
    });
    if let Some(mut p) = next {
        if let Some(o) = p.as_object_mut() {
            o.insert("layer".into(), json!(id));
        }
        app.run("layer.symbology", p);
    }
    ui.separator();
    ui.label(egui::RichText::new("Transparency").strong());
    let mut o = 100 - i32::from(opacity);
    if ui.add(egui::Slider::new(&mut o, 0..=100).suffix(" %")).drag_stopped() {
        app.run("layer.setOpacity", json!({ "layer": id, "opacity": 100 - o }));
    }
    ui.separator();
    ui.label(egui::RichText::new("Labels").strong());
    let mut on = labels.is_some();
    let mut lf = labels.as_ref().map(|l| l.field.clone()).unwrap_or_else(|| all.first().cloned().unwrap_or_default());
    let mut size = labels.as_ref().map_or(9.0, |l| l.size);
    let mut changed = ui.checkbox(&mut on, "Label features in this layer").changed();
    if on {
        egui::ComboBox::from_id_salt("lbl-field").width(ui.available_width()).selected_text(lf.as_str()).show_ui(ui, |ui| {
            for name in &all {
                changed |= ui.selectable_value(&mut lf, name.clone(), name).changed();
            }
        });
        changed |= ui.add(egui::Slider::new(&mut size, 6.0..=24.0).text("pt")).drag_stopped();
    }
    if changed {
        app.run("layer.labels", json!({ "layer": id, "field": on.then_some(lf), "size": size }));
    }
}

fn history(app: &mut MapApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new("History").strong().color(t.text));
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        if app.session.history().is_empty() {
            ui.label(egui::RichText::new("Nothing run yet.").color(t.muted));
        }
        for h in app.session.history() {
            let label = mkaegis_engine::Session::command(&h.command).map_or(h.command.as_str(), |c| c.label);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(if h.ok { "✔" } else { "✖" }).color(if h.ok { t.accent } else { t.error }));
                ui.label(egui::RichText::new(label).strong());
                ui.label(egui::RichText::new(&h.message).size(11.0).color(t.muted));
            });
        }
    });
}

pub fn show(app: &mut MapApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        // Four tabs and the chevron share the pane's width.
        ui.spacing_mut().item_spacing.x = 2.0;
        for (p, label) in
            [(Pane::Geoprocessing, "Geoprocessing"), (Pane::Symbology, "Symbology"), (Pane::History, "History"), (Pane::Charts, "Charts")]
        {
            let sel = app.pane == Some(p);
            if ui.selectable_label(sel, egui::RichText::new(label).size(12.5).color(if sel { t.accent } else { t.text })).clicked() {
                app.pane = Some(p);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // A chevron folds the pane to its strip, whose chevron points back to open it.
            let (rect, r) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
            if r.hovered() {
                ui.painter().rect_filled(rect, 4.0, t.window);
            }
            let icon = egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0));
            crate::icons::paint(ui.painter(), icon, crate::icons::Icon::ChevronRight, t.accent, t.accent);
            if r.on_hover_text("Close the pane").clicked() {
                app.pane = None;
            }
        });
    });
    ui.separator();
    egui::ScrollArea::vertical().id_salt("pane").auto_shrink([false, false]).show(ui, |ui| match app.pane {
        Some(Pane::Geoprocessing) => gp(app, ui),
        Some(Pane::Symbology) => symbology(app, ui),
        Some(Pane::History) => history(app, ui),
        Some(Pane::Charts) => crate::charts::show(app, ui),
        None => {}
    });
}

/// A conceptualization's name in the Spatial Weights form.
fn method_label(method: &str) -> &'static str {
    match method {
        "queen" => "Contiguity: queen (shared vertex)",
        "rook" => "Contiguity: rook (shared edge)",
        "knn" => "K nearest neighbors",
        "distance" => "Distance band",
        "kernel" => "Kernel (adaptive bisquare)",
        _ => "",
    }
}
