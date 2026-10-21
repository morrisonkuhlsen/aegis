# ADR 0001: Libraries we may depend on

*Status: accepted. Updated 2026-10-21.*

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
| `zune-jpeg`, `zune-core` | MIT OR Apache-2.0 OR Zlib | UI               | The splash artwork                         |
| `egui`, `eframe`         | MIT OR Apache-2.0         | UI, desktop app  | The interface (L5+)                        |
| `rfd`                    | MIT                       | desktop app      | Native file dialogs (L6)                   |
| `egui_kittest`           | MIT OR Apache-2.0         | UI tests         | Headless UI tests                          |

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
