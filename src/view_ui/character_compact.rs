//! Compact horizontal agent card: sprite on the left, info column on the
//! right, rounded border, used in the room-overview grid.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Padding, Paragraph},
    Frame,
};

use super::{
    animation::{animation_frame, session_phase_offset, status_color},
    context_bar::{session_permille, wide_context_bar},
    overlay::{render_agent_label, AgentLabelInputs},
    palettes::SPECIES_PALETTES,
    sprites::{render_sprite_lines, sprite_data},
    text::{
        agent_display_name, elapsed_hms, pick_species, sanitize_prompt, species_for, truncate_str,
        wrap_label,
    },
    types::{COMPACT_SPRITE_COLS, SPECIES_COUNT, SPRITE_RENDER_H},
};
use crate::{app::App, session::Session};

/// Color used for inactive (non-selected) compact card borders.
const INACTIVE_BORDER: Color = Color::Rgb(60, 60, 70);

/// Inputs to [`render_character_compact`], bundled to satisfy the
/// `too_many_arguments` ceiling.
pub(super) struct CompactParams<'frame> {
    /// Application state (used for species assignments and the summary store).
    pub dashboard: &'frame App,
    /// The session being rendered.
    pub session: &'frame Session,
    /// Card area in terminal cells.
    pub area: Rect,
    /// Global animation tick.
    pub tick: u64,
    /// `true` when the card should be drawn highlighted as the selection.
    pub is_selected: bool,
    /// 1-based agent label number `[N]`, if this slot has a digit shortcut.
    pub agent_label: Option<usize>,
}

/// Render the compact agent card.
pub(super) fn render_character_compact(frame: &mut Frame, params: &CompactParams) {
    let area = params.area;
    if area.height < 4 || area.width < COMPACT_SPRITE_COLS.saturating_add(6) {
        return;
    }
    let inner = render_compact_card(frame, area, params.is_selected);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    render_compact_body(frame, params, inner);
    if let Some(label_num) = params.agent_label {
        render_agent_label(
            frame,
            &AgentLabelInputs { area, index: label_num, color: Color::Yellow, x_offset: 1 },
        );
    }
}

/// Render the rounded-border card; return its inner rect.
fn render_compact_card(frame: &mut Frame, area: Rect, is_selected: bool) -> Rect {
    let border_color = if is_selected { Color::Cyan } else { INACTIVE_BORDER };
    let card = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1));
    let inner = card.inner(area);
    frame.render_widget(card, area);
    inner
}

/// Render the sprite + text columns inside the card's inner area.
fn render_compact_body(frame: &mut Frame, params: &CompactParams, inner: Rect) {
    let sprite_w = COMPACT_SPRITE_COLS.min(inner.width.saturating_sub(4));
    let chunks =
        Layout::horizontal([Constraint::Length(sprite_w), Constraint::Min(1)]).split(inner);

    if let Some(sprite_area) = chunks.first().copied() {
        render_compact_sprite(frame, params.session, sprite_area, params.tick);
    }
    if let Some(text_area) = chunks.get(1).copied() {
        let dot_color = status_color(&params.session.status);
        render_compact_text(
            frame,
            &CompactTextInputs {
                dashboard: params.dashboard,
                session: params.session,
                area: text_area,
                dot_color,
            },
        );
    }
}

/// Render the sprite portion of a compact card, vertically centering the
/// 5-line half-block sprite within `sprite_area`.
fn render_compact_sprite(frame: &mut Frame, session: &Session, sprite_area: Rect, tick: u64) {
    let offset = session_phase_offset(&session.id);
    let frame_idx = animation_frame(&session.status, tick.wrapping_add(offset));
    let species_seed = pick_species(&session.id);
    let (sprite, palette) = sprite_data(&session.status, frame_idx, species_seed);
    let lines = render_sprite_lines(sprite, palette);

    let sprite_pad = sprite_area.height.saturating_sub(SPRITE_RENDER_H) / 2;
    let sprite_rect = Rect {
        x: sprite_area.x,
        y: sprite_area.y.saturating_add(sprite_pad),
        width: sprite_area.width,
        height: SPRITE_RENDER_H.min(sprite_area.height),
    };
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), sprite_rect);
}

