# roostr — Claude Code Session Monitor

## Overview

A Rust TUI dashboard that monitors active Claude Code sessions, showing their status, resource usage, and providing quick navigation to any session's Warp terminal tab.

## Data Sources

### Primary: Claude Code Hooks

Claude Code fires lifecycle events (`SessionStart`, `Stop`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `Notification`) via its hooks system. We configure hooks in `~/.claude/settings.json` to write JSONL events to a shared file that roostr tails.

### Secondary: Session JSONL Files

Full conversation data lives in `~/.claude/projects/<encoded-path>/<session-id>.jsonl`. Each assistant message contains:
- `message.model` — e.g. `"claude-opus-4-6"`
- `message.usage.input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`

### Supplementary: Process Monitoring

`ps` / `sysinfo` to discover running `claude` processes, their TTYs, CPU state (`S+` = idle, `R+` = working), and command-line args (e.g. `--resume <session-id>`).

## Session States

| State | Signal |
|-------|--------|
| **Working** | Recent tool-use hook events, or process CPU > 0% |
| **Idle** | Process alive, sleeping, last event was `Stop` |
| **Awaiting Input** | Process alive, sleeping, last event was `Stop` or `Notification`, no `UserPromptSubmit` since |
| **Permission Request** | `PermissionRequest` hook fired, no subsequent `PostToolUse` |

## Dashboard View

### Session List (main view)

Table with one row per active session:

| Column | Example | Source |
|--------|---------|--------|
| **#** | `1` | Tab index in Warp |
| **Project** | `yaba` | Extracted from session's `cwd` / project path |
| **Status** | `WORKING` / `IDLE` / `INPUT` / `PERM` | Derived from hooks + process state |
| **Model** | `opus 4.6 (medium)` | `message.model` from session JSONL + reasoning effort if available |
| **Tokens** | `44k / 200k` | Cumulative usage from session JSONL / model context window |
| **Last Activity** | `2m ago` | Timestamp of last hook event |

### Visual Design

- Color coding: red = awaiting input / permission, green = working, dim/gray = idle
- Highlight the currently selected row
- Compact — fits in a small terminal pane

### Token Display

- **Used tokens**: Sum of `input_tokens + output_tokens` across all assistant messages in the session
- **Context window**: Mapped from model ID:
  - `claude-opus-4-6` → 200k
  - `claude-sonnet-4-6` → 200k
  - `claude-haiku-4-5-*` → 200k
- Format: `{used}k / {window}k` — e.g. `44k / 200k`
- Color: yellow when >75% used, red when >90%

### Model Display

- Map model IDs to human-readable names:
  - `claude-opus-4-6` → `Opus 4.6`
  - `claude-sonnet-4-6` → `Sonnet 4.6`
  - `claude-haiku-4-5-20251001` → `Haiku 4.5`
- Append reasoning effort if detectable: `(low)`, `(medium)`, `(high)`

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j` / `k` or arrows | Navigate session list |
| `Enter` or `1`-`9` | Jump to session's Warp tab |
| `r` | Force refresh |
| `q` | Quit |

## Warp Tab Control

Validated in POC (`src/main.rs`):
- **Direct jump**: `osascript` sends `Cmd+N` keystroke to Warp (tabs 1-9)
- **Sequential**: AXPress on "Switch to Next/Previous Tab" menu items via Accessibility API
- **Enumerate windows**: AXMenuItems in Window menu with `id="makeKeyAndOrderFront:"`

Warp's custom renderer does not expose individual tabs as AX elements. Tabs within a single window are not discoverable via AX — we track tab-to-session mapping ourselves via hooks.

## Architecture

```
┌─────────────────┐     JSONL events      ┌──────────────┐
│  Claude Code    │ ──── hooks ──────────► │ events file  │
│  (N sessions)   │                        └──────┬───────┘
└─────────────────┘                               │ tail
                                                  ▼
┌─────────────────┐     session JSONL      ┌──────────────┐
│ ~/.claude/      │ ◄──── read ──────────  │   roostr TUI  │
│ projects/       │                        │              │
└─────────────────┘     process info       │  ratatui +   │
┌─────────────────┐ ◄──── sysinfo ───────  │  crossterm + │
│ ps / sysinfo    │                        │  tokio       │
└─────────────────┘     tab control        │              │
┌─────────────────┐ ◄──── AX + osascript   └──────────────┘
│ Warp terminal   │
└─────────────────┘
```

## Tech Stack

| Crate | Purpose |
|-------|---------|
| `ratatui` + `crossterm` | TUI rendering |
| `tokio` | Async runtime |
| `linemux` | Tail JSONL event files |
| `sysinfo` | Process monitoring |
| `serde` + `serde_json` | Parse session data |
| `core-foundation` | macOS Accessibility API |

## Constraints

- Up to ~30 concurrent sessions — all state buffered in memory
- Warp tab direct jump limited to 9 tabs per window; use multiple windows for >9
- Hook commands must be fast (file append only)
- Session JSONL files can be large; read only the tail / recent entries for token sums
