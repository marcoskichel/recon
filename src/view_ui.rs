use std::collections::BTreeMap;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Padding, Paragraph},
};

use crate::app::App;
use crate::session::{Session, SessionStatus};

// Layout constants
const ROOMS_PER_PAGE: usize = 4;
const SPRITE_W: usize = 10; // pixel columns
const SPRITE_H: usize = 10; // pixel rows
const SPRITE_RENDER_H: u16 = (SPRITE_H as u16 + 1) / 2; // terminal lines for sprite (5)
const CHAR_WIDTH: u16 = (SPRITE_W as u16) + 30; // sprite + padding (wider for label legibility)
const NAME_LINES: u16 = 3; // wrap label across this many lines
const CHAR_LABEL_LINES: u16 = NAME_LINES + 2; // name(NAME_LINES) + branch + context bar
const CHAR_HEIGHT: u16 = SPRITE_RENDER_H + CHAR_LABEL_LINES;

// Compact horizontal card: sprite left, info right, rounded border.
const COMPACT_CARD_WIDTH: u16 = 46;
const COMPACT_CARD_HEIGHT: u16 = 9;
const COMPACT_SPRITE_COLS: u16 = 12; // sprite (10) + 1 col gutter each side

// ── Pixel sprite data ────────────────────────────────────────────────
// Each sprite is SPRITE_H rows x SPRITE_W cols. 0 = transparent.
// Positive values index into the per-state color palette.
// Only Working and Input have multiple frames (animated).

type Sprite = [[u8; SPRITE_W]; SPRITE_H];
type Palette = &'static [(u8, u8, u8)]; // index 0 unused (transparent)

// Egg palette: 1=cream shell, 2=shadow, 3=green spots
const PAL_EGG: &[(u8, u8, u8)] = &[
    (0, 0, 0),         // 0: unused
    (255, 250, 230),    // 1: cream shell
    (220, 200, 170),    // 2: shell shadow
    (180, 220, 180),    // 3: green spots
];

