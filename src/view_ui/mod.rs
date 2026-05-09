//! TUI rendering pipeline split into focused submodules.
//!
//! Public entry points used by `app.rs`, `main.rs`, and the daemon:
//!
//! * [`render`]                — full dashboard render (stacked rooms + bars).
//! * [`render_dock`]           — sidebar dock view (mini cards).
//! * [`resolve_zoom`]          — clamp selection / promote a zoom request.
//! * [`group_into_rooms_stable`] — room grouping with caller-stable order.
//! * [`pick_species`]          — stable species pick from a session id.
//! * [`SPECIES_COUNT`]         — number of supported species.
//! * [`SPECIES_NAMES`]         — ordered species display names.

mod animation;
mod card_grid;
mod character;
mod character_compact;
mod context_bar;
pub mod dock;
mod footer;
mod overlay;
mod palettes;
pub mod rooms;
mod rooms_view;
mod sprites;
pub mod text;
pub mod types;

use ratatui::{
    layout::{Constraint, Layout},
    Frame,
};

use crate::app::App;

use self::rooms::{group_into_rooms_stable, update_room_order};

use footer::{render_footer, render_rename_bar, render_search_bar};
use rooms_view::render_rooms;

/// Promote `view_zoom_index` into a concrete `view_zoomed_room` and clamp
/// selection within the active room (or across rooms when no zoom is set).
pub fn resolve_zoom(dashboard: &mut App) {
    update_room_order(dashboard);
    let filtered = dashboard.filtered_indices();
    let rooms = group_into_rooms_stable(&dashboard.sessions, &filtered, &dashboard.view_room_order);
    if let Some(zoom_idx) = dashboard.view_zoom_index.take() {
        if let Some(room) = rooms.get(zoom_idx) {
            dashboard.view_zoomed_room = Some(room.name.clone());
        }
    }

    if let Some(zoomed_name) = dashboard.view_zoomed_room.as_ref() {
        if let Some(room) = rooms.iter().find(|candidate| &candidate.name == zoomed_name) {
            if room.session_indices.is_empty() {
                dashboard.view_selected_agent = 0;
            } else {
                let last = room.session_indices.len().saturating_sub(1);
                dashboard.view_selected_agent = dashboard.view_selected_agent.min(last);
            }
        }
    } else {
        let total: usize = rooms.iter().map(|room| room.session_indices.len()).sum();
        if total > 0 {
            dashboard.view_selected_agent =
                dashboard.view_selected_agent.min(total.saturating_sub(1));
        } else {
            dashboard.view_selected_agent = 0;
        }
    }
}

/// Render the full dashboard: rooms area on top, then optional search and
/// rename overlay rows, then the footer.
pub fn render(frame: &mut Frame, dashboard: &App) {
    let show_search = dashboard.filter_active() || !dashboard.filter_text.is_empty();
    let show_rename = dashboard.rename_active();
    let extra: u16 = u16::from(show_search).saturating_add(u16::from(show_rename));
    let chunks = match extra {
        2 => Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area()),
        1 => Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)])
            .split(frame.area()),
        _ => Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(frame.area()),
    };

    let Some(rooms_area) = chunks.first().copied() else {
        return;
    };
    render_rooms(frame, dashboard, rooms_area);

    let mut next: usize = 1;
    if show_search {
        if let Some(rect) = chunks.get(next).copied() {
            render_search_bar(frame, dashboard, rect);
        }
        next = next.saturating_add(1);
    }
    if show_rename {
        if let Some(rect) = chunks.get(next).copied() {
            render_rename_bar(frame, dashboard, rect);
        }
        next = next.saturating_add(1);
    }
    if let Some(rect) = chunks.get(next).copied() {
        render_footer(frame, dashboard, rect);
    }
}

#[cfg(test)]
mod tests;
