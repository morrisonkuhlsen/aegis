//! MK aeGIS's project tasks, run with `cargo xtask <task>`:
//!
//! - `ci`: every quality gate before a commit: fmt, clippy (warnings are errors), tests,
//!   assets, layers, panics, wasm, parity. Stops at the first that fails.
//! - `layers`: each crate depends only on crates at its layer or below, and nothing below the
//!   UI layer on egui, eframe, winit, wgpu or rfd.
//! - `assets`: every asset file has its row in `ATTRIBUTION.md`.
//! - `panics`: every crate's root denies unwrap, expect, panic, todo, unimplemented and
//!   unreachable outside tests.
//! - `wasm`: the engine, the UI and the web app still build for WebAssembly.
//! - `parity [--check]`: regenerates `docs/parity-checklist.md` from the command registry, or
//!   (`--check`, part of `ci`) fails when it is stale.
//! - `stats`: the headline numbers for `ROADMAP.md`: Rust files, lines and tests per crate, and
//!   the commands in the registry.
//! - `version [set X.Y.Z[-pre]]`: prints the workspace version, or sets it in the root
//!   `Cargo.toml` and refreshes the workspace's entries in `Cargo.lock`.
//! - `contributors [--check]`: regenerates `contributors/contributors.json` from the commits:
//!   people by GitHub username (from their noreply address, else their author name) and the AI
//!   models credited in `Co-Authored-By` trailers.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Each internal crate's layer: L0 `core`/`geom`/`crs` → L1 `model` → L2 `geojson`/`csv`/`shp`/`render`
/// → L3 `analysis` → L4 `engine` → L5 `ui-egui`/`mcp` → L6 `apps/*`.
const LAYERS: [(&str, u8); 17] = [
    ("mkaegis-core", 0),
    ("mkaegis-geom", 0),
    ("mkaegis-crs", 0),
    ("mkaegis-model", 1),
    ("mkaegis-geojson", 2),
    ("mkaegis-csv", 2),
    ("mkaegis-kml", 2),
    ("mkaegis-shp", 2),
    ("mkaegis-render", 2),
    ("mkaegis-analysis", 3),
    ("mkaegis-stats", 3),
    ("mkaegis-engine", 4),
    ("mkaegis-ui-egui", 5),
    ("mkaegis-mcp", 5),
    ("mkaegis", 6),
    ("mkaegis-cli", 6),
    ("mkaegis-web", 6),
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
    cargo(&[
        "build",
        "--quiet",
        "--package",
        "mkaegis-engine",
        "--package",
        "mkaegis-ui-egui",
        "--package",
        "mkaegis-web",
        "--target",
        "wasm32-unknown-unknown",
    ])
}

/// The parity checklist, generated from the command registry.
const PARITY_CHECKLIST: &str = "docs/parity-checklist.md";

/// Writes the parity checklist, or with `check` fails when the committed one differs.
fn parity(check: bool) -> Result<(), String> {
    let path = root().join(PARITY_CHECKLIST);
    let fresh = parity_checklist();
    if check {
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        return if current == fresh { Ok(()) } else { Err(format!("{PARITY_CHECKLIST} is stale: run `cargo xtask parity`")) };
    }
    std::fs::write(&path, fresh).map_err(|e| format!("{PARITY_CHECKLIST}: {e}"))
}

