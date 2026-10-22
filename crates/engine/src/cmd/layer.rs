//! Layer commands: the Contents pane's context menu, the Feature Layer tabs (Appearance,
//! Labeling) and the attribute table's Add Field / Calculate Field.

use mkaegis_core::{Color, Error};
use mkaegis_model::query::Query;
use mkaegis_model::symbology::{Classification, Ramp};
use mkaegis_model::{Field, FieldType, Labels, Marker, Renderer, Symbol};
use serde_json::{Value as Json, json};

use super::{CommandSpec, Kind, has_layers, layer_key, opt_bool, opt_f64, opt_str, path, req_str};

fn color(p: &Json, key: &str) -> Result<Option<Color>, Error> {
    match opt_str(p, key) {
        None => Ok(None),
        Some(text) => Color::from_hex(text).map(Some).ok_or_else(|| Error::param(key, "a colour as #rrggbb or #rrggbbaa")),
    }
}

/// A size in points: positive and sane.
fn size(p: &Json, key: &str) -> Result<Option<f32>, Error> {
    match opt_f64(p, key)? {
        None => Ok(None),
        Some(x) if (0.0..=100.0).contains(&x) => Ok(Some(x as f32)),
        Some(_) => Err(Error::param(key, "between 0 and 100 points")),
    }
}

pub const REMOVE: CommandSpec = CommandSpec {
    id: "layer.remove",
    label: "Remove",
    ribbon: "Contents ▸ Layer",
    shortcut: None,
    params: "{layer}",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let i = s.map.find(&layer_key(p, "layer")?)?;
        let layer = s.map.layers.remove(i);
        // Its spatial weights go with it: they name features that no longer exist (#49).
        let before = s.map.weights.len();
        s.map.weights.retain(|w| w.layer != layer.id);
        let gone = before - s.map.weights.len();
        let weights = if gone > 0 { format!(" and its {gone} spatial weights") } else { String::new() };
        Ok(json!({ "message": format!("removed {}{weights}", layer.name) }))
    },
};

pub const RENAME: CommandSpec = CommandSpec {
    id: "layer.rename",
    label: "Rename",
    ribbon: "Contents ▸ Layer",
    shortcut: Some("F2"),
    params: "{layer, name}",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let key = layer_key(p, "layer")?;
        let name: String = req_str(p, "name")?.chars().take(200).collect();
        s.map.layer_mut(&key)?.name = name;
        Ok(json!({ "message": "renamed" }))
    },
};

pub const SET_VISIBLE: CommandSpec = CommandSpec {
    id: "layer.setVisible",
    label: "Visibility",
    ribbon: "Contents ▸ Layer",
    shortcut: None,
    params: "{layer, visible?} — toggles when visible is omitted",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let visible = opt_bool(p, "visible")?;
        let layer = s.map.layer_mut(&layer_key(p, "layer")?)?;
        layer.visible = visible.unwrap_or(!layer.visible);
        Ok(json!({ "visible": layer.visible }))
    },
};

pub const MOVE: CommandSpec = CommandSpec {
    id: "layer.move",
    label: "Move Layer",
    ribbon: "Contents ▸ Layer",
    shortcut: None,
    params: "{layer, to} — to: up, down, top, bottom, or a position (0 = top)",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let i = s.map.find(&layer_key(p, "layer")?)?;
        let last = s.map.layers.len().saturating_sub(1);
        let to = match p.get("to") {
            Some(Json::String(t)) => match t.as_str() {
                "up" => i.saturating_sub(1),
                "down" => (i + 1).min(last),
                "top" => 0,
                "bottom" => last,
                _ => return Err(Error::param("to", "up, down, top, bottom or a position")),
            },
            Some(Json::Number(n)) => n.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| Error::param("to", "a position"))?.min(last),
            _ => return Err(Error::param("to", "required")),
        };
        let layer = s.map.layers.remove(i);
        s.map.layers.insert(to.min(s.map.layers.len()), layer);
        Ok(json!({ "position": to }))
    },
};

pub const SET_OPACITY: CommandSpec = CommandSpec {
    id: "layer.setOpacity",
    label: "Transparency",
    ribbon: "Feature Layer ▸ Appearance",
    shortcut: None,
    params: "{layer, opacity} — 0 to 100",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let o = super::req_f64(p, "opacity")?;
        if !(0.0..=100.0).contains(&o) {
            return Err(Error::param("opacity", "0 to 100"));
        }
        s.map.layer_mut(&layer_key(p, "layer")?)?.opacity = o.round() as u8;
        Ok(json!({ "message": "opacity set" }))
    },
};

