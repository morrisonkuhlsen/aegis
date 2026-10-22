# Gaps: the ranked work list

*Updated 2026-10-21, after 0.1.2.*

Pick work from the top. The order follows the project's rules: a crash or lost work outranks
everything ("Never crash" in [`AGENTS.md`](../AGENTS.md)); then wrong statistical results, since
MK aeGIS is a GIS for statisticians; then the statistics themselves; then map handling. Each row is
a GitHub issue, where the detail and the acceptance criteria are.

## 1. Lost work and crashes

None open: #58 and #59 were fixed after 0.1.2 (see [`CHANGELOG.md`](../CHANGELOG.md)).

## 2. Wrong statistical results

None open: the six known ones were fixed in 0.1.2 (see [`CHANGELOG.md`](../CHANGELOG.md)).

## 3. Wrong geometry, coordinates and data

| Issue | Gap                                                                               | Priority |
|-------|-----------------------------------------------------------------------------------|----------|
| #52   | Buffer of a multipoint: overlapping circles aren't unioned (rings fixed in 0.1.2) | high     |
| #3    | `mkaegis-cli --out` writes the top layer, not the command's output                | high     |
| #53   | The `.prj` name fallback ignores the datum (SAD69, ED50, Córrego Alegre)          | medium   |
| #54   | Projection edge cases return clamped values or NaN                                | medium   |
| #57   | One bad feature aborts the whole GeoJSON or Shapefile read                        | medium   |

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
| 2026-10-21 | Fixed issues removed after 0.1.2          |
| 2026-10-21 | #58, #59 and #55 removed                  |
| 2026-10-21 | #6 removed                                |
