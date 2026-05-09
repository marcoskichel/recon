//! Top-level session discovery loop.
//!
//! Joins JSONL transcripts under `~/.claude/projects/` with the live tmux
//! session map produced by [`super::live::build_live_session_map`].

use std::collections::{HashMap, HashSet};
use std::fs::read_dir;
use std::path::{Path, PathBuf};

use super::git_info::{decode_project_path, git_project_info};
use super::jsonl::{parse_jsonl, ParseInputs, ParsedInfo};
use super::live::{build_live_session_map, LiveSessionInfo};
use super::resume::find_jsonl_for_resumed_session;
use super::status::determine_status;
use super::{Session, SessionStatus};

/// Discover sessions by scanning JSONL files, then matching to live tmux
/// panes.
///
/// `prev_sessions` carries the previous parse state per session id, enabling
/// incremental JSONL reads.
pub fn discover_sessions(prev_sessions: &HashMap<String, Session>) -> Vec<Session> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    let claude_dir = home.join(".claude").join("projects");

    if !claude_dir.exists() {
        return vec![];
    }

    let live_map = build_live_session_map();

    let mut state = DiscoveryState { sessions: Vec::new(), matched_session_ids: HashSet::new() };

    if let Ok(entries) = read_dir(&claude_dir) {
        for entry in entries.flatten() {
            let project_dir = entry.path();
            if !project_dir.is_dir() {
                continue;
            }
            scan_project_dir(&project_dir, &live_map, prev_sessions, &mut state);
        }
    }

    add_unmatched_live_sessions(&live_map, prev_sessions, &mut state);

    sort_sessions(&mut state.sessions);
    state.sessions
}

/// Working state threaded through the discovery loop.
struct DiscoveryState {
    sessions: Vec<Session>,
    matched_session_ids: HashSet<String>,
}

/// Bundle of inputs threaded through scan/replace/build helpers.
///
/// Keeps function signatures within the `too-many-arguments` budget by
/// grouping parameters that always travel together.
struct ScanContext<'borrow> {
    project_dir: &'borrow Path,
    live: &'borrow LiveSessionInfo,
    prev_sessions: &'borrow HashMap<String, Session>,
}

/// Walk one project directory, processing every `*.jsonl` file inside.
fn scan_project_dir(
    project_dir: &Path,
    live_map: &HashMap<String, LiveSessionInfo>,
    prev_sessions: &HashMap<String, Session>,
    state: &mut DiscoveryState,
) {
    let Ok(jsonl_files) = read_dir(project_dir) else {
        return;
    };

    for jentry in jsonl_files.flatten() {
        let path = jentry.path();
        if path.is_dir() {
            continue;
        }
        if path.extension().is_none_or(|stem| stem != "jsonl") {
            continue;
        }

        let session_id =
            path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();

        let Some(live) = live_map.get(&session_id) else {
            continue;
        };

        let context = ScanContext { project_dir, live, prev_sessions };

        if state.matched_session_ids.contains(&session_id) {
            replace_with_larger(&session_id, &path, &context, &mut state.sessions);
            continue;
        }

        let session = build_session_from_jsonl(session_id.clone(), &path, &context);
        state.matched_session_ids.insert(session_id);
        state.sessions.push(session);
    }
}

/// Same `session_id` can appear in multiple project dirs (e.g. session
/// started in one CWD then moved to a worktree). Prefer the larger file.
fn replace_with_larger(
    session_id: &str,
    path: &Path,
    context: &ScanContext,
    sessions: &mut [Session],
) {
    let Some(existing) = sessions.iter_mut().find(|sess| sess.id == session_id) else {
        return;
    };
    let existing_size = existing.jsonl_path.metadata().map_or(0, |meta| meta.len());
    let new_size = path.metadata().map_or(0, |meta| meta.len());
    if new_size <= existing_size {
        return;
    }

    let prev = context.prev_sessions.get(session_id);
    let parsed = parse_with_prev(path, prev);
    let working_dir = resolve_cwd(parsed.working_dir.clone(), prev, context.project_dir);
    let (project_name, relative_dir, branch) = git_project_info(&working_dir);

    existing.project_name = project_name;
    existing.relative_dir = relative_dir;
    existing.branch = branch;
    existing.cwd = working_dir;
    existing.model = parsed.model;
    existing.effort = parsed.effort;
    existing.total_input_tokens = parsed.input_tokens;
    existing.total_output_tokens = parsed.output_tokens;
    existing.last_activity = parsed.last_activity;
    existing.jsonl_path = path.to_path_buf();
    existing.last_file_size = parsed.file_size;
    existing.last_user_prompt = parsed.last_user_prompt;
}

