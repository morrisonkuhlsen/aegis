//! Map tab: Add Data, Selection, Navigate; and the map's own properties.

use mkaegis_core::{Error, Result};
use mkaegis_crs::Crs;
use mkaegis_geom::{Coord, Envelope};
use mkaegis_model::query::Query;
use serde_json::{Value as Json, json};

use super::{CommandSpec, Kind, always, has_layers, has_selection, layer_key, mode, opt_f64, opt_str, path, req_f64, req_str};
use crate::Session;

fn set_extent(s: &mut Session, e: Option<Envelope>) -> Result<Json> {
    let e = e.filter(Envelope::is_finite).ok_or_else(|| Error::param("extent", "nothing to zoom to"))?;
    // A single point or a flat line still gets some room.
    let pad = (e.width().max(e.height()) * 0.05).max(if s.map.crs.is_geographic() { 0.01 } else { 1000.0 });
    let e = e.buffer(pad);
    s.map.extent = Some(e);
    s.view_serial += 1;
    Ok(json!({ "extent": [e.min_x, e.min_y, e.max_x, e.max_y] }))
}

pub const ADD_DATA: CommandSpec = CommandSpec {
    id: "map.addData",
    label: "Add Data",
    ribbon: "Map ▸ Layer",
    shortcut: None,
    params: "{path} — a .geojson, .json or .shp file; added as a layer on top",
    kind: Kind::Edit,
    enabled: always,
    run: |s, p| {
        let file = path(p, "path")?;
        let (mut data, warnings) = crate::io::read_data(&file)?;
        data.name = s.map.unique_name(&data.name);
        let count = data.features.len();
        let first = s.map.layers.is_empty();
        let id = s.map.add_layer(data);
        if first {
            let e = s.map.full_extent();
            let _ = set_extent(s, e);
        }
        Ok(json!({ "message": format!("added {count} features"), "layer": id, "warnings": warnings }))
    },
};

pub const ADD_SAMPLE: CommandSpec = CommandSpec {
    id: "map.addSample",
    label: "Add Sample Data",
    ribbon: "Map ▸ Layer",
    shortcut: None,
    params: "{} — the made-up archipelago generated in code",
    kind: Kind::Edit,
    enabled: always,
    run: |s, _| {
        crate::sample::add_to(&mut s.map)?;
        s.view_serial += 1;
        Ok(json!({ "message": "sample layers added" }))
    },
};

pub const SELECT_BY_ATTRIBUTES: CommandSpec = CommandSpec {
    id: "map.selectByAttributes",
    label: "Select By Attributes",
    ribbon: "Map ▸ Selection",
    shortcut: None,
    params: "{layer, where, mode?} — SQL clause (`POP > 1000 AND NOME LIKE 'S%'`); mode new|add|remove|intersect",
    kind: Kind::View,
    enabled: has_layers,
    run: |s, p| {
        let key = layer_key(p, "layer")?;
        let clause = req_str(p, "where")?;
        let mode = mode(p)?;
        let layer = s.map.layer_mut(&key)?;
        let rows = Query::parse(clause, &layer.data)?.select(&layer.data);
        layer.select(rows, mode);
        let n = layer.selection.len();
        Ok(json!({ "message": format!("{n} selected"), "selected": n }))
    },
};

pub const SELECT_BY_LOCATION: CommandSpec = CommandSpec {
    id: "map.selectByLocation",
    label: "Select By Location",
    ribbon: "Map ▸ Selection",
    shortcut: None,
    params: "{layer, selecting, relation?, distance?, mode?} — relation intersect|withinDistance|within|contains; distance in metres; the selecting layer's selection is used if it has one",
    kind: Kind::View,
    enabled: has_layers,
    run: |s, p| {
        let target = layer_key(p, "layer")?;
        let selecting = layer_key(p, "selecting")?;
        let relation = mkaegis_analysis::Relation::parse(opt_str(p, "relation").unwrap_or("intersect"), opt_f64(p, "distance")?)?;
        let mode = mode(p)?;
        let sel = s.map.layer(&selecting)?;
        let picked: Vec<usize> = sel.selection.iter().copied().collect();
        let rows =
            mkaegis_analysis::select_by_location(&s.map.layer(&target)?.data, &sel.data, (!picked.is_empty()).then_some(&picked[..]), relation)?;
        let layer = s.map.layer_mut(&target)?;
        layer.select(rows, mode);
        let n = layer.selection.len();
        Ok(json!({ "message": format!("{n} selected"), "selected": n }))
    },
};

