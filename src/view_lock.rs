use std::fs;
use std::path::PathBuf;
use std::process;

fn lock_path() -> Option<PathBuf> {
    let mut p = dirs::cache_dir()?;
    p.push("roostr");
    let _ = fs::create_dir_all(&p);
    p.push("view.pid");
    Some(p)
}

pub struct ViewLock {
    path: PathBuf,
}

impl ViewLock {
    pub fn acquire() -> Option<Self> {
        let path = lock_path()?;
        if fs::write(&path, process::id().to_string()).is_err() {
            return None;
        }
        Some(Self { path })
    }
}

impl Drop for ViewLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn is_active() -> bool {
    let path = match lock_path() {
        Some(p) => p,
        None => return false,
    };
    let contents = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let pid: i32 = match contents.trim().parse() {
        Ok(p) => p,
        Err(_) => return false,
    };
    pid_alive(pid)
}

#[cfg(unix)]
fn pid_alive(pid: i32) -> bool {
    unsafe {
        if libc::kill(pid, 0) == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

#[cfg(not(unix))]
fn pid_alive(_pid: i32) -> bool {
    false
}
