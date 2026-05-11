//! Filter overlay (`/`) key handling: line-edit semantics on `filter_text`
//! plus arrow-key selection cycling.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    keys::{char_byte_offset, KeyOutcome},
    Application, InputMode,
};
use crate::tmux;

impl Application {
    /// Filter-overlay key handler.
    pub(super) fn handle_key_filter(&mut self, event: KeyEvent) {
        if matches!(self.dispatch_filter_overlay_special(event), KeyOutcome::Handled) {
            return;
        }
        if matches!(self.dispatch_filter_overlay_edit(event), KeyOutcome::Handled) {
            return;
        }
        self.dispatch_filter_overlay_movement(event);
    }

    /// Esc / Enter / Ctrl-modifier shortcuts inside the filter overlay.
    fn dispatch_filter_overlay_special(&mut self, event: KeyEvent) -> KeyOutcome {
        match event.code {
            KeyCode::Esc => {
                self.exit_filter_overlay();
                KeyOutcome::Handled
            }
            KeyCode::Enter => {
                self.try_attach_filter_match();
                KeyOutcome::Handled
            }
            KeyCode::Char(character)
                if event.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(character, 'a' | 'e' | 'u') =>
            {
                self.handle_filter_overlay_control(character);
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

    /// Cancel the filter overlay and clear its state.
    fn exit_filter_overlay(&mut self) {
        self.input_mode = InputMode::Normal;
        self.filter_text.clear();
        self.filter_cursor = 0;
        self.selected = 0;
    }

    /// Handle Ctrl-modifier shortcuts inside the filter overlay (`a` →
    /// home, `e` → end, `u` → clear).
    fn handle_filter_overlay_control(&mut self, character: char) {
        match character {
            'a' => self.filter_cursor = 0,
            'e' => self.filter_cursor = self.filter_text.chars().count(),
            'u' => {
                self.filter_text.clear();
                self.filter_cursor = 0;
                self.clamp_selection();
            }
            _ => {}
        }
    }

    /// Backspace / Delete / character insertion inside the filter overlay.
    fn dispatch_filter_overlay_edit(&mut self, event: KeyEvent) -> KeyOutcome {
        match event.code {
            KeyCode::Backspace => {
                self.filter_text_backspace();
                KeyOutcome::Handled
            }
            KeyCode::Delete => {
                self.filter_text_delete_forward();
                KeyOutcome::Handled
            }
            KeyCode::Char(character) if !event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_text_insert(character);
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

    /// Delete the character before the filter cursor.
    fn filter_text_backspace(&mut self) {
        if self.filter_cursor == 0 {
            return;
        }
        let prev = self.filter_cursor.saturating_sub(1);
        let start = char_byte_offset(&self.filter_text, prev).unwrap_or(0);
        let end_byte = char_byte_offset(&self.filter_text, self.filter_cursor)
            .unwrap_or(self.filter_text.len());
        self.filter_text.replace_range(start..end_byte, "");
        self.filter_cursor = prev;
        self.clamp_selection();
    }

    /// Delete the character at the filter cursor (forward delete).
    fn filter_text_delete_forward(&mut self) {
        let char_count = self.filter_text.chars().count();
        if self.filter_cursor >= char_count {
            return;
        }
        let start = char_byte_offset(&self.filter_text, self.filter_cursor)
            .unwrap_or(self.filter_text.len());
        let end_byte = char_byte_offset(&self.filter_text, self.filter_cursor.saturating_add(1))
            .unwrap_or(self.filter_text.len());
        self.filter_text.replace_range(start..end_byte, "");
        self.clamp_selection();
    }

    /// Insert `character` at the filter cursor.
    fn filter_text_insert(&mut self, character: char) {
        let start = char_byte_offset(&self.filter_text, self.filter_cursor)
            .unwrap_or(self.filter_text.len());
        self.filter_text.insert(start, character);
        self.filter_cursor = self.filter_cursor.saturating_add(1);
        self.clamp_selection();
    }

    /// Cursor and selection movement inside the filter overlay.
    fn dispatch_filter_overlay_movement(&mut self, event: KeyEvent) {
        match event.code {
            KeyCode::Left => {
                self.filter_cursor = self.filter_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                let char_count = self.filter_text.chars().count();
                if self.filter_cursor < char_count {
                    self.filter_cursor = self.filter_cursor.saturating_add(1);
                }
            }
            KeyCode::Home => self.filter_cursor = 0,
            KeyCode::End => self.filter_cursor = self.filter_text.chars().count(),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Tab | KeyCode::Char('i') => {
                self.jump_to_next_input();
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Enter
            | KeyCode::PageUp
            | KeyCode::PageDown
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

    /// Move the filtered-list cursor one step down, clamped to the end.
    fn move_selection_down(&mut self) {
        let count = self.filtered_indices().len();
        if count > 0 {
            let last = count.saturating_sub(1);
            self.selected = self.selected.saturating_add(1).min(last);
        }
    }

    /// On Enter from the filter overlay: if exactly one match, attach to it.
    fn try_attach_filter_match(&mut self) {
        let pane_opt = self.unique_filter_pane_target();
        if let Some(pane) = pane_opt {
            tmux::switch_to_pane(&pane);
            self.should_quit = true;
            return;
        }
        self.input_mode = InputMode::Normal;
    }

    /// If exactly one filtered session has a pane target, return it.
    fn unique_filter_pane_target(&self) -> Option<String> {
        let indices = self.filtered_indices();
        if indices.len() != 1 {
            return None;
        }
        indices
            .first()
            .and_then(|&index| self.sessions.get(index))
            .and_then(|sess| sess.pane_target.clone())
    }
}
