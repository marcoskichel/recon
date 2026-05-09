#!/usr/bin/env bash
#
# demo.sh — Set up fake claude sessions for a roostr demo.
#
# Creates 6 agents across 4 rooms with all states (New, Working, Idle, Input).
# Includes a monorepo subdirectory room to showcase relative_dir grouping.
# No real claude/API calls — uses node processes + fake session files.
#
# Usage:
#   ./demo/demo.sh          # Set up + launch roostr interactively
#   ./demo/demo.sh --setup  # Set up only (for vhs recording in another terminal)
#
# Press Ctrl-C to clean up.
#
set -euo pipefail

RID=$(head -c 100 /dev/urandom | LC_ALL=C tr -dc 'a-z0-9' | head -c 4)
TMPDIR_BASE="/tmp/roostr-demo-${RID}"
SESSIONS_DIR="$HOME/.claude/sessions"
PROJECTS_DIR="$HOME/.claude/projects"

echo "Demo ID: $RID"

# Track fake files for cleanup
FAKE_SESSION_FILES=()
FAKE_PROJECT_DIRS=()

# --- Git repo setup ---
init_git_repo() {
    local dir="$1" branch="$2"
    mkdir -p "$dir"
    git -C "$dir" init -q
    git -C "$dir" checkout -q -b "$branch" 2>/dev/null || git -C "$dir" switch -q -c "$branch"
    touch "$dir/.gitkeep"
    git -C "$dir" add .
    GIT_COMMITTER_NAME="demo" GIT_COMMITTER_EMAIL="demo@demo" \
    GIT_AUTHOR_NAME="demo" GIT_AUTHOR_EMAIL="demo@demo" \
    git -C "$dir" commit -q -m "init" --allow-empty 2>/dev/null
}

# --- Cleanup ---
cleanup() {
    echo ""
    echo "Cleaning up..."
    # Kill demo tmux sessions
    tmux list-sessions -F '#{session_name}' 2>/dev/null \
        | grep "^d${RID}-" \
        | while read -r s; do tmux kill-session -t "$s" 2>/dev/null || true; done
    # Remove fake session files
    for f in "${FAKE_SESSION_FILES[@]}"; do
        rm -f "$f"
    done
    # Remove fake project dirs
    for d in "${FAKE_PROJECT_DIRS[@]}"; do
        rm -rf "$d"
    done
    rm -rf "$TMPDIR_BASE"
    echo "Done."
}
trap cleanup EXIT

# --- Encode path to project dir name (same as claude does) ---
encode_project_path() {
    echo "$1" | tr '/' '-'
}

# --- Create a fake agent ---
# Args: tmux_name cwd session_id status_line tokens_in tokens_out model timestamp
create_fake_agent() {
    local tmux_name="$1"
    local cwd="$2"
    local session_id="$3"
    local status_line="$4"     # last line for pane_status detection
    local tokens_in="$5"
    local tokens_out="$6"
    local model="$7"
    local timestamp="$8"
    local started_at="$9"

    # Create tmux session running node (so pane_current_command = "node")
    # The node script prints some fake output then the status line, then sleeps
    local node_script
    node_script=$(cat <<NODEOF
process.stdout.write("${status_line}");
setTimeout(() => {}, 999999999);
NODEOF
)
    tmux new-session -d -s "$tmux_name" -c "$cwd" "node -e '${node_script}'"
    sleep 0.3

    # Get the pane PID
    local pane_pid
    pane_pid=$(tmux list-panes -t "$tmux_name" -F '#{pane_pid}' 2>/dev/null | head -1)
    if [[ -z "$pane_pid" ]]; then
        echo "  WARNING: could not get PID for $tmux_name"
        return 1
    fi

    # Write ~/.claude/sessions/{PID}.json
    local session_file="$SESSIONS_DIR/${pane_pid}.json"
    cat > "$session_file" <<EOF
{"pid": ${pane_pid}, "sessionId": "${session_id}", "startedAt": ${started_at}}
EOF
    FAKE_SESSION_FILES+=("$session_file")

    # Write JSONL file in project dir (skip for "New" — 0 tokens means no JSONL needed,
    # but we still need it for agents with tokens)
    if (( tokens_in > 0 || tokens_out > 0 )); then
        local encoded_path
        encoded_path=$(encode_project_path "$cwd")
        local project_dir="$PROJECTS_DIR/$encoded_path"
        mkdir -p "$project_dir"
        FAKE_PROJECT_DIRS+=("$project_dir")

        local jsonl_file="$project_dir/${session_id}.jsonl"
        cat > "$jsonl_file" <<EOF
{"type":"assistant","message":{"model":"${model}","usage":{"input_tokens":${tokens_in},"output_tokens":${tokens_out},"cache_creation_input_tokens":0,"cache_read_input_tokens":0}},"timestamp":"${timestamp}","cwd":"${cwd}"}
EOF
    fi

    echo "  Created $tmux_name (PID=$pane_pid, status=${status_line:0:20}...)"
}

