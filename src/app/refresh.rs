//! Refresh-loop helpers: pulling new session data and reconciling species
//! assignments with the live session set.

use std::collections::HashSet;

use super::Application;
use crate::{
    session::{self, Session},
    view_ui,
};

impl Application {
    /// Synchronously poll the system for live sessions and apply the result.
    pub fn refresh(&mut self) {
        let discovered = session::discover_sessions(&self.prev_sessions);
        let sessions: Vec<Session> =
            discovered.into_iter().filter(|session| session.tmux_name.is_some()).collect();

        self.prev_sessions =
            sessions.iter().map(|session| (session.id.clone(), session.clone())).collect();

        self.apply_snapshot(sessions);
    }

    /// Apply a snapshot produced by an external worker thread.
    pub fn apply_snapshot(&mut self, sessions: Vec<Session>) {
        self.loaded = true;
        for session in &sessions {
            if !session.jsonl_path.as_os_str().is_empty() {
                self.summarizer.maybe_enqueue(
                    &session.id,
                    &session.jsonl_path,
                    session.last_file_size,
                );
            }
        }

        self.sessions = sessions;
        self.assign_species_to_new_sessions();

        self.clamp_selection();
    }

    /// Garbage-collect species assignments for departed sessions and assign
    /// new ones to fresh sessions, preferring distinct species per dashboard.
    fn assign_species_to_new_sessions(&mut self) {
        let active_ids: HashSet<String> =
            self.sessions.iter().map(|session| session.id.clone()).collect();
        self.species_assignments.retain(|id, _| active_ids.contains(id));

        let used: HashSet<usize> = self.species_assignments.values().copied().collect();
        let mut available: Vec<usize> =
            (0..view_ui::types::SPECIES_COUNT).filter(|species| !used.contains(species)).collect();

        // Snapshot ids first to avoid borrowing `self.sessions` while mutating
        // `self.species_assignments`.
        let session_ids: Vec<String> =
            self.sessions.iter().map(|session| session.id.clone()).collect();
        for session_id in session_ids {
            self.species_assignments.entry(session_id.clone()).or_insert_with(|| {
                if available.is_empty() {
                    view_ui::text::pick_species(&session_id)
                } else {
                    available.remove(0)
                }
            });
        }
    }
}
