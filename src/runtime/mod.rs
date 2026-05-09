//! Subcommand entry points and the main TUI / dock event loops.
//!
//! `main.rs` only does CLI parsing and dispatch; everything below is the
//! per-command runtime: tmux helpers, daemon polling, the interactive TUI,
//! and the compact dock variant.

pub mod daemon;
pub mod dock;
pub mod dock_focus;
pub mod dock_info;
pub mod dock_toggle;
pub mod toggle;
pub mod tui;

mod refresh;
mod tmux_helper;