const SPRITE_EGG: [Sprite; 1] = [[
    [0,0,0,0,1,1,1,0,0,0],
    [0,0,0,1,1,1,1,1,0,0],
    [0,0,1,1,1,3,1,1,1,0],
    [0,0,1,1,1,1,1,1,1,0],
    [0,0,1,3,1,1,1,3,1,0],
    [0,0,1,1,1,1,1,1,1,0],
    [0,0,1,1,1,1,1,1,1,0],
    [0,0,0,1,2,1,2,1,0,0],
    [0,0,0,0,1,1,1,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
]];

// Working palette: 1=green body, 2=dark green, 3=eyes, 4=eye highlight,
//                  5=blush, 6=mouth, 7=feet, 8=sparkle
const PAL_WORKING: &[(u8, u8, u8)] = &[
    (0, 0, 0),
    (120, 220, 120),    // 1: green body
    (80, 180, 80),      // 2: darker green
    (40, 40, 40),       // 3: eyes
    (255, 255, 255),    // 4: eye highlight
    (255, 150, 150),    // 5: cheeks
    (200, 100, 80),     // 6: mouth
    (100, 200, 100),    // 7: feet
    (255, 220, 60),     // 8: sparkle
];

const SPRITE_WORKING: [Sprite; 3] = [
    // Frame 0: happy, sparkles top
    [
        [0,0,0,8,1,1,1,8,0,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,1,3,4,1,1,3,4,1,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,5,1,1,6,6,1,1,5,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,0,0,7,0,0,7,0,0,0],
        [0,0,0,0,0,0,0,0,0,0],
    ],
    // Frame 1: squinting
    [
        [0,0,0,1,1,1,1,0,0,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,1,1,3,1,1,3,1,1,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,5,1,6,1,1,6,1,5,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,0,7,0,0,0,0,7,0,0],
        [0,0,0,0,0,0,0,0,0,0],
    ],
    // Frame 2: arms out, sparkles
    [
        [0,0,8,1,1,1,1,8,0,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,1,4,3,1,1,4,3,1,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,5,1,1,6,6,1,1,5,0],
        [8,1,1,1,1,1,1,1,1,8],
        [0,0,1,1,1,1,1,1,0,0],
        [0,0,0,7,0,0,7,0,0,0],
        [0,0,0,0,0,0,0,0,0,0],
    ],
];

// Idle palette: 1=blue-grey body, 2=darker, 3=closed eyes, 4=highlight, 5=feet, 6=Zzz
const PAL_IDLE: &[(u8, u8, u8)] = &[
    (0, 0, 0),
    (140, 160, 200),    // 1: blue-grey body
    (110, 130, 170),    // 2: darker
    (60, 60, 80),       // 3: closed eyes
    (180, 190, 220),    // 4: highlight
    (120, 140, 180),    // 5: feet
    (200, 200, 255),    // 6: Zzz
];

const SPRITE_IDLE: [Sprite; 1] = [[
    [0,0,0,1,1,1,1,0,0,0],
    [0,0,1,1,1,1,1,1,0,6],
    [0,1,1,1,1,1,1,1,1,0],
    [0,1,3,3,1,1,3,3,1,6],
    [0,1,1,1,1,1,1,1,1,0],
    [0,1,1,1,1,1,1,1,1,0],
    [0,1,1,1,1,1,1,1,1,0],
    [0,0,1,1,1,1,1,1,0,0],
    [0,0,0,5,0,0,5,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
]];

// Input (angry) palette: 1=orange body, 2=darker, 3=pupils, 4=eye whites,
//                        5=angry red, 6=feet, 7=flush
const PAL_INPUT: &[(u8, u8, u8)] = &[
    (0, 0, 0),
    (255, 180, 60),     // 1: orange body
    (220, 150, 40),     // 2: darker
    (40, 40, 40),       // 3: pupils
    (255, 255, 255),    // 4: eye whites
    (255, 60, 60),      // 5: angry red (brows, mouth)
    (200, 140, 40),     // 6: feet
    (255, 100, 100),    // 7: flush/anger
];

const SPRITE_INPUT: [Sprite; 3] = [
    // Frame 0: angry brows down
    [
        [0,0,0,1,1,1,1,0,0,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,1,5,1,1,1,1,5,1,0],
        [0,1,1,4,3,3,4,1,1,0],
        [0,7,1,1,1,1,1,1,7,0],
        [0,1,1,5,5,5,5,1,1,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,0,0,6,0,0,6,0,0,0],
        [0,0,0,0,0,0,0,0,0,0],
    ],
    // Frame 1: brows shifted
    [
        [0,0,0,1,1,1,1,0,0,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,1,1,5,1,1,5,1,1,0],
        [0,1,1,4,3,3,4,1,1,0],
        [0,7,1,1,1,1,1,1,7,0],
        [0,1,1,1,5,5,1,1,1,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,0,6,0,0,0,0,6,0,0],
        [0,0,0,0,0,0,0,0,0,0],
    ],
    // Frame 2: wider stance
    [
        [0,0,0,1,1,1,1,0,0,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,1,5,1,1,1,1,5,1,0],
        [0,1,1,3,4,4,3,1,1,0],
        [0,1,7,1,1,1,1,7,1,0],
        [0,1,5,1,5,5,1,5,1,0],
        [0,1,1,1,1,1,1,1,1,0],
        [0,0,1,1,1,1,1,1,0,0],
        [0,0,0,6,0,0,6,0,0,0],
        [0,0,0,0,0,0,0,0,0,0],
    ],
];

// ── Sprite selection ─────────────────────────────────────────────────

fn sprite_data(status: &SessionStatus, frame: usize) -> (&'static Sprite, Palette) {
    match status {
        SessionStatus::New => (&SPRITE_EGG[0], PAL_EGG),
        SessionStatus::Working => (&SPRITE_WORKING[frame % 3], PAL_WORKING),
        SessionStatus::Idle => (&SPRITE_IDLE[0], PAL_IDLE),
        SessionStatus::Input => (&SPRITE_INPUT[frame % 3], PAL_INPUT),
    }
}

// ── Half-block renderer ──────────────────────────────────────────────
// Renders a pixel grid as Lines of Spans using ▀▄ with fg+bg colors.
// Each terminal line encodes 2 pixel rows.

fn render_sprite_lines(sprite: &Sprite, palette: Palette) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let rows = SPRITE_H;
    let cols = SPRITE_W;

    for y in (0..rows).step_by(2) {
        let mut spans: Vec<Span<'static>> = Vec::new();

        for x in 0..cols {
            let top = sprite[y][x];
            let bot = if y + 1 < rows { sprite[y + 1][x] } else { 0 };

            if top == 0 && bot == 0 {
                spans.push(Span::raw(" "));
            } else if top == 0 {
                // Bottom pixel only: ▄ with fg = bottom color
                let (r, g, b) = palette[bot as usize];
                spans.push(Span::styled(
                    "\u{2584}",
                    Style::default().fg(Color::Rgb(r, g, b)),
                ));
            } else if bot == 0 {
                // Top pixel only: ▀ with fg = top color
                let (r, g, b) = palette[top as usize];
                spans.push(Span::styled(
                    "\u{2580}",
                    Style::default().fg(Color::Rgb(r, g, b)),
                ));
            } else {
                // Both pixels: ▀ with fg = top, bg = bottom
                let (tr, tg, tb) = palette[top as usize];
                let (br, bg, bb) = palette[bot as usize];
                spans.push(Span::styled(
                    "\u{2580}",
                    Style::default()
                        .fg(Color::Rgb(tr, tg, tb))
                        .bg(Color::Rgb(br, bg, bb)),
                ));
            }
        }

        lines.push(Line::from(spans));
    }

    lines
}

// ── Room grouping ────────────────────────────────────────────────────

pub(crate) struct Room {
    pub name: String,
    pub session_indices: Vec<usize>,
    pub has_input: bool,
    pub last_activity: Option<String>,
}

pub(crate) fn group_into_rooms(sessions: &[Session], indices: &[usize]) -> Vec<Room> {
    let mut map: BTreeMap<String, Vec<usize>> = BTreeMap::new();

    for &i in indices {
        let s = &sessions[i];
        let room_name = if s.project_name.is_empty() {
            "unknown".to_string()
        } else {
            s.room_id()
        };
        map.entry(room_name).or_default().push(i);
    }

    let mut rooms: Vec<Room> = map
        .into_iter()
        .map(|(name, indices)| {
            let has_input = indices
                .iter()
                .any(|&i| sessions[i].status == SessionStatus::Input);
            let last_activity = indices
                .iter()
                .filter_map(|&i| sessions[i].last_activity.as_ref())
                .max()
                .cloned();
            Room {
                name,
                session_indices: indices,
                has_input,
                last_activity,
            }
        })
        .collect();

    rooms.sort_by(|a, b| {
        b.has_input
            .cmp(&a.has_input)
            .then_with(|| b.last_activity.cmp(&a.last_activity))
    });

    rooms
}

pub(crate) fn group_into_rooms_stable(
    sessions: &[Session],
    indices: &[usize],
    order: &[String],
) -> Vec<Room> {
    let rooms = group_into_rooms(sessions, indices);
    let mut by_name: std::collections::HashMap<String, Room> =
        rooms.into_iter().map(|r| (r.name.clone(), r)).collect();
    let mut out = Vec::with_capacity(by_name.len());
    for name in order {
        if let Some(r) = by_name.remove(name) {
            out.push(r);
        }
    }
    let mut leftover: Vec<Room> = by_name.into_values().collect();
    leftover.sort_by(|a, b| a.name.cmp(&b.name));
    out.extend(leftover);
    out
}

pub fn update_room_order(app: &mut App) {
    let filtered = app.filtered_indices();
    let rooms = group_into_rooms(&app.sessions, &filtered);
    let known: std::collections::HashSet<String> =
        app.view_room_order.iter().cloned().collect();
    for r in &rooms {
        if !known.contains(&r.name) {
            app.view_room_order.push(r.name.clone());
        }
    }
}

// ── Animation ────────────────────────────────────────────────────────

fn animation_frame(status: &SessionStatus, tick: u64) -> usize {
    match status {
        SessionStatus::Working => ((tick / 2) % 3) as usize,
        SessionStatus::Input => (tick % 3) as usize,
        _ => 0,
    }
}

fn session_phase_offset(session_id: &str) -> u64 {
    session_id
        .bytes()
        .fold(0u64, |a, b| a.wrapping_add(b as u64))
        % 7
}

fn status_color(status: &SessionStatus) -> Color {
    match status {
        SessionStatus::New => Color::Blue,
        SessionStatus::Working => Color::Green,
        SessionStatus::Idle => Color::DarkGray,
        SessionStatus::Input => Color::Yellow,
    }
}

// ── Context bar ──────────────────────────────────────────────────────

fn context_bar(ratio: f64) -> (String, Color) {
    let bar_width = 6usize;
    let filled = (ratio * bar_width as f64).round().min(bar_width as f64) as usize;
    let empty = bar_width - filled;
    let pct = (ratio * 100.0) as u32;
    let bar = format!(
        "{}{} {}%",
        "\u{2588}".repeat(filled),
        "\u{2591}".repeat(empty),
        pct
    );
    let color = if ratio > 0.75 {
        Color::Red
    } else if ratio > 0.40 {
        Color::Yellow
    } else {
        Color::Green
    };
    (bar, color)
}

// ── Public render entry point ────────────────────────────────────────

pub fn resolve_zoom(app: &mut App) {
    update_room_order(app);
    let filtered = app.filtered_indices();
    let rooms = group_into_rooms_stable(&app.sessions, &filtered, &app.view_room_order);
    let total_pages = (rooms.len() + ROOMS_PER_PAGE - 1) / ROOMS_PER_PAGE;
    if total_pages > 0 {
        app.view_page = app.view_page.min(total_pages - 1);
    } else {
        app.view_page = 0;
    }

    // Compact mode shows every room stacked vertically by default.
    // Auto-zoom is no longer applied — users may still zoom into a room
    // explicitly via 1-4.

    if let Some(idx) = app.view_zoom_index.take() {
        let page_start = app.view_page * ROOMS_PER_PAGE;
        if let Some(room) = rooms.get(page_start + idx) {
            app.view_zoomed_room = Some(room.name.clone());
        }
    }

    // Clamp agent selection within zoomed room bounds
    if let Some(ref zoomed_name) = app.view_zoomed_room {
        if let Some(room) = rooms.iter().find(|r| &r.name == zoomed_name) {
            if !room.session_indices.is_empty() {
                app.view_selected_agent =
                    app.view_selected_agent.min(room.session_indices.len() - 1);
            } else {
                app.view_selected_agent = 0;
            }
        }
    } else if true {
        // Compact non-zoomed: clamp to total session count across rooms.
        let total: usize = rooms.iter().map(|r| r.session_indices.len()).sum();
        if total > 0 {
            app.view_selected_agent = app.view_selected_agent.min(total - 1);
        } else {
            app.view_selected_agent = 0;
        }
    }
}

pub fn render(frame: &mut Frame, app: &App) {
    let show_search = app.filter_active || !app.filter_text.is_empty();
    let chunks = if show_search {
        Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area())
    } else {
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
            .split(frame.area())
    };

    render_rooms(frame, app, chunks[0]);
    if show_search {
        render_search_bar(frame, app, chunks[1]);
        render_footer(frame, app, chunks[2]);
    } else {
        render_footer(frame, app, chunks[1]);
    }
}

