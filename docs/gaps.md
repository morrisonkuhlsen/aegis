# Gaps: the ranked work list

*Updated 2026-10-21.*

Pick work from the top. The order follows the project's rules: a crash or lost work outranks
everything ("Never crash" in [`AGENTS.md`](../AGENTS.md)); then wrong statistical results, since
MK aeGIS is a GIS for statisticians; then the statistics themselves; then map handling. Each row is
a GitHub issue, where the detail and the acceptance criteria are.

## 1. Lost work and crashes

| Issue | Gap                                                                        | Priority |
|-------|----------------------------------------------------------------------------|----------|
| #41   | Undo past `project.open`/`project.new` keeps the path; Ctrl+S overwrites   | critical |
| #42   | Project save is not atomic                                                 | high     |
| #43   | Long AND/OR chains bypass the WHERE depth limit: stack overflow            | high     |
| #45   | New Map from the command search discards unsaved changes                   | high     |
| #44   | Ctrl+Shift+Z undoes; shortcuts fire while typing                           | high     |
| #59   | Undo clones the whole map before every Edit command, with no memory budget | medium   |
| #58   | Opening a project does no validation                                       | medium   |

## 2. Wrong statistical results

| Issue | Gap                                                                         | Priority |
|-------|-----------------------------------------------------------------------------|----------|
| #46   | Contiguity fails on projected coordinates ≥ 9,000,000 (northern Brazil UTM) | high     |
| #47   | SQL NULL ignores three-valued logic: `NOT (POP > x)` selects NULL rows      | high     |
| #4    | Summary Statistics on a Text field reports nulls and a sum of -0.0          | high     |
| #48   | Adaptive kernel weights: the k-th neighbour gets weight 0                   | medium   |
| #49   | Weights outlive their layer; GAL/GWT round trips are not safe               | medium   |
| #7    | Calculate Field division by zero wipes the column and reports success       | medium   |

## 3. Wrong geometry, coordinates and data

| Issue | Gap                                                                              | Priority |
|-------|----------------------------------------------------------------------------------|----------|
| #50   | Web Mercator layers measured and buffered as ground metres                       | high     |
| #51   | `within()` ignores holes and shared borders; WithinDistance ignores the distance | high     |
| #52   | Ring grouping tests only the first vertex                                        | high     |
| #55   | The dBASE writer loses data                                                      | high     |
| #6    | An unrecognised `.prj` is silently assumed to be WGS 84                          | high     |
| #3    | `mkaegis-cli --out` writes the top layer, not the command's output               | high     |
| #53   | The `.prj` name fallback ignores the datum (SAD69, ED50, Córrego Alegre)         | medium   |
| #54   | Projection edge cases return clamped values or NaN                               | medium   |
| #57   | One bad feature aborts the whole GeoJSON or Shapefile read                       | medium   |

## 4. Statistics: the road to beta

| Issue | Feature                                                                        | Priority |
|-------|--------------------------------------------------------------------------------|----------|
| #23   | Global autocorrelation: Moran's I, Geary's C, General G, permutation inference | high     |
| #24   | LISA cluster maps and Getis-Ord Gi* hot spots, with FDR/Bonferroni             | high     |
| #26   | Statistical classification: natural breaks, std dev, box map, percentile map   | high     |
| #25   | Linked exploratory views and brushing                                          | high     |
| #27   | CSV import with types; join by key with a join report                          | high     |
| #28   | Full descriptive statistics                                                    | high     |
| #29   | Rates: excess risk, Empirical Bayes and spatial EB smoothing                   | medium   |
| #30   | Spatial regression: OLS diagnostics, lag and error models                      | medium   |
| #31   | Point pattern analysis                                                         | medium   |
| #32   | Reproducible analyses: history as a script, seeds, report                      | medium   |

## 5. Everything else

The other open issues (map handling, formats, CRS coverage, survey tools, precision agriculture,
interop, the web app, project hygiene) are ranked by their `priority:` label on
[GitHub](https://github.com/morrisonkuhlsen/mkaegis/issues).

## Revision history

| Date       | Change                                    |
|------------|-------------------------------------------|
| 2026-10-21 | First version, from the open issues (#62) |
