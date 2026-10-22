//! Feature primitives: select rows, identify, edit attributes, delete (Edit tab).

use mkaegis_core::{Error, Value};
use mkaegis_model::FieldType;
use serde_json::{Value as Json, json};

use super::{CommandSpec, Kind, has_layers, layer_key, mode, req_str, rows};

/// A JSON value as a field's value.
pub(crate) fn value_for(v: &Json, kind: FieldType, field: &str) -> Result<Value, Error> {
    let bad = || Error::param("value", format!("not a valid {} for `{field}`", kind.name()));
    Ok(match (v, kind) {
        (Json::Null, _) => Value::Null,
        (Json::String(s), _) if s.is_empty() && kind != FieldType::Text => Value::Null,
        (Json::Number(n), FieldType::Integer) => Value::Int(n.as_i64().ok_or_else(bad)?),
        (Json::String(s), FieldType::Integer) => Value::Int(s.trim().parse().map_err(|_| bad())?),
        (Json::Number(n), FieldType::Double) => Value::Float(n.as_f64().filter(|x| x.is_finite()).ok_or_else(bad)?),
        (Json::String(s), FieldType::Double) => {
            Value::Float(s.trim().replace(',', ".").parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(bad)?)
        }
        (Json::Number(_) | Json::String(_), FieldType::Short) => {
            let n = v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or_else(bad)?;
            if n.fract() != 0.0 {
                return Err(bad());
            }
            FieldType::Short.coerce(Value::Float(n)).ok_or_else(bad)?
        }
        (Json::Bool(b), FieldType::Boolean) => Value::Bool(*b),
        (Json::String(s), FieldType::Boolean) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "sim" | "yes" => Value::Bool(true),
            "false" | "0" | "não" | "nao" | "no" => Value::Bool(false),
            _ => return Err(bad()),
        },
        (Json::String(s), FieldType::Text) => Value::Text(s.chars().take(4096).collect()),
        (other, FieldType::Text) => Value::Text(other.to_string()),
        _ => return Err(bad()),
    })
}

pub const SELECT: CommandSpec = CommandSpec {
    id: "feature.select",
    label: "Select Features",
    ribbon: "Map ▸ Selection",
    shortcut: None,
    params: "{layer, rows: [0-based row numbers], mode?}",
    kind: Kind::View,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer_mut(&layer_key(p, "layer")?)?;
        let picked = rows(p, "rows", layer.data.features.len())?;
        layer.select(picked, mode(p)?);
        Ok(json!({ "selected": layer.selection.len() }))
    },
};

pub const IDENTIFY: CommandSpec = CommandSpec {
    id: "feature.identify",
    label: "Explore",
    ribbon: "Map ▸ Navigate",
    shortcut: None,
    params: "{layer, row} — a feature's attributes and geometry summary",
    kind: Kind::Query,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer(&layer_key(p, "layer")?)?;
        let row = rows(&json!({ "rows": [p.get("row").cloned().unwrap_or(Json::Null)] }), "rows", layer.data.features.len())?;
        let row = row.first().copied().ok_or_else(|| Error::param("row", "required"))?;
        let f = layer.data.features.get(row).ok_or_else(|| Error::param("row", "no such row"))?;
        let attributes: serde_json::Map<String, Json> =
            layer.data.fields.iter().zip(&f.values).map(|(fd, v)| (fd.name.clone(), serde_json::to_value(v).unwrap_or(Json::Null))).collect();
        let geometry = f.geometry.as_ref().map(|g| json!({ "type": g.geometry_type().name(), "vertices": g.vertex_count() }));
        Ok(json!({ "layer": layer.name, "row": row, "attributes": attributes, "geometry": geometry }))
    },
};

pub const SET_VALUE: CommandSpec = CommandSpec {
    id: "feature.setValue",
    label: "Edit Attribute",
    ribbon: "Edit ▸ Attributes",
    shortcut: None,
    params: "{layer, row, field, value}",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer_mut(&layer_key(p, "layer")?)?;
        let row = rows(&json!({ "rows": [p.get("row").cloned().unwrap_or(Json::Null)] }), "rows", layer.data.features.len())?;
        let row = row.first().copied().ok_or_else(|| Error::param("row", "required"))?;
        let field = req_str(p, "field")?;
        let i = layer.data.field_index(field).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
        let kind = layer.data.fields.get(i).map(|f| f.kind).ok_or_else(|| Error::NotFound(format!("field `{field}`")))?;
        let v = value_for(p.get("value").unwrap_or(&Json::Null), kind, field)?;
        let slot = layer.data_mut().features.get_mut(row).and_then(|f| f.values.get_mut(i)).ok_or_else(|| Error::param("row", "no such row"))?;
        *slot = v;
        Ok(json!({ "message": "attribute edited" }))
    },
};

pub const DELETE: CommandSpec = CommandSpec {
    id: "feature.delete",
    label: "Delete",
    ribbon: "Edit ▸ Features",
    shortcut: Some("Delete"),
    params: "{layer, rows?} — the layer's selection when rows are omitted",
    kind: Kind::Edit,
    enabled: has_layers,
    run: |s, p| {
        let layer = s.map.layer_mut(&layer_key(p, "layer")?)?;
        let doomed: std::collections::BTreeSet<usize> = match p.get("rows") {
            Some(_) => rows(p, "rows", layer.data.features.len())?.into_iter().collect(),
            None => layer.selection.clone(),
        };
        if doomed.is_empty() {
            return Err(Error::param("rows", "nothing to delete: select features first"));
        }
        let mut i = 0;
        layer.data_mut().features.retain(|_| {
            let keep = !doomed.contains(&i);
            i += 1;
            keep
        });
        layer.selection.clear();
        Ok(json!({ "message": format!("{} deleted", doomed.len()) }))
    },
};
