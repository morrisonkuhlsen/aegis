//! The control channel: JSON lines on `127.0.0.1:PORT`, so agents and scripts can drive the
//! running app and look at it. Each request is `{"id", "method", "params"}`; each reply
//! `{"id", "result"}` or `{"id", "error"}`. Methods:
//!
//! - `engine.execute {command, params}` — run any command, as the ribbon does.
//! - `engine.commands` — every command with its ribbon path and parameters.
//! - `ui.state` — tab, pane, tool, active layer, view extent and scale, last message.
//! - `ui.set {tab?, pane?, tool?, table?, active?, backstage?}` — change what the window shows.
//! - `ui.screenshot {path}` — the window as a PNG (needs a presented frame).
//! - `ui.render {path, width?, height?}` — the map alone, rendered offscreen, as Export Map.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::{Value as Json, json};

use crate::{MapApp, Pane, Tab, Tool};

/// A request from a client, and where its reply goes.
struct Request {
    id: Json,
    method: String,
    params: Json,
    reply: Sender<String>,
}

pub struct Server {
    requests: Receiver<Request>,
    /// Screenshots asked for and not yet delivered: (path, reply, request id).
    pending: Vec<(String, Sender<String>, Json)>,
    pub port: u16,
}

impl Server {
    /// Listens on `127.0.0.1:port` in a background thread.
    pub fn start(port: u16) -> std::io::Result<Server> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let (tx, rx) = channel();
        std::thread::Builder::new().name("mkaegis-control".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                let _ = std::thread::Builder::new().name("mkaegis-control-conn".into()).spawn(move || serve(stream, tx));
            }
        })?;
        Ok(Server { requests: rx, pending: Vec::new(), port })
    }
}

fn serve(stream: TcpStream, tx: Sender<Request>) {
    let Ok(mut out) = stream.try_clone() else { return };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let (reply_tx, reply_rx) = channel();
        let reply = match serde_json::from_str::<Json>(&line) {
            Ok(req) => {
                let request = Request {
                    id: req.get("id").cloned().unwrap_or(Json::Null),
                    method: req.get("method").and_then(Json::as_str).unwrap_or_default().to_string(),
                    params: req.get("params").cloned().unwrap_or(json!({})),
                    reply: reply_tx,
                };
                if tx.send(request).is_err() {
                    break;
                }
                reply_rx.recv().unwrap_or_else(|_| json!({ "error": "the app closed" }).to_string())
            }
            Err(e) => json!({ "id": null, "error": format!("bad JSON: {e}") }).to_string(),
        };
        if writeln!(out, "{reply}").is_err() {
            break;
        }
    }
}

fn answer(id: &Json, result: Result<Json, String>) -> String {
    match result {
        Ok(r) => json!({ "id": id, "result": r }).to_string(),
        Err(e) => json!({ "id": id, "error": e }).to_string(),
    }
}

/// Handles waiting requests; called once per frame.
pub fn poll(app: &mut MapApp, ctx: &egui::Context) {
    // Deliver screenshots that have arrived.
    let shots: Vec<(egui::UserData, std::sync::Arc<egui::ColorImage>)> = ctx.input(|i| {
        i.raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Screenshot { user_data, image, .. } => Some((user_data.clone(), image.clone())),
                _ => None,
            })
            .collect()
    });
    if let Some(server) = &mut app.control {
        for (data, image) in shots {
            let Some(path) = data.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned() else { continue };
            if let Some(i) = server.pending.iter().position(|(p, _, _)| *p == path) {
                let (path, reply, id) = server.pending.remove(i);
                let result = save_png(&path, image.width(), image.height(), &image.pixels).map(|()| json!({ "path": path }));
                let _ = reply.send(answer(&id, result));
            }
        }
    }
    let requests: Vec<Request> = match &app.control {
        Some(server) => server.requests.try_iter().collect(),
        None => return,
    };
    for r in requests {
        let result = match r.method.as_str() {
            "engine.execute" => {
                let command = r.params.get("command").and_then(Json::as_str).unwrap_or_default().to_string();
                let params = r.params.get("params").cloned().unwrap_or(json!({}));
                match app.session.execute(&command, &params) {
                    Ok(v) => {
                        app.status = Some((format!("{command}: done"), false));
                        Ok(v)
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            "engine.commands" => Ok(Json::Array(
                mkaegis_engine::Session::commands()
                    .iter()
                    .map(|c| json!({ "id": c.id, "label": c.label, "ribbon": c.ribbon, "shortcut": c.shortcut, "params": c.params }))
                    .collect(),
            )),
            "ui.state" => Ok(state(app)),
            "ui.set" => set(app, &r.params).map(|()| state(app)),
            "ui.screenshot" => match r.params.get("path").and_then(Json::as_str) {
                Some(path) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(path.to_string())));
                    ctx.request_repaint();
                    if let Some(server) = &mut app.control {
                        server.pending.push((path.to_string(), r.reply, r.id));
                    }
                    continue;
                }
                None => Err("path required".into()),
            },
            "ui.render" => render(app, &r.params),
            other => Err(format!("unknown method `{other}`")),
        };
        let _ = r.reply.send(answer(&r.id, result));
        ctx.request_repaint();
    }
}

