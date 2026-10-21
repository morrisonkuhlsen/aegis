//! mkaeGIS desktop app.
//!
//! `mkaegis [--sample] [--control PORT] [file.mkaegis | data.geojson | data.shp]…`
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use mkaegis_engine::Session;
use mkaegis_ui_egui::splash::{Splash, Step};
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
    let mut steps = Vec::new();
    let mut control = None;
    let mut splash = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sample" => steps.push(Step::new("Sample map", |app: &mut MapApp, _: &egui::Context| {
                app.session = Session::sample();
                Ok(())
            })),
            "--control" => control = args.next().and_then(|p| p.parse::<u16>().ok()),
            "--no-splash" => splash = false,
            "--help" | "-h" => {
                println!("mkaegis [--sample] [--control PORT] [--no-splash] [project.mkaegis | data.geojson | data.shp]…");
                return Ok(());
            }
            path => {
                let path = path.to_string();
                let name = std::path::Path::new(&path).file_name().map_or(path.clone(), |n| n.to_string_lossy().into_owned());
                steps.push(Step::new(name, move |app: &mut MapApp, _: &egui::Context| {
                    let cmd = if path.ends_with(".mkaegis") { "project.open" } else { "map.addData" };
                    app.session.execute(cmd, &json!({ "path": path })).map(|_| ()).map_err(|e| e.to_string())
                }));
            }
        }
    }
    if let Some(port) = control {
        steps.push(Step::new("Control channel", move |app: &mut MapApp, _: &egui::Context| {
            let server = mkaegis_ui_egui::control::Server::start(port).map_err(|e| format!("port {port}: {e}"))?;
            app.control = Some(server);
            Ok(())
        }));
    }
    let mut app = MapApp::new(Session::default());
    app.services = services();
    let mut load = Splash::new(steps);
    if splash {
        app.splash = Some(load);
    } else {
        // Without the splash the steps run before the window opens.
        let ctx = egui::Context::default();
        while load.remaining() > 0 {
            if let Err(e) = load.run_next(&mut app, &ctx) {
                log::error!("{e}");
                app.status = Some((e, true));
            }
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