/// Build a fresh [`Session`] from a JSONL file plus its matching live entry.
fn build_session_from_jsonl(session_id: String, path: &Path, context: &ScanContext) -> Session {
    let prev = context.prev_sessions.get(&session_id);
    let parsed = parse_with_prev(path, prev);

    let working_dir = resolve_cwd(parsed.working_dir, prev, context.project_dir);
    let (project_name, relative_dir, branch) = git_project_info(&working_dir);

    let status = determine_status(
        parsed.input_tokens,
        parsed.output_tokens,
        Some(&context.live.pane_target),
    );

    Session {
        id: session_id,
        project_name,
        branch,
        cwd: working_dir,
        relative_dir,
        tmux_name: Some(context.live.tmux_session.clone()),
        pane_target: Some(context.live.pane_target.clone()),
        model: parsed.model,
        effort: parsed.effort,
        total_input_tokens: parsed.input_tokens,
        total_output_tokens: parsed.output_tokens,
        status,
        pid: Some(context.live.claude_pid),
        last_activity: parsed.last_activity,
        started_at: context.live.started_at,
        jsonl_path: path.to_path_buf(),
        last_file_size: parsed.file_size,
        last_user_prompt: parsed.last_user_prompt,
    }
}

/// Resolve the session CWD: prefer the value parsed from JSONL, then the
/// previous cached value, then a decoded form of the project directory name.
fn resolve_cwd(parsed: Option<String>, prev: Option<&Session>, project_dir: &Path) -> String {
    parsed
        .or_else(|| prev.map(|sess| sess.cwd.clone()))
        .unwrap_or_else(|| decode_project_path(project_dir))
}

/// Build [`ParseInputs`] from a previous-state lookup and run [`parse_jsonl`].
fn parse_with_prev(path: &Path, prev: Option<&Session>) -> ParsedInfo {
    let inputs = ParseInputs {
        file_size_at_last_poll: prev.map_or(0, |sess| sess.last_file_size),
        carried_input: prev.map_or(0, |sess| sess.total_input_tokens),
        carried_output: prev.map_or(0, |sess| sess.total_output_tokens),
        last_model: prev.and_then(|sess| sess.model.clone()),
        last_effort: prev.and_then(|sess| sess.effort.clone()),
        last_activity_ts: prev.and_then(|sess| sess.last_activity.clone()),
        last_seen_user_prompt: prev.and_then(|sess| sess.last_user_prompt.clone()),
    };
    parse_jsonl(path, &inputs)
}

/// Handle live sessions with no direct JSONL name match.
///
/// Covers two cases:
///   1. Brand-new sessions (no JSONL yet) → show as [`SessionStatus::New`]
///      placeholder.
///   2. Resumed sessions (`claude --resume` creates a new session-id in the
///      session file but continues appending to the original JSONL) → find
///      via lsof / tmux env / ps args, show real data.
///
/// Dedup by PID, not tmux session name. Multiple Claude instances can share
/// a tmux session (e.g. two panes). Deduping by session name would silently
/// hide the second instance.
fn add_unmatched_live_sessions(
    live_map: &HashMap<String, LiveSessionInfo>,
    prev_sessions: &HashMap<String, Session>,
    state: &mut DiscoveryState,
) {
    let known_pids: HashSet<i32> = state.sessions.iter().filter_map(|sess| sess.pid).collect();

    for (session_id_key, live) in live_map {
        if known_pids.contains(&live.claude_pid) {
            continue;
        }

        let resolved_path = resolve_resume_jsonl(session_id_key, live, prev_sessions);

        if let Some(path) = resolved_path.as_ref() {
            if let Some(stem) =
                path.file_stem().map(|raw_stem| raw_stem.to_string_lossy().into_owned())
            {
                state.matched_session_ids.insert(stem);
            }
        }

        state.sessions.push(build_unmatched_live_session(
            session_id_key,
            live,
            prev_sessions,
            resolved_path,
        ));
    }
}

