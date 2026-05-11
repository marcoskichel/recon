//! Room-level layout: stacks rooms vertically, lays out agent cards in a
//! grid within each room, and dispatches to the per-card renderer.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Padding},
    Frame,
};

use super::{
    card_grid::{render_card_grid, CardGridInputs},
    footer::{render_empty, render_loading},
    rooms::{group_into_rooms_stable, Room},
    types::{COMPACT_CARD_HEIGHT, COMPACT_CARD_WIDTH},
};
use crate::app::App;

/// Vertical gap between adjacent rooms in the stacked overview.
const ROOM_GAP: u16 = 1;
/// Top + bottom border consumption per room block.
const ROOM_BORDER_OVERHEAD: u16 = 2;

/// Inputs to [`render_room`], bundled to satisfy the `too_many_arguments`
/// ceiling.
struct RenderRoomParams<'frame> {
    /// The room being rendered.
    room: &'frame Room,
    /// Where to render in terminal cells.
    area: Rect,
    /// Optional `[N]` slot label drawn in the title.
    slot_num: Option<usize>,
    /// Index of the selected agent within the room's flat card grid.
    selected_agent: Option<usize>,
    /// Offset added to per-room indices when computing the global
    /// agent label number.
    agent_label_offset: Option<usize>,
    /// Compact mode (`true`) vs. classic mode (`false`).
    compact: bool,
}

/// Top-level rooms renderer; routes to either the zoomed single-room
/// view or the stacked overview.
pub(super) fn render_rooms(frame: &mut Frame, dashboard: &App, area: Rect) {
    let rooms = group_into_rooms_stable(
        &dashboard.sessions,
        &dashboard.filtered_indices(),
        &dashboard.view_room_order,
    );

    if rooms.is_empty() {
        if dashboard.loaded {
            render_empty(frame, area, dashboard.tick);
        } else {
            render_loading(frame, area, dashboard.tick);
        }
        return;
    }

    if let Some(zoomed_name) = dashboard.view_zoomed_room.as_ref() {
        if let Some(room) = rooms.iter().find(|candidate| &candidate.name == zoomed_name) {
            render_room(
                frame,
                dashboard,
                &RenderRoomParams {
                    room,
                    area,
                    slot_num: None,
                    selected_agent: Some(dashboard.view_selected_agent),
                    agent_label_offset: None,
                    compact: false,
                },
            );
            return;
        }
    }

    render_rooms_stacked(frame, dashboard, &rooms, area);
}

/// Stacked-rooms overview. Computes per-room heights, then renders each
/// in turn until the available height is exhausted.
fn render_rooms_stacked(frame: &mut Frame, dashboard: &App, rooms: &[Room], area: Rect) {
    if area.height == 0 || area.width == 0 || rooms.is_empty() {
        return;
    }

    let selection = locate_selection(rooms, dashboard.view_selected_agent);
    let chars_per_row = compute_chars_per_row(area.width);
    dashboard.view_chars_per_row.set(chars_per_row);

    let (constraints, visible_rooms, used_total) =
        build_stack_layout(rooms, area.height, chars_per_row);

    if visible_rooms.is_empty() {
        render_overflow_fallback(frame, dashboard, rooms, area);
        return;
    }

    let final_constraints = pad_constraints(constraints, used_total, area.height);
    let chunks = Layout::vertical(final_constraints).split(area);
    let prefix_sums = compute_prefix_sums(rooms);

    render_visible_rooms(
        frame,
        &VisibleRoomsContext {
            dashboard,
            rooms,
            visible_rooms: &visible_rooms,
            chunks: &chunks,
            prefix_sums: &prefix_sums,
            selection,
        },
    );
}

/// Cards-per-row count for `area_width`, accounting for the room border (2).
fn compute_chars_per_row(area_width: u16) -> usize {
    let inner_width = area_width.saturating_sub(2);
    let chars_per_row_u16 = inner_width.checked_div(COMPACT_CARD_WIDTH).unwrap_or(1).max(1);
    usize::from(chars_per_row_u16)
}

/// Append a `Min(0)` constraint when there is leftover space, so rooms
/// don't stretch to fill the area.
fn pad_constraints(
    mut constraints: Vec<Constraint>,
    used_total: u16,
    available_height: u16,
) -> Vec<Constraint> {
    if used_total < available_height {
        constraints.push(Constraint::Min(0));
    }
    constraints
}

/// Fallback when none of the rooms fit: render only the first one,
/// expanded to consume the full area.
fn render_overflow_fallback(frame: &mut Frame, dashboard: &App, rooms: &[Room], area: Rect) {
    if let Some(first) = rooms.first() {
        render_room(
            frame,
            dashboard,
            &RenderRoomParams {
                room: first,
                area,
                slot_num: None,
                selected_agent: None,
                agent_label_offset: Some(0),
                compact: true,
            },
        );
    }
}

