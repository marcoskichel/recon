use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::model;

/// Maximum bytes per JSONL line before discarding.
/// Prevents OOM from malicious files with unbounded lines.
const MAX_LINE_BYTES: usize = 10 * 1024 * 1024; // 10 MB

/// Read a line with a cap on allocation. Uses fill_buf/consume to avoid
/// allocating beyond the cap. Returns Ok(0) at EOF. Overlong lines are
/// consumed and discarded (buf left empty, positive byte count returned
/// so callers can distinguish from EOF).
pub(crate) fn read_line_capped<R: Read>(
    reader: &mut BufReader<R>,
    buf: &mut String,
) -> std::io::Result<usize> {
    let mut raw = Vec::new();
    let mut overflowed = false;
    let mut total_consumed = 0usize;

    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }

        let newline_pos = available.iter().position(|&b| b == b'\n');
        let chunk_end = newline_pos.map(|p| p + 1).unwrap_or(available.len());

        if !overflowed {
            if raw.len() + chunk_end <= MAX_LINE_BYTES {
                raw.extend_from_slice(&available[..chunk_end]);
            } else {
                overflowed = true;
                raw = Vec::new();
                buf.clear(); // ensure buf is empty on overflow even if caller didn't pre-clear
            }
        }

        total_consumed += chunk_end;
        reader.consume(chunk_end);

        if newline_pos.is_some() {
            break;
        }
    }

    if total_consumed == 0 {
        return Ok(0); // EOF
    }

    if !overflowed {
        *buf = String::from_utf8(raw).unwrap_or_default();
    }

    Ok(total_consumed)
}

