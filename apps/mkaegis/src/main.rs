//! MK aeGIS desktop app.
//!
//! `mkaegis [--sample] [--control PORT] [file.aegis | data.geojson | data.shp]…`, or
//! `mkaegis --version`.
// A GUI program on Windows: no console window beside the app. Debug builds keep theirs.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod tiles;

use std::io::Write as _;
use std::path::Path;

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, wgpu};
use mkaegis_engine::Session;
use mkaegis_ui_egui::{Language, MapApp, Services};
use serde_json::json;

const USAGE: &str = "usage: mkaegis [--sample] [--control PORT] [--no-splash] [project.aegis | data.geojson | data.shp]… | --help | --version";

struct App(MapApp);

/// What the command line asks for.
#[derive(Debug, Default, PartialEq)]
struct Args {
    /// The sample map, with the files on top of it.
    sample: bool,
    control: Option<u16>,
    no_splash: bool,
    files: Vec<String>,
    help: bool,
    version: bool,
}

/// Warnings and errors from MK aeGIS, and errors from anything, to stderr: what the window
/// can't show (before it opens, or from a thread) is still seen from a terminal (#63).
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Error || (m.level() <= log::Level::Warn && m.target().starts_with("mkaegis"))
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            let _ = writeln!(std::io::stderr(), "{} {}: {}", r.level(), r.target(), r.args());
        }
    }

    fn flush(&self) {}
}

static LOGGER: StderrLogger = StderrLogger;

struct SplashWindow(mkaegis_ui_egui::splash::SplashScreen);

impl eframe::App for SplashWindow {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

fn services() -> Services {
    Services {
        open_file: Some(Box::new(|title: &str, extensions: &[&str]| {
            rfd::FileDialog::new().set_title(title).add_filter("MK aeGIS", extensions).pick_file()
        })),
        save_file: Some(Box::new(|title: &str, extensions: &[&str], suggested: &str| {
            rfd::FileDialog::new().set_title(title).add_filter("MK aeGIS", extensions).set_file_name(suggested).save_file()
        })),
        save_recent: Some(Box::new(|files: &[std::path::PathBuf]| {
            if let Some(file) = settings_file("recent.txt") {
                let text: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
                let _ = file.parent().map(std::fs::create_dir_all);
                let _ = std::fs::write(file, text.join("\n"));
            }
        })),
        fetch: Some(Box::new(tiles::fetcher(tiles::cache_dir()))),
        save_language: Some(Box::new(|language: Language| {
            if let Some(file) = settings_file("language.txt") {
                let _ = file.parent().map(std::fs::create_dir_all);
                let _ = std::fs::write(file, language.code());
            }
        })),
        ..Default::default()
    }
}

/// The recent projects kept by earlier runs, newest first.
fn load_recent() -> Vec<std::path::PathBuf> {
    settings_file("recent.txt")
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|text| text.lines().filter(|l| !l.trim().is_empty()).map(std::path::PathBuf::from).collect())
        .unwrap_or_default()
}

/// The interface language chosen in an earlier run; else the system's (`LC_ALL`, `LC_MESSAGES`,
/// `LANG`) when the interface has it; else English.
fn load_language() -> Language {
    let saved = settings_file("language.txt").and_then(|f| std::fs::read_to_string(f).ok());
    let system = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|v| std::env::var(v).ok()).find(|v| !v.is_empty());
    saved.into_iter().chain(system).find_map(|code| Language::from_code(&code)).unwrap_or_default()
}

/// A file in the platform's per-user configuration folder, under `mkaegis`.
fn settings_file(name: &str) -> Option<std::path::PathBuf> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(std::path::PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("mkaegis").join(name))
}

/// The graphics backends to try, best first; `None` is wgpu's own choice. On Windows that choice
/// is Vulkan, and some Intel Vulkan drivers crash the app at start (igvk64.dll, #82), so it is
/// DirectX 12, then OpenGL. `WGPU_BACKEND` overrides it everywhere.
fn backends() -> Vec<Option<wgpu::Backends>> {
    if cfg!(target_os = "windows") && wgpu::Backends::from_env().is_none() {
        vec![Some(wgpu::Backends::DX12), Some(wgpu::Backends::GL)]
    } else {
        vec![None]
    }
}

/// A window on the given graphics backends.
fn options(viewport: egui::ViewportBuilder, backends: Option<wgpu::Backends>) -> eframe::NativeOptions {
    let mut wgpu_options = WgpuConfiguration::default();
    if let (Some(backends), WgpuSetup::CreateNew(setup)) = (backends, &mut wgpu_options.wgpu_setup) {
        setup.instance_descriptor.backends = backends;
    }
    eframe::NativeOptions { viewport, wgpu_options, ..Default::default() }
}

