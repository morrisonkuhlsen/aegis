# MK aeGIS — instructions for agents

MK aeGIS is a clean-room, open-source, Rust-native **statistical GIS: a geographic information system built for statisticians**. Spatial statistics come first: spatial weights, autocorrelation (Moran's I, LISA, Getis-Ord), hot spots, spatial regression, point patterns, geostatistics, rate smoothing, and exploratory views linked to the map. The reference for map handling is Esri ArcGIS Pro (parity, then superiority: speed, openness, agent control); the reference for statistics is the published literature, implemented clean-room from papers and textbooks (Moran 1950, Geary 1954, Getis & Ord 1992, Anselin 1995, Fotheringham et al. for GWR, Cressie for kriging), never from GPL code (GeoDa, R's spdep, QGIS plugins). Every statistic lands with tests against independently computed values, and its outputs report what a statistician needs to judge them (inference method, permutations, p-values, multiple-testing correction). The sibling `../mksigma` (SPSS-class) is the convention reference for statistical output. It runs natively on Linux, Windows and macOS, and its engine and UI build for the web via WASM. Siblings with the same conventions: `../gridcraft` (Excel-class, the conventions' reference), `../mksigma` (SPSS-class), and `storytold/cadcraft` (AutoCAD-class, the closest in domain: coordinates, geometry, a command line).

Standards and learnings shared across the crafting apps live in `craftrules` (`../../craftrules` when checked out, or `storytold/craftrules`). When it is available, read its `AGENTS.md` at the start of a session, follow its standards, and contribute reusable learnings back there. Never code: repos don't share code.

## Start every session here
1. Read `plan/STATUS.md` (current milestone, next task), then the task in `plan/execution-plan.md` and the relevant `plan/architecture.md` section. Behaviour reference: `plan/arcgis/*.md` (`observed-ui.md` holds measured observations of the running app).
2. Library decisions: `plan/adr/0001-libraries.md` (what we may depend on, what is rejected for licence reasons).
3. Follow the autonomous operation protocol (`plan/execution-plan.md` §7). Don't stop to ask unless §7 lists the decision as the user's.

`plan/` is gitignored (local only).

## Assets and iconography — absolute rule
**No Esri, Microsoft, Google, Adobe, Autodesk, QGIS, RustGIS (or any other vendor's or project's) iconography, images, artwork, fonts, basemaps, styles, symbol sets, colour ramps, templates, themes or sample data — ever.** This is the most important rule in this repository; breaking it is a serious failure.
- Every asset in the repo is original work by MK aeGIS contributors, or public domain / CC0, or open source / Creative Commons that allows redistribution (or licensed open source by the contributor who created it).
- Every asset file has a row in [`ATTRIBUTION.md`](ATTRIBUTION.md) (author, source, licence). `cargo xtask assets` enforces it. Adding an asset without a row is a bug.
- Prefer art generated in code: the UI icon set (`crates/ui-egui/src/icons.rs`), symbols, colour ramps and palettes (`crates/model/src/symbology.rs`), themes (`crates/ui-egui/src/theme.rs`) and the sample map (`crates/engine/src/sample.rs`) are drawn/defined in code and original.
- Never copy ArcGIS's icons, ribbon artwork, ArcGIS 2D/3D styles (`.stylx`), symbol and colour-ramp definitions, layer files (`.lyrx`), project templates (`.aptx`), basemaps or vector tile styles, Living Atlas content, sample data, help text or wording beyond feature names. Feature *names* (ribbon labels, tool names such as Buffer or Select Layer By Location, pane names such as Contents) are fine.
- **Other GIS projects are the same.** QGIS (GPL: its icons, SVG symbols, styles, colour ramps, `.qml`/`.qgs`/`.qpt` files, sample data and help text) and RustGIS (`opengeos/RustGIS`: its logo, icons, screenshots, artwork, styles, sample projects and wording) are never copied, not even "for reference". RustGIS is Apache-2.0, but MK aeGIS is MIT OR Apache-2.0: Apache-only code would take the MIT option away, and the codebase stays our own. Their public documentation and feature lists may be read to compare features (as `plan/` comparisons); their code is never copied, ported or translated.
- No Esri, Google or other commercial basemaps or tile services, not even fetched at run time. Basemaps, when they come, are open data with a redistributable licence (OpenStreetMap-derived under ODbL, Natural Earth public domain) and a row in `ATTRIBUTION.md`.
- Screenshots of Esri software are never committed (they live only in `plan/arcgis/screenshots/`).
- Brand files, if MK aeGIS ever adopts some, live in `docs/brand/` under their own licence file, with rows in `ATTRIBUTION.md`: the one exception.

## Never crash
People trust MK aeGIS with their data and their maps; a crash loses their work. **This outranks feature work.** Standard: [`craftrules/standards/never-crash.md`](https://github.com/storytold/craftrules/blob/main/standards/never-crash.md).
- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"`).
- **Errors are `Result<T, E>`** through `aegis_core::Error` and `?`. An unfinished feature returns `Error::NotAvailable`; it never panics.
- **Input-derived numbers are hostile** (shapefiles, GeoJSON, project files, `WHERE` clauses, command, MCP and control params): `get()` instead of indexing, checked/saturating arithmetic, no NaN casts, cap input-sized allocations (a shapefile's declared counts, a dBASE header, an image size, a buffer distance; `MAX_ITEMS`).
- **Coordinates are hostile too:** non-finite values are rejected at the format boundary; a projection that cannot take a point (Web Mercator at the poles, UTM far from its zone) returns an error, never a NaN that travels on.
- **Bound recursion** (`WHERE` clause nesting: `MAX_DEPTH`; JSON depth: serde's limit) and prefer iterative algorithms (Douglas–Peucker, point-in-polygon, ring grouping).
- **Last-resort guard:** `Session::execute` catches escaped panics and restores the map. It's a safety net, not a licence.
- **Prove it:** every crash fix lands with a regression test (`tests::every_command_survives_empty_params` and `tests::hostile_params_never_panic` fuzz every command; `aegis-shp`'s `hostile_files_never_panic` truncates and flips every byte of a shapefile).
- Crates carry `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`; `cargo xtask panics` enforces it.

## Non-negotiables
- **Clean-room.** Only generally available releases of ArcGIS Pro (Windows) may be *observed* black-box, and only under a paid commercial licence held by the person or organisation developing MK aeGIS. Never under ArcGIS for Personal Use, Student, Trial or other evaluation licences, an ArcGIS Online public account, or an employer's licence without its written approval: those are noncommercial, evaluation-only or for the licensee's internal use (Esri Master Agreement E204CW §2.7, Product-Specific Terms E300). Observe on one machine (E300 allows automated operation on one only), with synthetic data and no Esri basemaps or Data (E204CW §3.3(h) bars using Esri Data to train machine systems): use the UI, take screenshots (by window id) stored only under `plan/arcgis/screenshots/`, and note in `plan/` the licence each observation was made under. ArcGIS Online, its map viewer and other Esri Cloud Services are not observed for parity and never benchmarked, published or not (E204CW B.8.1(f) bars benchmarking their availability, performance or functionality). Agents never fetch Esri web documentation (pro.arcgis.com, doc.arcgis.com): Esri's website terms (G577 §2.3) bar harvesting it or using it in AI tools; people may read it, and only feature names come from it. Never read, disassemble or copy anything inside an ArcGIS installation (ArcPy and toolbox sources, `.tbx`/`.pyt` toolboxes, style files, projection-engine data), never commit files produced by ArcGIS (`.aprx`, `.lyrx`, geodatabases, shapefiles it wrote), never incorporate any part of an Esri offering (code, assets, data, help text) into MK aeGIS (E204CW B.1(i)). Never publish benchmark results of Esri beta products (B.1(j)); speed comparisons with released ArcGIS Pro may be published only with the full methodology (versions, hardware, data, settings) and without tools that call Esri cloud services. Never copy GPL/AGPL/LGPL code (QGIS, GRASS, SAGA, gvSIG, GEOS, GeoTools, PostGIS, LibreCAD, GeoDa, spdep…) — don't even read their sources. Permissively licensed GIS projects (RustGIS, GeoLibre, PySAL) are not copied either: read their docs, never port their code. Tool behaviour comes from public documentation and observation. "Clean-room" here means independent implementation from public specifications, the literature and black-box observation, never from anyone's code; a commit names the sources it was built from.
- **File formats and math come from public specifications:** the *ESRI Shapefile Technical Description* (1998) and dBASE III, GeoJSON (RFC 7946), OGC Simple Features (WKT/WKB), OGC GeoPackage, the OGC web services (WMS, WMTS, WFS), the EPSG dataset (under its terms of use) and IOGP Guidance Note 7-2, USGS Professional Paper 1395 (Snyder) and Karney's papers. The File Geodatabase format has no public specification: out of scope until the user decides otherwise.
- **Dependencies** are permissively licensed (MIT, Apache-2.0, BSD, Zlib or similar), pure Rust, and build for wasm below L5. PROJ and GDAL (MIT) may be used *outside the repository* as black-box oracles to cross-check numbers (in a scratch venv: pyproj, pyshp); their outputs are re-typed as test constants, their code is never copied or linked.
- **Numerical parity is a feature.** Every projection, measure and tool lands with tests against independently computed reference values (by hand, by numerical integration, or from a documented external oracle), with explicit tolerances (`crates/crs` holds UTM to the integrated meridian arc within 1 mm).
- **Fonts are never committed.** The app uses egui's defaults; when fonts come they live in [`storytold/craft-fonts`](https://github.com/storytold/craft-fonts) as an optional build input (`CRAFT_FONTS_DIR`), never a `Cargo.toml` dependency. Code and tests must work without it.
- **Test data is built in code.** Real ArcGIS-authored data and large binary fixtures never enter the repository (`/corpus` is gitignored); a test corpus, when needed, lives in a separate repo, fetched pinned and sha256-verified (craftrules `standards/test-corpora.md`).
- **Everything is a command.** User-visible behaviour = a command in `crates/engine/src/cmd/*` (`CommandSpec`: id, label, ribbon path, shortcut, params doc, kind, `enabled`, `run`) + tests. Ids follow ArcGIS Pro's ribbon and geoprocessing toolboxes (`map.selectByAttributes`, `analysis.buffer`, `management.project`) plus primitives (`feature.select`, `layer.setVisible`, `map.setExtent`). The ribbon, panes, dialogs, shortcuts, control channel, CLI and MCP all reach the same commands.
- **Programmatic calls never open dialogs**: a command with empty params runs with defaults or returns an error; only the ribbon and menus open a dialog or a pane, and a dialog only gathers parameters (OK runs the command).
- **Layering** is enforced by `cargo xtask layers`: `core`/`geom`/`crs` (L0) → `model` (L1) → `geojson`/`shp`/`render` (L2) → `analysis`/`stats` (L3) → `engine` (L4) → `ui-egui`/`mcp` (L5) → `apps/*` (L6). Nothing below L5 depends on egui/eframe/winit/wgpu/rfd. **The UI crate is swappable.**
- **The UI is thin**: panels read `Session` state and act through `app.run(id, params)`. Colours come from `theme::Tokens`. Platform services (file dialogs) come in through `Services`. The map view paints `aegis-render`'s output: what you see is what Export Map writes.
- **Rust only** (no handwritten JS/TS). **Never break wasm** (`cargo xtask wasm`).
- **Quality gates** before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests, assets, layers, panics, wasm). One task id per commit (`M1.2: polyline buffer`). Push only when the user asks.

## Running and looking at the app
- `cargo run --release -p aegis -- --sample --control 7979` (sample map + control channel). Pick a free port; other crafting apps use control ports too.
- Drive it: JSON lines on `127.0.0.1:7979`, e.g. `{"id":1,"method":"engine.execute","params":{"command":"map.selectByAttributes","params":{"layer":"Cidades","where":"POP > 100000"}}}` then `{"id":2,"method":"ui.screenshot","params":{"path":"/tmp/shot.png"}}` (or `ui.render` for the map alone, no presented frame needed). Methods: `crates/ui-egui/src/control.rs`, docs: `docs/control-protocol.md`.
- **Offscreen UI render** (no window, from a source checkout): `cargo run -p aegis-ui-egui --example snapshot -- --sample [--table] [--pane symbology] [--tab analysis] [--tool buffer] [--cmd 'id={json}']… [--dark] out.png`. **Never screenshot the desktop**: it captures whatever else the person has open.
- Headless: `aegis-cli info data.shp`, `aegis-cli run --sample --cmd 'analysis.buffer={"layer":"Cidades","distance":10,"unit":"km"}' --out buffers.geojson --png map.png`, `aegis-cli commands`, `aegis-cli mcp` (`claude mcp add aegis -- aegis-cli mcp`).
- **For UI work, look at the result** (snapshot PNG) and compare with `plan/arcgis/observed-ui.md`.
- UI behaviour is tested headless with `egui_kittest` (`crates/ui-egui/src/lib.rs` tests).
- Shell gotchas: `cargo fmt` reformats code, so re-read a file before an exact-text edit of what it touched; a gate run chained with `;` doesn't stop on a failure — use `cargo xtask ci && git commit …`.
- Parallel agents: separate `CARGO_TARGET_DIR` per agent; edit only the crates you own; delete your target dir when done.

## Code style
- Never use `println!` or `eprintln!` outside tests, the CLI's own output and `xtask`; use the `log` crate's macros.
- When two crates export the same type name, alias the import with a suffix: `use serde_json::Value as Json;`, `use foo::Bar as BarFoo;`.
- Prefer `use` imports over inline fully-qualified paths; qualify inline only for true one-offs or std collisions.
- Each command is a `CommandSpec` constant in the `cmd` module of its ribbon tab (`project`, `map`, `layer`, `feature`, `analysis`, `stats`), listed in `COMMANDS` in ribbon order; shared parameter helpers live in `cmd/mod.rs`.

### File layout
Organize for top-to-bottom reading. Important things first, details later.
- **Constants** at the top (after imports).
- **Structs/enums** next; outer structs above inner sub-structs.
- **In impl blocks**: constructors first, then public methods, then private helpers.
- Private helpers go *below* the functions that call them; among helpers, meatier logic above leaf-level formatters.
- **In test modules**: constants first, then test cases (grouped into sub-modules when 2+), then helper functions last.

## Markdown
- **Tables must be space-padded so columns align in plain text.** Markdown tables are read raw (terminals, diffs, editors) at least as often as rendered, and condensed tables are unreadable there. Pad every cell to its column width:

  ```markdown
  | Tool              | Toolbox         | Status      |
  |-------------------|-----------------|-------------|
  | Buffer            | Analysis        | points      |
  | Project           | Data Management | done        |
  ```

  Not: `| Tool | Toolbox | Status |` packed tight with varying widths per row.

## Roadmap
`ROADMAP.md` (committed) is the one-page summary: stage, headline numbers, dimensions, progress log. It follows craftrules `standards/progress-docs.md` and points at the detail in `docs/`: `target-app-parity.md` (authoritative assessment), `gaps.md` (ranked work list: pick from it), `roadmap.md` (current focus, beta gates, milestones), `architecture.md`, and the area checklists `geoprocessing-parity.md`, `projection-parity.md`, `file-format-parity.md`, `ui-parity.md`, `hardware-parity.md`, `localization-parity.md`. Update the affected docs (and their timestamp lines and revision history) whenever work lands. `cargo xtask parity` regenerates `docs/parity-checklist.md` from the command registry (`crates/engine/src/cmd`).
