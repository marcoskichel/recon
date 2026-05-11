//! Action helpers triggered by key bindings — attach, kill, spawn, open
//! external TUIs, and so on.

use super::{
    keys::{binary_in_path, file_name_or, KeyOutcome},
    Application,
};
use crate::tmux;

impl Application {
    /// Attach to the compact-view selection.
    pub(super) fn action_attach_compact(&mut self) -> KeyOutcome {
        let pane_opt = self.selected_compact_session().and_then(|sess| sess.pane_target.clone());
        if let Some(pane) = pane_opt {
            tmux::switch_to_pane(&pane);
            self.should_quit = true;
        }
        KeyOutcome::Handled
    }

    /// Kill the compact-view selection.
    pub(super) fn action_kill_compact(&mut self) -> KeyOutcome {
        let name_opt = self.selected_compact_session().and_then(|sess| sess.tmux_name.clone());
        if let Some(name) = name_opt {
            tmux::kill_session(&name);
            self.refresh();
        }
        KeyOutcome::Handled
    }

    /// Run an action that needs the working directory of the compact-view
    /// selection.
    pub(super) fn action_with_compact_cwd<F>(&mut self, mut action: F) -> KeyOutcome
    where
        F: FnMut(&mut Self, String),
    {
        if let Some(working_dir) = self.selected_compact_cwd() {
            action(self, working_dir);
        }
        KeyOutcome::Handled
    }

    /// Attach to the zoomed-view selection.
    pub(super) fn action_attach_zoomed(&mut self) -> KeyOutcome {
        let pane_opt = self.selected_zoomed_session().and_then(|sess| sess.pane_target.clone());
        if let Some(pane) = pane_opt {
            tmux::switch_to_pane(&pane);
            self.should_quit = true;
        }
        KeyOutcome::Handled
    }

    /// Kill the zoomed-view selection.
    pub(super) fn action_kill_zoomed(&mut self) -> KeyOutcome {
        let name_opt = self.selected_zoomed_session().and_then(|sess| sess.tmux_name.clone());
        if let Some(name) = name_opt {
            tmux::kill_session(&name);
            self.refresh();
        }
        KeyOutcome::Handled
    }

    /// Run an action that needs the working directory of the zoomed-view
    /// selection.
    pub(super) fn action_with_zoomed_cwd<F>(&mut self, mut action: F) -> KeyOutcome
    where
        F: FnMut(&mut Self, String),
    {
        if let Some(working_dir) = self.zoomed_room_cwd() {
            action(self, working_dir);
        }
        KeyOutcome::Handled
    }

    /// Begin renaming the compact-view selection.
    pub(super) fn action_rename_compact(&mut self) -> KeyOutcome {
        let session_id_opt = self.selected_compact_session().map(|sess| sess.id.clone());
        if let Some(session_id) = session_id_opt {
            self.start_rename(session_id);
        }
        KeyOutcome::Handled
    }

    /// Begin renaming the zoomed-view selection.
    pub(super) fn action_rename_zoomed(&mut self) -> KeyOutcome {
        let session_id_opt = self.selected_zoomed_session().map(|sess| sess.id.clone());
        if let Some(session_id) = session_id_opt {
            self.start_rename(session_id);
        }
        KeyOutcome::Handled
    }

    /// Spawn a fresh `claude` session in `working_dir`.
    pub(super) fn spawn_claude(&mut self, working_dir: &str) {
        let default_name = file_name_or(working_dir, "claude");
        if let Ok(name) = tmux::create_session(&default_name, working_dir, None, &[]) {
            tmux::switch_to_pane(&name);
            self.should_quit = true;
        }
    }

    /// Spawn the user's `$EDITOR` against `working_dir`.
    pub(super) fn spawn_editor(&mut self, working_dir: &str) {
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
        let session_name = file_name_or(working_dir, "editor");
        let command = format!("{editor} .");
        if let Ok(name) = tmux::create_session(&session_name, working_dir, Some(&command), &[]) {
            tmux::switch_to_pane(&name);
            self.should_quit = true;
        }
    }

    /// Spawn the user's `$SHELL` in `working_dir`.
    pub(super) fn spawn_terminal(&mut self, working_dir: &str) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
        let session_name = file_name_or(working_dir, "terminal");
        if let Ok(name) = tmux::create_session(&session_name, working_dir, Some(&shell), &[]) {
            tmux::switch_to_pane(&name);
            self.should_quit = true;
        }
    }

    /// Open a TUI tool (`lazygit`, `gh dash`, …) in a new tmux session.
    pub(super) fn open_tui_tool(&mut self, binary: &str, command: &str, working_dir: &str) {
        if !binary_in_path(binary) {
            return;
        }
        let label = file_name_or(working_dir, binary);
        if let Ok(name) = tmux::create_session(&label, working_dir, Some(command), &[]) {
            tmux::switch_to_pane(&name);
            self.should_quit = true;
        }
    }

    /// Open `diffnav` against `git diff HEAD`, or post a status message if
    /// there is no diff.
    pub(super) fn open_diffnav(&mut self, working_dir: &str) {
        if !binary_in_path("diffnav") {
            return;
        }
        let no_diff = std::process::Command::new("git")
            .args(["-C", working_dir, "diff", "HEAD", "--quiet"])
            .status()
            .map_or(true, |status| status.success());
        if no_diff {
            self.set_status("No diff");
            return;
        }
        let label = file_name_or(working_dir, "diffnav");
        if let Ok(name) = tmux::create_session_shell(&label, working_dir, "git diff HEAD | diffnav")
        {
            tmux::switch_to_pane(&name);
            self.should_quit = true;
        }
    }
}