/// For a session whose live entry has no direct JSONL counterpart, see if
/// it's a `claude --resume` and find the original JSONL.
fn resolve_resume_jsonl(
    session_id_key: &str,
    live: &LiveSessionInfo,
    prev_sessions: &HashMap<String, Session>,
) -> Option<PathBuf> {
    if session_id_key.starts_with("tmux-") {
        return None;
    }
    let cached = prev_sessions
        .get(session_id_key)
        .filter(|sess| !sess.jsonl_path.as_os_str().is_empty())
        .map(|sess| sess.jsonl_path.clone());
    cached.or_else(|| find_jsonl_for_resumed_session(&live.tmux_session, live.claude_pid))
}

/// Build a [`Session`] for an unmatched live session — either the resumed
/// branch (when `resolved_path` is `Some`) or the brand-new placeholder
/// branch.
fn build_unmatched_live_session(
    session_id_key: &str,
    live: &LiveSessionInfo,
    prev_sessions: &HashMap<String, Session>,
    resolved_path: Option<PathBuf>,
) -> Session {
    resolved_path.map_or_else(
        || build_placeholder_session(session_id_key, live),
        |path| build_resumed_session(session_id_key, live, prev_sessions, path),
    )
}

/// Build a [`Session`] for a resumed-but-not-directly-matched JSONL.
fn build_resumed_session(
    session_id_key: &str,
    live: &LiveSessionInfo,
    prev_sessions: &HashMap<String, Session>,
    path: PathBuf,
) -> Session {
    let prev = prev_sessions.get(session_id_key);
    let parsed = parse_with_prev(&path, prev);
    let working_dir = parsed.working_dir.clone().unwrap_or_else(|| live.pane_cwd.clone());
    let (project_name, relative_dir, branch) = git_project_info(&working_dir);
    let status =
        determine_status(parsed.input_tokens, parsed.output_tokens, Some(&live.pane_target));
    Session {
        id: session_id_key.to_owned(),
        project_name,
        relative_dir,
        branch,
        cwd: working_dir,
        tmux_name: Some(live.tmux_session.clone()),
        pane_target: Some(live.pane_target.clone()),
        model: parsed.model,
        effort: parsed.effort,
        total_input_tokens: parsed.input_tokens,
        total_output_tokens: parsed.output_tokens,
        status,
        pid: Some(live.claude_pid),
        last_activity: parsed.last_activity,
        started_at: live.started_at,
        jsonl_path: path,
        last_file_size: parsed.file_size,
        last_user_prompt: parsed.last_user_prompt,
    }
}

/// Build a placeholder [`Session`] for a brand-new pane that has no JSONL
/// yet.
fn build_placeholder_session(session_id_key: &str, live: &LiveSessionInfo) -> Session {
    let (project_name, relative_dir, branch) = git_project_info(&live.pane_cwd);
    Session {
        id: session_id_key.to_owned(),
        project_name,
        relative_dir,
        branch,
        cwd: live.pane_cwd.clone(),
        tmux_name: Some(live.tmux_session.clone()),
        pane_target: Some(live.pane_target.clone()),
        model: None,
        effort: None,
        total_input_tokens: 0,
        total_output_tokens: 0,
        status: SessionStatus::New,
        pid: Some(live.claude_pid),
        last_activity: None,
        started_at: live.started_at,
        jsonl_path: PathBuf::new(),
        last_file_size: 0,
        last_user_prompt: None,
    }
}

/// Stable order: tmux `pane_target` (`session:window.pane`), then
/// `started_at`, then `session_id`. Doesn't reshuffle when activity
/// timestamps change.
fn sort_sessions(sessions: &mut [Session]) {
    sessions.sort_by(|left, right| {
        left.pane_target
            .as_deref()
            .unwrap_or("")
            .cmp(right.pane_target.as_deref().unwrap_or(""))
            .then(left.started_at.cmp(&right.started_at))
            .then(left.id.cmp(&right.id))
    });
}
