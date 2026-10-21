//! Renders the mkaeGIS window offscreen (wgpu, no window) to a PNG, to look at the UI without
//! touching the screen.
//!
//! `cargo run -p mkaegis-ui-egui --example snapshot -- [--sample] [--in file.geojson|.shp|.mkaegis]
//!  [--cmd 'id={json}']... [--tab map|analysis|edit|view|share] [--pane geoprocessing|symbology|history]
//!  [--tool buffer|project|calculate|attributes|location|statistics] [--table] [--select] [--dark]
//!  [--splash] [--size 1280x800] out.png`

use mkaegis_engine::Session;
use mkaegis_ui_egui::{MapApp, Pane, Tab, Tool};
use serde_json::json;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut session = Session::default();
    let mut size = (1280.0f32, 800.0f32);
    let mut out = "snapshot.png".to_string();
    let (mut tab, mut pane, mut table, mut dark, mut select, mut gp) = (Tab::Map, None, false, false, false, None);
    let mut splash = false;
    let mut cmds = Vec::new();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        let mut value = || {
            i += 1;
            args.get(i).cloned().ok_or(format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--sample" => session = Session::sample(),
            "--in" => {
                let path = value()?;
                let cmd = if path.ends_with(".mkaegis") { "project.open" } else { "map.addData" };
                session.execute(cmd, &json!({ "path": path })).map_err(|e| e.to_string())?;
            }
            "--cmd" => cmds.push(value()?),
            "--tab" => {
                tab = match value()?.to_ascii_lowercase().as_str() {
                    "analysis" => Tab::Analysis,
                    "edit" => Tab::Edit,
                    "view" => Tab::View,
                    "share" => Tab::Share,
                    _ => Tab::Map,
                }
            }
            "--pane" => {
                pane = match value()?.to_ascii_lowercase().as_str() {
                    "symbology" => Some(Pane::Symbology),
                    "history" => Some(Pane::History),
                    _ => Some(Pane::Geoprocessing),
                }
            }
            "--tool" => gp = Some(value()?),
            "--table" => table = true,
            "--select" => select = true,
            "--dark" => dark = true,
            "--splash" => splash = true,
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--size WxH")?;
                size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            other => out = other.to_string(),
        }
        i += 1;
    }
    for c in cmds {
        let (id, params) = c.split_once('=').unwrap_or((c.as_str(), "{}"));
        let params: serde_json::Value = serde_json::from_str(params).map_err(|e| format!("{id}: {e}"))?;
        session.execute(id, &params).map_err(|e| format!("{id}: {e}"))?;
    }
    let mut app = MapApp::new(session);
    app.tab = tab;
    app.pane = pane;
    app.show_table = table;
    if select {
        app.tool = Tool::Select;
    }
    if let Some(name) = gp {
        use mkaegis_ui_egui::GpTool as T;
        let tool = match name.as_str() {
            "project" => T::Project,
            "calculate" => T::CalculateGeometry,
            "attributes" => T::SelectByAttributes,
            "location" => T::SelectByLocation,
            "statistics" => T::SummaryStatistics,
            _ => T::Buffer,
        };
        let active = app.active;
        app.gp.open(tool, active);
        app.pane = Some(Pane::Geoprocessing);
    }
    if splash {
        app.splash = Some(mkaegis_ui_egui::splash::Splash::new(Vec::new()).held());
    }
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut MapApp| app.ui(ui), app);
    harness.ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    harness.run_steps(6);
    let image = harness.render().map_err(|e| e.to_string())?;
    image.save(&out).map_err(|e| e.to_string())?;
    println!("{out}");
    Ok(())
}
