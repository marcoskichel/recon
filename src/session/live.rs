//! Live session discovery — joins tmux panes with `~/.claude/sessions/`.

use std::{
    collections::{HashMap, HashSet},
    fs::{read_dir, read_to_string},
    process::Command,
};

/// Info about a live Claude session, built from tmux + session files.
pub struct LiveSessionInfo {
    /// PID of the Claude process.
    pub claude_pid: i32,
    /// tmux session name hosting the Claude pane.
    pub tmux_session: String,
    /// Fully-qualified `session:window.pane` target.
    pub pane_target: String,
    /// CWD recorded by tmux for the pane.
    pub pane_cwd: String,
    /// Unix epoch seconds of session start.
    pub started_at: u64,
}

/// Build a map from JSONL `session_id` → live session info.
///
/// Joins two sources:
///   1. tmux list-panes: PID → (`tmux_session`, `pane_cwd`) for panes running
///      claude.
///   2. `~/.claude/sessions/{PID}.json`: PID → (`session_id`, `started_at`).
pub fn build_live_session_map() -> HashMap<String, LiveSessionInfo> {
    let pid_session_map = read_pid_session_map();
    let tmux_panes = discover_claude_tmux_panes();

    let mut joined: HashMap<String, LiveSessionInfo> = HashMap::new();
    for pane in tmux_panes {
        if let Some(info) = pid_session_map.get(&pane.claude_pid) {
            joined.insert(
                info.session_id.clone(),
                LiveSessionInfo {
                    claude_pid: pane.claude_pid,
                    tmux_session: pane.tmux_session,
                    pane_target: pane.pane_target,
                    pane_cwd: pane.pane_cwd,
                    started_at: info.started_at,
                },
            );
        } else {
            // Tmux pane running claude but no session file yet (just started).
            // Use pane_target (not tmux session name) as placeholder key so
            // that two Claude panes in the same tmux session don't collide.
            let placeholder_key = format!("tmux-{}", pane.pane_target);
            joined.insert(
                placeholder_key,
                LiveSessionInfo {
                    claude_pid: pane.claude_pid,
                    tmux_session: pane.tmux_session,
                    pane_target: pane.pane_target,
                    pane_cwd: pane.pane_cwd,
                    started_at: 0,
                },
            );
        }
    }
    joined
}

/// One row of `tmux list-panes` data after we've identified it as Claude.
struct ClaudePane {
    claude_pid: i32,
    tmux_session: String,
    pane_target: String,
    pane_cwd: String,
}

/// Per-PID metadata read from `~/.claude/sessions/{PID}.json`.
struct SessionFileInfo {
    session_id: String,
    started_at: u64,
}

/// Read `~/.claude/sessions/{PID}.json` files to build a PID → session info
/// map.
fn read_pid_session_map() -> HashMap<i32, SessionFileInfo> {
    let Some(home) = dirs::home_dir() else {
        return HashMap::new();
    };
    let sessions_dir = home.join(".claude").join("sessions");
    let Ok(entries) = read_dir(&sessions_dir) else {
        return HashMap::new();
    };

    let mut tree: HashMap<i32, SessionFileInfo> = HashMap::new();
    for entry in entries.flatten() {
        if let Some((claude_pid, info)) = parse_session_file(&entry.path()) {
            tree.insert(claude_pid, info);
        }
    }
    tree
}

/// Parse a single `~/.claude/sessions/{PID}.json` file.
fn parse_session_file(path: &std::path::Path) -> Option<(i32, SessionFileInfo)> {
    if path.extension().is_none_or(|stem| stem != "json") {
        return None;
    }
    let content = read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&content).ok()?;
    let pid_i64 = value.get("pid").and_then(serde_json::Value::as_i64)?;
    let session_id = value.get("sessionId").and_then(serde_json::Value::as_str)?;
    let pid_i32 = i32::try_from(pid_i64).ok()?;
    let started_at = value
        .get("startedAt")
        .and_then(serde_json::Value::as_u64)
        .map_or(0, |millis| millis.saturating_div(1000));
    Some((pid_i32, SessionFileInfo { session_id: session_id.to_owned(), started_at }))
}

