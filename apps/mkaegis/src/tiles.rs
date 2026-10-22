//! Basemap tiles on the desktop (#15): a few worker threads sharing one HTTP agent that names MK
//! aeGIS and where to reach it in its User-Agent, and a disk cache that keeps each tile seven days,
//! as OpenStreetMap's tile policy asks. Only the tiles a view asks for are fetched: nothing ahead.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mkaegis_ui_egui::FetchDone;

const USER_AGENT: &str = concat!("MK aeGIS/", env!("CARGO_PKG_VERSION"), " (+https://github.com/morrisonkuhlsen/mkaegis)");
const WORKERS: usize = 4;
/// How long a cached tile is used before it is fetched again.
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
/// The largest tile read: a 256-pixel tile is far smaller.
const MAX_BYTES: u64 = 4 << 20;
const TIMEOUT: Duration = Duration::from_secs(20);

/// A fetch service for `Services::fetch`, caching under `cache` when there is one.
pub fn fetcher(cache: Option<PathBuf>) -> impl Fn(String, FetchDone) {
    let (jobs, queue) = channel::<(String, FetchDone)>();
    let queue = Arc::new(Mutex::new(queue));
    let agent: ureq::Agent = ureq::Agent::config_builder().user_agent(USER_AGENT).timeout_global(Some(TIMEOUT)).build().into();
    for _ in 0..WORKERS {
        let (queue, agent, cache) = (Arc::clone(&queue), agent.clone(), cache.clone());
        std::thread::spawn(move || work(&queue, &agent, cache.as_deref()));
    }
    move |url: String, done: FetchDone| {
        // The workers outlive every request; a send fails only if they all stopped.
        if let Err(e) = jobs.send((url, done)) {
            let (_, done) = e.0;
            done(Err("the tile workers stopped".into()));
        }
    }
}

/// The per-user cache folder for tiles.
pub fn cache_dir() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    };
    base.map(|b| b.join("mkaegis").join("tiles"))
}

fn work(queue: &Mutex<Receiver<(String, FetchDone)>>, agent: &ureq::Agent, cache: Option<&Path>) {
    loop {
        let job = queue.lock().unwrap_or_else(PoisonError::into_inner).recv();
        let Ok((url, done)) = job else { return };
        done(fetch(agent, cache, &url));
    }
}

fn fetch(agent: &ureq::Agent, cache: Option<&Path>, url: &str) -> Result<Vec<u8>, String> {
    let file = cache.map(|c| c.join(cache_name(url)));
    if let Some(f) = &file
        && is_fresh(f)
        && let Ok(bytes) = std::fs::read(f)
    {
        return Ok(bytes);
    }
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let bytes = response.body_mut().with_config().limit(MAX_BYTES).read_to_vec().map_err(|e| e.to_string())?;
    if let Some(f) = file {
        let _ = f.parent().map(std::fs::create_dir_all);
        if let Err(e) = std::fs::write(&f, &bytes) {
            log::warn!("couldn't cache {}: {e}", f.display());
        }
    }
    Ok(bytes)
}

/// A cached tile younger than [`MAX_AGE`].
fn is_fresh(file: &Path) -> bool {
    std::fs::metadata(file).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < MAX_AGE)
}

/// A file name for a URL: its host and path, with anything but letters, digits, `.` and `-` as `_`.
fn cache_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One real tile from each source, fetched and decoded: `cargo test -p mkaegis -- --ignored`.
    #[test]
    #[ignore = "fetches from the network"]
    fn one_tile_from_each_source() {
        use mkaegis_render::basemap::{TileKey, TileStore, url};
        let fetch = fetcher(None);
        let store = TileStore::default();
        let (tx, rx) = channel();
        for (basemap, z, x, y) in [(mkaegis_model::Basemap::OpenStreetMap, 8, 99, 134), (mkaegis_model::Basemap::Satellite, 11, 794, 1074)] {
            let key = TileKey { basemap, z, x, y };
            let (store, tx) = (store.clone(), tx.clone());
            fetch(url(key), Box::new(move |r| tx.send((key, r.map_err(mkaegis_core::Error::Io).and_then(|b| store.arrive(key, &b)))).unwrap_or(())));
        }
        for _ in 0..2 {
            let (key, result) = rx.recv_timeout(Duration::from_secs(60)).unwrap();
            assert!(result.is_ok(), "{key:?}: {result:?}");
            assert!(store.get(key).is_some());
        }
    }

    #[test]
    fn cache_names_are_plain_file_names() {
        assert_eq!(cache_name("https://tile.openstreetmap.org/8/99/134.png"), "tile.openstreetmap.org_8_99_134.png");
        assert!(!cache_name("https://x/../../etc/passwd?a=b").contains('/'));
        assert!(USER_AGENT.starts_with("MK aeGIS/") && USER_AGENT.contains("github.com/morrisonkuhlsen/mkaegis"));
    }
}
