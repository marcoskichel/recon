//! Room grouping: turns a flat list of session indices into bucketed
//! rooms keyed by project, with stable cross-refresh ordering.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{
    app::App,
    session::{Session, SessionStatus},
};

/// A group of sessions sharing a project (the visible "room" in the UI).
pub struct Room {
    /// Display name (project / room id).
    pub name: String,
    /// Indices into `App::sessions` that belong to this room.
    pub session_indices: Vec<usize>,
    /// `true` if any session in the room is awaiting user input.
    pub has_input: bool,
    /// Most-recent activity timestamp across all sessions in the room.
    pub last_activity: Option<String>,
}

/// Bucket `indices` by room id and sort the resulting rooms.
///
/// Sort order: rooms with `has_input` first, then by `last_activity`
/// descending. Sessions with no project name are grouped under `"unknown"`.
pub(super) fn group_into_rooms(sessions: &[Session], indices: &[usize]) -> Vec<Room> {
    let mut buckets: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &session_idx in indices {
        let Some(session) = sessions.get(session_idx) else {
            continue;
        };
        let room_name =
            if session.project_name.is_empty() { "unknown".to_string() } else { session.room_id() };
        buckets.entry(room_name).or_default().push(session_idx);
    }

    let mut rooms: Vec<Room> = buckets
        .into_iter()
        .map(|(name, room_indices)| build_room(sessions, name, room_indices))
        .collect();

    rooms.sort_by(|left, right| {
        right
            .has_input
            .cmp(&left.has_input)
            .then_with(|| right.last_activity.cmp(&left.last_activity))
    });

    rooms
}

/// Construct a [`Room`] by computing its `has_input` and `last_activity`
/// rollups from the contained session indices.
fn build_room(sessions: &[Session], name: String, indices: Vec<usize>) -> Room {
    let has_input =
        indices.iter().any(|&session_idx| sessions.get(session_idx).is_some_and(is_input_session));
    let last_activity = indices
        .iter()
        .filter_map(|&session_idx| sessions.get(session_idx))
        .filter_map(|session| session.last_activity.as_ref())
        .max()
        .cloned();
    Room { name, session_indices: indices, has_input, last_activity }
}

/// Predicate: `session.status == Input`.
const fn is_input_session(session: &Session) -> bool {
    matches!(session.status, SessionStatus::Input)
}

/// Like [`group_into_rooms`], but reorders the result so rooms appear
/// in `order` first (preserving caller-controlled positioning across
/// refreshes); rooms not in `order` follow, sorted by name.
pub fn group_into_rooms_stable(
    sessions: &[Session],
    indices: &[usize],
    order: &[String],
) -> Vec<Room> {
    let rooms = group_into_rooms(sessions, indices);
    let mut by_name: HashMap<String, Room> =
        rooms.into_iter().map(|room| (room.name.clone(), room)).collect();
    let mut output: Vec<Room> = Vec::with_capacity(by_name.len());
    for name in order {
        if let Some(room) = by_name.remove(name) {
            output.push(room);
        }
    }
    let mut leftover: Vec<Room> = by_name.into_values().collect();
    leftover.sort_by(|left, right| left.name.cmp(&right.name));
    output.extend(leftover);
    output
}

/// Append any newly-discovered room names to `dashboard.view_room_order`.
///
/// This preserves the user's visual ordering across refreshes: existing
/// rooms keep their positions, new rooms are appended at the end.
pub fn update_room_order(dashboard: &mut App) {
    let filtered = dashboard.filtered_indices();
    let rooms = group_into_rooms(&dashboard.sessions, &filtered);
    let known: HashSet<String> = dashboard.view_room_order.iter().cloned().collect();
    for room in &rooms {
        if !known.contains(&room.name) {
            dashboard.view_room_order.push(room.name.clone());
        }
    }
}
