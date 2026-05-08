use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const STATUS_MESSAGE_TTL: Duration = Duration::from_secs(3);

use crate::session::{self, Session};
use crate::summarizer::Summarizer;
use crate::tmux;
use crate::view_ui;

pub struct App {
    pub sessions: Vec<Session>,
    pub selected: usize,
    pub should_quit: bool,
    pub tick: u64,
    pub view_zoomed_room: Option<String>,
    pub view_zoom_index: Option<usize>,
    pub view_selected_agent: usize,
    pub filter_active: bool,
    pub filter_text: String,
    pub filter_cursor: usize,
    pub view_chars_per_row: Cell<usize>,
    pub view_room_order: Vec<String>,
    pub summarizer: Summarizer,
    pub status_message: Option<(String, Instant)>,
    pub species_assignments: HashMap<String, usize>,
    pub custom_names: HashMap<String, String>,
    pub rename_active: bool,
    pub rename_session_id: Option<String>,
    pub rename_text: String,
    pub rename_cursor: usize,
    prev_sessions: HashMap<String, Session>,
}

impl App {
    pub fn new() -> Self {
        Self::with_summarizer(Summarizer::start())
    }

    pub fn new_blocking() -> Self {
        Self::with_summarizer(Summarizer::start_blocking())
    }

