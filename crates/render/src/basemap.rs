//! Web basemaps (#15): where their tiles come from, which ones a view needs, a store of the tiles
//! fetched so far, and drawing them under the layers. There is no network here: the app fetches
//! what [`TileStore::claim`] hands it and puts the decoded tiles in the store; the view and Export
//! Map draw from the store, with a lower zoom's tile stretched where one hasn't arrived.
//!
//! Tiles are Web Mercator (EPSG:3857) XYZ tiles of 256 pixels, so a basemap is drawn only on a map
//! in Web Mercator. The sources are open data: OpenStreetMap (ODbL; its tile policy asks for
//! attribution on the map and on exported images, and forbids fetching tiles nobody is looking at)
//! and NASA GIBS imagery (CC0).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use mkaegis_core::{Error, Result};
use mkaegis_crs::Crs;
use mkaegis_geom::Envelope;
use mkaegis_model::{Basemap, Map};

use crate::Viewport;
use crate::raster::Canvas;

/// Half the width of the Web Mercator world, in metres (π × 6 378 137).
const HALF_WORLD: f64 = 20_037_508.342_789_244;
/// A tile's side, in pixels.
pub const TILE_SIZE: usize = 256;
/// The most tiles one view asks for: a larger view takes a lower zoom.
const MAX_TILES: usize = 96;
/// How many zoom levels up a missing tile is looked for, to stretch in its place.
const FALLBACK_LEVELS: u8 = 8;
/// The most tiles being fetched at once: a view asks for more as these arrive.
const MAX_PENDING: usize = 12;
/// The most tiles held in memory (256 KB each).
const MAX_STORED: usize = 1024;
/// NASA GIBS' Web Mercator endpoint for its "best available" imagery.
const GIBS: &str = "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best";

/// One tile of one basemap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub basemap: Basemap,
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

/// What a basemap is and what it must say about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    /// The attribution shown on the map, linking to `attribution_url`.
    pub attribution: &'static str,
    /// The attribution written into exported images (ASCII: the export font has no ©).
    pub export_attribution: &'static str,
    pub attribution_url: &'static str,
    /// The highest zoom the source serves; beyond it tiles are stretched.
    pub max_zoom: u8,
}

/// The tiles fetched so far, shared by the view, Export Map and whoever fetches them. Cheap to
/// clone: clones share one store.
#[derive(Clone, Default)]
pub struct TileStore(Arc<Mutex<Inner>>);

#[derive(Default)]
struct Inner {
    tiles: HashMap<TileKey, Arc<Vec<u8>>>,
    /// Oldest first, for evicting past [`MAX_STORED`].
    order: VecDeque<TileKey>,
    pending: HashSet<TileKey>,
    failed: HashSet<TileKey>,
    /// Bumped whenever a tile arrives: views redraw.
    revision: u64,
}

impl std::fmt::Debug for TileStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.lock();
        write!(f, "TileStore {{ {} tiles, {} pending }}", inner.tiles.len(), inner.pending.len())
    }
}

impl TileStore {
    /// The tile's RGBA pixels, `TILE_SIZE` × `TILE_SIZE`, if it has arrived.
    pub fn get(&self, key: TileKey) -> Option<Arc<Vec<u8>>> {
        self.lock().tiles.get(&key).cloned()
    }

    /// Of `keys`, those nobody has fetched or is fetching, now marked as being fetched, at most
    /// [`MAX_PENDING`] in flight: the caller fetches exactly these. A tile that failed is not asked
    /// for again.
    pub fn claim(&self, keys: &[TileKey]) -> Vec<TileKey> {
        let mut inner = self.lock();
        let room = MAX_PENDING.saturating_sub(inner.pending.len());
        let fresh: Vec<TileKey> = keys
            .iter()
            .copied()
            .filter(|k| !inner.tiles.contains_key(k) && !inner.pending.contains(k) && !inner.failed.contains(k))
            .take(room)
            .collect();
        inner.pending.extend(fresh.iter().copied());
        fresh
    }

    /// A fetched tile's bytes (PNG or JPEG), decoded and kept; an undecodable one counts as failed.
    pub fn arrive(&self, key: TileKey, bytes: &[u8]) -> Result<()> {
        match decode(bytes) {
            Ok(rgba) => {
                self.insert(key, rgba);
                Ok(())
            }
            Err(e) => {
                self.fail(key);
                Err(e)
            }
        }
    }

