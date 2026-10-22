//! Files in and out: data by extension (GeoJSON, shapefile; CSV tables) and the project file (`.aegis`,
//! the map as JSON). Every file is written with [`write_atomic`], so a failed write never leaves
//! a truncated file in place of a good one.

use std::io::Write;
use std::path::{Path, PathBuf};

use mkaegis_core::{Error, Result};
use mkaegis_model::{FeatureClass, Map};

/// The extension of the project file.
pub const PROJECT_EXTENSION: &str = "aegis";
/// The project format this build writes, and the newest it reads.
pub const PROJECT_VERSION: u64 = 1;
/// The largest project file read: its JSON is held in memory whole, then the map.
const MAX_PROJECT_BYTES: u64 = 1 << 30;
/// The largest delimited text table read.
const MAX_TABLE_BYTES: u64 = 512 << 20;

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

/// Writes a feature class to a data file chosen by extension, and returns warnings about what
/// the format couldn't hold.
pub fn write_data(fc: &FeatureClass, path: &Path) -> Result<Vec<String>> {
    match extension(path).as_str() {
        "geojson" | "json" => write_atomic(path, mkaegis_geojson::write(fc)?.as_bytes()).map(|()| Vec::new()),
        // The attributes only: a table, for spreadsheets and statistics packages.
        "csv" => {
            let rows: Vec<&[mkaegis_core::Value]> = fc.features.iter().map(|f| f.values.as_slice()).collect();
            write_atomic(path, mkaegis_csv::write(&fc.fields, &rows).as_bytes()).map(|()| Vec::new())
        }
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
            Ok(files.warnings)
        }
        other => Err(Error::NotAvailable(format!("writing `.{other}` files"))),
    }
}

/// A delimited text table (CSV, TSV), with column types overridden by name in `types`.
pub fn read_table(path: &Path, types: &std::collections::HashMap<String, mkaegis_model::FieldType>) -> Result<mkaegis_csv::Table> {
    if std::fs::metadata(path)?.len() > MAX_TABLE_BYTES {
        return Err(Error::format("CSV", format!("larger than {} MB", MAX_TABLE_BYTES >> 20)));
    }
    mkaegis_csv::read(&std::fs::read(path)?, types)
}

pub fn save_project(map: &Map, path: &Path) -> Result<()> {
    let text = serde_json::to_string_pretty(&serde_json::json!({ "format": "aegis", "version": PROJECT_VERSION, "map": map }))
        .map_err(|e| Error::Internal(e.to_string()))?;
    write_atomic(path, text.as_bytes())
}

/// A project file's map, checked (see `Map::validate`), and the warnings about what was
/// repaired. A project from a newer MK aeGIS is refused rather than opened with parts missing
/// and then saved back without them (#58).
pub fn open_project(path: &Path) -> Result<(Map, Vec<String>)> {
    if std::fs::metadata(path)?.len() > MAX_PROJECT_BYTES {
        return Err(Error::format("project", format!("larger than {} MB", MAX_PROJECT_BYTES >> 20)));
    }
    let text = std::fs::read_to_string(path)?;
    read_project(&text)
}