# ===== Define demo rooms and agents =====

# Room 1: api-server (main) — 2 agents at repo root
DIR_API="$TMPDIR_BASE/repos/api-server"
init_git_repo "$DIR_API" "main"

# Room 2: api-server › services/auth (main) — 1 agent in subdirectory (same repo)
DIR_AUTH="$DIR_API/services/auth"
mkdir -p "$DIR_AUTH"

# Room 3: frontend (feat/dashboard) — 2 agents
DIR_FE="$TMPDIR_BASE/repos/frontend"
init_git_repo "$DIR_FE" "feat/dashboard"

# Room 4: mobile-app (feat/onboarding) — 1 agent
DIR_MOBILE="$TMPDIR_BASE/repos/mobile-app"
init_git_repo "$DIR_MOBILE" "feat/onboarding"

echo ""
echo "=== Creating 6 fake agents across 4 rooms ==="

NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
NOW_EPOCH=$(date +%s)

# --- Room 1: api-server (2 agents) ---

# Agent 1: Working — actively streaming
create_fake_agent \
    "d${RID}-api1" \
    "$DIR_API" \
    "demo-api-working-${RID}" \
    "✻ Reading file\u2026" \
    45000 8000 \
    "claude-sonnet-4-6" \
    "$NOW" \
    "$((NOW_EPOCH - 300))"

# Agent 2: Idle — finished work
create_fake_agent \
    "d${RID}-api2" \
    "$DIR_API" \
    "demo-api-idle-${RID}" \
    "? for shortcuts" \
    120000 35000 \
    "claude-opus-4-6" \
    "$(date -u -v-15M +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -d '15 minutes ago' +%Y-%m-%dT%H:%M:%SZ)" \
    "$((NOW_EPOCH - 1200))"

# --- Room 2: api-server › services/auth (1 agent, subdirectory of same repo) ---

# Agent 3: Input — waiting for permission
create_fake_agent \
    "d${RID}-auth" \
    "$DIR_AUTH" \
    "demo-auth-input-${RID}" \
    "Esc to cancel" \
    30000 5000 \
    "claude-sonnet-4-6" \
    "$NOW" \
    "$((NOW_EPOCH - 180))"

# --- Room 3: frontend (2 agents) ---

# Agent 4: Working
create_fake_agent \
    "d${RID}-fe1" \
    "$DIR_FE" \
    "demo-fe-working-${RID}" \
    "✻ Reading file\u2026" \
    80000 20000 \
    "claude-sonnet-4-6" \
    "$NOW" \
    "$((NOW_EPOCH - 600))"

# Agent 5: Idle
create_fake_agent \
    "d${RID}-fe2" \
    "$DIR_FE" \
    "demo-fe-idle-${RID}" \
    "? for shortcuts" \
    150000 40000 \
    "claude-opus-4-6" \
    "$(date -u -v-45M +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -d '45 minutes ago' +%Y-%m-%dT%H:%M:%SZ)" \
    "$((NOW_EPOCH - 3600))"

# --- Room 4: mobile-app (1 agent) ---

# Agent 6: New — just started, no tokens
create_fake_agent \
    "d${RID}-mobile" \
    "$DIR_MOBILE" \
    "demo-mobile-new-${RID}" \
    "? for shortcuts" \
    0 0 \
    "" \
    "" \
    "$((NOW_EPOCH - 10))"

echo ""
echo "=== Demo ready ==="
echo ""
echo "  Room 1: api-server (main)              — 2 agents: Working, Idle"
echo "  Room 2: api-server › services/auth (main) — 1 agent:  Input"
echo "  Room 3: frontend (feat/dashboard)      — 2 agents: Working, Idle"
echo "  Room 4: mobile-app (feat/onboarding)   — 1 agent:  New"
echo ""

if [[ "${1:-}" == "--setup" ]]; then
    echo "Run roostr in another terminal. Press Ctrl-C to clean up."
    while true; do sleep 60; done
else
    echo "Launching roostr... (Ctrl-C to exit and clean up)"
    sleep 1
    roostr
fi
