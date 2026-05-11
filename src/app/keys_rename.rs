//! Rename overlay (`r`) key handling: line-edit semantics on `rename_text`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    keys::{char_byte_offset, KeyOutcome},
    Application, InputMode,
};

impl Application {
    /// Rename-overlay key handler.
    pub(super) fn handle_key_rename(&mut self, event: KeyEvent) {
        if matches!(self.dispatch_rename_overlay_special(event), KeyOutcome::Handled) {
            return;
        }
        if matches!(self.dispatch_rename_overlay_edit(event), KeyOutcome::Handled) {
            return;
        }
        self.dispatch_rename_overlay_movement(event);
    }

    /// Esc / Enter / Ctrl-modifier shortcuts inside the rename overlay.
    fn dispatch_rename_overlay_special(&mut self, event: KeyEvent) -> KeyOutcome {
        match event.code {
            KeyCode::Esc => {
                self.cancel_rename();
                KeyOutcome::Handled
            }
            KeyCode::Enter => {
                self.commit_rename();
                KeyOutcome::Handled
            }
            KeyCode::Char('a') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.rename_cursor = 0;
                KeyOutcome::Handled
            }
            KeyCode::Char('e') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.rename_cursor = self.rename_text.chars().count();
                KeyOutcome::Handled
            }
            KeyCode::Char('u') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.rename_text.clear();
                self.rename_cursor = 0;
                KeyOutcome::Handled
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => KeyOutcome::Unhandled,
        }
    }

    /// Discard the rename overlay without saving.
    fn cancel_rename(&mut self) {
        self.input_mode = InputMode::Normal;
        self.rename_session_id = None;
        self.rename_text.clear();
        self.rename_cursor = 0;
    }

    /// Commit the rename overlay's text to `custom_names`.
    fn commit_rename(&mut self) {
        if let Some(session_id) = self.rename_session_id.take() {
            if self.rename_text.is_empty() {
                self.custom_names.remove(&session_id);
            } else {
                self.custom_names.insert(session_id, self.rename_text.clone());
            }
        }
        self.input_mode = InputMode::Normal;
        self.rename_text.clear();
        self.rename_cursor = 0;
    }

    /// Backspace / Delete / character insertion inside the rename overlay.
    fn dispatch_rename_overlay_edit(&mut self, event: KeyEvent) -> KeyOutcome {
        match event.code {
            KeyCode::Backspace => {
                self.rename_text_backspace();
                KeyOutcome::Handled
            }
            KeyCode::Delete => {
                self.rename_text_delete_forward();
                KeyOutcome::Handled
            }
            KeyCode::Char(character) if !event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.rename_text_insert(character);
                KeyOutcome::Handled
            }
            KeyCode::Char(_)
            | KeyCode::Enter
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Insert
            | KeyCode::Esc
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => KeyOutcome::Unhandled,
        }
    }

    /// Delete the character before the rename cursor.
    fn rename_text_backspace(&mut self) {
        if self.rename_cursor == 0 {
            return;
        }
        let prev = self.rename_cursor.saturating_sub(1);
        let start = char_byte_offset(&self.rename_text, prev).unwrap_or(0);
        let end_byte = char_byte_offset(&self.rename_text, self.rename_cursor)
            .unwrap_or(self.rename_text.len());
        self.rename_text.replace_range(start..end_byte, "");
        self.rename_cursor = prev;
    }

    /// Delete the character at the rename cursor.
    fn rename_text_delete_forward(&mut self) {
        let char_count = self.rename_text.chars().count();
        if self.rename_cursor >= char_count {
            return;
        }
        let start = char_byte_offset(&self.rename_text, self.rename_cursor)
            .unwrap_or(self.rename_text.len());
        let end_byte = char_byte_offset(&self.rename_text, self.rename_cursor.saturating_add(1))
            .unwrap_or(self.rename_text.len());
        self.rename_text.replace_range(start..end_byte, "");
    }

    /// Insert `character` at the rename cursor.
    fn rename_text_insert(&mut self, character: char) {
        let start = char_byte_offset(&self.rename_text, self.rename_cursor)
            .unwrap_or(self.rename_text.len());
        self.rename_text.insert(start, character);
        self.rename_cursor = self.rename_cursor.saturating_add(1);
    }

    /// Cursor movement inside the rename overlay.
    fn dispatch_rename_overlay_movement(&mut self, event: KeyEvent) {
        match event.code {
            KeyCode::Left => {
                self.rename_cursor = self.rename_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                if self.rename_cursor < self.rename_text.chars().count() {
                    self.rename_cursor = self.rename_cursor.saturating_add(1);
                }
            }
            KeyCode::Home => self.rename_cursor = 0,
            KeyCode::End => self.rename_cursor = self.rename_text.chars().count(),
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Enter
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::Esc
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
        }
    }
}