pub const SYMBOLOGY: CommandSpec = CommandSpec {
    id: "layer.symbology",
    label: "Symbology",
    ribbon: "Feature Layer ▸ Appearance",
    shortcut: None,
    params: "{layer, renderer?: single|uniqueValues|graduatedColors, field?, classes?, method?: equalInterval|quantile, ramp?: meadow|ember|depth|balance, fill?, outline?, width?, size?, marker?: circle|square|triangle|diamond}",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer_mut(&layer_key(p, "layer")?)?;
        let mut base: Symbol = layer.renderer.base();
        if let Some(c) = color(p, "fill")? {
            base.fill = c;
        }
        if let Some(c) = color(p, "outline")? {
            base.outline = c;
        }
        if let Some(w) = size(p, "width")? {
            base.width = w;
        }
        if let Some(sz) = size(p, "size")? {
            base.size = sz;
        }
        if let Some(m) = opt_str(p, "marker") {
            base.marker = Marker::parse(m).ok_or_else(|| Error::param("marker", "circle, square, triangle or diamond"))?;
        }
        let kind = opt_str(p, "renderer").unwrap_or(match &layer.renderer {
            Renderer::Single(_) => "single",
            Renderer::UniqueValues { .. } => "uniqueValues",
            Renderer::GraduatedColors { .. } => "graduatedColors",
        });
        let field = opt_str(p, "field").map(str::to_string).or_else(|| layer.renderer.field().map(str::to_string));
        layer.renderer = match kind.to_ascii_lowercase().as_str() {
            "single" | "singlesymbol" => Renderer::Single(base),
            "uniquevalues" => Renderer::unique_values(&layer.data, field.as_deref().ok_or_else(|| Error::param("field", "required"))?, base)?,
            "graduatedcolors" => {
                let classes = opt_f64(p, "classes")?.unwrap_or(5.0);
                if !(1.0..=32.0).contains(&classes) {
                    return Err(Error::param("classes", "1 to 32"));
                }
                let method =
                    Classification::parse(opt_str(p, "method").unwrap_or("")).ok_or_else(|| Error::param("method", "equalInterval or quantile"))?;
                let ramp =
                    Ramp::parse(opt_str(p, "ramp").unwrap_or("meadow")).ok_or_else(|| Error::param("ramp", "meadow, ember, depth or balance"))?;
                Renderer::graduated(
                    &layer.data,
                    field.as_deref().ok_or_else(|| Error::param("field", "required"))?,
                    classes as usize,
                    method,
                    ramp,
                    base,
                )?
            }
            other => return Err(Error::param("renderer", format!("unknown renderer `{other}`"))),
        };
        Ok(json!({ "message": "symbology changed", "classes": layer.renderer.legend().len() }))
    },
};

pub const LABELS: CommandSpec = CommandSpec {
    id: "layer.labels",
    label: "Label",
    ribbon: "Feature Layer ▸ Labeling",
    shortcut: None,
    params: "{layer, field?, color?, size?} — without a field, labels are turned off",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer_mut(&layer_key(p, "layer")?)?;
        let Some(field) = opt_str(p, "field") else {
            layer.labels = None;
            return Ok(json!({ "message": "labels off" }));
        };
        let i = layer.data.field_index(field).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
        let name = layer.data.fields.get(i).map(|f| f.name.clone()).unwrap_or_default();
        let old = layer.labels.clone();
        layer.labels = Some(Labels {
            field: name,
            color: color(p, "color")?.or(old.as_ref().map(|l| l.color)).unwrap_or(Color::BLACK),
            size: size(p, "size")?.or(old.map(|l| l.size)).unwrap_or(9.0).max(4.0),
        });
        Ok(json!({ "message": "labels on" }))
    },
};

pub const ADD_FIELD: CommandSpec = CommandSpec {
    id: "layer.addField",
    label: "Add Field",
    ribbon: "Table ▸ Field",
    shortcut: None,
    params: "{layer, name, type?: long|short|double|text|boolean}",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer_mut(&layer_key(p, "layer")?)?;
        let kind = match opt_str(p, "type").unwrap_or("double").to_ascii_lowercase().as_str() {
            "long" | "integer" | "int" => FieldType::Integer,
            "double" | "float" => FieldType::Double,
            "text" | "string" => FieldType::Text,
            "short" => FieldType::Short,
            "boolean" | "bool" => FieldType::Boolean,
            other => return Err(Error::param("type", format!("unknown field type `{other}`"))),
        };
        layer.data_mut().add_field(Field::new(req_str(p, "name")?, kind))?;
        Ok(json!({ "message": "field added" }))
    },
};

