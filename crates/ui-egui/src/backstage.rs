//! The File backstage: a page over the whole window under the title row, as MK Sigma's. On the
//! left, a column in Morrison Kühlsen's ink with its wordmark, MK aeGIS's name and motto, and the
//! places, grouped (Start: Home, New, Open; This project: Info, Save, Save As, Export, Close;
//! MK aeGIS: About, Exit). On the right, the place's page, with Back in its corner (Esc too) and
//! Athena fading in along a wide window's edge. The page only reports what was asked for; the
//! app does it.

use std::path::{Path, PathBuf};

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::icons::{self, Icon};
use crate::splash::{self, BRAND, INK, ON_INK, ON_INK_SOFT};
use crate::theme::Tokens;

/// The column's width.
const COLUMN: f32 = 248.0;
/// How many recent projects the backstage lists.
pub const RECENT: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Home,
    New,
    Open,
    Info,
    Export,
}

/// What was asked for in the backstage this frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    Back,
    Page(Page),
    /// A new, empty map.
    Blank,
    /// A new map with the sample data.
    Sample,
    /// A recent project.
    Open(PathBuf),
    Browse,
    AddData,
    Save,
    SaveAs,
    ExportMap,
    ExportFeatures,
    /// Close the project: a new empty map, asking first when there are unsaved changes.
    Close,
    About,
    Exit,
}

/// What the pages show.
pub(crate) struct View<'a> {
    pub page: Page,
    pub recent: &'a [PathBuf],
    pub file_name: Option<&'a str>,
    pub file_path: Option<&'a Path>,
    pub modified: bool,
    pub layers: usize,
    pub features: usize,
    pub crs: String,
    pub weights: usize,
    /// The layer Export Features writes, when one is active.
    pub active_layer: Option<&'a str>,
}

pub(crate) fn show(ui: &mut Ui, view: &View) -> Option<Action> {
    let t = Tokens::get(ui.ctx());
    let mut action = None;
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = Some(Action::Back);
    }
    // To the window's edges: the backstage covers everything under the title row.
    let below = ui.available_rect_before_wrap();
    let screen = ui.ctx().content_rect();
    let area = Rect::from_min_max(pos2(screen.left(), below.top()), screen.max);
    let column = Rect::from_min_size(area.min, vec2(COLUMN, area.height()));
    // The root Ui comes inset by a margin: paint to the window's edges past it.
    let edges = ui.painter().with_clip_rect(screen);
    edges.rect_filled(column, 0.0, INK);
    edges.rect_filled(Rect::from_min_max(pos2(column.right(), area.top()), area.max), 0.0, t.window);
    let art = artwork(&edges, ui.ctx(), area, t.window);

    let mut col = ui.new_child(egui::UiBuilder::new().max_rect(column).layout(egui::Layout::top_down(egui::Align::Min)));
    col.set_clip_rect(column);
    col.spacing_mut().item_spacing.y = 0.0;
    col.add_space(26.0);
    // The house's mark, then the program's name and motto.
    col.horizontal(|ui| {
        ui.add_space(28.0);
        if let Some(texture) = splash::wordmark(ui.ctx()) {
            let width = COLUMN - 70.0;
            let size = vec2(width, width * texture.size_vec2().y / texture.size_vec2().x.max(1.0));
            ui.add(egui::Image::new((texture.id(), size)).tint(ON_INK)).on_hover_text("Morrison Kühlsen");
        }
    });
    col.add_space(18.0);
    col.horizontal(|ui| {
        ui.add_space(30.0);
        ui.label(RichText::new(splash::NAME).size(20.0).color(ON_INK));
    });
    col.add_space(2.0);
    col.horizontal(|ui| {
        ui.add_space(30.0);
        ui.label(RichText::new(splash::MOTTO).italics().size(12.0).color(ON_INK_SOFT));
    });
    col.add_space(26.0);
    let mut go = |ui: &mut Ui, icon: Icon, label: &str, on: bool, then: Action| {
        if place(ui, icon, label, on) {
            action = Some(then);
        }
    };
    caption(&mut col, "START");
    go(&mut col, Icon::House, "Home", view.page == Page::Home, Action::Page(Page::Home));
    go(&mut col, Icon::New, "New", view.page == Page::New, Action::Page(Page::New));
    go(&mut col, Icon::Open, "Open", view.page == Page::Open, Action::Page(Page::Open));
    col.add_space(10.0);
    caption(&mut col, "THIS PROJECT");
    go(&mut col, Icon::Info, "Info", view.page == Page::Info, Action::Page(Page::Info));
    go(&mut col, Icon::Save, "Save", false, Action::Save);
    go(&mut col, Icon::Save, "Save As", false, Action::SaveAs);
    go(&mut col, Icon::Export, "Export", view.page == Page::Export, Action::Page(Page::Export));
    go(&mut col, Icon::Close, "Close", false, Action::Close);
    col.add_space(10.0);
    caption(&mut col, "MK AEGIS");
    go(&mut col, Icon::Logo, "About", false, Action::About);
    go(&mut col, Icon::ChevronLeft, "Exit", false, Action::Exit);
    // The house's line at the column's foot.
    let foot = pos2(column.left() + 30.0, column.bottom() - 22.0);
    col.painter().text(foot, Align2::LEFT_BOTTOM, splash::HOUSE, FontId::proportional(11.5), ON_INK_SOFT);
    col.painter().text(foot + vec2(0.0, 15.0), Align2::LEFT_BOTTOM, "morrisonkuhlsen.com", FontId::proportional(11.5), ON_INK);

    // The page stops before the artwork, when there is one.
    let right = art.map_or(area.right(), |a| a.left() + a.width() * 0.25);
    let content = Rect::from_min_max(pos2(column.right() + 48.0, area.top() + 30.0), pos2(right - 28.0, area.bottom()));
    let mut page = ui.new_child(egui::UiBuilder::new().max_rect(content).layout(egui::Layout::top_down(egui::Align::Min)));
    page.set_clip_rect(content.expand2(vec2(8.0, 0.0)));
    // Back to the map, in the page's corner.
    let corner = Rect::from_min_size(pos2(content.right() - 200.0, content.top() - 6.0), vec2(200.0, 32.0));
    let mut back = ui.new_child(egui::UiBuilder::new().max_rect(corner).layout(egui::Layout::right_to_left(egui::Align::Center)));
    let button = back.add(egui::Button::new(RichText::new("Back to the map").size(13.5)).min_size(vec2(0.0, 30.0)));
    if button.on_hover_text("Esc").clicked() {
        action = Some(Action::Back);
    }
    let page_action = egui::ScrollArea::vertical().id_salt("backstage").auto_shrink([false, false]).show(&mut page, |ui| pages(ui, view)).inner;
    ui.allocate_rect(area, Sense::hover());
    page_action.or(action)
}

