mod app;
mod cli;
mod history;
mod model;
mod new_session;
mod park;
mod session;
mod summarizer;
mod tmux;
mod ui;
mod view_lock;
mod view_ui;

use std::io;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;

use app::{App, ViewMode};
use cli::{Cli, Command};

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::New) => {
            let result = new_session::run_new_session_form()?;
            if let Some(name) = result {
                tmux::switch_to_pane(&name);
            }
        }
        Some(Command::Launch { name, cwd, command, attach, tag }) => {
            let (default_name, default_cwd) = tmux::default_new_session_info();
            let session_name = name.as_deref().unwrap_or(&default_name);
            let session_cwd = cwd.as_deref().unwrap_or(&default_cwd);
            match tmux::create_session(session_name, session_cwd, command.as_deref(), &tag) {
                Ok(name) => {
                    if attach {
                        tmux::switch_to_pane(&name);
                    }
                    eprintln!("Session: {name}");
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some(Command::Resume { id, name, no_attach }) => {
            if let Some(session_id) = id {
                match tmux::resume_session(&session_id, name.as_deref()) {
                    Ok(sess) => {
                        if !no_attach {
                            tmux::switch_to_pane(&sess);
                        }
                        eprintln!("Resumed in session: {sess}");
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                let result = history::run_resume_picker()?;
                if let Some((session_id, sess_name)) = result {
                    match tmux::resume_session(&session_id, Some(&sess_name)) {
                        Ok(sess) => {
                            tmux::switch_to_pane(&sess);
                            eprintln!("Resumed in session: {sess}");
                        }
                        Err(e) => {
                            eprintln!("Error: {e}");
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
        Some(Command::Next) => {
            let mut app = App::new();
            app.refresh();
            if let Some(session) = app.sessions.iter().find(|s| s.status == session::SessionStatus::Input) {
                if let Some(target) = &session.pane_target {
                    tmux::switch_to_pane(target);
                }
            }
        }
        Some(Command::Json { tag }) => {
            let mut app = App::new();
            app.refresh();
            println!("{}", app.to_json(&tag));
        }
        Some(Command::Park) => {
            park::park();
        }
        Some(Command::Unpark) => {
            park::unpark();
        }
        Some(Command::Daemon { interval }) => {
            run_daemon(interval);
        }
        Some(Command::View { compact }) => {
            run_tui(ViewMode::View, compact)?;
        }
        None => {
            run_tui(ViewMode::Table, false)?;
        }
    }

    Ok(())
}

fn run_daemon(interval_secs: u64) {
    let mut app = App::new();
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

fn run_tui(start_mode: ViewMode, compact: bool) -> io::Result<()> {
    let _view_lock = if start_mode == ViewMode::View {
        view_lock::ViewLock::acquire()
    } else {
        None
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, start_mode, compact);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    start_mode: ViewMode,
    compact: bool,
) -> io::Result<()> {
    let mut app = App::new();
    app.view_mode = start_mode;
    app.view_compact = compact;
    app.refresh();

    let refresh_interval = Duration::from_secs(2);
    let mut last_refresh = Instant::now();

    loop {
        if app.view_mode == ViewMode::View {
            view_ui::resolve_zoom(&mut app);
        }
        terminal.draw(|f| {
            match app.view_mode {
                ViewMode::Table => ui::render(f, &app),
                ViewMode::View => view_ui::render(f, &app),
            }
        })?;

        app.advance_tick();

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }

        if app.should_quit {
            return Ok(());
        }

        if last_refresh.elapsed() >= refresh_interval {
            app.refresh();
            last_refresh = Instant::now();
        }
    }
}