    fn with_summarizer(summarizer: Summarizer) -> Self {
        App {
            sessions: Vec::new(),
            selected: 0,
            should_quit: false,
            tick: 0,
            view_zoomed_room: None,
            view_zoom_index: None,
            view_selected_agent: 0,
            filter_active: false,
            filter_text: String::new(),
            filter_cursor: 0,
            view_chars_per_row: Cell::new(1),
            view_room_order: Vec::new(),
            summarizer,
            status_message: None,
            species_assignments: HashMap::new(),
            custom_names: HashMap::new(),
            rename_active: false,
            rename_session_id: None,
            rename_text: String::new(),
            rename_cursor: 0,
            prev_sessions: HashMap::new(),
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    pub fn active_status_message(&self) -> Option<&str> {
        self.status_message.as_ref().and_then(|(msg, ts)| {
            if ts.elapsed() < STATUS_MESSAGE_TTL { Some(msg.as_str()) } else { None }
        })
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
        for s in &sessions {
            if !s.jsonl_path.as_os_str().is_empty() {
                self.summarizer
                    .maybe_enqueue(&s.session_id, &s.jsonl_path, s.last_file_size);
            }
        }

        self.sessions = sessions;
        self.assign_species_to_new_sessions();

        let count = self.filtered_indices().len();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }

    fn assign_species_to_new_sessions(&mut self) {
        use std::collections::HashSet;
        let active_ids: HashSet<String> =
            self.sessions.iter().map(|s| s.session_id.clone()).collect();
        self.species_assignments.retain(|id, _| active_ids.contains(id));

        let used: HashSet<usize> = self.species_assignments.values().copied().collect();
        let mut available: Vec<usize> = (0..view_ui::SPECIES_COUNT)
            .filter(|s| !used.contains(s))
            .collect();

        for session in &self.sessions {
            if !self.species_assignments.contains_key(&session.session_id) {
                let species = if !available.is_empty() {
                    available.remove(0)
                } else {
                    view_ui::pick_species(&session.session_id)
                };
                self.species_assignments.insert(session.session_id.clone(), species);
            }
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
        if self.rename_active {
            self.handle_key_rename(key);
            return;
        }
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
                KeyCode::Char('e') => {
                    if let Some(cwd) = self.selected_compact_cwd() {
                        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                        let session_name = std::path::Path::new(&cwd)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "editor".to_string());
                        let cmd = format!("{editor} .");
                        if let Ok(name) = tmux::create_session(&session_name, &cwd, Some(&cmd), &[]) {
                            tmux::switch_to_pane(&name);
                            self.should_quit = true;
                        }
                    }
                    return;
                }
                KeyCode::Char('t') => {
                    if let Some(cwd) = self.selected_compact_cwd() {
                        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
                        let session_name = std::path::Path::new(&cwd)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "terminal".to_string());
                        if let Ok(name) = tmux::create_session(&session_name, &cwd, Some(&shell), &[]) {
                            tmux::switch_to_pane(&name);
                            self.should_quit = true;
                        }
                    }
                    return;
                }
                KeyCode::Char('g') => {
                    if let Some(cwd) = self.selected_compact_cwd() {
                        self.open_tui_tool("lazygit", "lazygit", &cwd);
                    }
                    return;
                }
                KeyCode::Char('d') => {
                    if let Some(cwd) = self.selected_compact_cwd() {
                        self.open_diffnav(&cwd);
                    }
                    return;
                }
                KeyCode::Char('D') => {
                    if let Some(cwd) = self.selected_compact_cwd() {
                        self.open_tui_tool("gh", "gh dash", &cwd);
                    }
                    return;
                }
                KeyCode::Char('r') => {
                    let sid = self.selected_compact_session().map(|s| s.session_id.clone());
                    if let Some(sid) = sid {
                        self.start_rename(sid);
                    }
                    return;
                }
                KeyCode::Char(c @ '1'..='9') => {
                    let idx = (c as usize) - ('1' as usize);
                    if idx < total {
                        self.view_selected_agent = idx;
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
                KeyCode::Char('e') => {
                    if let Some(cwd) = self.zoomed_room_cwd() {
                        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                        let session_name = std::path::Path::new(&cwd)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "editor".to_string());
                        let cmd = format!("{editor} .");
                        if let Ok(name) = tmux::create_session(&session_name, &cwd, Some(&cmd), &[]) {
                            tmux::switch_to_pane(&name);
                            self.should_quit = true;
                        }
                    }
                    return;
                }
                KeyCode::Char('t') => {
                    if let Some(cwd) = self.zoomed_room_cwd() {
                        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
                        let session_name = std::path::Path::new(&cwd)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "terminal".to_string());
                        if let Ok(name) = tmux::create_session(&session_name, &cwd, Some(&shell), &[]) {
                            tmux::switch_to_pane(&name);
                            self.should_quit = true;
                        }
                    }
                    return;
                }
                KeyCode::Char('g') => {
                    if let Some(cwd) = self.zoomed_room_cwd() {
                        self.open_tui_tool("lazygit", "lazygit", &cwd);
                    }
                    return;
                }
                KeyCode::Char('d') => {
                    if let Some(cwd) = self.zoomed_room_cwd() {
                        self.open_diffnav(&cwd);
                    }
                    return;
                }
                KeyCode::Char('D') => {
                    if let Some(cwd) = self.zoomed_room_cwd() {
                        self.open_tui_tool("gh", "gh dash", &cwd);
                    }
                    return;
                }
                KeyCode::Char('r') => {
                    let sid = self.selected_zoomed_session().map(|s| s.session_id.clone());
                    if let Some(sid) = sid {
                        self.start_rename(sid);
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

    fn start_rename(&mut self, session_id: String) {
        let current = self.custom_names.get(&session_id).cloned().unwrap_or_else(|| {
            let species = self.species_assignments.get(&session_id).copied()
                .unwrap_or_else(|| view_ui::pick_species(&session_id));
            view_ui::SPECIES_NAMES[species % view_ui::SPECIES_COUNT].to_string()
        });
        self.rename_text = current;
        self.rename_cursor = self.rename_text.chars().count();
        self.rename_session_id = Some(session_id);
        self.rename_active = true;
    }

    fn handle_key_rename(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.rename_active = false;
                self.rename_session_id = None;
                self.rename_text.clear();
                self.rename_cursor = 0;
            }
            KeyCode::Enter => {
                if let Some(sid) = self.rename_session_id.take() {
                    if self.rename_text.is_empty() {
                        self.custom_names.remove(&sid);
                    } else {
                        self.custom_names.insert(sid, self.rename_text.clone());
                    }
                }
                self.rename_active = false;
                self.rename_text.clear();
                self.rename_cursor = 0;
            }
            KeyCode::Backspace => {
                if self.rename_cursor > 0 {
                    let byte_pos = self.rename_text.char_indices()
                        .nth(self.rename_cursor - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    let next_byte = self.rename_text.char_indices()
                        .nth(self.rename_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.rename_text.len());
                    self.rename_text.replace_range(byte_pos..next_byte, "");
                    self.rename_cursor -= 1;
                }
            }
            KeyCode::Delete => {
                let char_count = self.rename_text.chars().count();
                if self.rename_cursor < char_count {
                    let byte_pos = self.rename_text.char_indices()
                        .nth(self.rename_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.rename_text.len());
                    let next_byte = self.rename_text.char_indices()
                        .nth(self.rename_cursor + 1)
                        .map(|(i, _)| i)
                        .unwrap_or(self.rename_text.len());
                    self.rename_text.replace_range(byte_pos..next_byte, "");
                }
            }
            KeyCode::Left => {
                if self.rename_cursor > 0 {
                    self.rename_cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.rename_cursor < self.rename_text.chars().count() {
                    self.rename_cursor += 1;
                }
            }
            KeyCode::Home => self.rename_cursor = 0,
            KeyCode::End => self.rename_cursor = self.rename_text.chars().count(),
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.rename_cursor = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.rename_cursor = self.rename_text.chars().count();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.rename_text.clear();
                self.rename_cursor = 0;
            }
            KeyCode::Char(c) => {
                let byte_pos = self.rename_text.char_indices()
                    .nth(self.rename_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.rename_text.len());
                self.rename_text.insert(byte_pos, c);
                self.rename_cursor += 1;
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

    fn open_tui_tool(&mut self, binary: &str, command: &str, cwd: &str) {
        if !binary_in_path(binary) {
            return;
        }
        let label = std::path::Path::new(cwd)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| binary.to_string());
        if let Ok(name) = tmux::create_session(&label, cwd, Some(command), &[]) {
            tmux::switch_to_pane(&name);
            self.should_quit = true;
        }
    }

    fn open_diffnav(&mut self, cwd: &str) {
        if !binary_in_path("diffnav") {
            return;
        }
        let no_diff = std::process::Command::new("git")
            .args(["-C", cwd, "diff", "HEAD", "--quiet"])
            .status()
            .map(|s| s.success())
            .unwrap_or(true);
        if no_diff {
            self.set_status("No diff");
            return;
        }
        let label = std::path::Path::new(cwd)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "diffnav".to_string());
        if let Ok(name) = tmux::create_session_shell(&label, cwd, "git diff HEAD | diffnav") {
            tmux::switch_to_pane(&name);
            self.should_quit = true;
        }
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

fn binary_in_path(name: &str) -> bool {
    std::env::var("PATH").ok().map(|path| {
        path.split(':').any(|dir| std::path::Path::new(dir).join(name).is_file())
    }).unwrap_or(false)
}
