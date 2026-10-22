//! The Contents pane: the map and its layers in drawing order, each with its visibility, its
//! symbol (and legend classes) and a context menu.

use egui::{Sense, Ui, vec2};
use mkaegis_geom::GeometryType;
use mkaegis_model::{Basemap, Layer, Symbol};
use serde_json::json;

use crate::i18n::tr;
use crate::theme::{Tokens, color};
use crate::{Dialog, MapApp, Pane};

/// The most features a layer may have for Contents to count its classes every frame.
const MAX_COUNTED: usize = 200_000;

/// The basemap under the layers, at the bottom of the drawing order (#15).
fn basemap_row(app: &mut MapApp, ui: &mut Ui) {
    let current = app.session.map.basemap;
    let name = |b: Option<Basemap>| match b {
        None => tr("None"),
        Some(Basemap::OpenStreetMap) => tr("OpenStreetMap"),
        Some(Basemap::Satellite) => tr("Satellite (NASA, c. 2000)"),
    };
    ui.indent("basemap", |ui| {
        ui.horizontal(|ui| {
            ui.label(tr("Basemap"));
            let mut chosen = current;
            egui::ComboBox::from_id_salt("basemap").selected_text(name(current)).show_ui(ui, |ui| {
                for b in [None, Some(Basemap::OpenStreetMap), Some(Basemap::Satellite)] {
                    ui.selectable_value(&mut chosen, b, name(b));
                }
            });
            if chosen != current {
                app.run("map.setBasemap", json!({ "basemap": chosen.map_or("none", Basemap::id) }));
            }
        });
    });
}

/// A symbol swatch: a square patch, a line or a marker, by geometry type.
fn swatch(ui: &mut Ui, kind: GeometryType, s: &Symbol) {
    let (rect, _) = ui.allocate_exact_size(vec2(18.0, 14.0), Sense::hover());
    let p = ui.painter();
    match kind {
        GeometryType::Polygon => {
            let r = rect.shrink(1.5);
            p.rect_filled(r, 1.0, color(s.fill));
            p.rect_stroke(r, 1.0, egui::Stroke::new(s.width.clamp(0.5, 2.0), color(s.outline)), egui::StrokeKind::Inside);
        }
        GeometryType::Polyline => {
            let y = rect.center().y;
            p.add(egui::Shape::line(
                vec![egui::pos2(rect.left() + 1.0, y + 3.0), egui::pos2(rect.center().x, y - 3.0), egui::pos2(rect.right() - 1.0, y + 2.0)],
                egui::Stroke::new(s.width.clamp(1.0, 4.0), color(s.outline)),
            ));
        }
        GeometryType::Point | GeometryType::Multipoint => {
            let outline = mkaegis_render::raster::marker_outline(s.marker, [rect.center().x, rect.center().y], s.size.clamp(4.0, 12.0));
            let pts: Vec<egui::Pos2> = outline.iter().map(|q| egui::pos2(q[0], q[1])).collect();
            p.add(egui::Shape::convex_polygon(pts, color(s.fill), egui::Stroke::new(s.width.clamp(0.5, 1.5), color(s.outline))));
        }
    }
}

