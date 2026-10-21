//! MK aeGIS desktop app.
//!
//! `aegis [--sample] [--control PORT] [file.aegis | data.geojson | data.shp]…`
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use aegis_engine::Session;
use aegis_ui_egui::{MapApp, Services};
use serde_json::json;

struct App(MapApp);

struct SplashWindow(aegis_ui_egui::splash::SplashScreen);

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

/// The splash screen in a small window of its own, centred and undecorated, until it closes.
fn splash() -> eframe::Result {
    let viewport = egui::ViewportBuilder::default()
        .with_title("MK aeGIS")
        .with_app_id("aegis")
        .with_inner_size(aegis_ui_egui::splash::SIZE)
        .with_decorations(false)
        .with_resizable(false)
        .with_taskbar(false);
    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
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
                println!("aegis [--sample] [--control PORT] [--no-splash] [project.aegis | data.geojson | data.shp]…");
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
    if show_splash {
        // Without its splash the program still starts.
        let _ = splash();
    }
    let mut app = MapApp::new(session);
    app.services = services();
    if let Some(p) = problems.first() {
        app.status = Some((p.clone(), true));
    }
    if let Some(port) = control {
        match aegis_ui_egui::control::Server::start(port) {
            Ok(server) => app.control = Some(server),
            Err(e) => log::error!("control channel on port {port}: {e}"),
        }
    }
    let viewport = egui::ViewportBuilder::default()
        .with_title("MK aeGIS")
        .with_decorations(false)
        .with_app_id("aegis")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([800.0, 520.0]);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("MK aeGIS", options, Box::new(|_cc| Ok(Box::new(App(app)))))
}
