//! The ribbon: the Project menu, the tabs (Map, Analysis, Edit, View, Share) and their groups
//! of buttons. Each button runs a command or opens the pane or dialog that gathers its
//! parameters.

use egui::{Rect, Sense, Ui, vec2};
use serde_json::json;

use crate::i18n::{Language, tr};
use crate::icons::{self, Icon};
use crate::pane::{Pane, Tool as GpTool};
use crate::theme::Tokens;
use crate::{Dialog, MapApp, Tool};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Map,
    Analysis,
    Statistics,
    Edit,
    View,
    Share,
}

impl Tab {
    const ALL: [Tab; 6] = [Tab::Map, Tab::Analysis, Tab::Statistics, Tab::Edit, Tab::View, Tab::Share];

    fn label(self) -> &'static str {
        match self {
            Tab::Map => tr("Map"),
            Tab::Analysis => tr("Analysis"),
            Tab::Statistics => tr("Statistics"),
            Tab::Edit => tr("Edit"),
            Tab::View => tr("View"),
            Tab::Share => tr("Share"),
        }
    }
}

const BUTTON: egui::Vec2 = vec2(66.0, 62.0);
/// The widest a ribbon button grows for its label.
const BUTTON_MAX_WIDTH: f32 = 120.0;
const BUTTON_FONT: f32 = 11.0;

/// A ribbon button's width: wide enough that no word of its label breaks and the label takes two
/// lines at most. Portuguese words run longer than English ones (Geoprocessamento, coordenadas).
fn button_width(ui: &Ui, label: &str) -> f32 {
    let font = egui::FontId::proportional(BUTTON_FONT);
    let color = egui::Color32::WHITE;
    let longest = label.split_whitespace().map(|w| ui.painter().layout_no_wrap(w.to_string(), font.clone(), color).size().x).fold(0.0, f32::max);
    let mut width = BUTTON.x.max(longest + 6.0).min(BUTTON_MAX_WIDTH);
    while width < BUTTON_MAX_WIDTH && ui.painter().layout(label.to_string(), font.clone(), color, width - 4.0).rows.len() > 2 {
        width += 6.0;
    }
    width
}

/// A large ribbon button: icon above, label below. Returns whether it was clicked.
fn button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, checked: bool, tip: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let size = vec2(button_width(ui, label), BUTTON.y);
    let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let response = response.on_hover_text(tip);
    // Named for screen readers (and UI tests), since the label is painted.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if checked {
        ui.painter().rect_filled(rect.shrink(1.0), 3.0, t.accent_soft);
    } else if enabled && response.hovered() {
        ui.painter().rect_filled(rect.shrink(1.0), 3.0, t.window);
    }
    let (fg, accent) = if enabled { (t.text, t.accent) } else { (t.muted.gamma_multiply(0.6), t.muted.gamma_multiply(0.6)) };
    let icon_rect = Rect::from_center_size(rect.center_top() + vec2(0.0, 18.0), vec2(26.0, 26.0));
    icons::paint(ui.painter(), icon_rect, icon, fg, accent);
    let galley = ui.painter().layout(label.to_string(), egui::FontId::proportional(BUTTON_FONT), fg, rect.width() - 4.0);
    let pos = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 34.0);
    ui.painter().galley(pos, galley, fg);
    enabled && response.clicked()
}

/// A group of buttons with its name underneath and a divider after it.
fn group(ui: &mut Ui, name: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    let inner = ui.vertical(|ui| {
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            add(ui);
        });
        let galley = ui.painter().layout_no_wrap(name.to_string(), egui::FontId::proportional(10.5), t.muted);
        let width = row.response.rect.width().max(galley.size().x);
        let (rect, _) = ui.allocate_exact_size(vec2(width, 14.0), Sense::hover());
        ui.painter().galley(egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top()), galley, t.muted);
    });
    let r = inner.response.rect;
    ui.add_space(4.0);
    let x = ui.cursor().left();
    ui.painter().vline(x, (r.top() + 6.0)..=(r.bottom() - 4.0), egui::Stroke::new(1.0, t.border));
    ui.add_space(6.0);
}

pub const TITLE_HEIGHT: f32 = 36.0;

/// A small square icon button for the title row and the status bar.
pub(crate) fn icon_button(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 26.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 0.0, t.window);
    }
    let c = if enabled { t.text } else { t.muted.gamma_multiply(0.5) };
    let a = if enabled { t.accent } else { c };
    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), icon, c, a);
    response.on_hover_text(tip)
}

