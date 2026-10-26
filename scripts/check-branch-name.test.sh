#!/usr/bin/env bash
# Checks `check-branch-name.sh` on names it must allow and names it must refuse. A new branch
# only: the remote-tip exemption needs a repository and is left to review.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
checker="$here/check-branch-name.sh"
failures=0

expect() {
  local want="$1" branch="$2"
  if bash "$checker" "$branch" 2>/dev/null; then got=allow; else got=refuse; fi
  if [ "$got" = "$want" ]; then
    echo "ok   $want $branch"
  else
    echo "FAIL $branch: expected $want, got $got"
    failures=$((failures + 1))
  fi
}

expect allow  "feat/4-ports-in-the-input"
expect allow  "chore/1-standalone-repo"
expect allow  "fix/12-a"
expect allow  "main"
expect refuse "claude/adoring-faraday-v8shlp"
expect refuse "feat/ports"
expect refuse "feat/4"
expect refuse "feature/4-ports"
expect refuse "feat/4-Ports"
expect refuse ""

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo "all cases passed"
