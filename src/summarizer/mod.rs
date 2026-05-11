//! Background labelling daemon for active Claude Code sessions.
//!
//! [`Summarizer`] owns a worker thread that pulls JSONL transcripts and feeds
//! them to one of three configured backends (Anthropic, the local `claude`
//! CLI, or a local Ollama server). The resulting topic labels are exposed via
//! [`LabelStore`] for the TUI to render in agent cards.

mod anthropic;
mod backend;
mod cache;
mod claude_cli;
mod ollama;
mod prompt;
mod runtime;
pub mod store;

use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Sender},
        Arc, Mutex,
    },
    thread,
};

use self::{
    backend::select_backend,
    cache::load_cache_into,
    runtime::{unix_now, worker_loop, LabelJob},
    store::LabelStore,
};

/// Minimum number of seconds between two enqueues for the same session.
const MIN_DEBOUNCE_SECS: u64 = 300;

/// Public handle for the summarizer subsystem.
pub struct Summarizer {
    /// Sender for the worker queue; `None` when the backend never started.
    sender: Option<Sender<LabelJob>>,
    /// Shared store of cached labels (also held by the worker thread).
    pub store: LabelStore,
    /// Per-session timestamp (seconds) of the most recent enqueue, for debouncing.
    last_enqueued: Mutex<HashMap<String, u64>>,
    /// `true` once a backend was successfully selected; `false` until probing finishes.
    enabled: Arc<AtomicBool>,
}

impl Summarizer {
    /// Start the summarizer with backend probing deferred to a worker thread.
    /// `enabled()` will return false until the probe completes; if the probe
    /// finds no backend, it stays false forever.
    #[must_use]
    pub fn start() -> Self {
        Self::start_inner(false)
    }

    /// Start the summarizer and block until backend selection finishes.
    /// Used by daemon mode where we exit immediately if no backend is available.
    #[must_use]
    pub fn start_blocking() -> Self {
        Self::start_inner(true)
    }

    /// Returns `true` once a backend has been selected and the worker thread is running.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Maybe enqueue a [`LabelJob`] for `session_id` if the file changed and the
    /// debounce window elapsed.
    pub fn maybe_enqueue(&self, session_id: &str, jsonl_path: &Path, file_size: u64) {
        if !self.enabled() || file_size == 0 {
            return;
        }
        let Some(sender) = self.sender.as_ref() else {
            return;
        };

        let cur_size = self.store.current_file_size(session_id).unwrap_or(0);
        if cur_size == file_size {
            return;
        }

        if !self.allow_enqueue(session_id) {
            return;
        }

        let previous_label = self.store.get(session_id);
        let _ = sender.send(LabelJob {
            session_id: session_id.to_owned(),
            jsonl_path: jsonl_path.to_path_buf(),
            file_size,
            previous_label,
        });
    }

    /// Apply debounce; returns `true` when the call should proceed and records the
    /// current timestamp.
    fn allow_enqueue(&self, session_id: &str) -> bool {
        let Ok(mut state) = self.last_enqueued.lock() else {
            return false;
        };
        let now_secs = unix_now();
        let last_secs = state.get(session_id).copied().unwrap_or(0);
        if now_secs.saturating_sub(last_secs) < MIN_DEBOUNCE_SECS {
            return false;
        }
        state.insert(session_id.to_owned(), now_secs);
        true
    }

    /// Common construction path used by [`start`](Self::start) and
    /// [`start_blocking`](Self::start_blocking).
    fn start_inner(blocking: bool) -> Self {
        let store = LabelStore::default();
        load_cache_into(&store);

        let enabled = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = channel::<LabelJob>();

        if blocking {
            spawn_blocking(&enabled, &store, receiver);
        } else {
            spawn_deferred(&enabled, &store, receiver);
        }

        Self { sender: Some(sender), store, last_enqueued: Mutex::new(HashMap::new()), enabled }
    }
}

/// Probe synchronously, then start the worker thread only if a backend exists.
fn spawn_blocking(
    enabled: &Arc<AtomicBool>,
    store: &LabelStore,
    receiver: std::sync::mpsc::Receiver<LabelJob>,
) {
    if let Some(backend) = select_backend() {
        enabled.store(true, Ordering::Release);
        let worker_store = store.clone();
        thread::spawn(move || worker_loop(&receiver, &worker_store, &backend));
    }
}

/// Defer probing to a background thread so startup stays non-blocking.
fn spawn_deferred(
    enabled: &Arc<AtomicBool>,
    store: &LabelStore,
    receiver: std::sync::mpsc::Receiver<LabelJob>,
) {
    let worker_store = store.clone();
    let enabled_clone = Arc::clone(enabled);
    thread::spawn(move || {
        if let Some(backend) = select_backend() {
            enabled_clone.store(true, Ordering::Release);
            worker_loop(&receiver, &worker_store, &backend);
        }
    });
}
