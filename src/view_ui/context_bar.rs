//! Token-usage progress bar primitives.
//!
//! Two flavors are exposed: a fixed 6-cell bar with trailing percentage
//! used in the non-compact agent card, and a width-adaptive bar with a
//! styled trailing percentage used in the compact card.
//!
//! All math is integer permille (parts-per-thousand) to avoid both the
//! crate's `clippy::float_arithmetic` denial and lossy `as` casts.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

use crate::{model, session::Session};

/// Width of the fixed-size bar produced by [`context_bar`], in cells.
const FIXED_BAR_WIDTH: usize = 6;

/// Compute token usage of `session` in permille (`0..=1000`).
///
/// Permille is preferred over a float ratio so callers can render bars
/// and percentages with pure integer arithmetic.
pub(super) fn session_permille(session: &Session) -> u32 {
    let used = session.total_input_tokens.saturating_add(session.total_output_tokens);
    let window = session.model.as_deref().map_or(200_000_u64, model::context_window);
    if window == 0 {
        return 0;
    }
    let scaled = used.saturating_mul(1000);
    let permille_u64 = scaled.checked_div(window).unwrap_or(0);
    u32::try_from(permille_u64.min(1000)).unwrap_or(1000)
}

/// Render a fixed-width string bar of `FIXED_BAR_WIDTH` cells plus a `nn%` suffix.
pub(super) fn context_bar(permille: u32) -> (String, Color) {
    let filled = filled_cells(permille, FIXED_BAR_WIDTH);
    let empty = FIXED_BAR_WIDTH.saturating_sub(filled);
    let percent = percent_label(permille);
    let rendered = format!("{}{} {percent}%", "\u{2588}".repeat(filled), "\u{2591}".repeat(empty));
    (rendered, threshold_color(permille))
}

/// Render a context bar that fills `total_width` cells, sharing the row
/// with a trailing percentage label.
///
/// Returns the styled spans plus the active threshold color so callers
/// can match other elements (e.g. text) to it.
pub(super) fn wide_context_bar(permille: u32, total_width: usize) -> (Vec<Span<'static>>, Color) {
    let percent = percent_label(permille);
    let pct_str = format!(" {percent}%");
    let pct_len = pct_str.chars().count();
    let bar_width = total_width.saturating_sub(pct_len.saturating_add(1)).max(1);
    let filled = filled_cells(permille, bar_width);
    let empty = bar_width.saturating_sub(filled);
    let color = threshold_color(permille);
    let dimmed = Color::Rgb(60, 60, 60);
    let spans = vec![
        Span::styled("\u{2588}".repeat(filled), Style::default().fg(color)),
        Span::styled("\u{2588}".repeat(empty), Style::default().fg(dimmed)),
        Span::styled(pct_str, Style::default().fg(color).add_modifier(Modifier::BOLD)),
    ];
    (spans, color)
}

/// Threshold color for an integer permille usage value.
pub(super) const fn threshold_color(permille: u32) -> Color {
    if permille > 750 {
        Color::Red
    } else if permille > 400 {
        Color::Yellow
    } else {
        Color::Green
    }
}

/// Number of filled cells in a bar of `width` cells given an integer permille.
///
/// Uses standard rounding (`round-half-up`) implemented in pure integer math.
pub(super) fn filled_cells(permille: u32, width: usize) -> usize {
    if width == 0 {
        return 0;
    }
    let width_u32 = u32::try_from(width).unwrap_or(u32::MAX);
    let numerator = u64::from(permille).saturating_mul(u64::from(width_u32)).saturating_add(500);
    let cells_u64 = numerator / 1000;
    let cells = usize::try_from(cells_u64).unwrap_or(width);
    cells.min(width)
}

/// Truncate a permille to its integer percentage label (saturating at 100).
pub(super) const fn percent_label(permille: u32) -> u32 {
    let label = match permille.checked_div(10) {
        Some(value) => value,
        None => 0,
    };
    if label > 100 {
        100
    } else {
        label
    }
}
