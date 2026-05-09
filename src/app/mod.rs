//! Top-level application state for the roostr TUI dashboard.
//!
//! This module hosts the [`App`] struct (re-exported as a type alias to keep
//! the public API stable) and orchestrates the four discrete concerns of the
//! dashboard:
//!
//! * [`refresh`] — pulling live session data from disk and tmux.
//! * [`keys`]    — translating keyboard events into state changes.
//! * [`grid`]    — selection arithmetic for the compact and zoomed views.
//!
//! Each submodule extends `Application` via `impl` blocks; the type itself is
//! defined here so its private fields stay visible only within `crate::app`.

mod grid;
mod keys;
mod keys_actions;
mod keys_filter;
mod keys_normal;
mod keys_rename;
mod refresh;

use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::session::Session;
use crate::state::{self, PersistedState};
use crate::summarizer::Summarizer;

/// How long a transient status-bar message remains visible.
const STATUS_MESSAGE_TTL: Duration = Duration::from_secs(3);

/// Public alias kept for backwards compatibility with `main.rs` /
/// `view_ui` / `runtime` modules, which all import [`App`] by that exact
/// name.
pub type App = Application;

/// Discrete editor state for the input bar.
///
/// Replaces the previous pair of `filter_active` / `rename_active` booleans so
/// the dashboard struct stays under the project's two-bool-fields ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// No modal input is active; key handler dispatches dashboard shortcuts.
    Normal,
    /// User is typing into the filter overlay.
    Filter,
    /// User is editing a session's custom display name.
    Rename,
}

/// Top-level dashboard state shared across the refresh loop and the renderer.
pub struct Application {
    /// Live sessions discovered on the most recent refresh.
    pub sessions: Vec<Session>,
    /// Index of the currently-selected entry in the (filtered) session list.
    pub selected: usize,
    /// Set to `true` to ask the main loop to exit cleanly.
    pub should_quit: bool,
    /// Monotonically increasing tick count, used to drive sprite animations.
    pub tick: u64,
    /// Name of the room currently zoomed in the room view, if any.
    pub view_zoomed_room: Option<String>,
    /// Index of the room to zoom on the next render frame, if any.
    pub view_zoom_index: Option<usize>,
    /// Cursor position into the flat list of sessions in the compact view.
    pub view_selected_agent: usize,
    /// Current free-text filter applied to the session list.
    pub filter_text: String,
    /// Caret position (in chars) within `filter_text`.
    pub filter_cursor: usize,
    /// Last layout's `chars_per_row` value for grid arithmetic; updated by the
    /// renderer and consumed by the key handler for vertical navigation.
    pub view_chars_per_row: Cell<usize>,
    /// Stable ordering of room ids in the compact view.
    pub view_room_order: Vec<String>,
    /// Background labeller producing human-readable session summaries.
    pub summarizer: Summarizer,
    /// Transient status-bar message and the instant it was set.
    pub status_message: Option<(String, Instant)>,
    /// Mapping of session id → species (sprite) index.
    pub species_assignments: HashMap<String, usize>,
    /// User-supplied custom display names per session id.
    pub custom_names: HashMap<String, String>,
    /// Session id whose name is currently being edited.
    pub rename_session_id: Option<String>,
    /// Working buffer for the rename overlay.
    pub rename_text: String,
    /// Caret position (in chars) within `rename_text`.
    pub rename_cursor: usize,
    /// `true` once the first refresh has been applied.
    pub loaded: bool,
    /// Active input modality (normal, filter overlay, rename overlay).
    pub input_mode: InputMode,
    /// Snapshot of the previous refresh's sessions, keyed by session id, used
    /// to do incremental JSONL parsing.
    pub(crate) prev_sessions: HashMap<String, Session>,
}

impl Application {
    /// Build a new app, spawning the background summarizer worker.
    #[must_use]
    pub fn new() -> Self {
        Self::with_summarizer(Summarizer::start())
    }