fn render_search_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![
        Span::styled("/", Style::default().fg(Color::Cyan)),
        Span::raw(&app.filter_text),
    ];
    if !app.filter_active && !app.filter_text.is_empty() {
        let count = app.filtered_indices().len();
        spans.push(Span::styled(
            format!("  ({} match{})", count, if count == 1 { "" } else { "es" }),
            Style::default().fg(Color::DarkGray),
        ));
    }
    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);

    if app.filter_active {
        frame.set_cursor_position((area.x + 1 + app.filter_cursor as u16, area.y));
    }
}

fn render_rooms(frame: &mut Frame, app: &App, area: Rect) {
    let rooms = group_into_rooms_stable(&app.sessions, &app.filtered_indices(), &app.view_room_order);

    if rooms.is_empty() {
        render_empty(frame, area, app.tick);
        return;
    }

    if let Some(ref zoomed_name) = app.view_zoomed_room {
        if let Some(room) = rooms.iter().find(|r| &r.name == zoomed_name) {
            render_room(frame, app, room, area, None, Some(app.view_selected_agent), None, false);
            return;
        }
    }

    if true {
        render_rooms_stacked(frame, app, &rooms, area);
        return;
    }

    let total_pages = (rooms.len() + ROOMS_PER_PAGE - 1) / ROOMS_PER_PAGE;
    let page = app.view_page.min(total_pages.saturating_sub(1));
    let page_start = page * ROOMS_PER_PAGE;
    let page_rooms: Vec<&Room> = rooms
        .iter()
        .skip(page_start)
        .take(ROOMS_PER_PAGE)
        .collect();

    let v_chunks = Layout::vertical([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(area);
    let top_h = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(v_chunks[0]);
    let bot_h = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(v_chunks[1]);
    let cells = [top_h[0], top_h[1], bot_h[0], bot_h[1]];

    for (i, cell) in cells.iter().enumerate() {
        if let Some(room) = page_rooms.get(i) {
            render_room(frame, app, room, *cell, Some(i + 1), None, None, false);
        } else {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(30, 30, 30)));
            frame.render_widget(block, *cell);
        }
    }
}

