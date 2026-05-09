//! Unit tests for room grouping. These exercise [`group_into_rooms`]
//! exclusively, which is the only piece of the renderer with non-trivial
//! ordering semantics.

use std::path::PathBuf;

use crate::session::{Session, SessionStatus};

use super::rooms::group_into_rooms;

/// Build a synthetic [`Session`] for tests with the minimum fields the
/// grouping logic looks at.
fn make_session(project_dir: &str, status: SessionStatus, last_activity: Option<&str>) -> Session {
    Session {
        id: String::new(),
        project_name: project_dir.to_string(),
        branch: None,
        cwd: project_dir.to_string(),
        relative_dir: None,
        tmux_name: None,
        pane_target: None,
        model: None,
        effort: None,
        total_input_tokens: 0,
        total_output_tokens: 0,
        status,
        pid: None,
        last_activity: last_activity.map(str::to_string),
        started_at: 0,
        jsonl_path: PathBuf::new(),
        last_file_size: 0,
        last_user_prompt: None,
    }
}

/// Look up a room name by its index in the sorted result, returning a
/// stable error string instead of panicking on out-of-range access.
fn room_name(rooms: &[super::rooms::Room], index: usize) -> &str {
    rooms.get(index).map_or("<missing-room>", |room| room.name.as_str())
}

/// # Panics
/// Panics on assertion failure, by design (this is a unit test).
#[test]
fn rooms_with_input_sort_first() {
    let sessions = vec![
        make_session("/a", SessionStatus::Idle, Some("2026-03-16T10:00:00Z")),
        make_session("/b", SessionStatus::Input, Some("2026-03-16T09:00:00Z")),
    ];
    let indices: Vec<usize> = (0..sessions.len()).collect();
    let rooms = group_into_rooms(&sessions, &indices);
    assert_eq!(room_name(&rooms, 0), "/b");
    assert_eq!(room_name(&rooms, 1), "/a");
}

/// # Panics
/// Panics on assertion failure, by design (this is a unit test).
#[test]
fn secondary_sort_by_last_activity_descending() {
    let sessions = vec![
        make_session("/old", SessionStatus::Idle, Some("2026-03-16T08:00:00Z")),
        make_session("/recent", SessionStatus::Idle, Some("2026-03-16T12:00:00Z")),
        make_session("/mid", SessionStatus::Idle, Some("2026-03-16T10:00:00Z")),
    ];
    let indices: Vec<usize> = (0..sessions.len()).collect();
    let rooms = group_into_rooms(&sessions, &indices);
    assert_eq!(room_name(&rooms, 0), "/recent");
    assert_eq!(room_name(&rooms, 1), "/mid");
    assert_eq!(room_name(&rooms, 2), "/old");
}

/// # Panics
/// Panics on assertion failure, by design (this is a unit test).
#[test]
fn new_sessions_sort_last() {
    let sessions = vec![
        make_session("/egg", SessionStatus::New, None),
        make_session("/active", SessionStatus::Idle, Some("2026-03-16T10:00:00Z")),
    ];
    let indices: Vec<usize> = (0..sessions.len()).collect();
    let rooms = group_into_rooms(&sessions, &indices);
    assert_eq!(room_name(&rooms, 0), "/active");
    assert_eq!(room_name(&rooms, 1), "/egg");
}

/// # Panics
/// Panics on assertion failure, by design (this is a unit test).
#[test]
fn room_activity_uses_max_across_sessions() {
    let sessions = vec![
        make_session("/repo", SessionStatus::Idle, Some("2026-03-16T08:00:00Z")),
        make_session("/repo", SessionStatus::New, None),
        make_session("/repo", SessionStatus::Idle, Some("2026-03-16T12:00:00Z")),
        make_session("/other", SessionStatus::Idle, Some("2026-03-16T10:00:00Z")),
    ];
    let indices: Vec<usize> = (0..sessions.len()).collect();
    let rooms = group_into_rooms(&sessions, &indices);
    assert_eq!(room_name(&rooms, 0), "/repo");
    assert_eq!(room_name(&rooms, 1), "/other");
}

/// # Panics
/// Panics on assertion failure, by design (this is a unit test).
#[test]
fn input_rooms_also_sorted_by_activity() {
    let sessions = vec![
        make_session("/old-input", SessionStatus::Input, Some("2026-03-16T08:00:00Z")),
        make_session("/new-input", SessionStatus::Input, Some("2026-03-16T12:00:00Z")),
    ];
    let indices: Vec<usize> = (0..sessions.len()).collect();
    let rooms = group_into_rooms(&sessions, &indices);
    assert_eq!(room_name(&rooms, 0), "/new-input");
    assert_eq!(room_name(&rooms, 1), "/old-input");
}

/// # Panics
/// Panics on assertion failure, by design (this is a unit test).
#[test]
fn worktrees_share_room_by_project_name() {
    // Two sessions with different CWDs but same project_name should be
    // in the same room (the room id is the tmux session or project name).
    let mut first = make_session("/repos/line5", SessionStatus::Idle, Some("2026-03-16T10:00:00Z"));
    first.project_name = "line5".to_string();
    let mut second =
        make_session("/worktrees/line5-feat", SessionStatus::Working, Some("2026-03-16T11:00:00Z"));
    second.project_name = "line5".to_string();
    let sessions = [first, second];
    let indices: Vec<usize> = (0..sessions.len()).collect();
    let rooms = group_into_rooms(&sessions, &indices);
    assert_eq!(rooms.len(), 1);
    assert_eq!(room_name(&rooms, 0), "line5");
    let session_count = rooms.first().map_or(0, |room| room.session_indices.len());
    assert_eq!(session_count, 2);
}

/// # Panics
/// Panics on assertion failure, by design (this is a unit test).
#[test]
fn subproject_gets_separate_room() {
    let mut root = make_session("/repos/line5", SessionStatus::Idle, Some("2026-03-16T10:00:00Z"));
    root.project_name = "line5".to_string();
    let mut nested =
        make_session("/repos/line5/tools/solo", SessionStatus::Idle, Some("2026-03-16T11:00:00Z"));
    nested.project_name = "line5".to_string();
    nested.relative_dir = Some("tools/solo".to_string());
    let sessions = [root, nested];
    let indices: Vec<usize> = (0..sessions.len()).collect();
    let rooms = group_into_rooms(&sessions, &indices);
    assert_eq!(rooms.len(), 2);
}

/// # Panics
/// Panics on assertion failure, by design (this is a unit test).
#[test]
fn mixed_input_and_activity_sorting() {
    let sessions = vec![
        make_session("/idle-recent", SessionStatus::Idle, Some("2026-03-16T15:00:00Z")),
        make_session("/input-old", SessionStatus::Input, Some("2026-03-16T08:00:00Z")),
        make_session("/egg", SessionStatus::New, None),
        make_session("/idle-old", SessionStatus::Idle, Some("2026-03-16T09:00:00Z")),
    ];
    let indices: Vec<usize> = (0..sessions.len()).collect();
    let rooms = group_into_rooms(&sessions, &indices);
    assert_eq!(room_name(&rooms, 0), "/input-old");
    assert_eq!(room_name(&rooms, 1), "/idle-recent");
    assert_eq!(room_name(&rooms, 2), "/idle-old");
    assert_eq!(room_name(&rooms, 3), "/egg");
}
