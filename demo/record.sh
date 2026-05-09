#!/usr/bin/env bash
#
# record.sh — Build and record the roostr demo GIF inside Docker.
#
# Usage: ./demo/record.sh
#
# Outputs: assets/demo.gif (README hero image)
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"

echo "=== Building Docker image (compiles roostr inside container) ==="
docker build -t roostr-demo -f "$SCRIPT_DIR/Dockerfile" "$REPO_DIR"

echo "=== Recording demo ==="
mkdir -p "$REPO_DIR/assets"
docker run --rm --entrypoint bash -v "$REPO_DIR/assets:/output" roostr-demo -c '
    # Ensure claude dirs exist
    mkdir -p /root/.claude/sessions /root/.claude/projects

    # Set up fake sessions
    /demo/demo.sh --setup &
    DEMO_PID=$!
    sleep 3

    # Record
    cd /demo && vhs tapes/readme.tape

    # Copy output
    cp /demo/readme.gif /output/demo.gif 2>/dev/null || true

    # Cleanup
    kill $DEMO_PID 2>/dev/null || true
'

echo "=== Done ==="
echo "Output: $REPO_DIR/assets/demo.gif"
