//! mkaeGIS desktop app.
//!
//! `mkaegis [--sample] [--control PORT] [file.mkaegis | data.geojson | data.shp]…`
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use mkaegis_engine::Session;
use mkaegis_ui_egui::{MapApp, Services};
use serde_json::json;

struct App(MapApp);

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

fn services() -> Services {
    Services {
        open_file: Some(Box::new(|title: &str, extensions: &[&str]| {
            rfd::FileDialog::new().set_title(title).add_filter("mkaeGIS", extensions).pick_file()
        })),
        save_file: Some(Box::new(|title: &str, extensions: &[&str], suggested: &str| {
            rfd::FileDialog::new().set_title(title).add_filter("mkaeGIS", extensions).set_file_name(suggested).save_file()
        })),
    }
}

fn main() -> eframe::Result {
    let mut session = Session::default();
    let mut control = None;
    let mut args = std::env::args().skip(1);
    let mut problems = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sample" => session = Session::sample(),
            "--control" => control = args.next().and_then(|p| p.parse::<u16>().ok()),
            "--help" | "-h" => {
                println!("mkaegis [--sample] [--control PORT] [project.mkaegis | data.geojson | data.shp]…");
                return Ok(());
            }
            path => {
                let cmd = if path.ends_with(".mkaegis") { "project.open" } else { "map.addData" };
                if let Err(e) = session.execute(cmd, &json!({ "path": path })) {
                    problems.push(format!("{path}: {e}"));
                }
            }
        }
    }
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
        .with_title("mkaeGIS")
        .with_decorations(false)
        .with_app_id("mkaegis")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([800.0, 520.0]);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("mkaeGIS", options, Box::new(|_cc| Ok(Box::new(App(app)))))
}