/// Validate that a CWD path is safe to pass to external commands.
/// Must be absolute and resolve to an existing directory.
pub(crate) fn validate_cwd(cwd: &str) -> bool {
    let path = Path::new(cwd);
    path.is_absolute() && path.is_dir()
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SessionStatus {
    New,
    Working,
    Idle,
    Input,
}

impl SessionStatus {
    pub fn label(&self) -> &str {
        match self {
            SessionStatus::New => "New",
            SessionStatus::Working => "Working",
            SessionStatus::Idle => "Idle",
            SessionStatus::Input => "Input",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub session_id: String,
    pub project_name: String,
    pub branch: Option<String>,
    pub cwd: String,
    pub relative_dir: Option<String>,
    pub tmux_session: Option<String>,
    pub pane_target: Option<String>,
    pub model: Option<String>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub status: SessionStatus,
    pub pid: Option<i32>,
    pub effort: Option<String>,
    pub last_activity: Option<String>,
    pub started_at: u64,
    pub jsonl_path: PathBuf,
    pub last_file_size: u64,
    pub last_user_prompt: Option<String>,
}

impl Session {
    pub fn room_id(&self) -> String {
        if let Some(name) = &self.tmux_session {
            return name.clone();
        }
        match &self.relative_dir {
            Some(dir) => format!("{} \u{203A} {}", self.project_name, dir),
            None => self.project_name.clone(),
        }
    }

    pub fn token_ratio(&self) -> f64 {
        let used = self.total_input_tokens + self.total_output_tokens;
        let window = self
            .model
            .as_deref()
            .map(model::context_window)
            .unwrap_or(200_000);
        if window == 0 {
            return 0.0;
        }
        used as f64 / window as f64
    }

}

/// Discover sessions by scanning JSONL files, then matching to live tmux panes.
///
/// `enrich_git` controls whether project_name/branch/relative_dir are populated
/// via git invocations on each session's CWD. The view needs them for display;
/// the daemon does not (it only feeds JSONL contents to the summarizer).
/// Skipping git in daemon mode avoids running `git -C <user-cwd>` per session,
/// which on macOS triggers TCC prompts whenever a CWD lives under a protected
/// directory (Documents, Desktop, Downloads, iCloud, external volumes).
pub fn discover_sessions(prev_sessions: &HashMap<String, Session>) -> Vec<Session> {
    discover_sessions_inner(prev_sessions)
}

fn discover_sessions_inner(
    prev_sessions: &HashMap<String, Session>,
) -> Vec<Session> {
    let claude_dir = match dirs::home_dir() {
        Some(h) => h.join(".claude").join("projects"),
        None => return vec![],
    };

    if !claude_dir.exists() {
        return vec![];
    }

    // Build the live session map: session_id → (pid, tmux_name, started_at)
    // by joining ~/.claude/sessions/{PID}.json with tmux pane info.
    let live_map = build_live_session_map();

    let mut sessions: Vec<Session> = Vec::new();
    let mut matched_session_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // Scan all JSONL files across project directories.
    // No mtime cutoff needed — the live_map check (below) already filters out
    // dead sessions, and skipping the stat() call is faster than doing it.
    let entries = match fs::read_dir(&claude_dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    for entry in entries.flatten() {
        let project_dir = entry.path();
        if !project_dir.is_dir() {
            continue;
        }

        let jsonl_files = match fs::read_dir(&project_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for jentry in jsonl_files.flatten() {
            let path = jentry.path();
            if path.is_dir() {
                continue;
            }
            if !path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                continue;
            }

            let session_id = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            // Look up in live map — skip if no live process
            let live = match live_map.get(&session_id) {
                Some(l) => l,
                None => continue,
            };

            // Same session_id can appear in multiple project dirs (e.g. session
            // started in one CWD then moved to a worktree). Prefer the larger file.
            if matched_session_ids.contains(&session_id) {
                if let Some(existing) = sessions.iter_mut().find(|s| s.session_id == session_id) {
                    let existing_size = existing.jsonl_path.metadata().ok().map(|m| m.len()).unwrap_or(0);
                    let new_size = path.metadata().ok().map(|m| m.len()).unwrap_or(0);
                    if new_size > existing_size {
                        let prev = prev_sessions.get(&session_id);
                        let info = parse_jsonl(
                            &path,
                            prev.map(|s| s.last_file_size).unwrap_or(0),
                            prev.map(|s| s.total_input_tokens).unwrap_or(0),
                            prev.map(|s| s.total_output_tokens).unwrap_or(0),
                            prev.and_then(|s| s.model.clone()),
                            prev.and_then(|s| s.effort.clone()),
                            prev.and_then(|s| s.last_activity.clone()),
                            prev.and_then(|s| s.last_user_prompt.clone()),
                        );
                        let cwd = info.cwd
                            .or_else(|| prev.map(|s| s.cwd.clone()))
                            .unwrap_or_else(|| decode_project_path(&project_dir));
                        let (project_name, relative_dir, branch) = git_project_info(&cwd);
                        existing.project_name = project_name;
                        existing.relative_dir = relative_dir;
                        existing.branch = branch;
                        existing.cwd = cwd;
                        existing.model = info.model;
                        existing.effort = info.effort;
                        existing.total_input_tokens = info.input_tokens;
                        existing.total_output_tokens = info.output_tokens;
                        existing.last_activity = info.last_activity;
                        existing.jsonl_path = path;
                        existing.last_file_size = info.file_size;
                        existing.last_user_prompt = info.last_user_prompt;
                    }
                }
                continue;
            }

            // Incremental JSONL parsing
            let prev = prev_sessions.get(&session_id);
            let info = parse_jsonl(
                &path,
                prev.map(|s| s.last_file_size).unwrap_or(0),
                prev.map(|s| s.total_input_tokens).unwrap_or(0),
                prev.map(|s| s.total_output_tokens).unwrap_or(0),
                prev.and_then(|s| s.model.clone()),
                prev.and_then(|s| s.effort.clone()),
                prev.and_then(|s| s.last_activity.clone()),
                prev.and_then(|s| s.last_user_prompt.clone()),
            );

            let cwd = info
                .cwd
                .or_else(|| prev.map(|s| s.cwd.clone()))
                .unwrap_or_else(|| decode_project_path(&project_dir));
            let (project_name, relative_dir, branch) = git_project_info(&cwd);

            let status = determine_status(
                &path,
                info.input_tokens,
                info.output_tokens,
                Some(&live.pane_target),
            );

            matched_session_ids.insert(session_id.clone());

            sessions.push(Session {
                session_id,
                project_name,
                branch,
                cwd,
                relative_dir,
                tmux_session: Some(live.tmux_session.clone()),
                pane_target: Some(live.pane_target.clone()),
                model: info.model,
                effort: info.effort,
                total_input_tokens: info.input_tokens,
                total_output_tokens: info.output_tokens,
                status,
                pid: Some(live.pid),
                last_activity: info.last_activity,
                started_at: live.started_at,
                jsonl_path: path,
                last_file_size: info.file_size,
                last_user_prompt: info.last_user_prompt,
            });
        }
    }

    // Handle live sessions with no direct JSONL name match.
    // This covers two cases:
    //   1. Brand-new sessions (no JSONL yet) → show as New placeholder
    //   2. Resumed sessions (claude --resume creates a new session-id in the session file
    //      but continues appending to the original JSONL) → find via lsof, show real data
    //
    // Dedup by PID, not tmux session name. Multiple Claude instances can share
    // a tmux session (e.g. two panes). Deduping by session name would silently
    // hide the second instance. PID is the unique identifier per Claude process,
    // so each instance gets its own stable entry in the table — even if the TUI
    // shows duplicate session names.
    let known_pids: std::collections::HashSet<i32> = sessions
        .iter()
        .filter_map(|s| s.pid)
        .collect();

    for (session_id_key, live) in &live_map {
        if known_pids.contains(&live.pid) {
            continue;
        }

        // For sessions that have a real session-id (not the "tmux-{name}" placeholder),
        // try to find the JSONL via resume detection. This handles resumed sessions
        // where the session file's session-id doesn't match the original JSONL filename.
        //
        // However, if the session was /reset after being resumed, the ps args still
        // show the old --resume ID while a new JSONL exists. In that case, the resume
        // JSONL is stale. We detect this: if the resumed JSONL's session-id matches
        // the session_id_key (from {PID}.json), the resume is current; otherwise
        // /reset happened and we skip the stale resume path.
        let jsonl_path = if !session_id_key.starts_with("tmux-") {
            let cached = prev_sessions
                .get(session_id_key.as_str())
                .filter(|s| !s.jsonl_path.as_os_str().is_empty())
                .map(|s| s.jsonl_path.clone());
            cached.or_else(|| find_jsonl_for_resumed_session(&live.tmux_session, live.pid))
        } else {
            None
        };

        let resolved_path = jsonl_path;

        // Mark as claimed so other sessions in the same dir don't grab the same JSONL
        if let Some(ref path) = resolved_path {
            if let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) {
                matched_session_ids.insert(stem);
            }
        }

        if let Some(path) = resolved_path {
            let prev = prev_sessions.get(session_id_key.as_str());
            let info = parse_jsonl(
                &path,
                prev.map(|s| s.last_file_size).unwrap_or(0),
                prev.map(|s| s.total_input_tokens).unwrap_or(0),
                prev.map(|s| s.total_output_tokens).unwrap_or(0),
                prev.and_then(|s| s.model.clone()),
                prev.and_then(|s| s.effort.clone()),
                prev.and_then(|s| s.last_activity.clone()),
                prev.and_then(|s| s.last_user_prompt.clone()),
            );

            let cwd = info.cwd.clone().unwrap_or_else(|| live.pane_cwd.clone());
            let (project_name, relative_dir, branch) = git_project_info(&cwd);

            let status = determine_status(
                &path,
                info.input_tokens,
                info.output_tokens,
                Some(&live.pane_target),
            );

            sessions.push(Session {
                session_id: session_id_key.clone(),
                project_name,
                relative_dir,
                branch,
                cwd,
                tmux_session: Some(live.tmux_session.clone()),
                pane_target: Some(live.pane_target.clone()),
                model: info.model,
                effort: info.effort,
                total_input_tokens: info.input_tokens,
                total_output_tokens: info.output_tokens,
                status,
                pid: Some(live.pid),
                last_activity: info.last_activity,
                started_at: live.started_at,
                jsonl_path: path,
                last_file_size: info.file_size,
                last_user_prompt: info.last_user_prompt,
            });
        } else {
            // No JSONL found — brand-new session, show as New placeholder
            let (project_name, relative_dir, branch) = git_project_info(&live.pane_cwd);
            sessions.push(Session {
                session_id: session_id_key.clone(),
                project_name,
                relative_dir,
                branch,
                cwd: live.pane_cwd.clone(),
                tmux_session: Some(live.tmux_session.clone()),
                pane_target: Some(live.pane_target.clone()),
                model: None,
                effort: None,
                total_input_tokens: 0,
                total_output_tokens: 0,
                status: SessionStatus::New,
                pid: Some(live.pid),
                last_activity: None,
                started_at: live.started_at,
                jsonl_path: PathBuf::new(),
                last_file_size: 0,
                last_user_prompt: None,
            });
        }
    }

    // Stable order: tmux pane_target (session:window.pane), then started_at, then session_id.
    // Doesn't reshuffle when activity timestamps change.
    sessions.sort_by(|a, b| {
        a.pane_target
            .as_deref()
            .unwrap_or("")
            .cmp(b.pane_target.as_deref().unwrap_or(""))
            .then(a.started_at.cmp(&b.started_at))
            .then(a.session_id.cmp(&b.session_id))
    });
    sessions
}

/// Info about a live claude session, built from tmux + session files.
struct LiveSessionInfo {
    pid: i32,
    tmux_session: String,
    pane_target: String,
    pane_cwd: String,
    started_at: u64,
}

/// Build a map from JSONL session_id → live session info.
///
/// Joins two sources:
///   1. tmux list-panes: PID → (tmux_session, pane_cwd) for panes running claude
///   2. ~/.claude/sessions/{PID}.json: PID → (session_id, started_at)
fn build_live_session_map() -> HashMap<String, LiveSessionInfo> {
    let pid_session_map = read_pid_session_map();
    let tmux_panes = discover_claude_tmux_panes();

    let mut map = HashMap::new();
    for (pid, tmux_session, pane_target, pane_cwd) in tmux_panes {
        if let Some(info) = pid_session_map.get(&pid) {
            map.insert(
                info.session_id.clone(),
                LiveSessionInfo {
                    pid,
                    tmux_session,
                    pane_target,
                    pane_cwd,
                    started_at: info.started_at,
                },
            );
        } else {
            // Tmux pane running claude but no session file yet (just started).
            // Use pane_target (not tmux session name) as placeholder key so that
            // two Claude panes in the same tmux session don't collide.
            map.insert(
                format!("tmux-{pane_target}"),
                LiveSessionInfo {
                    pid,
                    tmux_session,
                    pane_target,
                    pane_cwd,
                    started_at: 0,
                },
            );
        }
    }
    map
}

#[derive(Debug)]
struct ParsedInfo {
    input_tokens: u64,
    output_tokens: u64,
    model: Option<String>,
    effort: Option<String>,
    cwd: Option<String>,
    last_activity: Option<String>,
    file_size: u64,
    last_user_prompt: Option<String>,
}

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct StableGitInfo {
    repo_name: String,
    relative_dir: Option<String>,
}

struct BranchInfo {
    branch: Option<String>,
    fetched_at: Instant,
}

static STABLE_GIT_CACHE: Mutex<Option<HashMap<String, StableGitInfo>>> = Mutex::new(None);
static BRANCH_CACHE: Mutex<Option<HashMap<String, BranchInfo>>> = Mutex::new(None);

const BRANCH_CACHE_TTL: Duration = Duration::from_secs(300);

/// Allowlist parsed from `ROOSTR_TCC_ALLOW` (comma-separated absolute paths).
///
/// Any CWD under one of these prefixes bypasses the TCC-protected check —
/// useful when the user keeps real projects under `~/Documents` or `~/Desktop`
/// and is willing to grant the one-time macOS permission prompt.
fn tcc_allow_paths() -> &'static [PathBuf] {
    static CACHE: OnceLock<Vec<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(|| {
        std::env::var("ROOSTR_TCC_ALLOW")
            .ok()
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default()
    })
}

const TCC_PROTECTED_DIRS: &[&str] = &[
    "Pictures", "Desktop", "Documents", "Downloads", "Music", "Movies",
];

/// Pure check: is `path` inside a TCC-protected dir under `home`, after
/// honoring the explicit `allow` list?
fn is_tcc_protected_with(path: &Path, home: Option<&Path>, allow: &[PathBuf]) -> bool {
    if allow.iter().any(|p| path.starts_with(p)) {
        return false;
    }
    let Some(home) = home else {
        return false;
    };
    TCC_PROTECTED_DIRS
        .iter()
        .any(|dir| path.starts_with(home.join(dir)))
}

/// Returns true if `path` is inside a macOS TCC-protected directory.
///
/// Running `git -C <path>` inside these dirs triggers system permission
/// prompts (Photos, Desktop, Documents, Downloads, etc.) even when roostr
/// has no legitimate need to access those files.
///
/// Override via `ROOSTR_TCC_ALLOW=/abs/path1,/abs/path2`.
fn is_tcc_protected(path: &Path) -> bool {
    is_tcc_protected_with(path, dirs::home_dir().as_deref(), tcc_allow_paths())
}

/// Get the git project name, relative_dir, and branch for a directory.
///
/// repo_name and relative_dir are immutable per CWD — cached forever to avoid
/// repeated TCC prompts on macOS for git/canonicalize syscalls.
/// branch can change at runtime — refreshed every 30s.
fn git_project_info(cwd: &str) -> (String, Option<String>, Option<String>) {
    if !Path::new(cwd).is_absolute() {
        let fallback = Path::new(cwd)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cwd.to_string());
        return (fallback, None, None);
    }

    if is_tcc_protected(Path::new(cwd)) {
        let name = Path::new(cwd)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cwd.to_string());
        return (name, None, None);
    }

    let stable_hit = STABLE_GIT_CACHE
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(cwd))
        .map(|i| (i.repo_name.clone(), i.relative_dir.clone()));

    let branch_hit = BRANCH_CACHE
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(cwd))
        .filter(|i| i.fetched_at.elapsed() < BRANCH_CACHE_TTL)
        .map(|i| i.branch.clone());

    if let (Some((repo_name, relative_dir)), Some(branch)) = (stable_hit.clone(), branch_hit.clone()) {
        return (repo_name, relative_dir, branch);
    }

    // At least one cache miss — do a single combined git rev-parse call.
    let combined = fetch_combined_git_info(cwd);

    let (repo_name, relative_dir) = match stable_hit {
        Some(s) => s,
        None => {
            let repo_name = combined
                .as_ref()
                .and_then(|c| c.repo_name.clone())
                .unwrap_or_else(|| {
                    Path::new(cwd)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| cwd.to_string())
                });
            let relative_dir = combined.as_ref().and_then(|c| c.relative_dir.clone());
            let mut cache = STABLE_GIT_CACHE.lock().unwrap();
            cache.get_or_insert_with(HashMap::new).insert(
                cwd.to_string(),
                StableGitInfo {
                    repo_name: repo_name.clone(),
                    relative_dir: relative_dir.clone(),
                },
            );
            (repo_name, relative_dir)
        }
    };

    let branch = match branch_hit {
        Some(b) => b,
        None => {
            let branch = combined.as_ref().and_then(|c| c.branch.clone());
            let mut cache = BRANCH_CACHE.lock().unwrap();
            cache.get_or_insert_with(HashMap::new).insert(
                cwd.to_string(),
                BranchInfo {
                    branch: branch.clone(),
                    fetched_at: Instant::now(),
                },
            );
            branch
        }
    };

    (repo_name, relative_dir, branch)
}

