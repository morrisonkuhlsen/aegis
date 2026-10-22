//! The map view. The map is rasterised by `mkaegis-render` (the same pixels Export Map writes)
//! into a texture, redrawn only when the map or the view changes; labels are drawn on top with
//! egui's text. Drag pans (Explore) or draws a selection box (Select); the wheel zooms about
//! the pointer; middle-drag always pans.

use egui::{Color32, Pos2, Rect, Sense, Ui, Vec2};
use mkaegis_geom::{Coord, Envelope};
use mkaegis_render::{Options, Shape, Viewport};
use serde_json::json;

use crate::i18n::tr;
use crate::theme::Tokens;
use crate::{MapApp, Tool};

/// What was last drawn, so a frame with nothing new reuses the texture.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Key {
    revision: u64,
    /// The basemap tiles' revision: a tile arriving redraws.
    tiles: u64,
    center: (f64, f64),
    resolution: f64,
    size: (usize, usize),
}

#[derive(Default)]
pub struct State {
    vp: Option<Viewport>,
    serial: u64,
    texture: Option<egui::TextureHandle>,
    key: Option<Key>,
    labels: Vec<Shape>,
    /// Pixels the image has been dragged since it was drawn.
    offset: Vec2,
    box_start: Option<Pos2>,
    /// The map position under the pointer.
    pub pointer: Option<Coord>,
}

impl State {
    pub fn viewport(&self) -> Option<Viewport> {
        self.vp
    }
}

fn default_extent(app: &MapApp) -> Envelope {
    app.session.map.extent.or_else(|| app.session.map.full_extent()).unwrap_or_else(|| {
        // The world, in the map's CRS.
        let world = Envelope::new(Coord::new(-170.0, -60.0), Coord::new(170.0, 75.0));
        mkaegis_model::project_envelope(world, mkaegis_crs::Crs::WGS84, app.session.map.crs).unwrap_or(world)
    })
}

