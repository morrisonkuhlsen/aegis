# Architecture

*Updated 2026-10-21.*

MK aeGIS is a Cargo workspace of small crates in layers. A crate depends only on crates at its
layer or below, and nothing below the UI layer depends on egui, eframe, winit, wgpu or rfd:
`cargo xtask layers` enforces both. The UI crate is swappable, and everything below it builds for
WebAssembly (`cargo xtask wasm`).

## Layers

| Layer | Crate              | What it holds                                                                                    |
|-------|--------------------|--------------------------------------------------------------------------------------------------|
| L0    | `mkaegis-core`     | The error type every crate returns, attribute values, colours                                    |
| L0    | `mkaegis-geom`     | Planar geometry: coordinates, envelopes, point/multipoint/polyline/polygon, algorithms, WKT      |
| L0    | `mkaegis-crs`      | Coordinate reference systems by EPSG code and the projection math (Snyder, Karney)               |
| L1    | `mkaegis-model`    | The map document: feature classes, layers, the map, symbology, SQL `WHERE`, spatial weights      |
| L2    | `mkaegis-geojson`  | GeoJSON (RFC 7946) in and out                                                                    |
| L2    | `mkaegis-kml`      | KML 2.2 (OGC 07-147r2) in and out: a layer per geometry family                                   |
| L2    | `mkaegis-csv`      | Delimited text tables (CSV, TSV) in and out, with type inference, from RFC 4180                  |
| L2    | `mkaegis-shp`      | Shapefile (`.shp/.shx/.dbf/.prj/.cpg`) in and out, from the public technical description         |
| L2    | `mkaegis-render`   | Viewport, display list and a software rasteriser to PNG                                          |
| L3    | `mkaegis-analysis` | Geoprocessing tools: Buffer, Project, Calculate Geometry, Select By Location, Summary Statistics |
| L3    | `mkaegis-stats`    | Spatial statistics, clean-room from the literature: spatial weights, GAL/GWT                     |
| L4    | `mkaegis-engine`   | The `Session`: the map, undo/redo, the history, and every command                                |
| L5    | `mkaegis-ui-egui`  | The egui frontend and the control channel                                                        |
| L5    | `mkaegis-mcp`      | The Model Context Protocol server (stdio)                                                        |
| L6    | `mkaegis`          | The desktop app                                                                                  |
| L6    | `mkaegis-cli`      | The headless command line, which also serves MCP                                                 |
| L6    | `mkaegis-web`      | The web app: the desktop's window in the browser, files kept in memory (`Disk::Memory`)          |

`xtask` holds the project's tasks (`cargo xtask ci` runs every quality gate).

## Everything is a command

Everything a person can do is a command in `crates/engine/src/cmd/`: a `CommandSpec` with an id
(`map.selectByAttributes`, `analysis.buffer`, `stats.weights`), a label, a ribbon path, a
shortcut, its parameters, and the function that runs it. The ribbon, panes, dialogs, shortcuts,
the control channel, the CLI and the MCP server all go through `Session::execute`, so they can't
drift apart. `cargo xtask parity` lists every command in
[`parity-checklist.md`](parity-checklist.md).

Called programmatically, a command never opens a dialog: with empty parameters it runs with
defaults or returns an error. Only the ribbon and menus open dialogs, and a dialog only gathers
parameters; OK runs the command.

`Session::execute` records each command for undo and in the geoprocessing history, and catches a
panic that escapes a command, restoring the map. That is a last-resort guard: commands return
errors and never panic (`cargo xtask panics`).

## The UI is thin

Panels read the session's state and act only through `MapApp::run`, which executes commands.
Colours come from `theme::Tokens`; platform services (file dialogs) come in through `Services`.
The map view paints `mkaegis-render`'s display list, so what you see is what Export Map writes.

## Driving it from outside

| Way in          | Where                                 | Reference                                    |
|-----------------|---------------------------------------|----------------------------------------------|
| CLI             | `mkaegis-cli run --cmd 'id={json}'`   | `mkaegis-cli commands`                       |
| MCP             | `mkaegis-cli mcp`                     | `crates/mcp/src/lib.rs`                      |
| Control channel | `mkaegis --control PORT` (JSON lines) | [`control-protocol.md`](control-protocol.md) |

## Revision history

| Date       | Change              |
|------------|---------------------|
| 2026-10-21 | First version (#62) |