struct CombinedGitInfo {
    repo_name: Option<String>,
    relative_dir: Option<String>,
    branch: Option<String>,
}

/// Single `git rev-parse` call returning toplevel, common-dir, and branch.
/// Replaces three separate process spawns.
fn fetch_combined_git_info(cwd: &str) -> Option<CombinedGitInfo> {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            cwd,
            "rev-parse",
            "--show-toplevel",
            "--git-common-dir",
            "--abbrev-ref",
            "HEAD",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() < 3 {
        return None;
    }
    let toplevel = lines[0].trim();
    let common_dir = lines[1].trim();
    let branch_raw = lines[2].trim();

    // Repo name from --git-common-dir (stable across worktrees).
    let common_path = if Path::new(common_dir).is_absolute() {
        PathBuf::from(common_dir)
    } else {
        PathBuf::from(cwd).join(common_dir)
    };
    let repo_root = if common_path.file_name().map(|n| n == ".git").unwrap_or(false) {
        common_path.parent().map(|p| p.to_path_buf())
    } else {
        Some(common_path)
    };
    let repo_name = repo_root.and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

    // Relative dir from cwd vs --show-toplevel.
    let cwd_path = Path::new(cwd);
    let top_path = Path::new(toplevel);
    let relative = cwd_path
        .strip_prefix(top_path)
        .map(Path::to_path_buf)
        .or_else(|_| {
            let cwd_resolved = cwd_path.canonicalize().unwrap_or_else(|_| PathBuf::from(cwd));
            let top_resolved = top_path
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(toplevel));
            cwd_resolved.strip_prefix(&top_resolved).map(Path::to_path_buf)
        })
        .unwrap_or_default();
    let relative_dir = if relative.as_os_str().is_empty() || relative == Path::new(".") {
        None
    } else {
        Some(relative.display().to_string())
    };

    let branch = if branch_raw.is_empty() || branch_raw == "HEAD" {
        None
    } else {
        Some(branch_raw.to_string())
    };

    Some(CombinedGitInfo {
        repo_name,
        relative_dir,
        branch,
    })
}


