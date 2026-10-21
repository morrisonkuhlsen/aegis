//! Project tab: new, open, save; and undo/redo (Quick Access Toolbar).

use aegis_core::{Error, Result};
use serde_json::{Value as Json, json};

use super::{CommandSpec, Kind, always, opt_str, path};
use crate::Session;

pub const NEW: CommandSpec = CommandSpec {
    id: "project.new",
    label: "New Map",
    ribbon: "Project ▸ New",
    shortcut: Some("Ctrl+N"),
    params: "{} — an empty map in WGS 84 / Pseudo-Mercator",
    kind: Kind::Edit,
    enabled: always,
    run: |s, _| {
        s.map = aegis_model::Map::default();
        s.path = None;
        s.view_serial += 1;
        Ok(json!({ "message": "new map" }))
    },
};

pub const OPEN: CommandSpec = CommandSpec {
    id: "project.open",
    label: "Open Project",
    ribbon: "Project ▸ Open",
    shortcut: Some("Ctrl+O"),
    params: "{path} — a .aegis project",
    kind: Kind::Edit,
    enabled: always,
    run: |s, p| {
        let file = path(p, "path")?;
        s.map = crate::io::open_project(&file)?;
        s.path = Some(file);
        s.view_serial += 1;
        Ok(json!({ "message": "opened", "layers": s.map.layers.len() }))
    },
};

fn save_to(s: &mut Session, file: std::path::PathBuf) -> Result<Json> {
    let file = if file.extension().is_none() { file.with_extension(crate::io::PROJECT_EXTENSION) } else { file };
    crate::io::save_project(&s.map, &file)?;
    let shown = file.display().to_string();
    s.path = Some(file);
    s.modified = false;
    Ok(json!({ "message": format!("saved {shown}"), "path": shown }))
}

pub const SAVE: CommandSpec = CommandSpec {
    id: "project.save",
    label: "Save Project",
    ribbon: "Project ▸ Save",
    shortcut: Some("Ctrl+S"),
    params: "{path?} — the project's own path when omitted",
    kind: Kind::View,
    enabled: always,
    run: |s, p| {
        let file = match opt_str(p, "path") {
            Some(f) => std::path::PathBuf::from(f),
            None => s.path.clone().ok_or_else(|| Error::param("path", "the project has not been saved yet: give a path"))?,
        };
        save_to(s, file)
    },
};

pub const SAVE_AS: CommandSpec = CommandSpec {
    id: "project.saveAs",
    label: "Save Project As",
    ribbon: "Project ▸ Save As",
    shortcut: Some("Ctrl+Shift+S"),
    params: "{path}",
    kind: Kind::View,
    enabled: always,
    run: |s, p| save_to(s, path(p, "path")?),
};

pub const UNDO: CommandSpec = CommandSpec {
    id: "edit.undo",
    label: "Undo",
    ribbon: "Edit ▸ Undo",
    shortcut: Some("Ctrl+Z"),
    params: "{}",
    kind: Kind::View,
    enabled: |s| s.can_undo().is_some(),
    run: |_, _| Err(Error::Internal("undo runs in Session::execute".into())),
};

pub const REDO: CommandSpec = CommandSpec {
    id: "edit.redo",
    label: "Redo",
    ribbon: "Edit ▸ Redo",
    shortcut: Some("Ctrl+Y"),
    params: "{}",
    kind: Kind::View,
    enabled: |s| s.can_redo().is_some(),
    run: |_, _| Err(Error::Internal("redo runs in Session::execute".into())),
};
