//! MK aeGIS's egui frontend (L5), swappable: nothing below depends on it. Panels read the
//! session's state and act only through [`MapApp::run`], which executes engine commands.
//!
//! The window follows ArcGIS Pro's arrangement: the ribbon on top, the Contents pane on the
//! left, the map in the middle, a pane on the right (Geoprocessing, Symbology, History), the
//! attribute table below, and the status bar with scale and coordinates.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod backstage;
mod contents;
#[cfg(not(target_arch = "wasm32"))]
pub mod control;
mod credits;
mod dialogs;
pub mod icons;
mod mapview;
mod pane;
mod ribbon;
pub mod splash;
mod table;
pub mod theme;

use std::path::PathBuf;

use mkaegis_engine::Session;
use serde_json::{Value as Json, json};

pub use backstage::Page as BackstagePage;
pub use dialogs::{Dialog, Discard};
pub use pane::{Pane, Tool as GpTool};
pub use ribbon::Tab;
use theme::Tokens;

const STATUS_HEIGHT: f32 = 26.0;
const RAIL_WIDTH: f32 = 44.0;
const PANE_STRIP: f32 = 30.0;

/// What a click on the map does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    /// Drag pans, click identifies.
    #[default]
    Explore,
    /// Click or drag a box selects.
    Select,
}

/// Platform services the app crate provides (native file dialogs); without them the app asks
/// for paths in its own dialogs.
#[derive(Default)]
pub struct Services {
    /// Pick a file to open: (title, extensions).
    pub open_file: Option<Box<dyn Fn(&str, &[&str]) -> Option<PathBuf>>>,
    /// Pick a file to save: (title, extensions, suggested name).
    pub save_file: Option<Box<dyn Fn(&str, &[&str], &str) -> Option<PathBuf>>>,
    /// Keep the recent projects for later runs.
    pub save_recent: Option<Box<dyn Fn(&[PathBuf])>>,
}

/// The whole window's state.
pub struct MapApp {
    pub session: Session,
    pub tab: Tab,
    /// The layer the Contents pane has selected (by id).
    pub active: Option<u32>,
    pub tool: Tool,
    pub show_table: bool,
    pub pane: Option<Pane>,
    pub dialog: Option<Dialog>,
    /// The feature Explore last identified: (layer id, row).
    pub identify: Option<(u32, usize)>,
    /// The last message and whether it was an error.
    pub status: Option<(String, bool)>,
    pub services: Services,
    pub gp: pane::GpState,
    pub table_selected_only: bool,
    /// The ribbon hidden, only the tabs showing.
    pub ribbon_collapsed: bool,
    /// Save the project after every change (once it has a file).
    pub autosave: bool,
    /// The window's zoom, in percent.
    pub zoom: u32,
    /// The Contents pane open (the view rail toggles it).
    pub show_contents: bool,
    /// The command search, open with its query.
    pub search: Option<String>,
    /// Closing the window was confirmed (saved or discarded): the next close request goes through.
    pub close_confirmed: bool,
    /// The File backstage's page while it is open.
    pub backstage: Option<BackstagePage>,
    /// Projects opened or saved, newest first (the backstage lists them).
    pub recent: Vec<PathBuf>,
    view: mapview::State,
    themed: bool,
    title: String,
    #[cfg(not(target_arch = "wasm32"))]
    pub control: Option<control::Server>,
}

impl Default for MapApp {
    fn default() -> Self {
        MapApp::new(Session::default())
    }
}

impl MapApp {
    pub fn new(session: Session) -> Self {
        let active = session.map.layers.first().map(|l| l.id);
        MapApp {
            session,
            tab: Tab::Map,
            active,
            tool: Tool::Explore,
            show_table: false,
            pane: None,
            dialog: None,
            identify: None,
            status: None,
            services: Services::default(),
            gp: pane::GpState::default(),
            table_selected_only: false,
            ribbon_collapsed: false,
            autosave: false,
            zoom: 100,
            show_contents: true,
            search: None,
            close_confirmed: false,
            backstage: None,
            recent: Vec::new(),
            view: mapview::State::default(),
            themed: false,
            title: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            control: None,
        }
    }

    /// Runs a command and reports its outcome in the status bar.
    pub fn run(&mut self, id: &str, params: Json) -> Option<Json> {
        match self.session.execute(id, &params) {
            Ok(v) => {
                let label = mkaegis_engine::Session::command(id).map_or(id, |c| c.label);
                let msg = v.get("message").and_then(Json::as_str).map_or_else(|| label.to_string(), |m| format!("{label}: {m}"));
                if let Some(warnings) = v.get("warnings").and_then(Json::as_array).filter(|w| !w.is_empty()) {
                    let w: Vec<&str> = warnings.iter().filter_map(Json::as_str).collect();
                    self.status = Some((format!("{msg} — {}", w.join("; ")), false));
                } else {
                    self.status = Some((msg, false));
                }
                if let Some(id) = v.get("layer").and_then(Json::as_u64).and_then(|i| u32::try_from(i).ok()) {
                    self.active = Some(id);
                }
                self.fix_active();
                self.autosave_after(id);
                if matches!(id, "project.open" | "project.save" | "project.saveAs")
                    && let Some(path) = self.session.path.clone()
                {
                    self.remember(path);
                }
                Some(v)
            }
            Err(e) => {
                self.status = Some((e.to_string(), true));
                None
            }
        }
    }