fn render_rooms_stacked(frame: &mut Frame, app: &App, rooms: &[Room], area: Rect) {
    if area.height == 0 || area.width == 0 || rooms.is_empty() {
        return;
    }

    // Map global selection index to (room_idx, local_idx).
    let (selected_room_idx, selected_local_idx) = {
        let mut acc = 0usize;
        let total: usize = rooms.iter().map(|r| r.session_indices.len()).sum();
        if total == 0 {
            (None, 0)
        } else {
            let g = app.view_selected_agent.min(total - 1);
            let mut found: Option<usize> = None;
            let mut local = 0usize;
            for (i, r) in rooms.iter().enumerate() {
                let n = r.session_indices.len();
                if g < acc + n {
                    found = Some(i);
                    local = g - acc;
                    break;
                }
                acc += n;
            }
            (found, local)
        }
    };

    const ROOM_GAP: u16 = 1;
    const ROOM_BORDER_OVERHEAD: u16 = 2; // top + bottom border
    let inner_width = area.width.saturating_sub(2); // borders consume 2 cols
    let chars_per_row = (inner_width / COMPACT_CARD_WIDTH).max(1) as usize;
    app.view_chars_per_row.set(chars_per_row);

    let mut constraints: Vec<Constraint> = Vec::new();
    let mut visible_rooms: Vec<&Room> = Vec::new();
    let mut used: u16 = 0;

    for (idx, room) in rooms.iter().enumerate() {
        let n = room.session_indices.len().max(1);
        let rows = ((n + chars_per_row - 1) / chars_per_row) as u16;
        let needed = rows * COMPACT_CARD_HEIGHT + ROOM_BORDER_OVERHEAD;
        let gap = if idx == 0 { 0 } else { ROOM_GAP };
        if used.saturating_add(needed).saturating_add(gap) > area.height {
            break;
        }
        if gap > 0 {
            constraints.push(Constraint::Length(gap));
        }
        constraints.push(Constraint::Length(needed));
        visible_rooms.push(room);
        used += needed + gap;
    }

    if visible_rooms.is_empty() {
        // Fall back to letting the first room consume what it can.
        render_room(frame, app, &rooms[0], area, None, None, Some(0), true);
        return;
    }

    // Prefix sum of session counts so each visible room knows its global flat-index base.
    let prefix_sums: Vec<usize> = rooms
        .iter()
        .scan(0usize, |acc, r| {
            let v = *acc;
            *acc += r.session_indices.len();
            Some(v)
        })
        .collect();

    // Pad remaining vertical space so rooms don't stretch.
    if used < area.height {
        constraints.push(Constraint::Min(0));
    }

    let chunks = Layout::vertical(constraints).split(area);

    let mut chunk_idx = 0usize;
    for (i, room) in visible_rooms.iter().enumerate() {
        if i > 0 {
            chunk_idx += 1; // skip gap chunk
        }
        if chunk_idx >= chunks.len() {
            break;
        }
        // Find this room's index in the original rooms slice to match selection.
        let room_idx = rooms
            .iter()
            .position(|r| r.name == room.name)
            .unwrap_or(usize::MAX);
        let sel = if Some(room_idx) == selected_room_idx {
            Some(selected_local_idx)
        } else {
            None
        };
        let offset = prefix_sums.get(room_idx).copied().unwrap_or(0);
        // In compact (non-zoomed) mode, digits select agents, not rooms — drop room slot label.
        let slot = if true && app.view_zoomed_room.is_none() {
            None
        } else if i < ROOMS_PER_PAGE {
            Some(i + 1)
        } else {
            None
        };
        let agent_label_offset = if true && app.view_zoomed_room.is_none() {
            Some(offset)
        } else {
            None
        };
        render_room(frame, app, room, chunks[chunk_idx], slot, sel, agent_label_offset, true);
        chunk_idx += 1;
    }
}