    /// Build a new app with the summarizer running on the current thread.
    ///
    /// Used by the daemon process where threads are unwelcome.
    #[must_use]
    pub fn new_blocking() -> Self {
        Self::with_summarizer(Summarizer::start_blocking())
    }

    /// Internal constructor that hydrates state from disk and seeds the
    /// previous-sessions cache.
    fn with_summarizer(summarizer: Summarizer) -> Self {
        let persisted = state::load();
        let cached_sessions = persisted.sessions;
        let loaded = !cached_sessions.is_empty();
        let prev_sessions =
            cached_sessions.iter().map(|session| (session.id.clone(), session.clone())).collect();
        Self {
            sessions: cached_sessions,
            selected: persisted.selected,
            should_quit: false,
            tick: 0,
            view_zoomed_room: persisted.view_zoomed_room,
            view_zoom_index: persisted.view_zoom_index,
            view_selected_agent: persisted.view_selected_agent,
            filter_text: String::new(),
            filter_cursor: 0,
            view_chars_per_row: Cell::new(1),
            view_room_order: persisted.view_room_order,
            summarizer,
            status_message: None,
            species_assignments: persisted.species_assignments,
            custom_names: persisted.custom_names,
            rename_session_id: None,
            rename_text: String::new(),
            rename_cursor: 0,
            loaded,
            input_mode: InputMode::Normal,
            prev_sessions,
        }
    }

    /// Persist the dashboard state to disk best-effort.
    pub fn save_state(&self) {
        state::save(&PersistedState {
            selected: self.selected,
            view_zoomed_room: self.view_zoomed_room.clone(),
            view_zoom_index: self.view_zoom_index,
            view_selected_agent: self.view_selected_agent,
            view_room_order: self.view_room_order.clone(),
            species_assignments: self.species_assignments.clone(),
            custom_names: self.custom_names.clone(),
            sessions: self.sessions.clone(),
        });
    }

    /// Show a transient status-bar message that auto-expires after
    /// [`STATUS_MESSAGE_TTL`].
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some((message.into(), Instant::now()));
    }

    /// Return the active status message, or `None` if it has expired.
    #[must_use]
    pub fn active_status_message(&self) -> Option<&str> {
        self.status_message.as_ref().and_then(|&(ref message, set_at)| {
            if set_at.elapsed() < STATUS_MESSAGE_TTL {
                Some(message.as_str())
            } else {
                None
            }
        })
    }

    /// `true` if the filter overlay is currently focused.
    #[must_use]
    pub const fn filter_active(&self) -> bool {
        matches!(self.input_mode, InputMode::Filter)
    }

    /// `true` if the rename overlay is currently focused.
    #[must_use]
    pub const fn rename_active(&self) -> bool {
        matches!(self.input_mode, InputMode::Rename)
    }

    /// Build a snapshot of the current sessions for the background refresh
    /// worker to use as its previous-state cache.
    #[must_use]
    pub fn snapshot_prev(&self) -> HashMap<String, Session> {
        self.sessions.iter().map(|session| (session.id.clone(), session.clone())).collect()
    }

    /// Bump the animation tick once per render frame.
    pub const fn advance_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    /// Indices into `self.sessions` that pass the active filter.
    #[must_use]
    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.filter_text.is_empty() {
            return (0..self.sessions.len()).collect();
        }
        let query = self.filter_text.to_lowercase();
        self.sessions
            .iter()
            .enumerate()
            .filter(|&(_, session)| {
                session.project_name.to_lowercase().contains(&query)
                    || session.tmux_name.as_deref().unwrap_or("").to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Re-clamp `selected` so it always points to a valid filtered row.
    pub(crate) fn clamp_selection(&mut self) {
        let count = self.filtered_indices().len();
        if count == 0 {
            self.selected = 0;
        } else {
            let last = count.saturating_sub(1);
            self.selected = self.selected.min(last);
        }
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}