    /// With AutoSave on, an edit is written to the project file at once.
    fn autosave_after(&mut self, id: &str) {
        let edit =
            matches!(id, "edit.undo" | "edit.redo") || mkaegis_engine::Session::command(id).is_some_and(|c| c.kind == mkaegis_engine::Kind::Edit);
        if self.autosave
            && edit
            && self.session.path.is_some()
            && id != "project.open"
            && id != "project.new"
            && let Err(e) = self.session.execute("project.save", &json!({}))
        {
            self.status = Some((format!("AutoSave: {e}"), true));
        }
    }

    /// The project's name for the title: its file's, or Untitled.
    pub fn document_name(&self) -> String {
        self.session.path.as_ref().and_then(|p| p.file_stem()).map_or("Untitled".to_string(), |s| s.to_string_lossy().into_owned())
    }

    /// Runs a command the way its ribbon button does: commands that need parameters open their
    /// dialog or pane; the rest run on the active layer.
    pub fn invoke(&mut self, id: &str) {
        use pane::Tool as T;
        let active = self.active;
        let gp = |app: &mut MapApp, tool: T| {
            app.gp.open(tool, active);
            app.pane = Some(Pane::Geoprocessing);
        };
        match id {
            "map.addData" => self.add_data(),
            // New and Open replace the map: both ask first when it has unsaved changes (#45).
            "project.new" => self.new_project(),
            "project.open" => self.open_project(),
            "project.save" => self.save_project(false),
            "project.saveAs" => self.save_project(true),
            "map.selectByAttributes" => {
                if let Some(layer) = active {
                    self.dialog = Some(Dialog::SelectByAttributes { layer, clause: String::new(), mode: "new" });
                }
            }
            "map.selectByLocation" => gp(self, T::SelectByLocation),
            "analysis.buffer" => gp(self, T::Buffer),
            "management.project" => gp(self, T::Project),
            "management.calculateGeometry" => gp(self, T::CalculateGeometry),
            "analysis.summaryStatistics" => gp(self, T::SummaryStatistics),
            "layer.symbology" | "layer.labels" | "layer.setOpacity" => self.pane = Some(Pane::Symbology),
            "map.selectByRectangle" => self.tool = Tool::Select,
            "feature.identify" => self.tool = Tool::Explore,
            "layer.addField" => {
                if let Some(layer) = active {
                    self.dialog = Some(Dialog::AddField { layer, name: String::new(), kind: "double" });
                }
            }
            "layer.calculateField" => {
                if let Some(l) = self.active_layer() {
                    let field = l.data.fields.first().map(|f| f.name.clone()).unwrap_or_default();
                    self.dialog = Some(Dialog::CalculateField { layer: l.id, field, expression: String::new() });
                }
            }
            "layer.rename" => {
                if let Some(l) = self.active_layer() {
                    self.dialog = Some(Dialog::Rename { layer: l.id, name: l.name.clone() });
                }
            }
            "layer.info" => {
                if let Some(layer) = active {
                    self.dialog = Some(Dialog::LayerInfo { layer });
                }
            }
            "map.setCrs" => self.dialog = Some(Dialog::CoordinateSystem { epsg: self.session.map.crs.epsg().to_string() }),
            "share.exportMap" => self.save_to("Export Map", "share.exportMap", &["png"], "map.png", json!({ "width": 1600, "height": 1000 })),
            "layer.export" => {
                if let Some(l) = self.active_layer() {
                    let (layer, name) = (l.id, format!("{}.geojson", l.name));
                    self.save_to("Export Features", "layer.export", &["geojson", "shp"], &name, json!({ "layer": layer }));
                }
            }
            "map.describe" | "feature.setValue" | "map.setExtent" => self.show_table = true,
            other => {
                let needs_layer = other.starts_with("layer.") || other.starts_with("feature.") || other == "map.zoomToLayer";
                let params = if needs_layer { json!({ "layer": active }) } else { json!({}) };
                self.run(other, params);
            }
        }
    }

    /// The active layer still exists, or the top one is active.
    fn fix_active(&mut self) {
        let layers = &self.session.map.layers;
        if !self.active.is_some_and(|a| layers.iter().any(|l| l.id == a)) {
            self.active = layers.first().map(|l| l.id);
        }
        if self.identify.is_some_and(|(l, r)| !layers.iter().any(|x| x.id == l && r < x.data.features.len())) {
            self.identify = None;
        }
    }

    pub fn active_layer(&self) -> Option<&mkaegis_model::Layer> {
        self.active.and_then(|a| self.session.map.layers.iter().find(|l| l.id == a))
    }

    /// Opens a data file: the native dialog if there is one, else the app's own.
    pub fn add_data(&mut self) {
        match &self.services.open_file {
            Some(pick) => {
                if let Some(path) = pick("Add Data", &mkaegis_engine::io::DATA_EXTENSIONS) {
                    self.run("map.addData", json!({ "path": path.display().to_string() }));
                }
            }
            None => self.dialog = Some(Dialog::Path { title: "Add Data", command: "map.addData", path: String::new() }),
        }
    }

