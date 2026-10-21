//! The splash screen: the start-up artwork while the app loads its components, one step per
//! frame, with the step's name and a progress bar. Clicking or Esc skips the minimum display
//! time (the steps still all run). Without its artwork (a failed decode) it paints a gradient.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2, pos2, vec2};
use mkaegis_engine::Session;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::{MapApp, theme};

/// The artwork: see `ATTRIBUTION.md`.
const ARTWORK: &[u8] = include_bytes!("../assets/splash.jpg");
/// The largest artwork side decoded, against a corrupt header.
const MAX_SIDE: usize = 4096;
/// How long the splash stays at least, in seconds, so it reads as a screen and not a flicker.
const MIN_SECONDS: f64 = 1.6;
const SKY: Color32 = Color32::from_rgb(0x0f, 0x2c, 0x3a);
const CLOUD: Color32 = Color32::from_rgb(0xe8, 0x5a, 0x5f);
const BAR: Color32 = Color32::from_rgb(0xf4, 0x8a, 0x7e);

/// One loading step: what it is called on screen, and the work.
pub struct Step {
    pub label: String,
    pub run: Box<dyn FnOnce(&mut MapApp, &egui::Context) -> Result<(), String>>,
}

/// The splash's progress.
pub struct Splash {
    steps: std::collections::VecDeque<Step>,
    total: usize,
    current: String,
    started: Option<f64>,
    skip: bool,
    /// Stays on screen after loading (offscreen snapshots of the splash).
    hold: bool,
    artwork: Option<TextureHandle>,
    decoded: bool,
    problems: Vec<String>,
}

impl Step {
    pub fn new(label: impl Into<String>, run: impl FnOnce(&mut MapApp, &egui::Context) -> Result<(), String> + 'static) -> Self {
        Step { label: label.into(), run: Box::new(run) }
    }
}

impl Splash {
    /// The app's own components first, then `steps` (opening files, the control channel…).
    pub fn new(steps: Vec<Step>) -> Self {
        let mut all: std::collections::VecDeque<Step> = [
            Step::new("Theme and icons", |_, ctx| {
                theme::apply(ctx);
                Ok(())
            }),
            Step::new("Command registry", |_, _| if Session::commands().is_empty() { Err("no commands registered".into()) } else { Ok(()) }),
            Step::new("Coordinate systems", |_, _| {
                let known = mkaegis_crs::Crs::known();
                let p = mkaegis_geom::Coord::new(-46.63, -23.55);
                for crs in &known {
                    // Builds each projection once; a point it can't take is fine here.
                    let _ = crs.from_geographic(p);
                }
                if known.is_empty() { Err("no coordinate systems".into()) } else { Ok(()) }
            }),
        ]
        .into();
        all.extend(steps);
        all.push_back(Step::new("Map view", |app, _| {
            app.fix_active();
            Ok(())
        }));
        let total = all.len();
        Splash {
            steps: all,
            total,
            current: String::new(),
            started: None,
            skip: false,
            hold: false,
            artwork: None,
            decoded: false,
            problems: Vec::new(),
        }
    }

    /// Keeps the splash on screen once loaded, for snapshots.
    pub fn held(mut self) -> Self {
        self.hold = true;
        self
    }

    /// Steps not run yet.
    pub fn remaining(&self) -> usize {
        self.steps.len()
    }

    /// Runs the next step (for loading without the splash on screen).
    pub fn run_next(&mut self, app: &mut MapApp, ctx: &egui::Context) -> Result<(), String> {
        match self.steps.pop_front() {
            Some(step) => {
                let label = step.label.clone();
                (step.run)(app, ctx).map_err(|e| format!("{label}: {e}"))
            }
            None => Ok(()),
        }
    }
}

/// Runs the next step and paints the splash; returns whether it is finished (and takes it off).
pub fn show(app: &mut MapApp, ui: &mut egui::Ui) -> bool {
    let Some(mut splash) = app.splash.take() else { return true };
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let started = *splash.started.get_or_insert(now);
    if let Some(label) = splash.steps.front().map(|s| s.label.clone()) {
        splash.current = label;
        if let Err(e) = splash.run_next(app, &ctx) {
            splash.problems.push(e);
        }
    }
    paint(&mut splash, ui, now - started);
    let finished = splash.steps.is_empty() && !splash.hold && (splash.skip || now - started >= MIN_SECONDS);
    if finished {
        if let Some(p) = splash.problems.first() {
            app.status = Some((p.clone(), true));
        }
    } else {
        if !splash.hold {
            ctx.request_repaint();
        }
        app.splash = Some(splash);
    }
    finished
}

