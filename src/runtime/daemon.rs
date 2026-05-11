//! `roostr daemon` — background summarizer poller.

use std::{
    io::{self, Write},
    time::Duration,
};

use crate::{app::App, view_lock};

/// Run the summarizer daemon loop.
///
/// # Errors
/// Returns an I/O error if writing to stderr fails.
pub fn run_daemon(interval_secs: u64) -> io::Result<()> {
    let mut app = App::new_blocking();
    let mut stderr = io::stderr();

    if !app.summarizer.enabled() {
        writeln!(
            stderr,
            "roostr daemon: summarizer disabled (no Ollama and no ANTHROPIC_API_KEY)."
        )?;
        std::process::exit(1);
    }
    writeln!(stderr, "roostr daemon: polling every {interval_secs}s. Ctrl-C to stop.")?;
    let interval = Duration::from_secs(interval_secs.max(2));
    let mut was_paused = false;
    loop {
        if view_lock::is_active() {
            if !was_paused {
                writeln!(stderr, "roostr daemon: view active, pausing polling.")?;
                was_paused = true;
            }
        } else {
            if was_paused {
                writeln!(stderr, "roostr daemon: view closed, resuming polling.")?;
                was_paused = false;
            }
            app.refresh();
        }
        std::thread::sleep(interval);
    }
}
