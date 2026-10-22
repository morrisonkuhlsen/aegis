//! Renders the MK aeGIS window offscreen (wgpu, no window) to a PNG, to look at the UI without
//! touching the screen.
//!
//! `cargo run -p mkaegis-ui-egui --example snapshot -- [--sample] [--in file.geojson|.shp|.aegis]
//!  [--cmd 'id={json}']... [--tab map|analysis|statistics|edit|view|share] [--pane geoprocessing|symbology|history|charts]
//!  [--tool buffer|project|calculate|attributes|location|statistics|weights|autocorrelation|join|xy|crosstabs|correlations] [--click LABEL]... [--chart histogram|box|scatter|moran]
//!  [--table] [--select] [--dark]
//!  [--splash] [--icon] [--backstage home|new|open|info|export] [--size 1280x800] out.png`

use egui_kittest::kittest::Queryable;
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
    let mut backstage = None;
    let mut icon = false;
    let mut cmds = Vec::new();
    let mut clicks = Vec::new();
    let mut chart = None;
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
                let cmd = if path.ends_with(".aegis") { "project.open" } else { "map.addData" };
                session.execute(cmd, &json!({ "path": path })).map_err(|e| e.to_string())?;
            }
            "--cmd" => cmds.push(value()?),
            "--tab" => {
                tab = match value()?.to_ascii_lowercase().as_str() {
                    "analysis" => Tab::Analysis,
                    "statistics" => Tab::Statistics,
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
                    "charts" => Some(Pane::Charts),
                    _ => Some(Pane::Geoprocessing),
                }
            }
            "--tool" => gp = Some(value()?),
            // Clicks the widget with this label once the window is drawn (`--click Run`).
            "--click" => clicks.push(value()?),
            "--chart" => {
                use mkaegis_ui_egui::Chart;
                chart = Some(match value()?.to_ascii_lowercase().as_str() {
                    "box" | "boxplot" => Chart::BoxPlot,
                    "scatter" => Chart::Scatter,
                    "moran" => Chart::MoranScatter,
                    _ => Chart::Histogram,
                });
                pane = Some(Pane::Charts);
            }
            "--table" => table = true,
            "--select" => select = true,
            "--dark" => dark = true,
            "--splash" => splash = true,
            "--backstage" => {
                backstage = Some(match value()?.to_ascii_lowercase().as_str() {
                    "new" => mkaegis_ui_egui::BackstagePage::New,
                    "open" => mkaegis_ui_egui::BackstagePage::Open,
                    "info" => mkaegis_ui_egui::BackstagePage::Info,
                    "export" => mkaegis_ui_egui::BackstagePage::Export,
                    _ => mkaegis_ui_egui::BackstagePage::Home,
                })
            }
            "--icon" => icon = true,
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
    app.backstage = backstage;
    app.pane = pane;
    app.show_table = table;
    if select {
        app.tool = Tool::Select;
    }
    if let Some(kind) = chart {
        app.charts.kind = kind;
    }
    if let Some(name) = gp {
        use mkaegis_ui_egui::GpTool as T;
        let tool = match name.as_str() {
            "project" => T::Project,
            "calculate" => T::CalculateGeometry,
            "attributes" => T::SelectByAttributes,
            "location" => T::SelectByLocation,
            "statistics" => T::SummaryStatistics,
            "weights" => T::SpatialWeights,
            "autocorrelation" => T::SpatialAutocorrelation,
            "join" => T::JoinField,
            "crosstabs" => T::Crosstabs,
            "correlations" => T::Correlations,
            "xy" => T::XyTableToPoint,
            _ => T::Buffer,
        };
        let active = app.active;
        app.gp.open(tool, active);
        app.pane = Some(Pane::Geoprocessing);
    }
    if icon {
        // The app's icon, 256 px: the mark in coral on Morrison Kühlsen's ink.
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(128.0, 128.0)).with_pixels_per_point(2.0).wgpu().build_ui(|ui| {
            let rect = ui.ctx().content_rect();
            ui.painter().rect_filled(rect, 26.0, egui::Color32::from_rgb(0x14, 0x16, 0x1C));
            let coral = egui::Color32::from_rgb(0xE8, 0x5A, 0x5F);
            mkaegis_ui_egui::icons::paint(ui.painter(), rect.shrink(22.0), mkaegis_ui_egui::icons::Icon::Logo, coral, coral);
        });
        harness.run_steps(2);
        let image = harness.render().map_err(|e| e.to_string())?;
        image.save(&out).map_err(|e| e.to_string())?;
        println!("{out}");
        return Ok(());
    }
    if splash {
        // The splash screen alone, at its own size.
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(mkaegis_ui_egui::splash::SIZE[0], mkaegis_ui_egui::splash::SIZE[1]))
            .with_pixels_per_point(2.0)
            .wgpu()
            .build_ui_state(|ui, s: &mut mkaegis_ui_egui::splash::SplashScreen| s.ui(ui), Default::default());
        harness.run_steps(3);
        let image = harness.render().map_err(|e| e.to_string())?;
        image.save(&out).map_err(|e| e.to_string())?;
        println!("{out}");
        return Ok(());
    }
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut MapApp| app.ui(ui), app);
    harness.ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    harness.run_steps(6);
    for label in &clicks {
        harness.get_by_label(label).click();
        harness.run_steps(4);
    }
    let image = harness.render().map_err(|e| e.to_string())?;
    image.save(&out).map_err(|e| e.to_string())?;
    println!("{out}");
    Ok(())
}