/// An on/off switch: a pill with a knob.
fn switch(ui: &mut Ui, on: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, response) = ui.allocate_exact_size(vec2(30.0, 16.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 8.0, if on { t.accent } else { egui::Color32::TRANSPARENT });
    p.rect_stroke(r, 8.0, egui::Stroke::new(1.0, if on { t.accent } else { t.muted }), egui::StrokeKind::Inside);
    let knob = if on { egui::pos2(r.right() - 8.0, r.center().y) } else { egui::pos2(r.left() + 8.0, r.center().y) };
    p.circle_filled(knob, 5.0, if on { egui::Color32::WHITE } else { t.muted });
    response
}

/// The title row: the window's own title bar. Logo, autosave, save, undo and redo on the left;
/// the project's name in the middle; theme, share and search on the right. Drag it to move the
/// window, double-click to maximize.
fn title_bar(app: &mut MapApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (rect, background) = ui.allocate_exact_size(vec2(ui.available_width(), TITLE_HEIGHT), Sense::click_and_drag());
    if background.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if background.double_clicked() {
        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    let title = format!("{}{}", if app.session.modified { "• " } else { "" }, app.document_name());
    // Bold by drawing twice, half a point apart: the built-in fonts have no bold face.
    for dx in [0.0, 0.5] {
        ui.painter().text(rect.center() + vec2(dx, 0.0), egui::Align2::CENTER_CENTER, &title, egui::FontId::proportional(13.5), t.text);
    }
    let mut row =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(10.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    row.spacing_mut().item_spacing.x = 2.0;
    let (logo, _) = row.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
    icons::paint(row.painter(), logo, Icon::Logo, t.accent, t.accent);
    row.add_space(10.0);
    row.label(egui::RichText::new(tr("AutoSave")).size(12.5).color(t.muted));
    row.add_space(4.0);
    let tip = if app.autosave { tr("AutoSave is on: every change is saved to the project file") } else { tr("AutoSave is off") };
    if switch(&mut row, app.autosave).on_hover_text(tip).clicked() {
        app.autosave = !app.autosave;
        if app.autosave && app.session.path.is_none() {
            app.save_project(true);
        }
    }
    row.add_space(8.0);
    if icon_button(&mut row, Icon::Save, tr("Save (Ctrl+S)"), true).clicked() {
        app.save_project(false);
    }
    let undo = app.session.can_undo().map(|l| crate::trf("Undo {action}", &[("action", &tr(l))]));
    if icon_button(&mut row, Icon::Undo, undo.as_deref().unwrap_or(tr("Undo")), undo.is_some()).clicked() && undo.is_some() {
        app.run("edit.undo", json!({}));
    }
    let redo = app.session.can_redo().map(|l| crate::trf("Redo {action}", &[("action", &tr(l))]));
    if icon_button(&mut row, Icon::Redo, redo.as_deref().unwrap_or(tr("Redo")), redo.is_some()).clicked() && redo.is_some() {
        app.run("edit.redo", json!({}));
    }
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if icon_button(ui, Icon::Search, tr("Search commands and tools (Alt+Q)"), true).clicked() {
            app.search = Some(String::new());
        }
        let has_layers = !app.session.map.layers.is_empty();
        let share = icon_button(ui, Icon::Share, tr("Share: export the map or a layer"), has_layers);
        egui::Popup::menu(&share).show(|ui| {
            if ui.button(tr("Export Map (PNG)…")).clicked() {
                app.save_to(tr("Export Map"), "share.exportMap", &["png"], "map.png", json!({ "width": 1600, "height": 1000 }));
            }
            if ui.button(tr("Export Features…")).clicked()
                && let Some(l) = app.active_layer()
            {
                let (id, name) = (l.id, format!("{}.geojson", l.name));
                app.save_to(tr("Export Features"), "layer.export", &["geojson", "shp", "csv"], &name, json!({ "layer": id }));
            }
        });
        let dark = ui.ctx().global_style().visuals.dark_mode;
        let theme = icon_button(ui, Icon::Contrast, tr("Light or dark (right-click for more)"), true);
        if theme.clicked() {
            ui.ctx().set_theme(if dark { egui::ThemePreference::Light } else { egui::ThemePreference::Dark });
        }
        theme.context_menu(|ui| {
            for (pref, name) in [
                (egui::ThemePreference::Light, tr("Light")),
                (egui::ThemePreference::Dark, tr("Dark")),
                (egui::ThemePreference::System, tr("Follow the system")),
            ] {
                if ui.button(name).clicked() {
                    ui.ctx().set_theme(pref);
                }
            }
        });
    });
}

