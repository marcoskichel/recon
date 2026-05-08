use clap::{Parser, Subcommand};

/// Monitor Claude Code sessions running in tmux (compact view).
#[derive(Parser)]
#[command(name = "recon", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run summarizer in background. Polls active claude sessions, enqueues
    /// new transcripts to local LLM, persists labels to ~/.cache/recon/labels.
    Daemon {
        /// Poll interval seconds (default 10).
        #[arg(long, default_value_t = 10u64)]
        interval: u64,
    },
}