    /// Project › New, asking first when the map has unsaved changes.
    pub fn new_project(&mut self) {
        self.discard_or_ask(Discard::New);
    }

    /// Project › Open, asking first when the map has unsaved changes.
    pub fn open_project(&mut self) {
        self.discard_or_ask(Discard::Open);
    }

    /// Answers the Unsaved Changes dialog: saves first when asked (and stops if that didn't
    /// save), then does what was waiting.
    pub fn resolve_unsaved(&mut self, then: Discard, save: bool) {
        if save {
            self.save_project(false);
            if self.session.modified {
                return;
            }
        }
        match then {
            Discard::Close => self.close_confirmed = true,
            Discard::New => {
                self.run("project.new", json!({}));
            }
            Discard::Sample => {
                if self.run("project.new", json!({})).is_some() {
                    self.run("map.addSample", json!({}));
                    // The sample is where the map starts: nothing to save yet.
                    self.session.modified = false;
                }
            }
            Discard::Open => self.pick_project(),
            Discard::OpenFile(path) => {
                self.run("project.open", json!({ "path": path.display().to_string() }));
            }
        }
    }

    /// Puts `path` first among the recent projects and keeps the list for later runs.
    pub fn remember(&mut self, path: PathBuf) {
        self.recent.retain(|p| *p != path);
        self.recent.insert(0, path);
        self.recent.truncate(backstage::RECENT);
        if let Some(save) = &self.services.save_recent {
            save(&self.recent);
        }
    }

    /// Opens the File backstage on its Home page.
    pub fn open_backstage(&mut self) {
        self.backstage = Some(BackstagePage::Home);
    }

