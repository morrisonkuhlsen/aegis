//! The splash screen: a small window of its own, without decorations, before the main one, as
//! desktop suites have (the same as MK Sigma's). In two halves: on Morrison Kühlsen's ink,
//! MK aeGIS's mark, name, motto and version, a bar that fills while it shows with the program's
//! components named in turn, and the house's logo; beside them, Athena as artwork. It closes by
//! itself after a moment; a click or Esc closes it sooner.

use egui::{Align2, Color32, FontId, Rect, Sense, TextureHandle, TextureOptions, pos2, vec2};
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::icons::{self, Icon};

/// The window's size, in points: each half keeps the artwork's 680×800 proportions.
pub const SIZE: [f32; 2] = [816.0, 480.0];
/// The height the layout's measures are given for; they scale with the window's.
const LAYOUT_HEIGHT: f32 = 400.0;
/// Seconds it stays.
const HOLD: f64 = 5.0;
/// The artwork, the right half: see `ATTRIBUTION.md`.
const ARTWORK: &[u8] = include_bytes!("../assets/athena.jpg");
/// The Morrison Kühlsen wordmark, white on clear (a brand asset: `docs/brand/`).
const MORRISON_KUHLSEN: &[u8] = include_bytes!("../../../docs/brand/morrison-kuhlsen.png");
/// The largest image side decoded, against a corrupt header.
const MAX_SIDE: usize = 4096;
/// Morrison Kühlsen's ink, and the text on it (the File backstage's column too).
pub(crate) const INK: Color32 = Color32::from_rgb(0x14, 0x16, 0x1C);
pub(crate) const ON_INK: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub(crate) const ON_INK_SOFT: Color32 = Color32::from_rgba_premultiplied(184, 184, 184, 184);
/// MK aeGIS's colour: the coral of the artwork's clouds.
pub(crate) const BRAND: Color32 = Color32::from_rgb(0xE8, 0x5A, 0x5F);
pub(crate) const NAME: &str = "MK aeGIS";
pub(crate) const MOTTO: &str = "Ex nihilo, ad æternum";
pub(crate) const TAGLINE: &str = "Spatial statistics, open and auditable: every map with its method.";
pub(crate) const HOUSE: &str = "A Morrison Kühlsen project";

/// The program's components, named in turn as the bar fills.
const COMPONENTS: [&str; 12] = [
    "coordinate systems and geodesics",
    "the shapefile and dBASE reader",
    "the GeoJSON reader and writer",
    "the SQL query engine",
    "the map renderer",
    "symbology and classification",
    "geoprocessing tools",
    "spatial weights",
    "spatial statistics",
    "the attribute table",
    "the command registry and history",
    "the control channel and MCP",
];

#[derive(Debug, Default)]
pub struct SplashScreen {
    /// When it first showed, in egui's time.
    started: Option<f64>,
    closed: bool,
}

impl SplashScreen {
    /// Whether it asked its window to close.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Draws it over the whole window; asks the window to close when its time is up.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let started = *self.started.get_or_insert(now);
        let progress = ((now - started) / HOLD).clamp(0.0, 1.0) as f32;
        // The whole window: the root Ui comes inset by a margin.
        let rect = ctx.content_rect();
        let response = ui.allocate_rect(ui.max_rect(), Sense::click());
        paint(&ui.painter().with_clip_rect(rect), &ctx, rect, progress);
        let done = progress >= 1.0 || response.clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if done && !self.closed {
            self.closed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

fn paint(painter: &egui::Painter, ctx: &egui::Context, rect: Rect, progress: f32) {
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    // The layout is drawn for 400 points of height and scales with the window.
    let k = rect.height() / LAYOUT_HEIGHT;
    // Two halves: the words on Morrison Kühlsen's ink, the artwork beside them.
    let half = rect.left() + rect.width() / 2.0;
    let words = Rect::from_min_max(rect.min, pos2(half, rect.bottom()));
    let art = Rect::from_min_max(pos2(half, rect.top()), rect.max);
    painter.rect_filled(rect, 0.0, INK);
    if let Some(texture) = cached(ctx, "splash-athena", ARTWORK, decode_jpeg) {
        painter.image(texture.id(), art, uv, Color32::WHITE);
    }

    let left = words.left() + 36.0 * k;
    let width = words.width() - 72.0 * k;
    // The mark and the name.
    let mark = Rect::from_min_size(pos2(left, words.top() + 52.0 * k), vec2(46.0, 46.0) * k);
    icons::paint(painter, mark, Icon::Logo, BRAND, BRAND);
    painter.text(pos2(left + 58.0 * k, words.top() + 75.0 * k), Align2::LEFT_CENTER, NAME, FontId::proportional(34.0 * k), ON_INK);
    painter.text(pos2(left, words.top() + 128.0 * k), Align2::LEFT_CENTER, MOTTO, FontId::proportional(15.0 * k), ON_INK_SOFT);
    let tagline = painter.layout(TAGLINE.to_string(), FontId::proportional(12.5 * k), ON_INK_SOFT, width);
    let below = words.top() + 152.0 * k + tagline.size().y;
    painter.galley(pos2(left, words.top() + 152.0 * k), tagline, ON_INK_SOFT);
    painter.text(
        pos2(left, below + 10.0 * k),
        Align2::LEFT_TOP,
        format!("Version {}", env!("CARGO_PKG_VERSION")),
        FontId::proportional(12.5 * k),
        ON_INK_SOFT,
    );

    // The component loading, over the progress bar.
    let bar = Rect::from_min_size(pos2(left, words.bottom() - 96.0 * k), vec2(width, 3.0 * k));
    let shown = ((progress * COMPONENTS.len() as f32) as usize).min(COMPONENTS.len() - 1);
    let status = COMPONENTS.get(shown).map_or_else(String::new, |c| format!("Loading {c}…"));
    painter.text(bar.left_top() - vec2(0.0, 10.0 * k), Align2::LEFT_BOTTOM, status, FontId::proportional(12.0 * k), ON_INK_SOFT);
    painter.rect_filled(bar, 0.0, Color32::from_white_alpha(28));
    painter.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * progress, bar.height())), 0.0, BRAND);

    // The house: its logo and line at the foot.
    let foot = words.bottom() - 28.0 * k;
    if let Some(texture) = cached(ctx, "splash-morrison-kuhlsen", MORRISON_KUHLSEN, decode_png) {
        let logo_width = 104.0 * k;
        let size = vec2(logo_width, logo_width * texture.size_vec2().y / texture.size_vec2().x.max(1.0));
        painter.image(texture.id(), Rect::from_min_size(pos2(left, foot - size.y), size), uv, ON_INK);
    }
    painter.text(pos2(left + width, foot - 4.0 * k), Align2::RIGHT_BOTTOM, HOUSE, FontId::proportional(11.0 * k), ON_INK_SOFT);
}

