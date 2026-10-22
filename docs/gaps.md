# Gaps: the ranked work list

*Updated 2026-10-22, at 0.4.0.*

Pick work from the top. The order follows the project's rules: a crash or lost work outranks
everything ("Never crash" in [`AGENTS.md`](../AGENTS.md)); then wrong statistical results, since
MK aeGIS is a GIS for statisticians; then the statistics themselves; then map handling. Each row is
a GitHub issue, where the detail and the acceptance criteria are.

## 1. Lost work and crashes

None open: #58 and #59 were fixed after 0.1.2 (see [`CHANGELOG.md`](../CHANGELOG.md)).

## 2. Wrong statistical results

None open: the six known ones were fixed in 0.1.2 (see [`CHANGELOG.md`](../CHANGELOG.md)).

## 3. Wrong geometry, coordinates and data

None open: #57 and #54 were fixed after 0.1.2 (see [`CHANGELOG.md`](../CHANGELOG.md)).

## 4. Statistics: the road to beta

MK aeGIS is a spatial statistics workbench ([ADR 0002](adr/0002-positioning.md)): this is where
work goes first.

| Issue | Feature                                                                                             | Priority |
|-------|-----------------------------------------------------------------------------------------------------|----------|
| #28   | Full descriptive statistics                                                                         | high     |
| #29   | Rates: excess risk, Empirical Bayes and spatial EB smoothing                                        | medium   |
| #30   | Spatial regression: OLS diagnostics, lag and error models                                           | medium   |
| #32   | Reproducible analyses: history as a script, seeds, report                                           | medium   |
| #26   | Classification, the rest: diverging ramps on a chosen centre, rate legends                          | medium   |
| #25   | Linked views, the rest: LOWESS, scatter matrix, parallel coordinates, small multiples, chart export | medium   |
| #27   | Tables, the rest: standalone tables in the map, Add Join (temporary), GeoParquet                    | medium   |
| #31   | Point pattern analysis                                                                              | medium   |
| #34   | Geostatistics: IDW, variograms, kriging, cross-validation                                           | medium   |
| #33   | Geographically Weighted Regression                                                                  | low      |

## 5. Brazil: land surveying for SIGEF and INCRA

The vertical ([ADR 0002](adr/0002-positioning.md)).

| Issue | Feature                                                                | Priority |
|-------|------------------------------------------------------------------------|----------|
| #66   | Local Geodetic System (SGL): areas and azimuths as INCRA computes them | high     |
| #69   | SIGEF spreadsheet (ODS) with pre-validation                            | high     |
| #67   | Traverses, radiations and levelling                                    | high     |
| #68   | Descriptive memorial of a parcel                                       | medium   |

## 6. Map handling the statistics need

General GIS is kept "enough", not chased: these are what analyses depend on.

| Issue | Feature                                                  | Priority |
|-------|----------------------------------------------------------|----------|
| #8    | Buffer of polylines and polygons                         | high     |
| #10   | Overlay: Clip, Intersect, Union, Erase, Dissolve         | high     |
| #9    | Wider CRS coverage and the EPSG table                    | high     |
| #14   | More formats: GeoPackage, CSV XY/WKT, GeometryCollection | medium   |
| #39   | Rasters as inputs and outputs of statistics              | high     |

## 7. Everything else

The other open issues (precision agriculture, imagery, interop, layouts, project hygiene) are
ranked by their `priority:` label on [GitHub](https://github.com/morrisonkuhlsen/mkaegis/issues),
after the sections above. 3D, terrain, plugins, routing and vector tile styling are not planned.

## Revision history

| Date       | Change                                    |
|------------|-------------------------------------------|
| 2026-10-21 | First version, from the open issues (#62) |
| 2026-10-21 | Fixed issues removed after 0.1.2          |
| 2026-10-21 | #58, #59 and #55 removed                  |
| 2026-10-21 | #6 removed                                |
| 2026-10-21 | #3 removed                                |
| 2026-10-21 | #52 removed                               |
| 2026-10-21 | #53 removed                               |
| 2026-10-21 | #23 removed (the Moran scatter plot: #25) |
| 2026-10-21 | #24 removed                               |
| 2026-10-21 | #26 narrowed to what remains              |
| 2026-10-21 | #25 narrowed to what remains              |
| 2026-10-21 | #27 narrowed to what remains              |
| 2026-10-22 | #57 removed                               |
| 2026-10-22 | #54 removed                               |
| 2026-10-22 | Reordered by ADR 0002: statistics, Brazil |