pub const CALCULATE_FIELD: CommandSpec = CommandSpec {
    id: "layer.calculateField",
    label: "Calculate Field",
    ribbon: "Table ▸ Field",
    shortcut: None,
    params: "{layer, field, expression, selectedOnly?} — SQL expression over fields (`POP / 1000`, `UPPER(NOME)`)",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let only_selected = opt_bool(p, "selectedOnly")?.unwrap_or(false);
        let layer = s.map.layer_mut(&layer_key(p, "layer")?)?;
        let field = req_str(p, "field")?;
        let i = layer.data.field_index(field).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
        let kind = layer.data.fields.get(i).map(|f| f.kind).unwrap_or(FieldType::Text);
        let expression = req_str(p, "expression")?;
        let q = Query::parse(expression, &layer.data)?;
        let rows: Vec<usize> = if only_selected { layer.selection.iter().copied().collect() } else { (0..layer.data.features.len()).collect() };
        let mut values = Vec::with_capacity(rows.len());
        let mut nulls = 0usize;
        for r in &rows {
            let v = q.evaluate(&layer.data, *r);
            nulls += usize::from(v.is_null());
            let shown = v.to_string();
            let fits = kind
                .coerce(v)
                .ok_or_else(|| Error::param("expression", format!("row {}: `{shown}` doesn't fit the {} field `{field}`", r + 1, kind.name())))?;
            values.push((*r, fits));
        }
        // NULL on every row is almost always a mistake (`POP / 0`, an empty field) that would wipe
        // the column: refused, unless NULL is what was asked for (#7).
        if !rows.is_empty() && nulls == rows.len() && !expression.trim().eq_ignore_ascii_case("NULL") {
            return Err(Error::param(
                "expression",
                format!(
                    "`{expression}` is NULL on every row (a division by zero, or a NULL operand), so nothing was written; to empty `{field}`, use the expression NULL"
                ),
            ));
        }
        for (r, v) in values {
            if let Some(slot) = layer.data_mut().features.get_mut(r).and_then(|f| f.values.get_mut(i)) {
                *slot = v;
            }
        }
        let set_null = if nulls > 0 { format!(", {nulls} set to NULL") } else { String::new() };
        Ok(json!({ "message": format!("{} rows calculated{set_null}", rows.len()), "calculated": rows.len(), "nulls": nulls }))
    },
};

pub const EXPORT: CommandSpec = CommandSpec {
    id: "layer.export",
    label: "Export Features",
    ribbon: "Data ▸ Export",
    shortcut: None,
    params: "{layer, path, selectedOnly?} — .geojson or .shp",
    kind: Kind::View,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let file = path(p, "path")?;
        let mut data = (*layer.data).clone();
        if opt_bool(p, "selectedOnly")?.unwrap_or(false) {
            data.features = layer.selection.iter().filter_map(|r| layer.data.features.get(*r).cloned()).collect();
        }
        let warnings = crate::io::write_data(&data, &file)?;
        let message = format!("{} features written to {}", data.features.len(), file.display());
        let message = if warnings.is_empty() { message } else { format!("{message}; {}", warnings.join("; ")) };
        Ok(json!({ "message": message, "warnings": warnings }))
    },
};

pub const INFO: CommandSpec = CommandSpec {
    id: "layer.info",
    label: "Layer Properties",
    ribbon: "Contents ▸ Properties",
    shortcut: None,
    params: "{layer}",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let e = layer.data.extent();
        Ok(json!({
            "id": layer.id, "name": layer.name, "geometry": layer.data.geometry_type.name(),
            "crs": { "epsg": layer.data.crs.epsg(), "name": layer.data.crs.name(), "known": layer.data.crs.is_known(), "prj": layer.data.prj },
            "features": layer.data.features.len(), "selected": layer.selection.len(),
            "extent": e.map(|e| vec![e.min_x, e.min_y, e.max_x, e.max_y]),
            "fields": layer.data.fields.iter().map(|f| json!({"name": f.name, "type": f.kind.name()})).collect::<Vec<_>>(),
            "legend": layer.renderer.legend().into_iter().map(|(l, s)| json!({"label": l, "fill": s.fill.to_hex()})).collect::<Vec<_>>(),
        }))
    },
};
