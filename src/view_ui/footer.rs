//! Auxiliary single-line widgets sitting beneath the rooms area:
//! the search input, rename input, footer keymap, and the placeholder
//! sprites for the empty / loading states.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;
use crate::session::SessionStatus;

use super::sprites::{render_sprite_lines, sprite_data};

/// Render the `/`-prefixed search input bar.
pub(super) fn render_search_bar(frame: &mut Frame, dashboard: &App, area: Rect) {
    let mut spans = vec![
        Span::styled("/", Style::default().fg(Color::Cyan)),
        Span::raw(&dashboard.filter_text),
    ];
    if !dashboard.filter_active() && !dashboard.filter_text.is_empty() {
        let count = dashboard.filtered_indices().len();
        spans.push(Span::styled(
            format!("  ({count} match{})", if count == 1 { "" } else { "es" }),
            Style::default().fg(Color::DarkGray),
        ));
    }
    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);

    if dashboard.filter_active() {
        let cursor_x = u16::try_from(dashboard.filter_cursor).unwrap_or(u16::MAX);
        let position_x = area.x.saturating_add(1).saturating_add(cursor_x);
        frame.set_cursor_position((position_x, area.y));
    }
}

/// Render the rename input bar (only shown when [`App::rename_active`]).
pub(super) fn render_rename_bar(frame: &mut Frame, dashboard: &App, area: Rect) {
    let spans = vec![
        Span::styled("Rename: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(&dashboard.rename_text),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    if dashboard.rename_active() {
        let cursor_x = u16::try_from(dashboard.rename_cursor).unwrap_or(u16::MAX);
        let position_x = area.x.saturating_add(8).saturating_add(cursor_x);
        frame.set_cursor_position((position_x, area.y));
    }
}

/// Render the footer keymap line at the bottom of the dashboard.
pub(super) fn render_footer(frame: &mut Frame, dashboard: &App, area: Rect) {
    if let Some(message) = dashboard.active_status_message() {
        let line =
            Line::from(Span::styled(message.to_string(), Style::default().fg(Color::Yellow)));
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    if dashboard.view_zoomed_room.is_some() {
        push_zoomed_keys(&mut spans);
    } else {
        push_overview_keys(&mut spans);
    }
    push_common_keys(&mut spans);

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Append a hint of form `<KEY> <description>` to `spans`, in the
/// shared cyan/grey footer color scheme.
fn push_hint(spans: &mut Vec<Span<'static>>, label: &'static str, description: &'static str) {
    spans.push(Span::styled(label, Style::default().fg(Color::Cyan)));
    spans.push(Span::raw(description));
}

/// Footer key hints visible while a room is zoomed.
fn push_zoomed_keys(spans: &mut Vec<Span<'static>>) {
    push_hint(spans, "h/l", " select  ");
    push_hint(spans, "Enter", " switch  ");
    push_hint(spans, "x", " kill  ");
    push_hint(spans, "n", " new  ");
    push_hint(spans, "e", " editor  ");
    push_hint(spans, "t", " terminal  ");
    push_hint(spans, "g", " lazygit  ");
    push_hint(spans, "d", " diffnav  ");
    push_hint(spans, "D", " dash  ");
    push_hint(spans, "r", " rename  ");
}

/// Footer key hints visible in the room overview.
fn push_overview_keys(spans: &mut Vec<Span<'static>>) {
    push_hint(spans, "1-9", " select  ");
    push_hint(spans, "Enter", " switch  ");
    push_hint(spans, "e", " editor  ");
    push_hint(spans, "t", " terminal  ");
    push_hint(spans, "g", " lazygit  ");
    push_hint(spans, "d", " diffnav  ");
    push_hint(spans, "D", " dash  ");
    push_hint(spans, "r", " rename  ");
}

/// Footer key hints common to both views (search/next-input/quit).
fn push_common_keys(spans: &mut Vec<Span<'static>>) {
    push_hint(spans, "/", " search  ");
    push_hint(spans, "i", " next input  ");
    push_hint(spans, "q", " quit");
}

/// Render the "no active sessions" placeholder centered within `area`.
pub(super) fn render_empty(frame: &mut Frame, area: Rect, _tick: u64) {
    let (sprite, palette) = sprite_data(&SessionStatus::Idle, 0, 0);
    let mut lines: Vec<Line<'static>> = vec![Line::from("")];
    lines.extend(render_sprite_lines(sprite, palette));
    lines.push(Line::from(""));
    lines
        .push(Line::from(Span::styled("No active sessions", Style::default().fg(Color::DarkGray))));
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

/// Render the animated "loading…" placeholder centered within `area`.
pub(super) fn render_loading(frame: &mut Frame, area: Rect, tick: u64) {
    let frame_idx = usize::try_from(tick / 3).unwrap_or(0);
    let (sprite, palette) = sprite_data(&SessionStatus::Working, frame_idx, 0);
    let dots_count = usize::try_from((tick / 4) % 4).unwrap_or(0);
    let dots = ".".repeat(dots_count);
    let label = format!("Loading{dots:<3}");
    let mut lines: Vec<Line<'static>> = vec![Line::from("")];
    lines.extend(render_sprite_lines(sprite, palette));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(label, Style::default().fg(Color::DarkGray))));
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}