    /// Keeps a decoded tile, evicting the oldest past [`MAX_STORED`].
    pub fn insert(&self, key: TileKey, rgba: Vec<u8>) {
        let mut inner = self.lock();
        inner.pending.remove(&key);
        if inner.tiles.insert(key, Arc::new(rgba)).is_none() {
            inner.order.push_back(key);
        }
        while inner.order.len() > MAX_STORED {
            if let Some(old) = inner.order.pop_front() {
                inner.tiles.remove(&old);
            }
        }
        inner.revision = inner.revision.wrapping_add(1);
    }

    /// A tile that couldn't be fetched: not asked for again.
    pub fn fail(&self, key: TileKey) {
        let mut inner = self.lock();
        inner.pending.remove(&key);
        inner.failed.insert(key);
    }

    /// Changes whenever a tile arrives.
    pub fn revision(&self) -> u64 {
        self.lock().revision
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A basemap's source.
pub fn source(b: Basemap) -> Source {
    match b {
        Basemap::OpenStreetMap => Source {
            attribution: "© OpenStreetMap contributors",
            export_attribution: "(c) OpenStreetMap contributors, openstreetmap.org/copyright",
            attribution_url: "https://www.openstreetmap.org/copyright",
            max_zoom: 19,
        },
        Basemap::Satellite => Source {
            attribution: "Imagery: NASA GIBS (Blue Marble; Landsat WELD, c. 2000)",
            export_attribution: "Imagery: NASA GIBS (Blue Marble; Landsat WELD, c. 2000)",
            attribution_url: "https://www.earthdata.nasa.gov/engage/open-data-services-software-policies",
            max_zoom: 12,
        },
    }
}

/// Where a tile is fetched from.
pub fn url(key: TileKey) -> String {
    let TileKey { z, x, y, .. } = key;
    match key.basemap {
        Basemap::OpenStreetMap => format!("https://tile.openstreetmap.org/{z}/{x}/{y}.png"),
        // Blue Marble, a cloud-free world at 500 m, as far as it goes; then Landsat at 30 m, whose
        // latest global true-colour mosaic is of 2000.
        Basemap::Satellite if z <= 8 => format!("{GIBS}/BlueMarble_NextGeneration/default/default/GoogleMapsCompatible_Level8/{z}/{y}/{x}.jpg"),
        Basemap::Satellite => {
            format!(
                "{GIBS}/Landsat_WELD_CorrectedReflectance_TrueColor_Global_Annual/default/2000-12-01/GoogleMapsCompatible_Level12/{z}/{y}/{x}.jpg"
            )
        }
    }
}

/// The tiles a view of `map` needs, nearest its centre first: none without a basemap or for a
/// map in another coordinate system than Web Mercator.
pub fn wanted(map: &Map, vp: &Viewport) -> Vec<TileKey> {
    let Some(basemap) = map.basemap else { return Vec::new() };
    if map.crs != Crs::WEB_MERCATOR {
        return Vec::new();
    }
    let extent = vp.extent();
    let mut z = zoom_for(vp.resolution, source(basemap).max_zoom);
    let mut keys = tiles_in(extent, z, basemap);
    while keys.len() > MAX_TILES && z > 0 {
        z -= 1;
        keys = tiles_in(extent, z, basemap);
    }
    let centre = vp.center;
    keys.sort_by(|a, b| {
        let d = |k: &TileKey| bounds(*k).center().distance(centre);
        d(a).total_cmp(&d(b))
    });
    keys
}

/// Draws the basemap under the layers: each tile the view needs from the store or, until it
/// arrives, a lower zoom's tile stretched over its place. Returns how many tiles were drawn.
pub fn draw(canvas: &mut Canvas, map: &Map, vp: &Viewport, store: &TileStore) -> usize {
    let mut drawn = 0;
    for key in wanted(map, vp) {
        let e = bounds(key);
        let (a, b) = (vp.to_screen(mkaegis_geom::Coord::new(e.min_x, e.max_y)), vp.to_screen(mkaegis_geom::Coord::new(e.max_x, e.min_y)));
        let dest = [a[0], a[1], b[0], b[1]];
        let size = TILE_SIZE as f32;
        if let Some(img) = store.get(key) {
            canvas.image(&img, TILE_SIZE, [0.0, 0.0, size, size], dest);
            drawn += 1;
            continue;
        }
        for up in 1..=FALLBACK_LEVELS.min(key.z) {
            let parent = TileKey { z: key.z - up, x: key.x >> up, y: key.y >> up, ..key };
            if let Some(img) = store.get(parent) {
                let part = size / f32::from(1u16 << up);
                let (sx, sy) = ((key.x - (parent.x << up)) as f32 * part, (key.y - (parent.y << up)) as f32 * part);
                canvas.image(&img, TILE_SIZE, [sx, sy, part, part], dest);
                drawn += 1;
                break;
            }
        }
    }
    drawn
}

/// Writes the basemap's attribution in the image's bottom-right corner on a light band, as its
/// licence asks of exported maps.
pub fn attribute(canvas: &mut Canvas, basemap: Basemap) {
    let text = source(basemap).export_attribution;
    let scale = if canvas.width() >= 1200 { 2 } else { 1 };
    let pad = 3 * scale;
    let (w, h) = (crate::font::width(text) * scale + 2 * pad, crate::font::HEIGHT * scale + 2 * pad);
    let (x0, y0) = (canvas.width().saturating_sub(w), canvas.height().saturating_sub(h));
    canvas.fill_rect(x0, y0, canvas.width(), canvas.height(), mkaegis_core::Color::rgba(255, 255, 255, 200));
    canvas.text(text, x0 + pad, y0 + pad, scale, mkaegis_core::Color::rgba(40, 40, 40, 255));
}

/// The zoom whose tiles have the view's resolution (metres per pixel), within `0..=max_zoom`.
pub fn zoom_for(resolution: f64, max_zoom: u8) -> u8 {
    let zero = 2.0 * HALF_WORLD / TILE_SIZE as f64;
    let z = (zero / resolution).log2().round();
    if z.is_finite() { z.clamp(0.0, f64::from(max_zoom)) as u8 } else { 0 }
}

/// A tile's extent, in Web Mercator metres.
pub fn bounds(key: TileKey) -> Envelope {
    let side = 2.0 * HALF_WORLD / f64::from(1u32 << key.z.min(30));
    let (x, y) = (f64::from(key.x), f64::from(key.y));
    Envelope {
        min_x: -HALF_WORLD + x * side,
        max_x: -HALF_WORLD + (x + 1.0) * side,
        max_y: HALF_WORLD - y * side,
        min_y: HALF_WORLD - (y + 1.0) * side,
    }
}

/// The tiles of zoom `z` over `extent`, clipped to the world.
fn tiles_in(extent: Envelope, z: u8, basemap: Basemap) -> Vec<TileKey> {
    let n = 1u32 << z.min(30);
    let side = 2.0 * HALF_WORLD / f64::from(n);
    let index = |v: f64| -> u32 {
        let i = (v / side).floor();
        if i.is_finite() { i.clamp(0.0, f64::from(n - 1)) as u32 } else { 0 }
    };
    let (x0, x1) = (index(extent.min_x + HALF_WORLD), index(extent.max_x + HALF_WORLD));
    let (y0, y1) = (index(HALF_WORLD - extent.max_y), index(HALF_WORLD - extent.min_y));
    if extent.max_x < -HALF_WORLD || extent.min_x > HALF_WORLD || extent.max_y < -HALF_WORLD || extent.min_y > HALF_WORLD {
        return Vec::new();
    }
    (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| TileKey { basemap, z, x, y })).collect()
}

/// A tile's bytes (PNG or JPEG) as RGBA pixels; anything but a `TILE_SIZE` square is refused.
pub fn decode(bytes: &[u8]) -> Result<Vec<u8>> {
    let bad = |e: String| Error::format("tile", e);
    let (w, h, rgba) = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA);
        let mut reader = decoder.read_info().map_err(|e| bad(e.to_string()))?;
        let (w, h) = (reader.info().width as usize, reader.info().height as usize);
        if w != TILE_SIZE || h != TILE_SIZE {
            return Err(bad(format!("{w}×{h}, not {TILE_SIZE}×{TILE_SIZE}")));
        }
        let mut pixels = vec![0; reader.output_buffer_size().ok_or_else(|| bad("no buffer size".into()))?];
        let frame = reader.next_frame(&mut pixels).map_err(|e| bad(e.to_string()))?;
        pixels.truncate(frame.buffer_size());
        let rgba: Vec<u8> = match frame.color_type {
            png::ColorType::Rgba => pixels,
            png::ColorType::GrayscaleAlpha => pixels.as_chunks::<2>().0.iter().flat_map(|[g, a]| [*g, *g, *g, *a]).collect(),
            other => return Err(bad(format!("{other:?} pixels"))),
        };
        (w, h, rgba)
    } else {
        use zune_core::bytestream::ZCursor;
        use zune_core::colorspace::ColorSpace;
        use zune_core::options::DecoderOptions;
        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA).set_max_width(TILE_SIZE).set_max_height(TILE_SIZE);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
        let pixels = decoder.decode().map_err(|e| bad(format!("{e:?}")))?;
        let (w, h) = decoder.dimensions().ok_or_else(|| bad("no dimensions".into()))?;
        (w, h, pixels)
    };
    if w != TILE_SIZE || h != TILE_SIZE || rgba.len() != TILE_SIZE * TILE_SIZE * 4 {
        return Err(bad(format!("{} bytes for {w}×{h}", rgba.len())));
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkaegis_geom::Coord;

    #[test]
    fn tiles_cover_the_view_at_its_resolution() {
        // Petrolina at zoom 8 is tile 99, 134 (by the slippy-map formula, computed independently).
        let p = Crs::WEB_MERCATOR.from_geographic(Coord::new(-40.38, -8.87)).unwrap();
        let mut map = Map::default();
        map.basemap = Some(Basemap::Satellite);
        let res8 = 2.0 * HALF_WORLD / 256.0 / 256.0;
        let vp = Viewport { center: p, resolution: res8, width: 256.0, height: 256.0 };
        let keys = wanted(&map, &vp);
        assert_eq!(keys.first(), Some(&TileKey { basemap: Basemap::Satellite, z: 8, x: 99, y: 134 }));
        assert!(keys.len() <= 4, "a tile's worth of view needs at most four");
        assert!(bounds(keys[0]).contains(p));
        // Past the source's zoom, its last zoom (stretched); a huge view, a lower zoom.
        assert_eq!(zoom_for(res8 / 1024.0, 12), 12);
        let wide = Viewport { width: 8000.0, height: 8000.0, ..vp };
        assert!(wanted(&map, &wide).len() <= MAX_TILES);
        // No basemap, or a map in another CRS: nothing.
        map.crs = Crs::WGS84;
        assert!(wanted(&map, &vp).is_empty());
        assert!(wanted(&Map::default(), &vp).is_empty());
    }

    #[test]
    fn urls_follow_each_source() {
        let k = |basemap, z| TileKey { basemap, z, x: 99, y: 134 };
        assert_eq!(url(k(Basemap::OpenStreetMap, 8)), "https://tile.openstreetmap.org/8/99/134.png");
        assert!(url(k(Basemap::Satellite, 8)).contains("BlueMarble_NextGeneration/default/default/GoogleMapsCompatible_Level8/8/134/99.jpg"));
        assert!(
            url(k(Basemap::Satellite, 11))
                .contains("Landsat_WELD_CorrectedReflectance_TrueColor_Global_Annual/default/2000-12-01/GoogleMapsCompatible_Level12/11/134/99.jpg")
        );
    }

    #[test]
    fn the_store_hands_out_each_tile_once_and_draws_a_parent_meanwhile() {
        let store = TileStore::default();
        let key = |z, x, y| TileKey { basemap: Basemap::OpenStreetMap, z, x, y };
        let (a, b) = (key(3, 1, 2), key(3, 2, 2));
        assert_eq!(store.claim(&[a, b]), [a, b]);
        assert!(store.claim(&[a, b]).is_empty(), "already being fetched");
        store.insert(a, vec![255; TILE_SIZE * TILE_SIZE * 4]);
        assert!(store.arrive(b, b"not an image").is_err());
        assert!(store.claim(&[a, b]).is_empty(), "arrived, or failed and not asked for again");
        assert!(store.get(a).is_some() && store.get(b).is_none());
        assert!(store.revision() > 0);

        // A red parent at zoom 0 stands in for its child at zoom 2 until that arrives.
        let red: Vec<u8> = (0..TILE_SIZE * TILE_SIZE).flat_map(|_| [200, 0, 0, 255]).collect();
        store.insert(key(0, 0, 0), red);
        let mut map = Map::default();
        map.basemap = Some(Basemap::OpenStreetMap);
        let child = bounds(key(2, 1, 1));
        let vp = Viewport::fit(child, 256.0, 256.0);
        let vp = Viewport { resolution: 2.0 * HALF_WORLD / 256.0 / 4.0, ..vp };
        let mut canvas = Canvas::new(256, 256, mkaegis_core::Color::WHITE).unwrap();
        assert!(draw(&mut canvas, &map, &vp, &store) > 0);
        assert_eq!(canvas.pixel(128, 128), Some([200, 0, 0, 255]));
    }

    #[test]
    fn tiles_decode_and_hostile_bytes_never_panic() {
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 256, 256);
            enc.set_color(png::ColorType::Rgb);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&vec![10u8; 256 * 256 * 3]).unwrap();
        }
        let rgba = decode(&png_bytes).unwrap();
        assert_eq!((rgba.len(), &rgba[..4]), (256 * 256 * 4, &[10u8, 10, 10, 255][..]));
        for cut in [0, 8, 40, png_bytes.len() / 2] {
            assert!(decode(&png_bytes[..cut]).is_err());
        }
        assert!(decode(&[0xff, 0xd8, 0xff, 0x00]).is_err());
    }
}
