# ADR 0001: Libraries we may depend on

*Status: accepted. Updated 2026-10-22.*

## Context

MK aeGIS is MIT OR Apache-2.0, runs on Linux, Windows and macOS, and builds its engine and UI for
the web as WebAssembly. Its rules (AGENTS.md, "Non-negotiables") are clean-room: no GPL, AGPL or
LGPL code, not even read, and file formats and math come from public specifications. Every
dependency is code we ship, so it has to fit all of that.

## Decision

A dependency is allowed when it is:

1. **Permissively licensed:** MIT, Apache-2.0, BSD, Zlib, ISC, BSL-1.0 or similar, so that the
   binaries stay distributable under MIT OR Apache-2.0. A crate offered under a choice of licences
   (`MIT OR Apache-2.0 OR LGPL-2.1-or-later`) is taken under a permissive one.
2. **Pure Rust:** no C or C++ libraries to build or link, so `cargo build` is the whole build on
   every platform.
3. **Wasm-clean below the UI layer:** crates at L0–L4 build for `wasm32-unknown-unknown`
   (`cargo xtask wasm`). Only L5 and above may use the UI and platform stack (egui, eframe, winit,
   wgpu, rfd; `cargo xtask layers`).
4. **Worth it:** small, maintained, and doing something we shouldn't write ourselves. Geometry,
   projections, file formats and statistics are the product, so they are written here, from
   public specifications and the literature.

Test-only dependencies (`[dev-dependencies]`) follow the same licence rule; they aren't shipped,
so a weak-copyleft crate deep in a test tool's tree (today `colored`, MPL-2.0, through
`egui_kittest`) is tolerated there, never in a shipped binary.

## Direct dependencies today

| Crate                    | Licence                   | Used by          | Why                                        |
|--------------------------|---------------------------|------------------|--------------------------------------------|
| `thiserror`              | MIT OR Apache-2.0         | core             | The error type                             |
| `serde`, `serde_json`    | MIT OR Apache-2.0         | most crates      | Projects, GeoJSON, command params, MCP     |
| `log`                    | MIT OR Apache-2.0         | engine, UI, apps | Logging instead of `println!`              |
| `png`                    | MIT OR Apache-2.0         | render, UI       | Export Map, snapshots, the splash wordmark |
| `quick-xml`              | MIT                       | kml              | XML for KML; no DTD entities expanded      |
| `ureq`                   | MIT OR Apache-2.0         | desktop app      | Basemap tiles over HTTPS (L6, native only) |
| `zune-jpeg`, `zune-core` | MIT OR Apache-2.0 OR Zlib | render, UI       | Basemap tiles (JPEG), the splash artwork   |
| `egui`, `eframe`         | MIT OR Apache-2.0         | UI, desktop app  | The interface (L5+)                        |
| `rfd`                    | MIT                       | desktop, web app | File dialogs; the browser's picker (L6)    |
| `wasm-bindgen`, `js-sys` | MIT OR Apache-2.0         | web app          | The browser's APIs from Rust (L6, wasm32)  |
| `wasm-bindgen-futures`   | MIT OR Apache-2.0         | web app          | Awaiting the browser's promises            |
| `web-sys`                | MIT OR Apache-2.0         | web app          | Downloads, the canvas, leaving the page    |
| `egui_kittest`           | MIT OR Apache-2.0         | UI tests         | Headless UI tests                          |

`ureq` brings `rustls` with the `ring` crypto provider (ISC and Apache-2.0 style licences), whose
core is C and assembly: the one dependency that isn't pure Rust. It is the desktop app's alone (L6,
never wasm: the web app fetches through the browser), and it builds with the C compiler every
Rust target already has. The alternative, the OS's TLS through `native-tls`, would need OpenSSL's
headers on Linux.

egui embeds its default fonts (`epaint_default_fonts`: Ubuntu Light under the Ubuntu Font
Licence, Noto Emoji under the OFL 1.1, Hack and emoji-icon-font under MIT); `NOTICE` names them.

## Rejected

| Library                                                  | Licence  | Why not                                                                                                      |
|----------------------------------------------------------|----------|--------------------------------------------------------------------------------------------------------------|
| GEOS                                                     | LGPL-2.1 | Copyleft, and C++                                                                                            |
| GDAL/OGR                                                 | MIT      | C/C++ and doesn't build for wasm. Allowed only outside the repository, as a black-box oracle for test values |
| PROJ                                                     | MIT      | C/C++. Same: an oracle for test values (pyproj), never linked                                                |
| GPL GIS code (QGIS, GRASS, SAGA, GeoDa, R's spdep…)      | GPL/AGPL | Copyleft; under the clean-room rule not even read                                                            |
| Permissive GIS projects' code (RustGIS, GeoLibre, PySAL) | various  | Their docs may be read; their code is never ported, so the codebase stays our own                            |

## Consequences

- Adding a dependency is a decision: name it in the pull request with its licence and why it
  can't be written here, and add it to the table above.
- `cargo xtask wasm` and `cargo xtask layers` catch a dependency in the wrong layer; the licence
  check is by review today. A `cargo deny` licence gate is a candidate for `cargo xtask ci`.
