//! `roostr dock-focus` — spawn or focus the dock pane in the current window.

use std::io::{self, Write};

use super::tmux_helper::tmux_call;

/// tmux format spec for the active window id.
///
/// Defined as a constant so clippy's `literal_string_with_formatting_args`
/// lint does not mistake the `#{ ... }` syntax for a Rust formatting arg.
const TMUX_WINDOW_ID_FORMAT: &str = "#{window_id}";

/// tmux format spec for `list-panes`: emits `pane_id pane_title` per pane.
const TMUX_PANE_LIST_FORMAT: &str = "#{pane_id} #{pane_title}";

/// Find the existing dock pane (titled `roostr-dock`) in the given window.
///
/// # Errors
/// Returns the I/O error from the underlying `tmux` invocation.
fn find_dock_pane(window_id: &str) -> io::Result<Option<String>> {
    let panes = tmux_call(&["list-panes", "-t", window_id, "-F", TMUX_PANE_LIST_FORMAT])?;
    Ok(panes.lines().find_map(|line| {
        let mut parts = line.splitn(2, ' ');
        let pane_id = parts.next()?;
        let title = parts.next().unwrap_or("");
        if title == "roostr-dock" {
            Some(pane_id.to_string())
        } else {
            None
        }
    }))
}

/// Focus the dock pane in the current tmux window, spawning it if missing.
///
/// # Errors
/// Returns the underlying I/O error from `tmux` invocations.
pub fn run_dock_focus() -> io::Result<()> {
    if std::env::var_os("TMUX").is_none() {
        let mut stderr = io::stderr();
        writeln!(stderr, "roostr dock-focus: not inside tmux")?;
        std::process::exit(1);
    }

    let window_id = tmux_call(&["display-message", "-p", TMUX_WINDOW_ID_FORMAT])?;
    let dock_pane = find_dock_pane(&window_id)?;

    if let Some(pane_id) = dock_pane {
        tmux_call(&["select-pane", "-t", &pane_id])?;
    } else {
        // No -d so the new pane takes focus.
        tmux_call(&["split-window", "-h", "-l", "9", "-t", &window_id, "roostr dock"])?;
    }
    Ok(())
}
