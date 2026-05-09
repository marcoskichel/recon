---
name: roostr
description: Manage Claude Code tmux sessions via the roostr CLI
---

You have access to `roostr`, a CLI tool for monitoring and managing Claude Code sessions running in tmux.

## Step 1: Discover capabilities

```bash
roostr --help
```

## Step 2: Execute the task

Use the commands discovered in Step 1. Common examples:

- `roostr` — open the table dashboard
- `roostr view` — open the visual dashboard
- `roostr new` — interactive form to create a new session
- `roostr next` — jump to the next agent waiting for input
- `roostr resume` — interactive picker to resume a past session
- `roostr json` — get all session state as JSON (useful for scripting)
