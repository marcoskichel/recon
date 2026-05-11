//! Sidebar dock view: a vertical stack of mini-cards for very narrow panes.
//!
//! Each dock card shows a sextant-compact sprite, a thin token-usage bar,
//! and a slot label `[N]` overlaid on the top border.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Padding, Paragraph},
    Frame,
};

use super::{
    animation::{animation_frame, session_phase_offset},
    context_bar::{session_permille, threshold_color},
    footer::render_search_bar,
    rooms::group_into_rooms_stable,
    sprites::{render_sprite_compact, sprite_data},
    text::pick_species,
    types::{DOCK_CARD_H, DOCK_CARD_W, MINI_SPRITE_H},
};
use crate::{
    app::App,
    session::{Session, SessionStatus},
};

/// Color used for inactive card borders in the dock view.
const DOCK_BORDER_INACTIVE: Color = Color::Rgb(60, 60, 70);
/// Color used for the trailing portion of the thin token bar.
const DOCK_BAR_REST: Color = Color::Rgb(60, 60, 70);

/// Render the sidebar dock view.
///
/// Lays out as many cards as the pane height permits, scrolling is not
/// supported (the dock is intentionally fixed-size).
pub fn render_dock(frame: &mut Frame, dashboard: &App) {
    let full = frame.area();
    let show_search = dashboard.filter_active() || !dashboard.filter_text.is_empty();
    let chunks = if show_search {
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(full)
    } else {
        Layout::vertical([Constraint::Min(1)]).split(full)
    };
    let Some(dock_full) = chunks.first().copied() else {
        return;
    };

    let dock_w = DOCK_CARD_W.min(dock_full.width);
    let area = Rect { x: dock_full.x, y: dock_full.y, width: dock_w, height: dock_full.height };

    let filtered = dashboard.filtered_indices();
    let rooms = group_into_rooms_stable(&dashboard.sessions, &filtered, &dashboard.view_room_order);
    let indices: Vec<usize> =
        rooms.into_iter().flat_map(|room| room.session_indices.into_iter()).collect();

    if indices.is_empty() {
        let label = if dashboard.loaded { "no sessions" } else { "loading\u{2026}" };
        frame.render_widget(
            Paragraph::new(Span::styled(label, Style::default().fg(Color::DarkGray)))
                .alignment(Alignment::Center),
            area,
        );
        if show_search {
            if let Some(rect) = chunks.get(1).copied() {
                render_search_bar(frame, dashboard, rect);
            }
        }
        return;
    }

    render_card_stack(frame, dashboard, area, &indices);

    if show_search {
        if let Some(rect) = chunks.get(1).copied() {
            render_search_bar(frame, dashboard, rect);
        }
    }
}

/// Stack as many [`render_dock_card`] cards as fit in `area`.
fn render_card_stack(frame: &mut Frame, dashboard: &App, area: Rect, indices: &[usize]) {
    if DOCK_CARD_H == 0 {
        return;
    }
    let height = area.height;
    let max_cards = (height / DOCK_CARD_H).max(1);
    let max_cards_us = usize::from(max_cards);
    let card_count = max_cards_us.min(indices.len());
    let constraints: Vec<Constraint> =
        (0..card_count).map(|_| Constraint::Length(DOCK_CARD_H)).collect();
    let cards = Layout::vertical(constraints).split(area);

    let last_idx = indices.len().saturating_sub(1);
    let selected = dashboard.view_selected_agent.min(last_idx);

    for (slot, &session_idx) in indices.iter().take(card_count).enumerate() {
        let Some(session) = dashboard.sessions.get(session_idx) else {
            continue;
        };
        let Some(rect) = cards.get(slot).copied() else {
            continue;
        };
        let slot_label = slot.saturating_add(1);
        render_dock_card(
            frame,
            &DockCardInputs {
                session,
                area: rect,
                tick: dashboard.tick,
                index: slot_label,
                is_selected: slot == selected,
            },
        );
    }
}

/// Inputs to [`render_dock_card`].
struct DockCardInputs<'frame> {
    /// Session to render.
    session: &'frame Session,
    /// Card area in terminal cells.
    area: Rect,
    /// Global animation tick.
    tick: u64,
    /// 1-based slot number rendered on the top border.
    index: usize,
    /// Whether this is the active selection.
    is_selected: bool,
}

/// Render a single dock card.
fn render_dock_card(frame: &mut Frame, inputs: &DockCardInputs) {
    let &DockCardInputs { session, area, tick, index, is_selected } = inputs;
    if area.width < 5 || area.height < 4 {
        return;
    }

    let border_color = dock_border_color(&session.status, tick, is_selected);
    let card = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1));
    let inner = card.inner(area);
    frame.render_widget(card, area);

    if inner.width < 3 || inner.height < 3 {
        return;
    }

    let chunks =
        Layout::vertical([Constraint::Length(MINI_SPRITE_H), Constraint::Length(1)]).split(inner);

    if let Some(sprite_rect) = chunks.first().copied() {
        let offset = session_phase_offset(&session.id);
        let anim_tick = tick.wrapping_add(offset);
        let frame_idx = animation_frame(&session.status, anim_tick);
        let species = pick_species(&session.id);
        let (sprite, palette) = sprite_data(&session.status, frame_idx, species);
        let lines = render_sprite_compact(sprite, palette);
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), sprite_rect);
    }

    if let Some(bar_rect) = chunks.get(1).copied() {
        render_thin_bar(frame, session, bar_rect);
    }

    render_slot_label(frame, area, index, is_selected);
}

/// Pick the dock card's border color, including the input pulse animation.
const fn dock_border_color(status: &SessionStatus, tick: u64, is_selected: bool) -> Color {
    if matches!(status, SessionStatus::Input) {
        if tick.is_multiple_of(2) {
            Color::Yellow
        } else {
            Color::White
        }
    } else if is_selected {
        Color::Cyan
    } else {
        DOCK_BORDER_INACTIVE
    }
}

/// Render the thin token-usage bar at the bottom of a dock card.
fn render_thin_bar(frame: &mut Frame, session: &Session, area: Rect) {
    let permille = session_permille(session);
    let bar_color = threshold_color(permille);
    let bar_w = usize::from(area.width);
    let filled = super::context_bar::filled_cells(permille, bar_w);
    let empty = bar_w.saturating_sub(filled);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("\u{2581}".repeat(filled), Style::default().fg(bar_color)),
            Span::styled("\u{2581}".repeat(empty), Style::default().fg(DOCK_BAR_REST)),
        ])),
        area,
    );
}

/// Render the `[N]` overlay on the top border of a dock card.
fn render_slot_label(frame: &mut Frame, area: Rect, index: usize, is_selected: bool) {
    let label = format!("[{index}]");
    let label_chars = label.chars().count();
    let label_chars_u16 = u16::try_from(label_chars).unwrap_or(u16::MAX);
    let label_w = label_chars_u16.min(area.width.saturating_sub(2));
    if label_w == 0 {
        return;
    }
    let label_rect = Rect { x: area.x.saturating_add(1), y: area.y, width: label_w, height: 1 };
    let style = Style::default()
        .fg(if is_selected { Color::Cyan } else { Color::White })
        .add_modifier(Modifier::BOLD);
    frame.render_widget(Paragraph::new(label).style(style), label_rect);
}
