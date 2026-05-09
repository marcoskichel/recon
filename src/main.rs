mod app;
mod cli;
mod model;
mod session;
mod setup;
mod state;
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
        Some(Command::Dock) => run_dock(),
        Some(Command::DockToggle) => run_dock_toggle(),
        Some(Command::DockFocus) => run_dock_focus(),
        Some(Command::DockInfo { session_id }) => run_dock_info(&session_id),
        Some(Command::Toggle) => run_toggle(),
        Some(Command::Setup { action }) => setup::run(action),
        None => run_tui(),
    }
}

fn run_toggle() -> io::Result<()> {
    use std::process::Command as ProcCommand;

    let tmux = |args: &[&str]| -> io::Result<String> {
        let out = ProcCommand::new("tmux").args(args).output()?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).to_string();
            return Err(io::Error::new(io::ErrorKind::Other, msg));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };

    if std::env::var_os("TMUX").is_none() {
        eprintln!("roostr toggle: not inside tmux");
        std::process::exit(1);
    }

    let current_win = tmux(&["display-message", "-p", "#{window_id}"])?;
    let current_name = tmux(&["display-message", "-p", "#{window_name}"])?;
    let windows = tmux(&[
        "list-windows",
        "-F",
        "#{window_id} #{window_name}",
    ])?;

    let roostr_win = windows.lines().find_map(|line| {
        let mut parts = line.splitn(2, ' ');
        let id = parts.next()?;
        let name = parts.next().unwrap_or("");
        if name == "roostr" {
            Some(id.to_string())
        } else {
            None
        }
    });

    match roostr_win {
        Some(id) if current_name == "roostr" || id == current_win => {
            tmux(&["kill-window", "-t", &id])?;
        }
        Some(id) => {
            tmux(&["select-window", "-t", &id])?;
        }
        None => {
            tmux(&["new-window", "-n", "roostr", "roostr"])?;
        }
    }
    Ok(())
}

fn run_dock_info(session_id: &str) -> io::Result<()> {
    use crossterm::style::Stylize;
    use std::io::Write;

    let mut app = App::new();
    app.refresh();

    let session = app
        .sessions
        .iter()
        .find(|s| s.session_id == session_id)
        .cloned();

    let mut out = io::stdout();

    let width = crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    // Reserve a little inner padding from popup borders.
    let inner_w = width.saturating_sub(4).max(40);

    match session {
        Some(s) => {
            let title = s.tmux_session.clone().unwrap_or_else(|| "?".to_string());
            let branch = s.branch.clone().unwrap_or_else(|| "-".to_string());
            let model = s.model.clone().unwrap_or_else(|| "?".to_string());
            let status_str = match s.status {
                session::SessionStatus::New => "New",
                session::SessionStatus::Working => "Working",
                session::SessionStatus::Idle => "Idle",
                session::SessionStatus::Input => "Input",
            };
            let total = s.total_input_tokens + s.total_output_tokens;
            let pct = (s.token_ratio() * 100.0) as u32;
            let summary = app
                .summarizer
                .store
                .get(&s.session_id)
                .filter(|t| !t.trim().is_empty())
                .or_else(|| s.last_user_prompt.clone())
                .unwrap_or_else(|| "(no summary yet)".to_string());

            // === Summary first, big and bold — most important info. ===
            writeln!(out)?;
            writeln!(out, "  {}", "SUMMARY".dim().bold())?;
            writeln!(out, "  {}", "─".repeat(inner_w.saturating_sub(2)).dim())?;
            for line in wrap_text(&summary, inner_w.saturating_sub(2)) {
                writeln!(out, "  {}", line.as_str().bold().cyan())?;
            }
            writeln!(out)?;

            // Compact metadata block — secondary.
            writeln!(out, "  {} {}", "session".dim(), title.as_str().bold())?;
            writeln!(out, "  {} {}", "branch ".dim(), branch.as_str().green())?;
            writeln!(out, "  {} {}", "status ".dim(), status_str)?;
            writeln!(out, "  {} {}", "model  ".dim(), model)?;
            writeln!(
                out,
                "  {} {pct}%  ({total} used; in {} / out {})",
                "tokens ".dim(),
                s.total_input_tokens,
                s.total_output_tokens
            )?;
            writeln!(out, "  {} {}", "cwd    ".dim(), s.cwd)?;
        }
        None => {
            writeln!(out, "Session not found: {}", session_id)?;
        }
    }

    writeln!(out)?;
    writeln!(out, "{}", "Press any key to close…".dim())?;
    out.flush()?;

    use crossterm::event::{self, Event};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    enable_raw_mode().ok();
    loop {
        if event::poll(std::time::Duration::from_millis(500)).unwrap_or(false) {
            if let Ok(Event::Key(_)) = event::read() {
                break;
            }
        }
    }
    disable_raw_mode().ok();
    Ok(())
}

