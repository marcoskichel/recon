//! In-memory store of session labels with on-disk persistence semantics.
//!
//! The store is shared between the public [`crate::summarizer::Summarizer`] handle and the
//! background worker thread; it is cheap to clone via an internal `Arc<Mutex<_>>`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

/// Persistent record describing the current label for a JSONL session file.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CachedLabel {
    /// Size of the JSONL file the last time we generated a label for it.
    pub file_size: u64,
    /// Human-readable topic string.
    pub label: String,
    /// Unix timestamp (seconds) when the label was last updated.
    pub updated_at: u64,
}

/// Thread-safe shared map from JSONL session id to its [`CachedLabel`].
#[derive(Clone)]
pub struct LabelStore {
    /// Backing map; cloning the store shares this state across threads.
    inner: Arc<Mutex<HashMap<String, CachedLabel>>>,
}

impl Default for LabelStore {
    fn default() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }
}

impl LabelStore {
    /// Return the cached topic label for a session, if any.
    #[must_use]
    pub fn get(&self, session_id: &str) -> Option<String> {
        self.inner.lock().ok()?.get(session_id).map(|cached| cached.label.clone())
    }

    /// Return the file size we last associated with this session id, if any.
    pub(super) fn current_file_size(&self, session_id: &str) -> Option<u64> {
        self.inner.lock().ok()?.get(session_id).map(|cached| cached.file_size)
    }

    /// Insert or replace the cached entry for `session_id`.
    pub(super) fn put(&self, session_id: String, cached: CachedLabel) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.insert(session_id, cached);
        }
    }
}
