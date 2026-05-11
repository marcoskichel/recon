//! `roostr` binary entry point: parses CLI flags and dispatches to the
//! appropriate `runtime::run_*` function.

mod app;
mod cli;
mod model;
mod runtime;
mod session;
mod setup;
mod state;
mod summarizer;
mod tmux;
mod view_lock;
mod view_ui;

use std::io;

use clap::Parser;

/// Parse `argv` and run the requested subcommand.
///
/// # Errors
/// Returns whichever I/O error bubbles out of the chosen subcommand.
fn main() -> io::Result<()> {
    let args = cli::CliArgs::parse();

    match args.command {
        Some(cli::Command::Daemon { interval }) => runtime::daemon::run_daemon(interval),
        Some(cli::Command::Dock) => runtime::dock::run_dock(),
        Some(cli::Command::DockToggle) => runtime::dock_toggle::run_dock_toggle(),
        Some(cli::Command::DockFocus) => runtime::dock_focus::run_dock_focus(),
        Some(cli::Command::DockInfo { session_id }) => {
            runtime::dock_info::run_dock_info(&session_id)
        }
        Some(cli::Command::Toggle) => runtime::toggle::run_toggle(),
        Some(cli::Command::Setup { action }) => setup::execute(&action),
        None => runtime::tui::run_tui(),
    }
}
