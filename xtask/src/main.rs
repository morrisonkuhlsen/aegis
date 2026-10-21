//! MK aeGIS's project tasks, run with `cargo xtask <task>`:
//!
//! - `ci`: every quality gate before a commit: fmt, clippy (warnings are errors), tests,
//!   assets, layers, panics, wasm. Stops at the first that fails.
//! - `layers`: each crate depends only on crates at its layer or below, and nothing below the
//!   UI layer on egui, eframe, winit, wgpu or rfd.
//! - `assets`: every asset file has its row in `ATTRIBUTION.md`.
//! - `panics`: every crate's root denies unwrap, expect, panic, todo, unimplemented and
//!   unreachable outside tests.
//! - `wasm`: the engine and the UI still build for WebAssembly.
//! - `parity`: regenerates `docs/parity-checklist.md` from the command registry.
//! - `contributors [--check]`: regenerates `contributors/contributors.json` from the commits:
//!   people by GitHub username (from their noreply address, else their author name) and the AI
//!   models credited in `Co-Authored-By` trailers.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Each internal crate's layer: L0 `core`/`geom`/`crs` → L1 `model` → L2 `geojson`/`shp`/`render`
/// → L3 `analysis` → L4 `engine` → L5 `ui-egui`/`mcp` → L6 `apps/*`.
const LAYERS: [(&str, u8); 14] = [
    ("aegis-core", 0),
    ("aegis-geom", 0),
    ("aegis-crs", 0),
    ("aegis-model", 1),
    ("aegis-geojson", 2),
    ("aegis-shp", 2),
    ("aegis-render", 2),
    ("aegis-analysis", 3),
    ("aegis-stats", 3),
    ("aegis-engine", 4),
    ("aegis-ui-egui", 5),
    ("aegis-mcp", 5),
    ("aegis", 6),
    ("aegis-cli", 6),
];

/// The first layer allowed to use the UI toolkit.
const UI_LAYER: u8 = 5;

/// Crates only the UI layer and above may use.
const UI_ONLY: [&str; 5] = ["egui", "eframe", "winit", "wgpu", "rfd"];

/// Files under these folders are assets: each needs a row in ATTRIBUTION.md.
const ASSET_DIRS: [&str; 3] = ["assets", "crates/ui-egui/assets", "docs/brand"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default()
}

fn layer(name: &str) -> Option<u8> {
    LAYERS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

/// A manifest's package name and the names in its `[dependencies]` (not dev or build ones).
fn manifest(path: &Path) -> Result<(String, Vec<String>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut section = String::new();
    let (mut name, mut deps) = (None, Vec::new());
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        if section == "[package]" && key == "name" {
            name = Some(value.trim().trim_matches('"').to_string());
        } else if section == "[dependencies]" && !key.is_empty() && !key.starts_with('#') {
            deps.push(key.to_string());
        }
    }
    Ok((name.ok_or_else(|| format!("{}: no package name", path.display()))?, deps))
}

fn layers() -> Result<(), String> {
    let root = root();
    let mut problems = Vec::new();
    for group in ["crates", "apps"] {
        let entries = std::fs::read_dir(root.join(group)).map_err(|e| format!("{group}: {e}"))?;
        for entry in entries.flatten() {
            let path = entry.path().join("Cargo.toml");
            if !path.exists() {
                continue;
            }
            let (name, deps) = manifest(&path)?;
            let Some(own) = layer(&name) else {
                problems.push(format!("{name} has no layer: add it to xtask's LAYERS and to AGENTS.md"));
                continue;
            };
            for dep in deps {
                if let Some(theirs) = layer(&dep).filter(|theirs| *theirs > own) {
                    problems.push(format!("{name} (L{own}) depends on {dep} (L{theirs})"));
                }
                if own < UI_LAYER && UI_ONLY.contains(&dep.as_str()) {
                    problems.push(format!("{name} (L{own}) depends on {dep}, which only the UI layer may use"));
                }
            }
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join("\n")) }
}