pub const SELECT_BY_RECTANGLE: CommandSpec = CommandSpec {
    id: "map.selectByRectangle",
    label: "Select",
    ribbon: "Map ▸ Selection",
    shortcut: None,
    params: "{xmin, ymin, xmax, ymax, layer?, mode?} — in the map's CRS; every visible layer when layer is omitted",
    kind: Kind::View,
    enabled: has_layers,
    run: |s, p| {
        let (a, b) = (Coord::new(req_f64(p, "xmin")?, req_f64(p, "ymin")?), Coord::new(req_f64(p, "xmax")?, req_f64(p, "ymax")?));
        let mode = mode(p)?;
        let only = match p.get("layer").filter(|v| !v.is_null()) {
            Some(_) => Some(s.map.find(&layer_key(p, "layer")?)?),
            None => None,
        };
        let e = Envelope::new(a, b);
        let ring = vec![Coord::new(e.min_x, e.min_y), Coord::new(e.max_x, e.min_y), Coord::new(e.max_x, e.max_y), Coord::new(e.min_x, e.max_y)];
        let rect = mkaegis_geom::Geometry::polygon(vec![ring])?;
        let crs = s.map.crs;
        let mut total = 0;
        for (i, layer) in s.map.layers.iter_mut().enumerate() {
            if only.is_some_and(|o| o != i) || (only.is_none() && !layer.visible) {
                continue;
            }
            // The rectangle in the layer's CRS (densified edges keep it honest across projections).
            let Some(le) = mkaegis_model::project_envelope(e, crs, layer.data.crs) else { continue };
            let local = if crs == layer.data.crs { rect.clone() } else { rect.try_map(|c| crs.transform(layer.data.crs, c)).unwrap_or(rect.clone()) };
            let rows: Vec<usize> = layer
                .data
                .features
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    f.geometry
                        .as_ref()
                        .is_some_and(|g| g.envelope().is_some_and(|ge| ge.intersects(&le)) && mkaegis_geom::algo::intersects(g, &local, 0.0))
                })
                .map(|(r, _)| r)
                .collect();
            layer.select(rows, mode);
            total += layer.selection.len();
        }
        Ok(json!({ "message": format!("{total} selected"), "selected": total }))
    },
};

pub const CLEAR_SELECTION: CommandSpec = CommandSpec {
    id: "map.clearSelection",
    label: "Clear",
    ribbon: "Map ▸ Selection",
    shortcut: Some("Ctrl+Shift+A"),
    params: "{layer?} — every layer when omitted",
    kind: Kind::View,
    enabled: has_selection,
    run: |s, p| {
        match p.get("layer").filter(|v| !v.is_null()) {
            Some(_) => s.map.layer_mut(&layer_key(p, "layer")?)?.selection.clear(),
            None => s.map.layers.iter_mut().for_each(|l| l.selection.clear()),
        }
        Ok(json!({ "message": "selection cleared" }))
    },
};

pub const ZOOM_TO_LAYER: CommandSpec = CommandSpec {
    id: "map.zoomToLayer",
    label: "Zoom To Layer",
    ribbon: "Map ▸ Navigate",
    shortcut: None,
    params: "{layer}",
    kind: Kind::View,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let e = s.map.layer_extent(layer);
        set_extent(s, e)
    },
};

