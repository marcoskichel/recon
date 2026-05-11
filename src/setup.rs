//! Implementation of the `roostr setup` subcommands: install/uninstall the
//! tmux configuration snippet and the per-user summarizer daemon service.

use std::{
    fs as filesystem,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

use crate::cli::SetupAction;

/// Embedded tmux configuration shipped with the binary.
const TMUX_CONF: &str = include_str!("../tmux.conf");
/// Line appended to the user's `~/.tmux.conf` to source [`TMUX_CONF`].
const TMUX_SOURCE_LINE: &str = "source-file ~/.config/roostr/tmux.conf";

/// Dispatch a `roostr setup ...` action.
///
/// # Errors
///
/// Returns `Err` if the underlying filesystem operations fail (writing
/// configuration files, removing installed artifacts, etc.) or if no home
/// directory can be determined.
pub fn execute(action: &SetupAction) -> io::Result<()> {
    match *action {
        SetupAction::Tmux { force } => install_tmux(force),
        SetupAction::Daemon { force, interval } => install_daemon(force, interval),
        SetupAction::Everything { force, interval, with_daemon } => {
            if let Err(error) = install_tmux(force) {
                writeln!(io::stderr(), "warning: tmux setup failed: {error}")?;
            }
            if with_daemon {
                install_daemon(force, interval)
            } else {
                writeln!(
                    io::stdout(),
                    "daemon: skipped (opt-in). Run `roostr setup daemon` to install.",
                )?;
                Ok(())
            }
        }
        SetupAction::Uninstall => uninstall(),
    }
}

/// Look up the current user's home directory or return an `io::Error`.
///
/// # Errors
/// Returns `io::Error` (kind `NotFound`) when the home directory cannot be
/// determined.
fn home() -> io::Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home dir"))
}

/// Decide whether the cached tmux conf file needs to be (re)written.
///
/// Returns `Ok(true)` when the embedded [`TMUX_CONF`] differs from what is on
/// disk and we are allowed to overwrite, `Ok(false)` if the file is already
/// in sync or exists with custom content and `--force` was not passed.
///
/// # Errors
/// Returns the I/O error from the `writeln!` calls used to log status.
fn tmux_conf_needs_write(conf_path: &Path, force: bool) -> io::Result<bool> {
    match filesystem::read_to_string(conf_path) {
        Ok(existing) if existing == TMUX_CONF => {
            writeln!(io::stdout(), "tmux: {} already up to date", conf_path.display())?;
            Ok(false)
        }
        Ok(_) if !force => {
            writeln!(
                io::stdout(),
                "tmux: {} exists with different content — use --force to overwrite",
                conf_path.display(),
            )?;
            Ok(false)
        }
        Ok(_) | Err(_) => Ok(true),
    }
}

/// Append the `source-file` line to the user's `~/.tmux.conf` if not already
/// present.
///
/// # Errors
/// Returns the I/O error from filesystem reads/writes or status `writeln!`s.
fn ensure_tmux_source_line(user_tmux_conf: &Path) -> io::Result<()> {
    let existing = filesystem::read_to_string(user_tmux_conf).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == TMUX_SOURCE_LINE) {
        writeln!(io::stdout(), "tmux: {} already sources roostr config", user_tmux_conf.display())?;
        return Ok(());
    }
    let mut new_content = existing;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(TMUX_SOURCE_LINE);
    new_content.push('\n');
    filesystem::write(user_tmux_conf, new_content)?;
    writeln!(io::stdout(), "tmux: appended source-file line to {}", user_tmux_conf.display())?;
    Ok(())
}

/// Install the tmux configuration into `~/.config/roostr/tmux.conf` and
/// source it from `~/.tmux.conf`.
///
/// # Errors
/// Returns the underlying I/O error from filesystem operations or `writeln!`.
fn install_tmux(force: bool) -> io::Result<()> {
    let home_dir = home()?;
    let conf_dir = home_dir.join(".config").join("roostr");
    let conf_path = conf_dir.join("tmux.conf");
    let user_tmux_conf = home_dir.join(".tmux.conf");

    filesystem::create_dir_all(&conf_dir)?;

    if tmux_conf_needs_write(&conf_path, force)? {
        filesystem::write(&conf_path, TMUX_CONF)?;
        writeln!(io::stdout(), "tmux: wrote {}", conf_path.display())?;
    }

    ensure_tmux_source_line(&user_tmux_conf)?;

    writeln!(io::stdout(), "tmux: Run: tmux source-file ~/.tmux.conf  (or restart tmux)")?;
    Ok(())
}

