#!/bin/bash
# Session start: turn on the git hooks, and in a cloud session make the pinned Rust the default.
set -euo pipefail

repo_root="${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"

# commit-msg deletes the session link; pre-push checks the branch name. See CLAUDE.md.
git -C "$repo_root" config core.hooksPath .githooks

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

# rust-toolchain.toml binds only inside the repository. Making its channel the default lets a
# build started from any directory use the same compiler.
channel="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$repo_root/rust-toolchain.toml" | head -n1)"
if [ -z "$channel" ]; then
  echo "elk-layered: no channel found in rust-toolchain.toml" >&2
  exit 1
fi
rustup default "$channel" >/dev/null
echo "elk-layered: Rust $channel, git hooks on"