/// The title row alone, over the File backstage.
pub(crate) fn title_row(app: &mut MapApp, ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 0.0;
    title_bar(app, ui);
}

pub fn show(app: &mut MapApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.y = 0.0;
    title_bar(app, ui);
    // The tab row: File as a menu at its start, the tabs, and the chevron that hides the ribbon.
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.x = 16.0;
        ui.set_min_height(28.0);
        // File opens the backstage, as in MK Sigma: Home, New, Open, Info, Save, Export…
        let file = ui.add(egui::Button::new(egui::RichText::new(tr("File")).size(14.0).color(t.accent)).frame(false).min_size(vec2(0.0, 28.0)));
        if file.on_hover_text(tr("New, open, save and export projects")).clicked() {
            app.open_backstage();
        }
        ui.add_space(6.0);
        for tab in Tab::ALL {
            let on = app.tab == tab;
            let text = egui::RichText::new(tab.label()).size(14.0).color(if on { t.text } else { t.muted });
            let r = ui.add(egui::Button::new(if on { text.strong() } else { text }).frame(false).min_size(vec2(0.0, 28.0)));
            if on {
                let b = r.rect;
                ui.painter().rect_filled(
                    Rect::from_min_max(egui::pos2(b.left() + 4.0, b.bottom() - 3.0), egui::pos2(b.right() - 4.0, b.bottom())),
                    0.0,
                    t.accent,
                );
            }
            if r.double_clicked() {
                app.ribbon_collapsed = !app.ribbon_collapsed;
            } else if r.clicked() {
                app.tab = tab;
                app.ribbon_collapsed = false;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            let (icon, tip) = if app.ribbon_collapsed {
                (Icon::ChevronDown, tr("Show the ribbon (Ctrl+F1)"))
            } else {
                (Icon::ChevronUp, tr("Hide the ribbon (Ctrl+F1)"))
            };
            if icon_button(ui, icon, tip, true).clicked() {
                app.ribbon_collapsed = !app.ribbon_collapsed;
            }
        });
    });
    if app.ribbon_collapsed {
        ui.add_space(2.0);
        return;
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.set_min_height(BUTTON.y + 18.0);
        ui.add_space(6.0);
        match app.tab {
            Tab::Map => map_tab(app, ui),
            Tab::Analysis => analysis_tab(app, ui),
            Tab::Statistics => statistics_tab(app, ui),
            Tab::Edit => edit_tab(app, ui),
            Tab::View => view_tab(app, ui),
            Tab::Share => share_tab(app, ui),
        }
    });
    ui.add_space(4.0);
}

fn has_layers(app: &MapApp) -> bool {
    !app.session.map.layers.is_empty()
}

fn active_id(app: &MapApp) -> Option<u32> {
    app.active_layer().map(|l| l.id)
}

