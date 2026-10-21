use serde_json::{Value as Json, json};

use super::*;

/// A command that panicked shows up as this internal error.
fn panicked(r: &Result<Json>) -> bool {
    matches!(r, Err(Error::Internal(m)) if m.contains("failed unexpectedly"))
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mkaegis-test-{}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[test]
fn command_ids_are_unique_and_documented() {
    let mut ids: Vec<&str> = COMMANDS.iter().map(|c| c.id).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n);
    for c in COMMANDS {
        assert!(c.id.contains('.') && !c.label.is_empty() && c.ribbon.contains('▸') && !c.params.is_empty(), "{}", c.id);
    }
}

#[test]
fn every_command_survives_empty_params() {
    for c in COMMANDS {
        for params in [json!({}), Json::Null, json!([]), json!("x")] {
            let mut s = Session::sample();
            let r = s.execute(c.id, &params);
            assert!(!panicked(&r), "{} panicked with {params}", c.id);
        }
    }
}

#[test]
fn hostile_params_never_panic() {
    let missing = std::env::temp_dir().join("mkaegis-no-such-dir").join("x");
    let paths = [
        json!(""),
        json!(missing.with_extension("geojson").display().to_string()),
        json!(missing.with_extension("shp").display().to_string()),
        json!(missing.with_extension("png").display().to_string()),
        json!(missing.with_extension("mkaegis").display().to_string()),
    ];
    let values = [
        json!(null),
        json!(-1),
        json!(0),
        json!(1),
        json!(3),
        json!(1e308),
        json!(-1e308),
        json!(u64::MAX),
        json!(""),
        json!("NaN"),
        json!("inf"),
        json!("💥"),
        json!("Cidades"),
        json!("Ilhas"),
        json!("NOME"),
        json!("POP"),
        json!("POP > 1000 OR NOME LIKE '%a%'"),
        json!("(((("),
        json!("x".repeat(100_000)),
        json!([]),
        json!([0, 1, 2]),
        json!([-1, 1e300]),
        json!({}),
        json!(true),
        json!("graduatedColors"),
        json!("uniqueValues"),
        json!("withinDistance"),
        json!("#ff0000"),
        json!(4326),
        json!(32723),
    ];
    let keys = [
        "layer",
        "selecting",
        "where",
        "mode",
        "rows",
        "row",
        "field",
        "value",
        "name",
        "type",
        "expression",
        "to",
        "visible",
        "opacity",
        "renderer",
        "classes",
        "method",
        "ramp",
        "fill",
        "outline",
        "width",
        "size",
        "marker",
        "color",
        "distance",
        "unit",
        "relation",
        "epsg",
        "property",
        "output",
        "selectedOnly",
        "xmin",
        "ymin",
        "xmax",
        "ymax",
        "height",
    ];
    let mut seed: u64 = 0x5eed;
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (seed >> 33) as usize % n
    };
    for c in COMMANDS {
        for _ in 0..60 {
            let mut p = serde_json::Map::new();
            for _ in 0..(1 + next(6)) {
                let key = keys[next(keys.len())];
                p.insert(key.into(), values[next(values.len())].clone());
            }
            p.insert("path".into(), paths[next(paths.len())].clone());
            // Mostly a real layer, so the command gets past the lookup.
            if next(4) > 0 {
                p.insert("layer".into(), json!(["Cidades", "Ilhas", "Rios", "Estradas"][next(4)]));
            }
            let mut s = Session::sample();
            let params = Json::Object(p);
            let r = s.execute(c.id, &params);
            assert!(!panicked(&r), "{} panicked with {params}", c.id);
        }
    }
}

#[test]
fn undo_and_redo_restore_the_map() {
    let mut s = Session::sample();
    let before = s.map.clone();
    s.execute("layer.remove", &json!({ "layer": "Rios" })).unwrap();
    assert!(s.map.layer("Rios").is_err());
    assert_eq!(s.can_undo(), Some("Remove"));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.map, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(s.map.layer("Rios").is_err());
    assert!(s.execute("edit.redo", &json!({})).is_err());
}

#[test]
fn failed_commands_leave_the_map_untouched() {
    let mut s = Session::sample();
    let before = s.map.clone();
    assert!(s.execute("layer.symbology", &json!({ "layer": "Cidades", "renderer": "graduatedColors", "field": "NOME" })).is_err());
    assert!(s.execute("analysis.buffer", &json!({ "layer": "Rios", "distance": 100 })).is_err());
    assert_eq!(s.map, before);
    assert!(s.can_undo().is_none());
    assert!(s.history().iter().all(|h| !h.ok));
}