/// Install the summarizer daemon service for the current OS.
///
/// # Errors
/// Returns the I/O error from the OS-specific installer or `Unsupported`
/// when the host platform is neither Linux nor macOS.
fn install_daemon(force: bool, interval: u64) -> io::Result<()> {
    if cfg!(target_os = "macos") {
        install_daemon_macos(force, interval)
    } else if cfg!(target_os = "linux") {
        install_daemon_linux(force, interval)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported OS for `roostr setup daemon` (only Linux and macOS)",
        ))
    }
}

/// Resolve the absolute path to the running `roostr` binary, falling back to
/// the bare name `"roostr"` when the executable cannot be located.
fn binary_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .map_or_else(|| "roostr".to_string(), |path| path.to_string_lossy().to_string())
}

/// Build the systemd unit file body for the summarizer daemon.
fn linux_unit_body(binary: &str, interval: u64) -> String {
    format!(
        "[Unit]\n\
         Description=roostr summarizer daemon\n\
         After=default.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={binary} daemon --interval {interval}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         # Pick one backend. Either point at a running Ollama instance:\n\
         Environment=ROOSTR_OLLAMA_URL=http://localhost:11434\n\
         Environment=ROOSTR_OLLAMA_MODEL=gemma2:2b\n\
         # …or use the Anthropic API (uncomment and set your key):\n\
         # Environment=ANTHROPIC_API_KEY=sk-ant-...\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
    )
}

/// Install the summarizer daemon as a systemd user service.
///
/// # Errors
/// Returns the I/O error from filesystem writes or `systemctl` invocations.
fn install_daemon_linux(force: bool, interval: u64) -> io::Result<()> {
    let home_dir = home()?;
    let unit_dir = home_dir.join(".config").join("systemd").join("user");
    let unit_path = unit_dir.join("roostr-daemon.service");

    if unit_path.exists() && !force {
        writeln!(
            io::stderr(),
            "daemon: {} already exists — use --force to overwrite",
            unit_path.display(),
        )?;
        return Ok(());
    }

    filesystem::create_dir_all(&unit_dir)?;

    let binary = binary_path();
    filesystem::write(&unit_path, linux_unit_body(&binary, interval))?;
    writeln!(io::stdout(), "daemon: wrote {}", unit_path.display())?;

    run_logged("systemctl", &["--user", "daemon-reload"], false)?;
    run_logged("systemctl", &["--user", "enable", "--now", "roostr-daemon.service"], false)?;

    writeln!(
        io::stdout(),
        "daemon: check status with: systemctl --user status roostr-daemon.service",
    )?;
    writeln!(
        io::stdout(),
        "daemon: follow logs with:  journalctl --user -u roostr-daemon.service -f",
    )?;
    writeln!(
        io::stdout(),
        "daemon: tip: run `loginctl enable-linger \"$USER\"` to keep it running when logged out.",
    )?;
    Ok(())
}

/// Build the launchd plist body for the summarizer daemon.
fn macos_plist_body(binary: &str, interval: u64) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
             <key>Label</key>\n\
             <string>com.roostr.daemon</string>\n\
         \n\
             <key>ProgramArguments</key>\n\
             <array>\n\
                 <string>{binary}</string>\n\
                 <string>daemon</string>\n\
                 <string>--interval</string>\n\
                 <string>{interval}</string>\n\
             </array>\n\
         \n\
             <key>EnvironmentVariables</key>\n\
             <dict>\n\
                 <key>PATH</key>\n\
                 <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin</string>\n\
                 <key>ROOSTR_OLLAMA_URL</key>\n\
                 <string>http://localhost:11434</string>\n\
                 <key>ROOSTR_OLLAMA_MODEL</key>\n\
                 <string>gemma2:2b</string>\n\
             </dict>\n\
         \n\
             <key>RunAtLoad</key>\n\
             <true/>\n\
             <key>KeepAlive</key>\n\
             <true/>\n\
         \n\
             <key>StandardOutPath</key>\n\
             <string>/tmp/roostr-daemon.out.log</string>\n\
             <key>StandardErrorPath</key>\n\
             <string>/tmp/roostr-daemon.err.log</string>\n\
         </dict>\n\
         </plist>\n",
    )
}

/// Install the summarizer daemon as a launchd user agent.
///
/// # Errors
/// Returns the I/O error from filesystem writes or `launchctl` invocations.
fn install_daemon_macos(force: bool, interval: u64) -> io::Result<()> {
    let home_dir = home()?;
    let plist_dir = home_dir.join("Library").join("LaunchAgents");
    let plist_path = plist_dir.join("com.roostr.daemon.plist");

    if plist_path.exists() && !force {
        writeln!(
            io::stderr(),
            "daemon: {} already exists — use --force to overwrite",
            plist_path.display(),
        )?;
        return Ok(());
    }

    filesystem::create_dir_all(&plist_dir)?;

    let binary = binary_path();
    filesystem::write(&plist_path, macos_plist_body(&binary, interval))?;
    writeln!(io::stdout(), "daemon: wrote {}", plist_path.display())?;

    let plist_str = plist_path.to_string_lossy().to_string();
    run_logged("launchctl", &["load", "-w", &plist_str], true)?;

    writeln!(io::stdout(), "daemon: check status with: launchctl list | grep com.roostr.daemon")?;
    writeln!(io::stdout(), "daemon: follow logs at:    /tmp/roostr-daemon.err.log")?;
    Ok(())
}