/// Decode an encoded project directory name back to a path.
/// `-Users-gavra-repos-yaba` -> `/Users/gavra/repos/yaba`
/// This is a best-effort reverse of the encoding (ambiguous for `.` and `_`).
fn decode_project_path(project_dir: &Path) -> String {
    let name = project_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // The encoded name replaces / with -, so the first char is always -
    // Convert back: leading - becomes /, internal - becomes /
    // This is lossy (can't distinguish original - from / or . or _) but good enough
    if name.starts_with('-') {
        name.replacen('-', "/", 1)
            .replace('-', "/")
    } else {
        name
    }
}

/// Minimal serde structs for JSONL parsing.
#[derive(Deserialize)]
struct JsonlEntry {
    #[serde(default)]
    message: Option<MessageEntry>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default, rename = "isMeta")]
    is_meta: Option<bool>,
    #[serde(default, rename = "toolUseResult")]
    tool_use_result: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct MessageEntry {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<UsageEntry>,
    #[serde(default)]
    content: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct UsageEntry {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

/// Returns true if a user prompt is "substantive" — likely conveys task content
/// rather than being a continuation/affirmation/slash-command/system marker.
fn is_substantive_prompt(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with("<command-name>")
        || trimmed.starts_with("<local-command-stdout>")
        || trimmed.starts_with("<local-command-caveat>")
        || trimmed.starts_with("Caveat:")
        || trimmed.starts_with("This session is being continued")
        || trimmed.starts_with("[Request interrupted")
    {
        return false;
    }

    let cleaned = trimmed
        .split_whitespace()
        .filter(|w| !w.starts_with("[Image") && !w.starts_with("<command-"))
        .collect::<Vec<_>>()
        .join(" ");

    let lower = cleaned
        .to_lowercase()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_string();

    const STOPLIST: &[&str] = &[
        "continue", "contiue", "yes", "y", "yep", "yeah",
        "no", "n", "nope",
        "ok", "okay", "k", "kk",
        "sure", "retry", "go", "go ahead", "yes go ahead",
        "yes please", "go for it", "do it", "fix it",
        "please", "thanks", "ty", "thx", "thank you",
        "hmm", "what", "try again", "looks good", "all good",
        "perfect", "great", "nice", "cool", "awesome",
        "i approve", "i apoprove", "approved", "approve",
        "keep going", "next", "more", "good",
    ];
    if STOPLIST.contains(&lower.as_str()) {
        return false;
    }

    let word_count = cleaned.split_whitespace().count();
    let char_count = cleaned.chars().count();
    word_count >= 4 || char_count >= 20
}

/// Parse JSONL file, incrementally if possible.
fn parse_jsonl(
    path: &Path,
    prev_file_size: u64,
    prev_input: u64,
    prev_output: u64,
    prev_model: Option<String>,
    prev_effort: Option<String>,
    prev_activity: Option<String>,
    prev_last_user_prompt: Option<String>,
) -> ParsedInfo {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => {
            return ParsedInfo {
                input_tokens: prev_input,
                output_tokens: prev_output,
                model: prev_model,
                effort: prev_effort,
                cwd: None,
                last_activity: prev_activity,
                file_size: 0,
                last_user_prompt: prev_last_user_prompt,
            }
        }
    };

    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);

    if file_size == prev_file_size && prev_file_size > 0 {
        return ParsedInfo {
            input_tokens: prev_input,
            output_tokens: prev_output,
            model: prev_model,
            effort: prev_effort,
            cwd: None,
            last_activity: prev_activity,
            file_size,
            last_user_prompt: prev_last_user_prompt,
        };
    }

    let mut reader = BufReader::new(file);
    let mut total_input = prev_input;
    let mut total_output = prev_output;
    let mut model = prev_model;
    let mut effort = prev_effort;
    let mut last_activity = prev_activity;
    let mut cwd = None;
    let mut last_user_prompt = prev_last_user_prompt;

    if prev_file_size > 0 {
        let _ = reader.seek(SeekFrom::Start(prev_file_size));
    } else {
        total_input = 0;
        total_output = 0;
        model = None;
        effort = None;
        last_activity = None;
        last_user_prompt = None;
    }

    let mut line = String::new();
    loop {
        line.clear();
        match read_line_capped(&mut reader, &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }

        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains("\"type\"") {
            continue;
        }

        if trimmed.contains("\"type\":\"assistant\"") {
            // Skip synthetic entries — they have 0 tokens and overwrite real data
            if trimmed.contains("\"<synthetic>\"") {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<JsonlEntry>(trimmed) {
                if let Some(ts) = entry.timestamp {
                    last_activity = Some(ts);
                }
                if entry.cwd.is_some() {
                    cwd = entry.cwd;
                }
                if let Some(msg) = entry.message {
                    if let Some(m) = msg.model {
                        model = Some(m);
                    }
                    if let Some(usage) = msg.usage {
                        total_input = usage.input_tokens
                            + usage.cache_creation_input_tokens
                            + usage.cache_read_input_tokens;
                        total_output = usage.output_tokens;
                    }
                }
            }
        } else if trimmed.contains("\"type\":\"user\"") || trimmed.contains("\"type\":\"system\"") {
            if let Ok(entry) = serde_json::from_str::<JsonlEntry>(trimmed) {
                if let Some(ref ts) = entry.timestamp {
                    last_activity = Some(ts.clone());
                }
                if entry.cwd.is_some() {
                    cwd = entry.cwd.clone();
                }
                if entry.is_meta != Some(true) && entry.tool_use_result.is_none() {
                    if let Some(content) = entry
                        .message
                        .as_ref()
                        .and_then(|m| m.content.as_ref())
                        .and_then(|c| c.as_str())
                    {
                        if is_substantive_prompt(content) {
                            last_user_prompt = Some(content.to_string());
                        }
                    }
                }
            }
            // Extract model + effort from /model command stdout recorded in JSONL:
            //   "Set model to Opus 4.6 (1M context) (default) with max effort"
            //   "Set model to Sonnet 4.6"
            if trimmed.contains("<local-command-stdout>Set model to")
                && !trimmed.contains("toolUseResult")
                && !trimmed.contains("tool_result")
            {
                let stdout_pos = trimmed.find("<local-command-stdout>Set model to").unwrap();
                let tag_end = stdout_pos + "<local-command-stdout>Set model to".len();
                let raw_remainder = &trimmed[tag_end..];
                // Truncate at closing tag
                let raw_remainder = raw_remainder
                    .find("</local-command-stdout>")
                    .map_or(raw_remainder, |end| &raw_remainder[..end]);
                let remainder = strip_ansi(raw_remainder);
                let remainder = remainder.trim();

                // Extract effort if present ("with <effort> effort")
                let (model_part, new_effort) = if let Some(wp) = remainder.find("with ") {
                    let after_with = &remainder[wp + 5..];
                    let eff = after_with.find(" effort")
                        .map(|end| after_with[..end].trim().to_string())
                        .filter(|s| !s.is_empty());
                    (&remainder[..wp], eff)
                } else {
                    (&remainder[..], None)
                };
                if let Some(e) = new_effort {
                    effort = Some(e);
                }

                // Extract model: strip suffixes like "(1M context)" and "(default)"
                let model_name = model_part
                    .trim()
                    .trim_end_matches("(default)")
                    .trim()
                    .trim_end_matches("(1M context)")
                    .trim()
                    .trim_end_matches("(200k context)")
                    .trim();
                if let Some(id) = model::id_from_display_name(model_name) {
                    model = Some(id.to_string());
                }
            }
        }
    }

    ParsedInfo {
        input_tokens: total_input,
        output_tokens: total_output,
        model,
        effort,
        cwd,
        last_activity,
        file_size,
        last_user_prompt,
    }
}

/// For a resumed session, find the original JSONL by locating the session-id
/// that `claude --resume` was called with.
///
/// `claude --resume <orig-id>` writes a new session-id to its session file but
/// continues appending to the original JSONL (named after the old session-id).
///
/// Strategy (in order):
///  1. Read `ROOSTR_RESUMED_FROM` from the tmux session environment — set by
///     `roostr --resume` at session creation time. Reliable and zero-overhead.
///  2. Fall back to parsing `ps` args for sessions started outside of roostr
///     (e.g. the user ran `claude --resume <id>` in a tmux session manually).
fn find_jsonl_for_resumed_session(tmux_session: &str, pid: i32) -> Option<PathBuf> {
    // Try tmux environment variable first (set by roostr --resume)
    let original_id = read_tmux_env(tmux_session, "ROOSTR_RESUMED_FROM")
        // Fall back to parsing ps args
        .or_else(|| parse_resume_id_from_ps(pid))?;

    find_jsonl_by_session_id(&original_id)
}

/// Read a variable from a tmux session's environment table.
fn read_tmux_env(session_name: &str, var: &str) -> Option<String> {
    let output = std::process::Command::new("tmux")
        .args(["show-environment", "-t", session_name, var])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }
    // Output format: "VAR=value\n"
    let line = String::from_utf8_lossy(&output.stdout);
    line.trim().split_once('=').map(|(_, v)| v.to_string())
}