fn run_dock_focus() -> io::Result<()> {
    use std::process::Command as ProcCommand;

    let tmux = |args: &[&str]| -> io::Result<String> {
        let out = ProcCommand::new("tmux").args(args).output()?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).to_string();
            return Err(io::Error::new(io::ErrorKind::Other, msg));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };

    if std::env::var_os("TMUX").is_none() {
        eprintln!("roostr dock-focus: not inside tmux");
        std::process::exit(1);
    }

    let win = tmux(&["display-message", "-p", "#{window_id}"])?;
    let panes = tmux(&[
        "list-panes",
        "-t",
        &win,
        "-F",
        "#{pane_id} #{pane_title}",
    ])?;

    let dock_pane = panes.lines().find_map(|line| {
        let mut parts = line.splitn(2, ' ');
        let id = parts.next()?;
        let title = parts.next().unwrap_or("");
        if title == "roostr-dock" {
            Some(id.to_string())
        } else {
            None
        }
    });

    if let Some(id) = dock_pane {
        tmux(&["select-pane", "-t", &id])?;
    } else {
        // No -d so the new pane takes focus.
        tmux(&[
            "split-window",
            "-h",
            "-l",
            "9",
            "-t",
            &win,
            "roostr dock",
        ])?;
    }
    Ok(())
}

fn run_dock_toggle() -> io::Result<()> {
    use std::process::Command as ProcCommand;

    let tmux = |args: &[&str]| -> io::Result<String> {
        let out = ProcCommand::new("tmux").args(args).output()?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).to_string();
            return Err(io::Error::new(io::ErrorKind::Other, msg));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };

    if std::env::var_os("TMUX").is_none() {
        eprintln!("roostr dock-toggle: not inside tmux");
        std::process::exit(1);
    }

    let win = tmux(&["display-message", "-p", "#{window_id}"])?;
    let panes = tmux(&[
        "list-panes",
        "-t",
        &win,
        "-F",
        "#{pane_id} #{pane_title}",
    ])?;

    let dock_pane = panes.lines().find_map(|line| {
        let mut parts = line.splitn(2, ' ');
        let id = parts.next()?;
        let title = parts.next().unwrap_or("");
        if title == "roostr-dock" {
            Some(id.to_string())
        } else {
            None
        }
    });

    if let Some(id) = dock_pane {
        tmux(&["kill-pane", "-t", &id])?;
    } else {
        let new_id = tmux(&[
            "split-window",
            "-h",
            "-l",
            "9",
            "-d",
            "-P",
            "-F",
            "#{pane_id}",
            "-t",
            &win,
            "roostr dock",
        ])?;
        tmux(&["select-pane", "-t", &new_id, "-T", "roostr-dock"])?;
    }
    Ok(())
}

