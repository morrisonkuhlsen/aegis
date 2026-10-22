use mkaegis_crs::Crs;
use serde_json::{Value as Json, json};

use super::*;
use crate::cmd::{always, has_layers};

/// Commands that change the map, then panic: the map must come back as it was (#59).
pub(super) const FAILING: [CommandSpec; 2] = [
    CommandSpec {
        id: "test.editThenPanic",
        label: "Edit Then Panic",
        ribbon: "Test",
        shortcut: None,
        params: "{}",
        kind: Kind::Edit,
        enabled: always,
        run: |s, _| {
            if let Some(l) = s.map.layers.first_mut() {
                l.name = "half done".into();
                l.data_mut().features.clear();
            }
            panic!("a bug after the first change")
        },
    },
    CommandSpec {
        id: "test.selectThenPanic",
        label: "Select Then Panic",
        ribbon: "Test",
        shortcut: None,
        params: "{}",
        kind: Kind::View,
        enabled: has_layers,
        run: |s, _| {
            if let Some(l) = s.map.layers.first_mut() {
                l.select([0, 1], mkaegis_model::SelectionMode::New);
            }
            panic!("a bug on the second layer")
        },
    },
];

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
        json!(missing.with_extension("aegis").display().to_string()),
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
        // Chains as long as a clause may be: they once overflowed the stack (#43).
        json!(format!("{}1 = 1", "1+".repeat(32_000))),
        json!(format!("{}POP > 0", "POP > 0 OR ".repeat(5_000))),
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
fn short_fields_hold_16_bit_integers() {
    let mut s = Session::sample();
    s.execute("layer.addField", &json!({ "layer": "Cidades", "name": "N", "type": "short" })).unwrap();
    s.execute("feature.setValue", &json!({ "layer": "Cidades", "row": 1, "field": "N", "value": 7 })).unwrap();
    s.execute("feature.setValue", &json!({ "layer": "Cidades", "row": 2, "field": "N", "value": "-32768" })).unwrap();
    let r = s.execute("feature.identify", &json!({ "layer": "Cidades", "row": 1 })).unwrap();
    assert_eq!(r["attributes"]["N"], 7);
    for bad in [json!(40000), json!(1.5), json!("abc")] {
        assert!(s.execute("feature.setValue", &json!({ "layer": "Cidades", "row": 1, "field": "N", "value": bad })).is_err(), "{bad}");
    }
    // The sample's flags are Short 0/1, written to GeoJSON as numbers.
    let r = s.execute("feature.identify", &json!({ "layer": "Cidades", "row": 0 })).unwrap();
    assert_eq!(r["attributes"]["CAPITAL"], 1);
    let file = scratch("short").join("cidades.geojson");
    s.execute("layer.export", &json!({ "layer": "Cidades", "path": file.display().to_string() })).unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains("\"CAPITAL\":1"));
    let _ = std::fs::remove_dir_all(scratch("short"));
}

#[test]
fn calculate_field_refuses_values_that_do_not_fit() {
    let mut s = Session::sample();
    let before = s.map.clone();
    let r = s.execute("layer.calculateField", &json!({ "layer": "Cidades", "field": "CAPITAL", "expression": "100000" }));
    assert!(r.unwrap_err().to_string().contains("doesn't fit the Short field"));
    assert_eq!(s.map, before, "nothing was written");
    assert!(s.execute("layer.calculateField", &json!({ "layer": "Cidades", "field": "POP", "expression": "UPPER(NOME)" })).is_err());
    s.execute("layer.calculateField", &json!({ "layer": "Cidades", "field": "CAPITAL", "expression": "POP > 100000" })).unwrap();
    let big = s.execute("map.selectByAttributes", &json!({ "layer": "Cidades", "where": "CAPITAL = 1" })).unwrap();
    assert_eq!(big["selected"], 4);
}

