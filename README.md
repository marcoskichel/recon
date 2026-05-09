# roostr

A tmux-native dashboard for managing [Claude Code](https://claude.ai/claude-code) agents.

Run multiple Claude Code sessions in tmux, then manage them all without ever leaving the terminal — see what each agent is working on, which ones need your attention, switch between them, kill or spawn new ones, and resume past sessions. All from a single keybinding.

![roostr demo](assets/demo.gif)

## Views

### Tamagotchi View (`roostr view` or press `v`)

A visual dashboard where each agent is a pixel-art creature living in a room. Designed for a side monitor — glance over and instantly see who's working, sleeping, or needs attention.

Creatures are rendered as colored pixel art using half-block characters. Working and Input creatures animate; Idle and New stay still.

| State | Creature | Color |
|-------|----------|-------|
| **Working** | Happy blob with sparkles and feet | Green |
| **Input** | Angry blob with furrowed brows | Orange (pulsing) |
| **Idle** | Sleeping blob with Zzz | Blue-grey |
| **New** | Egg with spots | Cream |

- **Rooms** group agents by git repository — worktrees of the same repo share a room, while monorepo sub-projects get their own (e.g. `myapp` vs `myapp › tools/cli`) (2×2 grid, paginated)
- **Zoom** into a room with `1`-`4`, page with `j`/`k`
- **Context bar** per agent with green/yellow/red coloring

### Table View (default)

```
┌─ roostr — Claude Code Sessions ─────────────────────────────────────────────────────────────────────────┐
│  #  Session          Git(Project::Branch)   Directory          Status  Model       Context  Last Active │
│  1  api-refactor     myapp::feat/auth       ~/repos/myapp      ● Input Opus 4.6    45k/1M   2m ago      │
│  2  debug-pipeline   infra::main            ~/repos/infra      ● Work  Sonnet 4.6  12k/200k < 1m        │
│  3  write-tests      myapp::feat/auth       ~/repos/myapp      ● Work  Haiku 4.5   8k/200k  < 1m        │
│  4  code-review      webapp::pr-452         ~/repos/webapp     ● Idle  Sonnet 4.6  90k/200k 5m ago      │
│  5  scratch          roostr::main           ~/repos/roostr     ● Idle  Opus 4.6    3k/1M    10m ago     │
│  6  new-session      dotfiles::main         ~/repos/dotfiles   ● New   —           —        —           │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────┘
j/k navigate  Enter switch  / search  v view  q quit
```

- **Input** rows are highlighted — these sessions are blocked waiting for your approval
- **Working** sessions are actively streaming or running tools
- **Idle** sessions are done and waiting for your next prompt
- **New** sessions haven't had any interaction yet

## How it works

roostr is built around **tmux**. Each Claude Code instance runs in its own tmux session.

```
┌─────────────────────────────────────────────────────────┐
│                      tmux server                        │
│                                                         │
│  ┌───────────────┐  ┌───────────────┐  ┌──────────────┐ │
│  │ session:      │  │ session:      │  │ session:     │ │
│  │ api-refactor  │  │ debug-pipe    │  │ scratch      │ │
│  │               │  │               │  │              │ │
│  │  ┌──────────┐ │  │  ┌──────────┐ │  │  ┌────────┐  │ │
│  │  │  claude  │ │  │  │  claude  │ │  │  │ claude │  │ │
│  │  └──────────┘ │  │  └──────────┘ │  │  └────────┘  │ │
│  └───────┬───────┘  └───────┬───────┘  └───────┬──────┘ │
│          │                  │                  │        │
└──────────┼──────────────────┼──────────────────┼────────┘
           │                  │                  │
           ▼                  ▼                  ▼
     ┌──────────────────────────────────────────────┐
     │                 roostr (TUI)                  │
     │                                               │
     │  reads:                                       │
     │   • tmux list-panes → PID, session name       │
     │   • ~/.claude/sessions/{PID}.json             │
     │   • ~/.claude/projects/…/*.jsonl              │
     │   • tmux capture-pane → status bar text       │
     └──────────────────────────────────────────────┘
```

**Status detection** inspects the Claude Code TUI status bar at the bottom of each tmux pane:

| Status bar text | State |
|---|---|
| `esc to interrupt` | **Working** — streaming response or running a tool |
| `Esc to cancel` | **Input** — permission prompt, waiting for you |
| anything else | **Idle** — waiting for your next prompt |
| *(0 tokens)* | **New** — no interaction yet |

**Session matching** uses `~/.claude/sessions/{PID}.json` files that Claude Code writes, linking each process to its session ID. No `ps` parsing or CWD-based heuristics.

## Install

```bash
cargo install --path .
```

Requires tmux and [Claude Code](https://claude.ai/claude-code).

## Usage

```bash
roostr                                        # Table dashboard
roostr view                                   # Tamagotchi visual dashboard
roostr json                                   # JSON output (for scripting)
roostr launch                                 # Create a new claude session (background)
roostr launch --name foo --cwd ~/repos/myapp  # Custom name and directory
roostr launch --command "claude --model sonnet" --attach  # Custom command, attach to session
roostr launch --tag env:staging --tag role:reviewer       # Tag a session (key:value metadata)
roostr json --tag role:reviewer               # Filter JSON output by tag (must match all)
roostr new                                    # Interactive new session form
roostr resume                                 # Interactive resume picker
roostr resume --id <session-id>               # Resume a specific session
roostr resume --id <session-id> --name foo    # Resume with a custom tmux session name
roostr next                                   # Jump to the next agent waiting for input
roostr park                                   # Save all live sessions to disk
roostr unpark                                 # Restore previously parked sessions
roostr daemon                                 # Run the summarizer in the background
roostr daemon --interval 30                   # Custom poll interval (seconds)
```

## Daemon

`roostr daemon` runs the summarizer continuously in the background. It polls active Claude Code sessions, sends new transcripts to a summarizer backend (local Ollama or the Anthropic API), and writes generated labels to `~/.cache/roostr/labels`. Those labels then show up in both the table and Tamagotchi views.

The daemon needs at least one backend configured via environment variables:

| Variable | Default | Description |
|---|---|---|
| `ROOSTR_SUMMARIZER` | auto | `ollama`, `anthropic`, `claude`, or unset (auto-detect) |
| `ROOSTR_OLLAMA_URL` | `http://localhost:11434` | Ollama endpoint |
| `ROOSTR_OLLAMA_MODEL` | `gemma2:2b` | Ollama model |
| `ANTHROPIC_API_KEY` | — | Required for the Anthropic backend |
| `ROOSTR_ANTHROPIC_MODEL` | — | Override the Anthropic model |
| `ROOSTR_CLAUDE_BINARY` | `claude` | Claude CLI path (claude backend) |
| `ROOSTR_CLAUDE_MODEL` | — | Claude CLI model override |

If neither Ollama nor `ANTHROPIC_API_KEY` is reachable, the daemon exits with an error.

### Run on system startup — Linux (systemd user service)

Create `~/.config/systemd/user/roostr-daemon.service`:

```ini
[Unit]
Description=roostr summarizer daemon
After=default.target

[Service]
Type=simple
ExecStart=%h/.cargo/bin/roostr daemon --interval 10
Restart=on-failure
RestartSec=5
# Pick one backend. Either point at a running Ollama instance:
Environment=ROOSTR_OLLAMA_URL=http://localhost:11434
Environment=ROOSTR_OLLAMA_MODEL=gemma2:2b
# …or use the Anthropic API (uncomment and set your key):
# Environment=ANTHROPIC_API_KEY=sk-ant-...

[Install]
WantedBy=default.target
```

Enable and start it:

```bash
systemctl --user daemon-reload
systemctl --user enable --now roostr-daemon.service
systemctl --user status roostr-daemon.service
journalctl --user -u roostr-daemon.service -f      # follow logs
```

To make the service start at boot (without you logging in), enable lingering once:

```bash
sudo loginctl enable-linger "$USER"
```

The daemon talks to your tmux server, so it must run as your user — not as a root system service.

### Run on system startup — macOS (launchd LaunchAgent)

Create `~/Library/LaunchAgents/com.roostr.daemon.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.roostr.daemon</string>

    <key>ProgramArguments</key>
    <array>
        <string>/Users/YOUR_USERNAME/.cargo/bin/roostr</string>
        <string>daemon</string>
        <string>--interval</string>
        <string>10</string>
    </array>

    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin</string>
        <!-- Pick one backend: -->
        <key>ROOSTR_OLLAMA_URL</key>
        <string>http://localhost:11434</string>
        <key>ROOSTR_OLLAMA_MODEL</key>
        <string>gemma2:2b</string>
        <!-- Or set ANTHROPIC_API_KEY here instead. -->
    </dict>

    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>

    <key>StandardOutPath</key>
    <string>/tmp/roostr-daemon.out.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/roostr-daemon.err.log</string>
</dict>
</plist>
```

Replace `YOUR_USERNAME` with your macOS short name. Then load it:

```bash
launchctl load -w ~/Library/LaunchAgents/com.roostr.daemon.plist
launchctl list | grep com.roostr.daemon                # check it's running
tail -f /tmp/roostr-daemon.err.log                     # follow logs
```

To stop or remove it:

```bash
launchctl unload -w ~/Library/LaunchAgents/com.roostr.daemon.plist
```

LaunchAgents run as your user when you log in, which is required so the daemon can reach your tmux server.

### Keybindings — Table View

| Key | Action |
|---|---|
| `j` / `k` | Navigate sessions |
| `Enter` | Switch to selected tmux session |
| `/` | Search / filter sessions by name |
| `i` / `Tab` | Jump to next agent waiting for input |
| `x` | Kill selected session |
| `v` | Switch to Tamagotchi view |
| `q` / `Esc` | Quit (Esc clears filter first) |

### Keybindings — Tamagotchi View

| Key | Action |
|---|---|
| `1`-`4` | Zoom into room |
| `/` | Search / filter sessions by name |
| `j` / `k` | Previous / next page |
| `h` / `l` | Select agent (when zoomed) |
| `Enter` | Switch to selected agent (when zoomed) |
| `x` | Kill selected agent (when zoomed) |
| `n` | New session in room (when zoomed) |
| `Esc` | Zoom out (or quit) |
| `v` | Switch to table view |
| `q` | Quit |

## tmux config

The included `tmux.conf` provides keybindings to open roostr as a popup overlay:

```bash
# Add to your ~/.tmux.conf — capital letters chosen so default tmux
# bindings (e.g. prefix + n = next-window) stay intact.
bind G display-popup -E -w 80% -h 60% "roostr"       # prefix + G → dashboard
bind N display-popup -E -w 80% -h 60% "roostr new"    # prefix + N → new session
bind i run-shell "roostr next"                         # prefix + i → jump to next input agent
bind e run-shell "roostr dock-focus"                  # prefix + e → focus dock sidebar (spawn if missing)
bind E run-shell "roostr dock-toggle"                 # prefix + E → toggle dock sidebar (open/close)
bind X confirm-before -p "Kill session #S? (y/n)" kill-session
```

This lets you pop open the dashboard from any tmux session, pick a session with `Enter`, and jump straight to it. `prefix + e` spawns/kills a 14-col dock pane on the right of the current window — the dock shows a mini sprite + token bar per session and supports the same keys as the main view (`hjkl`, `Enter`, `x`, `n`, `1`-`9`, `q`).

## Known Limitations

- **`/clear` resets session tracking** — Claude Code's `/clear` command creates a new JSONL file without updating the session-to-process mapping. After `/clear`, roostr may show stale data (old tokens, old timestamps) until the session is restarted. Workaround: kill the session in roostr and create a new one.
- **macOS TCC prompts** — roostr runs `git -C <session-cwd>` to derive project name and branch. If a session's CWD is under a TCC-protected directory (`~/Pictures`, `~/Desktop`, `~/Documents`, `~/Downloads`, `~/Music`, `~/Movies`), roostr skips git enrichment to avoid permission prompts. To re-enable git for specific paths under those dirs, set `ROOSTR_TCC_ALLOW` to a comma-separated list of absolute prefixes:

  ```bash
  export ROOSTR_TCC_ALLOW=/Users/me/Documents/code,/Users/me/Desktop/work
  ```

## Contribution Policy

Issues and pull requests welcome. Please open an [Issue](https://github.com/marcoskichel/roostr/issues) for bug reports or feature requests.

## Origin

`roostr` is a fork of [gavraz/recon](https://github.com/gavraz/recon), forked at v0.6.1 and renamed to mark divergence in performance, UI, and feature set. Thanks to [@gavraz](https://github.com/gavraz) for the original work. See [`NOTICE`](NOTICE) for details.

## License

MIT — see [LICENSE](LICENSE).