fn render_room(
    frame: &mut Frame,
    app: &App,
    room: &Room,
    area: Rect,
    slot_num: Option<usize>,
    selected_agent: Option<usize>,
    agent_label_offset: Option<usize>,
    compact: bool,
) {
    let border_color = if room.has_input {
        if app.tick % 2 == 0 { Color::Yellow } else { Color::White }
    } else {
        Color::DarkGray
    };

    let title = match slot_num {
        Some(n) => format!(" [{}] {} ({}) ", n, room.name, room.session_indices.len()),
        None => format!(" {} ({}) ", room.name, room.session_indices.len()),
    };
    let title_style = if room.has_input {
        Style::default().fg(border_color)
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

    let card_width = if compact { COMPACT_CARD_WIDTH } else { CHAR_WIDTH };
    let card_height = if compact { COMPACT_CARD_HEIGHT } else { CHAR_HEIGHT };

    let chars_per_row = (inner.width / card_width).max(1) as usize;
    let char_rows: Vec<&[usize]> = room.session_indices.chunks(chars_per_row).collect();

    let needed_height = char_rows.len() as u16 * card_height;
    let v_pad = inner.height.saturating_sub(needed_height) / 2;
    let char_area = Rect {
        x: inner.x,
        y: inner.y + v_pad,
        width: inner.width,
        height: inner.height.saturating_sub(v_pad),
    };

    let row_constraints: Vec<Constraint> = char_rows
        .iter()
        .map(|_| Constraint::Length(card_height))
        .collect();
    let v_chunks = Layout::vertical(row_constraints).split(char_area);

    for (row_idx, indices) in char_rows.iter().enumerate() {
        if row_idx >= v_chunks.len() {
            break;
        }
        let col_constraints: Vec<Constraint> = indices
            .iter()
            .map(|_| Constraint::Length(card_width))
            .collect();
        let h_chunks = Layout::horizontal(col_constraints).split(v_chunks[row_idx]);

        for (col_idx, &session_idx) in indices.iter().enumerate() {
            if col_idx >= h_chunks.len() {
                break;
            }
            let flat_idx = row_idx * chars_per_row + col_idx;
            let is_selected = selected_agent == Some(flat_idx);
            let agent_label = agent_label_offset.and_then(|base| {
                let g = base + flat_idx;
                if g < 9 { Some(g + 1) } else { None }
            });
            if compact {
                render_character_compact(
                    frame,
                    app,
                    &app.sessions[session_idx],
                    h_chunks[col_idx],
                    app.tick,
                    is_selected,
                    agent_label,
                );
            } else {
                render_character(
                    frame,
                    app,
                    &app.sessions[session_idx],
                    h_chunks[col_idx],
                    app.tick,
                    is_selected,
                    agent_label,
                );
            }
        }
    }
}

fn render_character(
    frame: &mut Frame,
    app: &App,
    session: &Session,
    area: Rect,
    tick: u64,
    is_selected: bool,
    agent_label: Option<usize>,
) {
    if area.height < 3 || area.width < 4 {
        return;
    }

    let offset = session_phase_offset(&session.session_id);
    let anim_frame = animation_frame(&session.status, tick + offset);
    let (sprite, palette) = sprite_data(&session.status, anim_frame);
    let ratio = session.token_ratio();

    // Selection highlight background
    if is_selected {
        let bg = Block::default()
            .style(Style::default().bg(Color::Rgb(40, 40, 60)));
        frame.render_widget(bg, area);
    }

    let mut lines: Vec<Line> = Vec::new();

    // Pixel art sprite (5 terminal lines)
    let sprite_lines = render_sprite_lines(sprite, palette);
    lines.extend(sprite_lines);

    // Label priority: LLM summary > last user prompt > tmux session name
    let summary_owned = app
        .summarizer
        .store
        .get(&session.session_id)
        .map(|s: String| sanitize_prompt(s.as_str()))
        .filter(|s| !s.is_empty());
    let prompt_owned = session
        .last_user_prompt
        .as_deref()
        .map(sanitize_prompt)
        .filter(|s| !s.is_empty());
    let name = summary_owned
        .as_deref()
        .or(prompt_owned.as_deref())
        .or(session.tmux_session.as_deref())
        .unwrap_or("???");
    let name_style = if is_selected {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let name_lines = wrap_label(name, area.width as usize, NAME_LINES as usize);
    for line in name_lines.iter().take(NAME_LINES as usize) {
        lines.push(Line::from(Span::styled(line.clone(), name_style)));
    }

    // Git branch (no padding above — sits right under the name).
    let branch = session.branch.as_deref().unwrap_or("");
    lines.push(Line::from(Span::styled(
        truncate_str(branch, area.width as usize),
        Style::default().fg(Color::Green),
    )));

    // One blank line of breathing room between branch and context bar.
    lines.push(Line::from(""));

    // Context bar
    let (bar_str, bar_color) = context_bar(ratio);
    lines.push(Line::from(Span::styled(
        truncate_str(&bar_str, area.width as usize),
        Style::default().fg(bar_color),
    )));

    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, area);

    if let Some(n) = agent_label {
        let label = format!("[{}]", n);
        let label_w = (label.chars().count() as u16).min(area.width);
        if label_w > 0 {
            let label_rect = Rect { x: area.x, y: area.y, width: label_w, height: 1 };
            let style = Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD);
            frame.render_widget(Paragraph::new(label).style(style), label_rect);
        }
    }
}