#[test]
fn select_buffer_and_select_by_location() {
    let mut s = Session::sample();
    let r = s.execute("map.selectByAttributes", &json!({ "layer": "Cidades", "where": "CAPITAL = TRUE" })).unwrap();
    assert_eq!(r["selected"], 1);
    // The tool works on the selection: one buffer.
    let r = s.execute("analysis.buffer", &json!({ "layer": "Cidades", "distance": 25, "unit": "km", "output": "Raio" })).unwrap();
    let id = r["layer"].as_u64().unwrap();
    assert_eq!(s.map.layer("Raio").unwrap().data.features.len(), 1);
    s.execute("map.clearSelection", &json!({})).unwrap();
    let r = s.execute("map.selectByLocation", &json!({ "layer": "Cidades", "selecting": id, "relation": "intersect" })).unwrap();
    assert!(r["selected"].as_u64().unwrap() >= 1);
    let stats = s.execute("analysis.summaryStatistics", &json!({ "layer": "Cidades", "field": "POP", "selectedOnly": true })).unwrap();
    assert!(stats["sum"].as_f64().unwrap() >= 412_000.0);
}

#[test]
fn rectangles_select_across_projections() {
    let mut s = Session::sample();
    let e = s.map.full_extent().unwrap();
    let r = s.execute("map.selectByRectangle", &json!({ "xmin": e.min_x, "ymin": e.min_y, "xmax": e.max_x, "ymax": e.max_y })).unwrap();
    assert_eq!(r["selected"], 23 + 20 + 4 + 3);
    let r = s.execute("map.selectByRectangle", &json!({ "xmin": 0, "ymin": 0, "xmax": 1, "ymax": 1, "layer": "Cidades" })).unwrap();
    // Only the named layer's selection is replaced.
    assert_eq!(r["selected"], 0);
    assert_eq!(s.map.layer("Ilhas").unwrap().selection.len(), 3);
}

#[test]
fn symbology_labels_fields_and_calculations() {
    let mut s = Session::sample();
    s.execute(
        "layer.symbology",
        &json!({ "layer": "Cidades", "renderer": "graduatedColors", "field": "POP", "classes": 4, "method": "quantile", "ramp": "ember" }),
    )
    .unwrap();
    s.execute("layer.symbology", &json!({ "layer": "Ilhas", "renderer": "uniqueValues", "field": "NOME" })).unwrap();
    s.execute("management.calculateGeometry", &json!({ "layer": "Ilhas", "field": "AREA_KM2", "property": "areaKm2" })).unwrap();
    s.execute("layer.addField", &json!({ "layer": "Cidades", "name": "POP_MIL", "type": "double" })).unwrap();
    s.execute("layer.calculateField", &json!({ "layer": "Cidades", "field": "POP_MIL", "expression": "POP / 1000" })).unwrap();
    let r = s.execute("map.selectByAttributes", &json!({ "layer": "Cidades", "where": "POP_MIL = 412" })).unwrap();
    assert_eq!(r["selected"], 1);
    let big = s.execute("map.selectByAttributes", &json!({ "layer": "Ilhas", "where": "AREA_KM2 > 20000" })).unwrap();
    assert_eq!(big["selected"], 1);
    s.execute("layer.labels", &json!({ "layer": "Ilhas", "field": "nome", "size": 11 })).unwrap();
    assert_eq!(s.map.layer("Ilhas").unwrap().labels.as_ref().unwrap().field, "NOME");
}

#[test]
fn data_and_projects_round_trip_through_files() {
    let dir = scratch("files");
    let mut s = Session::sample();
    for ext in ["geojson", "shp"] {
        let file = dir.join(format!("ilhas.{ext}"));
        s.execute("layer.export", &json!({ "layer": "Ilhas", "path": file.display().to_string() })).unwrap();
        let r = s.execute("map.addData", &json!({ "path": file.display().to_string() })).unwrap();
        let added = s.map.layer(&r["layer"].to_string()).unwrap();
        assert_eq!(added.data.features.len(), 3, "{ext}");
        assert!(added.data.field_index("NOME").is_some());
    }
    let project = dir.join("sample.mkaegis");
    s.execute("project.saveAs", &json!({ "path": project.display().to_string() })).unwrap();
    assert!(!s.modified);
    let mut t = Session::default();
    t.execute("project.open", &json!({ "path": project.display().to_string() })).unwrap();
    assert_eq!(t.map, s.map);
    let png = dir.join("map.png");
    s.execute("share.exportMap", &json!({ "path": png.display().to_string(), "width": 320, "height": 200 })).unwrap();
    assert!(std::fs::metadata(&png).unwrap().len() > 1000);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn map_crs_changes_reproject_the_view() {
    let mut s = Session::sample();
    s.execute("map.setCrs", &json!({ "epsg": 32726 })).unwrap();
    let e = s.map.extent.unwrap();
    // UTM 26S (central meridian 27°W) around 24.6°W, 24°S: about 245 km east of 500 km.
    assert!(e.center().x > 650_000.0 && e.center().x < 850_000.0 && e.center().y > 7_000_000.0, "{e:?}");
    assert!(s.execute("map.setCrs", &json!({ "epsg": 1234 })).is_err());
}
