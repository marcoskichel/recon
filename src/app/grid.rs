//! Grid-position math for the compact view and zoomed-room navigation.
//!
//! The compact view lays out sessions as a sequence of rooms, each rendered
//! as a `chars_per_row`-wide grid. These helpers translate between flat
//! "selected agent" indices and `(room, row, column)` triples so the key
//! handler can move the cursor j/k/h/l around the grid.

use super::Application;
use crate::{session::Session, view_ui};

/// Per-room layout: `(session_count, row_count, base_offset_into_flat_list)`.
type RoomLayout = (usize, usize, usize);

/// Position inside the compact grid.
#[derive(Clone, Copy)]
struct GridPosition {
    /// Index of the room within the flat list of room layouts.
    room: usize,
    /// Row within the room (zero-based).
    row_index: usize,
    /// Column within the row (zero-based).
    column: usize,
}

impl Application {
    /// Sessions in the zoomed room, indexed into `self.sessions`.
    pub(super) fn zoomed_room_session_indices(&self) -> Vec<usize> {
        let Some(ref room_name) = self.view_zoomed_room else {
            return Vec::new();
        };
        self.sessions
            .iter()
            .enumerate()
            .filter(|&(_, session)| {
                let name = if session.project_name.is_empty() {
                    "unknown".to_string()
                } else {
                    session.room_id()
                };
                &name == room_name
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Currently-selected session in the zoomed room view.
    pub(super) fn selected_zoomed_session(&self) -> Option<&Session> {
        let indices = self.zoomed_room_session_indices();
        if indices.is_empty() {
            return None;
        }
        let last = indices.len().saturating_sub(1);
        let clamped = self.view_selected_agent.min(last);
        let session_index = *indices.get(clamped)?;
        self.sessions.get(session_index)
    }

    /// Working directory of the selected session in the zoomed room view.
    pub(super) fn zoomed_room_cwd(&self) -> Option<String> {
        self.selected_zoomed_session().map(|session| session.cwd.clone())
    }

    /// Flatten the rooms in the compact view into a single list of session
    /// indices, in display order.
    pub(super) fn compact_flat_session_indices(&self) -> Vec<usize> {
        let filtered = self.filtered_indices();
        let rooms = view_ui::rooms::group_into_rooms_stable(
            &self.sessions,
            &filtered,
            &self.view_room_order,
        );
        rooms.into_iter().flat_map(|room| room.session_indices.into_iter()).collect()
    }

    /// Currently-selected session in the compact view.
    pub(super) fn selected_compact_session(&self) -> Option<&Session> {
        let indices = self.compact_flat_session_indices();
        if indices.is_empty() {
            return None;
        }
        let last = indices.len().saturating_sub(1);
        let clamped = self.view_selected_agent.min(last);
        let session_index = *indices.get(clamped)?;
        self.sessions.get(session_index)
    }

    /// Working directory of the selected session in the compact view.
    pub(super) fn selected_compact_cwd(&self) -> Option<String> {
        self.selected_compact_session().map(|session| session.cwd.clone())
    }

    /// Build the per-room grid layouts for the compact view.
    fn compact_room_layouts(&self, cols_per_row: usize) -> (Vec<RoomLayout>, usize) {
        let filtered = self.filtered_indices();
        let rooms = view_ui::rooms::group_into_rooms_stable(
            &self.sessions,
            &filtered,
            &self.view_room_order,
        );
        let mut layouts: Vec<RoomLayout> = Vec::with_capacity(rooms.len());
        let mut base = 0_usize;
        for room in &rooms {
            let count = room.session_indices.len();
            let rows = rows_for_grid(count, cols_per_row);
            layouts.push((count, rows, base));
            base = base.saturating_add(count);
        }
        (layouts, base)
    }

    /// Cursor target for `j`/Down in the compact grid.
    pub(super) fn compact_grid_move_down(&self) -> usize {
        let cols_per_row = self.view_chars_per_row.get().max(1);
        let (layouts, _total) = self.compact_room_layouts(cols_per_row);
        let cursor = self.view_selected_agent;
        let Some(position) = idx_to_pos(cursor, &layouts, cols_per_row) else {
            return cursor;
        };
        let Some(&(count, rows, base)) = layouts.get(position.room) else {
            return cursor;
        };

        if position.row_index.saturating_add(1) < rows {
            return target_in_room(
                base,
                count,
                cols_per_row,
                GridPosition {
                    room: position.room,
                    row_index: position.row_index.saturating_add(1),
                    column: position.column,
                },
            );
        }
        let next_room = position.room.saturating_add(1);
        let Some(&(next_count, _next_rows, next_base)) = layouts.get(next_room) else {
            return cursor;
        };
        if next_count == 0 {
            return cursor;
        }
        target_in_room(
            next_base,
            next_count,
            cols_per_row,
            GridPosition { room: next_room, row_index: 0, column: position.column },
        )
    }

    /// Cursor target for `k`/Up in the compact grid.
    pub(super) fn compact_grid_move_up(&self) -> usize {
        let cols_per_row = self.view_chars_per_row.get().max(1);
        let (layouts, _total) = self.compact_room_layouts(cols_per_row);
        let cursor = self.view_selected_agent;
        let Some(position) = idx_to_pos(cursor, &layouts, cols_per_row) else {
            return cursor;
        };
        let Some(&(count, _rows, base)) = layouts.get(position.room) else {
            return cursor;
        };

        if position.row_index > 0 {
            return target_in_room(
                base,
                count,
                cols_per_row,
                GridPosition {
                    room: position.room,
                    row_index: position.row_index.saturating_sub(1),
                    column: position.column,
                },
            );
        }
        if position.room == 0 {
            return cursor;
        }
        let prev_room = position.room.saturating_sub(1);
        let Some(&(prev_count, prev_rows, prev_base)) = layouts.get(prev_room) else {
            return cursor;
        };
        if prev_count == 0 || prev_rows == 0 {
            return cursor;
        }
        let last_row = prev_rows.saturating_sub(1);
        target_in_room(
            prev_base,
            prev_count,
            cols_per_row,
            GridPosition { room: prev_room, row_index: last_row, column: position.column },
        )
    }
}

/// Number of rows needed to fit `count` cells, `cols_per_row` per row.
///
/// Returns 0 if `cols_per_row` is 0 (defensive — the navigation logic guards
/// against this with `.max(1)`).
fn rows_for_grid(count: usize, cols_per_row: usize) -> usize {
    if cols_per_row == 0 {
        return 0;
    }
    count.saturating_add(cols_per_row.saturating_sub(1)).checked_div(cols_per_row).unwrap_or(0)
}

/// Translate a flat index into a [`GridPosition`].
fn idx_to_pos(flat: usize, layouts: &[RoomLayout], cols_per_row: usize) -> Option<GridPosition> {
    if cols_per_row == 0 {
        return None;
    }
    for (room_index, &(count, _rows, base)) in layouts.iter().enumerate() {
        if flat < base.saturating_add(count) {
            let local = flat.saturating_sub(base);
            let row_index = local.checked_div(cols_per_row)?;
            let column = local.checked_rem(cols_per_row)?;
            return Some(GridPosition { room: room_index, row_index, column });
        }
    }
    None
}

/// Number of populated cells in `row_index` of a room with `count` sessions.
fn cells_in_row(count: usize, row_index: usize, cols_per_row: usize) -> usize {
    if cols_per_row == 0 {
        return 0;
    }
    let rows = rows_for_grid(count, cols_per_row);
    if row_index.saturating_add(1) == rows {
        let remainder = count.checked_rem(cols_per_row).unwrap_or(0);
        if remainder == 0 {
            cols_per_row
        } else {
            remainder
        }
    } else {
        cols_per_row
    }
}

/// Compute the flat index for the cell at `position` of the room whose
/// flat-list base is `base`.  The position's `col` is clamped to the
/// populated cells in the chosen `row`.
fn target_in_room(base: usize, count: usize, cols_per_row: usize, position: GridPosition) -> usize {
    let cells = cells_in_row(count, position.row_index, cols_per_row);
    let target_col = position.column.min(cells.saturating_sub(1));
    base.saturating_add(position.row_index.saturating_mul(cols_per_row)).saturating_add(target_col)
}