/// Get tmux panes running claude.
///
/// Performance: builds a single `ppid → children` map from one
/// `ps -eo pid,ppid` call, avoiding per-pane `pgrep` spawns.
fn discover_claude_tmux_panes() -> Vec<ClaudePane> {
    let output = match Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_pid}|||#{session_name}|||#{pane_current_command}|||#{pane_current_path}|||#{window_index}|||#{pane_index}",
        ])
        .output()
    {
        Ok(success) if success.status.success() => success,
        Ok(_) | Err(_) => return vec![],
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    let session_pids = read_session_pids();
    let children_map = ProcessChildren::load();

    for line in stdout.lines() {
        if let Some(pane) = parse_pane_line(line, &session_pids, &children_map) {
            results.push(pane);
        }
    }

    results
}

/// Parse a single tmux `list-panes` row, returning a [`ClaudePane`] if the
/// pane is hosting a Claude process.
fn parse_pane_line(
    line: &str,
    session_pids: &HashSet<i32>,
    children_map: &ProcessChildren,
) -> Option<ClaudePane> {
    let parts: Vec<&str> = line.splitn(6, "|||").collect();
    let raw_pid = parts.first()?;
    let session_name = *parts.get(1)?;
    let command = *parts.get(2)?;
    let pane_path = *parts.get(3)?;
    let window_index = *parts.get(4)?;
    let pane_index = *parts.get(5)?;

    let pane_pid: i32 = raw_pid.parse().ok()?;

    let claude_pid = if is_claude_command(command) {
        // pane_pid may be claude itself (roostr launch) or a shell with
        // claude as the foreground child (manual `claude` in a terminal).
        // Try pane_pid first, fall back to its descendants.
        if session_pids.contains(&pane_pid) {
            Some(pane_pid)
        } else {
            children_map.find_descendant_in(pane_pid, session_pids)
        }
    } else if matches!(command, "bash" | "sh" | "zsh") {
        children_map.find_descendant_in(pane_pid, session_pids)
    } else {
        None
    }?;

    let pane_target = format!("{session_name}:{window_index}.{pane_index}");
    Some(ClaudePane {
        claude_pid,
        tmux_session: session_name.to_owned(),
        pane_target,
        pane_cwd: pane_path.to_owned(),
    })
}

/// Recognize `tmux pane_current_command` values that mean Claude.
///
/// Claude shows up as a version number (e.g. `2.1.76`) or `claude` or `node`.
/// On macOS the npm-distributed binary's internal process name is
/// `claude.exe` (a bundler convention, not a Windows artifact).
fn is_claude_command(command: &str) -> bool {
    let starts_with_digit = command.chars().next().is_some_and(|first| first.is_ascii_digit());
    starts_with_digit || command == "claude" || command == "claude.exe" || command == "node"
}

/// Enumerate PIDs that have a `~/.claude/sessions/{PID}.json` file.
fn read_session_pids() -> HashSet<i32> {
    let Some(home) = dirs::home_dir() else {
        return HashSet::new();
    };
    let sessions_dir = home.join(".claude").join("sessions");
    let Ok(entries) = read_dir(&sessions_dir) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_some_and(|stem| stem == "json") {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .and_then(|stem| stem.parse::<i32>().ok())
            } else {
                None
            }
        })
        .collect()
}

/// Process tree built from a single `ps` call.
struct ProcessChildren {
    children_by_parent: HashMap<i32, Vec<i32>>,
}

impl ProcessChildren {
    /// Read `ps -eo pid=,ppid=` once and bucket children by parent.
    fn load() -> Self {
        let output = match Command::new("ps").args(["-eo", "pid=,ppid="]).output() {
            Ok(success) if success.status.success() => success,
            Ok(_) | Err(_) => {
                return Self { children_by_parent: HashMap::new() };
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut tree: HashMap<i32, Vec<i32>> = HashMap::new();
        for line in stdout.lines() {
            let mut parts = line.split_whitespace();
            let child_pid: Option<i32> = parts.next().and_then(|token| token.parse().ok());
            let parent_pid: Option<i32> = parts.next().and_then(|token| token.parse().ok());
            if let (Some(child), Some(parent)) = (child_pid, parent_pid) {
                tree.entry(parent).or_default().push(child);
            }
        }
        Self { children_by_parent: tree }
    }

    /// BFS from `parent` looking for any descendant whose PID is in
    /// `target_set`.
    fn find_descendant_in(&self, parent: i32, target_set: &HashSet<i32>) -> Option<i32> {
        let mut stack = vec![parent];
        let mut seen: HashSet<i32> = HashSet::new();
        while let Some(current) = stack.pop() {
            if !seen.insert(current) {
                continue;
            }
            if let Some(children) = self.children_by_parent.get(&current) {
                for &child in children {
                    if target_set.contains(&child) {
                        return Some(child);
                    }
                    stack.push(child);
                }
            }
        }
        None
    }
}
