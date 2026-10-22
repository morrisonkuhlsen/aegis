//! MK aeGIS in the browser.
//!
//! The same [`mkaegis_ui_egui::MapApp`] as the desktop app, through eframe's web runner (wgpu:
//! WebGPU where the browser has it, WebGL2 otherwise). Build with `trunk build --release` in this
//! directory (the site lands in `dist/web`).
//!
//! What differs from the desktop: everything stays in the page, nothing is uploaded. The session's
//! files are held in memory (`mkaegis_engine::Disk::Memory`): Add Data and Open use the browser's
//! file picker, files dropped on the page open by extension, and whatever a command writes (Save,
//! Export Map, Export Features, Export Weights) downloads. No control channel, no splash.
//!
//! URL flags: `?sample` opens the sample map; `?basemap=openStreetMap` (or `satellite`) opens with
//! that basemap; `?webgl` skips WebGPU.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

/// Built natively only so the workspace's checks cover it; it runs in the browser.
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    log::error!("mkaegis-web runs in the browser: build it with `trunk build --release` in apps/mkaegis-web");
}
