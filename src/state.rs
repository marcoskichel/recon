//! Disk-backed dashboard state (selection, room layout, sprite assignments).

use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::session::Session;

/// State persisted to `~/.claude/roostr-state.json` between TUI runs.
#[derive(Default, Serialize, Deserialize)]
pub struct PersistedState {
    /// Index of the currently-selected session in the list view.
    #[serde(default)]
    pub selected: usize,
    /// Identifier of the room currently zoomed in the room view, if any.
    #[serde(default)]
    pub view_zoomed_room: Option<String>,
    /// Index into the zoomed room's session list, if a room is zoomed.
    #[serde(default)]
    pub view_zoom_index: Option<usize>,
    /// Index of the selected agent card in room view.
    #[serde(default)]
    pub view_selected_agent: usize,
    /// Persisted ordering of room ids in the room view.
    #[serde(default)]
    pub view_room_order: Vec<String>,
    /// Mapping of session id → species (sprite type) index.
    #[serde(default)]
    pub species_assignments: HashMap<String, usize>,
    /// User-supplied custom display names per session id.
    #[serde(default)]
    pub custom_names: HashMap<String, String>,
    /// Snapshot of the most recently observed sessions.
    #[serde(default)]
    pub sessions: Vec<Session>,
}

/// Resolve the on-disk path used to persist dashboard state.
fn state_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".claude").join("roostr-state.json"))
}

/// Load persisted state from disk; returns defaults if missing or unreadable.
#[must_use]
pub fn load() -> PersistedState {
    let Some(path) = state_path() else {
        return PersistedState::default();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return PersistedState::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Persist state to disk best-effort; failures are silently ignored.
pub fn save(state: &PersistedState) {
    let Some(path) = state_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec_pretty(state) {
        let _ = std::fs::write(path, json);
    }
}
