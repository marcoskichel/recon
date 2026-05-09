//! Shared helper for invoking the `tmux` binary and capturing stdout.

use std::io;
use std::process::Command as ProcCommand;

/// Run `tmux` with the given arguments and return trimmed stdout.
///
/// # Errors
/// Returns the underlying I/O error if the process cannot be spawned, or an
/// `io::Error::other` carrying the captured stderr if `tmux` exits non-zero.
pub fn tmux_call(args: &[&str]) -> io::Result<String> {
    let output = ProcCommand::new("tmux").args(args).output()?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(io::Error::other(message));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