fn assets() -> Result<(), String> {
    let root = root();
    let attribution = std::fs::read_to_string(root.join("ATTRIBUTION.md")).map_err(|e| format!("ATTRIBUTION.md: {e}"))?;
    let mut missing = Vec::new();
    for dir in ASSET_DIRS {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { continue };
        for entry in entries.flatten() {
            let file = format!("{dir}/{}", entry.file_name().to_string_lossy());
            // A row names the file in backticks.
            if !attribution.contains(&format!("`{file}`")) {
                missing.push(file);
            }
        }
    }
    if missing.is_empty() { Ok(()) } else { Err(format!("assets without a row in ATTRIBUTION.md: {}", missing.join(", "))) }
}

/// The attribute every crate's root carries, so clippy refuses a panic outside tests.
const NO_PANICS: &str = "#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]";

fn panics() -> Result<(), String> {
    let root = root();
    let mut missing = Vec::new();
    for (group, file) in [("crates", "src/lib.rs"), ("apps", "src/main.rs")] {
        let entries = std::fs::read_dir(root.join(group)).map_err(|e| format!("{group}: {e}"))?;
        for entry in entries.flatten() {
            let path = entry.path().join(file);
            let Ok(source) = std::fs::read_to_string(&path) else { continue };
            if !source.contains(NO_PANICS) {
                missing.push(path.strip_prefix(&root).unwrap_or(&path).display().to_string());
            }
        }
    }
    if missing.is_empty() { Ok(()) } else { Err(format!("without the no-panic attribute: {}", missing.join(", "))) }
}

/// One gate: it passes, or says why not.
type Task = fn() -> Result<(), String>;

/// Runs cargo with these arguments in the workspace.
fn cargo(args: &[&str]) -> Result<(), String> {
    let program = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(program).args(args).current_dir(root()).status().map_err(|e| format!("cargo {}: {e}", args.join(" ")))?;
    if status.success() { Ok(()) } else { Err(format!("cargo {} failed", args.join(" "))) }
}

fn wasm() -> Result<(), String> {
    cargo(&["build", "--quiet", "--package", "aegis-engine", "--package", "aegis-ui-egui", "--target", "wasm32-unknown-unknown"])
}

/// docs/parity-checklist.md: every command by ribbon tab.
fn parity() -> Result<(), String> {
    let mut rows: Vec<(&str, &str, &str, &str)> =
        aegis_engine::Session::commands().iter().map(|c| (c.ribbon, c.label, c.id, c.shortcut.unwrap_or(""))).collect();
    rows.sort_by(|a, b| a.0.cmp(b.0).then(a.1.cmp(b.1)));
    let width = |col: usize, head: &str| {
        rows.iter().map(|r| [r.0, r.1, r.2, r.3].get(col).map_or(0, |s| s.chars().count())).max().unwrap_or(0).max(head.chars().count())
    };
    // The id column is shown in backticks.
    let (w0, w1, w2, w3) = (width(0, "Ribbon"), width(1, "Command"), width(2, "Id") + 2, width(3, "Shortcut"));
    let pad = |s: &str, n: usize| format!("{s}{}", " ".repeat(n.saturating_sub(s.chars().count())));
    let mut out = String::from(
        "# Parity checklist — commands\n\nGenerated by `cargo xtask parity` from the command registry (`crates/engine/src/cmd`). Do not edit by hand.\n\n",
    );
    out.push_str(&format!("| {} | {} | {} | {} |\n", pad("Ribbon", w0), pad("Command", w1), pad("Id", w2), pad("Shortcut", w3)));
    out.push_str(&format!("|{}|{}|{}|{}|\n", "-".repeat(w0 + 2), "-".repeat(w1 + 2), "-".repeat(w2 + 2), "-".repeat(w3 + 2)));
    for (ribbon, label, id, shortcut) in &rows {
        out.push_str(&format!("| {} | {} | {} | {} |\n", pad(ribbon, w0), pad(label, w1), pad(&format!("`{id}`"), w2), pad(shortcut, w3)));
    }
    out.push_str(&format!("\n{} commands.\n", rows.len()));
    std::fs::write(root().join("docs/parity-checklist.md"), out).map_err(|e| format!("docs/parity-checklist.md: {e}"))
}

