# ADR 0002: A spatial statistics workbench, with Brazil as its vertical

*Status: accepted. 2026-10-22, at 0.4.0.*

## Context

MK aeGIS shares its foundations with general-purpose GIS: a Rust desktop and web app, a ribbon
modelled on ArcGIS Pro, a command registry reached by the CLI and MCP. RustGIS
(`opengeos/RustGIS`, read through its public documentation only) is a general-purpose GIS on the
same foundations and is far broader: layouts and PDF, rasters, a 3D globe and terrain, vector
tiles and MapLibre styles, plugins, a Python package, an assistant, ModelBuilder, a thousand
tools, sixteen languages. Competing on breadth with it, with QGIS or with ArcGIS Pro, is a race
MK aeGIS can't win and doesn't need to run.

The open issues pulled three ways at once: spatial statistics (#25–#36), land surveying for
SIGEF/INCRA (#66–#69) and precision agriculture (#72–#76). Several releases went to general GIS
features (the web app, KML, basemaps, the interface language): needed groundwork, but the ground
where general GIS are strongest.

## Decision

MK aeGIS is a **spatial statistics workbench**: the statistician's tool for exploratory spatial
data analysis and spatial models, implemented from the literature, with every result reporting
what is needed to judge it (inference method, permutations, p-values, multiple-testing
correction) and reproducible. With MK Sigma (SPSS-class) it forms one statistics-and-space
toolset. Its vertical is **Brazil**: Portuguese first, SIRGAS 2000, and land surveying as SIGEF
and INCRA require it.

1. **Statistics first.** Work goes to statistics before map handling: descriptive statistics
   (#28), rates and their smoothing (#29), spatial regression (#30), reproducible analyses (#32),
   then classification, linked views, tables and point patterns (#26, #25, #27, #31), then
   geostatistics and GWR (#34, #33).
2. **The Brazilian vertical second:** the local geodetic system and areas as INCRA computes them
   (#66), the SIGEF spreadsheet (#69), field computations (#67) and the descriptive memorial
   (#68).
3. **General GIS is kept "enough", not chased.** What a statistical analysis needs from map
   handling is done well (geoprocessing statistics depend on: buffers of lines and polygons #8,
   overlay #10, wider CRS #9, formats #14, rasters as inputs #39); what it doesn't (3D, terrain,
   cartographic layouts beyond a figure, plugins, routing, vector tile styling) is not planned.
   MK aeGIS talks to QGIS, RustGIS and ArcGIS through files (GeoPackage, GeoJSON, KML,
   shapefiles, GAL/GWT), not by matching them feature for feature.
4. **Precision agriculture waits** (#72–#76): it builds on rasters and imagery (#39, #71) and on
   the statistics above; it is ranked after them.

## Consequences

- `docs/gaps.md` ranks work in this order; issues outside it keep their `priority:` label but
  are picked after it.
- New feature requests are weighed against this: a general-GIS feature lands when a statistical
  analysis or the Brazilian vertical needs it.
- The ArcGIS Pro reference in `AGENTS.md` stays for how map handling looks and behaves, not as a
  list to complete.