pub fn show(app: &mut MapApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();
    let (w, h) = ((rect.width() * ppp).round().max(1.0), (rect.height() * ppp).round().max(1.0));

    // The view: refit when a command moved it, keep the centre and resolution on resize.
    let serial = app.session.view_serial;
    let mut vp = match app.view.vp {
        Some(v) if app.view.serial == serial => Viewport { width: f64::from(w), height: f64::from(h), ..v },
        _ => Viewport::fit(default_extent(app), f64::from(w), f64::from(h)),
    };
    app.view.serial = serial;
    let to_px = |p: Pos2| [(p.x - rect.left()) * ppp, (p.y - rect.top()) * ppp];
    let mut moved = false;

    // Zoom with the wheel or a pinch, about the pointer.
    if let Some(hover) = response.hover_pos() {
        let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = if zoom != 1.0 { f64::from(zoom) } else { (f64::from(scroll) / 200.0).exp() };
        if (factor - 1.0).abs() > 1e-4 {
            vp.zoom(factor, to_px(hover));
            moved = true;
        }
        app.view.pointer = Some(vp.to_map(to_px(hover)));
    } else {
        app.view.pointer = None;
    }

    // Pan: drag with Explore, middle-drag with any tool.
    let middle = ui.input(|i| i.pointer.middle_down());
    let panning = response.dragged() && (app.tool == Tool::Explore || middle);
    if panning {
        app.view.offset += response.drag_delta();
    }
    if (response.drag_stopped() || (!response.dragged() && app.view.offset != Vec2::ZERO)) && app.view.offset != Vec2::ZERO {
        vp.pan(app.view.offset.x * ppp, app.view.offset.y * ppp);
        app.view.offset = Vec2::ZERO;
        moved = true;
    }

    // Select: a box, or a click.
    if app.tool == Tool::Select && !middle {
        if response.drag_started() {
            app.view.box_start = response.interact_pointer_pos();
        }
        if response.drag_stopped()
            && let (Some(a), Some(b)) = (app.view.box_start.take(), response.interact_pointer_pos())
        {
            let (p, q) = (vp.to_map(to_px(a)), vp.to_map(to_px(b)));
            let mode = selection_mode(ui);
            app.run("map.selectByRectangle", json!({ "xmin": p.x, "ymin": p.y, "xmax": q.x, "ymax": q.y, "mode": mode }));
        }
    }
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let hit = mkaegis_render::hit_test(&app.session.map, &vp, to_px(pos), 4.0 * f64::from(ppp));
        match app.tool {
            Tool::Explore => {
                app.identify = hit.and_then(|(li, row)| app.session.map.layers.get(li).map(|l| (l.id, row)));
            }
            Tool::Select => match hit.and_then(|(li, row)| app.session.map.layers.get(li).map(|l| (l.id, row))) {
                Some((id, row)) => {
                    app.run("feature.select", json!({ "layer": id, "rows": [row], "mode": selection_mode(ui) }));
                }
                None if selection_mode(ui) == "new" => {
                    app.run("map.clearSelection", json!({}));
                }
                None => {}
            },
        }
    }
    if moved {
        let e = vp.extent();
        // Keep the map's extent in step, so a saved project opens where it was left.
        let _ = app.session.execute("map.setExtent", &json!({ "xmin": e.min_x, "ymin": e.min_y, "xmax": e.max_x, "ymax": e.max_y }));
    }
    app.view.vp = Some(vp);

    fetch_tiles(app, ui.ctx(), &vp);

    // Redraw the texture when something changed.
    let key = Key {
        revision: app.session.revision,
        tiles: app.session.tiles.revision(),
        center: (vp.center.x, vp.center.y),
        resolution: vp.resolution,
        size: (w as usize, h as usize),
    };
    if app.view.key != Some(key) || app.view.texture.is_none() {
        let shapes = mkaegis_render::display_list(&app.session.map, &vp, &Options { pixels_per_point: ppp, ..Options::default() });
        if let Ok(mut canvas) = mkaegis_render::raster::Canvas::new(w as u32, h as u32, app.session.map.background) {
            mkaegis_render::basemap::draw(&mut canvas, &app.session.map, &vp, &app.session.tiles);
            app.view.labels.clear();
            for s in shapes {
                if matches!(s, Shape::Label { .. }) {
                    app.view.labels.push(s);
                } else {
                    canvas.draw(&s);
                }
            }
            let image = egui::ColorImage::from_rgba_unmultiplied([canvas.width(), canvas.height()], canvas.pixels());
            match &mut app.view.texture {
                Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                None => app.view.texture = Some(ui.ctx().load_texture("map", image, egui::TextureOptions::LINEAR)),
            }
            app.view.key = Some(key);
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, crate::theme::color(app.session.map.background));
    if let Some(tex) = &app.view.texture {
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(tex.id(), rect.translate(app.view.offset), uv, Color32::WHITE);
    }
    for l in &app.view.labels {
        if let Shape::Label { at, text, color, size } = l {
            let pos = egui::pos2(rect.left() + at[0] / ppp, rect.top() + at[1] / ppp) + app.view.offset;
            let font = egui::FontId::proportional(size / ppp * 1.25);
            for d in [egui::vec2(-1.0, 0.0), egui::vec2(1.0, 0.0), egui::vec2(0.0, -1.0), egui::vec2(0.0, 1.0)] {
                painter.text(pos + d, egui::Align2::LEFT_CENTER, text, font.clone(), t.label_halo);
            }
            painter.text(pos, egui::Align2::LEFT_CENTER, text, font, crate::theme::color(*color));
        }
    }
    if let (Some(a), Some(b)) = (app.view.box_start, response.hover_pos().or(response.interact_pointer_pos()))
        && app.tool == Tool::Select
        && response.dragged()
    {
        let r = Rect::from_two_pos(a, b);
        painter.rect_filled(r, 0.0, t.accent.gamma_multiply(0.12));
        painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Middle);
    }
    scale_bar(app, &painter, rect, &vp, ppp);
    attribution(app, ui, rect);
    if app.session.map.layers.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr("Add data to the map: Map ▸ Add Data, or Project ▸ Add Sample Data"),
            egui::FontId::proportional(14.0),
            t.muted,
        );
    }
    if let Some((layer, row)) = app.identify {
        identify_popup(app, ui, layer, row);
    }
    let cursor = match app.tool {
        Tool::Explore if response.dragged() => egui::CursorIcon::Grabbing,
        Tool::Explore => egui::CursorIcon::Grab,
        Tool::Select => egui::CursorIcon::Crosshair,
    };
    if response.hovered() {
        ui.ctx().set_cursor_icon(cursor);
    }
}

/// Shift adds, Ctrl removes, as ArcGIS's interactive selection does.
fn selection_mode(ui: &Ui) -> &'static str {
    let m = ui.input(|i| i.modifiers);
    if m.shift {
        "add"
    } else if m.command {
        "remove"
    } else {
        "new"
    }
}

/// A scale bar of a round length, bottom left.
/// Asks for the basemap tiles the view needs and nobody has fetched yet; each, once it arrives,
/// redraws the view. Only what is on screen is fetched, as OpenStreetMap's tile policy asks.
fn fetch_tiles(app: &MapApp, ctx: &egui::Context, vp: &Viewport) {
    let Some(fetch) = &app.services.fetch else { return };
    let wanted = mkaegis_render::basemap::wanted(&app.session.map, vp);
    for key in app.session.tiles.claim(&wanted) {
        let (store, ctx) = (app.session.tiles.clone(), ctx.clone());
        fetch(
            mkaegis_render::basemap::url(key),
            Box::new(move |result| {
                if let Err(e) = result.map_err(mkaegis_core::Error::Io).and_then(|bytes| store.arrive(key, &bytes)) {
                    log::warn!("basemap tile {}/{}/{}: {e}", key.z, key.x, key.y);
                    store.fail(key);
                }
                ctx.request_repaint();
            }),
        );
    }
}

