//! `roostr` (default) — the interactive ratatui dashboard.

use std::{
    io::{self, Write},
    sync::mpsc,
    thread,
    time::Duration,
};

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};

use super::refresh::run_refresh_worker;
use crate::{app::App, session::Session, view_lock, view_ui};

/// Set up the alternate-screen terminal, run the event loop, and tear down.
///
/// # Errors
/// Returns any I/O error from terminal setup, draw, or input polling.
pub fn run_tui() -> io::Result<()> {
    let _view_lock = view_lock::ViewLock::acquire();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(error) = result {
        let mut stderr = io::stderr();
        writeln!(stderr, "Error: {error}")?;
    }

    Ok(())
}

/// Drive the dashboard event loop until `app.should_quit` is set.
///
/// # Errors
/// Propagates I/O errors from drawing or input polling.
fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = App::new();

    let (tx, rx) = mpsc::channel::<Vec<Session>>();
    let initial_prev = app.snapshot_prev();
    thread::spawn(move || run_refresh_worker(&tx, initial_prev));

    loop {
        view_ui::resolve_zoom(&mut app);
        terminal.draw(|frame| view_ui::render(frame, &app))?;
        app.advance_tick();

        drain_input(&mut app)?;
        drain_snapshots(&rx, &mut app);

        if app.should_quit {
            app.save_state();
            return Ok(());
        }
    }
}

/// Drain available key events into `app`, blocking up to ~100ms for the
/// first one.
///
/// # Errors
/// Propagates I/O errors from `event::poll` / `event::read`.
fn drain_input(app: &mut App) -> io::Result<()> {
    if event::poll(Duration::from_millis(100))? {
        loop {
            if let Event::Key(event) = event::read()? {
                app.handle_key(event);
            }
            if !event::poll(Duration::from_millis(0))? {
                break;
            }
        }
    }
    Ok(())
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
