//! Dialogs: they only gather parameters; OK runs the command.

use serde_json::{Value as Json, json};

use crate::MapApp;
use crate::i18n::tr;
use crate::theme::Tokens;

#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    /// A path for a command that reads or writes a file (when there is no native file dialog).
    Path {
        title: &'static str,
        command: &'static str,
        path: String,
    },
    /// Same, with more parameters to send along.
    SavePath {
        title: &'static str,
        command: &'static str,
        path: String,
        params: Json,
    },
    SelectByAttributes {
        layer: u32,
        clause: String,
        mode: &'static str,
    },
    Rename {
        layer: u32,
        name: String,
    },
    AddField {
        layer: u32,
        name: String,
        kind: &'static str,
    },
    CalculateField {
        layer: u32,
        field: String,
        expression: String,
    },
    EditValue {
        layer: u32,
        row: usize,
        field: String,
        value: String,
    },
    CoordinateSystem {
        epsg: String,
    },
    LayerInfo {
        layer: u32,
    },
    /// Save, Don't Save or Cancel before `then` throws away a modified map.
    UnsavedChanges {
        then: Discard,
    },
    About,
}

/// What would throw away the current map, waiting on the Unsaved Changes dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discard {
    /// Closing the window.
    Close,
    /// Project › New.
    New,
    /// A new map with the sample data (File › New › Sample map).
    Sample,
    /// Project › Open.
    Open,
    /// A recent project, from the File backstage.
    OpenFile(std::path::PathBuf),
}

/// OK and Cancel; returns (ok, cancel).
fn buttons(ui: &mut egui::Ui, ok_enabled: bool) -> (bool, bool) {
    ui.add_space(6.0);
    let mut out = (false, false);
    ui.horizontal(|ui| {
        out.0 = ui.add_enabled(ok_enabled, egui::Button::new(tr("OK")).min_size(egui::vec2(70.0, 0.0))).clicked();
        out.1 = ui.button(tr("Cancel")).clicked();
    });
    out
}