pub const ZOOM_TO_SELECTION: CommandSpec = CommandSpec {
    id: "map.zoomToSelection",
    label: "Zoom To Selection",
    ribbon: "Map ▸ Selection",
    shortcut: None,
    params: "{}",
    kind: Kind::View,
    enabled: has_selection,
    run: |s, _| {
        let map = &s.map;
        let e = map
            .layers
            .iter()
            .flat_map(|l| {
                l.selection.iter().filter_map(move |r| {
                    let g = l.data.features.get(*r)?.geometry.as_ref()?;
                    mkaegis_model::project_envelope(g.envelope()?, l.data.crs, map.crs)
                })
            })
            .reduce(Envelope::union);
        set_extent(s, e)
    },
};

pub const FULL_EXTENT: CommandSpec = CommandSpec {
    id: "map.fullExtent",
    label: "Full Extent",
    ribbon: "Map ▸ Navigate",
    shortcut: None,
    params: "{}",
    kind: Kind::View,
    enabled: has_layers,
    run: |s, _| {
        let e = s.map.full_extent();
        set_extent(s, e)
    },
};

pub const SET_EXTENT: CommandSpec = CommandSpec {
    id: "map.setExtent",
    label: "Set Extent",
    ribbon: "Map ▸ Navigate",
    shortcut: None,
    params: "{xmin, ymin, xmax, ymax} — in the map's CRS",
    kind: Kind::Query,
    enabled: always,
    run: |s, p| {
        let (a, b) = (Coord::new(req_f64(p, "xmin")?, req_f64(p, "ymin")?), Coord::new(req_f64(p, "xmax")?, req_f64(p, "ymax")?));
        if a.x == b.x || a.y == b.y {
            return Err(Error::param("extent", "the extent has no area"));
        }
        let e = Envelope::new(a, b);
        s.map.extent = Some(e);
        Ok(json!({ "extent": [e.min_x, e.min_y, e.max_x, e.max_y] }))
    },
};

pub const SET_CRS: CommandSpec = CommandSpec {
    id: "map.setCrs",
    label: "Coordinate System",
    ribbon: "Map ▸ Properties",
    shortcut: None,
    params: "{epsg} — the map's coordinate system; layers are projected on the fly",
    kind: Kind::Edit,
    enabled: always,
    run: |s, p| {
        let code = req_f64(p, "epsg")?;
        let crs = Crs::from_epsg(u32::try_from(code as i64).map_err(|_| Error::param("epsg", "not an EPSG code"))?)?;
        let old = s.map.crs;
        let extent = s.map.extent.and_then(|e| mkaegis_model::project_envelope(e, old, crs));
        s.map.crs = crs;
        s.map.extent = extent.or_else(|| s.map.full_extent());
        s.view_serial += 1;
        Ok(json!({ "message": format!("map in {}", crs.name()) }))
    },
};

pub const DESCRIBE: CommandSpec = CommandSpec {
    id: "map.describe",
    label: "Describe Map",
    ribbon: "Map ▸ Properties",
    shortcut: None,
    params: "{} — the map, its CRS and its layers",
    kind: Kind::Query,
    enabled: always,
    run: |s, _| {
        let layers: Vec<Json> = s
            .map
            .layers
            .iter()
            .map(|l| {
                json!({
                    "id": l.id, "name": l.name, "visible": l.visible,
                    "geometry": l.data.geometry_type.name(), "features": l.data.features.len(),
                    "selected": l.selection.len(), "crs": l.data.crs.epsg(),
                    "fields": l.data.fields.iter().map(|f| json!({"name": f.name, "type": f.kind.name()})).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(json!({
            "name": s.map.name, "crs": { "epsg": s.map.crs.epsg(), "name": s.map.crs.name() },
            "extent": s.map.extent.map(|e| vec![e.min_x, e.min_y, e.max_x, e.max_y]),
            "layers": layers, "modified": s.modified,
        }))
    },
};
