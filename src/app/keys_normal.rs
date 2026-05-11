//! Default-mode key handling: compact view + zoomed-room view + global keys.

use crossterm::event::{KeyCode, KeyEvent};

use super::{
    keys::{digit_index, KeyOutcome},
    Application, InputMode,
};

impl Application {
    /// Default key handler — used when no modal overlay is active.
    pub(super) fn handle_key_view(&mut self, event: KeyEvent) {
        if self.view_zoomed_room.is_none() {
            if matches!(self.dispatch_compact_nav(event), KeyOutcome::Handled) {
                return;
            }
            if matches!(self.dispatch_compact_actions(event), KeyOutcome::Handled) {
                return;
            }
        }
        if self.view_zoomed_room.is_some() {
            if matches!(self.dispatch_zoomed_nav(event), KeyOutcome::Handled) {
                return;
            }
            if matches!(self.dispatch_zoomed_actions(event), KeyOutcome::Handled) {
                return;
            }
        }
        self.dispatch_global(event);
    }

    /// Compact-view navigation keys (h/j/k/l, arrows, digit jumps).
    fn dispatch_compact_nav(&mut self, event: KeyEvent) -> KeyOutcome {
        let total = self.compact_flat_session_indices().len();
        if total == 0 {
            return KeyOutcome::Unhandled;
        }
        match event.code {
            KeyCode::Char('l') | KeyCode::Right => {
                let last = total.saturating_sub(1);
                self.view_selected_agent = self.view_selected_agent.saturating_add(1).min(last);
                KeyOutcome::Handled
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.view_selected_agent = self.view_selected_agent.saturating_sub(1);
                KeyOutcome::Handled
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.view_selected_agent = self.compact_grid_move_down();
                KeyOutcome::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.view_selected_agent = self.compact_grid_move_up();
                KeyOutcome::Handled
            }
            KeyCode::Char(digit @ '1'..='9') => {
                self.try_jump_to_digit(digit, total);
                KeyOutcome::Handled
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Enter
            | KeyCode::Home
            | KeyCode::End
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
            | KeyCode::Modifier(_) => KeyOutcome::Unhandled,
        }
    }

    /// Helper for digit-jump in the compact view.
    fn try_jump_to_digit(&mut self, digit: char, total: usize) {
        if let Some(index) = digit_index(digit) {
            if index < total {
                self.view_selected_agent = index;
            }
        }
    }

    /// Compact-view actions (open, kill, new, edit, terminal, lazygit, …).
    fn dispatch_compact_actions(&mut self, event: KeyEvent) -> KeyOutcome {
        match event.code {
            KeyCode::Enter => self.action_attach_compact(),
            KeyCode::Char('x') => self.action_kill_compact(),
            KeyCode::Char('n') => {
                self.action_with_compact_cwd(|state, working_dir| state.spawn_claude(&working_dir))
            }
            KeyCode::Char('e') => {
                self.action_with_compact_cwd(|state, working_dir| state.spawn_editor(&working_dir))
            }
            KeyCode::Char('t') => self
                .action_with_compact_cwd(|state, working_dir| state.spawn_terminal(&working_dir)),
            KeyCode::Char('g') => self.action_with_compact_cwd(|state, working_dir| {
                state.open_tui_tool("lazygit", "lazygit", &working_dir);
            }),
            KeyCode::Char('d') => {
                self.action_with_compact_cwd(|state, working_dir| state.open_diffnav(&working_dir))
            }
            KeyCode::Char('D') => self.action_with_compact_cwd(|state, working_dir| {
                state.open_tui_tool("gh", "gh dash", &working_dir);
            }),
            KeyCode::Char('r') => self.action_rename_compact(),
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

    /// Zoomed-view navigation keys.
    const fn dispatch_zoomed_nav(&mut self, event: KeyEvent) -> KeyOutcome {
        match event.code {
            KeyCode::Char('l') | KeyCode::Right => {
                self.view_selected_agent = self.view_selected_agent.saturating_add(1);
                KeyOutcome::Handled
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.view_selected_agent = self.view_selected_agent.saturating_sub(1);
                KeyOutcome::Handled
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Enter
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

    /// Zoomed-view actions (mirror compact actions but operate on the zoomed
    /// room).
    fn dispatch_zoomed_actions(&mut self, event: KeyEvent) -> KeyOutcome {
        match event.code {
            KeyCode::Enter => self.action_attach_zoomed(),
            KeyCode::Char('x') => self.action_kill_zoomed(),
            KeyCode::Char('n') => {
                self.action_with_zoomed_cwd(|state, working_dir| state.spawn_claude(&working_dir))
            }
            KeyCode::Char('e') => {
                self.action_with_zoomed_cwd(|state, working_dir| state.spawn_editor(&working_dir))
            }
            KeyCode::Char('t') => {
                self.action_with_zoomed_cwd(|state, working_dir| state.spawn_terminal(&working_dir))
            }
            KeyCode::Char('g') => self.action_with_zoomed_cwd(|state, working_dir| {
                state.open_tui_tool("lazygit", "lazygit", &working_dir);
            }),
            KeyCode::Char('d') => {
                self.action_with_zoomed_cwd(|state, working_dir| state.open_diffnav(&working_dir))
            }
            KeyCode::Char('D') => self.action_with_zoomed_cwd(|state, working_dir| {
                state.open_tui_tool("gh", "gh dash", &working_dir);
            }),
            KeyCode::Char('r') => self.action_rename_zoomed(),
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

    /// Global keys that work in both compact and zoomed views (`/`, `q`,
    /// `Esc`, digit-zooms).
    fn dispatch_global(&mut self, event: KeyEvent) {
        match event.code {
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Filter;
                self.filter_text.clear();
                self.filter_cursor = 0;
                self.selected = 0;
            }
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => self.handle_global_escape(),
            KeyCode::Char(digit @ '1'..='9') => {
                if let Some(index) = digit_index(digit) {
                    self.view_zoom_index = Some(index);
                    self.view_selected_agent = 0;
                }
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
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
            | KeyCode::Modifier(_) => {}
        }
    }

    /// Tiered Esc behaviour: leave zoom → clear filter → quit.
    fn handle_global_escape(&mut self) {
        if self.view_zoomed_room.is_some() {
            self.view_zoomed_room = None;
            self.view_selected_agent = 0;
        } else if self.filter_text.is_empty() {
            self.should_quit = true;
        } else {
            self.filter_text.clear();
            self.selected = 0;
        }
    }
}