/// Inputs to the compact-card text column renderer.
struct CompactTextInputs<'frame> {
    /// Application state, used by name + description rendering.
    dashboard: &'frame App,
    /// Session whose information is shown.
    session: &'frame Session,
    /// Subregion of the card reserved for text.
    area: Rect,
    /// Status-dot color matching `session.status`.
    dot_color: Color,
}

/// Render the text portion (name, summary, branch, status, bar).
fn render_compact_text(frame: &mut Frame, inputs: &CompactTextInputs) {
    let &CompactTextInputs { dashboard, session, area, dot_color } = inputs;
    let text_w = usize::from(area.width);
    let mut lines: Vec<Line<'static>> = Vec::new();

    lines.push(name_line(dashboard, session, text_w));
    extend_description_lines(&mut lines, dashboard, session, text_w);
    lines.push(branch_line(session, text_w));
    lines.push(status_line(session, dot_color));
    lines.push(bar_line(session, text_w));

    frame.render_widget(Paragraph::new(lines), area);
}

/// Bold, species-colored display name on the first line.
fn name_line(dashboard: &App, session: &Session, max_width: usize) -> Line<'static> {
    let species = species_for(session, dashboard);
    let palette =
        SPECIES_PALETTES.get(species % SPECIES_COUNT).copied().unwrap_or(SPECIES_PALETTES[0]);
    let (channel_r, channel_g, channel_b) = palette.get(1).copied().unwrap_or((255, 255, 255));
    let display_name = agent_display_name(session, dashboard);
    Line::from(Span::styled(
        truncate_str(&display_name, max_width),
        Style::default()
            .fg(Color::Rgb(channel_r, channel_g, channel_b))
            .add_modifier(Modifier::BOLD),
    ))
}

/// Push two lines describing the session (LLM summary or last prompt).
///
/// Always pushes exactly two lines so card heights stay aligned across
/// rows in the grid.
fn extend_description_lines(
    lines: &mut Vec<Line<'static>>,
    dashboard: &App,
    session: &Session,
    max_width: usize,
) {
    let style = Style::default().fg(Color::Gray).add_modifier(Modifier::DIM);

    if !dashboard.summarizer.enabled() {
        lines.push(Line::from(""));
        lines.push(Line::from(""));
        return;
    }

    let summary = dashboard
        .summarizer
        .store
        .get(&session.id)
        .map(|stored| sanitize_prompt(stored.as_str()))
        .filter(|sanitized| !sanitized.is_empty());
    let prompt = session
        .last_user_prompt
        .as_deref()
        .map(sanitize_prompt)
        .filter(|sanitized| !sanitized.is_empty());

    let chosen = summary.as_deref().or(prompt.as_deref());
    if let Some(text) = chosen {
        let wrapped = wrap_label(text, max_width, 2);
        for line in wrapped.iter().take(2) {
            lines.push(Line::from(Span::styled(line.clone(), style)));
        }
        while lines.len() < 3 {
            lines.push(Line::from(""));
        }
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(""));
    }
}

/// Branch line in green.
fn branch_line(session: &Session, max_width: usize) -> Line<'static> {
    let branch = session.branch.as_deref().unwrap_or("");
    Line::from(Span::styled(truncate_str(branch, max_width), Style::default().fg(Color::Green)))
}

/// Status dot + label + uptime line.
fn status_line(session: &Session, dot_color: Color) -> Line<'static> {
    let timer = elapsed_hms(session.started_at);
    let status_label = session.status.label();
    Line::from(vec![
        Span::styled("\u{25CF} ", Style::default().fg(dot_color)),
        Span::styled(status_label.to_string(), Style::default().fg(Color::White)),
        Span::raw("   "),
        Span::styled("\u{29D6} ", Style::default().fg(Color::DarkGray)),
        Span::styled(timer, Style::default().fg(Color::Gray)),
    ])
}

/// Width-adaptive token-usage bar.
fn bar_line(session: &Session, max_width: usize) -> Line<'static> {
    let permille = session_permille(session);
    let (bar_spans, _color) = wide_context_bar(permille, max_width);
    Line::from(bar_spans)
}
