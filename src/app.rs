use std::cell::Cell;
use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::session::{self, Session};
use crate::tmux;
use crate::view_ui;

pub struct App {
    pub sessions: Vec<Session>,
    pub selected: usize,
    pub should_quit: bool,
    pub tick: u64,
    pub view_page: usize,
    pub view_zoomed_room: Option<String>,
    pub view_zoom_index: Option<usize>,
    pub view_selected_agent: usize,
    pub filter_active: bool,
    pub filter_text: String,
    pub filter_cursor: usize,
    pub view_chars_per_row: Cell<usize>,
    pub view_room_order: Vec<String>,
    prev_sessions: HashMap<String, Session>,
}

impl App {
    pub fn new() -> Self {
        App {
            sessions: Vec::new(),
            selected: 0,
            should_quit: false,
            tick: 0,
            view_page: 0,
            view_zoomed_room: None,
            view_zoom_index: None,
            view_selected_agent: 0,
            filter_active: false,
            filter_text: String::new(),
            filter_cursor: 0,
            view_chars_per_row: Cell::new(1),
            view_room_order: Vec::new(),
            prev_sessions: HashMap::new(),
        }
    }

    pub fn refresh(&mut self) {
        let raw = session::discover_sessions(&self.prev_sessions);
        let sessions: Vec<Session> = raw
            .into_iter()
            .filter(|s| s.tmux_session.is_some())
            .collect();

        self.prev_sessions = sessions
            .iter()
            .map(|s| (s.session_id.clone(), s.clone()))
            .collect();

        self.apply_snapshot(sessions);
    }

    pub fn apply_snapshot(&mut self, sessions: Vec<Session>) {
        self.sessions = sessions;

        let count = self.filtered_indices().len();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }

    pub fn snapshot_prev(&self) -> HashMap<String, Session> {
        self.sessions
            .iter()
            .map(|s| (s.session_id.clone(), s.clone()))
            .collect()
    }

    pub fn advance_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.filter_text.is_empty() {
            return (0..self.sessions.len()).collect();
        }
        let query = self.filter_text.to_lowercase();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.project_name.to_lowercase().contains(&query)
                    || s.tmux_session
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn clamp_selection(&mut self) {
        let count = self.filtered_indices().len();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.filter_active {
            self.handle_key_filter(key);
            return;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::Char('i')) {
            self.jump_to_next_input();
            return;
        }
        self.handle_key_view(key);
    }

    fn jump_to_next_input(&mut self) {
        if let Some(session) = self.sessions.iter().find(|s| s.status == session::SessionStatus::Input) {
            if let Some(target) = &session.pane_target {
                tmux::switch_to_pane(target);
                self.should_quit = true;
            }
        }
    }

    fn handle_key_view(&mut self, key: KeyEvent) {
        let total = self.compact_flat_session_indices().len();
        if total > 0 && self.view_zoomed_room.is_none() {
            match key.code {
                KeyCode::Char('l') | KeyCode::Right => {
                    self.view_selected_agent = (self.view_selected_agent + 1).min(total - 1);
                    return;
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    self.view_selected_agent = self.view_selected_agent.saturating_sub(1);
                    return;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.view_selected_agent = self.compact_grid_move_down();
                    return;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.view_selected_agent = self.compact_grid_move_up();
                    return;
                }
                KeyCode::Enter => {
                    if let Some(session) = self.selected_compact_session() {
                        if let Some(target) = session.pane_target.clone() {
                            tmux::switch_to_pane(&target);
                            self.should_quit = true;
                        }
                    }
                    return;
                }
                KeyCode::Char('x') => {
                    if let Some(session) = self.selected_compact_session() {
                        if let Some(name) = session.tmux_session.clone() {
                            tmux::kill_session(&name);
                            self.refresh();
                        }
                    }
                    return;
                }
                KeyCode::Char('n') => {
                    if let Some(cwd) = self.selected_compact_cwd() {
                        let default_name = std::path::Path::new(&cwd)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "claude".to_string());
                        if let Ok(name) = tmux::create_session(&default_name, &cwd, None, &[]) {
                            tmux::switch_to_pane(&name);
                            self.should_quit = true;
                        }
                    }
                    return;
                }
                KeyCode::Char(c @ '1'..='9') => {
                    let idx = (c as usize) - ('1' as usize);
                    if idx < total {
                        self.view_selected_agent = idx;
                        if let Some(session) = self.selected_compact_session() {
                            if let Some(target) = session.pane_target.clone() {
                                tmux::switch_to_pane(&target);
                                self.should_quit = true;
                            }
                        }
                    }
                    return;
                }
                _ => {}
            }
        }