pub fn show(app: &mut MapApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.dialog.take() else { return };
    let t = Tokens::get(ctx);
    let title = match &dialog {
        Dialog::Path { title, .. } | Dialog::SavePath { title, .. } => title.to_string(),
        Dialog::SelectByAttributes { .. } => tr("Select By Attributes").into(),
        Dialog::Rename { .. } => tr("Rename Layer").into(),
        Dialog::AddField { .. } => tr("Add Field").into(),
        Dialog::CalculateField { .. } => tr("Calculate Field").into(),
        Dialog::EditValue { .. } => tr("Edit Attribute").into(),
        Dialog::CoordinateSystem { .. } => tr("Map Coordinate System").into(),
        Dialog::LayerInfo { .. } => tr("Layer Properties").into(),
        Dialog::UnsavedChanges { .. } => tr("Unsaved Changes").into(),
        Dialog::About => tr("About MK aeGIS").into(),
    };
    let info: Option<Json> = match &dialog {
        Dialog::LayerInfo { layer } => app.session.execute("layer.info", &json!({ "layer": layer })).ok(),
        _ => None,
    };
    let layer_name = |id: u32| app.session.map.layers.iter().find(|l| l.id == id).map(|l| l.name.clone()).unwrap_or_default();
    let fields = |id: u32| -> Vec<String> {
        app.session.map.layers.iter().find(|l| l.id == id).map(|l| l.data.fields.iter().map(|f| f.name.clone()).collect()).unwrap_or_default()
    };
    // In the browser a file is only a name: opened ones come from its picker, saved ones download.
    let path_label = if app.session.disk.is_memory() { tr("File name") } else { tr("Path") };
    let mut action: Option<(&'static str, Json)> = None;
    let mut decided: Option<(Discard, bool)> = None;
    let mut keep = true;
    egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
        ui.set_min_width(360.0);
        match &mut dialog {
            Dialog::Path { command, path, .. } => {
                ui.label(path_label);
                let r = ui.add(egui::TextEdit::singleline(path).desired_width(f32::INFINITY));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let (ok, cancel) = buttons(ui, !path.trim().is_empty());
                if ok || enter {
                    action = Some((command, json!({ "path": path.trim() })));
                }
                keep = !(ok || cancel || enter);
            }
            Dialog::SavePath { command, path, params, .. } => {
                ui.label(path_label);
                ui.add(egui::TextEdit::singleline(path).desired_width(f32::INFINITY));
                let (ok, cancel) = buttons(ui, !path.trim().is_empty());
                if ok {
                    let mut p = params.clone();
                    if let Some(o) = p.as_object_mut() {
                        o.insert("path".into(), json!(path.trim()));
                    }
                    action = Some((command, p));
                }
                keep = !(ok || cancel);
            }
            Dialog::SelectByAttributes { layer, clause, mode } => {
                ui.label(egui::RichText::new(crate::trf("Input rows: {layer}", &[("layer", &layer_name(*layer))])).strong());
                ui.horizontal(|ui| {
                    ui.label(tr("Selection type"));
                    for (m, label) in
                        [("new", tr("New selection")), ("add", tr("Add to")), ("remove", tr("Remove from")), ("intersect", tr("Select subset"))]
                    {
                        ui.selectable_value(mode, m, label);
                    }
                });
                ui.label(tr("Where clause (SQL)"));
                ui.add(
                    egui::TextEdit::multiline(clause)
                        .hint_text(tr("POP > 10000 AND NOME LIKE 'S%'"))
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .code_editor(),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tr("Fields:")).color(t.muted));
                    for f in fields(*layer) {
                        if ui.small_button(&f).clicked() {
                            clause.push_str(&f);
                            clause.push(' ');
                        }
                    }
                });
                let (ok, cancel) = buttons(ui, !clause.trim().is_empty());
                if ok {
                    action = Some(("map.selectByAttributes", json!({ "layer": layer, "where": clause, "mode": mode })));
                }
                keep = !(ok || cancel);
            }
            Dialog::Rename { layer, name } => {
                ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                let (ok, cancel) = buttons(ui, !name.trim().is_empty());
                if ok {
                    action = Some(("layer.rename", json!({ "layer": layer, "name": name })));
                }
                keep = !(ok || cancel);
            }
            Dialog::AddField { layer, name, kind } => {
                ui.label(crate::trf("Layer: {layer}", &[("layer", &layer_name(*layer))]));
                ui.label(tr("Field name"));
                ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    ui.label(tr("Type"));
                    for k in ["long", "short", "double", "text"] {
                        ui.selectable_value(kind, k, k);
                    }
                });
                let (ok, cancel) = buttons(ui, !name.trim().is_empty());
                if ok {
                    action = Some(("layer.addField", json!({ "layer": layer, "name": name, "type": kind })));
                }
                keep = !(ok || cancel);
            }
            Dialog::CalculateField { layer, field, expression } => {
                ui.label(crate::trf("Layer: {layer}", &[("layer", &layer_name(*layer))]));
                ui.label(tr("Field"));
                egui::ComboBox::from_id_salt("calc-field").selected_text(field.as_str()).show_ui(ui, |ui| {
                    for f in fields(*layer) {
                        ui.selectable_value(field, f.clone(), f);
                    }
                });
                ui.label(format!("{field} ="));
                ui.add(egui::TextEdit::multiline(expression).hint_text(tr("POP / 1000")).desired_rows(2).desired_width(f32::INFINITY).code_editor());
                let (ok, cancel) = buttons(ui, !expression.trim().is_empty() && !field.is_empty());
                if ok {
                    action = Some(("layer.calculateField", json!({ "layer": layer, "field": field, "expression": expression })));
                }
                keep = !(ok || cancel);
            }
            Dialog::EditValue { layer, row, field, value } => {
                ui.label(crate::trf("{layer} — row {row} — {field}", &[("layer", &layer_name(*layer)), ("row", &(*row + 1)), ("field", field)]));
                ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
                ui.label(egui::RichText::new(tr("Leave empty for Null.")).size(11.0).color(t.muted));
                let (ok, cancel) = buttons(ui, true);
                if ok {
                    let v = if value.is_empty() { Json::Null } else { json!(value) };
                    action = Some(("feature.setValue", json!({ "layer": layer, "row": row, "field": field, "value": v })));
                }
                keep = !(ok || cancel);
            }
            Dialog::CoordinateSystem { epsg } => {
                ui.label(tr("EPSG code"));
                ui.add(egui::TextEdit::singleline(epsg).desired_width(f32::INFINITY));
                let known = epsg.trim().parse::<u32>().ok().and_then(|c| mkaegis_crs::Crs::from_epsg(c).ok());
                ui.label(egui::RichText::new(known.map_or(tr("Unknown or unsupported code").to_string(), |c| c.name())).color(if known.is_some() {
                    t.text
                } else {
                    t.error
                }));
                ui.horizontal_wrapped(|ui| {
                    for (code, label) in [(3857, "Web Mercator"), (4326, "WGS 84"), (4674, "SIRGAS 2000"), (31983, "SIRGAS 2000 / UTM 23S")] {
                        if ui.small_button(label).clicked() {
                            *epsg = code.to_string();
                        }
                    }
                });
                let (ok, cancel) = buttons(ui, known.is_some());
                if ok {
                    action = Some(("map.setCrs", json!({ "epsg": epsg.trim() })));
                }
                keep = !(ok || cancel);
            }
            Dialog::LayerInfo { .. } => {
                if let Some(info) = &info {
                    egui::Grid::new("info").striped(true).num_columns(2).show(ui, |ui| {
                        for (k, label) in
                            [("name", tr("Name")), ("geometry", tr("Geometry")), ("features", tr("Features")), ("selected", tr("Selected"))]
                        {
                            ui.label(egui::RichText::new(label).strong());
                            ui.label(info.get(k).map(|v| v.as_str().map_or(v.to_string(), str::to_string)).unwrap_or_default());
                            ui.end_row();
                        }
                        ui.label(egui::RichText::new(tr("Coordinate system")).strong());
                        ui.label(format!(
                            "{} (EPSG:{})",
                            info.pointer("/crs/name").and_then(Json::as_str).unwrap_or(""),
                            info.pointer("/crs/epsg").map(Json::to_string).unwrap_or_default()
                        ));
                        ui.end_row();
                    });
                    ui.separator();
                    ui.label(egui::RichText::new(tr("Fields")).strong());
                    for f in info.get("fields").and_then(Json::as_array).into_iter().flatten() {
                        ui.label(format!(
                            "{} — {}",
                            f.get("name").and_then(Json::as_str).unwrap_or(""),
                            f.get("type").and_then(Json::as_str).unwrap_or("")
                        ));
                    }
                }
                keep = !ui.button(tr("Close")).clicked();
            }
            Dialog::UnsavedChanges { then } => {
                ui.label(crate::trf("Save changes to {name}?", &[("name", &app.document_name())]));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(tr("Save")).min_size(egui::vec2(70.0, 0.0))).clicked() {
                        decided = Some((then.clone(), true));
                    }
                    if ui.button(tr("Don't Save")).clicked() {
                        decided = Some((then.clone(), false));
                    }
                    keep = !ui.button(tr("Cancel")).clicked() && decided.is_none();
                });
            }
            Dialog::About => {
                ui.label(egui::RichText::new(tr("MK aeGIS")).strong().size(18.0));
                ui.label(crate::trf("Version {version}", &[("version", &env!("CARGO_PKG_VERSION"))]));
                ui.label(tr("An open-source, clean-room geographic information system in pure Rust."));
                ui.label(egui::RichText::new(tr("Licensed MIT OR Apache-2.0.")).color(t.muted));
                let (people, models) = crate::credits::credits();
                for (title, list) in [(tr("Contributors"), people), (tr("Models"), models)] {
                    ui.separator();
                    ui.label(egui::RichText::new(title).strong());
                    if list.is_empty() {
                        ui.label(egui::RichText::new("—").color(t.muted));
                    }
                    for (name, commits) in list.iter().take(50) {
                        ui.label(crate::trf("{name} — {count} commits", &[("name", &name), ("count", &commits)]));
                    }
                }
                keep = !ui.button(tr("Close")).clicked();
            }
        }
    });
    if keep {
        app.dialog = Some(dialog);
    }
    if let Some((command, params)) = action {
        app.run(command, params);
    }
    if let Some((then, save)) = decided {
        app.resolve_unsaved(then, save);
    }
}