/// Athena, the splash's artwork, as a texture (the File backstage shows it too).
pub(crate) fn artwork(ctx: &egui::Context) -> Option<TextureHandle> {
    cached(ctx, "splash-athena", ARTWORK, decode_jpeg)
}

/// The Morrison Kühlsen wordmark, white on clear, as a texture.
pub(crate) fn wordmark(ctx: &egui::Context) -> Option<TextureHandle> {
    cached(ctx, "splash-morrison-kuhlsen", MORRISON_KUHLSEN, decode_png)
}

/// A decoded image as a texture, loaded once per context.
fn cached(ctx: &egui::Context, name: &str, bytes: &[u8], decode: fn(&[u8]) -> Result<egui::ColorImage, String>) -> Option<TextureHandle> {
    let id = egui::Id::new(name);
    if let Some(texture) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(texture);
    }
    let image = decode(bytes).map_err(|e| log::warn!("{name}: {e}")).ok()?;
    let texture = ctx.load_texture(name, image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

/// A JPEG as an egui image (RGBA), refusing absurd sizes.
fn decode_jpeg(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA).set_max_width(MAX_SIDE).set_max_height(MAX_SIDE);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let pixels = decoder.decode().map_err(|e| format!("{e:?}"))?;
    let (w, h) = decoder.dimensions().ok_or("no dimensions")?;
    if pixels.len() != w.saturating_mul(h).saturating_mul(4) {
        return Err(format!("{} bytes for {w}×{h}", pixels.len()));
    }
    Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels))
}

/// A PNG as an egui image (RGBA), refusing absurd sizes.
fn decode_png(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let (w, h) = (reader.info().width as usize, reader.info().height as usize);
    if w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("{w}×{h} is too large"));
    }
    let mut pixels = vec![0; reader.output_buffer_size().ok_or("no buffer size")?];
    let frame = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    pixels.truncate(frame.buffer_size());
    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => pixels,
        png::ColorType::Rgb => pixels.as_chunks::<3>().0.iter().flat_map(|[r, g, b]| [*r, *g, *b, 255]).collect(),
        png::ColorType::GrayscaleAlpha => pixels.as_chunks::<2>().0.iter().flat_map(|[g, a]| [*g, *g, *g, *a]).collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::Indexed => return Err("indexed colour".into()),
    };
    if rgba.len() != w.saturating_mul(h).saturating_mul(4) {
        return Err(format!("{} bytes for {w}×{h}", rgba.len()));
    }
    Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_images_decode() {
        assert_eq!(decode_jpeg(ARTWORK).unwrap().size, [680, 800]);
        let logo = decode_png(MORRISON_KUHLSEN).unwrap();
        assert!(logo.size[0] > logo.size[1] * 2, "a wide wordmark");
    }

    #[test]
    fn hostile_images_never_panic() {
        assert!(decode_jpeg(&[]).is_err() && decode_png(&[]).is_err());
        for bytes in [ARTWORK, MORRISON_KUHLSEN] {
            for cut in [2, 60, bytes.len() / 2] {
                let _ = decode_jpeg(bytes.get(..cut).unwrap());
                let _ = decode_png(bytes.get(..cut).unwrap());
            }
            let mut flipped = bytes.to_vec();
            for i in (0..flipped.len()).step_by(997) {
                if let Some(b) = flipped.get_mut(i) {
                    *b ^= 0xff;
                }
                let _ = decode_jpeg(&flipped);
                let _ = decode_png(&flipped);
            }
        }
    }

    #[test]
    fn it_closes_on_esc_and_after_its_time() {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(SIZE[0], SIZE[1]))
            .build_ui_state(|ui, s: &mut SplashScreen| s.ui(ui), SplashScreen::default());
        harness.run_steps(2);
        assert!(!harness.state().closed());
        harness.key_press(egui::Key::Escape);
        harness.run_steps(1);
        assert!(harness.state().closed());
        let mut waiting = egui_kittest::Harness::builder()
            .with_size(egui::vec2(SIZE[0], SIZE[1]))
            .with_step_dt(1.0)
            .build_ui_state(|ui, s: &mut SplashScreen| s.ui(ui), SplashScreen::default());
        waiting.run_steps(7);
        assert!(waiting.state().closed(), "gone after {HOLD} s");
    }
}