/// Parse `--resume <session-id>` from the process command line via ps.
/// Fallback for sessions not created by `roostr --resume`.
fn parse_resume_id_from_ps(pid: i32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "args="])
        .output()
        .ok()?;

    let args = String::from_utf8_lossy(&output.stdout);
    args.trim()
        .split_whitespace()
        .skip_while(|&a| a != "--resume")
        .nth(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Strip ANSI escape sequences from a string.
/// Handles both raw ESC byte (\x1b[...m) and JSON-encoded form (\\u001b[...m).
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Raw ESC byte: skip until 'm'
            for next in chars.by_ref() {
                if next == 'm' { break; }
            }
        } else if c == '\\' && chars.peek() == Some(&'u') {
            // Check for JSON-escaped \\u001b
            let rest: String = chars.clone().take(5).collect();
            if rest.starts_with("u001b") || rest.starts_with("u001B") {
                // Consume "u001b" (5 chars)
                for _ in 0..5 { chars.next(); }
                // Skip the ANSI parameter sequence until 'm'
                for next in chars.by_ref() {
                    if next == 'm' { break; }
                }
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Find the JSONL file for a given session-id by scanning all project directories.
fn find_jsonl_by_session_id(session_id: &str) -> Option<PathBuf> {
    let projects_dir = dirs::home_dir()?.join(".claude").join("projects");
    let mut best: Option<(PathBuf, u64)> = None;
    for entry in fs::read_dir(&projects_dir).ok()?.flatten() {
        let candidate = entry.path().join(format!("{session_id}.jsonl"));
        if candidate.exists() {
            let size = candidate.metadata().ok().map(|m| m.len()).unwrap_or(0);
            if best.as_ref().map_or(true, |(_, s)| size > *s) {
                best = Some((candidate, size));
            }
        }
    }
    best.map(|(p, _)| p)
}


/// Determine session status from file recency and token counts.
/// - New: no tokens yet (never interacted)
/// - Working: JSONL modified in last 5s
/// - Input: last activity within 10 minutes (active conversation, waiting for user)
/// - Idle: last activity older than 10 minutes
fn determine_status(_path: &Path, input_tokens: u64, output_tokens: u64, pane_target: Option<&str>) -> SessionStatus {
    // tmux pane content is the source of truth for active sessions
    if let Some(target) = pane_target {
        let pane = pane_status(target);
        // Only show New if pane also looks idle (no active streaming)
        if input_tokens == 0 && output_tokens == 0 && pane == SessionStatus::Idle {
            return SessionStatus::New;
        }
        return pane;
    }

    if input_tokens == 0 && output_tokens == 0 {
        SessionStatus::New
    } else {
        SessionStatus::Idle
    }
}

/// Determine status by inspecting the Claude Code TUI pane content.
///
/// Scans the last few non-empty lines bottom-up looking for:
///   - Working: a line starting with a Unicode spinner (✽✢✳✶⏺) that also
///     contains "…" — these are thinking/tool-execution progress indicators
///   - Input: "Esc to cancel" on the last line, or a selection menu ("❯ N.")
///   - Idle: anything else
fn pane_status(pane_target: &str) -> SessionStatus {
    let output = match std::process::Command::new("tmux")
        .args(["capture-pane", "-t", pane_target, "-p"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return SessionStatus::Idle,
    };

    let content = String::from_utf8_lossy(&output.stdout);

    let mut lines_checked = 0;
    for line in content.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Input: permission prompt on the very last non-empty line
        if lines_checked == 0 && trimmed.contains("Esc to cancel") {
            return SessionStatus::Input;
        }

        // Working: line starts with a spinner character and contains "…"
        // Spinners: ✽(U+273D) ✢(U+2722) ✳(U+2733) ✶(U+2736) ⏺(U+23FA)
        if let Some(first) = trimmed.chars().next() {
            if is_spinner(first) && trimmed.contains('\u{2026}') {
                return SessionStatus::Working;
            }
        }

        // Input: selection-style permission prompts ("❯ N.")
        if let Some(pos) = trimmed.find('\u{276F}') { // ❯
            let after = trimmed[pos + '\u{276F}'.len_utf8()..].trim_start();
            if after.starts_with(|c: char| c.is_ascii_digit()) {
                return SessionStatus::Input;
            }
        }

        lines_checked += 1;
        if lines_checked >= 10 {
            break;
        }
    }

    SessionStatus::Idle
}

/// Check if a character is a Claude Code activity indicator.
/// Covers dingbat spinners (✽✢✳✶✻ etc.), record symbol (⏺),
/// and middle dot (·) used for progress lines.
fn is_spinner(c: char) -> bool {
    matches!(c,
        '\u{2720}'..='\u{2767}' | // Dingbats: ✽✢✳✶✻✺✴✵ etc.
        '\u{23FA}'              | // ⏺ (record)
        '\u{00B7}'                // · (middle dot, used for progress)
    )
}

// --- Live session discovery ---

struct SessionFileInfo {
    session_id: String,
    started_at: u64,
}

/// Read ~/.claude/sessions/{PID}.json files to build a PID → session info map.
fn read_pid_session_map() -> HashMap<i32, SessionFileInfo> {
    let sessions_dir = match dirs::home_dir() {
        Some(h) => h.join(".claude").join("sessions"),
        None => return HashMap::new(),
    };

    let entries = match fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return HashMap::new(),
    };

    let mut map = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let (Some(pid), Some(sid)) = (
                        v.get("pid").and_then(|p| p.as_i64()),
                        v.get("sessionId").and_then(|s| s.as_str()),
                    ) {
                        let started_at = v
                            .get("startedAt")
                            .and_then(|s| s.as_u64())
                            .map(|ms| ms / 1000)
                            .unwrap_or(0);
                        map.insert(
                            pid as i32,
                            SessionFileInfo {
                                session_id: sid.to_string(),
                                started_at,
                            },
                        );
                    }
                }
            }
        }
    }
    map
}