/// Strip the roostr `source-file` line from the user's `~/.tmux.conf` if
/// present.
///
/// # Errors
/// Returns the I/O error from filesystem writes or `writeln!`.
fn uninstall_tmux_source_line(user_tmux_conf: &Path) -> io::Result<()> {
    let Ok(existing) = filesystem::read_to_string(user_tmux_conf) else {
        return Ok(());
    };
    let kept: Vec<&str> = existing.lines().filter(|line| line.trim() != TMUX_SOURCE_LINE).collect();
    let new_content = if kept.is_empty() {
        String::new()
    } else {
        let mut joined = kept.join("\n");
        joined.push('\n');
        joined
    };
    if new_content != existing {
        filesystem::write(user_tmux_conf, new_content)?;
        writeln!(io::stdout(), "removed source-file line from {}", user_tmux_conf.display())?;
    }
    Ok(())
}

/// Disable and remove the systemd user unit installed by `setup daemon`.
///
/// # Errors
/// Returns the I/O error from `systemctl` invocations or `remove_file`.
fn uninstall_daemon_linux(home_dir: &Path) -> io::Result<()> {
    let unit_path =
        home_dir.join(".config").join("systemd").join("user").join("roostr-daemon.service");
    run_logged("systemctl", &["--user", "disable", "--now", "roostr-daemon.service"], true)?;
    if unit_path.exists() {
        filesystem::remove_file(&unit_path)?;
        writeln!(io::stdout(), "removed {}", unit_path.display())?;
    }
    run_logged("systemctl", &["--user", "daemon-reload"], true)?;
    Ok(())
}

/// Unload and remove the launchd plist installed by `setup daemon`.
///
/// # Errors
/// Returns the I/O error from `launchctl` invocations or `remove_file`.
fn uninstall_daemon_macos(home_dir: &Path) -> io::Result<()> {
    let plist_path = home_dir.join("Library").join("LaunchAgents").join("com.roostr.daemon.plist");
    let plist_str = plist_path.to_string_lossy().to_string();
    run_logged("launchctl", &["unload", "-w", &plist_str], true)?;
    if plist_path.exists() {
        filesystem::remove_file(&plist_path)?;
        writeln!(io::stdout(), "removed {}", plist_path.display())?;
    }
    Ok(())
}

/// Reverse of [`install_tmux`] / [`install_daemon`].
///
/// # Errors
/// Returns the underlying I/O error from filesystem operations or service
/// commands.
fn uninstall() -> io::Result<()> {
    let home_dir = home()?;

    let user_tmux_conf = home_dir.join(".tmux.conf");
    uninstall_tmux_source_line(&user_tmux_conf)?;

    let conf_path = home_dir.join(".config").join("roostr").join("tmux.conf");
    if conf_path.exists() {
        filesystem::remove_file(&conf_path)?;
        writeln!(io::stdout(), "removed {}", conf_path.display())?;
    }

    if cfg!(target_os = "linux") {
        uninstall_daemon_linux(&home_dir)?;
    } else if cfg!(target_os = "macos") {
        uninstall_daemon_macos(&home_dir)?;
    } else {
        // Other platforms have no daemon installation, so nothing to undo.
    }

    Ok(())
}

/// Spawn a side-effect command, log success/failure to stdout/stderr and
/// optionally swallow non-zero exits.
///
/// # Errors
/// Returns the I/O error from the diagnostic `writeln!` calls.
fn run_logged(program: &str, args: &[&str], ignore_errors: bool) -> io::Result<()> {
    let pretty = format!("{} {}", program, args.join(" "));
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => {
            writeln!(io::stdout(), "ran: {pretty}")?;
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if ignore_errors {
                writeln!(io::stdout(), "ran: {pretty} (non-zero, ignored)")?;
            } else {
                writeln!(io::stderr(), "warning: `{pretty}` exited non-zero: {}", stderr.trim())?;
            }
        }
        Err(error) => {
            if ignore_errors {
                writeln!(io::stdout(), "skipped: {pretty} ({error})")?;
            } else {
                writeln!(io::stderr(), "warning: failed to run `{pretty}`: {error}")?;
            }
        }
    }
    Ok(())
}
