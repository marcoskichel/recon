#!/usr/bin/env bash
# Installs git hooks from scripts/ into .git/hooks/.
# Idempotent — safe to re-run.

set -e

REPO_ROOT="$(git rev-parse --show-toplevel)"
GIT_DIR="$(git rev-parse --git-common-dir)"
HOOKS_DIR="${GIT_DIR}/hooks"

mkdir -p "$HOOKS_DIR"

install_hook() {
  local name="$1"
  local src="${REPO_ROOT}/scripts/${name}.sh"
  local dst="${HOOKS_DIR}/${name}"

  if [[ ! -f "$src" ]]; then
    echo "skip: ${src} not found"
    return
  fi

  cp "$src" "$dst"
  chmod +x "$dst"
  echo "installed: ${dst}"
}

install_hook pre-commit

echo "Done."
