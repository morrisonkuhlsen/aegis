# MK aeGIS roadmap

*Updated 2026-10-21, at version 0.1.2.*

MK aeGIS is a statistical GIS: a geographic information system built for statisticians. This page
is the one-page summary; the detail is in [`docs/`](docs/).

## Stage

**Alpha (0.1.x).** The map handling a statistician needs before any statistic works: open
shapefiles and GeoJSON, project on the fly, symbolise, select by attributes and by location,
edit attributes, buffer points, save a project, export a map. Spatial statistics have started
with spatial weights. Nothing is stable yet: project files and command parameters may change
between 0.x versions.

**Next stage, beta**, when the core of exploratory spatial data analysis works end to end, from a
shapefile to a defensible result: global and local autocorrelation with permutation inference
and multiple-testing correction, statistical classification, linked exploratory views, CSV join,
and no open `critical` or `high` bug. The ranked list is [`docs/gaps.md`](docs/gaps.md).

## Headline numbers

From `cargo xtask stats` and `cargo xtask parity`:

| Measure                | Value  |
|------------------------|--------|
| Commands               | 44     |
| Rust lines (15 crates) | 13,859 |
| Tests (`#[test]`)      | 127    |
| Releases               | 3      |
| Open issues (bugs)     | 12     |
| Open issues (features) | 43     |

## Dimensions

| Dimension          | Where it stands                                                                                                                                | Detail                                                 |
|--------------------|------------------------------------------------------------------------------------------------------------------------------------------------|--------------------------------------------------------|
| Spatial statistics | Weights (contiguity, k-NN, distance band, kernel; GAL/GWT); global Moran's I, Geary's C, General G; LISA and Gi* hot spots with FDR/Bonferroni | issues #25–#36                                         |
| Geoprocessing      | Buffer (points), Project, Calculate Geometry, Select By Location, Summary Statistics                                                           | [`docs/parity-checklist.md`](docs/parity-checklist.md) |
| Coordinate systems | WGS 84, SIRGAS 2000, Web Mercator, UTM on both datums                                                                                          | issue #9                                               |
| File formats       | Shapefile and GeoJSON in and out; `.aegis` projects; PNG export                                                                                | issue #14                                              |
| User interface     | Ribbon, Contents, map, Geoprocessing/Symbology/History panes, table, light/dark                                                                | [`docs/architecture.md`](docs/architecture.md)         |
| Automation         | Every action is a command: CLI, MCP server, control channel                                                                                    | [`docs/control-protocol.md`](docs/control-protocol.md) |
| Platforms          | Linux x86_64, Windows x64, macOS universal; engine and UI build for wasm                                                                       | [`docs/releasing.md`](docs/releasing.md)               |

## Progress log

| Date       | Version | What landed                                                                                    |
|------------|---------|------------------------------------------------------------------------------------------------|
| 2026-10-21 | 0.1.2   | 14 bugs fixed: lost work and crashes, SQL NULL logic, weights, measures, Select By Location    |
| 2026-10-21 | 0.1.1   | Windows starts on DirectX 12 (#82), no console window (#78); MK Sigma's dark palette           |
| 2026-10-21 | 0.1.0   | First release: map handling, spatial weights (#22), Statistics tab, CI (#19), release archives |
