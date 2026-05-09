//! Background worker thread that polls `session::discover_sessions` and
//! ships fresh snapshots over an mpsc channel to the foreground TUI.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::session::{self, Session};

/// Period between refresh polls.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// Run the refresh loop, sending snapshots through `tx` until the receiver
/// is dropped.
pub fn run_refresh_worker(tx: &mpsc::Sender<Vec<Session>>, initial_prev: HashMap<String, Session>) {
    let mut prev = initial_prev;
    let mut first = true;
    loop {
        if !first {
            thread::sleep(REFRESH_INTERVAL);
        }
        first = false;
        let sessions: Vec<Session> = session::discover_sessions(&prev)
            .into_iter()
            .filter(|sess| sess.tmux_name.is_some())
            .collect();
        prev = sessions.iter().map(|sess| (sess.id.clone(), sess.clone())).collect();
        if tx.send(sessions).is_err() {
            break;
        }
    }
}