/// The basemap's attribution in the map's bottom-right corner, linking to its licence: shown
/// whenever the basemap is, as its licence asks.
fn attribution(app: &MapApp, ui: &mut Ui, rect: Rect) {
    let Some(basemap) = app.session.map.basemap else { return };
    let source = mkaegis_render::basemap::source(basemap);
    let (text, url) = if app.session.map.crs == mkaegis_crs::Crs::WEB_MERCATOR {
        (source.attribution, Some(source.attribution_url))
    } else {
        (tr("Basemaps are drawn in Web Mercator maps."), None)
    };
    let font = egui::FontId::proportional(11.0);
    let width = ui.painter().layout_no_wrap(text.to_string(), font.clone(), Color32::BLACK).size().x + 10.0;
    let r = Rect::from_min_size(egui::pos2(rect.right() - width - 4.0, rect.bottom() - 22.0), egui::vec2(width, 18.0));
    ui.painter().rect_filled(r, 2.0, Color32::from_white_alpha(210));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(egui::vec2(5.0, 1.0))));
    let shown = egui::RichText::new(text).font(font);
    match url {
        Some(url) => {
            child.hyperlink_to(shown.color(Color32::from_rgb(40, 60, 110)), url);
        }
        None => {
            child.add(egui::Label::new(shown.color(Color32::from_gray(60))));
        }
    }
}

fn scale_bar(app: &MapApp, painter: &egui::Painter, rect: Rect, vp: &Viewport, ppp: f32) {
    let crs = app.session.map.crs;
    let lat = crs.to_geographic(vp.center).map_or(0.0, |g| g.y);
    let metres_per_px = vp.resolution * crs.metres_per_unit(lat) * f64::from(ppp);
    if !(metres_per_px.is_finite() && metres_per_px > 0.0) {
        return;
    }
    let target = metres_per_px * 120.0;
    let pow = 10f64.powf(target.log10().floor());
    let nice = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * pow).filter(|v| *v <= target).fold(pow, f64::max);
    let len = (nice / metres_per_px) as f32;
    let label = if nice >= 1000.0 { format!("{} km", nice / 1000.0) } else { format!("{nice} m") };
    let base = egui::pos2(rect.left() + 16.0, rect.bottom() - 18.0);
    let stroke = egui::Stroke::new(2.0, Color32::from_rgb(0x30, 0x30, 0x30));
    let bg = Rect::from_min_max(base - egui::vec2(6.0, 18.0), base + egui::vec2(len + 50.0, 8.0));
    painter.rect_filled(bg, 3.0, Color32::from_rgba_unmultiplied(255, 255, 255, 200));
    painter.line_segment([base, base + egui::vec2(len, 0.0)], stroke);
    for x in [0.0, len / 2.0, len] {
        painter.line_segment([base + egui::vec2(x, -5.0), base + egui::vec2(x, 0.0)], stroke);
    }
    painter.text(
        base + egui::vec2(len + 6.0, -2.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.0),
        Color32::from_rgb(0x30, 0x30, 0x30),
    );
}

/// The Explore pop-up: the identified feature's attributes.
fn identify_popup(app: &mut MapApp, ui: &Ui, layer: u32, row: usize) {
    let Some(l) = app.session.map.layers.iter().find(|l| l.id == layer) else { return };
    let mut open = true;
    let title = format!("{} — {}", l.name, row + 1);
    let mut select = false;
    egui::Window::new(title)
        .id(egui::Id::new("identify"))
        .open(&mut open)
        .resizable(true)
        .default_width(280.0)
        .default_pos(ui.max_rect().right_top() + egui::vec2(-300.0, 20.0))
        .show(ui.ctx(), |ui| {
            egui::Grid::new("identify-grid").striped(true).num_columns(2).show(ui, |ui| {
                if let Some(f) = l.data.features.get(row) {
                    for (field, v) in l.data.fields.iter().zip(&f.values) {
                        ui.label(egui::RichText::new(&field.name).strong());
                        ui.label(crate::i18n::value_text(v));
                        ui.end_row();
                    }
                    if let Some(g) = &f.geometry {
                        ui.label(egui::RichText::new(tr("Shape")).strong());
                        ui.label(crate::trf(
                            "{shape} ({count} vertices)",
                            &[("shape", &crate::i18n::geometry_name(g.geometry_type())), ("count", &g.vertex_count())],
                        ));
                        ui.end_row();
                    }
                }
            });
            ui.separator();
            select = ui.button(tr("Select this feature")).clicked();
        });
    if select {
        app.run("feature.select", json!({ "layer": layer, "rows": [row] }));
    }
    if !open {
        app.identify = None;
    }
}
