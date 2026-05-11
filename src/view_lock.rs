//! Pidfile lock signalling that a TUI is currently running.
//!
//! The summarizer daemon polls this file and pauses while the TUI holds
//! the lock, so the dashboard does not contend with the background process.

use std::{fs as filesystem, path::PathBuf, process};

/// Resolve the pidfile path used to coordinate TUI/daemon hand-off.
fn lock_path() -> Option<PathBuf> {
    let mut path = dirs::cache_dir()?;
    path.push("roostr");
    let _ = filesystem::create_dir_all(&path);
    path.push("view.pid");
    Some(path)
}

/// RAII guard that writes the current process id to the lock file on
/// construction and removes it on drop.
pub struct ViewLock {
    /// Path to the pidfile owned by this guard.
    path: PathBuf,
}

impl ViewLock {
    /// Acquire the view lock by writing the current PID to the pidfile.
    /// Returns `None` if the pidfile cannot be created or written.
    #[must_use]
    pub fn acquire() -> Option<Self> {
        let path = lock_path()?;
        if filesystem::write(&path, process::id().to_string()).is_err() {
            return None;
        }
        Some(Self { path })
    }
}

impl Drop for ViewLock {
    fn drop(&mut self) {
        let _ = filesystem::remove_file(&self.path);
    }
}

/// Returns true if a live process currently holds the view lock.
#[must_use]
pub fn is_active() -> bool {
    let Some(path) = lock_path() else { return false };
    let Ok(contents) = filesystem::read_to_string(&path) else {
        return false;
    };
    let Ok(process_id) = contents.trim().parse::<i32>() else {
        return false;
    };
    pid_alive(process_id)
}

/// Probe whether `process_id` corresponds to a live process.
///
/// Returns `true` when `kill(pid, 0)` succeeds, and also when it fails with
/// `EPERM` (the process exists but we lack permission to signal it).
#[cfg(unix)]
fn pid_alive(process_id: i32) -> bool {
    let target = nix::unistd::Pid::from_raw(process_id);
    match nix::sys::signal::kill(target, None) {
        Ok(()) => true,
        Err(errno) => errno == nix::errno::Errno::EPERM,
    }
}

/// Non-unix fallback: assume no process is alive.
#[cfg(not(unix))]
fn pid_alive(_process_id: i32) -> bool {
    false
}