/// docs/parity-checklist.md: every command by ribbon tab.
fn parity_checklist() -> String {
    let mut rows: Vec<(&str, &str, &str, &str)> =
        mkaegis_engine::Session::commands().iter().map(|c| (c.ribbon, c.label, c.id, c.shortcut.unwrap_or(""))).collect();
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
    out
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

/// `cargo xtask version`: prints the workspace version; `version set X.Y.Z[-pre]` sets it. It lives
/// only in `[workspace.package]` of the root `Cargo.toml`; every crate inherits it, and a release
/// tag must be `v` + it.
fn version(args: &[String]) -> Result<(), String> {
    let path = root().join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Cargo.toml: {e}"))?;
    match args {
        [] => {
            println!("{}", workspace_version(&text)?);
            Ok(())
        }
        [set, new] if set == "set" => {
            let old = workspace_version(&text)?;
            let updated = with_workspace_version(&text, new)?;
            // Through a temporary file: a half-written root manifest breaks every build.
            let tmp = root().join("target/Cargo.toml.xtask-version");
            std::fs::create_dir_all(root().join("target")).map_err(|e| format!("target: {e}"))?;
            std::fs::write(&tmp, updated).map_err(|e| format!("{}: {e}", tmp.display()))?;
            std::fs::rename(&tmp, &path).map_err(|e| format!("Cargo.toml: {e}"))?;
            // Only the workspace's own entries in Cargo.lock change, never a dependency.
            cargo(&["update", "--quiet", "--workspace", "--offline"]).or_else(|_| cargo(&["update", "--quiet", "--workspace"]))?;
            eprintln!("version {old} -> {new} (Cargo.toml, Cargo.lock)");
            Ok(())
        }
        _ => Err("usage: cargo xtask version [set X.Y.Z[-pre]]".to_string()),
    }
}

/// The `version = "…"` of `[workspace.package]` in a root manifest.
fn workspace_version(manifest: &str) -> Result<String, String> {
    let (_, line) = workspace_version_line(manifest)?;
    let value = line.split_once('=').map_or("", |(_, v)| v.trim().trim_matches('"'));
    if value.is_empty() { Err("[workspace.package] has an empty version".to_string()) } else { Ok(value.to_string()) }
}

/// The manifest with its workspace version replaced; every other byte stays as it was.
fn with_workspace_version(manifest: &str, new: &str) -> Result<String, String> {
    check_version(new)?;
    let (index, _) = workspace_version_line(manifest)?;
    let mut out = String::with_capacity(manifest.len() + 8);
    for (i, line) in manifest.split_inclusive('\n').enumerate() {
        if i == index {
            let eol = line.strip_prefix(line.trim_end_matches(['\r', '\n'])).unwrap_or_default();
            out.push_str(&format!("version = \"{new}\"{eol}"));
        } else {
            out.push_str(line);
        }
    }
    Ok(out)
}

/// Semantic versioning without build metadata: `MAJOR.MINOR.PATCH`, optionally `-pre.release`
/// (`0.2.0`, `0.2.0-rc.1`), so that the tag `v<version>` names exactly one release.
fn check_version(v: &str) -> Result<(), String> {
    let (core, pre) = match v.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (v, None),
    };
    let number = |n: &str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && (n == "0" || !n.starts_with('0'));
    let numbers: Vec<&str> = core.split('.').collect();
    let identifier = |id: &str| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if numbers.len() == 3 && numbers.iter().all(|n| number(n)) && pre.is_none_or(|p| p.split('.').all(identifier)) {
        Ok(())
    } else {
        Err(format!("`{v}` is not a version like 0.2.0 or 0.2.0-rc.1"))
    }
}

/// The index and text of the `version` line in `[workspace.package]`.
fn workspace_version_line(manifest: &str) -> Result<(usize, &str), String> {
    let mut section = "";
    for (i, line) in manifest.lines().enumerate() {
        let t = line.trim();
        if t.starts_with('[') {
            section = t;
        } else if section == "[workspace.package]" && t.strip_prefix("version").is_some_and(|rest| rest.trim_start().starts_with('=')) {
            return Ok((i, line));
        }
    }
    Err("no version in [workspace.package] of the root Cargo.toml".to_string())
}

/// `cargo xtask stats`: Rust files, lines and tests per crate, as a Markdown table, then the
/// command count. A test is a `#[test]` attribute.
fn stats() -> Result<(), String> {
    let root = root();
    let mut rows: Vec<(String, Counts)> = Vec::new();
    for group in ["crates", "apps"] {
        let entries = std::fs::read_dir(root.join(group)).map_err(|e| format!("{group}: {e}"))?;
        for entry in entries.flatten().filter(|e| e.path().join("Cargo.toml").exists()) {
            let (name, _) = manifest(&entry.path().join("Cargo.toml"))?;
            rows.push((name, count_rust(&entry.path())));
        }
    }
    rows.push(("xtask".to_string(), count_rust(&root.join("xtask"))));
    rows.sort_by(|a, b| layer(&a.0).unwrap_or(u8::MAX).cmp(&layer(&b.0).unwrap_or(u8::MAX)).then(a.0.cmp(&b.0)));
    let total =
        rows.iter().fold(Counts::default(), |t, (_, c)| Counts { files: t.files + c.files, lines: t.lines + c.lines, tests: t.tests + c.tests });
    rows.push(("**total**".to_string(), total));
    let w = rows.iter().map(|(n, _)| n.chars().count()).max().unwrap_or(0).max("Crate".len());
    println!("| {:<w$} | Files |  Lines | Tests |", "Crate");
    println!("|{}|-------|--------|-------|", "-".repeat(w + 2));
    for (name, c) in &rows {
        println!("| {name:<w$} | {:>5} | {:>6} | {:>5} |", c.files, c.lines, c.tests);
    }
    println!("\n{} commands.", mkaegis_engine::Session::commands().len());
    Ok(())
}