fn map_tab(app: &mut MapApp, ui: &mut Ui) {
    let layers = has_layers(app);
    group(ui, tr("Navigate"), |ui| {
        if button(ui, Icon::Explore, tr("Explore"), true, app.tool == Tool::Explore, tr("Pan, zoom and identify features")) {
            app.tool = Tool::Explore;
        }
        if button(ui, Icon::FullExtent, tr("Full Extent"), layers, false, tr("Zoom to every layer")) {
            app.run("map.fullExtent", json!({}));
        }
        if button(ui, Icon::ZoomToLayer, tr("Zoom To Layer"), layers, false, tr("Zoom to the layer selected in Contents"))
            && let Some(id) = active_id(app)
        {
            app.run("map.zoomToLayer", json!({ "layer": id }));
        }
    });
    group(ui, tr("Layer"), |ui| {
        if button(ui, Icon::AddData, tr("Add Data"), true, false, tr("Add a GeoJSON or shapefile layer")) {
            app.add_data();
        }
    });
    let selection = app.session.enabled("map.clearSelection");
    group(ui, tr("Selection"), |ui| {
        if button(ui, Icon::Select, tr("Select"), layers, app.tool == Tool::Select, tr("Click or drag a box on the map to select features")) {
            app.tool = Tool::Select;
        }
        if button(ui, Icon::SelectByAttributes, tr("Select By Attributes"), layers, false, tr("Select features with a SQL clause"))
            && let Some(id) = active_id(app)
        {
            app.dialog = Some(Dialog::SelectByAttributes { layer: id, clause: String::new(), mode: "new" });
        }
        if button(ui, Icon::SelectByLocation, tr("Select By Location"), layers, false, tr("Select features by their relation to another layer's")) {
            app.gp.open(GpTool::SelectByLocation, active_id(app));
            app.pane = Some(Pane::Geoprocessing);
        }
        if button(ui, Icon::ZoomToLayer, tr("Zoom To Selection"), selection, false, tr("Zoom to the selected features")) {
            app.run("map.zoomToSelection", json!({}));
        }
        if button(ui, Icon::Clear, tr("Clear"), selection, false, tr("Clear the selection (Ctrl+Shift+A)")) {
            app.run("map.clearSelection", json!({}));
        }
    });
    group(ui, tr("Appearance"), |ui| {
        if button(ui, Icon::Symbology, tr("Symbology"), layers, app.pane == Some(Pane::Symbology), tr("Symbology and labels of the selected layer")) {
            app.pane = Some(Pane::Symbology);
        }
        let labelled = app.active_layer().is_some_and(|l| l.labels.is_some());
        if button(ui, Icon::Labels, tr("Label"), layers, labelled, tr("Label the selected layer's features"))
            && let Some(l) = app.active_layer()
        {
            let id = l.id;
            let field = if labelled {
                None
            } else {
                l.data.fields.iter().find(|f| f.kind == mkaegis_model::FieldType::Text).or(l.data.fields.first()).map(|f| f.name.clone())
            };
            app.run("layer.labels", json!({ "layer": id, "field": field }));
        }
    });
    group(ui, tr("Inquiry"), |ui| {
        if button(ui, Icon::Table, tr("Attribute Table"), layers, app.show_table, tr("Open the selected layer's attribute table (Ctrl+T)")) {
            app.show_table = !app.show_table;
        }
    });
}

fn analysis_tab(app: &mut MapApp, ui: &mut Ui) {
    let layers = has_layers(app);
    group(ui, tr("Geoprocessing"), |ui| {
        if button(ui, Icon::Tools, tr("Tools"), true, app.pane == Some(Pane::Geoprocessing), tr("Open the Geoprocessing pane")) {
            app.pane = if app.pane == Some(Pane::Geoprocessing) { None } else { Some(Pane::Geoprocessing) };
            app.gp.tool = None;
        }
        if button(ui, Icon::Statistics, tr("History"), true, app.pane == Some(Pane::History), tr("Geoprocessing history")) {
            app.pane = Some(Pane::History);
        }
    });
    group(ui, tr("Tools"), |ui| {
        for (icon, label, tool) in [
            (Icon::Buffer, tr("Buffer"), GpTool::Buffer),
            (Icon::Project, tr("Project|tool"), GpTool::Project),
            (Icon::Calculate, tr("Calculate Geometry"), GpTool::CalculateGeometry),
            (Icon::Statistics, tr("Summary Statistics"), GpTool::SummaryStatistics),
        ] {
            if button(ui, icon, label, layers, app.gp.tool == Some(tool) && app.pane == Some(Pane::Geoprocessing), tool.description()) {
                app.gp.open(tool, active_id(app));
                app.pane = Some(Pane::Geoprocessing);
            }
        }
    });
    // Tables from statistics packages and spreadsheets: joined to a layer, or placed as points.
    group(ui, tr("Tables"), |ui| {
        for (icon, label, tool, enabled) in
            [(Icon::Table, tr("Join Field"), GpTool::JoinField, layers), (Icon::AddData, tr("XY Table To Point"), GpTool::XyTableToPoint, true)]
        {
            if button(ui, icon, label, enabled, app.gp.tool == Some(tool) && app.pane == Some(Pane::Geoprocessing), tool.description()) {
                app.gp.open(tool, active_id(app));
                app.pane = Some(Pane::Geoprocessing);
            }
        }
    });
}