fn row(app: &mut MapApp, ui: &mut Ui, index: usize, last: usize) {
    let t = Tokens::get(ui.ctx());
    let Some(layer) = app.session.map.layers.get(index) else { return };
    let (id, mut visible, name) = (layer.id, layer.visible, layer.name.clone());
    let kind = layer.data.geometry_type;
    let legend = layer.renderer.legend();
    // Each class's feature count, as statisticians read a choropleth; skipped on very large
    // layers, where counting every frame would cost more than it tells.
    let counts = if layer.data.features.len() <= MAX_COUNTED { layer.renderer.class_counts(&layer.data) } else { Vec::new() };
    let selected = layer.selection.len();
    let crs_known = layer.data.crs.is_known();
    let active = app.active == Some(id);
    let response = ui
        .horizontal(|ui| {
            if ui.checkbox(&mut visible, "").on_hover_text(tr("Visible")).changed() {
                app.run("layer.setVisible", json!({ "layer": id, "visible": visible }));
            }
            if let [(label, s)] = legend.as_slice()
                && label.is_empty()
            {
                swatch(ui, kind, s);
            }
            let text = egui::RichText::new(&name).color(t.text);
            let r = ui.selectable_label(active, if active { text.strong() } else { text });
            if selected > 0 {
                ui.label(egui::RichText::new(format!("({selected})")).size(10.5).color(t.accent));
            }
            if !crs_known {
                ui.label(egui::RichText::new(tr("CRS?")).size(10.5).color(t.error)).on_hover_text(tr(
                    "Unknown coordinate system: the layer is drawn as it is, not reprojected, and can't be measured. Define Projection sets it.",
                ));
            }
            r
        })
        .inner;
    if response.clicked() {
        app.active = Some(id);
    }
    if response.double_clicked() {
        app.active = Some(id);
        app.pane = Some(Pane::Symbology);
    }
    response.context_menu(|ui| {
        app.active = Some(id);
        if ui.button(tr("Zoom To Layer")).clicked() {
            app.run("map.zoomToLayer", json!({ "layer": id }));
            ui.close();
        }
        if ui.button(tr("Attribute Table")).clicked() {
            app.show_table = true;
            ui.close();
        }
        if ui.button(tr("Symbology")).clicked() {
            app.pane = Some(Pane::Symbology);
            ui.close();
        }
        ui.separator();
        if ui.add_enabled(index > 0, egui::Button::new(tr("Move Up"))).clicked() {
            app.run("layer.move", json!({ "layer": id, "to": "up" }));
            ui.close();
        }
        if ui.add_enabled(index < last, egui::Button::new(tr("Move Down"))).clicked() {
            app.run("layer.move", json!({ "layer": id, "to": "down" }));
            ui.close();
        }
        if ui.button(tr("Rename")).clicked() {
            app.dialog = Some(Dialog::Rename { layer: id, name: name.clone() });
            ui.close();
        }
        if ui.button(tr("Export Features…")).clicked() {
            app.save_to(tr("Export Features"), "layer.export", &["geojson", "shp", "csv"], &format!("{name}.geojson"), json!({ "layer": id }));
            ui.close();
        }
        if ui.button(tr("Properties")).clicked() {
            app.dialog = Some(Dialog::LayerInfo { layer: id });
            ui.close();
        }
        ui.separator();
        if ui.button(tr("Remove")).clicked() {
            app.run("layer.remove", json!({ "layer": id }));
            ui.close();
        }
    });
    // Classes of unique-values and graduated renderers, indented under the layer.
    if !(legend.len() == 1 && legend.first().is_some_and(|(l, _)| l.is_empty())) {
        if let Some(field) = app.session.map.layers.get(index).and_then(|l: &Layer| l.renderer.field().map(str::to_string)) {
            ui.horizontal(|ui| {
                ui.add_space(30.0);
                ui.label(egui::RichText::new(field).size(10.5).color(t.muted));
            });
        }
        for (k, (label, s)) in legend.iter().enumerate().take(40) {
            ui.horizontal(|ui| {
                ui.add_space(30.0);
                swatch(ui, kind, s);
                ui.label(egui::RichText::new(label).size(11.0).color(t.text));
                if let Some(n) = counts.get(k) {
                    ui.label(egui::RichText::new(format!("({n})")).size(10.5).color(t.muted));
                }
            });
        }
        if legend.len() > 40 {
            ui.horizontal(|ui| {
                ui.add_space(30.0);
                ui.label(egui::RichText::new(crate::trf("… {count} more", &[("count", &(legend.len() - 40))])).size(10.5).color(t.muted));
            });
        }
    }
}

pub fn show(app: &mut MapApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(tr("Contents")).strong().size(14.0).color(t.text));
    ui.label(egui::RichText::new(tr("Drawing Order")).size(10.5).color(t.muted));
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal(|ui| {
            crate::icons::square(ui, 16.0, crate::icons::Icon::FullExtent);
            ui.label(egui::RichText::new(&app.session.map.name).strong().color(t.text)).on_hover_text(format!(
                "{} (EPSG:{})",
                app.session.map.crs.name(),
                app.session.map.crs.epsg()
            ));
        });
        let n = app.session.map.layers.len();
        ui.indent("layers", |ui| {
            for i in 0..n {
                row(app, ui, i, n.saturating_sub(1));
            }
        });
        basemap_row(app, ui);
        if n == 0 {
            ui.add_space(12.0);
            ui.label(egui::RichText::new(tr("No layers yet.")).color(t.muted));
            if ui.link(tr("Add Data…")).clicked() {
                app.add_data();
            }
            if ui.link(tr("Add the sample data")).clicked() {
                app.run("map.addSample", json!({}));
            }
        }
    });
}
