//! Full-size agent card: pixel-art sprite stacked above name, summary,
//! branch, and a tokens-used progress bar.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use super::{
    animation::{animation_frame, session_phase_offset},
    context_bar::{context_bar, session_permille},
    overlay::{render_agent_label, AgentLabelInputs},
    palettes::SPECIES_PALETTES,
    sprites::{render_sprite_lines, sprite_data},
    text::{
        agent_display_name, pick_species, sanitize_prompt, species_for, truncate_str, wrap_label,
    },
    types::SPECIES_COUNT,
};
use crate::{app::App, session::Session};

/// Background color for the selected card highlight.
const SELECTED_BG: Color = Color::Rgb(40, 40, 60);

/// Inputs to the full-size agent card renderer.
///
/// Bundled into a struct so the function fits the project's
/// 4-argument ceiling on `too_many_arguments`.
pub(super) struct CharacterParams<'frame> {
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

/// Render the non-compact agent card.
pub(super) fn render_character(frame: &mut Frame, params: &CharacterParams) {
    let area = params.area;
    if area.height < 3 || area.width < 4 {
        return;
    }

    if params.is_selected {
        let backdrop = Block::default().style(Style::default().bg(SELECTED_BG));
        frame.render_widget(backdrop, area);
    }

    let lines = build_character_lines(params);
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);

    if let Some(label_num) = params.agent_label {
        render_agent_label(
            frame,
            &AgentLabelInputs { area, index: label_num, color: Color::Yellow, x_offset: 0 },
        );
    }
}

/// Build the vertical line stack for the non-compact agent card.
fn build_character_lines(params: &CharacterParams) -> Vec<Line<'static>> {
    let dashboard = params.dashboard;
    let session = params.session;
    let area = params.area;
    let tick = params.tick;
    let is_selected = params.is_selected;
    let offset = session_phase_offset(&session.id);
    let frame_idx = animation_frame(&session.status, tick.wrapping_add(offset));
    let species_seed = pick_species(&session.id);
    let (sprite, palette) = sprite_data(&session.status, frame_idx, species_seed);
    let permille = session_permille(session);

    let area_width = usize::from(area.width);

    let mut lines: Vec<Line<'static>> = render_sprite_lines(sprite, palette);
    lines.push(name_line(dashboard, session, area_width));
    extend_with_description(DescriptionInputs {
        lines: &mut lines,
        dashboard,
        session,
        max_width: area_width,
        is_selected,
    });
    lines.push(branch_line(session, area_width));
    lines.push(Line::from(""));
    lines.push(context_bar_line(permille, area_width));
    lines
}

/// One line styled in the species' primary color, holding the display name.
fn name_line(dashboard: &App, session: &Session, max_width: usize) -> Line<'static> {
    let species = species_for(session, dashboard);
    let palette_idx = species % SPECIES_COUNT;
    let palette = SPECIES_PALETTES.get(palette_idx).copied().unwrap_or(SPECIES_PALETTES[0]);
    let (channel_r, channel_g, channel_b) = palette.get(1).copied().unwrap_or((255, 255, 255));
    let display_name = agent_display_name(session, dashboard);
    Line::from(Span::styled(
        truncate_str(&display_name, max_width),
        Style::default()
            .fg(Color::Rgb(channel_r, channel_g, channel_b))
            .add_modifier(Modifier::BOLD),
    ))
}

/// Inputs to [`extend_with_description`].
struct DescriptionInputs<'frame> {
    /// Buffer to push lines onto.
    lines: &'frame mut Vec<Line<'static>>,
    /// Application state (used to access the summarizer store).
    dashboard: &'frame App,
    /// Session whose summary or prompt should be rendered.
    session: &'frame Session,
    /// Card width in cells, used for word-wrapping.
    max_width: usize,
    /// Whether the parent card is the selected one (changes color).
    is_selected: bool,
}

/// Append the (optional) summary/prompt description, padding to two lines.
fn extend_with_description(inputs: DescriptionInputs) {
    let DescriptionInputs { lines, dashboard, session, max_width, is_selected } = inputs;

    let style = if is_selected {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(Color::Gray).add_modifier(Modifier::DIM)
    };

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
        // Pad to a stable height: sprite (5) + name (1) + description (2) = 8 lines.
        while lines.len() < 8 {
            lines.push(Line::from(""));
        }
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(""));
    }
}

/// One line styled green, holding the (truncated) git branch.
fn branch_line(session: &Session, max_width: usize) -> Line<'static> {
    let branch = session.branch.as_deref().unwrap_or("");
    Line::from(Span::styled(truncate_str(branch, max_width), Style::default().fg(Color::Green)))
}

/// Token-bar line with usage-threshold coloring.
fn context_bar_line(permille: u32, max_width: usize) -> Line<'static> {
    let (bar_str, bar_color) = context_bar(permille);
    Line::from(Span::styled(truncate_str(&bar_str, max_width), Style::default().fg(bar_color)))
}
