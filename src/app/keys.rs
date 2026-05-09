//! Top-level keyboard event dispatcher.
//!
//! The actual per-mode handlers live in sibling modules:
//!
//! * [`super::keys_normal`] — default key handler (compact + zoomed views).
//! * [`super::keys_filter`] — filter overlay (`/`).
//! * [`super::keys_rename`] — rename overlay (`r`).
//! * [`super::keys_actions`] — shared "do thing in cwd" helpers.

use crossterm::event::{KeyCode, KeyEvent};

use crate::session;
use crate::tmux;
use crate::view_ui;

use super::{Application, InputMode};

/// Outcome of a single key dispatch helper. `Handled` short-circuits the
/// caller; `Unhandled` lets it fall through to the next stage.
pub(super) enum KeyOutcome {
    /// The key was consumed; the caller should return immediately.
    Handled,
    /// The key was not relevant; the caller should keep dispatching.
    Unhandled,
}

impl Application {
    /// Top-level dispatcher; routes keys to the active modal handler.
    pub fn handle_key(&mut self, event: KeyEvent) {
        match self.input_mode {
            InputMode::Rename => {
                self.handle_key_rename(event);
            }
            InputMode::Filter => {
                self.handle_key_filter(event);
            }
            InputMode::Normal => {
                if matches!(event.code, KeyCode::Tab | KeyCode::Char('i')) {
                    self.jump_to_next_input();
                    return;
                }
                self.handle_key_view(event);
            }
        }
    }

    /// Switch to the next session that's waiting for user input, if any.
    pub(super) fn jump_to_next_input(&mut self) {
        let pane_opt = self
            .sessions
            .iter()
            .find(|sess| sess.status == session::SessionStatus::Input)
            .and_then(|sess| sess.pane_target.clone());
        if let Some(pane) = pane_opt {
            tmux::switch_to_pane(&pane);
            self.should_quit = true;
        }
    }

    /// Begin editing the rename overlay for `session_id`.
    pub(super) fn start_rename(&mut self, session_id: String) {
        let current = self.custom_names.get(&session_id).cloned().unwrap_or_else(|| {
            let species = self
                .species_assignments
                .get(&session_id)
                .copied()
                .unwrap_or_else(|| view_ui::text::pick_species(&session_id));
            let slot = species % view_ui::types::SPECIES_COUNT;
            view_ui::types::SPECIES_NAMES
                .get(slot)
                .map_or_else(String::new, |name| (*name).to_string())
        });
        self.rename_text = current;
        self.rename_cursor = self.rename_text.chars().count();
        self.rename_session_id = Some(session_id);
        self.input_mode = InputMode::Rename;
    }
}

/// Convert an ASCII digit `'1'..='9'` into its zero-based index, or `None`
/// if `value` isn't actually a digit.
pub(super) fn digit_index(value: char) -> Option<usize> {
    let digit = value.to_digit(10)?;
    let zero_based = digit.checked_sub(1)?;
    usize::try_from(zero_based).ok()
}

/// Byte offset of the `nth_char`-th character in `text`, or `None` if out of
/// range.
pub(super) fn char_byte_offset(text: &str, nth_char: usize) -> Option<usize> {
    if nth_char == text.chars().count() {
        return Some(text.len());
    }
    text.char_indices().nth(nth_char).map(|(byte_offset, _)| byte_offset)
}

/// File name component of `path` (best-effort), falling back to `default`.
pub(super) fn file_name_or(path: &str, default: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| default.to_string(), |name| name.to_string_lossy().into_owned())
}

/// `true` iff `name` is an executable file in any directory on `$PATH`.
pub(super) fn binary_in_path(name: &str) -> bool {
    let Ok(path_var) = std::env::var("PATH") else {
        return false;
    };
    path_var.split(':').any(|directory| std::path::Path::new(directory).join(name).is_file())
}
