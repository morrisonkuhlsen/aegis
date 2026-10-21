# Changelog

Every release of MK aeGIS, newest first, in the spirit of
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow
[semantic versioning](https://semver.org/); before 1.0, a minor version may break projects and
commands.

**Changed results** lists the changes that give different numbers or selections for the same
data and parameters. Read it before re-running an analysis with a new version.

## [Unreleased]

### Added

- **The File backstage**, as in MK Sigma. File opens a page over the window with these places:
  - Home: a new map, the sample map, opening a project, adding data, and the recent projects.
  - New and Open.
  - Info: the project's layers, features, coordinate system, spatial weights, and whether it is
    saved.
  - Save, Save As and Export (the map image, a layer's features).
  - Close, About and Exit.

  Esc or Back returns to the map. Recent projects are kept between runs. The control channel's
  `ui.set` takes `backstage`.

### Fixed

- **Memory:** every map-changing command used to copy the whole map for Undo, and 50 steps had
  no byte limit: toggling a 1 GB layer's visibility 50 times held about 50 GB. Undo steps now
  share every layer they didn't change, and the steps kept are capped at 1 GB of what they
  changed (#59).
- A failure inside a command that changes the selection or the view (not only edits) now
  restores the map too (#59).
- Undoing back to the saved state no longer marks the project unsaved (#59).


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

[Unreleased]: https://github.com/morrisonkuhlsen/mkaegis/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/morrisonkuhlsen/mkaegis/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/morrisonkuhlsen/mkaegis/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/morrisonkuhlsen/mkaegis/releases/tag/v0.1.0