/// Get tmux panes running claude.
/// Returns Vec<(pid, session_name, pane_cwd)>.
///
/// Performance: builds a single ppid→children map from one `ps -eo pid,ppid`
/// call, avoiding per-pane `pgrep` spawns. Also enumerates session files once.
fn discover_claude_tmux_panes() -> Vec<(i32, String, String, String)> {
    let output = match std::process::Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_pid}|||#{session_name}|||#{pane_current_command}|||#{pane_current_path}|||#{window_index}|||#{pane_index}",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    let session_pids = read_session_pids();
    let children_map = ProcessChildren::load();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(6, "|||").collect();
        if parts.len() < 6 {
            continue;
        }
        let pid: i32 = match parts[0].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let session_name = parts[1];
        let command = parts[2];
        let pane_path = parts[3];
        let window_index = parts[4];
        let pane_index = parts[5];

        // Claude shows up as a version number (e.g. "2.1.76") or "claude" or "node".
        // On macOS, the npm-distributed binary's internal process name is "claude.exe"
        // (a bundler convention, not a Windows artifact), so tmux reports that instead.
        // If another binary name surfaces, consider switching to a `starts_with("claude")`
        // match as a general case.
        let is_claude = command
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
            || command == "claude"
            || command == "claude.exe"
            || command == "node";

        if is_claude {
            // pane_pid is the initial process — it may be claude itself (roostr launch)
            // or a shell with claude as the foreground child (manual `claude` in a terminal).
            // Try the pane PID first, fall back to searching children.
            let claude_pid = if session_pids.contains(&pid) {
                Some(pid)
            } else {
                children_map.find_descendant_in(pid, &session_pids)
            };
            if let Some(cpid) = claude_pid {
                let pane_target = format!("{session_name}:{window_index}.{pane_index}");
                results.push((cpid, session_name.to_string(), pane_target, pane_path.to_string()));
            }
        } else if command == "bash" || command == "sh" || command == "zsh" {
            if let Some(claude_pid) = children_map.find_descendant_in(pid, &session_pids) {
                let pane_target = format!("{session_name}:{window_index}.{pane_index}");
                results.push((claude_pid, session_name.to_string(), pane_target, pane_path.to_string()));
            }
        }
    }

    results
}