fn elapsed_hms(started_at: u64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let elapsed = now.saturating_sub(started_at);
    let h = elapsed / 3600;
    let m = (elapsed % 3600) / 60;
    let s = elapsed % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

fn wide_context_bar(ratio: f64, total_width: usize) -> (Vec<Span<'static>>, Color) {
    let pct = (ratio * 100.0) as u32;
    let pct_str = format!(" {}%", pct);
    let pct_len = pct_str.chars().count();
    let bar_width = total_width.saturating_sub(pct_len + 1).max(1);
    let filled = (ratio * bar_width as f64).round().min(bar_width as f64) as usize;
    let empty = bar_width.saturating_sub(filled);
    let color = if ratio > 0.75 {
        Color::Red
    } else if ratio > 0.40 {
        Color::Yellow
    } else {
        Color::Green
    };
    let dim = Color::Rgb(60, 60, 60);
    let spans = vec![
        Span::styled(
            "\u{2588}".repeat(filled),
            Style::default().fg(color),
        ),
        Span::styled(
            "\u{2588}".repeat(empty),
            Style::default().fg(dim),
        ),
        Span::styled(pct_str, Style::default().fg(color).add_modifier(Modifier::BOLD)),
    ];
    (spans, color)
}

fn render_character_compact(
    frame: &mut Frame,
    app: &App,
    session: &Session,
    area: Rect,
    tick: u64,
    is_selected: bool,
    agent_label: Option<usize>,
) {
    if area.height < 4 || area.width < (COMPACT_SPRITE_COLS + 6) {
        return;
    }

    let status_color = status_color(&session.status);
    let border_color = if is_selected { Color::Cyan } else { Color::Rgb(60, 60, 70) };

    let card = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1));
    let inner = card.inner(area);
    frame.render_widget(card, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Split: sprite | text
    let sprite_w = COMPACT_SPRITE_COLS.min(inner.width.saturating_sub(4));
    let chunks = Layout::horizontal([
        Constraint::Length(sprite_w),
        Constraint::Min(1),
    ])
    .split(inner);

    // Sprite area — vertically center the 5-row sprite within the inner height.
    let sprite_area = chunks[0];
    let offset = session_phase_offset(&session.session_id);
    let anim_frame = animation_frame(&session.status, tick + offset);
    let (sprite, palette) = sprite_data(&session.status, anim_frame);
    let sprite_lines = render_sprite_lines(sprite, palette);
    let sprite_pad = sprite_area.height.saturating_sub(SPRITE_RENDER_H) / 2;
    let sprite_rect = Rect {
        x: sprite_area.x,
        y: sprite_area.y + sprite_pad,
        width: sprite_area.width,
        height: SPRITE_RENDER_H.min(sprite_area.height),
    };
    frame.render_widget(
        Paragraph::new(sprite_lines).alignment(Alignment::Left),
        sprite_rect,
    );

    // Text area
    let text_area = chunks[1];
    let text_w = text_area.width as usize;

    // Label priority: LLM summary > last user prompt > tmux session name
    let summary_owned = app
        .summarizer
        .store
        .get(&session.session_id)
        .map(|s: String| sanitize_prompt(s.as_str()))
        .filter(|s| !s.is_empty());
    let prompt_owned = session
        .last_user_prompt
        .as_deref()
        .map(sanitize_prompt)
        .filter(|s| !s.is_empty());
    let name = summary_owned
        .as_deref()
        .or(prompt_owned.as_deref())
        .or(session.tmux_session.as_deref())
        .unwrap_or("???");
    let name_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let name_lines = wrap_label(name, text_w, 2);

    let branch = session.branch.as_deref().unwrap_or("");
    let timer = elapsed_hms(session.started_at);
    let status_label = session.status.label();

    let mut lines: Vec<Line> = Vec::new();
    for line in name_lines.iter().take(2) {
        lines.push(Line::from(Span::styled(line.clone(), name_style)));
    }
    while lines.len() < 2 {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        truncate_str(branch, text_w),
        Style::default().fg(Color::Green),
    )));
    lines.push(Line::from(vec![
        Span::styled("\u{25CF} ", Style::default().fg(status_color)),
        Span::styled(
            status_label.to_string(),
            Style::default().fg(Color::White),
        ),
        Span::raw("   "),
        Span::styled("\u{29D6} ", Style::default().fg(Color::DarkGray)),
        Span::styled(timer, Style::default().fg(Color::Gray)),
    ]));
    let (bar_spans, _bar_color) = wide_context_bar(session.token_ratio(), text_w);
    lines.push(Line::from(bar_spans));

    frame.render_widget(Paragraph::new(lines), text_area);

    if let Some(n) = agent_label {
        let label = format!("[{}]", n);
        let label_w = (label.chars().count() as u16).min(area.width);
        if label_w > 0 {
            let label_rect = Rect {
                x: area.x + 1,
                y: area.y,
                width: label_w,
                height: 1,
            };
            let style = Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD);
            frame.render_widget(Paragraph::new(label).style(style), label_rect);
        }
    }
}