fn state(app: &MapApp) -> Json {
    let vp = app.view.viewport();
    json!({
        "tab": format!("{:?}", app.tab),
        "pane": app.pane.map(|p| format!("{p:?}")),
        "tool": format!("{:?}", app.tool),
        "table": app.show_table,
        "active": app.active,
        "backstage": app.backstage.map(|p| format!("{p:?}")),
        "extent": vp.map(|v| { let e = v.extent(); vec![e.min_x, e.min_y, e.max_x, e.max_y] }),
        "scale": vp.map(|v| v.scale(app.session.map.crs)),
        "status": app.status.as_ref().map(|(m, e)| json!({ "message": m, "error": e })),
    })
}

fn set(app: &mut MapApp, p: &Json) -> Result<(), String> {
    if let Some(tab) = p.get("tab").and_then(Json::as_str) {
        app.tab = match tab.to_ascii_lowercase().as_str() {
            "map" => Tab::Map,
            "analysis" => Tab::Analysis,
            "statistics" => Tab::Statistics,
            "edit" => Tab::Edit,
            "view" => Tab::View,
            "share" => Tab::Share,
            other => return Err(format!("unknown tab `{other}`")),
        };
    }
    if let Some(pane) = p.get("pane") {
        app.pane = match pane.as_str().map(str::to_ascii_lowercase).as_deref() {
            None | Some("") | Some("none") => None,
            Some("geoprocessing") => Some(Pane::Geoprocessing),
            Some("symbology") => Some(Pane::Symbology),
            Some("history") => Some(Pane::History),
            Some("charts") => Some(Pane::Charts),
            Some(other) => return Err(format!("unknown pane `{other}`")),
        };
    }
    if let Some(tool) = p.get("tool").and_then(Json::as_str) {
        app.tool = if tool.eq_ignore_ascii_case("select") { Tool::Select } else { Tool::Explore };
    }
    if let Some(table) = p.get("table").and_then(Json::as_bool) {
        app.show_table = table;
    }
    if let Some(active) = p.get("active").and_then(Json::as_u64).and_then(|a| u32::try_from(a).ok()) {
        app.active = Some(active);
    }
    if let Some(page) = p.get("backstage") {
        use crate::BackstagePage as B;
        app.backstage = match page.as_str().map(str::to_ascii_lowercase).as_deref() {
            None | Some("") | Some("none") => None,
            Some("home") => Some(B::Home),
            Some("new") => Some(B::New),
            Some("open") => Some(B::Open),
            Some("info") => Some(B::Info),
            Some("export") => Some(B::Export),
            Some(other) => return Err(format!("unknown backstage page `{other}`")),
        };
    }
    Ok(())
}

fn render(app: &MapApp, p: &Json) -> Result<Json, String> {
    let path = p.get("path").and_then(Json::as_str).ok_or("path required")?;
    let dim = |k: &str, d: u64| p.get(k).and_then(Json::as_u64).unwrap_or(d).clamp(1, 8192) as u32;
    let extent = app.view.viewport().map(|v| v.extent());
    let png = mkaegis_render::export_png(&app.session.map, dim("width", 1280), dim("height", 800), extent, Some(&app.session.tiles))
        .map_err(|e| e.to_string())?;
    std::fs::write(path, png).map_err(|e| e.to_string())?;
    Ok(json!({ "path": path }))
}

fn save_png(path: &str, w: usize, h: usize, pixels: &[egui::Color32]) -> Result<(), String> {
    let bytes: Vec<u8> = pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), u32::try_from(w).unwrap_or(0), u32::try_from(h).unwrap_or(0));
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&bytes).map_err(|e| e.to_string())
}