/// The Statistics tab: spatial weights, then the statistics that use them.
fn statistics_tab(app: &mut MapApp, ui: &mut Ui) {
    let layers = has_layers(app);
    let weights = app.session.enabled("stats.weightsInfo");
    group(ui, tr("Weights"), |ui| {
        let open = app.gp.tool == Some(GpTool::SpatialWeights) && app.pane == Some(Pane::Geoprocessing);
        if button(ui, Icon::Weights, tr("Spatial Weights"), layers, open, GpTool::SpatialWeights.description()) {
            app.gp.open(GpTool::SpatialWeights, active_id(app));
            app.pane = Some(Pane::Geoprocessing);
        }
        let selected = app.active_layer().and_then(|l| l.selection.first().copied().map(|r| (l.id, r)));
        let for_layer = selected.and_then(|(id, _)| app.session.map.weights.iter().rev().find(|w| w.layer == id).map(|w| w.name.clone()));
        if button(
            ui,
            Icon::Select,
            tr("Neighbors"),
            selected.is_some() && for_layer.is_some(),
            false,
            tr("Select the neighbours of the selected feature, by the layer's latest weights"),
        ) && let (Some((_, row)), Some(name)) = (selected, for_layer)
        {
            app.run("stats.selectNeighbors", json!({ "weights": name, "row": row }));
        }
        if button(ui, Icon::Open, tr("Import Weights"), layers, false, tr("Read a GAL or GWT weights file for the selected layer"))
            && let Some(id) = active_id(app)
        {
            app.import_weights(id);
        }
        if button(ui, Icon::Export, tr("Export Weights"), weights, false, tr("Write the latest weights as a GAL or GWT file"))
            && let Some(name) = app.session.map.weights.last().map(|w| w.name.clone())
        {
            app.save_to(tr("Export Weights"), "stats.exportWeights", &["gal", "gwt"], &format!("{name}.gal"), json!({ "weights": name }));
        }
    });
    group(ui, tr("Autocorrelation"), |ui| {
        let open = app.gp.tool == Some(GpTool::SpatialAutocorrelation) && app.pane == Some(Pane::Geoprocessing);
        let open_global = open && matches!(app.gp.statistic, "stats.globalMoran" | "stats.gearyC" | "stats.generalG");
        if button(ui, Icon::Statistics, tr("Global Moran's I"), weights, open_global, GpTool::SpatialAutocorrelation.description()) {
            open_statistic(app, "stats.globalMoran");
        }
    });
    group(ui, tr("Explore"), |ui| {
        let open = app.pane == Some(Pane::Charts);
        if button(ui, Icon::Statistics, tr("Charts"), layers, open, tr("Histogram, box plot, scatter and Moran scatter plots, linked to the map")) {
            app.pane = Some(Pane::Charts);
        }
    });
    group(ui, tr("Local"), |ui| {
        let open = app.gp.tool == Some(GpTool::SpatialAutocorrelation) && app.pane == Some(Pane::Geoprocessing);
        let tip = tr("Where the clusters and outliers are: local Moran's I (LISA), mapped High-High, Low-Low, High-Low, Low-High");
        if button(ui, Icon::Statistics, tr("Cluster Map (LISA)"), weights, open && app.gp.statistic == "stats.localMoran", tip) {
            open_statistic(app, "stats.localMoran");
        }
        let tip = tr("Where values are significantly high or low with their neighbours: Getis-Ord Gi*, at 90, 95 and 99 % confidence");
        if button(ui, Icon::Statistics, tr("Hot Spots (Gi*)"), weights, open && app.gp.statistic == "stats.hotSpots", tip) {
            open_statistic(app, "stats.hotSpots");
        }
    });
}

/// Opens the Spatial Autocorrelation tool on one of its statistics.
fn open_statistic(app: &mut MapApp, command: &'static str) {
    app.gp.open(GpTool::SpatialAutocorrelation, active_id(app));
    app.gp.statistic = command;
    app.pane = Some(Pane::Geoprocessing);
}