/// A project from its JSON text: see [`open_project`].
pub fn read_project(text: &str) -> Result<(Map, Vec<String>)> {
    let mut root: serde_json::Value = serde_json::from_str(text).map_err(|e| Error::format("project", e.to_string()))?;
    if root.get("format").and_then(serde_json::Value::as_str) != Some("aegis") {
        return Err(Error::format("project", "not a MK aeGIS project"));
    }
    // Projects written before the version was read all say 1.
    let version = match root.get("version") {
        None => 1,
        Some(v) => v.as_u64().ok_or_else(|| Error::format("project", "the format version is not a whole number"))?,
    };
    if version > PROJECT_VERSION {
        return Err(Error::format(
            "project",
            format!(
                "it was saved by a newer MK aeGIS (project format {version}); this one reads up to {PROJECT_VERSION}: update MK aeGIS to open it"
            ),
        ));
    }
    let map = root.get_mut("map").map(serde_json::Value::take).ok_or_else(|| Error::format("project", "no map"))?;
    let mut map: Map = serde_json::from_value(map).map_err(|e| Error::format("project", e.to_string()))?;
    let warnings = map.validate()?;
    Ok((map, warnings))
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

    /// #58: project files are hostile input.
    mod hostile_projects {
        use super::*;
        use serde_json::{Value as Json, json};

        /// The sample map as project JSON, to tamper with.
        fn sample() -> Json {
            let s = crate::Session::sample();
            json!({ "format": "aegis", "version": PROJECT_VERSION, "map": s.map })
        }

        fn open(project: &Json) -> Result<(Map, Vec<String>)> {
            read_project(&project.to_string())
        }

        #[test]
        fn newer_versions_are_refused_and_old_files_without_one_open() {
            let mut p = sample();
            p["version"] = json!(PROJECT_VERSION + 1);
            let err = open(&p).unwrap_err().to_string();
            assert!(err.contains("newer MK aeGIS"), "{err}");
            p["version"] = json!("2");
            assert!(open(&p).is_err());
            p.as_object_mut().unwrap().remove("version");
            assert!(open(&p).is_ok());
        }

        #[test]
        fn the_next_layer_id_follows_the_ids_in_use() {
            let mut p = sample();
            p["map"]["next_id"] = json!(1);
            let (mut map, _) = open(&p).unwrap();
            let ids: Vec<u32> = map.layers.iter().map(|l| l.id).collect();
            let fc = mkaegis_model::FeatureClass::new("new", mkaegis_geom::GeometryType::Point, map.crs, vec![]);
            let added = map.add_layer(fc);
            assert!(!ids.contains(&added), "{added} is already used by {ids:?}");
        }

        #[test]
        fn what_cannot_be_trusted_is_refused() {
            let cases: [(&str, fn(&mut Json)); 5] = [
                ("two layers have the id", |p| p["map"]["layers"][1]["id"] = p["map"]["layers"][0]["id"].clone()),
                ("values for", |p| p["map"]["layers"][0]["data"]["features"][0]["values"] = json!([])),
                ("doesn't fit", |p| {
                    let pop = p["map"]["layers"][0]["data"]["fields"].as_array().unwrap().iter().position(|f| f["name"] == "POP").unwrap();
                    p["map"]["layers"][0]["data"]["features"][0]["values"][pop] = json!("many");
                }),
                ("a Polygon in a Point layer", |p| {
                    p["map"]["layers"][0]["data"]["features"][0]["geometry"] =
                        json!({ "Polygon": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]] })
                }),
                ("two fields", |p| {
                    let first = p["map"]["layers"][0]["data"]["fields"][0].clone();
                    p["map"]["layers"][0]["data"]["fields"][1]["name"] = first["name"].clone();
                }),
            ];
            for (want, tamper) in cases {
                let mut p = sample();
                tamper(&mut p);
                let err = open(&p).map(|_| ()).unwrap_err().to_string();
                assert!(err.contains(want), "{want}: {err}");
            }
        }

        #[test]
        fn what_can_be_repaired_is_repaired_with_a_warning() {
            let mut s = crate::Session::sample();
            s.execute("stats.weights", &json!({ "layer": "Cidades" })).unwrap();
            let mut p = json!({ "format": "aegis", "version": PROJECT_VERSION, "map": s.map });
            p["map"]["layers"][0]["selection"] = json!([0, 1, 999]);
            p["map"]["weights"][0]["neighbours"][0] = json!([[5000, 1.0]]);
            let (map, warnings) = open(&p).unwrap();
            assert_eq!(map.layers[0].selection.len(), 2);
            assert!(map.weights.is_empty());
            assert_eq!(warnings.len(), 2, "{warnings:?}");
        }

        #[test]
        fn hostile_projects_never_panic() {
            let text = sample().to_string();
            let bytes = text.as_bytes();
            for cut in (0..bytes.len()).step_by(bytes.len() / 200 + 1) {
                let _ = read_project(&String::from_utf8_lossy(bytes.get(..cut).unwrap_or_default()));
            }
            let mut flipped = bytes.to_vec();
            for i in (0..flipped.len()).step_by(97) {
                let original = flipped[i];
                for b in *b"09-e\"[{1" {
                    flipped[i] = b;
                    let _ = read_project(&String::from_utf8_lossy(&flipped));
                }
                flipped[i] = original;
            }
        }
    }
}
