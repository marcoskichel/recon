#!/usr/bin/env bash
# Pre-commit hook for roostr — runs strict lint stack before allowing commit.
# Installed via scripts/install-hooks.sh

set -e

echo "[pre-commit] cargo fmt --check"
cargo fmt --all -- --check

echo "[pre-commit] cargo clippy"
cargo clippy --all-targets --all-features -- -D warnings

echo "[pre-commit] file length gate"
bash ci/check_file_length.sh

echo "[pre-commit] OK"
