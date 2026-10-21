//! The engine (L4): a [`Session`] holds the map, undo/redo and the geoprocessing history, and
//! runs commands. Everything a person can do is a command (`cmd`): the UI, the control channel,
//! the CLI and the MCP server all go through [`Session::execute`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod cmd;
pub mod io;
pub mod sample;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

pub use mkaegis_core::{Error, Result};
pub use mkaegis_model::Map;
use serde_json::Value as Json;

pub use cmd::{COMMANDS, CommandSpec, Kind};

/// Undo steps kept.
const UNDO_DEPTH: usize = 50;
/// History entries kept.
const HISTORY_DEPTH: usize = 500;

/// One finished command, for the geoprocessing history.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub command: String,
    pub ok: bool,
    pub message: String,
}

/// An open project: the map and everything around it.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub map: Map,
    pub path: Option<PathBuf>,
    pub modified: bool,
    /// Bumped whenever a command asks the view to move (zoom to layer, open…): the UI refits.
    pub view_serial: u64,
    /// Bumped whenever the map may have changed: views redraw.
    pub revision: u64,
    undo: Vec<(String, Map)>,
    redo: Vec<(String, Map)>,
    history: Vec<HistoryEntry>,
}

impl Session {
    /// A session with the sample map.
    pub fn sample() -> Session {
        let mut s = Session::default();
        if sample::add_to(&mut s.map).is_ok() {
            s.view_serial += 1;
        }
        s
    }

    pub fn commands() -> &'static [CommandSpec] {
        COMMANDS
    }

    pub fn command(id: &str) -> Option<&'static CommandSpec> {
        COMMANDS.iter().find(|c| c.id == id)
    }

    pub fn can_undo(&self) -> Option<&str> {
        self.undo.last().map(|(l, _)| l.as_str())
    }

    pub fn can_redo(&self) -> Option<&str> {
        self.redo.last().map(|(l, _)| l.as_str())
    }

    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    /// Whether the command exists and can run now.
    pub fn enabled(&self, id: &str) -> bool {
        Session::command(id).is_some_and(|c| (c.enabled)(self))
    }

    /// Runs a command with JSON parameters. On an error — or a panic, caught here as the last
    /// resort — the map is restored as it was and the error returned.
    pub fn execute(&mut self, id: &str, params: &Json) -> Result<Json> {
        match id {
            "edit.undo" => return self.undo(),
            "edit.redo" => return self.redo(),
            _ => {}
        }
        let spec = Session::command(id).ok_or_else(|| Error::NotFound(format!("command `{id}`")))?;
        if !(spec.enabled)(self) {
            return Err(Error::param("command", format!("`{id}` is not available now")));
        }
        let before = (spec.kind == Kind::Edit).then(|| self.map.clone());
        let (serial, path) = (self.view_serial, self.path.clone());
        let result = catch_unwind(AssertUnwindSafe(|| (spec.run)(self, params)))
            .unwrap_or_else(|_| Err(Error::Internal(format!("`{id}` failed unexpectedly; the map was restored"))));
        match (&result, before) {
            (Ok(_), Some(before)) => {
                self.undo.push((spec.label.to_string(), before));
                if self.undo.len() > UNDO_DEPTH {
                    self.undo.remove(0);
                }
                self.redo.clear();
                self.modified = true;
            }
            (Err(_), Some(before)) => {
                self.map = before;
                self.view_serial = serial;
                self.path = path;
            }
            _ => {}
        }
        if result.is_ok() && spec.kind != Kind::Query {
            self.revision += 1;
        }
        if spec.kind != Kind::Query {
            let message = match &result {
                Ok(v) => v.get("message").and_then(Json::as_str).unwrap_or("done").to_string(),
                Err(e) => e.to_string(),
            };
            self.history.push(HistoryEntry { command: id.to_string(), ok: result.is_ok(), message });
            if self.history.len() > HISTORY_DEPTH {
                self.history.remove(0);
            }
        }
        result
    }

    fn undo(&mut self) -> Result<Json> {
        let (label, map) = self.undo.pop().ok_or_else(|| Error::param("edit.undo", "nothing to undo"))?;
        let now = std::mem::replace(&mut self.map, map);
        self.redo.push((label.clone(), now));
        self.modified = true;
        self.revision += 1;
        Ok(serde_json::json!({ "undone": label }))
    }

    fn redo(&mut self) -> Result<Json> {
        let (label, map) = self.redo.pop().ok_or_else(|| Error::param("edit.redo", "nothing to redo"))?;
        let now = std::mem::replace(&mut self.map, map);
        self.undo.push((label.clone(), now));
        self.modified = true;
        self.revision += 1;
        Ok(serde_json::json!({ "redone": label }))
    }
}

#[cfg(test)]
mod tests;