fn pages(ui: &mut Ui, view: &View) -> Option<Action> {
    let t = Tokens::get(ui.ctx());
    let mut action = None;
    match view.page {
        Page::Home | Page::New => {
            if view.page == Page::Home {
                heading(ui, "Welcome to MK aeGIS", splash::TAGLINE);
            } else {
                heading(ui, "New", "Start a map: empty, or with the sample data to try every tool.");
            }
            section(ui, "START");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(16.0, 16.0);
                if card(ui, Icon::New, "New map", "An empty map in Web Mercator. Add shapefiles or GeoJSON to it.") {
                    action = Some(Action::Blank);
                }
                if card(ui, Icon::Layers, "Sample map", "A made-up archipelago with islands, towns, roads and rivers.") {
                    action = Some(Action::Sample);
                }
                if view.page == Page::Home {
                    if card(ui, Icon::Open, "Open a project", "An MK aeGIS project (.aegis): the map, its layers and its spatial weights.") {
                        action = Some(Action::Browse);
                    }
                    if card(ui, Icon::AddData, "Add data", "A shapefile (.shp) or GeoJSON file, to this map.") {
                        action = Some(Action::AddData);
                    }
                }
            });
            if view.page == Page::Home {
                ui.add_space(30.0);
                section(ui, "RECENT");
                if let Some(a) = recent(ui, view.recent) {
                    action = Some(a);
                }
            }
        }
        Page::Open => {
            heading(ui, "Open", "An MK aeGIS project (.aegis). To add a shapefile or GeoJSON to this map, use Add Data.");
            let browse = egui::Button::new(RichText::new("Browse…").size(14.0).color(ON_INK)).fill(BRAND).min_size(vec2(130.0, 34.0));
            if ui.add(browse).on_hover_text("Ctrl+O").clicked() {
                action = Some(Action::Browse);
            }
            ui.add_space(28.0);
            section(ui, "RECENT");
            if let Some(a) = recent(ui, view.recent) {
                action = Some(a);
            }
        }
        Page::Info => {
            let path = view.file_path.map_or_else(|| "Not saved yet".to_string(), |p| p.display().to_string());
            heading(ui, view.file_name.unwrap_or("Untitled"), &path);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
                fact(ui, "Layers", &view.layers.to_string());
                fact(ui, "Features", &view.features.to_string());
                fact(ui, "Coordinate system", &view.crs);
                fact(ui, "Spatial weights", &view.weights.to_string());
                fact(ui, "Saved", if view.modified { "Not yet" } else { "Yes" });
            });
            if view.modified {
                ui.add_space(18.0);
                ui.label(RichText::new("This project has changes that aren't saved.").color(t.muted));
            }
        }
        Page::Export => {
            heading(ui, "Export", "Write the map or a layer to a file other programs read.");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(16.0, 16.0);
                let has_map = view.layers > 0;
                if ui
                    .add_enabled_ui(has_map, |ui| card(ui, Icon::Share, "Map image", "The current view as a PNG image, as Export Map writes it."))
                    .inner
                {
                    action = Some(Action::ExportMap);
                }
                let line = view.active_layer.map_or_else(
                    || "Select a layer in Contents to export its features.".to_string(),
                    |name| format!("The layer {name} as GeoJSON or a shapefile."),
                );
                if ui.add_enabled_ui(view.active_layer.is_some(), |ui| card(ui, Icon::Export, "Features", &line)).inner {
                    action = Some(Action::ExportFeatures);
                }
            });
        }
    }
    action
}