fn render_empty(frame: &mut Frame, area: Rect, _tick: u64) {
    let (sprite, palette) = sprite_data(&SessionStatus::Idle, 0);
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    lines.extend(render_sprite_lines(sprite, palette));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "No active sessions",
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let rooms = group_into_rooms_stable(&app.sessions, &app.filtered_indices(), &app.view_room_order);
    let total_pages = (rooms.len() + ROOMS_PER_PAGE - 1) / ROOMS_PER_PAGE;
    let page = app.view_page.min(total_pages.saturating_sub(1));

    let mut spans = vec![];

    if app.view_zoomed_room.is_some() {
        spans.push(Span::styled("h/l", Style::default().fg(Color::Cyan)));
        spans.push(Span::raw(" select  "));
        spans.push(Span::styled("Enter", Style::default().fg(Color::Cyan)));
        spans.push(Span::raw(" switch  "));
        spans.push(Span::styled("x", Style::default().fg(Color::Cyan)));
        spans.push(Span::raw(" kill  "));
        spans.push(Span::styled("n", Style::default().fg(Color::Cyan)));
        spans.push(Span::raw(" new  "));
        if !true {
            spans.push(Span::styled("Esc", Style::default().fg(Color::Cyan)));
            spans.push(Span::raw(" back  "));
        }
    } else {
        spans.push(Span::styled("1-4", Style::default().fg(Color::Cyan)));
        spans.push(Span::raw(" zoom  "));
        if total_pages > 1 {
            spans.push(Span::styled("j/k", Style::default().fg(Color::Cyan)));
            spans.push(Span::raw(format!(" page ({}/{})  ", page + 1, total_pages)));
        }
    }

    spans.push(Span::styled("/", Style::default().fg(Color::Cyan)));
    spans.push(Span::raw(" search  "));
    spans.push(Span::styled("i", Style::default().fg(Color::Cyan)));
    spans.push(Span::raw(" next input  "));
    spans.push(Span::styled("v", Style::default().fg(Color::Cyan)));
    spans.push(Span::raw(" table  "));
    spans.push(Span::styled("q", Style::default().fg(Color::Cyan)));
    spans.push(Span::raw(" quit"));

    let footer = Paragraph::new(Line::from(spans));
    frame.render_widget(footer, area);
}

// ── Helpers ──────────────────────────────────────────────────────────

