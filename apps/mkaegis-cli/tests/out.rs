//! `mkaegis-cli run --out` writes the layer the commands made, not the top one (#3).

use std::process::Command;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mkaegis-cli-test-{}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mkaegis-cli")).arg("run").args(args).output().unwrap()
}

#[test]
fn out_writes_the_commands_output() {
    let dir = scratch("out");
    let file = dir.join("buffers.geojson");
    let out = run(&["--sample", "--cmd", r#"analysis.buffer={"layer":"Cidades","distance":5,"unit":"km"}"#, "--out", file.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"Cidades_Buffer\"") && text.contains("\"Polygon\"") && !text.contains("\"Point\""), "{}", &text[..200.min(text.len())]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("(layer Cidades_Buffer)"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn out_layer_names_the_layer() {
    let dir = scratch("out-layer");
    let file = dir.join("rios.geojson");
    let out = run(&[
        "--sample",
        "--cmd",
        r#"analysis.buffer={"layer":"Cidades","distance":5,"unit":"km"}"#,
        "--out",
        file.to_str().unwrap(),
        "--out-layer",
        "Rios",
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(std::fs::read_to_string(&file).unwrap().contains("\"Rios\""));
    let bad = run(&["--sample", "--out", file.to_str().unwrap(), "--out-layer", "Nowhere"]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("--out-layer"));
    let _ = std::fs::remove_dir_all(&dir);
}
