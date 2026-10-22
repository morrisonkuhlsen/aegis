# Changelog

Every release of MK aeGIS, newest first, in the spirit of
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow
[semantic versioning](https://semver.org/); before 1.0, a minor version may break projects and
commands.

**Changed results** lists the changes that give different numbers or selections for the same
data and parameters. Read it before re-running an analysis with a new version.

## [Unreleased]

### Added

- **The interface in Portuguese (Brazil)** (#37): View ▸ Language switches between English and
  Português (Brasil), and the choice is kept for later runs; the web app starts in the browser's
  language, the desktop in the one chosen before (on Linux and macOS, else the system's). Ribbon,
  panes, forms, results, dialogs, the File backstage and the splash are translated, and numbers
  on screen follow the language (decimal comma, thousands point; L and O for East and West).
  Command ids, parameters, files, the CLI and MCP don't change. The messages commands return
  are still in English (`docs/localization-parity.md`). `cargo xtask i18n` (part of `ci`) fails
  on a string missing from the catalogue or shown without a translation lookup.
- **KML** (#14, in part): Add Data reads `.kml` (KML 2.2) and Export Features writes it, on the
  desktop, in the CLI and in the web app. Placemarks in any Folder are read with their `name`,
  `description` and `ExtendedData` (`Data` and `SchemaData`), typed from their values. A document
  mixing points, lines and polygons (a SIGEF parcel with its vertices) becomes a layer for each:
  `<name>_Polygons`, `<name>_Lines`, `<name>_Points`. A Placemark whose geometry can't be read
  keeps its attributes with a null geometry, and the warnings count it. Export writes WGS 84,
  with `name` and `description` from fields so named and the other fields as `ExtendedData`.
  `.kmz` is not read yet.

### Changed

- **English numbers on screen group thousands with a comma** (#37): the scale and the status
  bar's counts read `1:2,983,532`, where they used to read `1:2.983.532` in English too. Files,
  commands and exported values are unchanged.
- **Form choices have names**: Buffer's units, Calculate Geometry's properties, Select By
  Location's relations and the weights' standardization show `Area (ha)` or `Within a distance`
  rather than the parameter value (`areaHectares`, `withinDistance`) the command takes.
- **Ribbon buttons widen for their label**, so no word breaks across lines.

## [0.2.0] - 2026-10-22

### Changed results

- **Projections no longer move points they can't hold** (#54). Project to Web Mercator
  (3857) of a point past ±85.05° latitude is now an error; it used to be moved to 85.05°, up to
  550 km away, with no warning. The map still draws such data at its edge. A UTM coordinate past
  the pole or too far from its zone (a corrupt easting) is an error where it gave NaN.
- **Longitudes from UTM and Web Mercator are in (−180°, 180°]** (#54): Project and GeoJSON
  export of a point at 179°E in UTM zone 1 give 179°, not −181°.
- **Buffer refuses distances over 1 000 km** (#54). A buffer is a circle drawn in a UTM zone,
  1.2 % too small at 1 000 km and 11 % at 3 000 km; it used to accept up to 20 000 km. Geodesic
  buffers come with #8.
- **A shapefile record whose shape can't be read keeps its row** (#57). It used to be skipped,
  attributes and all, so counts, sums and every statistic on the layer's attributes left it
  out. It now reads with a null shape and its attributes, and the warning counts such records.
  A GeoJSON feature with an unreadable geometry, which used to fail the whole file, reads the
  same way.
- **Shapefile integer columns 19 or 20 characters wide read as Integer** (#55). They used to
  read as Double, so values past 2^53 (9,007,199,254,740,992) were rounded. Integer columns six
  characters wide or narrower read as Short when every value fits 16 bits.
- **A `.prj` MK aeGIS doesn't recognise no longer means WGS 84** (#6). The layer's coordinate
  system is now unknown: it is drawn as it is, not reprojected, and marked `CRS?` in Contents.
  Measurements, Buffer and Project refuse it until Define Projection sets it. Before, projected
  coordinates were read as degrees, so the features landed far away and every measurement on
  them was wrong. The `.prj` is kept, and exporting the layer to a shapefile writes it back.
  Projected data without a `.prj` now opens the same way, where it used to be refused.
- **UTM on a datum other than WGS 84 or SIRGAS 2000 is no longer read as WGS 84 UTM** (#53).
  A `.prj` named `SAD_1969_UTM_Zone_23S`, `ED_1950_UTM_Zone_31N` or `Corrego_Alegre_UTM_Zone_23S`
  used to load silently as WGS 84 UTM, tens of metres off, and reprojection carried the error
  on. Its coordinate system is now unknown (see #6). So is a `.prj` whose EPSG authority MK aeGIS
  doesn't know, such as 29193, whatever its names say.
- **Buffer of a multipoint unions its circles** (#52). Where they overlap, the circles of one
  feature used to be rings of one polygon that cut holes in each other: two points 500 m apart
  with a 1 km buffer gave an area of about 0. Now they are one shape, of 4.1 km² in that example.

### Added

- **The web app** (`apps/mkaegis-web`, #38): MK aeGIS in the browser, built with
  `trunk build --release`. Nothing is uploaded: the session keeps its files in memory. Add Data
  and Open use the browser's file picker (a shapefile is picked with its `.dbf` and `.shx`),
  files dropped on the page open by extension, and Save, Export Map, Export Features and Export
  Weights download what they write. `?sample` opens the sample map; `?webgl` skips WebGPU.
  Leaving the page with unsaved changes asks first. Each release attaches it as
  `mkaegis-web-<ver>.zip` and deploys it to <https://morrisonkuhlsen.github.io/mkaegis/>.
- **Tables** (#27, in part):
  - **Join Field** (`management.joinField`): copies a CSV table's columns, or another layer's,
    onto a layer by a common key. It reports the features matched and unmatched, the table rows
    left unused and the keys the table repeats, with examples of each.
  - **XY Table To Point** (`management.xyTableToPoint`): a layer from a CSV table's X/Y
    columns, or from a column of WKT shapes. Rows without usable coordinates are skipped and
    counted.
  - Reading guesses the delimiter (`,` `;` tab `|`), a decimal comma and the encoding (UTF-8,
    else Windows-1252), and infers column types, which `types` overrides. A code with leading
    zeros (`0012`, IBGE and FIPS codes) is read as text and keeps its zeros.
  - Export Features writes `.csv` too: the attribute table.
- **Charts linked to the map** (#25, in part): a Charts pane (Statistics ▸ Charts) with a
  histogram, a box plot, a scatter plot and the Moran scatter plot.
  - Bins: Sturges, Freedman–Diaconis, or a count.
  - Box plot: fences at 1.5 or 3 × IQR, with the outliers.
  - Scatter plot: least-squares line, r and R².
  - Moran scatter plot: z against Wz, with its slope.

  Selected features are highlighted in the chart. Dragging across a chart, or clicking a bar or
  a point, selects features on the map, through `feature.select` (Shift adds). The same
  statistics are commands: `stats.histogram`, `stats.boxPlot`, `stats.scatterPlot` and
  `stats.moranScatter`.
- **Statistical classification** (#26, in part) in Symbology and `layer.symbology`:
  - **Natural breaks**: Fisher's exact optimal classes, which Jenks's method approximates.
    Above 4,000 values it classifies a quantile sample, and says so.
  - **Standard deviation map**, **box map** and **percentile map**: six named classes on a
    diverging ramp. The box map's hinges are 1.5 × IQR, or `hinge` (3 for extreme outliers).
  - **Geometric interval** and **manual** `breaks`.
  - Quartiles and percentiles use linear interpolation (Hyndman and Fan's definition 7).
  - Every classification reports its goodness of variance fit (GVF), and Contents shows how many
    features each class holds.
- **Local statistics** (#24), written as fields on the layer and mapped at once:
  - Cluster and Outlier Analysis (`stats.localMoran`, Statistics ▸ Cluster Map (LISA)): local
    Moran's Iᵢ (Anselin 1995), its z-score under randomisation, and a pseudo p-value from
    conditional permutations (999 from seed 12345 by default). Each feature is labelled
    High-High, Low-Low, High-Low, Low-High or Not significant. Fields: `LISA_I`, `LISA_Z`,
    `LISA_P` and `LISA_CL`.
  - Hot Spot Analysis (`stats.hotSpots`, Statistics ▸ Hot Spots (Gi*)): Getis-Ord Gi* z-scores
    and p-values, binned as hot and cold spots at 90, 95 and 99 % confidence. Fields: `GI_Z`,
    `GI_P`, `GI_BIN` (−3 to 3) and `GI_CL`.
  - Both take a multiple-testing correction, none by default: Bonferroni or the false
    discovery rate (Benjamini–Hochberg). The result says how many features were tested and the
    p-value threshold applied.
- **Global spatial autocorrelation** (#23): Global Moran's I (`stats.globalMoran`), Geary's C
  (`stats.gearyC`) and Getis-Ord General G (`stats.generalG`) of a field, for a spatial weights
  matrix. Each reports the statistic and its expectation; z-scores and two-sided p-values under
  normality and under randomisation (General G: randomisation only); and a permutation test,
  999 permutations from seed 12345 by default, with its one-sided pseudo p-value. A stated seed
  gives the same pseudo p-value every time. In the app: Statistics ▸ Global Moran's I opens the
  Spatial Autocorrelation tool.
- `mkaegis-cli run --out-layer NAME` picks the layer `--out` writes (#3).
- **Define Projection** (`management.defineProjection {layer, epsg}`, #6) says which coordinate
  system a layer's coordinates are in, without moving them. `layer.info` reports whether the
  coordinate system is known, and the `.prj` text of an unknown one.
- **The File backstage**, as in MK Sigma. File opens a page over the window with these places:
  - Home: a new map, the sample map, opening a project, adding data, and the recent projects.
  - New and Open.
  - Info: the project's layers, features, coordinate system, spatial weights, and whether it is
    saved.
  - Save, Save As and Export (the map image, a layer's features).
  - Close, About and Exit.

  Esc or Back returns to the map. Recent projects are kept between runs. The control channel's
  `ui.set` takes `backstage`.

### Changed

- **CI runs the quality gate on Linux, Windows and macOS** (#19), not on Linux only. A
  `.gitattributes` keeps text files LF in every checkout, so `parity --check` no longer fails on
  a Windows clone whose files Git turned into CRLF.
- The right pane folds with a chevron (›), mirroring the one (‹) that opens it, instead of ×.

### Fixed

- **The MCP server keeps going whatever a client sends** (#60). Bytes that aren't UTF-8 made it
  exit; they now get a parse error (-32700), as a line over 16 MB does, which is no longer held
  whole. A message that isn't a JSON object, or a request without a `method`, gets -32600
  instead of nothing; a client's responses get no reply, as JSON-RPC requires (they got
  -32601); an unknown tool is a -32602 error instead of a tool result.
- **Command lines** (#63): `mkaegis a.shp --sample` drops the file no more (the sample goes
  under it, either way round); `--control abc`, a `--control` without a port and an unknown
  option are refused with the usage and exit code 2, as are `mkaegis-cli`'s usage errors (a
  failed run stays 1). A control channel that can't open (a port in use) says so in the status
  bar; the app logs warnings and errors to stderr. `MAP.AEGIS` opens as a project, and
  `--out x.AEGIS` saves one.
- **Calculate Geometry Attributes refuses a property the layer's shapes don't have** (#5): an
  area of points or lines, a length of points. It used to report "rows calculated" and add a
  column of nulls; it now says what the property needs, and writes nothing. The form offers only
  the properties the chosen layer has, and point layers are offered for their centroids.
- **One bad feature no longer costs the whole file** (#57). In GeoJSON, a geometry that can't
  be read (a polygon whose exterior has fewer than 3 vertices, a position that isn't two numbers)
  failed the whole FeatureCollection; empty `coordinates`, which RFC 7946 allows, did too. In a
  shapefile, a record of another shape type than the file's aborted the read. Each now reads with
  a null geometry and its attributes, empty geometries read as null, a polygon hole or a
  shapefile ring with fewer than 3 vertices is left out on its own, and the warnings count all
  of it. A `.dbf` with fewer rows than the `.shp` has shapes now says so. A GeoJSON file over
  1 GB is refused by its size before it is read, not after it has been held in memory.
- `mkaegis-cli run --out` wrote the top layer, not the one the commands made: `analysis.buffer`
  on points with `--out buffers.geojson` wrote the points (#3). It now writes the layer the last
  command made or added, and says which.
- **Memory:** every map-changing command used to copy the whole map for Undo, and 50 steps had
  no byte limit: toggling a 1 GB layer's visibility 50 times held about 50 GB. Undo steps now
  share every layer they didn't change, and the steps kept are capped at 1 GB of what they
  changed (#59).
- A failure inside a command that changes the selection or the view (not only edits) now
  restores the map too (#59).
- Undoing back to the saved state no longer marks the project unsaved (#59).
- **Opening a project checks it** (#58):
  - A project saved by a newer MK aeGIS is refused with a clear message. Before, it opened with
    parts missing, and saving wrote it back without them.
  - A damaged project is refused: two layers with one id, a row whose values don't match its
    fields, a shape of the wrong type or with non-finite coordinates.
  - What is safe to repair is repaired with a warning: the next layer id is moved past the ids
    in use (before, Add Data could give two layers one id), selected rows past the end are
    cleared, and spatial weights naming missing rows or layers are dropped.
- **Lost data:** Export Features to a shapefile changed values (#55):
  - Integers went through a double and were cut to 18 digits, so a 19-digit id came out wrong.
    They are now written exactly, in a column up to 20 characters wide.
  - Very large or very small doubles lost their exponent: `-1.2e23` came out as `-1.2`, and
    `1e-15` as 0. They are now written in exponent notation when it is closer.
  - Dates read from a `D` column were written back as text; they are written as dates again.
  - Short fields read back as Integer.
  - Text past 254 bytes, the dBASE limit, is still cut, but Export Features now says which
    field and how many values.

## [0.1.2] - 2026-10-21

### Changed results

- **WHERE clauses use SQL's three-valued logic** (#47). A comparison with NULL is unknown and
  no longer false. Clauses select only the rows where they are true: `NOT POP > 1000000` no
  longer selects rows where POP is NULL, and `X NOT IN ('a', NULL)` selects nothing. Calculate
  Field writes NULL where its expression is unknown, where it used to write `false`.
- **Adaptive kernel weights include the k-th neighbour** (#48). The bandwidth is now the k-th
  nearest neighbour's distance × (1 + 10⁻¹²), so each row has its k nearest neighbours
  (ties aside). Before, triangular and bisquare kernels gave the k-th neighbour weight 0: k = 1
  made every row an island, and k = 8 gave 7 neighbours.
- **Web Mercator layers are measured in ground metres** (#50). Calculate Geometry Attributes
  and Buffer on EPSG:3857 data now work in the local UTM zone, as geographic data already did.
  Before, areas came out 19 % too large at 23.5°S and 4× at 60°N, and a 10 km buffer was half
  its size at 60°N.
- **Select By Location** (#51):
  - `within` now counts the boundary as inside, so a municipality on its state's border is
    within the state. It now rejects a shape that covers one of the polygon's holes.
  - `withinDistance` measures the true distance between lines and polygons, where it used to
    require them to touch. On geographic data the distance is in ground metres in every
    direction; before, it reached 8.6 % too far north–south at 23°S and 2× at 60°.
  - `haveTheirCenterIn` is its own relation: the feature's centre must be in a selecting shape.
    It used to behave like `within`.
- **Polygon rings that touch keep their roles** (#52). A part touching another part at a vertex
  no longer turns into a hole, and a hole touching its shell no longer turns into a part. This
  corrects areas, centroids, and GeoJSON, shapefile and WKT output.

### Changed

- **Summary Statistics refuses a Text field** with a clear error (#4). It used to report every
  value as null and a sum of -0.0. A sum over no values is now 0.
- **Calculate Field refuses an expression that is NULL on every row** (`POP / 0`) and writes
  nothing (#7). Use the expression `NULL` to empty a field on purpose. Otherwise its result
  counts the rows set to NULL.
- **Opening or starting a project clears Undo** (#41), as desktop GIS does.
- **Removing a layer removes its spatial weights** (#49). `stats.weightsInfo` reports `problem`
  when weights no longer fit their layer.
- **GAL/GWT export and import refuse ambiguous ids** (#49): ids that repeat once spaces become
  `_`, a short file, and a feature or pair listed twice. Numeric ids are written in full; they
  used to be rounded to 6 decimals.
- The splash screen is a fifth larger (816 × 480), without the coral stripe along the top.

### Fixed

- **Lost work:** Undo after opening a project, followed by Save, wrote the previous map over
  the opened file (#41).
- **Lost work:** Save truncated the project in place, so a crash or a full disk mid-write left
  a broken file. Every file is now written to a temporary file and then renamed over the old
  one (#42).
- **Crash:** long AND/OR/`+` chains in a WHERE clause overflowed the stack and aborted the
  program (#43).
- **Lost work:** New from the command search (Alt+Q) didn't ask before discarding changes (#45).
- Ctrl+Shift+Z undid instead of redoing, and map shortcuts fired while typing in a text box
  (#44).
- Queen and rook contiguity failed with "coordinate out of range" on coordinates of 9,000,000
  or more, such as UTM northings in northern Brazil (#46).
- Undoing a change of the map's coordinate system left the view blank (#41).

### Added

- `mkaegis --version` and `mkaegis-cli --version`.
- The `haveTheirCenterIn` relation in Select By Location (#51).
- Project documentation (#62): `ROADMAP.md`, `CONTRIBUTING.md`, `SECURITY.md` (private
  vulnerability reports), `NOTICE`, `docs/gaps.md`, `docs/architecture.md`,
  `docs/control-protocol.md`, `docs/contributors.md`, the dependency policy in
  `docs/adr/0001-libraries.md`, and issue templates.
- For maintainers: `cargo xtask version [set X.Y.Z]`, `cargo xtask stats`, and
  `cargo xtask parity --check` in `cargo xtask ci`.

## [0.1.1] - 2026-10-21

### Fixed

- Windows: the app crashed at start on some Intel graphics drivers. It now starts on
  DirectX 12, then OpenGL, and never Vulkan by default (#82).
- Windows: the desktop app opened a console window beside its own (#78).

### Changed

- The dark theme uses MK Sigma's dark palette.
- Crates, binaries and release archives are named `mkaegis`.

## [0.1.0] - 2026-10-21

First release. It includes:

- Map handling: shapefiles and GeoJSON in and out, on-the-fly projection (WGS 84, SIRGAS 2000,
  Web Mercator, UTM), symbology, Select By Attributes and By Location, attribute editing, point
  buffers, projects, and Export Map.
- Spatial weights (#22): contiguity, k nearest neighbours, distance bands and kernels, GAL/GWT,
  and the Statistics tab.
- Every action is a command, reachable from the ribbon, the CLI, MCP and the control channel.
- Builds for Linux, Windows and macOS.

[Unreleased]: https://github.com/morrisonkuhlsen/mkaegis/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/morrisonkuhlsen/mkaegis/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/morrisonkuhlsen/mkaegis/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/morrisonkuhlsen/mkaegis/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/morrisonkuhlsen/mkaegis/releases/tag/v0.1.0
