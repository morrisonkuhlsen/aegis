//! The attribute table of the active layer: virtualised rows, the selection highlighted, a
//! click selects (Ctrl adds), a double click edits a cell.

use egui::Ui;
use serde_json::json;

use crate::theme::Tokens;
use crate::{Dialog, MapApp};

const ROW: f32 = 20.0;
const OID_WIDTH: f32 = 56.0;
const COL_WIDTH: f32 = 120.0;

pub fn show(app: &mut MapApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(layer) = app.active_layer() else {
        ui.label(egui::RichText::new("No layer selected in Contents.").color(t.muted));
        return;
    };
    let id = layer.id;
    let n = layer.data.features.len();
    let selected = layer.selection.len();
    let fields: Vec<String> = layer.data.fields.iter().map(|f| f.name.clone()).collect();
    let name = layer.name.clone();
    let mut close = false;
    ui.horizontal_wrapped(|ui| {
        crate::icons::square(ui, 16.0, crate::icons::Icon::Table);
        ui.label(egui::RichText::new(&name).strong().color(t.text));
        ui.label(egui::RichText::new(format!("{selected} of {n} selected")).color(t.muted));
        ui.separator();
        ui.toggle_value(&mut app.table_selected_only, "Show selected records");
        if ui.add_enabled(selected > 0, egui::Button::new("Clear selection")).clicked() {
            app.run("map.clearSelection", json!({ "layer": id }));
        }
        if ui.add_enabled(selected > 0, egui::Button::new("Zoom to selection")).clicked() {
            app.run("map.zoomToSelection", json!({}));
        }
        if ui.button("Add Field").clicked() {
            app.dialog = Some(Dialog::AddField { layer: id, name: String::new(), kind: "double" });
        }
        if ui.button("Calculate Field").clicked() {
            app.dialog = Some(Dialog::CalculateField { layer: id, field: fields.first().cloned().unwrap_or_default(), expression: String::new() });
        }
        close = ui.button("×").on_hover_text("Close the table").clicked();
    });
    if close {
        app.show_table = false;
        return;
    }
    ui.separator();
    let Some(layer) = app.active_layer() else { return };
    let rows: Vec<usize> = if app.table_selected_only { layer.selection.iter().copied().collect() } else { (0..n).collect() };
    let mut clicked: Option<(usize, bool)> = None;
    let mut edit: Option<(usize, usize)> = None;
    let width = OID_WIDTH + COL_WIDTH * fields.len() as f32;
    egui::ScrollArea::horizontal().id_salt("table-h").auto_shrink([false, false]).show(ui, |ui| {
        ui.set_min_width(width);
        // Header.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (name, w) in std::iter::once(("OID", OID_WIDTH)).chain(fields.iter().map(|f| (f.as_str(), COL_WIDTH))) {
                let (r, _) = ui.allocate_exact_size(egui::vec2(w, ROW), egui::Sense::hover());
                ui.painter().rect_filled(r, 0.0, t.window);
                ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                ui.painter().text(r.left_center() + egui::vec2(5.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(12.0), t.text);
            }
        });
        egui::ScrollArea::vertical().id_salt("table-v").auto_shrink([false, false]).show_rows(ui, ROW, rows.len(), |ui, range| {
            for &r in rows.get(range).unwrap_or_default() {
                let sel = layer.selection.contains(&r);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let cells = std::iter::once((format!("{}", r + 1), OID_WIDTH, None)).chain(
                        fields
                            .iter()
                            .enumerate()
                            .map(|(i, _)| (layer.data.value(r, i).map(|v| v.to_string()).unwrap_or_default(), COL_WIDTH, Some(i))),
                    );
                    for (text, w, field) in cells {
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, ROW), egui::Sense::click());
                        if sel {
                            ui.painter().rect_filled(rect, 0.0, t.selected_row);
                        }
                        ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(0.5, t.border), egui::StrokeKind::Inside);
                        let shown: String = text.chars().take(40).collect();
                        let color = if text == "<Null>" { t.muted } else { t.text };
                        ui.painter().with_clip_rect(rect.shrink(1.0)).text(
                            rect.left_center() + egui::vec2(5.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            shown,
                            egui::FontId::proportional(12.0),
                            color,
                        );
                        if resp.double_clicked()
                            && let Some(f) = field
                        {
                            edit = Some((r, f));
                        } else if resp.clicked() {
                            clicked = Some((r, ui.input(|i| i.modifiers.command)));
                        }
                    }
                });
            }
        });
    });
    if let Some((row, add)) = clicked {
        app.run("feature.select", json!({ "layer": id, "rows": [row], "mode": if add { "add" } else { "new" } }));
    }
    if let Some((row, field)) = edit
        && let Some(name) = fields.get(field)
    {
        let value = app.active_layer().and_then(|l| l.data.value(row, field)).filter(|v| !v.is_null()).map(|v| v.to_string()).unwrap_or_default();
        app.dialog = Some(Dialog::EditValue { layer: id, row, field: name.clone(), value });
    }
}
