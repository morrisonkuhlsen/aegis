//! MK aeGIS's headless command line.
//!
//! - `mkaegis-cli info FILE` — a data file's geometry type, CRS, fields and extent.
//! - `mkaegis-cli run [--sample] [--in FILE]… [--cmd 'id={json}']… [--out FILE [--out-layer NAME]]
//!   [--png FILE]` — runs commands in order; `--out` saves a project (`.aegis`) or a layer
//!   (`.geojson`, `.shp`): `--out-layer`'s, else the last one a command made or added, else the
//!   top one; `--png` exports the map.
//! - `mkaegis-cli commands` — every command with its parameters.
//! - `mkaegis-cli mcp [--sample]` — a Model Context Protocol server on stdio.
//! - `mkaegis-cli --version` — the version, `mkaegis-cli X.Y.Z`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

use mkaegis_engine::Session;
use serde_json::{Value as Json, json};

const USAGE: &str = "usage: mkaegis-cli info FILE | run [--sample] [--in FILE]… [--cmd 'id={json}']… [--out FILE [--out-layer NAME]] [--png FILE] | commands | mcp [--sample] | --version";

fn info(path: &str) -> Result<(), String> {
    let mut s = Session::default();
    s.execute("map.addData", &json!({ "path": path })).map_err(|e| e.to_string())?;
    let layer = s.map.layers.first().map(|l| l.id).ok_or("no layer")?;
    let v = s.execute("layer.info", &json!({ "layer": layer })).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?);
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let mut s = Session::default();
    let (mut out, mut out_layer, mut png) = (None, None, None);
    // The layer the last command made or added (its result's numeric `layer` id): what `--out`
    // writes (#3). New layers are inserted by geometry type, so the top one is often not it.
    let mut last_layer: Option<u64> = None;
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        let mut value = || {
            i += 1;
            args.get(i).cloned().ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--sample" => {
                s.execute("map.addSample", &json!({})).map_err(|e| e.to_string())?;
            }
            "--in" => {
                let path = value()?;
                let cmd = if path.ends_with(".aegis") { "project.open" } else { "map.addData" };
                let r = s.execute(cmd, &json!({ "path": path })).map_err(|e| format!("{path}: {e}"))?;
                last_layer = r.get("layer").and_then(Json::as_u64).or(last_layer);
                for w in r.get("warnings").and_then(Json::as_array).into_iter().flatten() {
                    eprintln!("warning: {path}: {}", w.as_str().unwrap_or_default());
                }
            }
            "--cmd" => {
                let c = value()?;
                let (id, params) = c.split_once('=').unwrap_or((c.as_str(), "{}"));
                let params: Json = serde_json::from_str(params).map_err(|e| format!("{id}: bad JSON: {e}"))?;
                let r = s.execute(id, &params).map_err(|e| format!("{id}: {e}"))?;
                last_layer = r.get("layer").and_then(Json::as_u64).or(last_layer);
                println!("{r}");
            }
            "--out" => out = Some(value()?),
            "--out-layer" => out_layer = Some(value()?),
            "--png" => png = Some(value()?),
            other => return Err(format!("unknown argument `{other}`\n{USAGE}")),
        }
        i += 1;
    }
    if let Some(path) = out {
        if path.ends_with(".aegis") {
            s.execute("project.saveAs", &json!({ "path": path })).map_err(|e| e.to_string())?;
            eprintln!("wrote {path}");
        } else {
            let layer = match &out_layer {
                Some(name) => s.map.layer(name).map_err(|e| format!("--out-layer: {e}"))?,
                None => {
                    // A later command may have removed the layer the last one made.
                    let made = last_layer.and_then(|id| s.map.layer(&id.to_string()).ok());
                    made.or_else(|| s.map.layers.first()).ok_or("no layer to write")?
                }
            };
            let (id, name) = (layer.id, layer.name.clone());
            let r = s.execute("layer.export", &json!({ "layer": id, "path": path })).map_err(|e| e.to_string())?;
            for w in r.get("warnings").and_then(Json::as_array).into_iter().flatten() {
                eprintln!("warning: {path}: {}", w.as_str().unwrap_or_default());
            }
            eprintln!("wrote {path} (layer {name})");
        }
    }
    if let Some(path) = png {
        if s.map.extent.is_none() {
            let _ = s.execute("map.fullExtent", &json!({}));
        }
        s.execute("share.exportMap", &json!({ "path": path })).map_err(|e| e.to_string())?;
        eprintln!("wrote {path}");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("info") => args.get(1).ok_or_else(|| USAGE.to_string()).and_then(|p| info(p)),
        Some("run") => run(args.get(1..).unwrap_or_default()),
        Some("commands") => {
            for c in Session::commands() {
                println!("{:<32} {:<30} {}", c.id, c.ribbon, c.params);
            }
            Ok(())
        }
        Some("--version" | "-V" | "version") => {
            println!("mkaegis-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("mcp") => {
            let session = if args.iter().any(|a| a == "--sample") { Session::sample() } else { Session::default() };
            mkaegis_mcp::serve(session).map_err(|e| e.to_string())
        }
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