#[test]
fn spatial_weights_are_built_inspected_saved_and_exchanged() {
    let mut s = Session::sample();
    // Points default to 4 nearest neighbours, row-standardised.
    let r = s.execute("stats.weights", &json!({ "layer": "Cidades" })).unwrap();
    assert_eq!(r["name"], "Cidades_knn");
    assert_eq!(r["summary"]["n"], 23);
    assert_eq!(r["summary"]["min_neighbours"], 4);
    let info = s.execute("stats.weightsInfo", &json!({ "weights": "Cidades_knn", "row": 0 })).unwrap();
    let total: f64 = info["neighbours"].as_array().unwrap().iter().map(|n| n["weight"].as_f64().unwrap()).sum();
    assert!((total - 1.0).abs() < 1e-12);
    let picked = s.execute("stats.selectNeighbors", &json!({ "weights": "Cidades_knn", "row": 0 })).unwrap();
    assert_eq!(picked["selected"], 4);
    // Polygons default to queen contiguity: the islands are apart, so each is alone.
    let q = s.execute("stats.weights", &json!({ "layer": "Ilhas" })).unwrap();
    assert_eq!(q["summary"]["islands"].as_array().unwrap().len(), 3);
    // The smallest distance band leaves no city alone.
    let d = s.execute("stats.weights", &json!({ "layer": "Cidades", "method": "distance" })).unwrap();
    assert!(d["summary"]["islands"].as_array().unwrap().is_empty());
    // GAL out and back in, by an id field.
    let dir = scratch("weights");
    let gal = dir.join("cidades.gal");
    s.execute("stats.exportWeights", &json!({ "weights": "Cidades_knn", "path": gal.display().to_string(), "idField": "NOME" })).unwrap();
    assert!(std::fs::read_to_string(&gal).unwrap().starts_with("0 23 Cidades NOME\n"));
    let back = s.execute("stats.importWeights", &json!({ "layer": "Cidades", "path": gal.display().to_string(), "idField": "NOME" })).unwrap();
    assert_eq!(back["name"], "cidades");
    assert_eq!(s.map.weights.iter().find(|w| w.name == "cidades").unwrap().neighbours, s.map.weights[0].neighbours);
    // Saved with the project; stale after deleting a feature.
    let project = dir.join("w.aegis");
    s.execute("project.saveAs", &json!({ "path": project.display().to_string() })).unwrap();
    let mut t = Session::default();
    t.execute("project.open", &json!({ "path": project.display().to_string() })).unwrap();
    assert_eq!(t.map.weights.len(), 4);
    t.execute("feature.delete", &json!({ "layer": "Cidades", "rows": [0] })).unwrap();
    assert!(
        t.execute("stats.selectNeighbors", &json!({ "weights": "Cidades_knn", "row": 1 }))
            .unwrap_err()
            .to_string()
            .contains("build the weights again")
    );
    t.execute("stats.removeWeights", &json!({ "weights": "Cidades_knn" })).unwrap();
    assert_eq!(t.map.weights.len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #49: weights outlived their layer, attached to a new layer by name, and exported ids that
/// collided.
#[test]
fn weights_follow_their_layer_and_their_files_name_each_feature_once() {
    let mut s = Session::sample();
    s.execute("stats.weights", &json!({ "layer": "Cidades" })).unwrap();
    let id = s.map.layer("Cidades").unwrap().id;
    // Stale after a feature is deleted: reported, and refused where rows matter.
    s.execute("feature.delete", &json!({ "layer": "Cidades", "rows": [0] })).unwrap();
    let info = s.execute("stats.weightsInfo", &json!({ "weights": "Cidades_knn" })).unwrap();
    assert!(info["problem"].as_str().is_some_and(|p| p.contains("build the weights again")), "{info}");
    assert!(s.execute("stats.weightsInfo", &json!({ "weights": "Cidades_knn", "row": 1 })).is_err());
    let dir = scratch("weights-life");
    let gal = dir.join("w.gal").display().to_string();
    assert!(s.execute("stats.exportWeights", &json!({ "weights": "Cidades_knn", "path": gal })).is_err());
    s.execute("edit.undo", &json!({})).unwrap();
    // Removing the layer removes its weights; a new layer named after the old id doesn't get them.
    let r = s.execute("layer.remove", &json!({ "layer": "Cidades" })).unwrap();
    assert!(r["message"].as_str().unwrap().contains("1 spatial weights"), "{r}");
    assert!(s.map.weights.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("map.addSample", &json!({})).unwrap();
    let copy = s.map.layers.iter().find(|l| l.name == "Cidades" && l.id != id).map(|l| l.id).unwrap();
    s.execute("layer.rename", &json!({ "layer": copy, "name": id.to_string() })).unwrap();
    s.map.weights.retain(|w| w.name == "Cidades_knn");
    s.execute("layer.remove", &json!({ "layer": id })).unwrap();
    assert!(s.map.weights.is_empty(), "the weights went with layer {id}, not to the layer named \"{id}\"");
    // Ids: unique after spaces become `_`, numbers in full; a Text id round-trips.
    s.execute("stats.weights", &json!({ "layer": copy })).unwrap();
    let w = s.map.weights.first().map(|w| w.name.clone()).unwrap();
    s.execute("stats.exportWeights", &json!({ "weights": w, "path": gal, "idField": "NOME" })).unwrap();
    assert!(std::fs::read_to_string(&gal).unwrap().contains("Porto_Celeste"));
    let back = s.execute("stats.importWeights", &json!({ "layer": copy, "path": gal, "idField": "NOME" })).unwrap();
    let names = [w.clone(), back["name"].as_str().unwrap().to_string()];
    let lists: Vec<_> = names.iter().map(|n| s.map.weights.iter().find(|x| &x.name == n).unwrap().neighbours.clone()).collect();
    assert_eq!(lists[0], lists[1]);
    let err = s.execute("stats.exportWeights", &json!({ "weights": w, "path": gal, "idField": "ILHA" })).unwrap_err().to_string();
    assert!(err.contains("same id"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #6: a `.prj` MK aeGIS doesn't know used to be read as WGS 84.
#[test]
fn an_unknown_coordinate_system_is_kept_until_defined() {
    let dir = scratch("unknown-crs");
    let mut s = Session::sample();
    let file = dir.join("cidades.shp");
    s.execute("layer.export", &json!({ "layer": "Cidades", "path": file.display().to_string() })).unwrap();
    let prj = r#"PROJCS["Some_Local_Grid",GEOGCS["GCS_Unknown"],PROJECTION["Oblique_Stereographic"],UNIT["Meter",1.0]]"#;
    std::fs::write(file.with_extension("prj"), prj).unwrap();
    let r = s.execute("map.addData", &json!({ "path": file.display().to_string() })).unwrap();
    assert!(r.to_string().contains("Some_Local_Grid"), "{r}");
    let id = r["layer"].to_string();
    let info = s.execute("layer.info", &json!({ "layer": id })).unwrap();
    assert_eq!(info["crs"]["known"], json!(false), "{info}");
    assert_eq!(info["crs"]["prj"], json!(prj));
    let err = s.execute("analysis.buffer", &json!({ "layer": id, "distance": 10 })).unwrap_err().to_string();
    assert!(err.contains("Define Projection"), "{err}");
    assert!(s.execute("management.project", &json!({ "layer": id, "epsg": 4326 })).is_err());

    // The `.prj` and the unknown state survive a project and an export.
    let project = dir.join("unknown.aegis");
    s.execute("project.saveAs", &json!({ "path": project.display().to_string() })).unwrap();
    let mut t = Session::default();
    t.execute("project.open", &json!({ "path": project.display().to_string() })).unwrap();
    assert_eq!(t.map, s.map);
    let again = dir.join("again.shp");
    s.execute("layer.export", &json!({ "layer": id, "path": again.display().to_string() })).unwrap();
    assert_eq!(std::fs::read_to_string(again.with_extension("prj")).unwrap(), prj);

    // Define Projection sets it without moving the coordinates, and undoes.
    let before = s.map.layer(&id).unwrap().data.extent();
    s.execute("management.defineProjection", &json!({ "layer": id, "epsg": 4326 })).unwrap();
    let layer = s.map.layer(&id).unwrap();
    assert_eq!((layer.data.crs, layer.data.prj.clone(), layer.data.extent()), (Crs::WGS84, None, before));
    s.execute("analysis.buffer", &json!({ "layer": id, "distance": 10 })).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.map.layer(&id).unwrap().data.crs, Crs::UNKNOWN);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #23: global autocorrelation of a field, by the weights' layer.
#[test]
fn global_autocorrelation_reports_its_inference() {
    let mut s = Session::sample();
    let r = s.execute("stats.weights", &json!({ "layer": "Cidades", "method": "knn", "k": 4, "name": "w" })).unwrap();
    assert_eq!(r["name"], json!("w"));
    let moran = s.execute("stats.globalMoran", &json!({ "weights": "w", "field": "POP" })).unwrap();
    // The same numbers as the statistics crate gives directly.
    let layer = s.map.layer("Cidades").unwrap();
    let pop: Vec<f64> = layer.data.features.iter().map(|f| f.values[1].as_f64().unwrap()).collect();
    let direct =
        mkaegis_stats::autocorrelation::global(mkaegis_stats::autocorrelation::Statistic::MoranI, &pop, &s.map.weights[0].neighbours, 999, 12_345)
            .unwrap();
    assert_eq!(moran["value"], json!(direct.value));
    assert_eq!(moran["expected"], json!(-1.0 / (pop.len() as f64 - 1.0)));
    assert_eq!((moran["permutation"]["permutations"].clone(), moran["permutation"]["seed"].clone()), (json!(999), json!(12_345)));
    assert!(moran["normality"]["p"].as_f64().is_some() && moran["randomization"]["z"].as_f64().is_some());
    assert!(moran["message"].as_str().unwrap().starts_with("Moran's I of POP = "), "{}", moran["message"]);
    // Reproducible, and a different seed draws differently.
    assert_eq!(s.execute("stats.globalMoran", &json!({ "weights": "w", "field": "POP" })).unwrap(), moran);
    let other = s.execute("stats.globalMoran", &json!({ "weights": "w", "field": "POP", "seed": 7 })).unwrap();
    assert_ne!(other["permutation"], moran["permutation"]);
    let none = s.execute("stats.globalMoran", &json!({ "weights": "w", "field": "POP", "permutations": 0 })).unwrap();
    assert_eq!(none["permutation"], Json::Null);
    let geary = s.execute("stats.gearyC", &json!({ "weights": "w", "field": "POP" })).unwrap();
    assert_eq!((geary["statistic"].clone(), geary["expected"].clone()), (json!("Geary's C"), json!(1.0)));
    let g = s.execute("stats.generalG", &json!({ "weights": "w", "field": "POP" })).unwrap();
    assert_eq!(g["normality"], Json::Null);

    // Refused: a text field, a NULL, too many permutations, unknown weights.
    assert!(s.execute("stats.globalMoran", &json!({ "weights": "w", "field": "NOME" })).is_err());
    assert!(s.execute("stats.globalMoran", &json!({ "weights": "w", "field": "POP", "permutations": 100_000 })).is_err());
    assert!(s.execute("stats.globalMoran", &json!({ "weights": "nope", "field": "POP" })).is_err());
    s.execute("map.selectByAttributes", &json!({ "layer": "Cidades", "where": "NOME = 'Rochedo'" })).unwrap();
    s.execute("layer.calculateField", &json!({ "layer": "Cidades", "field": "POP", "expression": "NULL", "selectedOnly": true })).unwrap();
    let err = s.execute("stats.globalMoran", &json!({ "weights": "w", "field": "POP" })).unwrap_err().to_string();
    assert!(err.contains("1 features have no `POP`"), "{err}");
}

/// #24: local statistics write their results as fields and map them, and re-run cleanly.
#[test]
fn local_statistics_write_fields_and_map_them() {
    let mut s = Session::sample();
    s.execute("stats.weights", &json!({ "layer": "Cidades", "method": "knn", "k": 4, "name": "w" })).unwrap();
    let r = s.execute("stats.localMoran", &json!({ "weights": "w", "field": "POP", "correction": "fdr" })).unwrap();
    assert!(r["message"].as_str().unwrap().starts_with("Local Moran's I of POP: "), "{}", r["message"]);
    assert_eq!(r["summary"]["correction"], json!("fdr"));
    let layer = s.map.layer("Cidades").unwrap();
    for f in ["LISA_I", "LISA_Z", "LISA_P", "LISA_CL"] {
        assert!(layer.data.field_index(f).is_some(), "{f}");
    }
    assert_eq!(layer.renderer.field(), Some("LISA_CL"));
    // Σ Iᵢ = n · I with row-standardised weights.
    let i = layer.data.field_index("LISA_I").unwrap();
    let sum: f64 = layer.data.features.iter().filter_map(|f| f.values[i].as_f64()).sum();
    let n = layer.data.features.len() as f64;
    let global = s.execute("stats.globalMoran", &json!({ "weights": "w", "field": "POP", "permutations": 0 })).unwrap();
    assert!((sum - n * global["value"].as_f64().unwrap()).abs() < 1e-9);
    // A re-run overwrites its fields instead of adding more; Undo takes the run back.
    let fields = s.map.layer("Cidades").unwrap().data.fields.len();
    s.execute("stats.localMoran", &json!({ "weights": "w", "field": "POP" })).unwrap();
    assert_eq!(s.map.layer("Cidades").unwrap().data.fields.len(), fields);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.map.layer("Cidades").unwrap().data.field_index("LISA_I").is_none());

    let h = s.execute("stats.hotSpots", &json!({ "weights": "w", "field": "POP" })).unwrap();
    assert!(h["hot"].as_u64().is_some() && h["cold"].as_u64().is_some());
    let layer = s.map.layer("Cidades").unwrap();
    assert_eq!(layer.renderer.field(), Some("GI_CL"));
    let bin = layer.data.field_index("GI_BIN").unwrap();
    assert!(layer.data.features.iter().all(|f| (-3.0..=3.0).contains(&f.values[bin].as_f64().unwrap())));
    // A field of the same name and another type is not overwritten.
    s.execute("layer.addField", &json!({ "layer": "Ilhas", "name": "LISA_CL", "type": "double" })).unwrap();
    s.execute("stats.weights", &json!({ "layer": "Ilhas", "method": "knn", "k": 1, "name": "wi" })).unwrap();
    assert!(s.execute("stats.localMoran", &json!({ "weights": "wi", "field": "LISA_CL" })).is_err());
    assert!(s.execute("stats.localMoran", &json!({ "weights": "w", "field": "POP", "correction": "holm" })).is_err());
    assert!(s.execute("stats.localMoran", &json!({ "weights": "w", "field": "POP", "alpha": 1.5 })).is_err());
}

/// #26: every classification reports its fit and class counts; named classes survive a project.
#[test]
fn classifications_report_their_fit() {
    let mut s = Session::sample();
    for method in ["equalInterval", "quantile", "naturalBreaks", "geometricInterval", "standardDeviation", "boxMap", "percentile"] {
        let r = s
            .execute("layer.symbology", &json!({ "layer": "Cidades", "renderer": "graduatedColors", "field": "POP", "method": method, "classes": 4 }))
            .unwrap();
        let gvf = r["gvf"].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&gvf), "{method}: {gvf}");
        let counts: u64 = r["counts"].as_array().unwrap().iter().map(|c| c.as_u64().unwrap()).sum();
        assert_eq!(counts, 23, "{method}: every city in a class");
    }
    let manual = s.execute("layer.symbology", &json!({ "layer": "Cidades", "method": "manual", "breaks": [10000, 100000] })).unwrap();
    assert_eq!(manual["classes"], json!(3));
    assert!(s.execute("layer.symbology", &json!({ "layer": "Cidades", "method": "manual" })).is_err());
    assert!(s.execute("layer.symbology", &json!({ "layer": "Cidades", "method": "manual", "breaks": [5, 1] })).is_err());
    assert!(s.execute("layer.symbology", &json!({ "layer": "Cidades", "method": "jenkins" })).is_err());
    s.execute("layer.symbology", &json!({ "layer": "Cidades", "method": "boxMap", "hinge": 3 })).unwrap();
    let info = s.execute("layer.info", &json!({ "layer": "Cidades" })).unwrap();
    assert_eq!(info["legend"][0]["label"], json!("Lower outlier"));
    let dir = scratch("classes");
    let file = dir.join("box.aegis");
    s.execute("project.saveAs", &json!({ "path": file.display().to_string() })).unwrap();
    let mut t = Session::default();
    t.execute("project.open", &json!({ "path": file.display().to_string() })).unwrap();
    assert_eq!(t.map, s.map);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #25: the charts' statistics are commands too, so scripts and agents get the same numbers.
#[test]
fn chart_statistics_are_queries() {
    let mut s = Session::sample();
    let h = s.execute("stats.histogram", &json!({ "layer": "Cidades", "field": "POP", "bins": "5" })).unwrap();
    assert_eq!(h["counts"].as_array().unwrap().len(), 5);
    assert_eq!(h["counts"].as_array().unwrap().iter().map(|c| c.as_u64().unwrap()).sum::<u64>(), 23);
    let b = s.execute("stats.boxPlot", &json!({ "layer": "Cidades", "field": "POP" })).unwrap();
    assert!(b["q1"].as_f64().unwrap() <= b["median"].as_f64().unwrap() && b["median"].as_f64().unwrap() <= b["q3"].as_f64().unwrap());
    let f = s.execute("stats.scatterPlot", &json!({ "layer": "Cidades", "x": "POP", "y": "POP" })).unwrap();
    assert_eq!((f["slope"].as_f64().unwrap(), f["r2"].as_f64().unwrap()), (1.0, 1.0));
    s.execute("stats.weights", &json!({ "layer": "Cidades", "method": "knn", "k": 4, "name": "w" })).unwrap();
    let m = s.execute("stats.moranScatter", &json!({ "weights": "w", "field": "POP" })).unwrap();
    let i = s.execute("stats.globalMoran", &json!({ "weights": "w", "field": "POP", "permutations": 0 })).unwrap();
    assert!((m["slope"].as_f64().unwrap() - i["value"].as_f64().unwrap()).abs() < 1e-12);
    // Queries leave the map, its undo history and its revision alone.
    let revision = s.revision;
    s.execute("stats.histogram", &json!({ "layer": "Cidades", "field": "POP" })).unwrap();
    assert_eq!(s.revision, revision);
    assert!(s.execute("stats.histogram", &json!({ "layer": "Cidades", "field": "NOME" })).is_err());
    assert!(s.execute("stats.histogram", &json!({ "layer": "Cidades", "field": "POP", "bins": "0" })).is_err());
}

/// #27: a CSV table joins by key with an exact report; codes keep their zeros; a table of
/// coordinates becomes points.
#[test]
fn csv_tables_join_and_become_points() {
    let dir = scratch("csv");
    let mut s = Session::sample();
    let table = dir.join("indicadores.csv");
    // Rochedo twice (a duplicate key), Nowhere (unused), every other city missing.
    std::fs::write(&table, "CIDADE;COD;TAXA\nRochedo;0012;1,5\nRochedo;0013;9,9\nMirante;0021;2,25\nNowhere;0099;3\n").unwrap();
    let r = s
        .execute("management.joinField", &json!({ "layer": "Cidades", "field": "NOME", "path": table.display().to_string(), "joinField": "CIDADE" }))
        .unwrap();
    assert_eq!(
        (r["matched"].clone(), r["unmatched"].clone(), r["unused"].clone(), r["duplicate_keys"].clone()),
        (json!(2), json!(21), json!(1), json!(1))
    );
    assert_eq!(r["fields"], json!(["COD", "TAXA"]));
    assert!(r["warnings"][0].as_str().unwrap().contains("first row was used"));
    let layer = s.map.layer("Cidades").unwrap();
    let (nome, cod, taxa) =
        (layer.data.field_index("NOME").unwrap(), layer.data.field_index("COD").unwrap(), layer.data.field_index("TAXA").unwrap());
    let rochedo = layer.data.features.iter().find(|f| f.values[nome] == mkaegis_core::Value::Text("Rochedo".into())).unwrap();
    assert_eq!(
        (rochedo.values[cod].clone(), rochedo.values[taxa].clone()),
        (mkaegis_core::Value::Text("0012".into()), mkaegis_core::Value::Float(1.5))
    );
    // The code survives an export to CSV and back.
    let out = dir.join("cidades.csv");
    s.execute("layer.export", &json!({ "layer": "Cidades", "path": out.display().to_string() })).unwrap();
    let back = crate::io::read_table(&Disk::Native, &out, &Default::default()).unwrap();
    let c = back.fields.iter().position(|f| f.name == "COD").unwrap();
    assert!(back.rows.iter().any(|r| r[c] == mkaegis_core::Value::Text("0012".into())));
    // Join from another layer, then refusals.
    assert!(s.execute("management.joinField", &json!({ "layer": "Cidades", "field": "NOME", "joinField": "CIDADE" })).is_err());
    assert!(
        s.execute("management.joinField", &json!({ "layer": "Cidades", "field": "NOME", "path": table.display().to_string(), "joinField": "NOPE" }))
            .is_err()
    );

    let xy = dir.join("pontos.csv");
    std::fs::write(&xy, "ID,LON,LAT\n001,-46.6,-23.5\n002,-43.2,-22.9\n003,,\n004,200,95\n").unwrap();
    let r = s.execute("management.xyTableToPoint", &json!({ "path": xy.display().to_string(), "x": "LON", "y": "LAT" })).unwrap();
    let layer = s.map.layer(&r["layer"].to_string()).unwrap();
    assert_eq!(layer.data.features.len(), 2);
    assert_eq!(layer.data.features[0].values[0], mkaegis_core::Value::Text("001".into()));
    assert!(r["warnings"].to_string().contains("2 rows"), "{}", r["warnings"]);
    let wkt = dir.join("wkt.csv");
    std::fs::write(&wkt, "NAME,SHAPE\na,\"POLYGON ((0 0, 1 0, 1 1, 0 0))\"\nb,nonsense\n").unwrap();
    let r = s.execute("management.xyTableToPoint", &json!({ "path": wkt.display().to_string(), "wkt": "SHAPE" })).unwrap();
    assert_eq!(s.map.layer(&r["layer"].to_string()).unwrap().data.geometry_type, mkaegis_geom::GeometryType::Polygon);
    assert!(s.execute("management.xyTableToPoint", &json!({ "path": xy.display().to_string(), "x": "LON" })).is_err());
    let _ = std::fs::remove_dir_all(&dir);
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
    let project = dir.join("sample.aegis");
    s.execute("project.saveAs", &json!({ "path": project.display().to_string() })).unwrap();
    assert!(!s.modified);
    let mut t = Session::default();
    t.execute("project.open", &json!({ "path": project.display().to_string() })).unwrap();
    assert_eq!(t.map, s.map);
    assert!(!t.modified, "a just-opened project has nothing unsaved");
    assert!(t.execute("edit.undo", &json!({})).is_err(), "opening a project is not undoable");
    t.execute("project.new", &json!({})).unwrap();
    assert!(!t.modified);
    let png = dir.join("map.png");
    s.execute("share.exportMap", &json!({ "path": png.display().to_string(), "width": 320, "height": 200 })).unwrap();
    assert!(std::fs::metadata(&png).unwrap().len() > 1000);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kml_mixing_points_and_polygons_adds_a_layer_for_each() {
    let mut s = Session::sample();
    s.disk = Disk::Memory(Default::default());
    s.execute("layer.export", &json!({ "layer": "Ilhas", "path": "ilhas.kml" })).unwrap();
    let r = s.execute("map.addData", &json!({ "path": "ilhas.kml" })).unwrap();
    assert_eq!(s.map.layer(&r["layer"].to_string()).unwrap().data.features.len(), 3);
    // A parcel and its vertices, as SIGEF exports them: two layers, the polygons first.
    let kml = r#"<kml xmlns="http://www.opengis.net/kml/2.2"><Document>
        <Placemark><name>Lote</name><Polygon><outerBoundaryIs><LinearRing><coordinates>-40,-9 -39.9,-9 -39.9,-8.9 -40,-9</coordinates></LinearRing></outerBoundaryIs></Polygon></Placemark>
        <Folder><Placemark><name>V1</name><Point><coordinates>-40,-9</coordinates></Point></Placemark>
        <Placemark><name>V2</name><Point><coordinates>-39.9,-9</coordinates></Point></Placemark></Folder>
    </Document></kml>"#;
    s.disk.memory_mut().unwrap().put("sitio.kml", kml.as_bytes().to_vec()).unwrap();
    let r = s.execute("map.addData", &json!({ "path": "sitio.kml" })).unwrap();
    assert_eq!(r["message"], "added 3 features in 2 layers");
    let names: Vec<String> = r["layers"].as_array().unwrap().iter().map(|id| s.map.layer(&id.to_string()).unwrap().name.clone()).collect();
    assert_eq!(names, ["sitio_Polygons", "sitio_Points"]);
    assert_eq!(r["layer"], r["layers"][0]);
}

#[test]
fn files_in_memory_take_the_same_commands_as_files_on_disk() {
    // The browser: what a person picks goes in, what commands write comes back out.
    let mut s = Session { disk: Disk::Memory(Default::default()), ..Session::sample() };
    let written =
        |s: &mut Session| -> Vec<String> { s.disk.memory_mut().unwrap().take_written().into_iter().map(|(p, _)| p.display().to_string()).collect() };
    s.execute("layer.export", &json!({ "layer": "Ilhas", "path": "ilhas.shp" })).unwrap();
    assert_eq!(written(&mut s), ["ilhas.shp", "ilhas.shx", "ilhas.dbf", "ilhas.prj", "ilhas.cpg"]);
    let r = s.execute("map.addData", &json!({ "path": "ilhas.shp" })).unwrap();
    assert_eq!(s.map.layer(&r["layer"].to_string()).unwrap().data.features.len(), 3);

    let geojson = mkaegis_geojson::write(&s.map.layer("Cidades").unwrap().data).unwrap();
    s.disk.memory_mut().unwrap().put("picked.geojson", geojson.into_bytes()).unwrap();
    let r = s.execute("map.addData", &json!({ "path": "picked.geojson" })).unwrap();
    assert_eq!(s.map.layer(&r["layer"].to_string()).unwrap().data.features.len(), 23);
    assert!(written(&mut s).is_empty(), "a picked file is the person's own, not a download");
    assert!(s.execute("map.addData", &json!({ "path": "nowhere.geojson" })).is_err());

    s.execute("stats.weights", &json!({ "layer": "Cidades" })).unwrap();
    s.execute("stats.exportWeights", &json!({ "weights": "Cidades_knn", "path": "knn.gal" })).unwrap();
    s.execute("stats.importWeights", &json!({ "layer": "Cidades", "path": "knn.gal" })).unwrap();
    s.execute("share.exportMap", &json!({ "path": "map.png", "width": 320, "height": 200 })).unwrap();
    s.execute("project.saveAs", &json!({ "path": "sample.aegis" })).unwrap();
    assert_eq!(written(&mut s), ["knn.gal", "map.png", "sample.aegis"]);
    // Save writes the project again, under its name; Open reads it back.
    s.execute("project.save", &json!({})).unwrap();
    assert_eq!(written(&mut s), ["sample.aegis"]);
    let map = s.map.clone();
    s.execute("project.new", &json!({})).unwrap();
    s.execute("project.open", &json!({ "path": "sample.aegis" })).unwrap();
    assert_eq!(s.map, map);
}

#[test]
fn undo_never_puts_an_old_map_under_an_opened_projects_path() {
    let dir = scratch("undo-open");
    let project = dir.join("opened.aegis");
    let mut s = Session::sample();
    s.execute("project.saveAs", &json!({ "path": project.display().to_string() })).unwrap();
    let saved = std::fs::read(&project).unwrap();
    // Edits in another session, then the project is opened over them.
    let mut t = Session::default();
    t.execute("map.addSample", &json!({})).unwrap();
    t.execute("layer.remove", &json!({ "layer": "Rios" })).unwrap();
    t.execute("project.open", &json!({ "path": project.display().to_string() })).unwrap();
    assert!(t.can_undo().is_none() && t.can_redo().is_none(), "a fresh history");
    assert!(t.execute("edit.undo", &json!({})).is_err());
    t.execute("project.save", &json!({})).unwrap();
    assert_eq!(std::fs::read(&project).unwrap(), saved, "the opened file is untouched");
    // The same after New: nothing to undo back into.
    t.execute("layer.remove", &json!({ "layer": "Rios" })).unwrap();
    t.execute("project.new", &json!({})).unwrap();
    assert!(t.execute("edit.undo", &json!({})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undoing_a_crs_change_refits_the_view() {
    let mut s = Session::sample();
    // The sample map is in Web Mercator (metres); 4326 is in degrees.
    s.execute("map.setCrs", &json!({ "epsg": 4326 })).unwrap();
    let serial = s.view_serial;
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.view_serial > serial, "an extent in degrees doesn't fit a map in metres");
    let serial = s.view_serial;
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(s.view_serial > serial);
    // An undo that keeps the CRS keeps the view where the person left it.
    s.execute("layer.remove", &json!({ "layer": "Rios" })).unwrap();
    let serial = s.view_serial;
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.view_serial, serial);
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

/// #7: `POP / 0` wiped the column to NULL and reported success.
#[test]
fn calculate_field_never_silently_wipes_a_column() {
    let mut s = Session::sample();
    let before = s.map.clone();
    let err = s.execute("layer.calculateField", &json!({ "layer": "Cidades", "field": "POP", "expression": "POP / 0" })).unwrap_err();
    assert!(err.to_string().contains("NULL on every row"), "{err}");
    assert_eq!(s.map, before, "nothing written");
    // Some NULLs are reported, not refused.
    s.execute("feature.setValue", &json!({ "layer": "Cidades", "row": 0, "field": "POP", "value": null })).unwrap();
    let r = s.execute("layer.calculateField", &json!({ "layer": "Cidades", "field": "POP", "expression": "POP * 2" })).unwrap();
    assert_eq!((r["calculated"].as_u64(), r["nulls"].as_u64()), (Some(23), Some(1)));
    assert!(r["message"].as_str().unwrap().ends_with("1 set to NULL"), "{r}");
    // Emptying a field on purpose still works.
    let r = s.execute("layer.calculateField", &json!({ "layer": "Cidades", "field": "POP", "expression": "null" })).unwrap();
    assert_eq!(r["nulls"].as_u64(), Some(23));
}

/// #59: a panic in any command that changes the map restores it; the restore path runs.
#[test]
fn a_panicking_command_leaves_the_map_as_it_was() {
    let mut s = Session::sample();
    let before = s.map.clone();
    for id in ["test.editThenPanic", "test.selectThenPanic"] {
        let r = s.execute(id, &json!({}));
        assert!(panicked(&r), "{id}: {r:?}");
        assert_eq!(s.map, before, "{id}");
    }
    assert!(s.can_undo().is_none(), "a failed command is no undo step");
}

/// #59: undo snapshots share every layer a step didn't change, and count only what it did.
#[test]
fn undo_shares_unchanged_layers_and_counts_changed_ones() {
    let mut s = Session::sample();
    for _ in 0..10 {
        s.execute("layer.setVisible", &json!({ "layer": "Ilhas", "visible": false })).unwrap();
        s.execute("layer.setVisible", &json!({ "layer": "Ilhas", "visible": true })).unwrap();
    }
    assert_eq!(s.undo.len(), 20);
    assert!(s.undo.iter().all(|step| step.bytes == 0), "toggling visibility copies no features");
    for step in &s.undo {
        for (a, b) in step.map.layers.iter().zip(&s.map.layers) {
            assert!(std::sync::Arc::ptr_eq(&a.data, &b.data));
        }
    }
    s.execute("layer.calculateField", &json!({ "layer": "Cidades", "field": "POP", "expression": "POP * 2" })).unwrap();
    let cidades = s.map.layer("Cidades").unwrap().data.approx_bytes();
    assert_eq!(s.undo.last().map(|step| step.bytes), Some(cidades), "only the changed layer is held");
    // Past the budget, the oldest steps go and the newest stays.
    s.trim_undo(cidades / 2);
    assert_eq!(s.undo.len(), 1);
    assert_eq!(s.can_undo(), Some("Calculate Field"));
}

/// #59: undoing back to the saved state leaves nothing unsaved; redoing away from it does.
#[test]
fn undo_back_to_the_saved_state_is_not_modified() {
    let dir = scratch("saved-state");
    let mut s = Session::sample();
    s.execute("project.saveAs", &json!({ "path": dir.join("p.aegis").display().to_string() })).unwrap();
    s.execute("layer.rename", &json!({ "layer": "Cidades", "name": "Towns" })).unwrap();
    assert!(s.modified);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!s.modified, "back where it was saved");
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(s.modified);
    // Saved after the edit: undoing goes away from the saved state.
    s.execute("project.save", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.modified);
    let _ = std::fs::remove_dir_all(&dir);
}
