//! `roostr toggle` — toggle a `roostr` window in the current tmux session.

use std::io::{self, Write};

use super::tmux_helper::tmux_call;

/// Toggle a `roostr` tmux window: focus, kill if focused, or create.
///
/// # Errors
/// Returns the underlying I/O error from `tmux` invocations.
pub fn run_toggle() -> io::Result<()> {
    if std::env::var_os("TMUX").is_none() {
        let mut stderr = io::stderr();
        writeln!(stderr, "roostr toggle: not inside tmux")?;
        std::process::exit(1);
    }

    let current_win = tmux_call(&["display-message", "-p", "#{window_id}"])?;
    let current_name = tmux_call(&["display-message", "-p", "#{window_name}"])?;
    let windows = tmux_call(&["list-windows", "-F", "#{window_id} #{window_name}"])?;

    let roostr_win = windows.lines().find_map(|line| {
        let mut parts = line.splitn(2, ' ');
        let win_id = parts.next()?;
        let name = parts.next().unwrap_or("");
        if name == "roostr" {
            Some(win_id.to_string())
        } else {
            None
        }
    });

    match roostr_win {
        Some(win_id) if current_name == "roostr" || win_id == current_win => {
            tmux_call(&["kill-window", "-t", &win_id])?;
        }
        Some(win_id) => {
            tmux_call(&["select-window", "-t", &win_id])?;
        }
        None => {
            tmux_call(&["new-window", "-n", "roostr", "roostr"])?;
        }
    }
    Ok(())
}