/// The splash screen in a small window of its own, centred and undecorated, until it closes.
fn splash(backends: Option<wgpu::Backends>) -> eframe::Result {
    let viewport = egui::ViewportBuilder::default()
        .with_title("MK aeGIS")
        .with_app_id("mkaegis")
        .with_inner_size(mkaegis_ui_egui::splash::SIZE)
        .with_decorations(false)
        .with_resizable(false)
        .with_taskbar(false);
    let options = eframe::NativeOptions { centered: true, ..options(viewport, backends) };
    eframe::run_native("MK aeGIS splash", options, Box::new(|_cc| Ok(Box::new(SplashWindow(Default::default())))))
}

/// The command line, in any order: `--sample` adds the sample under the files rather than
/// replacing them, and a value that isn't one (`--control abc`) or an unknown option is refused.
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sample" => parsed.sample = true,
            "--control" => {
                let port = args.next().ok_or("--control needs a port")?;
                parsed.control = Some(port.parse::<u16>().ok().filter(|p| *p > 0).ok_or(format!("--control: `{port}` is not a port (1 to 65535)"))?);
            }
            "--no-splash" => parsed.no_splash = true,
            "--help" | "-h" => parsed.help = true,
            "--version" | "-V" => parsed.version = true,
            option if option.starts_with("--") => return Err(format!("unknown option `{option}`")),
            path => parsed.files.push(path.to_string()),
        }
    }
    Ok(parsed)
}

/// A project file, whatever the case of its extension (`MAP.AEGIS`).
fn is_project(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case(mkaegis_engine::io::PROJECT_EXTENSION))
}

fn main() -> eframe::Result {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("mkaegis: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    if args.help {
        println!("{USAGE}");
        return Ok(());
    }
    if args.version {
        println!("mkaegis {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    // The splash shows in it too.
    let language = load_language();
    language.set();
    let mut session = if args.sample { Session::sample() } else { Session::default() };
    let mut problems = Vec::new();
    for path in &args.files {
        let cmd = if is_project(path) { "project.open" } else { "map.addData" };
        if let Err(e) = session.execute(cmd, &json!({ "path": path })) {
            problems.push(format!("{path}: {e}"));
        }
    }
    let (control, show_splash) = (args.control, !args.no_splash);
    // The splash also finds a graphics backend that starts, for the main window to use; without
    // its splash (--no-splash, or any other error) the program still starts, on the first one.
    let candidates = backends();
    let first = candidates.first().copied().flatten();
    let backends =
        if show_splash { candidates.iter().copied().find(|b| !matches!(splash(*b), Err(eframe::Error::Wgpu(_)))).unwrap_or(first) } else { first };
    let mut app = MapApp::new(session);
    app.services = services();
    app.recent = load_recent();
    app.language = language;
    if let Some(p) = problems.first() {
        app.status = Some((p.clone(), true));
    }
    if let Some(port) = control {
        match mkaegis_ui_egui::control::Server::start(port) {
            Ok(server) => app.control = Some(server),
            // In the status bar too: a port in use is the usual cause, and the window is where
            // it is seen (#63).
            Err(e) => {
                log::error!("control channel on port {port}: {e}");
                app.status = Some((format!("Control channel on port {port}: {e}"), true));
            }
        }
    }
    let viewport = egui::ViewportBuilder::default()
        .with_title("MK aeGIS")
        .with_decorations(false)
        .with_app_id("mkaegis")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([800.0, 520.0]);
    eframe::run_native("MK aeGIS", options(viewport, backends), Box::new(|_cc| Ok(Box::new(App(app)))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_add_the_sample_and_refuse_what_they_cant_take() {
        let parse = |a: &[&str]| parse_args(a.iter().map(|s| s.to_string()));
        // The file is kept whichever side of --sample it is on.
        for args in [["a.shp", "--sample"], ["--sample", "a.shp"]] {
            assert_eq!(parse(&args).unwrap(), Args { sample: true, files: vec!["a.shp".into()], ..Args::default() });
        }
        assert_eq!(parse(&["--control", "7979", "--no-splash"]).unwrap(), Args { control: Some(7979), no_splash: true, ..Args::default() });
        for bad in [&["--control", "abc"][..], &["--control"], &["--control", "0"], &["--control", "70000"], &["--bogus"]] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn projects_are_recognised_in_any_case() {
        assert!(is_project("map.aegis") && is_project("MAP.AEGIS") && is_project("dir/Map.Aegis"));
        assert!(!is_project("map.geojson") && !is_project("aegis"));
    }
}