/// Rust files, lines and tests under one crate.
#[derive(Clone, Copy, Default)]
struct Counts {
    files: usize,
    lines: usize,
    tests: usize,
}

/// Counts the `.rs` files under a crate's folder, iteratively, skipping build output.
fn count_rust(dir: &Path) -> Counts {
    let mut counts = Counts::default();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if name != "target" && !name.starts_with('.') {
                    stack.push(path);
                }
            } else if name.ends_with(".rs")
                && let Ok(source) = std::fs::read_to_string(&path)
            {
                counts.files += 1;
                counts.lines += source.lines().count();
                counts.tests += source.lines().filter(|l| l.trim_start().starts_with("#[test]")).count();
            }
        }
    }
    counts
}

fn ci() -> Result<(), String> {
    let steps: [(&str, Task); 8] = [
        ("fmt", || cargo(&["fmt", "--all", "--check"])),
        ("clippy", || cargo(&["clippy", "--quiet", "--workspace", "--all-targets", "--", "-D", "warnings"])),
        ("tests", || cargo(&["test", "--quiet", "--workspace"])),
        ("assets", assets),
        ("layers", layers),
        ("panics", panics),
        ("wasm", wasm),
        ("parity", || parity(true)),
    ];
    for (done, (name, step)) in steps.iter().enumerate() {
        eprintln!("xtask ci: {name}");
        if let Err(e) = step() {
            let passed: Vec<&str> = steps.iter().take(done).map(|(n, _)| *n).collect();
            let skipped: Vec<&str> = steps.iter().skip(done + 1).map(|(n, _)| *n).collect();
            return Err(format!("{name}: {e}\nxtask ci: passed: {}; not run: {}", list(&passed), list(&skipped)));
        }
    }
    eprintln!("xtask ci: all {} gates passed", steps.len());
    Ok(())
}

/// Gate names for the summary, or a dash for none.
fn list(names: &[&str]) -> String {
    if names.is_empty() { "-".to_string() } else { names.join(", ") }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let result =
        match args.first().map(String::as_str).unwrap_or_default() {
            "ci" => ci(),
            "layers" => layers(),
            "assets" => assets(),
            "panics" => panics(),
            "wasm" => wasm(),
            "parity" => parity(check),
            "contributors" => contributors(check),
            "stats" => stats(),
            "version" => version(args.get(1..).unwrap_or_default()),
            _ => Err("usage: cargo xtask <ci|layers|assets|panics|wasm|parity [--check]|contributors [--check]|stats|version [set X.Y.Z[-pre]]>"
                .to_string()),
        };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "[workspace]\nmembers = [\"a\"]\n\n[workspace.package]\nversion = \"0.1.1\"\nedition = \"2024\"\n\n[workspace.dependencies]\nfoo = { version = \"1\" }\n";

    #[test]
    fn reads_the_workspace_version() {
        assert_eq!(workspace_version(MANIFEST).unwrap(), "0.1.1");
        assert!(workspace_version("[package]\nversion = \"1.0.0\"\n").is_err());
        assert!(workspace_version("[workspace.package]\nversion = \"\"\n").is_err());
    }

    #[test]
    fn sets_only_the_workspace_version() {
        let out = with_workspace_version(MANIFEST, "0.2.0-rc.1").unwrap();
        assert_eq!(out, MANIFEST.replace("version = \"0.1.1\"", "version = \"0.2.0-rc.1\""));
        assert!(out.contains("foo = { version = \"1\" }"));
        assert!(with_workspace_version(MANIFEST, "v0.2.0").is_err());
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let crlf = MANIFEST.replace('\n', "\r\n");
        assert_eq!(with_workspace_version(&crlf, "1.0.0").unwrap(), crlf.replace("\"0.1.1\"", "\"1.0.0\""));
    }

    #[test]
    fn accepts_only_tag_safe_versions() {
        for ok in ["0.1.0", "0.10.20", "1.0.0-rc.1", "1.0.0-alpha", "1.0.0-x-y.2"] {
            assert!(check_version(ok).is_ok(), "{ok}");
        }
        for bad in ["", "1.0", "1.0.0.0", "v1.0.0", "01.0.0", "1.a.0", "1.0.0-", "1.0.0-a..b", "1.0.0+build", "1.0.0-rc/1"] {
            assert!(check_version(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_root_manifest_has_a_valid_version() {
        let text = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
        check_version(&workspace_version(&text).unwrap()).unwrap();
    }
}