/// Inputs for [`render_visible_rooms`].
struct VisibleRoomsContext<'ctx> {
    /// Application state; used by per-card renderers.
    dashboard: &'ctx App,
    /// All rooms (used to compute the original index of each visible room).
    rooms: &'ctx [Room],
    /// Subset of `rooms` that fit in the available height.
    visible_rooms: &'ctx [&'ctx Room],
    /// Layout chunks (alternating gap / room) the parent computed.
    chunks: &'ctx [Rect],
    /// Prefix sums of session counts across `rooms`, indexed by room id.
    prefix_sums: &'ctx [usize],
    /// `(room_index, local_index)` of the selected agent, if any.
    selection: (Option<usize>, usize),
}

/// Render each visible room into its assigned chunk.
fn render_visible_rooms(frame: &mut Frame, context: &VisibleRoomsContext) {
    let (selected_room_idx, selected_local_idx) = context.selection;
    let mut chunk_idx = 0_usize;
    for (visible_pos, room) in context.visible_rooms.iter().enumerate() {
        if visible_pos > 0 {
            chunk_idx = chunk_idx.saturating_add(1);
        }
        let Some(rect) = context.chunks.get(chunk_idx).copied() else {
            break;
        };
        let room_idx = context
            .rooms
            .iter()
            .position(|candidate| candidate.name == room.name)
            .unwrap_or(usize::MAX);
        let agent_selection =
            if Some(room_idx) == selected_room_idx { Some(selected_local_idx) } else { None };
        let label_offset = context.prefix_sums.get(room_idx).copied().unwrap_or(0);
        render_room(
            frame,
            context.dashboard,
            &RenderRoomParams {
                room,
                area: rect,
                slot_num: None,
                selected_agent: agent_selection,
                agent_label_offset: Some(label_offset),
                compact: true,
            },
        );
        chunk_idx = chunk_idx.saturating_add(1);
    }
}

/// Find which (room, local-index) pair holds the global selection cursor.
fn locate_selection(rooms: &[Room], view_selected_agent: usize) -> (Option<usize>, usize) {
    let total: usize = rooms.iter().map(|room| room.session_indices.len()).sum();
    if total == 0 {
        return (None, 0);
    }
    let global = view_selected_agent.min(total.saturating_sub(1));
    let mut accumulator = 0_usize;
    for (room_position, room) in rooms.iter().enumerate() {
        let count = room.session_indices.len();
        let upper = accumulator.saturating_add(count);
        if global < upper {
            let local = global.saturating_sub(accumulator);
            return (Some(room_position), local);
        }
        accumulator = upper;
    }
    (None, 0)
}

/// Determine which rooms fit and their height constraints.
fn build_stack_layout<'rooms>(
    rooms: &'rooms [Room],
    available_height: u16,
    chars_per_row: usize,
) -> (Vec<Constraint>, Vec<&'rooms Room>, u16) {
    let mut constraints: Vec<Constraint> = Vec::new();
    let mut visible: Vec<&'rooms Room> = Vec::new();
    let mut used: u16 = 0;
    let row_capacity = chars_per_row.max(1);

    for (room_position, room) in rooms.iter().enumerate() {
        let count = room.session_indices.len().max(1);
        let rows_us = count
            .saturating_add(row_capacity.saturating_sub(1))
            .checked_div(row_capacity)
            .unwrap_or(count);
        let rows = u16::try_from(rows_us).unwrap_or(u16::MAX);
        let needed = rows.saturating_mul(COMPACT_CARD_HEIGHT).saturating_add(ROOM_BORDER_OVERHEAD);
        let spacer = if room_position == 0 { 0 } else { ROOM_GAP };
        if used.saturating_add(needed).saturating_add(spacer) > available_height {
            break;
        }
        if spacer > 0 {
            constraints.push(Constraint::Length(spacer));
        }
        constraints.push(Constraint::Length(needed));
        visible.push(room);
        used = used.saturating_add(needed).saturating_add(spacer);
    }

    (constraints, visible, used)
}

/// Pre-compute the running session-count prefix sum across rooms.
fn compute_prefix_sums(rooms: &[Room]) -> Vec<usize> {
    let mut sums = Vec::with_capacity(rooms.len());
    let mut total = 0_usize;
    for room in rooms {
        sums.push(total);
        total = total.saturating_add(room.session_indices.len());
    }
    sums
}

/// Render a single [`Room`] block: bordered title + grid of agent cards.
fn render_room(frame: &mut Frame, dashboard: &App, params: &RenderRoomParams) {
    let RenderRoomParams { room, area, slot_num, selected_agent, agent_label_offset, compact } =
        *params;

    let border_color = if room.has_input {
        if dashboard.tick.is_multiple_of(2) {
            Color::Yellow
        } else {
            Color::White
        }
    } else {
        Color::DarkGray
    };

    let title = slot_num.map_or_else(
        || format!(" {} ({}) ", room.name, room.session_indices.len()),
        |slot| format!(" [{slot}] {} ({}) ", room.name, room.session_indices.len()),
    );
    let title_style = if room.has_input {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(title, title_style))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    render_card_grid(
        frame,
        &CardGridInputs { dashboard, room, inner, selected_agent, agent_label_offset, compact },
    );
}