/// People (by login) and models with their commit counts, most commits first.
fn contributors_json() -> Result<String, String> {
    let out = Command::new("git")
        .args(["log", "--no-merges", "--format=%aN%x1f%aE%x1f%B%x1e"])
        .current_dir(root())
        .output()
        .map_err(|e| format!("git log: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut people: Vec<(String, u64)> = Vec::new();
    let mut models: Vec<(String, u64)> = Vec::new();
    let bump = |list: &mut Vec<(String, u64)>, key: String| match list.iter_mut().find(|(k, _)| *k == key) {
        Some((_, n)) => *n += 1,
        None => list.push((key, 1)),
    };
    for commit in text.split('\u{1e}').map(str::trim).filter(|c| !c.is_empty()) {
        let mut parts = commit.splitn(3, '\u{1f}');
        let (name, email, body) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        // 12345+login@users.noreply.github.com → login.
        let login = email
            .strip_suffix("@users.noreply.github.com")
            .map(|l| l.rsplit('+').next().unwrap_or(l).to_string())
            .unwrap_or_else(|| name.to_string());
        bump(&mut people, login);
        for line in body.lines() {
            if let Some(rest) = line.trim().strip_prefix("Co-Authored-By:").or_else(|| line.trim().strip_prefix("Co-authored-by:")) {
                let who = rest.split('<').next().unwrap_or("").trim().to_string();
                if rest.contains("noreply@anthropic.com") || who.starts_with("Claude") {
                    bump(&mut models, who);
                }
            }
        }
    }
    let sort = |list: &mut Vec<(String, u64)>| list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    sort(&mut people);
    sort(&mut models);
    let json = serde_json::json!({
        "generated_by": "cargo xtask contributors",
        "people": people.iter().map(|(l, n)| serde_json::json!({ "login": l, "commits": n })).collect::<Vec<_>>(),
        "models": models.iter().map(|(m, n)| serde_json::json!({ "name": m, "commits": n })).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&json).map(|s| s + "\n").map_err(|e| e.to_string())
}

fn contributors(check: bool) -> Result<(), String> {
    let path = root().join("contributors/contributors.json");
    let fresh = contributors_json()?;
    if check {
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        return if current == fresh { Ok(()) } else { Err("contributors/contributors.json is stale: run `cargo xtask contributors`".into()) };
    }
    std::fs::write(&path, fresh).map_err(|e| format!("{}: {e}", path.display()))
}

fn ci() -> Result<(), String> {
    let steps: [(&str, Task); 7] = [
        ("fmt", || cargo(&["fmt", "--all", "--check"])),
        ("clippy", || cargo(&["clippy", "--quiet", "--workspace", "--all-targets", "--", "-D", "warnings"])),
        ("tests", || cargo(&["test", "--quiet", "--workspace"])),
        ("assets", assets),
        ("layers", layers),
        ("panics", panics),
        ("wasm", wasm),
    ];
    for (name, step) in steps {
        eprintln!("xtask ci: {name}");
        step().map_err(|e| format!("{name}: {e}"))?;
    }
    eprintln!("xtask ci: every gate passed");
    Ok(())
}

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let result = match task.as_str() {
        "ci" => ci(),
        "layers" => layers(),
        "assets" => assets(),
        "panics" => panics(),
        "wasm" => wasm(),
        "parity" => parity(),
        "contributors" => contributors(std::env::args().any(|a| a == "--check")),
        _ => Err("usage: cargo xtask <ci|layers|assets|panics|wasm|parity|contributors [--check]>".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
