//! Grid layout for agent cards within a room block.
//!
//! Stacks rows of cards vertically inside the room's inner area, each row
//! laying out cards horizontally. Cards are rendered as either compact
//! horizontal cards (the overview view) or full-size cards (zoomed view).

use ratatui::{
    layout::{Constraint, Layout, Rect},
    Frame,
};

use crate::app::App;

use super::character::{render_character, CharacterParams};
use super::character_compact::{render_character_compact, CompactParams};
use super::rooms::Room;
use super::types::{CHAR_HEIGHT, CHAR_WIDTH, COMPACT_CARD_HEIGHT, COMPACT_CARD_WIDTH};

/// Highest agent label number that gets a digit shortcut (`1..=9`).
const MAX_LABEL: usize = 9;

/// Inputs to [`render_card_grid`].
pub(super) struct CardGridInputs<'frame> {
    /// Application state (used to look up sessions and the global tick).
    pub dashboard: &'frame App,
    /// Room whose session indices populate the grid.
    pub room: &'frame Room,
    /// Inner area of the room block (after border + padding).
    pub inner: Rect,
    /// Index of the selected agent within the flat grid, if any.
    pub selected_agent: Option<usize>,
    /// Offset added to flat indices when computing the global agent label.
    pub agent_label_offset: Option<usize>,
    /// `true` for the compact horizontal card style; `false` for full-size.
    pub compact: bool,
}

/// Lay out and render the grid of agent cards inside a room block.
pub(super) fn render_card_grid(frame: &mut Frame, inputs: &CardGridInputs) {
    let card_width = if inputs.compact { COMPACT_CARD_WIDTH } else { CHAR_WIDTH };
    let card_height = if inputs.compact { COMPACT_CARD_HEIGHT } else { CHAR_HEIGHT };

    let chars_per_row_u16 = inputs.inner.width.checked_div(card_width).unwrap_or(1).max(1);
    let chars_per_row = usize::from(chars_per_row_u16);
    let char_rows: Vec<&[usize]> = inputs.room.session_indices.chunks(chars_per_row).collect();

    let char_area = compute_grid_area(inputs.inner, char_rows.len(), card_height);

    let row_constraints: Vec<Constraint> =
        char_rows.iter().map(|_| Constraint::Length(card_height)).collect();
    let v_chunks = Layout::vertical(row_constraints).split(char_area);

    for (row_idx, indices) in char_rows.iter().enumerate() {
        let Some(row_rect) = v_chunks.get(row_idx).copied() else {
            break;
        };
        render_card_row(
            frame,
            inputs,
            &CardRowInputs { indices, row_rect, row_idx, chars_per_row, card_width },
        );
    }
}

/// Compute the inner rectangle used for grid rows, vertically centering
/// the visible cards within the room's inner area.
fn compute_grid_area(inner: Rect, row_count: usize, card_height: u16) -> Rect {
    let needed_height_us = row_count.saturating_mul(usize::from(card_height));
    let needed_height = u16::try_from(needed_height_us).unwrap_or(u16::MAX);
    let v_pad = inner.height.saturating_sub(needed_height) / 2;
    Rect {
        x: inner.x,
        y: inner.y.saturating_add(v_pad),
        width: inner.width,
        height: inner.height.saturating_sub(v_pad),
    }
}

/// Inputs to [`render_card_row`].
struct CardRowInputs<'row> {
    /// Session indices that belong to this row.
    indices: &'row [usize],
    /// Rectangle reserved for the row.
    row_rect: Rect,
    /// Row index within the grid (used for flat-index math).
    row_idx: usize,
    /// Cards per row at this width.
    chars_per_row: usize,
    /// Per-card width in cells.
    card_width: u16,
}

/// Render a single row of agent cards.
fn render_card_row<'frame>(
    frame: &mut Frame,
    grid: &CardGridInputs<'frame>,
    current: &CardRowInputs<'frame>,
) {
    let col_constraints: Vec<Constraint> =
        current.indices.iter().map(|_| Constraint::Length(current.card_width)).collect();
    let h_chunks = Layout::horizontal(col_constraints).split(current.row_rect);

    for (col_idx, &session_idx) in current.indices.iter().enumerate() {
        let Some(card_rect) = h_chunks.get(col_idx).copied() else {
            break;
        };
        let flat_idx =
            current.row_idx.saturating_mul(current.chars_per_row).saturating_add(col_idx);
        render_one_card(frame, grid, &SingleCardInputs { session_idx, card_rect, flat_idx });
    }
}

/// Inputs to [`render_one_card`].
struct SingleCardInputs {
    /// Index into `dashboard.sessions` for the agent.
    session_idx: usize,
    /// Rectangle reserved for the card.
    card_rect: Rect,
    /// Flat (row-major) index of the card within the grid.
    flat_idx: usize,
}

/// Render a single card at `card.card_rect` within the grid.
fn render_one_card(frame: &mut Frame, grid: &CardGridInputs, card: &SingleCardInputs) {
    let &SingleCardInputs { session_idx, card_rect, flat_idx } = card;
    let is_selected = grid.selected_agent == Some(flat_idx);
    let label = grid.agent_label_offset.and_then(|base| {
        let global = base.saturating_add(flat_idx);
        if global < MAX_LABEL {
            Some(global.saturating_add(1))
        } else {
            None
        }
    });
    let Some(session) = grid.dashboard.sessions.get(session_idx) else {
        return;
    };
    if grid.compact {
        render_character_compact(
            frame,
            &CompactParams {
                dashboard: grid.dashboard,
                session,
                area: card_rect,
                tick: grid.dashboard.tick,
                is_selected,
                agent_label: label,
            },
        );
    } else {
        render_character(
            frame,
            &CharacterParams {
                dashboard: grid.dashboard,
                session,
                area: card_rect,
                tick: grid.dashboard.tick,
                is_selected,
                agent_label: label,
            },
        );
    }
}
