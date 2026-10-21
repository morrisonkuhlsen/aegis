//! MK aeGIS desktop app.
//!
//! `mkaegis [--sample] [--control PORT] [file.aegis | data.geojson | data.shp]…`, or
//! `mkaegis --version`.
// A GUI program on Windows: no console window beside the app. Debug builds keep theirs.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, wgpu};
use mkaegis_engine::Session;
use mkaegis_ui_egui::{MapApp, Services};
use serde_json::json;

struct App(MapApp);

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
    }
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

fn main() -> eframe::Result {
    let mut session = Session::default();
    let mut control = None;
    let mut show_splash = true;
    let mut args = std::env::args().skip(1);
    let mut problems = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sample" => session = Session::sample(),
            "--control" => control = args.next().and_then(|p| p.parse::<u16>().ok()),
            "--no-splash" => show_splash = false,
            "--help" | "-h" => {
                println!("mkaegis [--sample] [--control PORT] [--no-splash] [project.aegis | data.geojson | data.shp]… | --version");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("mkaegis {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            path => {
                let cmd = if path.ends_with(".aegis") { "project.open" } else { "map.addData" };
                if let Err(e) = session.execute(cmd, &json!({ "path": path })) {
                    problems.push(format!("{path}: {e}"));
                }
            }
        }
    }
    // The splash also finds a graphics backend that starts, for the main window to use; without
    // its splash (--no-splash, or any other error) the program still starts, on the first one.
    let candidates = backends();
    let first = candidates.first().copied().flatten();
    let backends =
        if show_splash { candidates.iter().copied().find(|b| !matches!(splash(*b), Err(eframe::Error::Wgpu(_)))).unwrap_or(first) } else { first };
    let mut app = MapApp::new(session);
    app.services = services();
    if let Some(p) = problems.first() {
        app.status = Some((p.clone(), true));
    }
    if let Some(port) = control {
        match mkaegis_ui_egui::control::Server::start(port) {
            Ok(server) => app.control = Some(server),
            Err(e) => log::error!("control channel on port {port}: {e}"),
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
