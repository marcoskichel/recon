//! tmux session creation, switching, killing, and name sanitization.

use std::process::Command;

use crate::session;

/// Switch to a tmux pane (inside tmux) or attach to its session (outside tmux).
///
/// `target` is a pane target like `"mywork:0.0"` (`session:window.pane`).
pub fn switch_to_pane(target: &str) {
    let inside_tmux = std::env::var("TMUX").is_ok();
    if inside_tmux {
        let _status = Command::new("tmux").args(["switch-client", "-t", target]).status();
    } else {
        let _status = Command::new("tmux").args(["attach-session", "-t", target]).status();
    }
}

/// Launch a command in a new tmux session with the given name and working
/// directory.
///
/// If `command` is `None`, runs `claude`. Otherwise splits the command on
/// whitespace and passes the parts as the binary + args to tmux (no shell
/// wrapper, so aliases won't resolve — use full paths).
///
/// Returns the session name on success.
///
/// # Errors
///
/// Returns `Err` if the working directory is invalid or if the underlying
/// `tmux new-session` invocation fails or exits non-zero.
pub fn create_session(
    name: &str,
    working_dir: &str,
    command: Option<&str>,
    tags: &[String],
) -> Result<String, String> {
    if !session::validate_cwd(working_dir) {
        return Err(format!("Invalid working directory: {working_dir}"));
    }

    let base_name = sanitize_session_name(name);
    let session_name = unique_session_name(&base_name);

    let mut tmux_args = vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        session_name.clone(),
        "-c".to_string(),
        working_dir.to_string(),
    ];

    if !tags.is_empty() {
        let tags_val = tags.join(",");
        tmux_args.push("-e".to_string());
        tmux_args.push(format!("ROOSTR_TAGS={tags_val}"));
    }

    if let Some(raw_cmd) = command {
        for part in raw_cmd.split_whitespace() {
            tmux_args.push(part.to_string());
        }
    } else {
        let claude_path = which_claude().unwrap_or_else(|| "claude".to_string());
        tmux_args.push(claude_path);
    }

    let status = Command::new("tmux")
        .args(&tmux_args)
        .status()
        .map_err(|error| format!("Failed to create tmux session: {error}"))?;

    if !status.success() {
        return Err("tmux new-session failed".to_string());
    }

    Ok(session_name)
}

/// Launch a shell command (with pipes, redirects, etc.) in a new tmux session.
///
/// Wraps the command in `sh -c` so shell features work.
///
/// # Errors
///
/// Returns `Err` if the working directory is invalid or if the underlying
/// `tmux new-session` invocation fails or exits non-zero.
pub fn create_session_shell(
    name: &str,
    working_dir: &str,
    shell_cmd: &str,
) -> Result<String, String> {
    if !session::validate_cwd(working_dir) {
        return Err(format!("Invalid working directory: {working_dir}"));
    }

    let base_name = sanitize_session_name(name);
    let session_name = unique_session_name(&base_name);

    let tmux_args = vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        session_name.clone(),
        "-c".to_string(),
        working_dir.to_string(),
        "sh".to_string(),
        "-c".to_string(),
        shell_cmd.to_string(),
    ];

    let status = Command::new("tmux")
        .args(&tmux_args)
        .status()
        .map_err(|error| format!("Failed to create tmux session: {error}"))?;

    if !status.success() {
        return Err("tmux new-session failed".to_string());
    }

    Ok(session_name)
}

/// Build a non-colliding tmux session name by appending `-2`, `-3`, ... when
/// the requested base already exists.
fn unique_session_name(base_name: &str) -> String {
    if !session_exists(base_name) {
        return base_name.to_string();
    }
    let mut suffix: u32 = 2;
    loop {
        let candidate = format!("{base_name}-{suffix}");
        if !session_exists(&candidate) {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}

/// Returns `true` if `tmux has-session -t <name>` succeeds.
fn session_exists(name: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", name])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Locate the `claude` binary on `PATH` via `which(1)`.
fn which_claude() -> Option<String> {
    let output = Command::new("which").arg("claude").output().ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// Kill a tmux session by name.
pub fn kill_session(name: &str) -> bool {
    Command::new("tmux")
        .args(["kill-session", "-t", name])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Sanitize a string for use as a tmux session name.
///
/// Uses an allowlist (alphanumeric, `-`, `_`) to prevent injection via
/// crafted directory names. Leading dashes are stripped to avoid flag
/// injection.
fn sanitize_session_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(
            |character| {
                if character.is_alphanumeric() || character == '_' {
                    character
                } else {
                    '-'
                }
            },
        )
        .collect();

    let trimmed = sanitized.trim_start_matches('-');

    if trimmed.is_empty() {
        "claude".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_session_name;

    /// # Panics
    /// Panics on assertion failure, by design (this is a unit test).
    #[test]
    fn sanitize_normal_name() {
        assert_eq!(sanitize_session_name("my-project"), "my-project");
        assert_eq!(sanitize_session_name("foo_bar"), "foo_bar");
    }

    /// # Panics
    /// Panics on assertion failure, by design (this is a unit test).
    #[test]
    fn sanitize_dots_and_colons() {
        assert_eq!(sanitize_session_name("my.project:1"), "my-project-1");
    }

    /// # Panics
    /// Panics on assertion failure, by design (this is a unit test).
    #[test]
    fn sanitize_shell_metacharacters() {
        assert_eq!(sanitize_session_name("$HOME;rm -rf /"), "HOME-rm--rf--");
    }

    /// # Panics
    /// Panics on assertion failure, by design (this is a unit test).
    #[test]
    fn sanitize_control_chars() {
        assert_eq!(sanitize_session_name("hello\x00\x1bworld"), "hello--world");
    }

    /// # Panics
    /// Panics on assertion failure, by design (this is a unit test).
    #[test]
    fn sanitize_leading_dashes_stripped() {
        assert_eq!(sanitize_session_name("--flag"), "flag");
        assert_eq!(sanitize_session_name("...name"), "name");
    }

    /// # Panics
    /// Panics on assertion failure, by design (this is a unit test).
    #[test]
    fn sanitize_all_special_becomes_claude() {
        assert_eq!(sanitize_session_name("..."), "claude");
        assert_eq!(sanitize_session_name(""), "claude");
    }

    /// # Panics
    /// Panics on assertion failure, by design (this is a unit test).
    #[test]
    fn sanitize_unicode_preserved() {
        assert_eq!(sanitize_session_name("café"), "café");
    }
}