    /// Does what the backstage asked for. What replaces the map leaves the backstage first, so
    /// the Unsaved Changes question, if any, shows over the map it is about.
    fn backstage_act(&mut self, action: backstage::Action, ctx: &egui::Context) {
        use backstage::Action as A;
        match action {
            A::Back => self.backstage = None,
            A::Page(page) => self.backstage = Some(page),
            A::Blank => {
                self.backstage = None;
                self.discard_or_ask(Discard::New);
            }
            A::Sample => {
                self.backstage = None;
                self.discard_or_ask(Discard::Sample);
            }
            A::Open(path) => {
                self.backstage = None;
                self.discard_or_ask(Discard::OpenFile(path));
            }
            A::Browse => {
                self.backstage = None;
                self.open_project();
            }
            A::AddData => {
                self.backstage = None;
                self.add_data();
            }
            A::Save => {
                self.save_project(false);
                if !self.session.modified {
                    self.backstage = None;
                }
            }
            A::SaveAs => {
                self.backstage = None;
                self.save_project(true);
            }
            A::ExportMap => {
                self.backstage = None;
                self.invoke("share.exportMap");
            }
            A::ExportFeatures => {
                self.backstage = None;
                self.invoke("layer.export");
            }
            A::Close => {
                self.backstage = None;
                self.discard_or_ask(Discard::New);
            }
            A::About => {
                self.backstage = None;
                self.dialog = Some(Dialog::About);
            }
            A::Exit => {
                self.backstage = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// The File backstage over the window under its title row.
    fn show_backstage(&mut self, ui: &mut egui::Ui, page: BackstagePage) {
        let map = &self.session.map;
        let name = self.session.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
        let active = self.active.and_then(|id| map.layers.iter().find(|l| l.id == id)).map(|l| l.name.clone());
        let view = backstage::View {
            page,
            recent: &self.recent,
            file_name: name.as_deref(),
            file_path: self.session.path.as_deref(),
            modified: self.session.modified,
            layers: map.layers.len(),
            features: map.layers.iter().map(|l| l.data.features.len()).sum(),
            crs: map.crs.name(),
            weights: map.weights.len(),
            active_layer: active.as_deref(),
        };
        if let Some(action) = backstage::show(ui, &view) {
            let ctx = ui.ctx().clone();
            self.backstage_act(action, &ctx);
        }
    }

    fn discard_or_ask(&mut self, then: Discard) {
        if self.session.modified {
            self.dialog = Some(Dialog::UnsavedChanges { then });
        } else {
            self.resolve_unsaved(then, false);
        }
    }

    fn pick_project(&mut self) {
        match &self.services.open_file {
            Some(pick) => {
                if let Some(path) = pick("Open Project", &[mkaegis_engine::io::PROJECT_EXTENSION]) {
                    self.run("project.open", json!({ "path": path.display().to_string() }));
                }
            }
            None => self.dialog = Some(Dialog::Path { title: "Open Project", command: "project.open", path: String::new() }),
        }
    }

    pub fn save_project(&mut self, as_new: bool) {
        if !as_new && self.session.path.is_some() {
            self.run("project.save", json!({}));
            return;
        }
        match &self.services.save_file {
            Some(pick) => {
                if let Some(path) = pick("Save Project As", &[mkaegis_engine::io::PROJECT_EXTENSION], "map.aegis") {
                    self.run("project.saveAs", json!({ "path": path.display().to_string() }));
                }
            }
            None => self.dialog = Some(Dialog::Path { title: "Save Project As", command: "project.saveAs", path: "map.aegis".into() }),
        }
    }

    /// Statistics › Import Weights: a GAL or GWT file for `layer`.
    pub fn import_weights(&mut self, layer: u32) {
        match &self.services.open_file {
            Some(pick) => {
                if let Some(path) = pick("Import Weights", &["gal", "gwt"]) {
                    self.run("stats.importWeights", json!({ "layer": layer, "path": path.display().to_string() }));
                }
            }
            None => {
                self.dialog = Some(Dialog::SavePath {
                    title: "Import Weights",
                    command: "stats.importWeights",
                    path: String::new(),
                    params: json!({ "layer": layer }),
                })
            }
        }
    }

    /// Asks where to write, then runs `command` with `params` plus that `path`.
    pub fn save_to(&mut self, title: &'static str, command: &'static str, extensions: &[&str], suggested: &str, params: Json) {
        match &self.services.save_file {
            Some(pick) => {
                if let Some(path) = pick(title, extensions, suggested) {
                    let mut p = params;
                    if let Some(o) = p.as_object_mut() {
                        o.insert("path".into(), json!(path.display().to_string()));
                    }
                    self.run(command, p);
                }
            }
            None => self.dialog = Some(Dialog::SavePath { title, command, path: suggested.to_string(), params }),
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
        // While a text box has focus, its own undo, redo and Delete come first: the map-level
        // shortcuts wait until it lets go (#44).
        if !ctx.egui_wants_keyboard_input() {
            // Shift first: a shortcut matches with extra modifiers held, so Ctrl+Z would also
            // take Ctrl+Shift+Z.
            if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
                self.run("edit.redo", json!({}));
            } else if pressed(Modifiers::COMMAND, Key::Z) {
                self.run("edit.undo", json!({}));
            }
            if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
                self.save_project(true);
            } else if pressed(Modifiers::COMMAND, Key::S) {
                self.save_project(false);
            }
            if pressed(Modifiers::COMMAND, Key::O) {
                self.open_project();
            }
            if pressed(Modifiers::COMMAND, Key::N) {
                self.new_project();
            }
            if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::A) {
                self.run("map.clearSelection", json!({}));
            }
            if pressed(Modifiers::NONE, Key::Delete)
                && let Some(layer) = self.active_layer().filter(|l| !l.selection.is_empty()).map(|l| l.id)
            {
                self.run("feature.delete", json!({ "layer": layer }));
            }
        }
        if pressed(Modifiers::CTRL, Key::F1) {
            self.ribbon_collapsed = !self.ribbon_collapsed;
        }
        if pressed(Modifiers::ALT, Key::Q) {
            self.search = Some(String::new());
        }
        if pressed(Modifiers::CTRL, Key::T) {
            self.show_table = !self.show_table;
        }
    }

    /// Draws the window and handles its input.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.themed {
            theme::apply(&ctx);
            self.themed = true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        control::poll(self, &ctx);
        self.shortcuts(&ctx);
        self.guard_close(&ctx);
        self.fix_active();
        let name = self.session.path.as_ref().and_then(|p| p.file_stem()).map_or("Untitled".to_string(), |s| s.to_string_lossy().into_owned());
        let title = format!("{}{name} — MK aeGIS", if self.session.modified { "• " } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        let t = Tokens::get(&ctx);
        let zoom = self.zoom as f32 / 100.0;
        if (ctx.zoom_factor() - zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
        }
        let line = egui::Stroke::new(1.0, t.border);

        if let Some(page) = self.backstage {
            // The File backstage: the title row, then the backstage over everything else, on the
            // window's colour to its very edges.
            ui.painter().with_clip_rect(ctx.content_rect()).rect_filled(ctx.content_rect(), 0.0, t.window);
            egui::Panel::top("ribbon").frame(egui::Frame::new().fill(t.ribbon).stroke(line)).show(ui, |ui| ribbon::title_row(self, ui));
            egui::CentralPanel::no_frame().show(ui, |ui| self.show_backstage(ui, page));
            dialogs::show(self, &ctx);
            return;
        }
        egui::Panel::top("ribbon").frame(egui::Frame::new().fill(t.ribbon).stroke(line)).show(ui, |ui| ribbon::show(self, ui));
        egui::Panel::bottom("status")
            .exact_size(STATUS_HEIGHT)
            .frame(egui::Frame::new().fill(t.status).stroke(line))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("rail")
            .exact_size(RAIL_WIDTH)
            .resizable(false)
            .frame(egui::Frame::new().fill(t.ribbon).stroke(line))
            .show(ui, |ui| self.view_rail(ui));
        if self.show_contents {
            egui::Panel::left("contents")
                .resizable(true)
                .default_size(250.0)
                .min_size(170.0)
                .frame(egui::Frame::new().fill(t.panel).inner_margin(egui::Margin::same(6)).stroke(line))
                .show(ui, |ui| contents::show(self, ui));
        }
        if self.pane.is_some() {
            egui::Panel::right("pane")
                .resizable(true)
                .default_size(300.0)
                .min_size(220.0)
                .frame(egui::Frame::new().fill(t.panel).inner_margin(egui::Margin::same(8)).stroke(line))
                .show(ui, |ui| pane::show(self, ui));
        } else {
            egui::Panel::right("pane-strip")
                .exact_size(PANE_STRIP)
                .resizable(false)
                .frame(egui::Frame::new().fill(t.ribbon).stroke(line))
                .show(ui, |ui| self.pane_strip(ui));
        }
        if self.show_table {
            egui::Panel::bottom("table")
                .resizable(true)
                .default_size(220.0)
                .min_size(90.0)
                .frame(egui::Frame::new().fill(t.panel).inner_margin(egui::Margin::same(4)).stroke(line))
                .show(ui, |ui| table::show(self, ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| mapview::show(self, ui));
        dialogs::show(self, &ctx);
        self.search_popup(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        if self.control.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    /// Closing the window with unsaved changes asks first; once answered, the window closes.
    fn guard_close(&mut self, ctx: &egui::Context) {
        if self.close_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.session.modified {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::UnsavedChanges { then: Discard::Close });
        }
    }

    /// The view rail: toggles for the Contents pane, the attribute table and the map tools.
    fn view_rail(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        let rect = ui.max_rect();
        let items = [
            (icons::Icon::Layers, "Contents: the map's layers", self.show_contents),
            (icons::Icon::Table, "Attribute Table (Ctrl+T)", self.show_table),
            (icons::Icon::Explore, "Explore: pan, zoom and identify", self.tool == Tool::Explore),
            (icons::Icon::Select, "Select features on the map", self.tool == Tool::Select),
        ];
        for (k, (icon, tip, on)) in items.into_iter().enumerate() {
            let slot = egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 4.0 + 42.0 * k as f32), egui::vec2(RAIL_WIDTH - 1.0, 40.0));
            let response = ui.interact(slot, ui.id().with(("rail", k)), egui::Sense::click());
            let painter = ui.painter();
            if on {
                painter.rect_filled(slot, 0.0, t.accent_soft);
                painter.rect_filled(egui::Rect::from_min_size(slot.min, egui::vec2(3.0, slot.height())), 0.0, t.accent);
            } else if response.hovered() {
                painter.rect_filled(slot, 0.0, t.window);
            }
            icons::paint(painter, egui::Rect::from_center_size(slot.center(), egui::vec2(20.0, 20.0)), icon, t.text, t.accent);
            if response.on_hover_text(tip).clicked() {
                match k {
                    0 => self.show_contents = !self.show_contents,
                    1 => self.show_table = !self.show_table,
                    2 => self.tool = Tool::Explore,
                    _ => self.tool = Tool::Select,
                }
            }
        }
    }

    /// The right pane folded to a strip: a chevron and the panes' names, written downwards.
    fn pane_strip(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        let rect = ui.max_rect();
        let chevron = egui::Rect::from_min_size(rect.min + egui::vec2(1.0, 6.0), egui::vec2(PANE_STRIP - 2.0, 24.0));
        let r = ui.interact(chevron, ui.id().with("strip-open"), egui::Sense::click());
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(chevron.center(), egui::vec2(14.0, 14.0)),
            icons::Icon::ChevronLeft,
            t.accent,
            t.accent,
        );
        if r.on_hover_text("Open the pane").clicked() {
            self.pane = Some(Pane::Geoprocessing);
        }
        let mut y = chevron.bottom() + 10.0;
        for (pane, name, icon) in [
            (Pane::Geoprocessing, "Geoprocessing", icons::Icon::Tools),
            (Pane::Symbology, "Symbology", icons::Icon::Symbology),
            (Pane::History, "History", icons::Icon::History),
        ] {
            let galley = ui.painter().layout_no_wrap(name.to_string(), egui::FontId::proportional(12.5), t.muted);
            let h = galley.size().x + 30.0;
            let slot = egui::Rect::from_min_size(egui::pos2(rect.left() + 1.0, y), egui::vec2(PANE_STRIP - 2.0, h));
            let r = ui.interact(slot, ui.id().with(("strip", name)), egui::Sense::click());
            if r.hovered() {
                ui.painter().rect_filled(slot, 0.0, t.window);
            }
            // The name runs downwards, read with the head tilted right.
            let x = slot.center().x + galley.size().y / 2.0;
            ui.painter().add(egui::epaint::TextShape::new(egui::pos2(x, slot.top() + 2.0), galley, t.muted).with_angle(std::f32::consts::FRAC_PI_2));
            icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(egui::pos2(slot.center().x, slot.bottom() - 12.0), egui::vec2(14.0, 14.0)),
                icon,
                t.muted,
                t.accent,
            );
            if r.on_hover_text(format!("Open {name}")).clicked() {
                self.pane = Some(pane);
            }
            y += h + 8.0;
        }
    }

