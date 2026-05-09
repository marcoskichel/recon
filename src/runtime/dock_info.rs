//! `roostr dock-info` — popup-friendly session detail view.

use std::io::{self, Write};
use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::style::Stylize;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::app::App;
use crate::model;
use crate::session::{Session, SessionStatus};

/// Wrap `text` to lines no wider than `max_width` graphemes.
///
/// Returns at least one (possibly empty) line so callers can iterate safely.
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let word_w = word.chars().count();
        let cur_w = current.chars().count();
        let needed =
            if cur_w == 0 { word_w } else { cur_w.saturating_add(1).saturating_add(word_w) };
        if needed <= max_width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        } else {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
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
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Map a session status to a static label string.
const fn status_label(status: &SessionStatus) -> &'static str {
    match *status {
        SessionStatus::New => "New",
        SessionStatus::Working => "Working",
        SessionStatus::Idle => "Idle",
        SessionStatus::Input => "Input",
    }
}

/// Compute integer percent (0..=100) of context window consumed by `sess`,
/// using only `u64` arithmetic so float / cast lints stay quiet.
fn token_percent(sess: &Session) -> u64 {
    let used = sess.total_input_tokens.saturating_add(sess.total_output_tokens);
    let window = sess.model.as_deref().map_or(200_000_u64, model::context_window);
    if window == 0 {
        return 0;
    }
    let scaled = used.saturating_mul(100);
    let percent = scaled.checked_div(window).unwrap_or(0);
    percent.min(100)
}

/// Pretty-print the detail card for the session matching `session_id`.
///
/// Writes to stdout and waits for a keypress, intended to be invoked from
/// `tmux display-popup`.
///
/// # Errors
/// Returns the underlying I/O error from terminal queries or `write!` calls.
pub fn run_dock_info(session_id: &str) -> io::Result<()> {
    let mut app = App::new();
    app.refresh();

    let session = app.sessions.iter().find(|sess| sess.id == session_id).cloned();

    let mut output = io::stdout();
    let width: usize = match crossterm::terminal::size() {
        Ok((cols, _)) => usize::from(cols),
        Err(_) => 80,
    };
    // Reserve a little inner padding from popup borders.
    let inner_w = width.saturating_sub(4).max(40);

    if let Some(sess) = session {
        write_detail_card(&mut output, &app, &sess, inner_w)?;
    } else {
        writeln!(output, "Session not found: {session_id}")?;
    }

    writeln!(output)?;
    writeln!(output, "{}", "Press any key to close…".dim())?;
    output.flush()?;

    enable_raw_mode().ok();
    loop {
        if event::poll(Duration::from_millis(500)).unwrap_or(false) {
            if let Ok(Event::Key(_)) = event::read() {
                break;
            }
        }
    }
    disable_raw_mode().ok();
    Ok(())
}

/// Render the session summary block + metadata block to `output`.
///
/// # Errors
/// Returns the I/O error from any underlying `writeln!`.
fn write_detail_card<W: Write>(
    output: &mut W,
    app: &App,
    sess: &Session,
    inner_w: usize,
) -> io::Result<()> {
    let title = sess.tmux_name.clone().unwrap_or_else(|| "?".to_string());
    let branch = sess.branch.clone().unwrap_or_else(|| "-".to_string());
    let model_label = sess.model.clone().unwrap_or_else(|| "?".to_string());
    let status_str = status_label(&sess.status);
    let total = sess.total_input_tokens.saturating_add(sess.total_output_tokens);
    let percent = token_percent(sess);
    let summary = app
        .summarizer
        .store
        .get(&sess.id)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| sess.last_user_prompt.clone())
        .unwrap_or_else(|| "(no summary yet)".to_string());

    // === Summary first, big and bold — most important info. ===
    writeln!(output)?;
    writeln!(output, "  {}", "SUMMARY".dim().bold())?;
    writeln!(output, "  {}", "─".repeat(inner_w.saturating_sub(2)).dim())?;
    for line in wrap_text(&summary, inner_w.saturating_sub(2)) {
        writeln!(output, "  {}", line.as_str().bold().cyan())?;
    }
    writeln!(output)?;

    // Compact metadata block — secondary.
    writeln!(output, "  {} {}", "session".dim(), title.as_str().bold())?;
    writeln!(output, "  {} {}", "branch ".dim(), branch.as_str().green())?;
    writeln!(output, "  {} {status_str}", "status ".dim())?;
    writeln!(output, "  {} {model_label}", "model  ".dim())?;
    writeln!(
        output,
        "  {} {percent}%  ({total} used; in {} / out {})",
        "tokens ".dim(),
        sess.total_input_tokens,
        sess.total_output_tokens
    )?;
    writeln!(output, "  {} {}", "cwd    ".dim(), sess.cwd)?;
    Ok(())
}