fn edit_tab(app: &mut MapApp, ui: &mut Ui) {
    group(ui, tr("Manage Edits"), |ui| {
        if button(ui, Icon::Undo, tr("Undo"), app.session.can_undo().is_some(), false, tr("Undo (Ctrl+Z)")) {
            app.run("edit.undo", json!({}));
        }
        if button(ui, Icon::Redo, tr("Redo"), app.session.can_redo().is_some(), false, tr("Redo (Ctrl+Y)")) {
            app.run("edit.redo", json!({}));
        }
    });
    let selected = app.active_layer().is_some_and(|l| !l.selection.is_empty());
    group(ui, tr("Features"), |ui| {
        if button(ui, Icon::Select, tr("Select"), has_layers(app), app.tool == Tool::Select, tr("Select features to edit")) {
            app.tool = Tool::Select;
        }
        if button(ui, Icon::Clear, tr("Delete"), selected, false, tr("Delete the selected features of the selected layer (Delete)"))
            && let Some(id) = active_id(app)
        {
            app.run("feature.delete", json!({ "layer": id }));
        }
        if button(ui, Icon::Table, tr("Attributes"), has_layers(app), app.show_table, tr("Edit attributes in the table")) {
            app.show_table = true;
        }
    });
    group(ui, tr("Fields"), |ui| {
        if button(ui, Icon::New, tr("Add Field"), has_layers(app), false, tr("Add a field to the selected layer"))
            && let Some(id) = active_id(app)
        {
            app.dialog = Some(Dialog::AddField { layer: id, name: String::new(), kind: "double" });
        }
        if button(ui, Icon::Calculate, tr("Calculate Field"), has_layers(app), false, tr("Calculate a field with an expression"))
            && let Some(l) = app.active_layer()
        {
            let field = l.data.fields.first().map(|f| f.name.clone()).unwrap_or_default();
            app.dialog = Some(Dialog::CalculateField { layer: l.id, field, expression: String::new() });
        }
    });
}

fn view_tab(app: &mut MapApp, ui: &mut Ui) {
    group(ui, tr("Windows"), |ui| {
        if button(ui, Icon::Table, tr("Attribute Table"), has_layers(app), app.show_table, tr("Show or hide the attribute table")) {
            app.show_table = !app.show_table;
        }
        if button(ui, Icon::Tools, tr("Geoprocessing"), true, app.pane == Some(Pane::Geoprocessing), tr("Show or hide the Geoprocessing pane")) {
            app.pane = if app.pane == Some(Pane::Geoprocessing) { None } else { Some(Pane::Geoprocessing) };
        }
        if button(ui, Icon::Symbology, tr("Symbology"), has_layers(app), app.pane == Some(Pane::Symbology), tr("Show the Symbology pane")) {
            app.pane = if app.pane == Some(Pane::Symbology) { None } else { Some(Pane::Symbology) };
        }
    });
    group(ui, tr("Map"), |ui| {
        if button(ui, Icon::CoordinateSystem, tr("Coordinate System"), true, false, tr("Change the map's coordinate system")) {
            app.dialog = Some(Dialog::CoordinateSystem { epsg: app.session.map.crs.epsg().to_string() });
        }
        let dark = ui.ctx().global_style().visuals.dark_mode;
        if button(ui, Icon::FullExtent, if dark { tr("Light Theme") } else { tr("Dark Theme") }, true, false, tr("Switch between light and dark")) {
            ui.ctx().set_theme(if dark { egui::Theme::Light } else { egui::Theme::Dark });
        }
    });
    group(ui, tr("Language"), |ui| {
        // Two languages: the button goes to the other one, as the theme button does.
        let next = Language::ALL.iter().copied().cycle().skip_while(|l| *l != app.language).nth(1).unwrap_or_default();
        let tip = crate::trf(
            "Interface language: {language}. Click for {next}.",
            &[("language", &app.language.native_name()), ("next", &next.native_name())],
        );
        if button(ui, Icon::Language, app.language.native_name(), true, false, &tip) {
            app.set_language(next);
        }
    });
}

fn share_tab(app: &mut MapApp, ui: &mut Ui) {
    group(ui, tr("Output"), |ui| {
        if button(ui, Icon::Export, tr("Export Map"), has_layers(app), false, tr("Export the current view as PNG")) {
            app.save_to(tr("Export Map"), "share.exportMap", &["png"], "map.png", json!({ "width": 1600, "height": 1000 }));
        }
    });
    group(ui, tr("Data"), |ui| {
        if button(ui, Icon::Export, tr("Export Features"), has_layers(app), false, tr("Write the selected layer to GeoJSON or a shapefile"))
            && let Some(l) = app.active_layer()
        {
            let (id, name) = (l.id, format!("{}.geojson", l.name));
            app.save_to(tr("Export Features"), "layer.export", &["geojson", "shp", "csv"], &name, json!({ "layer": id }));
        }
        if button(ui, Icon::Save, tr("Save Project"), true, false, tr("Save the project (Ctrl+S)")) {
            app.save_project(false);
        }
    });
}