fn run_daemon(interval_secs: u64) {
    let mut app = App::new_blocking();
    if !app.summarizer.enabled() {
        eprintln!("roostr daemon: summarizer disabled (no Ollama and no ANTHROPIC_API_KEY).");
        std::process::exit(1);
    }
    eprintln!("roostr daemon: polling every {}s. Ctrl-C to stop.", interval_secs);
    let interval = Duration::from_secs(interval_secs.max(2));
    let mut was_paused = false;
    loop {
        if view_lock::is_active() {
            if !was_paused {
                eprintln!("roostr daemon: view active, pausing polling.");
                was_paused = true;
            }
        } else {
            if was_paused {
                eprintln!("roostr daemon: view closed, resuming polling.");
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
            app.save_state();
            return Ok(());
        }
    }
}

fn run_dock() -> io::Result<()> {
    let _view_lock = view_lock::ViewLock::acquire();

    // Set tmux pane title via OSC so dock-toggle and hooks can identify
    // this pane reliably regardless of which command spawned it.
    {
        use std::io::Write;
        let mut stdout = io::stdout();
        let _ = write!(stdout, "\u{1b}]2;roostr-dock\u{1b}\\");
        let _ = stdout.flush();
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_dock_loop(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }
    Ok(())
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let word_w = word.chars().count();
        let cur_w = current.chars().count();
        let needed = if cur_w == 0 { word_w } else { cur_w + 1 + word_w };
        if needed <= max_width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        } else {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            if word_w <= max_width {
                current.push_str(word);
            } else {
                let chunk: String = word.chars().take(max_width).collect();
                current = chunk;
            }
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn dock_selected_session_id(app: &App) -> Option<String> {
    let filtered = app.filtered_indices();
    let rooms = view_ui::group_into_rooms_stable(
        &app.sessions,
        &filtered,
        &app.view_room_order,
    );
    let indices: Vec<usize> = rooms
        .into_iter()
        .flat_map(|r| r.session_indices.into_iter())
        .collect();
    if indices.is_empty() {
        return None;
    }
    let sel = app.view_selected_agent.min(indices.len() - 1);
    Some(app.sessions[indices[sel]].session_id.clone())
}

fn run_dock_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = App::new();

    let (tx, rx) = mpsc::channel::<Vec<Session>>();
    let initial_prev = app.snapshot_prev();
    thread::spawn(move || run_refresh_worker(tx, initial_prev));

    loop {
        terminal.draw(|f| view_ui::render_dock(f, &app))?;
        app.advance_tick();

        if event::poll(Duration::from_millis(100))? {
            loop {
                if let Event::Key(key) = event::read()? {
                    use crossterm::event::{KeyCode, KeyModifiers};

                    // `i` opens an info popup for the selected session.
                    // Skip the rest of the key pipeline so the main key
                    // handler doesn't see it.
                    if !app.filter_active && matches!(key.code, KeyCode::Char('i')) {
                        if let Some(session_id) = dock_selected_session_id(&app) {
                            let _ = std::process::Command::new("tmux")
                                .args([
                                    "display-popup",
                                    "-E",
                                    "-w",
                                    "70%",
                                    "-h",
                                    "60%",
                                    "-T",
                                    " Session detail ",
                                    &format!("roostr dock-info {}", session_id),
                                ])
                                .status();
                        }
                        if !event::poll(Duration::from_millis(0))? {
                            break;
                        }
                        continue;
                    }

                    let translated = if !app.filter_active {
                        let mut k = key;
                        k.code = match key.code {
                            KeyCode::Char('j') | KeyCode::Down => KeyCode::Char('l'),
                            KeyCode::Char('k') | KeyCode::Up => KeyCode::Char('h'),
                            c => c,
                        };
                        k
                    } else {
                        key
                    };

                    // Only q / Esc / Ctrl-C should quit the dock. Other
                    // actions (Enter to switch, x to kill, 1-9 to jump,
                    // n to new) set should_quit so the main TUI exits;
                    // the dock should stay open in the background.
                    let is_quit_key = match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => !app.filter_active,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
                        _ => false,
                    };
                    let was_quit = app.should_quit;
                    app.handle_key(translated);
                    if app.should_quit && !was_quit && !is_quit_key {
                        app.should_quit = false;
                    }
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
            app.save_state();
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
