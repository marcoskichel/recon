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
cargo install --path .          # build + install binary
roostr setup all                # install tmux keybindings (daemon is opt-in)
roostr setup daemon             # opt-in: install background summarizer service
```

Requires tmux and [Claude Code](https://claude.ai/claude-code).

`roostr setup` is a one-command installer for the tmux config and the daemon service. The daemon is **opt-in** — `setup all` installs the tmux config only; install the daemon explicitly if you want background summaries. Sub-commands:

- `roostr setup tmux` — writes the bundled tmux keybindings to `~/.config/roostr/tmux.conf` and appends a `source-file` line to `~/.tmux.conf` (idempotent; `--force` overwrites a divergent config file).
- `roostr setup daemon` — writes a user-level service unit (`~/.config/systemd/user/roostr-daemon.service` on Linux, `~/Library/LaunchAgents/com.roostr.daemon.plist` on macOS) using the running binary's path, then enables/loads it. `--interval <secs>` sets the poll interval, `--force` overwrites an existing unit.
- `roostr setup all` — installs the tmux config. Add `--with-daemon` to also install the daemon in one shot.
- `roostr setup uninstall` — reverses everything: removes the `source-file` line, deletes the bundled tmux config, disables and deletes the service unit. Idempotent — safe to run if pieces are already gone.

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
roostr setup all                              # Install tmux keybindings (daemon opt-in)
roostr setup all --with-daemon                # Install tmux keybindings + daemon
roostr setup tmux                             # Just install tmux keybindings
roostr setup daemon                           # Install the daemon as a user service
roostr setup uninstall                        # Reverse install
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

### Run on system startup

Use `roostr setup daemon` to install a user-level service:

- **Linux** — writes `~/.config/systemd/user/roostr-daemon.service` and runs `systemctl --user enable --now`. Check with `systemctl --user status roostr-daemon.service`; follow logs with `journalctl --user -u roostr-daemon.service -f`. To keep the service running after logout, enable lingering once: `sudo loginctl enable-linger "$USER"`.
- **macOS** — writes `~/Library/LaunchAgents/com.roostr.daemon.plist` and runs `launchctl load -w`. Check with `launchctl list | grep com.roostr.daemon`; logs go to `/tmp/roostr-daemon.{out,err}.log`.

The daemon talks to your tmux server, so it must run as your user — not as a root system service. Edit the unit file directly if you want to customize environment variables or interval, then `systemctl --user daemon-reload && systemctl --user restart roostr-daemon.service` (Linux) or `launchctl unload && launchctl load -w …` (macOS). To remove the service, run `roostr setup uninstall`.

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

`roostr setup tmux` installs the bundled keybindings — it writes the config to `~/.config/roostr/tmux.conf` and appends a `source-file` line to your `~/.tmux.conf`. Re-run with `--force` to overwrite a divergent config.

Capital letters are chosen so default tmux bindings (e.g. `prefix + n` = next-window) stay intact.

| Keybind | Action |
|---|---|
| `prefix + G` | Toggle the dashboard tmux window (focus / open / close) |
| `prefix + N` | Open the new-session form as a popup |
| `prefix + i` | Jump to the next agent waiting for input |
| `prefix + e` | Focus the dock sidebar (spawn if missing) |
| `prefix + E` | Toggle the dock sidebar open/close |
| `prefix + X` | Kill the current tmux session (with confirm) |

The config also installs `after-new-window` and `session-created` hooks that auto-spawn the dock pane on the right of every new window. `prefix + e` / `E` lets you focus or close it on demand — the dock shows a mini sprite + token bar per session and supports the same keys as the main view (`hjkl`, `Enter`, `x`, `n`, `1`-`9`, `q`).

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
