//! The engine (L4): a [`Session`] holds the map, undo/redo and the geoprocessing history, and
//! runs commands. Everything a person can do is a command (`cmd`): the UI, the control channel,
//! the CLI and the MCP server all go through [`Session::execute`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod cmd;
pub mod disk;
pub mod io;
pub mod sample;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

pub use mkaegis_core::{Error, Result};
pub use mkaegis_model::Map;
use serde_json::Value as Json;

pub use cmd::{COMMANDS, CommandSpec, Kind};
pub use disk::Disk;
/// Calculate Geometry's properties, so the UI offers only those a layer's shapes have.
pub use mkaegis_analysis::Property;
/// Summary Statistics' default quantile definition, for the UI's form.
pub use mkaegis_stats::descriptive::DEFAULT_QUANTILE_TYPE;
/// The exploratory views' statistics, for the UI's charts (they draw what this computes).
pub use mkaegis_stats::explore;

/// Undo steps kept.
const UNDO_DEPTH: usize = 50;
/// The most memory undo holds apart from the current map: snapshots share every layer a step
/// didn't change, so this counts only what steps changed or removed (#59). The newest step is
/// kept whatever its size.
const UNDO_BYTES: usize = 1 << 30;
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
    /// Where commands read and write files: the file system, or memory in the browser.
    pub disk: Disk,
    /// The basemap tiles fetched so far (by the app, as the map is viewed), which Export Map
    /// draws from: commands never fetch.
    pub tiles: mkaegis_render::basemap::TileStore,
    pub modified: bool,
    /// Bumped whenever a command asks the view to move (zoom to layer, open…): the UI refits.
    pub view_serial: u64,
    /// Bumped whenever the map may have changed: views redraw.
    pub revision: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    history: Vec<HistoryEntry>,
    /// Which state the map is in, and which one was last saved: undoing back to the saved state
    /// leaves nothing unsaved.
    state: u64,
    saved: u64,
    /// The next state number to hand out.
    states: u64,
}

/// A map an undo or redo step returns to. It shares the layers it has in common with the
/// current map (their features are behind an `Arc`); `bytes` estimates what it doesn't share.
#[derive(Debug, Clone)]
struct Snapshot {
    label: String,
    map: Map,
    state: u64,
    bytes: usize,
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
        let found = COMMANDS.iter().find(|c| c.id == id);
        // Tests add commands that panic, to prove the map is restored.
        #[cfg(test)]
        let found = found.or_else(|| tests::FAILING.iter().find(|c| c.id == id));
        found
    }

    pub fn can_undo(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn can_redo(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    /// Whether the command exists and can run now.
    pub fn enabled(&self, id: &str) -> bool {
        Session::command(id).is_some_and(|c| (c.enabled)(self))
    }

    /// Runs a command with JSON parameters. On an error, or a panic caught here as the last
    /// resort, the map is restored as it was (for every command but queries, which only read
    /// it) and the error returned. Edit commands are undoable.
    pub fn execute(&mut self, id: &str, params: &Json) -> Result<Json> {
        // A save, an open or a new map marks the state it leaves as saved.
        self.mark_saved();
        match id {
            "edit.undo" => return self.undo(),
            "edit.redo" => return self.redo(),
            _ => {}
        }
        let spec = Session::command(id).ok_or_else(|| Error::NotFound(format!("command `{id}`")))?;
        if !(spec.enabled)(self) {
            return Err(Error::param("command", format!("`{id}` is not available now")));
        }
        // Cheap: the clone shares every layer's features until a command changes them.
        let before = (spec.kind != Kind::Query).then(|| self.map.clone());
        let (serial, path) = (self.view_serial, self.path.clone());
        let result = catch_unwind(AssertUnwindSafe(|| (spec.run)(self, params)))
            .unwrap_or_else(|_| Err(Error::Internal(format!("`{id}` failed unexpectedly; the map was restored"))));
        match (&result, before) {
            // A new or opened project starts a fresh undo history and has nothing unsaved. Undoing
            // back to the previous map would keep the new file's path, and Save would then write
            // the old map over it (#41).
            (Ok(_), Some(_)) if matches!(id, "project.new" | "project.open") => {
                self.undo.clear();
                self.redo.clear();
                self.state = self.next_state();
                self.modified = false;
            }
            (Ok(_), Some(before)) if spec.kind == Kind::Edit => {
                let bytes = unshared_bytes(&before, &self.map);
                let next = self.next_state();
                self.undo.push(Snapshot { label: spec.label.to_string(), map: before, state: self.state, bytes });
                self.state = next;
                self.trim_undo(UNDO_BYTES);
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
        self.mark_saved();
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
        let step = self.undo.pop().ok_or_else(|| Error::param("edit.undo", "nothing to undo"))?;
        let label = step.label.clone();
        let back = self.restore(step);
        self.redo.push(back);
        Ok(serde_json::json!({ "undone": label }))
    }

    fn redo(&mut self) -> Result<Json> {
        let step = self.redo.pop().ok_or_else(|| Error::param("edit.redo", "nothing to redo"))?;
        let label = step.label.clone();
        let back = self.restore(step);
        self.undo.push(back);
        self.trim_undo(UNDO_BYTES);
        Ok(serde_json::json!({ "redone": label }))
    }

    /// Puts a snapshot's map back and returns the one it replaced, as a snapshot to go back to.
    /// The project is unsaved unless this is the state last saved; views redraw, and when the
    /// CRS differs the view refits, since its extent is in the other CRS's units.
    fn restore(&mut self, step: Snapshot) -> Snapshot {
        let now = std::mem::replace(&mut self.map, step.map);
        let bytes = unshared_bytes(&now, &self.map);
        let back = Snapshot { label: step.label, map: now, state: self.state, bytes };
        self.state = step.state;
        self.modified = self.state != self.saved;
        self.revision += 1;
        if back.map.crs != self.map.crs {
            self.view_serial += 1;
        }
        back
    }

    /// Drops the oldest undo steps past `UNDO_DEPTH`, or past `budget` bytes held apart from the
    /// current map; the newest step stays.
    fn trim_undo(&mut self, budget: usize) {
        let mut held: usize = self.undo.iter().map(|s| s.bytes).sum();
        while self.undo.len() > UNDO_DEPTH || (held > budget && self.undo.len() > 1) {
            let dropped = self.undo.remove(0);
            held = held.saturating_sub(dropped.bytes);
        }
    }

    fn next_state(&mut self) -> u64 {
        self.states += 1;
        self.states
    }

    /// Nothing unsaved: the current state is the saved one.
    fn mark_saved(&mut self) {
        if !self.modified {
            self.saved = self.state;
        }
    }
}

/// About how many bytes `snapshot` holds that `current` doesn't share: the features of layers
/// and the weights that aren't the same allocation in both.
fn unshared_bytes(snapshot: &Map, current: &Map) -> usize {
    let layers: usize = snapshot
        .layers
        .iter()
        .filter(|l| !current.layers.iter().any(|c| std::sync::Arc::ptr_eq(&c.data, &l.data)))
        .map(|l| l.data.approx_bytes())
        .sum();
    let weights: usize =
        snapshot.weights.iter().filter(|w| !current.weights.iter().any(|c| std::sync::Arc::ptr_eq(c, w))).map(|w| w.approx_bytes()).sum();
    layers.saturating_add(weights)
}

#[cfg(test)]
mod tests;
