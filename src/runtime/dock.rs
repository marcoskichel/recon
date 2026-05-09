//! `roostr dock` — compact mini-sprite sidebar designed for a thin tmux pane.

use std::io::{self, Write};
use std::process::Command as ProcCommand;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;

use crate::app::App;
use crate::session::Session;
use crate::view_lock;
use crate::view_ui;

use super::refresh::run_refresh_worker;

/// Set up the dock terminal (alternate screen + OSC pane title), run the
/// event loop, and tear down.
///
/// # Errors
/// Returns any I/O error from terminal setup, draw, or input polling.
pub fn run_dock() -> io::Result<()> {
    let _view_lock = view_lock::ViewLock::acquire();

    set_pane_title()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_dock_loop(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(error) = result {
        let mut stderr = io::stderr();
        writeln!(stderr, "Error: {error}")?;
    }
    Ok(())
}

/// Emit the OSC pane-title escape so dock-toggle and hooks can identify
/// this pane regardless of which command spawned it.
///
/// # Errors
/// Returns the I/O error from writing to stdout.
fn set_pane_title() -> io::Result<()> {
    let mut stdout = io::stdout();
    write!(stdout, "\u{1b}]2;roostr-dock\u{1b}\\")?;
    stdout.flush()?;
    Ok(())
}

/// Identify the session currently selected in the dock view.
fn dock_selected_session_id(app: &App) -> Option<String> {
    let filtered = app.filtered_indices();
    let rooms =
        view_ui::rooms::group_into_rooms_stable(&app.sessions, &filtered, &app.view_room_order);
    let indices: Vec<usize> =
        rooms.into_iter().flat_map(|room| room.session_indices.into_iter()).collect();
    if indices.is_empty() {
        return None;
    }
    let last = indices.len().saturating_sub(1);
    let selected_idx = app.view_selected_agent.min(last);
    let session_idx = indices.get(selected_idx)?;
    let session = app.sessions.get(*session_idx)?;
    Some(session.id.clone())
}

/// Spawn the `roostr dock-info` popup for the currently selected session.
fn open_info_popup(app: &App) {
    let Some(session_id) = dock_selected_session_id(app) else {
        return;
    };
    let popup_cmd = format!("roostr dock-info {session_id}");
    let _ = ProcCommand::new("tmux")
        .args([
            "display-popup",
            "-E",
            "-w",
            "70%",
            "-h",
            "60%",
            "-T",
            " Session detail ",
            &popup_cmd,
        ])
        .status();
}

/// Vertical-axis remapping for the narrow dock: j/k and arrows move between
/// agent cards, which are translated into the wider TUI's h/l keys.
const fn translate_key(app: &App, event: KeyEvent) -> KeyEvent {
    if app.filter_active() {
        return event;
    }
    let mut translated = event;
    translated.code = match event.code {
        KeyCode::Char('j') | KeyCode::Down => KeyCode::Char('l'),
        KeyCode::Char('k') | KeyCode::Up => KeyCode::Char('h'),
        KeyCode::Char(_)
        | KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Null
        | KeyCode::Esc
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => event.code,
    };
    translated
}

/// `true` when `event` should actually quit the dock (q / Esc / Ctrl-C).
const fn is_quit_key(app: &App, event: KeyEvent) -> bool {
    match event.code {
        KeyCode::Char('q') | KeyCode::Esc => !app.filter_active(),
        KeyCode::Char('c') if event.modifiers.contains(KeyModifiers::CONTROL) => true,
        KeyCode::Char(_)
        | KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => false,
    }
}

/// Outcome of handling a single key in the dock event loop.
enum KeyOutcome {
    /// Continue the inner key-drain loop.
    Continue,
    /// Break the inner key-drain loop (no more events queued).
    Break,
}

/// Process a single key event in the dock context.
///
/// # Errors
/// Propagates I/O errors from `event::poll`.
fn handle_dock_key(app: &mut App, event: KeyEvent) -> io::Result<KeyOutcome> {
    // `i` opens an info popup for the selected session. Skip the rest of
    // the key pipeline so the main key handler doesn't see it.
    if !app.filter_active() && matches!(event.code, KeyCode::Char('i')) {
        open_info_popup(app);
        if !event::poll(Duration::from_millis(0))? {
            return Ok(KeyOutcome::Break);
        }
        return Ok(KeyOutcome::Continue);
    }

    let translated = translate_key(app, event);
    let quit_key = is_quit_key(app, event);

    // Only q / Esc / Ctrl-C should quit the dock. Other actions (Enter to
    // switch, x to kill, 1-9 to jump, n to new) set should_quit so the main
    // TUI exits; the dock should stay open in the background.
    let was_quit = app.should_quit;
    app.handle_key(translated);
    if app.should_quit && !was_quit && !quit_key {
        app.should_quit = false;
    }
    Ok(KeyOutcome::Continue)
}

/// Drain available key events into `app`, blocking up to ~100ms for the
/// first one.
///
/// # Errors
/// Propagates I/O errors from `event::poll` / `event::read`.
fn drain_input(app: &mut App) -> io::Result<()> {
    if !event::poll(Duration::from_millis(100))? {
        return Ok(());
    }
    loop {
        if let Event::Key(event) = event::read()? {
            match handle_dock_key(app, event)? {
                KeyOutcome::Continue => {}
                KeyOutcome::Break => return Ok(()),
            }
        }
        if !event::poll(Duration::from_millis(0))? {
            return Ok(());
        }
    }
}

/// Drain pending session snapshots, applying only the most recent one.
fn drain_snapshots(rx: &mpsc::Receiver<Vec<Session>>, app: &mut App) {
    let mut latest: Option<Vec<Session>> = None;
    while let Ok(snapshot) = rx.try_recv() {
        latest = Some(snapshot);
    }
    if let Some(snapshot) = latest {
        app.apply_snapshot(snapshot);
    }
}

/// Drive the dock event loop until `app.should_quit` is set.
///
/// # Errors
/// Propagates I/O errors from drawing or event handling.
fn run_dock_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = App::new();

    let (tx, rx) = mpsc::channel::<Vec<Session>>();
    let initial_prev = app.snapshot_prev();
    thread::spawn(move || run_refresh_worker(&tx, initial_prev));

    loop {
        terminal.draw(|frame| view_ui::dock::render_dock(frame, &app))?;
        app.advance_tick();

        drain_input(&mut app)?;
        drain_snapshots(&rx, &mut app);

        if app.should_quit {
            app.save_state();
            return Ok(());
        }
    }
}
