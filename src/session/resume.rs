//! Resume detection — finds the original JSONL for a `claude --resume`
//! session.
//!
//! `claude --resume <orig-id>` writes a *new* session-id to its session file
//! but continues appending to the *original* JSONL (named after the old
//! session-id). To reunite display state with the right transcript we have
//! to recover that original id.

use std::{fs::read_dir, path::PathBuf, process::Command};

/// For a resumed session, find the original JSONL by locating the session-id
/// that `claude --resume` was called with.
///
/// Strategy (in order):
///  1. Read `ROOSTR_RESUMED_FROM` from the tmux session environment — set by
///     `roostr --resume` at session creation time. Reliable, zero-overhead.
///  2. Fall back to parsing `ps` args for sessions started outside roostr
///     (e.g. the user ran `claude --resume <id>` in a tmux session manually).
pub fn find_jsonl_for_resumed_session(tmux_session: &str, claude_pid: i32) -> Option<PathBuf> {
    let original_id = read_tmux_env(tmux_session, "ROOSTR_RESUMED_FROM")
        .or_else(|| parse_resume_id_from_ps(claude_pid))?;

    find_jsonl_by_session_id(&original_id)
}

/// Read a variable from a tmux session's environment table.
fn read_tmux_env(session_name: &str, env_var: &str) -> Option<String> {
    let output = Command::new("tmux")
        .args(["show-environment", "-t", session_name, env_var])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout);
    line.trim().split_once('=').map(|(_, value)| value.to_owned())
}

/// Parse `--resume <session-id>` from the process command line via `ps`.
/// Fallback for sessions not created by `roostr --resume`.
fn parse_resume_id_from_ps(claude_pid: i32) -> Option<String> {
    let output =
        Command::new("ps").args(["-p", &claude_pid.to_string(), "-o", "args="]).output().ok()?;

    let stdout_text = String::from_utf8_lossy(&output.stdout);
    stdout_text
        .split_whitespace()
        .skip_while(|&token| token != "--resume")
        .nth(1)
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
}

/// Find the JSONL file for a given session id.
///
/// Scans all project directories under `~/.claude/projects/` and returns
/// the largest match if multiple project dirs contain the same session id
/// (handles worktrees that re-use the same id).
pub fn find_jsonl_by_session_id(session_id: &str) -> Option<PathBuf> {
    let projects_dir = dirs::home_dir()?.join(".claude").join("projects");
    let mut best: Option<(PathBuf, u64)> = None;
    for entry in read_dir(&projects_dir).ok()?.flatten() {
        let candidate = entry.path().join(format!("{session_id}.jsonl"));
        if !candidate.exists() {
            continue;
        }
        let size = candidate.metadata().map_or(0, |meta| meta.len());
        let take = best.as_ref().is_none_or(|prev| size > prev.1);
        if take {
            best = Some((candidate, size));
        }
    }
    best.map(|(path, _)| path)
}