fn paint(splash: &mut Splash, ui: &mut egui::Ui, elapsed: f64) {
    let rect = ui.max_rect();
    let response = ui.interact(rect, ui.id().with("splash"), Sense::click_and_drag());
    if response.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if response.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        splash.skip = true;
    }
    let painter = ui.painter_at(rect);
    if !splash.decoded {
        splash.decoded = true;
        match decode(ARTWORK) {
            Ok(image) => splash.artwork = Some(ui.ctx().load_texture("splash", image, TextureOptions::LINEAR)),
            Err(e) => log::warn!("splash artwork: {e}"),
        }
    }
    match &splash.artwork {
        Some(tex) => {
            let uv = cover(tex.size_vec2(), rect.size());
            painter.image(tex.id(), rect, uv, Color32::WHITE);
        }
        None => gradient(&painter, rect),
    }
    // A shade under the text, darker towards the bottom.
    let shade = Rect::from_min_max(pos2(rect.left(), rect.bottom() - rect.height() * 0.42), rect.max);
    let mut mesh = egui::Mesh::default();
    let clear = Color32::from_black_alpha(0);
    let dark = Color32::from_black_alpha(200);
    let base = mesh.vertices.len() as u32;
    for (p, c) in [(shade.left_top(), clear), (shade.right_top(), clear), (shade.right_bottom(), dark), (shade.left_bottom(), dark)] {
        mesh.colored_vertex(p, c);
    }
    mesh.add_triangle(base, base + 1, base + 2);
    mesh.add_triangle(base, base + 2, base + 3);
    painter.add(mesh);

    let margin = (rect.width() * 0.05).clamp(24.0, 64.0);
    let left = rect.left() + margin;
    let bottom = rect.bottom() - margin;
    let white = Color32::from_rgb(0xfa, 0xf6, 0xf2);
    let soft = Color32::from_rgba_unmultiplied(0xfa, 0xf6, 0xf2, 0xb0);
    painter.text(pos2(left, bottom - 104.0), Align2::LEFT_BOTTOM, "mkaeGIS", FontId::proportional(46.0), white);
    painter.text(
        pos2(left + 2.0, bottom - 80.0),
        Align2::LEFT_BOTTOM,
        format!("Statistical GIS · version {}", env!("CARGO_PKG_VERSION")),
        FontId::proportional(15.0),
        soft,
    );

    let done = splash.total.saturating_sub(splash.steps.len());
    let fraction = if splash.total == 0 { 1.0 } else { done as f32 / splash.total as f32 };
    // The bar fills with the steps but never faster than the minimum time.
    let fraction = fraction.min((elapsed / MIN_SECONDS) as f32 + 0.15).clamp(0.0, 1.0);
    let width = (rect.width() - 2.0 * margin).min(520.0);
    let track = Rect::from_min_size(pos2(left, bottom - 36.0), vec2(width, 4.0));
    painter.rect_filled(track, 2.0, Color32::from_white_alpha(40));
    painter.rect_filled(Rect::from_min_size(track.min, vec2(width * fraction, 4.0)), 2.0, BAR);
    painter.text(
        pos2(left, bottom - 10.0),
        Align2::LEFT_BOTTOM,
        format!("Loading {}…", splash.current.to_lowercase()),
        FontId::proportional(13.0),
        soft,
    );
    painter.text(
        pos2(rect.right() - margin, bottom - 10.0),
        Align2::RIGHT_BOTTOM,
        "Open source · MIT OR Apache-2.0",
        FontId::proportional(12.0),
        Color32::from_rgba_unmultiplied(0xfa, 0xf6, 0xf2, 0x80),
    );
}

/// The artwork as an egui image (RGBA), refusing absurd sizes.
fn decode(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA).set_max_width(MAX_SIDE).set_max_height(MAX_SIDE);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let pixels = decoder.decode().map_err(|e| format!("{e:?}"))?;
    let (w, h) = decoder.dimensions().ok_or("no dimensions")?;
    if pixels.len() != w.saturating_mul(h).saturating_mul(4) {
        return Err(format!("{} bytes for {w}×{h}", pixels.len()));
    }
    Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels))
}

/// The texture coordinates that fill `area` with an image of `size`, cropping its overflow.
fn cover(size: Vec2, area: Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 || area.x <= 0.0 || area.y <= 0.0 {
        return Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    }
    let (image, view) = (size.x / size.y, area.x / area.y);
    if view > image {
        let h = image / view;
        Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
    } else {
        let w = view / image;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    }
}

/// The fallback background: the artwork's sky over its clouds.
fn gradient(painter: &egui::Painter, rect: Rect) {
    let mut mesh = egui::Mesh::default();
    for (p, c) in [(rect.left_top(), SKY), (rect.right_top(), SKY), (rect.right_bottom(), CLOUD), (rect.left_bottom(), CLOUD)] {
        mesh.colored_vertex(p, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_splash_runs_every_step_then_gives_way_to_the_map() {
        let mut app = MapApp::new(Session::default());
        app.splash = Some(Splash::new(vec![
            Step::new("Sample map", |app, _| {
                app.session = Session::sample();
                Ok(())
            }),
            Step::new("broken.shp", |_, _| Err("no such file".into())),
        ]));
        let mut harness =
            egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_ui_state(|ui, app: &mut MapApp| app.ui(ui), app);
        harness.key_press(egui::Key::Escape);
        harness.run_steps(4);
        let app = harness.state();
        assert!(app.splash.is_none(), "loaded and skipped");
        assert_eq!(app.session.map.layers.len(), 4);
        assert!(app.active.is_some());
        let (message, error) = app.status.clone().unwrap();
        assert!(error && message.contains("broken.shp: no such file"), "{message}");
    }

    #[test]
    fn the_artwork_decodes() {
        let image = decode(ARTWORK).unwrap();
        assert_eq!(image.size, [1280, 720]);
    }

    #[test]
    fn hostile_artwork_never_panics() {
        assert!(decode(&[]).is_err());
        for cut in [2, 100, ARTWORK.len() / 2] {
            let _ = decode(ARTWORK.get(..cut).unwrap());
        }
        let mut flipped = ARTWORK.to_vec();
        for i in (0..flipped.len()).step_by(997) {
            if let Some(b) = flipped.get_mut(i) {
                *b ^= 0xff;
            }
            let _ = decode(&flipped);
        }
    }

    #[test]
    fn cover_crops_the_overflow() {
        let uv = cover(vec2(1280.0, 720.0), vec2(800.0, 800.0));
        assert!((uv.width() - 0.5625).abs() < 1e-4);
        assert!((uv.height() - 1.0).abs() < 1e-6);
        let uv = cover(vec2(1280.0, 720.0), vec2(1600.0, 400.0));
        assert!((uv.width() - 1.0).abs() < 1e-6);
        assert_eq!(cover(Vec2::ZERO, vec2(10.0, 10.0)), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
    }
}
