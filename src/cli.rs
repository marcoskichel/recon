//! Command-line interface definitions for the `roostr` binary.

use clap::{Parser, Subcommand};

/// Monitor Claude Code sessions running in tmux (compact view).
#[derive(Parser)]
#[command(name = "roostr", version)]
pub struct CliArgs {
    /// Optional subcommand; when omitted the TUI is launched.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Top-level subcommands exposed by the `roostr` binary.
#[derive(Subcommand)]
pub enum Command {
    /// Run summarizer in background. Polls active claude sessions, enqueues
    /// new transcripts to local LLM, persists labels to ~/.cache/roostr/labels.
    Daemon {
        /// Poll interval seconds (default 10).
        #[arg(long, default_value_t = 10u64)]
        interval: u64,
    },
    /// Show a compact dock: one mini-sprite per session. Designed to run in
    /// a small tmux pane. Press q to quit.
    Dock,
    /// Toggle the dock pane (sidebar) in the current tmux window.
    /// Spawns it on the right if missing, kills it if present.
    DockToggle,
    /// Focus the dock pane in the current tmux window. Spawns it if
    /// missing. Use this for a "jump-to-sidebar" keybind.
    DockFocus,
    /// Print formatted session details, then wait for a keypress.
    /// Designed to run inside `tmux display-popup` — invoked by the
    /// dock when the user presses `i` on a selected card.
    DockInfo {
        /// Session id (from ~/.claude/projects/*.jsonl).
        session_id: String,
    },
    /// Toggle a `roostr` tmux window in the current session: focus it if
    /// it exists elsewhere, kill it if already focused, or create it.
    /// Designed for a single-keystroke binding (e.g. `C-y`).
    Toggle,
    /// One-command install for tmux keybindings and the daemon service.
    Setup {
        /// Which install/uninstall step to run.
        #[command(subcommand)]
        action: SetupAction,
    },
}

/// Sub-actions for `roostr setup`.
#[derive(Subcommand)]
pub enum SetupAction {
    /// Install tmux keybindings (writes ~/.config/roostr/tmux.conf and sources it from ~/.tmux.conf).
    Tmux {
        /// Overwrite an existing config without prompting.
        #[arg(long)]
        force: bool,
    },
    /// Install daemon as a user service (systemd on Linux, launchd on macOS).
    Daemon {
        /// Overwrite an existing service unit without prompting.
        #[arg(long)]
        force: bool,
        /// Poll interval seconds (default 10).
        #[arg(long, default_value_t = 10u64)]
        interval: u64,
    },
    /// Install tmux config (and optionally the daemon with --with-daemon).
    /// Daemon is opt-in: pass --with-daemon to also install the background service.
    #[command(name = "all")]
    Everything {
        /// Overwrite existing files without prompting.
        #[arg(long)]
        force: bool,
        /// Poll interval seconds passed through to the daemon installer.
        #[arg(long, default_value_t = 10u64)]
        interval: u64,
        /// Also install the summarizer daemon as a user service.
        #[arg(long)]
        with_daemon: bool,
    },
    /// Reverse install: remove sourced line, delete unit, disable service.
    Uninstall,
}