    /// Search: find a command or tool by name and run it as its button would.
    fn search_popup(&mut self, ctx: &egui::Context) {
        let Some(mut query) = self.search.take() else { return };
        let t = Tokens::get(ctx);
        let mut chosen = None;
        let mut open = true;
        let at = egui::pos2(ctx.content_rect().right() - 432.0, ribbon::TITLE_HEIGHT);
        egui::Window::new("Search")
            .id(egui::Id::new("search"))
            .title_bar(false)
            .resizable(false)
            .fixed_pos(at)
            .fixed_size(egui::vec2(420.0, 0.0))
            .show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search commands and tools").desired_width(f32::INFINITY));
                r.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    open = false;
                }
                let q = query.to_lowercase();
                let hits: Vec<&mkaegis_engine::CommandSpec> = mkaegis_engine::Session::commands()
                    .iter()
                    .filter(|c| {
                        q.is_empty()
                            || c.label.to_lowercase().contains(&q)
                            || c.id.to_lowercase().contains(&q)
                            || c.ribbon.to_lowercase().contains(&q)
                    })
                    .take(12)
                    .collect();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    chosen = hits.first().map(|c| c.id);
                }
                ui.add_space(4.0);
                for c in hits {
                    let enabled = self.session.enabled(c.id);
                    let text = format!("{}   ·   {}", c.label, c.ribbon);
                    if ui
                        .add_enabled(enabled, egui::Button::new(egui::RichText::new(text).color(t.text)).frame(false))
                        .on_hover_text(c.params)
                        .clicked()
                    {
                        chosen = Some(c.id);
                    }
                }
            });
        if let Some(id) = chosen {
            self.invoke(id);
        } else if open {
            self.search = Some(query);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        let rect = ui.max_rect();
        let mut row = ui
            .new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(10.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let crs = self.session.map.crs;
        let layers = self.session.map.layers.len();
        let features: usize = self.session.map.layers.iter().map(|l| l.data.features.len()).sum();
        let selected: usize = self.session.map.layers.iter().map(|l| l.selection.len()).sum();
        let mut facts = vec![format!("{layers} layers"), format!("{features} features")];
        if selected > 0 {
            facts.push(format!("{selected} selected"));
        }
        let mut zoom = self.zoom;
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // Zoom: − slider + and the percentage (click it for 100%).
            let percent = ui.add(egui::Label::new(egui::RichText::new(format!("{zoom}%")).color(t.muted).size(12.0)).sense(egui::Sense::click()));
            if percent.on_hover_text("Reset the zoom to 100%").clicked() {
                zoom = 100;
            }
            if ribbon::icon_button(ui, icons::Icon::Plus, "Zoom in", true).clicked() {
                zoom = (zoom / 10 * 10 + 10).min(200);
            }
            ui.spacing_mut().slider_width = 110.0;
            ui.add(egui::Slider::new(&mut zoom, 50..=200).show_value(false)).on_hover_text("Interface zoom");
            if ribbon::icon_button(ui, icons::Icon::Minus, "Zoom out", true).clicked() {
                zoom = (zoom.div_ceil(10) * 10).saturating_sub(10).max(50);
            }
            ui.add_space(16.0);
            ui.label(egui::RichText::new(facts.join("   ·   ")).color(t.muted).size(12.0));
            ui.add_space(16.0);
            ui.label(egui::RichText::new(crs.name()).color(t.muted).size(12.0)).on_hover_text(format!("EPSG:{}", crs.epsg()));
            if let Some(pointer) = self.view.pointer {
                ui.add_space(12.0);
                let text = match crs.to_geographic(pointer) {
                    Ok(g) => format!("{} {}", dms(g.x, 'E', 'W'), dms(g.y, 'N', 'S')),
                    Err(_) => format!("{:.2} {:.2}", pointer.x, pointer.y),
                };
                ui.label(egui::RichText::new(text).color(t.text).size(12.0).monospace());
            }
            if let Some(vp) = self.view.viewport() {
                ui.add_space(12.0);
                ui.label(egui::RichText::new(format!("1:{}", group_thousands(vp.scale(crs)))).color(t.text).size(12.0).monospace())
                    .on_hover_text("Map scale");
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let (text, color) = match &self.status {
                    Some((m, true)) => (m.as_str(), t.error),
                    Some((m, false)) => (m.as_str(), t.muted),
                    None if layers == 0 => ("Empty map. Use File ▸ Add Data… or File ▸ Add Sample Data.", t.muted),
                    None => ("Ready", t.muted),
                };
                ui.add(egui::Label::new(egui::RichText::new(text).color(color).size(12.0)).truncate());
            });
        });
        self.zoom = zoom;
    }
}

