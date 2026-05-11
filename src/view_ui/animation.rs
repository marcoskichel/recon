//! Animation timing helpers.
//!
//! Sprite animation cycles every 3 frames, ticked by `App::tick`. To avoid
//! every agent breathing in lock-step (visually distracting), each session
//! gets a small per-id phase offset.

use ratatui::style::Color;

use crate::session::SessionStatus;

/// Map a global tick to the active animation frame index for `status`.
///
/// `Working` advances every two ticks (slower breathing), `Input` every
/// tick (more urgent), all others stay on frame 0.
pub(super) fn animation_frame(status: &SessionStatus, tick: u64) -> usize {
    match *status {
        SessionStatus::Working => usize::try_from((tick / 2) % 3).unwrap_or(0),
        SessionStatus::Input => usize::try_from(tick % 3).unwrap_or(0),
        SessionStatus::New | SessionStatus::Idle => 0,
    }
}

/// Per-session phase offset in `0..=6`, derived from `session_id` bytes.
///
/// Adding this to the global tick desynchronizes animations between agents.
pub(super) fn session_phase_offset(session_id: &str) -> u64 {
    session_id.bytes().fold(0_u64, |total, byte| total.wrapping_add(u64::from(byte))) % 7
}

/// Map a status to its short status-dot color.
pub(super) const fn status_color(status: &SessionStatus) -> Color {
    match *status {
        SessionStatus::New => Color::Blue,
        SessionStatus::Working => Color::Green,
        SessionStatus::Idle => Color::DarkGray,
        SessionStatus::Input => Color::Yellow,
    }
}
