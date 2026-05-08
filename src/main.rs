mod app;
mod cli;
mod model;
mod session;
mod summarizer;
mod tmux;
mod view_lock;
mod view_ui;

use std::collections::HashMap;
use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use clap::Parser;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;

use app::App;
use cli::{Cli, Command};
use session::Session;

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Daemon { interval }) => {
            run_daemon(interval);
            Ok(())
        }
        None => run_tui(),
    }
}

fn run_daemon(interval_secs: u64) {
    let mut app = App::new_blocking();
    if !app.summarizer.enabled() {
        eprintln!("recon daemon: summarizer disabled (no Ollama and no ANTHROPIC_API_KEY).");
        std::process::exit(1);
    }
    eprintln!("recon daemon: polling every {}s. Ctrl-C to stop.", interval_secs);
    let interval = Duration::from_secs(interval_secs.max(2));
    let mut was_paused = false;
    loop {
        if view_lock::is_active() {
            if !was_paused {
                eprintln!("recon daemon: view active, pausing polling.");
                was_paused = true;
            }
        } else {
            if was_paused {
                eprintln!("recon daemon: view closed, resuming polling.");
                was_paused = false;
            }
            app.refresh();
        }
        std::thread::sleep(interval);
    }
}

fn run_tui() -> io::Result<()> {
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

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = App::new();

    let (tx, rx) = mpsc::channel::<Vec<Session>>();
    let initial_prev = app.snapshot_prev();
    thread::spawn(move || run_refresh_worker(tx, initial_prev));

    loop {
        view_ui::resolve_zoom(&mut app);
        terminal.draw(|f| view_ui::render(f, &app))?;
        app.advance_tick();

        if event::poll(Duration::from_millis(100))? {
            loop {
                if let Event::Key(key) = event::read()? {
                    app.handle_key(key);
                }
                if !event::poll(Duration::from_millis(0))? {
                    break;
                }
            }
        }

        let mut latest: Option<Vec<Session>> = None;
        while let Ok(snapshot) = rx.try_recv() {
            latest = Some(snapshot);
        }
        if let Some(snapshot) = latest {
            app.apply_snapshot(snapshot);
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn run_refresh_worker(tx: mpsc::Sender<Vec<Session>>, initial_prev: HashMap<String, Session>) {
    let interval = Duration::from_secs(2);
    let mut prev = initial_prev;
    let mut first = true;
    loop {
        if !first {
            thread::sleep(interval);
        }
        first = false;
        let sessions: Vec<Session> = session::discover_sessions(&prev)
            .into_iter()
            .filter(|s| s.tmux_session.is_some())
            .collect();
        prev = sessions
            .iter()
            .map(|s| (s.session_id.clone(), s.clone()))
            .collect();
        if tx.send(sessions).is_err() {
            break;
        }
    }
}
