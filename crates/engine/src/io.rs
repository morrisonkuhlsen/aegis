//! Files in and out: data by extension (GeoJSON, shapefile) and the project file (`.aegis`,
//! the map as JSON). Every file is written with [`write_atomic`], so a failed write never leaves
//! a truncated file in place of a good one.

use std::io::Write;
use std::path::{Path, PathBuf};

use mkaegis_core::{Error, Result};
use mkaegis_model::{FeatureClass, Map};

/// The extension of the project file.
pub const PROJECT_EXTENSION: &str = "aegis";

/// The data file extensions Add Data opens.
pub const DATA_EXTENSIONS: [&str; 3] = ["geojson", "json", "shp"];

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "layer".into())
}

/// A feature class from a data file, and any warnings.
pub fn read_data(path: &Path) -> Result<(FeatureClass, Vec<String>)> {
    match extension(path).as_str() {
        "geojson" | "json" => {
            let text = std::fs::read_to_string(path)?;
            let r = mkaegis_geojson::read(&text, &stem(path))?;
            let warnings = if r.skipped > 0 { vec![format!("{} features of another geometry type skipped", r.skipped)] } else { Vec::new() };
            Ok((r.data, warnings))
        }
        "shp" => {
            let r = mkaegis_shp::read_path(path)?;
            Ok((r.data, r.warnings))
        }
        other => Err(Error::NotAvailable(format!("reading `.{other}` files"))),
    }
}

/// Writes a feature class to a data file chosen by extension.
pub fn write_data(fc: &FeatureClass, path: &Path) -> Result<()> {
    match extension(path).as_str() {
        "geojson" | "json" => write_atomic(path, mkaegis_geojson::write(fc)?.as_bytes()),
        "shp" => {
            let files = mkaegis_shp::write(fc)?;
            write_atomic(&path.with_extension("shp"), &files.shp)?;
            write_atomic(&path.with_extension("shx"), &files.shx)?;
            write_atomic(&path.with_extension("dbf"), &files.dbf)?;
            if let Some(prj) = &files.prj {
                write_atomic(&path.with_extension("prj"), prj.as_bytes())?;
            }
            if let Some(cpg) = &files.cpg {
                write_atomic(&path.with_extension("cpg"), cpg.as_bytes())?;
            }
            Ok(())
        }
        other => Err(Error::NotAvailable(format!("writing `.{other}` files"))),
    }
}

pub fn save_project(map: &Map, path: &Path) -> Result<()> {
    let text = serde_json::to_string_pretty(&serde_json::json!({ "format": "aegis", "version": 1, "map": map }))
        .map_err(|e| Error::Internal(e.to_string()))?;
    write_atomic(path, text.as_bytes())
}

pub fn open_project(path: &Path) -> Result<Map> {
    let text = std::fs::read_to_string(path)?;
    let mut root: serde_json::Value = serde_json::from_str(&text).map_err(|e| Error::format("project", e.to_string()))?;
    if root.get("format").and_then(serde_json::Value::as_str) != Some("aegis") {
        return Err(Error::format("project", "not a MK aeGIS project"));
    }
    let map = root.get_mut("map").map(serde_json::Value::take).ok_or_else(|| Error::format("project", "no map"))?;
    serde_json::from_value(map).map_err(|e| Error::format("project", e.to_string()))
}

/// Writes a whole file or leaves the old one as it was: the bytes go to a temporary file in the
/// same folder, are flushed to disk, and the temporary file then replaces `path` in one rename. A
/// full disk, a crash or a power cut mid-write can't truncate the only copy of someone's work
/// (#42). The new file keeps the old one's permissions, and a symbolic link is written through.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with(path, |file| file.write_all(bytes))
}

/// [`write_atomic`] with the writing done by `write`, so a test can make it fail.
fn write_atomic_with(path: &Path, write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>) -> Result<()> {
    let target = if path.is_symlink() { std::fs::canonicalize(path)? } else { path.to_path_buf() };
    let temp = temporary_beside(&target)?;
    let written = (|| {
        let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&temp)?;
        write(&mut file)?;
        if let Ok(old) = std::fs::metadata(&target) {
            file.set_permissions(old.permissions())?;
        }
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    Ok(written?)
}

/// A temporary file name in `path`'s folder (a rename is atomic only within one file system).
fn temporary_beside(path: &Path) -> Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| Error::param("path", format!("`{}` names no file", path.display())))?;
    Ok(path.with_file_name(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mkaegis-io-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// Everything in a folder, to see that no temporary file is left behind.
    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_intact() {
        let dir = scratch("fail");
        let file = dir.join("work.aegis");
        std::fs::write(&file, "the only copy").unwrap();
        // Half the bytes go out, then the disk is full.
        let r = write_atomic_with(&file, |f| {
            f.write_all(b"half of the ne")?;
            Err(std::io::Error::other("no space left on device"))
        });
        assert!(r.is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "the only copy");
        assert_eq!(listing(&dir), ["work.aegis"], "no temporary file left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_write_replaces_the_file_whole() {
        let dir = scratch("ok");
        let file = dir.join("work.aegis");
        write_atomic(&file, b"first").unwrap();
        write_atomic(&file, b"second, longer").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "second, longer");
        assert_eq!(listing(&dir), ["work.aegis"]);
        assert!(write_atomic(&dir.join("missing-folder").join("x.aegis"), b"x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn permissions_and_links_are_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("perm");
        let file = dir.join("work.aegis");
        std::fs::write(&file, "old").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.join("link.aegis");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(link.is_symlink(), "the link stays a link");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
