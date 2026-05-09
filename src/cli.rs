use clap::{Parser, Subcommand};

/// Monitor Claude Code sessions running in tmux (compact view).
#[derive(Parser)]
#[command(name = "roostr", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

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
}
