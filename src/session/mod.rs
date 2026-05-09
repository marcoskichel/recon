//! Session discovery, parsing, and status detection.
//!
//! This module is the core of roostr's data model: it joins four sources
//! (tmux panes, `~/.claude/sessions/{PID}.json`, JSONL transcripts under
//! `~/.claude/projects/`, and `tmux capture-pane` output) into a single
//! [`Session`] view used by the TUI and the daemon.
//!
//! See [`discover_sessions`] for the entry point.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

mod discover;
mod git_info;
mod jsonl;
mod live;
mod resume;
mod status;

/// Validate that a CWD path is safe to pass to external commands.
///
/// Returns `true` only when `cwd` is absolute and resolves to an existing
/// directory. Used to gate `tmux new-session -c <cwd>` and similar
/// invocations so we never feed user-controlled relative paths to a
/// subprocess.
#[must_use]
pub fn validate_cwd(working_dir: &str) -> bool {
    let path = Path::new(working_dir);
    path.is_absolute() && path.is_dir()
}

/// Discover sessions by scanning JSONL files, then matching to live tmux
/// panes.
///
/// `prev_sessions` carries the previous parse state per session id, enabling
/// incremental JSONL reads.
#[must_use]
pub fn discover_sessions(prev_sessions: &HashMap<String, Session>) -> Vec<Session> {
    discover::discover_sessions(prev_sessions)
}

#[cfg(test)]
mod cwd_tests {
    use super::validate_cwd;

    /// Relative paths are rejected.
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn validate_cwd_rejects_relative() {
        assert!(!validate_cwd("relative/path"));
    }

    /// Nonexistent absolute paths are rejected.
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn validate_cwd_rejects_nonexistent() {
        assert!(!validate_cwd("/nonexistent/path/that/does/not/exist"));
    }

    /// `/tmp` (which exists in unit-test environments) is accepted.
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn validate_cwd_accepts_real_dir() {
        assert!(validate_cwd("/tmp"));
    }
}

/// High-level state of a Claude session, derived from tmux pane content
/// and JSONL token counters.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionStatus {
    /// No tokens have been recorded yet — likely brand-new session.
    New,
    /// Claude is actively producing output (spinner visible in pane).
    Working,
    /// Claude is awaiting user confirmation (permission prompt).
    Input,
    /// Conversation idle — last activity is older than the active window.
    Idle,
}

impl SessionStatus {
    /// Short human-readable label for the status (TUI display).
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match *self {
            Self::New => "New",
            Self::Working => "Working",
            Self::Idle => "Idle",
            Self::Input => "Input",
        }
    }
}

/// A single Claude session, joined from JSONL + tmux + session files.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    /// JSONL session id (filename stem of the `*.jsonl` file).
    #[serde(rename = "session_id")]
    pub id: String,
    /// Display name of the project (git repo root or CWD basename).
    pub project_name: String,
    /// Current git branch, if any.
    pub branch: Option<String>,
    /// Working directory of the Claude process.
    pub cwd: String,
    /// Path of CWD relative to the git repo root, if applicable.
    pub relative_dir: Option<String>,
    /// tmux session name hosting the Claude pane.
    #[serde(rename = "tmux_session")]
    pub tmux_name: Option<String>,
    /// `session:window.pane` target for the Claude pane.
    pub pane_target: Option<String>,
    /// Currently selected model id, if known.
    pub model: Option<String>,
    /// Cumulative input tokens (including cache reads/writes).
    pub total_input_tokens: u64,
    /// Cumulative output tokens.
    pub total_output_tokens: u64,
    /// Derived status from pane + token counts.
    pub status: SessionStatus,
    /// PID of the Claude process, if discovered via `~/.claude/sessions/`.
    pub pid: Option<i32>,
    /// Reasoning effort label parsed from `/model` slash command output.
    pub effort: Option<String>,
    /// ISO-8601 timestamp of the most recent JSONL entry.
    pub last_activity: Option<String>,
    /// Unix epoch seconds when the session was started (from session file).
    pub started_at: u64,
    /// Path to the JSONL file backing the session.
    pub jsonl_path: PathBuf,
    /// Last observed file size for incremental parsing.
    pub last_file_size: u64,
    /// Most recent substantive user prompt.
    pub last_user_prompt: Option<String>,
}

impl Session {
    /// Stable id for grouping sessions into rooms in the UI.
    ///
    /// Prefers the tmux session name; falls back to project + relative dir.
    #[must_use]
    pub fn room_id(&self) -> String {
        if let Some(name) = self.tmux_name.as_ref() {
            return name.clone();
        }
        self.relative_dir.as_ref().map_or_else(
            || self.project_name.clone(),
            |suffix| format!("{} \u{203A} {}", self.project_name, suffix),
        )
    }
}