/// Enumerate PIDs that have a `~/.claude/sessions/{PID}.json` file.
fn read_session_pids() -> std::collections::HashSet<i32> {
    let sessions_dir = match dirs::home_dir() {
        Some(h) => h.join(".claude").join("sessions"),
        None => return std::collections::HashSet::new(),
    };
    let entries = match fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return std::collections::HashSet::new(),
    };
    entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().map(|x| x == "json").unwrap_or(false) {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<i32>().ok())
            } else {
                None
            }
        })
        .collect()
}

/// Process tree built from a single `ps` call.
struct ProcessChildren {
    map: HashMap<i32, Vec<i32>>,
}

impl ProcessChildren {
    fn load() -> Self {
        let output = match std::process::Command::new("ps")
            .args(["-eo", "pid=,ppid="])
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => return ProcessChildren { map: HashMap::new() },
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut map: HashMap<i32, Vec<i32>> = HashMap::new();
        for line in stdout.lines() {
            let mut parts = line.split_whitespace();
            let pid: Option<i32> = parts.next().and_then(|s| s.parse().ok());
            let ppid: Option<i32> = parts.next().and_then(|s| s.parse().ok());
            if let (Some(pid), Some(ppid)) = (pid, ppid) {
                map.entry(ppid).or_default().push(pid);
            }
        }
        ProcessChildren { map }
    }