/// A place in the column: a bar, coral when it is the page shown.
fn place(ui: &mut Ui, icon: Icon, label: &str, on: bool) -> bool {
    let (slot, response) = ui.allocate_exact_size(vec2(COLUMN, 38.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
    let bar = slot.shrink2(vec2(14.0, 3.0));
    if on {
        ui.painter().rect_filled(bar, 0.0, BRAND);
    } else if response.hovered() {
        ui.painter().rect_filled(bar, 0.0, Color32::from_white_alpha(18));
    }
    let color = if on { ON_INK } else { ON_INK_SOFT };
    let accent = if on { ON_INK } else { BRAND };
    icons::paint(ui.painter(), Rect::from_center_size(pos2(bar.left() + 22.0, bar.center().y), vec2(16.0, 16.0)), icon, color, accent);
    ui.painter().text(pos2(bar.left() + 44.0, bar.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.5), color);
    response.clicked()
}

/// A caption over a group of places.
fn caption(ui: &mut Ui, text: &str) {
    let (slot, _) = ui.allocate_exact_size(vec2(COLUMN, 26.0), Sense::hover());
    ui.painter().text(pos2(slot.left() + 30.0, slot.bottom() - 6.0), Align2::LEFT_BOTTOM, text, FontId::proportional(11.0), ON_INK_SOFT);
}

/// Athena along the right of a wide window, fading into the page from her left edge. Her
/// rectangle, when there is room for her.
fn artwork(painter: &egui::Painter, ctx: &egui::Context, area: Rect, page: Color32) -> Option<Rect> {
    let width = (area.width() - COLUMN) * 0.36;
    if area.width() < 1100.0 || width < 280.0 {
        return None;
    }
    let texture = splash::artwork(ctx)?;
    let rect = Rect::from_min_max(pos2(area.right() - width, area.top()), area.max);
    // Cover the rectangle: crop the image to its shape, around its centre.
    let image = texture.size_vec2();
    let (wanted, have) = (rect.width() / rect.height(), image.x / image.y.max(1.0));
    let uv = if wanted < have {
        let w = wanted / have;
        Rect::from_min_max(pos2(0.5 - w / 2.0, 0.0), pos2(0.5 + w / 2.0, 1.0))
    } else {
        let h = have / wanted;
        Rect::from_min_max(pos2(0.0, 0.5 - h / 2.0), pos2(1.0, 0.5 + h / 2.0))
    };
    painter.image(texture.id(), rect, uv, Color32::WHITE);
    // The page's colour over her left part, solid at the edge and gone by the middle.
    let fade = Rect::from_min_max(rect.min, pos2(rect.left() + rect.width() * 0.55, rect.bottom()));
    let clear = Color32::from_rgba_unmultiplied(page.r(), page.g(), page.b(), 0);
    let white = egui::epaint::WHITE_UV;
    let mut mesh = egui::Mesh::default();
    mesh.vertices.extend([
        egui::epaint::Vertex { pos: fade.left_top(), uv: white, color: page },
        egui::epaint::Vertex { pos: fade.right_top(), uv: white, color: clear },
        egui::epaint::Vertex { pos: fade.left_bottom(), uv: white, color: page },
        egui::epaint::Vertex { pos: fade.right_bottom(), uv: white, color: clear },
    ]);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
    Some(rect)
}

fn heading(ui: &mut Ui, title: &str, line: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(title).size(30.0).color(t.text));
    ui.label(RichText::new(line).size(14.0).color(t.muted));
    ui.add_space(22.0);
}

fn section(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).size(12.0).color(t.muted));
    ui.add_space(8.0);
}

