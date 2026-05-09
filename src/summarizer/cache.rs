//! On-disk cache for session labels.
//!
//! Labels are persisted under the user's cache directory so that previously
//! computed topic strings survive across daemon and TUI restarts.

use std::fs;
use std::path::PathBuf;

use super::store::{CachedLabel, LabelStore};

/// Returns the directory used for label cache files, creating it if needed.
pub(super) fn cache_dir() -> Option<PathBuf> {
    let mut path = dirs::cache_dir()?;
    path.push("roostr");
    path.push("labels");
    let _ = fs::create_dir_all(&path);
    Some(path)
}

/// Load every cached label JSON file into the in-memory [`LabelStore`].
pub(super) fn load_cache_into(store: &LabelStore) {
    let Some(dir_path) = cache_dir() else {
        return;
    };
    let Ok(entries) = fs::read_dir(&dir_path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|file_stem| file_stem.to_str()) else {
            continue;
        };
        let session_id = stem.to_owned();
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(cached) = serde_json::from_str::<CachedLabel>(&content) {
            store.put(session_id, cached);
        }
    }
}

/// Persist a single cached label to disk. Errors are silently ignored — the
/// in-memory store remains the source of truth for this run.
pub(super) fn persist_label(session_id: &str, cached: &CachedLabel) {
    let Some(dir_path) = cache_dir() else {
        return;
    };
    let path = dir_path.join(format!("{session_id}.json"));
    if let Ok(serialized) = serde_json::to_string(cached) {
        let _ = fs::write(path, serialized);
    }
}