        if self.view_zoomed_room.is_some() {
            match key.code {
                KeyCode::Char('l') | KeyCode::Right => {
                    self.view_selected_agent = self.view_selected_agent.saturating_add(1);
                    return;
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    self.view_selected_agent = self.view_selected_agent.saturating_sub(1);
                    return;
                }
                KeyCode::Enter => {
                    if let Some(session) = self.selected_zoomed_session() {
                        if let Some(target) = session.pane_target.clone() {
                            tmux::switch_to_pane(&target);
                            self.should_quit = true;
                        }
                    }
                    return;
                }
                KeyCode::Char('x') => {
                    if let Some(session) = self.selected_zoomed_session() {
                        if let Some(name) = session.tmux_session.clone() {
                            tmux::kill_session(&name);
                            self.refresh();
                        }
                    }
                    return;
                }
                KeyCode::Char('n') => {
                    if let Some(cwd) = self.zoomed_room_cwd() {
                        let default_name = std::path::Path::new(&cwd)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "claude".to_string());
                        if let Ok(name) = tmux::create_session(&default_name, &cwd, None, &[]) {
                            tmux::switch_to_pane(&name);
                            self.should_quit = true;
                        }
                    }
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('/') => {
                self.filter_active = true;
                self.filter_text.clear();
                self.filter_cursor = 0;
                self.selected = 0;
            }
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => {
                if self.view_zoomed_room.is_some() {
                    self.view_zoomed_room = None;
                    self.view_selected_agent = 0;
                } else if !self.filter_text.is_empty() {
                    self.filter_text.clear();
                    self.selected = 0;
                } else {
                    self.should_quit = true;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.view_page = self.view_page.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.view_page = self.view_page.saturating_sub(1);
            }
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as usize) - ('1' as usize);
                self.view_zoom_index = Some(idx);
                self.view_selected_agent = 0;
            }
            _ => {}
        }
    }

    fn handle_key_filter(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.filter_active = false;
                self.filter_text.clear();
                self.filter_cursor = 0;
                self.selected = 0;
            }
            KeyCode::Enter => {
                let indices = self.filtered_indices();
                if indices.len() == 1 {
                    if let Some(session) = self.sessions.get(indices[0]) {
                        if let Some(target) = &session.pane_target {
                            tmux::switch_to_pane(target);
                            self.should_quit = true;
                            return;
                        }
                    }
                }
                self.filter_active = false;
            }
            KeyCode::Backspace => {
                if self.filter_cursor > 0 {
                    let byte_pos = self.filter_text.char_indices()
                        .nth(self.filter_cursor - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    let next_byte = self.filter_text.char_indices()
                        .nth(self.filter_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.filter_text.len());
                    self.filter_text.replace_range(byte_pos..next_byte, "");
                    self.filter_cursor -= 1;
                    self.clamp_selection();
                }
            }
            KeyCode::Delete => {
                let char_count = self.filter_text.chars().count();
                if self.filter_cursor < char_count {
                    let byte_pos = self.filter_text.char_indices()
                        .nth(self.filter_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.filter_text.len());
                    let next_byte = self.filter_text.char_indices()
                        .nth(self.filter_cursor + 1)
                        .map(|(i, _)| i)
                        .unwrap_or(self.filter_text.len());
                    self.filter_text.replace_range(byte_pos..next_byte, "");
                    self.clamp_selection();
                }
            }
            KeyCode::Left => {
                if self.filter_cursor > 0 {
                    self.filter_cursor -= 1;
                }
            }
            KeyCode::Right => {
                let char_count = self.filter_text.chars().count();
                if self.filter_cursor < char_count {
                    self.filter_cursor += 1;
                }
            }
            KeyCode::Home => self.filter_cursor = 0,
            KeyCode::End => self.filter_cursor = self.filter_text.chars().count(),
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_cursor = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_cursor = self.filter_text.chars().count();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_text.clear();
                self.filter_cursor = 0;
                self.clamp_selection();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let count = self.filtered_indices().len();
                if count > 0 {
                    self.selected = (self.selected + 1).min(count - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Tab | KeyCode::Char('i') => {
                self.jump_to_next_input();
            }
            KeyCode::Char(c) => {
                let byte_pos = self.filter_text.char_indices()
                    .nth(self.filter_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.filter_text.len());
                self.filter_text.insert(byte_pos, c);
                self.filter_cursor += 1;
                self.clamp_selection();
            }
            _ => {}
        }
    }

    fn zoomed_room_session_indices(&self) -> Vec<usize> {
        let Some(ref room_name) = self.view_zoomed_room else {
            return Vec::new();
        };
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                let name = if s.project_name.is_empty() {
                    "unknown".to_string()
                } else {
                    s.room_id()
                };
                &name == room_name
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn selected_zoomed_session(&self) -> Option<&Session> {
        let indices = self.zoomed_room_session_indices();
        if indices.is_empty() {
            return None;
        }
        let clamped = self.view_selected_agent.min(indices.len() - 1);
        self.sessions.get(indices[clamped])
    }

    fn zoomed_room_cwd(&self) -> Option<String> {
        self.selected_zoomed_session().map(|s| s.cwd.clone())
    }

    fn compact_flat_session_indices(&self) -> Vec<usize> {
        let filtered = self.filtered_indices();
        let rooms = view_ui::group_into_rooms_stable(&self.sessions, &filtered, &self.view_room_order);
        rooms
            .into_iter()
            .flat_map(|r| r.session_indices.into_iter())
            .collect()
    }

    fn selected_compact_session(&self) -> Option<&Session> {
        let indices = self.compact_flat_session_indices();
        if indices.is_empty() {
            return None;
        }
        let clamped = self.view_selected_agent.min(indices.len() - 1);
        self.sessions.get(indices[clamped])
    }

    fn selected_compact_cwd(&self) -> Option<String> {
        self.selected_compact_session().map(|s| s.cwd.clone())
    }

    fn compact_room_layouts(&self, cpr: usize) -> (Vec<(usize, usize, usize)>, usize) {
        let filtered = self.filtered_indices();
        let rooms = view_ui::group_into_rooms_stable(&self.sessions, &filtered, &self.view_room_order);
        let mut out = Vec::with_capacity(rooms.len());
        let mut base = 0usize;
        for r in &rooms {
            let n = r.session_indices.len();
            let rows = if cpr == 0 { 0 } else { (n + cpr - 1) / cpr };
            out.push((n, rows, base));
            base += n;
        }
        (out, base)
    }

    fn idx_to_pos(g: usize, layouts: &[(usize, usize, usize)], cpr: usize) -> Option<(usize, usize, usize)> {
        for (i, &(n, _rows, base)) in layouts.iter().enumerate() {
            if g < base + n {
                let local = g - base;
                return Some((i, local / cpr, local % cpr));
            }
        }
        None
    }

    fn cells_in_row(n: usize, row: usize, cpr: usize) -> usize {
        let rows = (n + cpr - 1) / cpr;
        if row + 1 == rows {
            let rem = n % cpr;
            if rem == 0 { cpr } else { rem }
        } else {
            cpr
        }
    }

    fn compact_grid_move_down(&self) -> usize {
        let cpr = self.view_chars_per_row.get().max(1);
        let (layouts, _total) = self.compact_room_layouts(cpr);
        let g = self.view_selected_agent;
        let Some((room_i, row, col)) = Self::idx_to_pos(g, &layouts, cpr) else { return g };
        let (n, rows, base) = layouts[room_i];

        if row + 1 < rows {
            let cells = Self::cells_in_row(n, row + 1, cpr);
            let target_col = col.min(cells.saturating_sub(1));
            return base + (row + 1) * cpr + target_col;
        }
        if room_i + 1 < layouts.len() {
            let (next_n, _next_rows, next_base) = layouts[room_i + 1];
            if next_n == 0 { return g; }
            let cells = Self::cells_in_row(next_n, 0, cpr);
            let target_col = col.min(cells.saturating_sub(1));
            return next_base + target_col;
        }
        g
    }

    fn compact_grid_move_up(&self) -> usize {
        let cpr = self.view_chars_per_row.get().max(1);
        let (layouts, _total) = self.compact_room_layouts(cpr);
        let g = self.view_selected_agent;
        let Some((room_i, row, col)) = Self::idx_to_pos(g, &layouts, cpr) else { return g };
        let (_n, _rows, base) = layouts[room_i];

        if row > 0 {
            let cells = Self::cells_in_row(_n, row - 1, cpr);
            let target_col = col.min(cells.saturating_sub(1));
            return base + (row - 1) * cpr + target_col;
        }
        if room_i > 0 {
            let (prev_n, prev_rows, prev_base) = layouts[room_i - 1];
            if prev_n == 0 || prev_rows == 0 { return g; }
            let last_row = prev_rows - 1;
            let cells = Self::cells_in_row(prev_n, last_row, cpr);
            let target_col = col.min(cells.saturating_sub(1));
            return prev_base + last_row * cpr + target_col;
        }
        g
    }
}