/// 1234567.8 as "1.234.568" (thousands grouped by points, as pt-BR and the scale box show them).
pub fn group_thousands(x: f64) -> String {
    if !x.is_finite() {
        return "—".into();
    }
    let digits = format!("{:.0}", x.abs());
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    if x < 0.0 { format!("-{out}") } else { out }
}

/// Degrees, minutes and seconds: 46°37'59.9"W.
pub fn dms(v: f64, pos: char, neg: char) -> String {
    let h = if v < 0.0 { neg } else { pos };
    let a = v.abs();
    let d = a.floor();
    let m = ((a - d) * 60.0).floor();
    let s = ((a - d) * 60.0 - m) * 60.0;
    format!("{d:.0}°{m:02.0}'{s:04.1}\"{h}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn formats() {
        assert_eq!(group_thousands(1_234_567.8), "1.234.568");
        assert_eq!(group_thousands(999.0), "999");
        assert_eq!(group_thousands(f64::NAN), "—");
        assert_eq!(dms(-46.633_3, 'E', 'W'), "46°37'59.9\"W");
    }

    #[test]
    fn the_window_renders_headless_with_every_pane_open() {
        let mut app = MapApp::new(Session::sample());
        app.show_table = true;
        app.pane = Some(Pane::Geoprocessing);
        let mut harness =
            egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_ui_state(|ui, app: &mut MapApp| app.ui(ui), app);
        harness.run_steps(4);
        for pane in [Pane::Symbology, Pane::History] {
            harness.state_mut().pane = Some(pane);
            harness.run_steps(2);
        }
        harness.state_mut().pane = None;
        harness.state_mut().search = Some("buf".into());
        harness.state_mut().ribbon_collapsed = true;
        harness.run_steps(2);
        assert!(harness.state().search.is_some());
        harness.state_mut().invoke("analysis.buffer");
        harness.run_steps(2);
        assert_eq!(harness.state().pane, Some(Pane::Geoprocessing));
        for tab in [Tab::Map, Tab::Analysis, Tab::Statistics, Tab::Edit, Tab::View, Tab::Share] {
            harness.state_mut().tab = tab;
            harness.run_steps(2);
        }
        assert!(harness.state().status.as_ref().is_none_or(|(_, err)| !err));
    }

    #[test]
    fn closing_with_unsaved_changes_asks_first() {
        let mut harness = harness(MapApp::new(Session::sample()));
        harness.state_mut().run("layer.rename", json!({ "layer": "Cidades", "name": "Towns" }));
        assert!(harness.state().session.modified);
        request_close(&mut harness);
        assert_eq!(harness.state().dialog, Some(Dialog::UnsavedChanges { then: Discard::Close }));
        assert!(!harness.state().close_confirmed);
        harness.get_by_label("Cancel").click();
        harness.run_steps(2);
        assert!(harness.state().dialog.is_none());
        assert!(!harness.state().close_confirmed, "Cancel keeps the window open");

        request_close(&mut harness);
        harness.get_by_label("Don't Save").click();
        harness.run_steps(2);
        assert!(harness.state().close_confirmed);
    }

    #[test]
    fn the_spatial_weights_form_builds_weights_and_neighbors_selects_them() {
        let mut harness = harness(MapApp::new(Session::sample()));
        harness.state_mut().tab = Tab::Statistics;
        harness.state_mut().active = harness.state().session.map.layer("Cidades").ok().map(|l| l.id);
        harness.run_steps(2);
        harness.get_by_label("Spatial Weights").click();
        harness.run_steps(2);
        assert_eq!(harness.state().pane, Some(Pane::Geoprocessing));
        harness.get_by_label("Run").click();
        harness.run_steps(2);
        let names: Vec<String> = harness.state().session.map.weights.iter().map(|w| w.name.clone()).collect();
        assert_eq!(names, vec!["Cidades_knn".to_string()]);
        harness.state_mut().run("feature.select", json!({ "layer": "Cidades", "rows": [0] }));
        harness.run_steps(1);
        harness.get_by_label("Neighbors").click();
        harness.run_steps(2);
        assert_eq!(harness.state().session.map.layer("Cidades").map(|l| l.selection.len()).unwrap_or(0), 4);
    }

    #[test]
    fn closing_a_saved_map_does_not_ask() {
        let mut harness = harness(MapApp::new(Session::sample()));
        assert!(!harness.state().session.modified);
        request_close(&mut harness);
        assert!(harness.state().dialog.is_none());
    }

    #[test]
    fn save_in_the_dialog_saves_then_closes() {
        let dir = std::env::temp_dir().join(format!("mkaegis-unsaved-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("map.aegis");
        let mut app = MapApp::new(Session::sample());
        app.run("project.saveAs", json!({ "path": path.display().to_string() }));
        app.run("layer.rename", json!({ "layer": "Cidades", "name": "Towns" }));
        let mut harness = harness(app);
        request_close(&mut harness);
        harness.get_by_label("Save").click();
        harness.run_steps(2);
        assert!(!harness.state().session.modified);
        assert!(harness.state().close_confirmed);
        assert!(std::fs::read_to_string(&path).unwrap().contains("Towns"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_and_open_ask_before_discarding_changes() {
        let mut app = MapApp::new(Session::sample());
        app.run("layer.rename", json!({ "layer": "Cidades", "name": "Towns" }));
        app.new_project();
        assert_eq!(app.dialog, Some(Dialog::UnsavedChanges { then: Discard::New }));
        assert!(!app.session.map.layers.is_empty(), "nothing is discarded before the answer");
        app.dialog = None;
        app.resolve_unsaved(Discard::New, false);
        assert!(app.session.map.layers.is_empty());
        app.open_project();
        assert!(!matches!(app.dialog, Some(Dialog::UnsavedChanges { .. })), "an empty new map has nothing to lose");
    }

    #[test]
    fn new_from_the_command_search_asks_before_discarding_changes() {
        let mut app = MapApp::new(Session::sample());
        app.run("layer.rename", json!({ "layer": "Cidades", "name": "Towns" }));
        // The command search runs the chosen command through invoke.
        app.invoke("project.new");
        assert_eq!(app.dialog, Some(Dialog::UnsavedChanges { then: Discard::New }));
        assert!(app.session.map.layer("Towns").is_ok(), "nothing is discarded before the answer");
    }

    #[test]
    fn ctrl_shift_z_redoes() {
        let mut harness = harness(MapApp::new(Session::sample()));
        harness.state_mut().run("layer.remove", json!({ "layer": "Rios" }));
        harness.state_mut().run("layer.remove", json!({ "layer": "Ilhas" }));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run_steps(1);
        assert!(harness.state().session.map.layer("Ilhas").is_ok(), "Ctrl+Z undid the last removal");
        assert!(harness.state().session.map.layer("Rios").is_err(), "and only that one");
        harness.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
        harness.run_steps(1);
        assert!(harness.state().session.map.layer("Ilhas").is_err(), "Ctrl+Shift+Z redid it");
        assert!(harness.state().session.map.layer("Rios").is_err(), "instead of undoing again");
    }

    #[test]
    fn map_shortcuts_wait_while_typing() {
        let mut app = MapApp::new(Session::sample());
        app.run("layer.remove", json!({ "layer": "Rios" }));
        app.run("feature.select", json!({ "layer": "Cidades", "rows": [0] }));
        let cidades = app.session.map.layer("Cidades").map(|l| (l.id, l.data.features.len())).unwrap();
        app.active = Some(cidades.0);
        app.dialog = Some(Dialog::Rename { layer: cidades.0, name: "New name".into() });
        let mut harness = harness(app);
        harness.get_by_role(egui::accesskit::Role::TextInput).focus();
        harness.run_steps(1);
        assert!(harness.ctx.egui_wants_keyboard_input(), "the text box has the keyboard");
        for (m, k) in [(egui::Modifiers::COMMAND, egui::Key::Z), (egui::Modifiers::NONE, egui::Key::Delete), (egui::Modifiers::COMMAND, egui::Key::N)]
        {
            harness.key_press_modifiers(m, k);
            harness.run_steps(1);
        }
        let s = &harness.state().session;
        assert!(s.map.layer("Rios").is_err(), "Ctrl+Z in a text box is the text box's undo");
        assert_eq!(s.map.layer("Cidades").map(|l| l.data.features.len()).ok(), Some(cidades.1), "Delete edits the text");
        assert!(matches!(harness.state().dialog, Some(Dialog::Rename { .. })), "Ctrl+N doesn't start a new map");
    }

    #[test]
    fn file_opens_the_backstage_and_esc_closes_it() {
        let mut harness = harness(MapApp::new(Session::sample()));
        harness.get_by_label("File").click();
        harness.run_steps(2);
        assert_eq!(harness.state().backstage, Some(BackstagePage::Home));
        harness.get_by_label("Info").click();
        harness.run_steps(2);
        assert_eq!(harness.state().backstage, Some(BackstagePage::Info));
        harness.key_press(egui::Key::Escape);
        harness.run_steps(2);
        assert_eq!(harness.state().backstage, None);
    }

    #[test]
    fn the_sample_map_card_asks_before_discarding_changes() {
        let mut app = MapApp::new(Session::sample());
        app.run("layer.rename", json!({ "layer": "Cidades", "name": "Towns" }));
        app.backstage = Some(BackstagePage::New);
        let mut harness = harness(app);
        harness.get_by_label("Sample map").click();
        harness.run_steps(2);
        assert_eq!(harness.state().dialog, Some(Dialog::UnsavedChanges { then: Discard::Sample }));
        assert!(harness.state().session.map.layer("Towns").is_ok(), "nothing discarded before the answer");
        harness.get_by_label("Don't Save").click();
        harness.run_steps(2);
        let s = &harness.state().session;
        assert!(s.map.layer("Cidades").is_ok() && !s.modified, "a fresh sample map, nothing to save");
    }

    #[test]
    fn saved_projects_are_recent_and_open_from_the_backstage() {
        let dir = std::env::temp_dir().join(format!("mkaegis-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("islands.aegis");
        let kept = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut app = MapApp::new(Session::sample());
        let sink = kept.clone();
        app.services.save_recent = Some(Box::new(move |files: &[std::path::PathBuf]| *sink.borrow_mut() = files.to_vec()));
        app.run("project.saveAs", json!({ "path": path.display().to_string() }));
        assert_eq!(app.recent, vec![path.clone()]);
        assert_eq!(*kept.borrow(), vec![path.clone()], "kept for later runs");
        app.run("project.new", json!({}));
        app.backstage = Some(BackstagePage::Home);
        let mut harness = harness(app);
        harness.get_by_label("islands.aegis").click();
        harness.run_steps(2);
        assert_eq!(harness.state().backstage, None);
        assert_eq!(harness.state().session.path.as_deref(), Some(path.as_path()));
        assert!(harness.state().session.map.layer("Cidades").is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    fn harness(app: MapApp) -> egui_kittest::Harness<'static, MapApp> {
        let mut harness =
            egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_ui_state(|ui, app: &mut MapApp| app.ui(ui), app);
        harness.run_steps(2);
        harness
    }

    fn request_close(harness: &mut egui_kittest::Harness<'static, MapApp>) {
        harness.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
        harness.run_steps(2);
    }
}
