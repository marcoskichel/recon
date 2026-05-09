use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::session::Session;

#[derive(Default, Serialize, Deserialize)]
pub struct PersistedState {
    #[serde(default)]
    pub selected: usize,
    #[serde(default)]
    pub view_zoomed_room: Option<String>,
    #[serde(default)]
    pub view_zoom_index: Option<usize>,
    #[serde(default)]
    pub view_selected_agent: usize,
    #[serde(default)]
    pub view_room_order: Vec<String>,
    #[serde(default)]
    pub species_assignments: HashMap<String, usize>,
    #[serde(default)]
    pub custom_names: HashMap<String, String>,
    #[serde(default)]
    pub sessions: Vec<Session>,
}

fn state_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".claude").join("roostr-state.json"))
}

pub fn load() -> PersistedState {
    let Some(path) = state_path() else {
        return PersistedState::default();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return PersistedState::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save(state: &PersistedState) {
    let Some(path) = state_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec_pretty(state) {
        let _ = std::fs::write(path, json);
    }
}