    /// BFS from `parent` looking for any descendant whose PID is in `target_set`.
    fn find_descendant_in(&self, parent: i32, target_set: &std::collections::HashSet<i32>) -> Option<i32> {
        let mut stack = vec![parent];
        let mut seen = std::collections::HashSet::new();
        while let Some(pid) = stack.pop() {
            if !seen.insert(pid) {
                continue;
            }
            if let Some(children) = self.map.get(&pid) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[test]
    fn read_line_capped_normal() {
        let data = b"hello\nworld\n";
        let mut reader = BufReader::new(Cursor::new(data));
        let mut buf = String::new();

        let n = read_line_capped(&mut reader, &mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf, "hello\n");

        buf.clear();
        let n = read_line_capped(&mut reader, &mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf, "world\n");

        buf.clear();
        let n = read_line_capped(&mut reader, &mut buf).unwrap();
        assert_eq!(n, 0); // EOF
    }

    #[test]
    fn read_line_capped_no_trailing_newline() {
        let data = b"no newline";
        let mut reader = BufReader::new(Cursor::new(data));
        let mut buf = String::new();

        let n = read_line_capped(&mut reader, &mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf, "no newline");
    }

    #[test]
    fn read_line_capped_empty() {
        let data = b"";
        let mut reader = BufReader::new(Cursor::new(data));
        let mut buf = String::new();

        let n = read_line_capped(&mut reader, &mut buf).unwrap();
        assert_eq!(n, 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn read_line_capped_overlong_discarded() {
        // Create a line that exceeds MAX_LINE_BYTES, followed by a normal line
        let mut data = vec![b'x'; MAX_LINE_BYTES + 100];
        data.push(b'\n');
        data.extend_from_slice(b"ok\n");

        let mut reader = BufReader::new(Cursor::new(data));
        let mut buf = String::new();

        // First line is overlong — should be discarded
        let n = read_line_capped(&mut reader, &mut buf).unwrap();
        assert!(n > 0); // consumed bytes, not EOF
        assert!(buf.is_empty()); // but buf is empty

        // Second line should read normally
        buf.clear();
        let n = read_line_capped(&mut reader, &mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf, "ok\n");
    }

    #[test]
    fn read_line_capped_overflow_clears_stale_buf() {
        let mut data = vec![b'x'; MAX_LINE_BYTES + 100];
        data.push(b'\n');

        let mut reader = BufReader::new(Cursor::new(data));
        let mut buf = String::from("stale data");

        let n = read_line_capped(&mut reader, &mut buf).unwrap();
        assert!(n > 0);
        assert!(buf.is_empty()); // stale data cleared
    }

    #[test]
    fn validate_cwd_rejects_relative() {
        assert!(!validate_cwd("relative/path"));
    }

    #[test]
    fn validate_cwd_rejects_nonexistent() {
        assert!(!validate_cwd("/nonexistent/path/that/does/not/exist"));
    }

    #[test]
    fn validate_cwd_accepts_real_dir() {
        assert!(validate_cwd("/tmp"));
    }

    #[test]
    fn tcc_protected_detects_known_dirs() {
        let home = Path::new("/Users/test");
        let no_allow: Vec<PathBuf> = vec![];
        assert!(is_tcc_protected_with(
            &home.join("Pictures").join("Photos Library.photoslibrary"),
            Some(home),
            &no_allow,
        ));
        assert!(is_tcc_protected_with(&home.join("Desktop").join("a"), Some(home), &no_allow));
        assert!(is_tcc_protected_with(&home.join("Documents").join("work"), Some(home), &no_allow));
        assert!(is_tcc_protected_with(&home.join("Downloads"), Some(home), &no_allow));
        assert!(!is_tcc_protected_with(&home.join("dev").join("project"), Some(home), &no_allow));
        assert!(!is_tcc_protected_with(Path::new("/tmp/x"), Some(home), &no_allow));
    }

    #[test]
    fn tcc_allow_list_overrides_protection() {
        let home = Path::new("/Users/test");
        let allow = vec![home.join("Documents").join("code")];
        assert!(!is_tcc_protected_with(
            &home.join("Documents").join("code").join("project"),
            Some(home),
            &allow,
        ));
        assert!(is_tcc_protected_with(
            &home.join("Documents").join("personal"),
            Some(home),
            &allow,
        ));
    }

    #[test]
    fn tcc_protected_no_home_returns_false() {
        let allow: Vec<PathBuf> = vec![];
        assert!(!is_tcc_protected_with(Path::new("/anywhere"), None, &allow));
    }
}