fn sanitize_prompt(raw: &str) -> String {
    let collapsed: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    collapsed.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn wrap_label(text: &str, max_width: usize, max_lines: usize) -> Vec<String> {
    if max_width == 0 || max_lines == 0 {
        return Vec::new();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<String> = Vec::with_capacity(max_lines);
    let mut current = String::new();

    for w in &words {
        let word_chars = w.chars().count();
        let cur_chars = current.chars().count();
        let needed = if cur_chars == 0 { word_chars } else { cur_chars + 1 + word_chars };

        if needed <= max_width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(w);
            continue;
        }

        if cur_chars > 0 {
            lines.push(std::mem::take(&mut current));
            if lines.len() == max_lines {
                break;
            }
        }

        if word_chars <= max_width {
            current.push_str(w);
        } else {
            let chunk: String = w.chars().take(max_width).collect();
            current = chunk;
        }
    }

    if lines.len() < max_lines && !current.is_empty() {
        lines.push(std::mem::take(&mut current));
    }

    let total_chars: usize = words.iter().map(|w| w.chars().count()).sum::<usize>()
        + words.len().saturating_sub(1);
    let used_chars: usize = lines.iter().map(|l| l.chars().count()).sum::<usize>()
        + lines.len().saturating_sub(1);

    if used_chars < total_chars {
        if let Some(last) = lines.last_mut() {
            if last.chars().count() == max_width {
                let mut chars: Vec<char> = last.chars().collect();
                if chars.len() > 1 {
                    chars.pop();
                }
                chars.push('\u{2026}');
                *last = chars.into_iter().collect();
            } else {
                last.push('\u{2026}');
                if last.chars().count() > max_width {
                    let truncated: String = last.chars().take(max_width).collect();
                    *last = truncated;
                }
            }
        }
    }

    lines
}

fn truncate_str(s: &str, max_width: usize) -> String {
    let char_count: usize = s.chars().count();
    if char_count <= max_width {
        s.to_string()
    } else if max_width > 1 {
        let truncated: String = s.chars().take(max_width - 1).collect();
        format!("{}\u{2026}", truncated)
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_session(cwd: &str, status: SessionStatus, last_activity: Option<&str>) -> Session {
        Session {
            session_id: String::new(),
            project_name: cwd.to_string(),
            branch: None,
            cwd: cwd.to_string(),
            relative_dir: None,
            tmux_session: None,
            pane_target: None,
            model: None,
            effort: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            status,
            pid: None,
            last_activity: last_activity.map(|s| s.to_string()),
            started_at: 0,
            jsonl_path: PathBuf::new(),
            last_file_size: 0,
            tags: std::collections::HashMap::new(),
            last_user_prompt: None,
        }
    }

    #[test]
    fn rooms_with_input_sort_first() {
        let sessions = vec![
            make_session("/a", SessionStatus::Idle, Some("2026-03-16T10:00:00Z")),
            make_session("/b", SessionStatus::Input, Some("2026-03-16T09:00:00Z")),
        ];
        let all: Vec<usize> = (0..sessions.len()).collect();
        let rooms = group_into_rooms(&sessions, &all);
        assert_eq!(rooms[0].name, "/b");
        assert_eq!(rooms[1].name, "/a");
    }

    #[test]
    fn secondary_sort_by_last_activity_descending() {
        let sessions = vec![
            make_session("/old", SessionStatus::Idle, Some("2026-03-16T08:00:00Z")),
            make_session("/recent", SessionStatus::Idle, Some("2026-03-16T12:00:00Z")),
            make_session("/mid", SessionStatus::Idle, Some("2026-03-16T10:00:00Z")),
        ];
        let all: Vec<usize> = (0..sessions.len()).collect();
        let rooms = group_into_rooms(&sessions, &all);
        assert_eq!(rooms[0].name, "/recent");
        assert_eq!(rooms[1].name, "/mid");
        assert_eq!(rooms[2].name, "/old");
    }

    #[test]
    fn new_sessions_sort_last() {
        let sessions = vec![
            make_session("/egg", SessionStatus::New, None),
            make_session("/active", SessionStatus::Idle, Some("2026-03-16T10:00:00Z")),
        ];
        let all: Vec<usize> = (0..sessions.len()).collect();
        let rooms = group_into_rooms(&sessions, &all);
        assert_eq!(rooms[0].name, "/active");
        assert_eq!(rooms[1].name, "/egg");
    }

    #[test]
    fn room_activity_uses_max_across_sessions() {
        let sessions = vec![
            make_session("/repo", SessionStatus::Idle, Some("2026-03-16T08:00:00Z")),
            make_session("/repo", SessionStatus::New, None),
            make_session("/repo", SessionStatus::Idle, Some("2026-03-16T12:00:00Z")),
            make_session("/other", SessionStatus::Idle, Some("2026-03-16T10:00:00Z")),
        ];
        let all: Vec<usize> = (0..sessions.len()).collect();
        let rooms = group_into_rooms(&sessions, &all);
        assert_eq!(rooms[0].name, "/repo");
        assert_eq!(rooms[1].name, "/other");
    }

    #[test]
    fn input_rooms_also_sorted_by_activity() {
        let sessions = vec![
            make_session("/old-input", SessionStatus::Input, Some("2026-03-16T08:00:00Z")),
            make_session("/new-input", SessionStatus::Input, Some("2026-03-16T12:00:00Z")),
        ];
        let all: Vec<usize> = (0..sessions.len()).collect();
        let rooms = group_into_rooms(&sessions, &all);
        assert_eq!(rooms[0].name, "/new-input");
        assert_eq!(rooms[1].name, "/old-input");
    }

    #[test]
    fn worktrees_share_room_by_project_name() {
        // Two sessions with different CWDs but same project_name should be in the same room
        let mut s1 = make_session("/repos/line5", SessionStatus::Idle, Some("2026-03-16T10:00:00Z"));
        s1.project_name = "line5".to_string();
        let mut s2 = make_session("/worktrees/line5-feat", SessionStatus::Working, Some("2026-03-16T11:00:00Z"));
        s2.project_name = "line5".to_string();
        let sessions = [s1, s2];
        let all: Vec<usize> = (0..sessions.len()).collect();
        let rooms = group_into_rooms(&sessions, &all);
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].name, "line5");
        assert_eq!(rooms[0].session_indices.len(), 2);
    }

    #[test]
    fn subproject_gets_separate_room() {
        // Root and subproject should be different rooms
        let mut s1 = make_session("/repos/line5", SessionStatus::Idle, Some("2026-03-16T10:00:00Z"));
        s1.project_name = "line5".to_string();
        let mut s2 = make_session("/repos/line5/tools/solo", SessionStatus::Idle, Some("2026-03-16T11:00:00Z"));
        s2.project_name = "line5".to_string();
        s2.relative_dir = Some("tools/solo".to_string());
        let sessions = [s1, s2];
        let all: Vec<usize> = (0..sessions.len()).collect();
        let rooms = group_into_rooms(&sessions, &all);
        assert_eq!(rooms.len(), 2);
    }

    #[test]
    fn mixed_input_and_activity_sorting() {
        let sessions = vec![
            make_session("/idle-recent", SessionStatus::Idle, Some("2026-03-16T15:00:00Z")),
            make_session("/input-old", SessionStatus::Input, Some("2026-03-16T08:00:00Z")),
            make_session("/egg", SessionStatus::New, None),
            make_session("/idle-old", SessionStatus::Idle, Some("2026-03-16T09:00:00Z")),
        ];
        let all: Vec<usize> = (0..sessions.len()).collect();
        let rooms = group_into_rooms(&sessions, &all);
        assert_eq!(rooms[0].name, "/input-old");   // input first regardless of activity
        assert_eq!(rooms[1].name, "/idle-recent");  // most recent activity
        assert_eq!(rooms[2].name, "/idle-old");     // older activity
        assert_eq!(rooms[3].name, "/egg");           // no activity last
    }
}