/// A large card to start something: an icon in a round badge, a title and a line about it.
fn card(ui: &mut Ui, icon: Icon, title: &str, text: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let size = vec2(236.0, 132.0);
    let enabled = ui.is_enabled();
    let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, title));
    let hot = response.hovered() && enabled;
    let mut p = ui.painter().clone();
    if !enabled {
        p.set_opacity(0.45);
    }
    p.rect_filled(rect, 0.0, t.panel);
    p.rect_stroke(rect, 0.0, Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { BRAND } else { t.border }), egui::StrokeKind::Inside);
    let badge = pos2(rect.left() + 34.0, rect.top() + 34.0);
    p.circle_filled(badge, 18.0, if hot { BRAND } else { t.accent_soft });
    let (color, accent) = if hot { (ON_INK, ON_INK) } else { (t.accent, BRAND) };
    icons::paint(&p, Rect::from_center_size(badge, vec2(18.0, 18.0)), icon, color, accent);
    p.text(pos2(rect.left() + 18.0, rect.top() + 66.0), Align2::LEFT_TOP, title, FontId::proportional(15.5), t.text);
    let galley = p.layout(text.to_string(), FontId::proportional(12.5), t.muted, size.x - 36.0);
    p.galley(pos2(rect.left() + 18.0, rect.top() + 88.0), galley, t.muted);
    response.clicked() && enabled
}

/// One fact about the project on Info: a card of a fixed size (so the row knows when to wrap)
/// with its name over its value; a long value is set smaller.
fn fact(ui: &mut Ui, name: &str, value: &str) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(220.0, 72.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, t.panel);
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    p.text(pos2(rect.left() + 16.0, rect.top() + 12.0), Align2::LEFT_TOP, name, FontId::proportional(12.0), t.muted);
    let size = if value.chars().count() > 16 { 14.0 } else { 20.0 };
    let galley = p.layout(value.to_string(), FontId::proportional(size), t.text, rect.width() - 32.0);
    p.galley(pos2(rect.left() + 16.0, rect.top() + 32.0), galley, t.text);
}

/// How long ago, in words: "just now", "5 min ago", "3 h ago", "yesterday", "2 days ago".
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        86_400..172_800 => "yesterday".into(),
        _ => format!("{} days ago", seconds / 86_400),
    }
}

/// The recent projects, newest first: a row each with the name, folder and when the file last
/// changed. Files gone since show dimmed and can't be opened.
fn recent(ui: &mut Ui, files: &[PathBuf]) -> Option<Action> {
    let t = Tokens::get(ui.ctx());
    if files.is_empty() {
        ui.label(RichText::new("Projects you open or save show here.").color(t.muted));
        return None;
    }
    let mut action = None;
    let width = ui.available_width().min(760.0);
    for path in files {
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
        let changed = changed_ago(path);
        let exists = changed.is_some();
        let (rect, response) = ui.allocate_exact_size(vec2(width, 44.0), if exists { Sense::click() } else { Sense::hover() });
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, exists, &name));
        if response.hovered() && exists {
            ui.painter().rect_filled(rect, 0.0, t.accent_soft);
        }
        let mut p = ui.painter().clone();
        if !exists {
            p.set_opacity(0.45);
        }
        icons::paint(&p, Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(18.0, 18.0)), Icon::Layers, t.accent, BRAND);
        p.text(pos2(rect.left() + 46.0, rect.center().y - 8.0), Align2::LEFT_CENTER, &name, FontId::proportional(14.0), t.text);
        p.text(pos2(rect.left() + 46.0, rect.center().y + 9.0), Align2::LEFT_CENTER, &folder, FontId::proportional(11.5), t.muted);
        let when = changed.unwrap_or_else(|| "not found".into());
        p.text(pos2(rect.right() - 14.0, rect.center().y), Align2::RIGHT_CENTER, when, FontId::proportional(12.0), t.muted);
        if response.clicked() && exists {
            action = Some(Action::Open(path.clone()));
        }
    }
    action
}

/// When the file last changed, in words; `None` when it is gone. The web build has no clock and
/// no files: it says nothing.
fn changed_ago(path: &Path) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        #[allow(clippy::disallowed_methods)] // native only: the clock is there
        let age = std::time::SystemTime::now().duration_since(modified).map_or(0, |d| d.as_secs());
        Some(ago(age))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ago_in_words() {
        assert_eq!(super::ago(5), "just now");
        assert_eq!(super::ago(125), "2 min ago");
        assert_eq!(super::ago(7200), "2 h ago");
        assert_eq!(super::ago(90_000), "yesterday");
        assert_eq!(super::ago(3 * 86_400), "3 days ago");
    }
}
