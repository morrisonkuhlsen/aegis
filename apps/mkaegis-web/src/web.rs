//! The browser shell: the web `Services` (file picker, downloads), files dropped on the page, the
//! guard against leaving with unsaved changes, and eframe's web runner.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::PoisonError;

use eframe::egui_wgpu::{WgpuSetup, wgpu};
use mkaegis_engine::Session;
use mkaegis_ui_egui::{Arrival, Inbox, Language, MapApp, Services};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;

const CANVAS_ID: &str = "mkaegis_canvas";
const LOADING_ID: &str = "mkaegis_loading";
/// Where the interface language is kept between visits.
const LANGUAGE_KEY: &str = "mkaegis.language";

/// The app, and what the page around it needs from each frame.
struct Shell {
    app: MapApp,
    inbox: Inbox,
    /// The map has unsaved changes: leaving the page asks first.
    unsaved: Rc<Cell<bool>>,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        if !dropped.is_empty() {
            // Read together, so a shapefile dropped with its sidecars arrives as one.
            let (inbox, ctx) = (Inbox::clone(&self.inbox), ctx.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let mut files = Vec::with_capacity(dropped.len());
                for f in &dropped {
                    let name = f.path().display().to_string();
                    match f.bytes_async().await {
                        Ok(bytes) => files.push((name, bytes)),
                        Err(e) => log::error!("couldn't read the dropped file {name}: {e}"),
                    }
                }
                deliver(&inbox, &ctx, Arrival { files, dropped: true });
            });
        }
        self.app.ui(ui);
        self.unsaved.set(self.app.session.modified);
    }
}

pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            log::error!("the page has no <canvas id=\"{CANVAS_ID}\">");
            return;
        };
        let mut options = eframe::WebOptions::default();
        if has_flag("webgl")
            && let WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup
        {
            setup.instance_descriptor.backends = wgpu::Backends::GL;
        }
        let unsaved = Rc::new(Cell::new(false));
        guard_leaving(Rc::clone(&unsaved));
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(move |cc| {
                    // `Session::default()` keeps its files in memory here: there is no file system.
                    let session = if has_flag("sample") { Session::sample() } else { Session::default() };
                    let inbox = Inbox::default();
                    let mut app = MapApp::new(session);
                    app.services = services(Inbox::clone(&inbox), cc.egui_ctx.clone());
                    app.language = language();
                    Ok(Box::new(Shell { app, inbox, unsaved }))
                }),
            )
            .await;
        if let Some(loading) = document.get_element_by_id(LOADING_ID) {
            match result {
                Ok(()) => loading.remove(),
                Err(e) => {
                    log::error!("MK aeGIS failed to start: {e:?}");
                    loading.set_text_content(Some("MK aeGIS couldn't start: it needs a browser with WebGPU or WebGL2."));
                }
            }
        }
    });
}

/// The browser's file picker for opening, and downloads for what commands write.
fn services(inbox: Inbox, ctx: egui::Context) -> Services {
    let pick_inbox = Inbox::clone(&inbox);
    Services {
        pick_files: Some(Box::new(move |title: &str, extensions: &[&str], several: bool| {
            let dialog = rfd::AsyncFileDialog::new().set_title(title).add_filter(title, extensions);
            let (inbox, ctx) = (Inbox::clone(&pick_inbox), ctx.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let picked = if several { dialog.pick_files().await } else { dialog.pick_file().await.map(|f| vec![f]) };
                let Some(picked) = picked else { return };
                let mut files = Vec::with_capacity(picked.len());
                for f in picked {
                    files.push((f.file_name(), f.read().await));
                }
                deliver(&inbox, &ctx, Arrival { files, dropped: false });
            });
        })),
        download: Some(Box::new(|name: &str, bytes: &[u8]| {
            if let Err(e) = download(name, bytes) {
                log::error!("couldn't download {name}: {e}");
            }
        })),
        save_language: Some(Box::new(|language: Language| {
            if let Some(store) = local_storage()
                && let Err(e) = store.set_item(LANGUAGE_KEY, language.code())
            {
                log::warn!("couldn't keep the language: {e:?}");
            }
        })),
        inbox: Some(inbox),
        ..Default::default()
    }
}

/// Hands files to the app and wakes it to take them.
fn deliver(inbox: &Inbox, ctx: &egui::Context, arrival: Arrival) {
    inbox.lock().unwrap_or_else(PoisonError::into_inner).push(arrival);
    ctx.request_repaint();
}

/// Saves `bytes` as a download named `name`, through a link to a blob.
fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(media_type(name));
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
    let link: web_sys::HtmlAnchorElement = document.create_element("a").map_err(js)?.dyn_into().map_err(|_| "not a link")?;
    link.set_href(&url);
    link.set_download(name);
    link.click();
    // The download has its bytes once it starts; the blob is let go a little later.
    let revoke = Closure::once_into_js(move || {
        web_sys::Url::revoke_object_url(&url).ok();
    });
    window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000).map_err(js)?;
    Ok(())
}

fn media_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("geojson") => "application/geo+json",
        Some("kml") => "application/vnd.google-earth.kml+xml",
        Some("json" | "aegis") => "application/json",
        Some("csv") => "text/csv",
        Some("gal" | "gwt" | "prj" | "cpg") => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Leaving the page (closing the tab, reloading, going elsewhere) with unsaved changes asks first:
/// the browser's own question, the one way a page can.
fn guard_leaving(unsaved: Rc<Cell<bool>>) {
    let Some(window) = web_sys::window() else { return };
    let on_leave = Closure::<dyn FnMut(web_sys::BeforeUnloadEvent)>::new(move |event: web_sys::BeforeUnloadEvent| {
        if unsaved.get() {
            event.prevent_default();
        }
    });
    if window.add_event_listener_with_callback("beforeunload", on_leave.as_ref().unchecked_ref()).is_ok() {
        // Listening as long as the page lives.
        on_leave.forget();
    }
}

/// The language chosen on an earlier visit; else the browser's, when the interface has it.
fn language() -> Language {
    let saved = local_storage().and_then(|s| s.get_item(LANGUAGE_KEY).ok().flatten());
    let browser = web_sys::window().and_then(|w| w.navigator().language());
    saved.into_iter().chain(browser).find_map(|code| Language::from_code(&code)).unwrap_or_default()
}

/// The browser's storage for this site, when it allows it (a private window may not).
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// The page's address has `?flag` (or `&flag`, `flag=…`).
fn has_flag(flag: &str) -> bool {
    let search = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
    web_sys::UrlSearchParams::new_with_str(&search).is_ok_and(|p| p.has(flag))
}
