use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

use crate::cli;

const TMUX_CONF: &str = include_str!("../tmux.conf");
const TMUX_SOURCE_LINE: &str = "source-file ~/.config/roostr/tmux.conf";

pub fn run(action: cli::SetupAction) -> io::Result<()> {
    match action {
        cli::SetupAction::Tmux { force } => install_tmux(force),
        cli::SetupAction::Daemon { force, interval } => install_daemon(force, interval),
        cli::SetupAction::All { force, interval, with_daemon } => {
            if let Err(e) = install_tmux(force) {
                eprintln!("warning: tmux setup failed: {e}");
            }
            if with_daemon {
                install_daemon(force, interval)
            } else {
                println!("daemon: skipped (opt-in). Run `roostr setup daemon` to install.");
                Ok(())
            }
        }
        cli::SetupAction::Uninstall => uninstall(),
    }
}

fn home() -> io::Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home dir"))
}

fn install_tmux(force: bool) -> io::Result<()> {
    let home = home()?;
    let conf_dir = home.join(".config").join("roostr");
    let conf_path = conf_dir.join("tmux.conf");
    let user_tmux_conf = home.join(".tmux.conf");

    fs::create_dir_all(&conf_dir)?;

    let needs_write = match fs::read_to_string(&conf_path) {
        Ok(existing) if existing == TMUX_CONF => {
            println!("tmux: {} already up to date", conf_path.display());
            false
        }
        Ok(_) if !force => {
            println!(
                "tmux: {} exists with different content — use --force to overwrite",
                conf_path.display()
            );
            false
        }
        Ok(_) => true,
        Err(_) => true,
    };

    if needs_write {
        fs::write(&conf_path, TMUX_CONF)?;
        println!("tmux: wrote {}", conf_path.display());
    }

    let existing = fs::read_to_string(&user_tmux_conf).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == TMUX_SOURCE_LINE) {
        println!("tmux: {} already sources roostr config", user_tmux_conf.display());
    } else {
        let mut new_content = existing.clone();
        if !new_content.is_empty() && !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(TMUX_SOURCE_LINE);
        new_content.push('\n');
        fs::write(&user_tmux_conf, new_content)?;
        println!("tmux: appended source-file line to {}", user_tmux_conf.display());
    }

    println!("tmux: Run: tmux source-file ~/.tmux.conf  (or restart tmux)");
    Ok(())
}

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

fn binary_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "roostr".to_string())
}

fn install_daemon_linux(force: bool, interval: u64) -> io::Result<()> {
    let home = home()?;
    let unit_dir = home.join(".config").join("systemd").join("user");
    let unit_path = unit_dir.join("roostr-daemon.service");

    if unit_path.exists() && !force {
        eprintln!(
            "daemon: {} already exists — use --force to overwrite",
            unit_path.display()
        );
        return Ok(());
    }

    fs::create_dir_all(&unit_dir)?;

    let bin = binary_path();
    let unit = format!(
        "[Unit]\n\
         Description=roostr summarizer daemon\n\
         After=default.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={bin} daemon --interval {interval}\n\
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
    );

    fs::write(&unit_path, unit)?;
    println!("daemon: wrote {}", unit_path.display());

    run_logged("systemctl", &["--user", "daemon-reload"], false);
    run_logged(
        "systemctl",
        &["--user", "enable", "--now", "roostr-daemon.service"],
        false,
    );

    println!("daemon: check status with: systemctl --user status roostr-daemon.service");
    println!("daemon: follow logs with:  journalctl --user -u roostr-daemon.service -f");
    println!("daemon: tip: run `loginctl enable-linger \"$USER\"` to keep it running when logged out.");
    Ok(())
}

fn install_daemon_macos(force: bool, interval: u64) -> io::Result<()> {
    let home = home()?;
    let plist_dir = home.join("Library").join("LaunchAgents");
    let plist_path = plist_dir.join("com.roostr.daemon.plist");

    if plist_path.exists() && !force {
        eprintln!(
            "daemon: {} already exists — use --force to overwrite",
            plist_path.display()
        );
        return Ok(());
    }

    fs::create_dir_all(&plist_dir)?;

    let bin = binary_path();
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
             <key>Label</key>\n\
             <string>com.roostr.daemon</string>\n\
         \n\
             <key>ProgramArguments</key>\n\
             <array>\n\
                 <string>{bin}</string>\n\
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
    );

    fs::write(&plist_path, plist)?;
    println!("daemon: wrote {}", plist_path.display());

    let plist_str = plist_path.to_string_lossy().to_string();
    run_logged("launchctl", &["load", "-w", &plist_str], true);

    println!("daemon: check status with: launchctl list | grep com.roostr.daemon");
    println!("daemon: follow logs at:    /tmp/roostr-daemon.err.log");
    Ok(())
}

fn uninstall() -> io::Result<()> {
    let home = home()?;

    let user_tmux_conf = home.join(".tmux.conf");
    if let Ok(existing) = fs::read_to_string(&user_tmux_conf) {
        let kept: Vec<&str> = existing
            .lines()
            .filter(|l| l.trim() != TMUX_SOURCE_LINE)
            .collect();
        let new_content = if kept.is_empty() {
            String::new()
        } else {
            let mut s = kept.join("\n");
            s.push('\n');
            s
        };
        if new_content != existing {
            fs::write(&user_tmux_conf, new_content)?;
            println!("removed source-file line from {}", user_tmux_conf.display());
        }
    }

    let conf_path = home.join(".config").join("roostr").join("tmux.conf");
    if conf_path.exists() {
        fs::remove_file(&conf_path)?;
        println!("removed {}", conf_path.display());
    }

    if cfg!(target_os = "linux") {
        let unit_path = home
            .join(".config")
            .join("systemd")
            .join("user")
            .join("roostr-daemon.service");
        run_logged(
            "systemctl",
            &["--user", "disable", "--now", "roostr-daemon.service"],
            true,
        );
        if unit_path.exists() {
            fs::remove_file(&unit_path)?;
            println!("removed {}", unit_path.display());
        }
        run_logged("systemctl", &["--user", "daemon-reload"], true);
    } else if cfg!(target_os = "macos") {
        let plist_path = home
            .join("Library")
            .join("LaunchAgents")
            .join("com.roostr.daemon.plist");
        let plist_str = plist_path.to_string_lossy().to_string();
        run_logged("launchctl", &["unload", "-w", &plist_str], true);
        if plist_path.exists() {
            fs::remove_file(&plist_path)?;
            println!("removed {}", plist_path.display());
        }
    }

    Ok(())
}

fn run_logged(program: &str, args: &[&str], ignore_errors: bool) {
    let pretty = format!("{} {}", program, args.join(" "));
    match Command::new(program).args(args).output() {
        Ok(out) if out.status.success() => {
            println!("ran: {pretty}");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if ignore_errors {
                println!("ran: {pretty} (non-zero, ignored)");
            } else {
                eprintln!("warning: `{pretty}` exited non-zero: {}", stderr.trim());
            }
        }
        Err(e) => {
            if ignore_errors {
                println!("skipped: {pretty} ({e})");
            } else {
                eprintln!("warning: failed to run `{pretty}`: {e}");
            }
        }
    }
}

