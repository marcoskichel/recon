//! Local `claude` CLI subprocess client for the summarizer.

use std::io::Write as _;
use std::process::{Command, Stdio};

use super::prompt::clean_label;

/// Default `claude` model identifier used when `ROOSTR_CLAUDE_MODEL` is unset.
pub(super) const CLAUDE_CLI_DEFAULT_MODEL: &str = "claude-haiku-4-5";
/// Default binary name used when `ROOSTR_CLAUDE_BINARY` is unset.
pub(super) const CLAUDE_CLI_DEFAULT_BINARY: &str = "claude";

/// Returns `true` when invoking `<binary> --version` exits successfully.
pub(super) fn claude_cli_available(binary: &str) -> bool {
    Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Run the `claude` CLI in `--print` mode and return a cleaned single-line label.
///
/// The flags below intentionally block project/local settings and MCP servers
/// so the CLI cannot load user-configured filesystem crawlers under the
/// daemon's launchd context (which would trigger macOS TCC prompts for
/// protected directories). Skills/hooks/plugins are also disabled here via the
/// empty `mcp-config` plus `strict-mcp-config` plus a scoped `setting-sources`.
pub(super) fn call_claude_cli(
    binary: &str,
    model: &str,
    system_prompt: &str,
    prompt: &str,
) -> Option<String> {
    let mut child = Command::new(binary)
        .args([
            "--print",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--mcp-config",
            r#"{"mcpServers":{}}"#,
            "--setting-sources",
            "user",
            "--model",
            model,
            "--system-prompt",
            system_prompt,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    {
        let stdin = child.stdin.as_mut()?;
        stdin.write_all(prompt.as_bytes()).ok()?;
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    clean_label(&text)
}
